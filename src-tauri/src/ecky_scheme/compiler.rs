#![allow(clippy::result_large_err, clippy::too_many_arguments)]

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use steel_core::parser::ast::{Atom, Define, ExprKind, Let};
use steel_core::parser::parser::Parser;
use steel_core::parser::tokens::TokenType;
use steel_core::rvals::SteelVal;

use crate::contracts::{AppError, AppErrorCode, AppResult};
use crate::ecky_core_ir::{
    CompilerError, CompilerErrorKind, CoreArrayOp, CoreBinding, CoreBooleanOp, CoreFeatureDecl,
    CoreFrameOp, CoreKeywordArg, CoreLiteral, CoreMetaOp, CoreNode, CoreNodeKind, CoreOperation,
    CoreParameter, CoreParameterConstraints, CoreParameterKind, CoreParameterValue, CorePart,
    CorePathOp, CorePrimitive, CoreProgram, CoreProgramConstraints, CoreReference,
    CoreRelationConstraint, CoreRelationOperand, CoreRelationOperator, CoreResult,
    CoreSelectorPayload, CoreShapeBinding, CoreSurfaceOp, CoreSymbol, CoreTransformOp,
    CoreValueKind, CoreVerifyClause, CoreVerifySection, CoreVerifyValue, NodeId, ParamId, PartId,
    ProgramId, SourceFileId, SourceSpan,
};
use crate::ecky_deterministic;
use crate::ecky_ir::edge_ops::{
    parse_core_edge_selector_payload, parse_core_face_selector_payload,
};

use super::bootstrap;

const ECKY_COMPILE_STACK_SIZE: usize = 32 * 1024 * 1024;
const ECKY_SOURCE_MAX_BYTES: usize = 512 * 1024;
const ECKY_SOURCE_MAX_LIST_FORMS: usize = 20_000;
const ECKY_SOURCE_MAX_PAREN_DEPTH: usize = 256;

/// Internal representation of a component clause in the compiler's AST.
/// Tracks the role of a component (root for model, output for part/feature, library for define-component)
/// and the original spelling for emit and compatibility.
///
/// Spelling is carried per-clause (see `ExpandedModelClause::component`) and never stored
/// globally: compilations can run concurrently, and emit derives part/feature spelling
/// from `CoreProgram::feature_decls`, which is per-program.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Library role and clause tagging are consumed by define-component (T2).
struct ComponentClause {
    role: ComponentRole,
    spelling: String,
}

/// Role of a component in the AST:
/// - Root: the model itself, from `(model ...)`
/// - Output: a part or feature, from `(part ...)` or `(feature ...)`
/// - Library: a reusable component definition, from `(define-component ...)` (T2)
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum ComponentRole {
    Root,
    Output,
    Library,
}

fn selector_payload_for_keyword(name: &str, value: &CoreNode) -> Option<CoreSelectorPayload> {
    let selector = match &value.kind {
        CoreNodeKind::Literal(CoreLiteral::Text(text)) => text.as_str(),
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => match symbol {
            CoreSymbol::Start => "start",
            CoreSymbol::End => "end",
            CoreSymbol::Xy => "xy",
            CoreSymbol::Yz => "yz",
            CoreSymbol::Xz => "xz",
            CoreSymbol::Min => "min",
            CoreSymbol::Center => "center",
            CoreSymbol::Max => "max",
        },
        _ => return None,
    };
    match name {
        "edges" => parse_core_edge_selector_payload(selector).ok(),
        "faces" => parse_core_face_selector_payload(selector).ok(),
        _ => None,
    }
}

pub fn try_compile_to_legacy_source(source: &str) -> Option<AppResult<String>> {
    match compile_to_legacy_source(source) {
        Ok(compiled) => Some(Ok(compiled)),
        Err(err) if should_fallback_to_legacy(source, &err) => None,
        Err(err) => Some(Err(err)),
    }
}

pub fn try_compile_to_core_program(source: &str) -> Option<AppResult<CoreProgram>> {
    match compile_to_core_program(source) {
        Ok(program) => Some(Ok(program)),
        Err(err) if should_fallback_to_legacy(source, &core_err_to_app(err.clone())) => None,
        Err(err) => Some(Err(core_err_to_app(err))),
    }
}

pub fn compile_to_legacy_source(source: &str) -> AppResult<String> {
    let program = compile_to_core_program(source).map_err(core_err_to_app)?;
    Ok(emit_program(&program))
}

pub fn compile_to_core_program(source: &str) -> CoreResult<CoreProgram> {
    validate_source_budget_before_steel(source)?;
    compile_to_core_program_on_guarded_stack(source)
}

fn compile_to_core_program_on_guarded_stack(source: &str) -> CoreResult<CoreProgram> {
    let source = source.to_owned();
    let handle = std::thread::Builder::new()
        .name("ecky-scheme-compile".into())
        .stack_size(ECKY_COMPILE_STACK_SIZE)
        .spawn(move || compile_to_core_program_inner(&source))
        .map_err(|err| {
            CompilerError::new(
                CompilerErrorKind::Internal,
                format!("Failed to start guarded Ecky compiler thread: {err}"),
            )
        })?;

    handle.join().map_err(|_| {
        CompilerError::new(
            CompilerErrorKind::Internal,
            "Ecky compiler panicked while lowering source.",
        )
    })?
}

fn compile_to_core_program_inner(source: &str) -> CoreResult<CoreProgram> {
    bootstrap::validate_user_source(source)
        .map_err(|err| CompilerError::new(CompilerErrorKind::Parse, err))?;
    let source = rewrite_sequence_destructuring_source(source)?;
    reject_model_level_sequence_forms(&source)?;
    let source = lower_component_definitions_source(&source)?;

    if can_use_expanded_ast(&source) {
        if let Ok(program) = compile_to_core_program_from_expanded_ast(&source) {
            return verify_compiled_core_program(program);
        }
    }

    compile_to_core_program_via_runtime(&source).and_then(verify_compiled_core_program)
}

fn verify_compiled_core_program(program: CoreProgram) -> CoreResult<CoreProgram> {
    crate::ecky_core_ir::verify_core_program(&program)?;
    Ok(program)
}

fn validate_source_budget_before_steel(source: &str) -> CoreResult<()> {
    if source.len() > ECKY_SOURCE_MAX_BYTES {
        return Err(source_budget_error(format!(
            "Ecky source is too large before Steel lowering: {} bytes exceeds limit {} bytes.",
            source.len(),
            ECKY_SOURCE_MAX_BYTES
        )));
    }

    let mut depth = 0usize;
    let mut list_forms = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;

    for (offset, ch) in source.char_indices() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }

        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            ';' => in_comment = true,
            '"' => in_string = true,
            '(' => {
                depth += 1;
                list_forms += 1;
                if depth > ECKY_SOURCE_MAX_PAREN_DEPTH {
                    return Err(source_budget_error(format!(
                        "Ecky source nesting depth is too high before Steel lowering: depth {} at byte {} exceeds limit {}.",
                        depth, offset, ECKY_SOURCE_MAX_PAREN_DEPTH
                    )));
                }
                if list_forms > ECKY_SOURCE_MAX_LIST_FORMS {
                    return Err(source_budget_error(format!(
                        "Ecky source has too many list forms before Steel lowering: {} exceeds limit {}.",
                        list_forms, ECKY_SOURCE_MAX_LIST_FORMS
                    )));
                }
            }
            ')' => {
                if depth == 0 {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        format!("Unexpected `)` at byte {offset}."),
                    ));
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    if in_string {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Unterminated string literal in Ecky source.",
        ));
    }

    if depth != 0 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Unclosed `(` in Ecky source.",
        ));
    }

    Ok(())
}

fn source_budget_error(message: String) -> CompilerError {
    CompilerError::new(CompilerErrorKind::UnsupportedFeature, message).with_help(
        "Reduce generated repetition, use `range`/`map`/`repeat-*` helpers, or split the model into smaller parts before rendering.",
    )
}

fn can_use_expanded_ast(source: &str) -> bool {
    !source.contains("(define-syntax ") && !source.contains("(set! ")
}

fn compile_to_core_program_via_runtime(source: &str) -> CoreResult<CoreProgram> {
    let source = lower_component_definitions_source(source)?;
    let mut engine = bootstrap::new_engine();
    let runtime_source = rewrite_runtime_model_clause_wrappers(&source)?;
    seed_symbol_bindings(&mut engine, &runtime_source);
    let wrapped = bootstrap::wrap_user_source(&runtime_source);
    let values = engine
        .compile_and_run_raw_program(wrapped)
        .map_err(|err| compiler_error(CompilerErrorKind::Parse, err))?;

    let root = if let Some(last) = values.last().cloned() {
        if matches!(last, SteelVal::Void) {
            engine
                .extract_value("model-root")
                .map_err(|err| compiler_error(CompilerErrorKind::Resolve, err))?
        } else {
            last
        }
    } else {
        engine
            .extract_value("model-root")
            .map_err(|err| compiler_error(CompilerErrorKind::Resolve, err))?
    };

    parse_program(&root)
}

fn rewrite_runtime_model_clause_wrappers(source: &str) -> CoreResult<String> {
    let forms = Parser::parse_without_lowering(source)
        .map_err(|err| compiler_error(CompilerErrorKind::Parse, err))?;
    Ok(forms
        .iter()
        .map(rewrite_runtime_expr_source)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn rewrite_runtime_expr_source(expr: &ExprKind) -> String {
    match expr {
        ExprKind::List(list) => {
            if expr_list_head_is(&list.args, "define-syntax") {
                return expr.to_string();
            }
            if expr_list_head_is(&list.args, "model") {
                return rewrite_runtime_model_source(&list.args);
            }
            format!(
                "({})",
                list.args
                    .iter()
                    .map(rewrite_runtime_expr_source)
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        ExprKind::Define(def) => format!(
            "(define {} {})",
            def.name,
            rewrite_runtime_expr_source(&def.body)
        ),
        ExprKind::Begin(begin) => format!(
            "(begin {})",
            begin
                .exprs
                .iter()
                .map(rewrite_runtime_expr_source)
                .collect::<Vec<_>>()
                .join(" ")
        ),
        _ => expr.to_string(),
    }
}

fn rewrite_runtime_model_source(items: &[ExprKind]) -> String {
    let groups = items
        .iter()
        .skip(1)
        .map(rewrite_runtime_model_clause_group_source)
        .collect::<Vec<_>>();
    format!("(cons 'model {})", append_runtime_clause_groups(groups))
}

fn rewrite_runtime_model_clause_group_source(expr: &ExprKind) -> String {
    let Ok(items) = expr_list_items(expr, "model clause") else {
        return format!("(list {})", expr);
    };
    let Some(head) = items.first().and_then(expr_head_name) else {
        return format!("(list {})", expr);
    };

    match head.as_str() {
        "verify" => format!("(list (quote {}))", expr),
        "begin" => append_runtime_clause_groups(
            items
                .iter()
                .skip(1)
                .map(rewrite_runtime_model_clause_group_source)
                .collect(),
        ),
        "let" | "let*" if items.len() >= 3 => {
            let body = append_runtime_clause_groups(
                items
                    .iter()
                    .skip(2)
                    .map(rewrite_runtime_model_clause_group_source)
                    .collect(),
            );
            format!("({} {} {})", head, items[1], body)
        }
        _ => format!("(list {})", expr),
    }
}

fn append_runtime_clause_groups(groups: Vec<String>) -> String {
    match groups.len() {
        0 => "'()".to_string(),
        1 => groups[0].clone(),
        _ => format!("(append {})", groups.join(" ")),
    }
}

fn reject_model_level_sequence_forms(source: &str) -> CoreResult<()> {
    let forms = Parser::parse_without_lowering(source)
        .map_err(|err| compiler_error(CompilerErrorKind::Parse, err))?;
    for form in &forms {
        reject_model_level_sequence_forms_in_top_level(form)?;
    }
    Ok(())
}

fn reject_model_level_sequence_forms_in_top_level(form: &ExprKind) -> CoreResult<()> {
    match form {
        ExprKind::Begin(begin) => {
            for item in &begin.exprs {
                reject_model_level_sequence_forms_in_top_level(item)?;
            }
        }
        ExprKind::List(list) if expr_list_head_is(&list.args, "model") => {
            reject_model_level_sequence_forms_in_model(&list.args[1..])?;
        }
        _ => {}
    }
    Ok(())
}

fn reject_model_level_sequence_forms_in_model(forms: &[ExprKind]) -> CoreResult<()> {
    for form in forms {
        reject_model_level_sequence_form_group(form)?;
    }
    Ok(())
}

fn reject_model_level_sequence_form_group(form: &ExprKind) -> CoreResult<()> {
    match form {
        ExprKind::Begin(begin) => {
            for item in &begin.exprs {
                reject_model_level_sequence_form_group(item)?;
            }
        }
        ExprKind::Let(let_expr) => {
            reject_model_level_sequence_form_group(&let_expr.body_expr)?;
        }
        ExprKind::List(_) | ExprKind::Vector(_) => {
            let items = expr_list_items(form, "model clause")?;
            if let Some(head) = items.first().and_then(expr_head_name) {
                match head.as_str() {
                    "begin" => {
                        for item in items.iter().skip(1) {
                            reject_model_level_sequence_form_group(item)?;
                        }
                    }
                    "let" | "let*" if items.len() >= 3 => {
                        for item in items.iter().skip(2) {
                            reject_model_level_sequence_form_group(item)?;
                        }
                    }
                    "map" | "range" => return Err(model_level_sequence_form_error(head.as_str())),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn model_level_sequence_form_error(name: &str) -> CompilerError {
    CompilerError::new(
        CompilerErrorKind::UnsupportedFeature,
        format!(
            "Model children are clauses, not sequence expressions. Supported direct clauses: `params`, `verify`, `part`, `feature`, `meta`. Supported wrappers: `begin`, `let`, `let*`. `{}` belongs inside `(part ...)` geometry/list expressions.",
            name
        ),
    )
}

fn compile_to_core_program_from_expanded_ast(source: &str) -> CoreResult<CoreProgram> {
    validate_source_budget_before_steel(source)?;
    let source = rewrite_sequence_destructuring_source(source)?;
    let source = lower_component_definitions_source(&source)?;
    let mut engine = bootstrap::new_engine();
    let wrapped = wrap_expanded_ast_source(&source);
    let forms = engine
        .emit_expanded_ast_without_optimizations(&wrapped, None)
        .map_err(|err| compiler_error(CompilerErrorKind::Parse, err))?;
    let decoded = forms.iter().map(decode_expanded_expr).collect::<Vec<_>>();
    parse_expanded_program(&decoded)
}

fn rewrite_sequence_destructuring_source(source: &str) -> CoreResult<String> {
    if !source.contains("(lambda ((") {
        return Ok(source.to_string());
    }
    let forms = Parser::parse_without_lowering(source)
        .map_err(|err| compiler_error(CompilerErrorKind::Parse, err))?;
    forms
        .iter()
        .map(rewrite_sequence_destructuring_expr)
        .collect::<CoreResult<Vec<_>>>()
        .map(|forms| forms.join("\n"))
}

fn rewrite_sequence_destructuring_expr(expr: &ExprKind) -> CoreResult<String> {
    match expr {
        ExprKind::List(list) => {
            if expr_list_head_is(&list.args, "map") && list.args.len() == 3 {
                if let Some((names, body_source)) = sequence_destructuring_lambda(&list.args[1])? {
                    return rewrite_map_destructuring(&names, &body_source, &list.args[2]);
                }
            }
            let rendered = list
                .args
                .iter()
                .map(rewrite_sequence_destructuring_expr)
                .collect::<CoreResult<Vec<_>>>()?;
            Ok(format!("({})", rendered.join(" ")))
        }
        ExprKind::LambdaFunction(_) => {
            if sequence_destructuring_lambda(expr)?.is_some() {
                return Err(sequence_callable_kind_error(
                    "`map` lambda parameter",
                    "symbol outside supported static destructuring",
                    "destructuring",
                    expr_source_span(expr),
                ));
            }
            Ok(expr.to_string())
        }
        _ => Ok(expr.to_string()),
    }
}

fn sequence_destructuring_lambda(expr: &ExprKind) -> CoreResult<Option<(Vec<String>, String)>> {
    let (pattern, body_source) = match expr {
        ExprKind::LambdaFunction(lambda) => {
            if lambda.args.len() != 1 {
                return Ok(None);
            }
            (
                lambda.args[0].clone(),
                rewrite_sequence_destructuring_expr(&lambda.body)?,
            )
        }
        ExprKind::List(list) => {
            if !expr_list_head_is(&list.args, "lambda") || list.args.len() < 3 {
                return Ok(None);
            }
            let params =
                expr_list_items(&list.args[1], "`map` lambda destructuring parameter list")?;
            if params.len() != 1 {
                return Ok(None);
            }
            let body_parts = list.args[2..]
                .iter()
                .map(rewrite_sequence_destructuring_expr)
                .collect::<CoreResult<Vec<_>>>()?;
            let body_source = if body_parts.len() == 1 {
                body_parts[0].clone()
            } else {
                format!("(begin {})", body_parts.join(" "))
            };
            (params[0].clone(), body_source)
        }
        _ => return Ok(None),
    };
    let Ok(pattern_items) = expr_list_items(&pattern, "`map` lambda destructuring parameter")
    else {
        return Ok(None);
    };
    let names = pattern_items
        .iter()
        .map(|item| {
            expr_identifier(item).ok_or_else(|| {
                sequence_callable_kind_error(
                    "`map` lambda parameter",
                    "symbol in destructuring pattern",
                    &expr_actual_kind_label(item),
                    expr_source_span(item),
                )
            })
        })
        .collect::<CoreResult<Vec<_>>>()?;
    if names.is_empty() {
        return Err(sequence_callable_arity_error(
            "`map` lambda parameter",
            1,
            0,
            expr_source_span(&pattern),
        ));
    }
    Ok(Some((names, body_source)))
}

fn rewrite_map_destructuring(
    names: &[String],
    body_source: &str,
    source: &ExprKind,
) -> CoreResult<String> {
    let source_items = expr_list_items(source, "`map` destructuring source")?;
    let source_head = source_items
        .first()
        .and_then(expr_identifier)
        .map(|name| normalize_hygienic_op_name(&name))
        .unwrap_or_default();
    let params = names.join(" ");
    match source_head.as_str() {
        "zip" => {
            if source_items.len() - 1 != names.len() {
                return Err(sequence_callable_arity_error(
                    "`map` lambda parameter",
                    source_items.len() - 1,
                    names.len(),
                    expr_source_span(source),
                ));
            }
            let sources = source_items[1..]
                .iter()
                .map(rewrite_sequence_destructuring_expr)
                .collect::<CoreResult<Vec<_>>>()?;
            Ok(format!(
                "(map (lambda ({params}) {body_source}) {})",
                sources.join(" ")
            ))
        }
        "enumerate" => {
            if names.len() != 2 {
                return Err(sequence_callable_arity_error(
                    "`map` lambda parameter",
                    2,
                    names.len(),
                    expr_source_span(source),
                ));
            }
            if source_items.len() != 2 {
                return Err(sequence_arity_error(
                    "`enumerate`",
                    "one list",
                    source_items.len().saturating_sub(1),
                    expr_source_span(source),
                ));
            }
            let count = static_sequence_length(&source_items[1]).ok_or_else(|| {
                sequence_callable_kind_error(
                    "`map` lambda parameter",
                    "static `enumerate` source for destructuring",
                    "dynamic destructuring",
                    expr_source_span(source),
                )
            })?;
            let source = rewrite_sequence_destructuring_expr(&source_items[1])?;
            Ok(format!(
                "(map (lambda ({params}) {body_source}) (range 0 {count}) {source})"
            ))
        }
        _ => Err(sequence_callable_kind_error(
            "`map` lambda parameter",
            "`zip` or static `enumerate` source for destructuring",
            "destructuring",
            expr_source_span(source),
        )),
    }
}

fn static_sequence_length(expr: &ExprKind) -> Option<i64> {
    let items = expr_list_items(expr, "static sequence").ok()?;
    let head = items
        .first()
        .and_then(expr_identifier)
        .map(|name| normalize_hygienic_op_name(&name))?;
    match head.as_str() {
        "list" => Some(items.len().saturating_sub(1) as i64),
        "range" if items.len() == 3 => {
            let start = parse_integer_literal(&items[1], "range start").ok()?;
            let end = parse_integer_literal(&items[2], "range end").ok()?;
            Some((end - start).max(0))
        }
        _ => None,
    }
}

const COMPONENT_MAX_NESTING_DEPTH: usize = 32;

#[derive(Clone, Debug)]
struct ComponentSignatureEntry {
    key: String,
    default_source: Option<String>,
}

#[derive(Clone, Debug)]
struct ComponentDefinition {
    name: String,
    entries: Vec<ComponentSignatureEntry>,
    body: ExprKind,
    verify_clauses: Vec<ExprKind>,
}

/// Lowers `define-component` definitions and keyword instantiations into plain
/// `define` + positional calls, shared by both compile paths: the expanded-AST
/// path inlines the resulting helper functions (fresh node ids, call-site span
/// on the expansion root) and the Steel runtime path evaluates them as plain
/// lambdas. Sources without `define-component` pass through byte-identical.
fn lower_component_definitions_source(source: &str) -> CoreResult<String> {
    if !source.contains("(define-component") {
        return Ok(source.to_string());
    }
    let forms = Parser::parse_without_lowering(source)
        .map_err(|err| compiler_error(CompilerErrorKind::Parse, err))?;

    let mut definitions: Vec<ComponentDefinition> = Vec::new();
    for form in &forms {
        collect_component_definitions(form, &mut definitions)?;
    }
    let mut registry: BTreeMap<String, ComponentDefinition> = BTreeMap::new();
    for definition in &definitions {
        if registry
            .insert(definition.name.clone(), definition.clone())
            .is_some()
        {
            return Err(CompilerError::new(
                CompilerErrorKind::Parse,
                format!("Component `{}` is defined more than once.", definition.name),
            ));
        }
    }

    for definition in &definitions {
        check_component_closedness(definition, &registry)?;
    }
    check_component_graph(&definitions, &registry)?;

    let mut out = String::new();
    for definition in &definitions {
        let keys = definition
            .entries
            .iter()
            .map(|entry| entry.key.clone())
            .collect::<Vec<_>>()
            .join(" ");
        let body = rewrite_component_calls(&definition.body, &registry)?;
        out.push_str(&format!(
            "(define ({} {}) {})\n",
            definition.name, keys, body
        ));
    }
    for form in &forms {
        if component_definition_items(form).is_some() {
            continue;
        }
        out.push_str(&rewrite_component_calls_without_definitions(
            form, &registry,
        )?);
        out.push('\n');
    }
    Ok(out)
}

fn component_definition_items(form: &ExprKind) -> Option<Vec<ExprKind>> {
    let items = expr_list_items(form, "form").ok()?;
    if items.first().and_then(expr_identifier).as_deref() == Some("define-component") {
        Some(items)
    } else {
        None
    }
}

fn collect_component_definitions(
    form: &ExprKind,
    definitions: &mut Vec<ComponentDefinition>,
) -> CoreResult<()> {
    if let Some(items) = component_definition_items(form) {
        definitions.push(parse_component_definition(&items)?);
        return Ok(());
    }
    // Also lift definitions written as direct model clauses.
    if let Ok(items) = expr_list_items(form, "form") {
        if items.first().and_then(expr_head_name).as_deref() == Some("model") {
            for clause in items.iter().skip(1) {
                if let Some(clause_items) = component_definition_items(clause) {
                    definitions.push(parse_component_definition(&clause_items)?);
                }
            }
        }
    }
    Ok(())
}

fn parse_component_definition(items: &[ExprKind]) -> CoreResult<ComponentDefinition> {
    if items.len() < 4 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "`define-component` needs a name, a signature list, and a geometry body.",
        ));
    }
    let name = expr_identifier(&items[1]).ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "`define-component` name must be a symbol.",
        )
    })?;
    let signature_items = expr_list_items(&items[2], "component signature")?;
    let mut entries = Vec::new();
    for entry in &signature_items {
        entries.push(parse_component_signature_entry(&name, entry)?);
    }
    let mut verify_clauses = Vec::new();
    let mut geometry = Vec::new();
    for form in items.iter().skip(3) {
        let head = expr_list_items(form, "component body form")
            .ok()
            .and_then(|body_items| body_items.first().and_then(expr_identifier));
        if head.as_deref() == Some("verify") {
            verify_clauses.push(form.clone());
        } else {
            geometry.push(form.clone());
        }
    }
    if geometry.len() != 1 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            format!(
                "Component `{}` body must be a single geometry expression (plus optional `(verify ...)` clauses); found {} geometry forms.",
                name,
                geometry.len()
            ),
        ));
    }
    Ok(ComponentDefinition {
        name,
        entries,
        body: geometry.remove(0),
        verify_clauses,
    })
}

fn parse_component_signature_entry(
    component: &str,
    entry: &ExprKind,
) -> CoreResult<ComponentSignatureEntry> {
    let items = expr_list_items(entry, "component signature entry")?;
    if items.len() < 2 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            format!(
                "Component `{}` signature entries are `(kind key [default] [:keyword value ...])`.",
                component
            ),
        ));
    }
    let key = expr_identifier(&items[1]).ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            format!(
                "Component `{}` signature entry key must be a symbol.",
                component
            ),
        )
    })?;
    let mut default_source = None;
    let mut index = 2usize;
    while index < items.len() {
        if let Some(keyword) = instantiation_keyword_name(&items[index]) {
            if index + 1 >= items.len() {
                return Err(CompilerError::new(
                    CompilerErrorKind::Parse,
                    format!(
                        "Component `{}` signature entry `{}` has keyword `:{}` without a value.",
                        component, key, keyword
                    ),
                ));
            }
            index += 2;
            continue;
        }
        if default_source.is_some() {
            return Err(CompilerError::new(
                CompilerErrorKind::Parse,
                format!(
                    "Component `{}` signature entry `{}` has more than one default value.",
                    component, key
                ),
            ));
        }
        default_source = Some(items[index].to_string());
        index += 1;
    }
    Ok(ComponentSignatureEntry {
        key,
        default_source,
    })
}

fn instantiation_keyword_name(expr: &ExprKind) -> Option<String> {
    if let ExprKind::Atom(atom) = expr {
        match &atom.syn.ty {
            TokenType::Keyword(name) => {
                let normalized = normalize_keyword(&name.to_string());
                return normalized.strip_prefix(':').map(str::to_string);
            }
            TokenType::Identifier(name) => {
                let text = name.to_string();
                return text.strip_prefix(':').map(str::to_string);
            }
            _ => {}
        }
    }
    None
}

fn component_builtin_names() -> &'static BTreeSet<String> {
    use std::sync::OnceLock;
    static NAMES: OnceLock<BTreeSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = BTreeSet::new();
        for export in super::cad::MODULE.exports {
            names.insert((*export).to_string());
        }
        for export in super::core::MODULE.exports {
            names.insert((*export).to_string());
        }
        for export in super::params::MODULE.exports {
            names.insert((*export).to_string());
        }
        for builtin in [
            "+",
            "-",
            "*",
            "/",
            "=",
            "<",
            "<=",
            ">",
            ">=",
            "abs",
            "min",
            "max",
            "sqrt",
            "sin",
            "cos",
            "tan",
            "asin",
            "acos",
            "atan",
            "atan2",
            "floor",
            "ceiling",
            "round",
            "expt",
            "exp",
            "log",
            "modulo",
            "remainder",
            "quotient",
            "not",
            "and",
            "or",
            "if",
            "cond",
            "else",
            "when",
            "unless",
            "begin",
            "let",
            "let*",
            "lambda",
            "quote",
            "list",
            "cons",
            "car",
            "cdr",
            "cadr",
            "first",
            "second",
            "third",
            "list-ref",
            "length",
            "append",
            "reverse",
            "map",
            "filter",
            "fold",
            "foldl",
            "foldr",
            "reduce",
            "range",
            "apply",
            "take",
            "drop",
            "null?",
            "empty?",
            "list?",
            "even?",
            "odd?",
            "zero?",
            "signed-pow",
            "point",
            "verify",
            "tag",
            "metric",
            "expect",
        ] {
            names.insert(builtin.to_string());
        }
        names
    })
}

fn check_component_closedness(
    definition: &ComponentDefinition,
    registry: &BTreeMap<String, ComponentDefinition>,
) -> CoreResult<()> {
    let mut bound: BTreeSet<String> = definition
        .entries
        .iter()
        .map(|entry| entry.key.clone())
        .collect();
    let mut free = BTreeSet::new();
    collect_component_free_vars(&definition.body, &mut bound, registry, &mut free);
    if let Some(variable) = free.iter().next() {
        return Err(CompilerError::new(
            CompilerErrorKind::Resolve,
            format!(
                "Component `{}` references free variable `{}`. Components are closed: add `{}` to the signature or bind it inside the body.",
                definition.name, variable, variable
            ),
        ));
    }
    Ok(())
}

/// Free-variable analysis over an authored expression using the compiler's
/// binding resolution (let/let*/lambda/repeat/build scopes, builtin table).
/// Used by component closedness checks and by component extraction.
pub(crate) fn collect_free_variables(
    expr: &ExprKind,
    bound: &BTreeSet<String>,
) -> BTreeSet<String> {
    let registry = BTreeMap::new();
    let mut scope = bound.clone();
    let mut free = BTreeSet::new();
    collect_component_free_vars(expr, &mut scope, &registry, &mut free);
    free
}

fn collect_component_free_vars(
    expr: &ExprKind,
    bound: &mut BTreeSet<String>,
    registry: &BTreeMap<String, ComponentDefinition>,
    free: &mut BTreeSet<String>,
) {
    match expr {
        ExprKind::Atom(_) => {
            if instantiation_keyword_name(expr).is_some() {
                return;
            }
            if let Some(name) = expr_identifier(expr) {
                if !bound.contains(&name)
                    && !component_builtin_names().contains(&name)
                    && !registry.contains_key(&name)
                {
                    free.insert(name);
                }
            }
        }
        ExprKind::Quote(_) => {}
        ExprKind::If(if_expr) => {
            collect_component_free_vars(&if_expr.test_expr, bound, registry, free);
            collect_component_free_vars(&if_expr.then_expr, bound, registry, free);
            collect_component_free_vars(&if_expr.else_expr, bound, registry, free);
        }
        ExprKind::Begin(begin) => {
            for item in &begin.exprs {
                collect_component_free_vars(item, bound, registry, free);
            }
        }
        ExprKind::Let(let_expr) => {
            let mut scope = bound.clone();
            for (name_expr, value_expr) in &let_expr.bindings {
                collect_component_free_vars(value_expr, bound, registry, free);
                if let Some(name) = expr_identifier(name_expr) {
                    scope.insert(name);
                }
            }
            collect_component_free_vars(&let_expr.body_expr, &mut scope, registry, free);
        }
        ExprKind::LambdaFunction(lambda) => {
            let mut scope = bound.clone();
            for arg in &lambda.args {
                bind_pattern_names(arg, &mut scope);
            }
            collect_component_free_vars(&lambda.body, &mut scope, registry, free);
        }
        ExprKind::List(_) | ExprKind::Vector(_) => {
            let Ok(items) = expr_list_items(expr, "component body form") else {
                return;
            };
            let head = items.first().and_then(expr_identifier);
            match head.as_deref() {
                Some("quote") => {}
                Some("let") | Some("let*") if items.len() >= 3 => {
                    let sequential = head.as_deref() == Some("let*");
                    let mut scope = bound.clone();
                    if let Ok(bindings) = expr_list_items(&items[1], "let bindings") {
                        for binding in &bindings {
                            if let Ok(pair) = expr_list_items(binding, "let binding") {
                                if pair.len() == 2 {
                                    if sequential {
                                        collect_component_free_vars(
                                            &pair[1], &mut scope, registry, free,
                                        );
                                    } else {
                                        collect_component_free_vars(
                                            &pair[1], bound, registry, free,
                                        );
                                    }
                                    if let Some(name) = expr_identifier(&pair[0]) {
                                        scope.insert(name);
                                    }
                                }
                            }
                        }
                    }
                    for item in items.iter().skip(2) {
                        collect_component_free_vars(item, &mut scope, registry, free);
                    }
                }
                Some("lambda") if items.len() >= 3 => {
                    let mut scope = bound.clone();
                    if let Ok(params) = expr_list_items(&items[1], "lambda params") {
                        for param in &params {
                            bind_pattern_names(param, &mut scope);
                        }
                    } else {
                        bind_pattern_names(&items[1], &mut scope);
                    }
                    for item in items.iter().skip(2) {
                        collect_component_free_vars(item, &mut scope, registry, free);
                    }
                }
                Some("repeat")
                | Some("repeat-union")
                | Some("repeat-compound")
                | Some("repeat-pick")
                    if items.len() >= 3 =>
                {
                    let mut scope = bound.clone();
                    if let Some(index_name) = items.get(1).and_then(expr_identifier) {
                        scope.insert(index_name);
                    }
                    for item in items.iter().skip(2) {
                        collect_component_free_vars(item, &mut scope, registry, free);
                    }
                }
                Some("build") => {
                    let mut scope = bound.clone();
                    for item in items.iter().skip(1) {
                        if let Ok(clause) = expr_list_items(item, "build clause") {
                            let clause_head = clause.first().and_then(expr_identifier);
                            if clause_head.as_deref() == Some("shape") && clause.len() == 3 {
                                collect_component_free_vars(&clause[2], &mut scope, registry, free);
                                if let Some(name) = expr_identifier(&clause[1]) {
                                    scope.insert(name);
                                }
                                continue;
                            }
                        }
                        collect_component_free_vars(item, &mut scope, registry, free);
                    }
                }
                _ => {
                    let mut iter = items.iter();
                    if head.is_some() {
                        // Skip the operator position; it is either a builtin,
                        // a component (checked via registry), or reported when
                        // unknown below.
                        if let Some(op) = iter.next().and_then(expr_identifier) {
                            if !component_builtin_names().contains(&op)
                                && !registry.contains_key(&op)
                                && !bound.contains(&op)
                            {
                                free.insert(op);
                            }
                        }
                    }
                    for item in iter {
                        collect_component_free_vars(item, bound, registry, free);
                    }
                }
            }
        }
        _ => {}
    }
}

fn bind_pattern_names(pattern: &ExprKind, scope: &mut BTreeSet<String>) {
    if let Some(name) = expr_identifier(pattern) {
        scope.insert(name);
        return;
    }
    if let Ok(items) = expr_list_items(pattern, "binding pattern") {
        for item in &items {
            bind_pattern_names(item, scope);
        }
    }
}

fn check_component_graph(
    definitions: &[ComponentDefinition],
    registry: &BTreeMap<String, ComponentDefinition>,
) -> CoreResult<()> {
    fn depth_of(
        name: &str,
        registry: &BTreeMap<String, ComponentDefinition>,
        stack: &mut Vec<String>,
        memo: &mut BTreeMap<String, usize>,
    ) -> CoreResult<usize> {
        if let Some(depth) = memo.get(name) {
            return Ok(*depth);
        }
        if stack.iter().any(|entry| entry == name) {
            let mut chain = stack.clone();
            chain.push(name.to_string());
            return Err(CompilerError::new(
                CompilerErrorKind::UnsupportedFeature,
                format!(
                    "Component instantiation cycle detected: {}.",
                    chain.join(" -> ")
                ),
            ));
        }
        let Some(definition) = registry.get(name) else {
            return Ok(0);
        };
        stack.push(name.to_string());
        let mut dependencies = BTreeSet::new();
        collect_component_dependencies(&definition.body, registry, &mut dependencies);
        let mut depth = 1usize;
        for dependency in &dependencies {
            depth = depth.max(1 + depth_of(dependency, registry, stack, memo)?);
        }
        stack.pop();
        if depth > COMPONENT_MAX_NESTING_DEPTH {
            return Err(CompilerError::new(
                CompilerErrorKind::UnsupportedFeature,
                format!(
                    "Component `{}` exceeds the maximum component nesting depth of {}.",
                    name, COMPONENT_MAX_NESTING_DEPTH
                ),
            ));
        }
        memo.insert(name.to_string(), depth);
        Ok(depth)
    }

    let mut memo = BTreeMap::new();
    for definition in definitions {
        let mut stack = Vec::new();
        depth_of(&definition.name, registry, &mut stack, &mut memo)?;
    }
    Ok(())
}

fn collect_component_dependencies(
    expr: &ExprKind,
    registry: &BTreeMap<String, ComponentDefinition>,
    dependencies: &mut BTreeSet<String>,
) {
    match expr {
        ExprKind::Quote(_) => {}
        ExprKind::List(_) | ExprKind::Vector(_) => {
            let Ok(items) = expr_list_items(expr, "component body form") else {
                return;
            };
            if let Some(head) = items.first().and_then(expr_identifier) {
                if head == "quote" {
                    return;
                }
                if registry.contains_key(&head) {
                    dependencies.insert(head);
                }
            }
            for item in &items {
                collect_component_dependencies(item, registry, dependencies);
            }
        }
        ExprKind::If(if_expr) => {
            collect_component_dependencies(&if_expr.test_expr, registry, dependencies);
            collect_component_dependencies(&if_expr.then_expr, registry, dependencies);
            collect_component_dependencies(&if_expr.else_expr, registry, dependencies);
        }
        ExprKind::Let(let_expr) => {
            for (_, value_expr) in &let_expr.bindings {
                collect_component_dependencies(value_expr, registry, dependencies);
            }
            collect_component_dependencies(&let_expr.body_expr, registry, dependencies);
        }
        ExprKind::LambdaFunction(lambda) => {
            collect_component_dependencies(&lambda.body, registry, dependencies);
        }
        ExprKind::Begin(begin) => {
            for item in &begin.exprs {
                collect_component_dependencies(item, registry, dependencies);
            }
        }
        _ => {}
    }
}

fn rewrite_component_calls_without_definitions(
    expr: &ExprKind,
    registry: &BTreeMap<String, ComponentDefinition>,
) -> CoreResult<String> {
    if let Ok(items) = expr_list_items(expr, "form") {
        if items.first().and_then(expr_head_name).as_deref() == Some("model") {
            let mut rendered = vec!["model".to_string()];
            for clause in items.iter().skip(1) {
                if component_definition_items(clause).is_some() {
                    continue;
                }
                rendered.push(rewrite_component_calls(clause, registry)?);
                rendered.extend(component_verify_clauses_for_model_clause(clause, registry)?);
            }
            return Ok(format!("({})", rendered.join(" ")));
        }
    }
    rewrite_component_calls(expr, registry)
}

/// Verify clauses authored inside `define-component` travel with each
/// instantiation: every `(part ...)`/`(feature ...)` clause that instantiates
/// components gains those components' verify clauses (transitively), with the
/// tag namespaced as `partkey/tag`. Identical clauses are emitted once.
fn component_verify_clauses_for_model_clause(
    clause: &ExprKind,
    registry: &BTreeMap<String, ComponentDefinition>,
) -> CoreResult<Vec<String>> {
    let Ok(items) = expr_list_items(clause, "model clause") else {
        return Ok(Vec::new());
    };
    let head = items.first().and_then(expr_head_name);
    if !matches!(head.as_deref(), Some("part") | Some("feature")) {
        return Ok(Vec::new());
    }
    let Some(part_key) = items.get(1).and_then(expr_identifier) else {
        return Ok(Vec::new());
    };

    let mut dependencies = BTreeSet::new();
    collect_component_dependencies(clause, registry, &mut dependencies);
    let mut rendered = Vec::new();
    let mut seen = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for dependency in &dependencies {
        append_transitive_component_verifies(
            dependency,
            &part_key,
            registry,
            &mut visited,
            &mut seen,
            &mut rendered,
        )?;
    }
    Ok(rendered)
}

fn append_transitive_component_verifies(
    component: &str,
    part_key: &str,
    registry: &BTreeMap<String, ComponentDefinition>,
    visited: &mut BTreeSet<String>,
    seen: &mut BTreeSet<String>,
    rendered: &mut Vec<String>,
) -> CoreResult<()> {
    if !visited.insert(component.to_string()) {
        return Ok(());
    }
    let Some(definition) = registry.get(component) else {
        return Ok(());
    };
    for clause in &definition.verify_clauses {
        let namespaced = render_namespaced_verify_clause(clause, part_key, &definition.name)?;
        if seen.insert(namespaced.clone()) {
            rendered.push(namespaced);
        }
    }
    let mut nested = BTreeSet::new();
    collect_component_dependencies(&definition.body, registry, &mut nested);
    for dependency in &nested {
        append_transitive_component_verifies(
            dependency, part_key, registry, visited, seen, rendered,
        )?;
    }
    Ok(())
}

fn render_namespaced_verify_clause(
    clause: &ExprKind,
    part_key: &str,
    component: &str,
) -> CoreResult<String> {
    let items = expr_list_items(clause, "verify clause")?;
    let mut rendered = vec!["verify".to_string()];
    let mut saw_tag = false;
    for section in items.iter().skip(1) {
        let section_items = expr_list_items(section, "verify section")?;
        if section_items.first().and_then(expr_identifier).as_deref() == Some("tag") {
            let Some(tag) = section_items.get(1).and_then(expr_identifier) else {
                return Err(CompilerError::new(
                    CompilerErrorKind::Parse,
                    format!(
                        "Component `{}` verify clause needs `(tag symbol)` to namespace per instance.",
                        component
                    ),
                ));
            };
            rendered.push(format!("(tag {}/{})", part_key, tag));
            saw_tag = true;
        } else {
            rendered.push(section.to_string());
        }
    }
    if !saw_tag {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            format!(
                "Component `{}` verify clause needs a `(tag ...)` section to namespace per instance.",
                component
            ),
        ));
    }
    Ok(format!("({})", rendered.join(" ")))
}

fn rewrite_component_calls(
    expr: &ExprKind,
    registry: &BTreeMap<String, ComponentDefinition>,
) -> CoreResult<String> {
    match expr {
        ExprKind::Atom(_) => Ok(expr.to_string()),
        ExprKind::Quote(_) => Ok(expr.to_string()),
        ExprKind::Define(def) => Ok(format!(
            "(define {} {})",
            def.name,
            rewrite_component_calls(&def.body, registry)?
        )),
        ExprKind::Begin(begin) => {
            let rendered = begin
                .exprs
                .iter()
                .map(|item| rewrite_component_calls(item, registry))
                .collect::<CoreResult<Vec<_>>>()?;
            Ok(format!("(begin {})", rendered.join(" ")))
        }
        ExprKind::If(if_expr) => Ok(format!(
            "(if {} {} {})",
            rewrite_component_calls(&if_expr.test_expr, registry)?,
            rewrite_component_calls(&if_expr.then_expr, registry)?,
            rewrite_component_calls(&if_expr.else_expr, registry)?
        )),
        ExprKind::Let(let_expr) => {
            let bindings = let_expr
                .bindings
                .iter()
                .map(|(name, value)| {
                    Ok(format!(
                        "({} {})",
                        name,
                        rewrite_component_calls(value, registry)?
                    ))
                })
                .collect::<CoreResult<Vec<_>>>()?;
            Ok(format!(
                "(let ({}) {})",
                bindings.join(" "),
                rewrite_component_calls(&let_expr.body_expr, registry)?
            ))
        }
        ExprKind::LambdaFunction(lambda) => {
            let args = lambda
                .args
                .iter()
                .map(|arg| arg.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            Ok(format!(
                "(lambda ({}) {})",
                args,
                rewrite_component_calls(&lambda.body, registry)?
            ))
        }
        ExprKind::List(_) | ExprKind::Vector(_) => {
            let items = expr_list_items(expr, "form")?;
            if let Some(head) = items.first().and_then(expr_identifier) {
                if head == "quote" {
                    return Ok(expr.to_string());
                }
                if let Some(definition) = registry.get(&head) {
                    return rewrite_component_instantiation(definition, &items[1..], registry);
                }
            }
            let rendered = items
                .iter()
                .map(|item| rewrite_component_calls(item, registry))
                .collect::<CoreResult<Vec<_>>>()?;
            Ok(format!("({})", rendered.join(" ")))
        }
        other => Ok(other.to_string()),
    }
}

fn rewrite_component_instantiation(
    definition: &ComponentDefinition,
    args: &[ExprKind],
    registry: &BTreeMap<String, ComponentDefinition>,
) -> CoreResult<String> {
    let signature_keys = definition
        .entries
        .iter()
        .map(|entry| entry.key.clone())
        .collect::<Vec<_>>();
    let mut overrides: BTreeMap<String, String> = BTreeMap::new();
    let mut index = 0usize;
    while index < args.len() {
        let Some(keyword) = instantiation_keyword_name(&args[index]) else {
            return Err(CompilerError::new(
                CompilerErrorKind::Resolve,
                format!(
                    "Component `{}` must be instantiated with keyword arguments, e.g. `({} :{} ...)`. Signature: ({}).",
                    definition.name,
                    definition.name,
                    signature_keys.first().map(String::as_str).unwrap_or("key"),
                    signature_keys.join(" ")
                ),
            ));
        };
        if !signature_keys.iter().any(|key| key == &keyword) {
            return Err(CompilerError::new(
                CompilerErrorKind::Resolve,
                format!(
                    "Component `{}` has no parameter `:{}`. Signature: ({}).",
                    definition.name,
                    keyword,
                    signature_keys.join(" ")
                ),
            ));
        }
        let Some(value_expr) = args.get(index + 1) else {
            return Err(CompilerError::new(
                CompilerErrorKind::Resolve,
                format!(
                    "Component `{}` keyword `:{}` is missing a value.",
                    definition.name, keyword
                ),
            ));
        };
        if overrides
            .insert(
                keyword.clone(),
                rewrite_component_calls(value_expr, registry)?,
            )
            .is_some()
        {
            return Err(CompilerError::new(
                CompilerErrorKind::Resolve,
                format!(
                    "Component `{}` keyword `:{}` is given more than once.",
                    definition.name, keyword
                ),
            ));
        }
        index += 2;
    }

    let mut positional = Vec::new();
    for entry in &definition.entries {
        if let Some(value) = overrides.remove(&entry.key) {
            positional.push(value);
        } else if let Some(default_source) = &entry.default_source {
            positional.push(default_source.clone());
        } else {
            return Err(CompilerError::new(
                CompilerErrorKind::Resolve,
                format!(
                    "Component `{}` requires `:{}` (no default). Signature: ({}).",
                    definition.name,
                    entry.key,
                    signature_keys.join(" ")
                ),
            ));
        }
    }
    if positional.is_empty() {
        Ok(format!("({})", definition.name))
    } else {
        Ok(format!("({} {})", definition.name, positional.join(" ")))
    }
}

fn wrap_expanded_ast_source(source: &str) -> String {
    let keyword_re = Regex::new(r#"(^|[\s(])\:([A-Za-z][A-Za-z0-9_-]*)"#).expect("keyword regex");
    let normalized = keyword_re.replace_all(source, "$1#:$2");
    format!(
        "(require \"ecky/params\")\n(require \"ecky/cad\")\n{}",
        normalized
    )
}

fn decode_expanded_expr(value: &ExprKind) -> ExprKind {
    match value {
        ExprKind::Atom(_) => value.clone(),
        ExprKind::If(if_expr) => {
            let mut decoded = (**if_expr).clone();
            decoded.test_expr = decode_expanded_expr(&decoded.test_expr);
            decoded.then_expr = decode_expanded_expr(&decoded.then_expr);
            decoded.else_expr = decode_expanded_expr(&decoded.else_expr);
            ExprKind::If(Box::new(decoded))
        }
        ExprKind::Let(let_expr) => {
            let mut decoded = (**let_expr).clone();
            decoded.bindings = decoded
                .bindings
                .iter()
                .map(|(name, body)| (decode_expanded_expr(name), decode_expanded_expr(body)))
                .collect();
            decoded.body_expr = decode_expanded_expr(&decoded.body_expr);
            ExprKind::Let(Box::new(decoded))
        }
        ExprKind::Define(def) => {
            let mut decoded = (**def).clone();
            decoded.name = decode_expanded_expr(&decoded.name);
            decoded.body = decode_expanded_expr(&decoded.body);
            ExprKind::Define(Box::new(decoded))
        }
        ExprKind::LambdaFunction(lambda) => {
            let mut decoded = (**lambda).clone();
            decoded.args = decoded.args.iter().map(decode_expanded_expr).collect();
            decoded.body = decode_expanded_expr(&decoded.body);
            ExprKind::LambdaFunction(Box::new(decoded))
        }
        ExprKind::Begin(begin) => {
            let mut decoded = (**begin).clone();
            decoded.exprs = decoded.exprs.iter().map(decode_expanded_expr).collect();
            ExprKind::Begin(Box::new(decoded))
        }
        ExprKind::Return(ret) => {
            let mut decoded = (**ret).clone();
            decoded.expr = decode_expanded_expr(&decoded.expr);
            ExprKind::Return(Box::new(decoded))
        }
        ExprKind::Quote(quote) => decode_quoted_expr(&quote.expr),
        ExprKind::Macro(_) | ExprKind::SyntaxRules(_) | ExprKind::Require(_) => value.clone(),
        ExprKind::Set(set) => {
            let mut decoded = (**set).clone();
            decoded.variable = decode_expanded_expr(&decoded.variable);
            decoded.expr = decode_expanded_expr(&decoded.expr);
            ExprKind::Set(Box::new(decoded))
        }
        ExprKind::List(list) => {
            let mut decoded = list.clone();
            decoded.args = decoded.args.iter().map(decode_expanded_expr).collect();
            if decoded
                .args
                .first()
                .and_then(|head| expr_name(head).ok())
                .as_deref()
                .is_some_and(|name| matches!(name, "#%prim.list" | "list"))
            {
                decoded.args = decoded.args.iter().skip(1).cloned().collect();
            }
            ExprKind::List(decoded)
        }
        ExprKind::Vector(vector) => {
            let mut decoded = vector.clone();
            decoded.args = decoded.args.iter().map(decode_expanded_expr).collect();
            ExprKind::Vector(decoded)
        }
    }
}

fn decode_quoted_expr(value: &ExprKind) -> ExprKind {
    match value {
        ExprKind::Atom(_) => value.clone(),
        ExprKind::List(list) => {
            let mut decoded = list.clone();
            decoded.args = decoded.args.iter().map(decode_quoted_expr).collect();
            ExprKind::List(decoded)
        }
        ExprKind::Vector(vector) => {
            let mut decoded = vector.clone();
            decoded.args = decoded.args.iter().map(decode_quoted_expr).collect();
            ExprKind::Vector(decoded)
        }
        other => decode_expanded_expr(other),
    }
}

#[derive(Clone, Debug)]
enum ExpandedHelper {
    Value(ExprKind),
    Function { params: Vec<String>, body: ExprKind },
}

type ExpandedHelperMap = BTreeMap<String, ExpandedHelper>;

#[derive(Clone, Debug)]
struct ExpandedModelClause {
    items: Vec<ExprKind>,
    helpers: ExpandedHelperMap,
    component: Option<ComponentClause>,
}

impl ExpandedModelClause {
    #[allow(dead_code)] // consumed by define-component parsing in T2
    fn with_component(mut self, component: ComponentClause) -> Self {
        self.component = Some(component);
        self
    }
}

#[derive(Clone, Debug)]
enum PendingRelationOperand {
    ParameterKey(String),
    Number(f64),
}

#[derive(Clone, Debug)]
struct PendingRelationConstraint {
    operator: CoreRelationOperator,
    left: PendingRelationOperand,
    right: PendingRelationOperand,
}

fn parse_expanded_program(forms: &[ExprKind]) -> CoreResult<CoreProgram> {
    let (root, helpers) = collect_expanded_model_context(forms).ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel source must evaluate to `(model ...)` or bind `model-root` to one.",
        )
    })?;
    parse_expanded_model(&root, &helpers)
}

fn collect_expanded_model_context(forms: &[ExprKind]) -> Option<(ExprKind, ExpandedHelperMap)> {
    let mut helpers = ExpandedHelperMap::new();
    let mut candidate = None;
    for form in forms {
        collect_expanded_context_in_expr(form, &mut helpers, &mut candidate);
    }
    candidate
}

fn collect_expanded_context_in_expr(
    expr: &ExprKind,
    helpers: &mut ExpandedHelperMap,
    candidate: &mut Option<(ExprKind, ExpandedHelperMap)>,
) {
    if is_model_expr(expr) {
        *candidate = Some((expr.clone(), helpers.clone()));
        return;
    }

    match expr {
        ExprKind::Define(def) => {
            if expr_name(&def.name)
                .map(|name| name == "model-root")
                .unwrap_or(false)
            {
                *candidate = Some((def.body.clone(), helpers.clone()));
            } else if let Some((name, helper)) = parse_expanded_helper(def) {
                helpers.insert(name, helper);
            }
            collect_expanded_context_in_expr(&def.body, helpers, candidate);
        }
        ExprKind::Begin(begin) => {
            for item in &begin.exprs {
                collect_expanded_context_in_expr(item, helpers, candidate);
            }
        }
        ExprKind::Let(let_expr) => {
            collect_expanded_context_in_expr(&let_expr.body_expr, helpers, candidate)
        }
        _ => {}
    }
}

fn parse_expanded_helper(def: &Define) -> Option<(String, ExpandedHelper)> {
    let name = expr_identifier(&def.name)?;
    match &def.body {
        ExprKind::LambdaFunction(lambda) if !lambda.rest && !lambda.kwargs => Some((
            name,
            ExpandedHelper::Function {
                params: lambda
                    .args
                    .iter()
                    .map(expr_identifier)
                    .collect::<Option<Vec<_>>>()?,
                body: lambda.body.clone(),
            },
        )),
        body if is_supported_helper_value(body) => {
            Some((name, ExpandedHelper::Value(body.clone())))
        }
        _ => None,
    }
}

fn is_supported_helper_value(value: &ExprKind) -> bool {
    match value {
        ExprKind::Atom(_)
        | ExprKind::Quote(_)
        | ExprKind::If(_)
        | ExprKind::Let(_)
        | ExprKind::Begin(_) => true,
        ExprKind::List(list) => list
            .args
            .first()
            .and_then(|head| expr_name(head).ok())
            .map(|name| name != "%proto-hash-get%")
            .unwrap_or(true),
        ExprKind::Vector(_) => true,
        _ => false,
    }
}

fn expand_helper_value_expr(
    value: &ExprKind,
    helpers: &ExpandedHelperMap,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<ExprKind> {
    let Some(name) = expr_identifier(value) else {
        return Ok(value.clone());
    };
    let Some(ExpandedHelper::Value(helper_expr)) = helpers.get(&name) else {
        return Ok(value.clone());
    };
    if helper_stack.contains(&name) {
        return Err(CompilerError::new(
            CompilerErrorKind::UnsupportedFeature,
            format!(
                "Recursive helper value `{}` is not supported by expanded AST compile.",
                name
            ),
        ));
    }
    let mut next_stack = helper_stack.clone();
    next_stack.insert(name);
    expand_helper_value_expr(helper_expr, helpers, &next_stack)
}

fn is_model_expr(expr: &ExprKind) -> bool {
    matches!(
        expr,
        ExprKind::List(list)
            if list
                .args
                .first()
                .and_then(|item| expr_name(item).ok())
                .as_deref()
                == Some("model")
    )
}

fn expr_list_head_is(items: &[ExprKind], expected: &str) -> bool {
    items.first().and_then(expr_head_name).as_deref() == Some(expected)
}

pub(crate) fn expr_head_name(value: &ExprKind) -> Option<String> {
    match value {
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::Begin => Some("begin".to_string()),
            TokenType::Define => Some("define".to_string()),
            TokenType::DefineSyntax => Some("define-syntax".to_string()),
            TokenType::If => Some("if".to_string()),
            TokenType::Lambda => Some("lambda".to_string()),
            TokenType::Let => Some("let".to_string()),
            TokenType::Quote => Some("quote".to_string()),
            TokenType::Require => Some("require".to_string()),
            TokenType::Set => Some("set!".to_string()),
            TokenType::SyntaxRules => Some("syntax-rules".to_string()),
            _ => expr_name(value).ok(),
        },
        _ => expr_name(value).ok(),
    }
}

fn parse_expanded_model(value: &ExprKind, helpers: &ExpandedHelperMap) -> CoreResult<CoreProgram> {
    let forms = expr_list_items(value, "model root")?;
    let head = expr_name(forms.first().ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel source returned an empty list.",
        )
    })?)?;
    if head != "model" {
        return Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            "Steel source must evaluate to `(model ...)` or bind `model-root` to one.",
        ));
    }

    let mut params = Vec::new();
    let mut pending_relations = Vec::new();
    let mut next_param = 1u64;
    let mut next_part = 1u64;
    let mut next_node = 1u64;
    let mut feature_decls = BTreeMap::new();
    let mut verify_clauses = Vec::new();

    let clauses = collect_expanded_model_clauses(&forms[1..], helpers)?;
    let mut raw_parts = Vec::new();

    for clause_form in clauses {
        let clause =
            expr_name(clause_form.items.first().ok_or_else(|| {
                CompilerError::new(CompilerErrorKind::Parse, "Empty model clause.")
            })?)?;
        match clause.as_str() {
            "params" => {
                let (mut parsed_params, mut parsed_relations) = parse_expanded_params_clause(
                    &clause_form.items,
                    &mut next_param,
                    &clause_form.helpers,
                )?;
                params.append(&mut parsed_params);
                pending_relations.append(&mut parsed_relations);
            }
            "verify" => verify_clauses.push(parse_expanded_verify_clause(&clause_form.items)?),
            "part" | "feature" => raw_parts.push(clause_form),
            "meta" => {}
            "map" | "range" => return Err(model_level_sequence_form_error(&clause)),
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported top-level model clause `{}`.", other),
                ))
            }
        }
    }

    if raw_parts.is_empty() {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel model needs at least one `(part ...)` clause.",
        ));
    }

    let param_ids = params
        .iter()
        .map(|param| (param.key.clone(), param.id))
        .collect::<BTreeMap<_, _>>();
    let mut constraints = resolve_relation_constraints(&mut params, pending_relations)?;
    constraints.verify_clauses = verify_clauses;
    let mut parts = Vec::new();
    let mut part_components: BTreeMap<String, ComponentClause> = BTreeMap::new();
    for part_clause in &raw_parts {
        let clause_name =
            expr_name(part_clause.items.first().ok_or_else(|| {
                CompilerError::new(CompilerErrorKind::Parse, "Empty model clause.")
            })?)?;
        match clause_name.as_str() {
            "part" => {
                let part = parse_expanded_part_decl(
                    &part_clause.items,
                    &mut next_part,
                    &mut next_node,
                    &param_ids,
                    &part_clause.helpers,
                )?;
                if let Some(component) = &part_clause.component {
                    part_components.insert(part.key.clone(), component.clone());
                }
                parts.push(part);
            }
            "feature" => {
                let (part, decl) = parse_expanded_feature_decl(
                    &part_clause.items,
                    &mut next_part,
                    &mut next_node,
                    &param_ids,
                    &part_clause.helpers,
                )?;
                if let Some(component) = &part_clause.component {
                    part_components.insert(part.key.clone(), component.clone());
                }
                feature_decls.insert(part.key.clone(), decl);
                parts.push(part);
            }
            _ => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported top-level model clause `{}`.", clause_name),
                ))
            }
        }
    }

    if parts.is_empty() {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel model needs at least one `(part ...)` clause.",
        ));
    }

    // Component clause tagging (role + spelling) stays on the parse-layer clauses;
    // emit derives part/feature spelling from per-program feature_decls. No global state.
    let _ = part_components;

    Ok(CoreProgram::new(ProgramId::new(1), params, parts)
        .with_feature_decls(feature_decls)
        .with_constraints(constraints))
}

fn parse_expanded_verify_clause(items: &[ExprKind]) -> CoreResult<CoreVerifyClause> {
    if items.len() != 4 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Verify clause expects `(tag ...)`, `(metric ...)`, and `(expect ...)`.",
        ));
    }

    Ok(CoreVerifyClause {
        tag: parse_expanded_verify_section(&items[1], "tag")?,
        metric: parse_expanded_verify_section(&items[2], "metric")?,
        expect: parse_expanded_verify_section(&items[3], "expect")?,
    })
}

fn parse_expanded_verify_section(
    value: &ExprKind,
    expected: &str,
) -> CoreResult<CoreVerifySection> {
    let items = expr_list_items(value, "verify section")?;
    let name = expr_name(items.first().ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            format!("Verify `{expected}` section is empty."),
        )
    })?)?;
    if name != expected {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            format!("Verify clause expected `({expected} ...)`, found `({name} ...)`."),
        ));
    }

    Ok(CoreVerifySection {
        items: items[1..]
            .iter()
            .map(parse_expanded_verify_value)
            .collect::<CoreResult<Vec<_>>>()?,
    })
}

fn parse_expanded_verify_value(value: &ExprKind) -> CoreResult<CoreVerifyValue> {
    match value {
        ExprKind::List(_) | ExprKind::Vector(_) => Ok(CoreVerifyValue::List(
            expr_list_items(value, "verify list")?
                .iter()
                .map(parse_expanded_verify_value)
                .collect::<CoreResult<Vec<_>>>()?,
        )),
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::Identifier(name) | TokenType::Keyword(name) => {
                Ok(CoreVerifyValue::Symbol(name.to_string()))
            }
            TokenType::StringLiteral(text) => Ok(CoreVerifyValue::Text(text.to_string())),
            TokenType::Number(number) => number.resolve().to_string().parse::<f64>().map_or_else(
                |_| {
                    Err(CompilerError::new(
                        CompilerErrorKind::TypeMismatch,
                        format!("verify value expected a number, received {:?}", atom.syn.ty),
                    ))
                },
                |parsed| Ok(CoreVerifyValue::Number(parsed)),
            ),
            TokenType::BooleanLiteral(flag) => Ok(CoreVerifyValue::Boolean(*flag)),
            other => Err(CompilerError::new(
                CompilerErrorKind::TypeMismatch,
                format!(
                    "verify value expected literal or list, received {:?}",
                    other
                ),
            )),
        },
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!(
                "verify value expected literal or list, received {:?}",
                other
            ),
        )),
    }
}

fn parse_expanded_params_clause(
    items: &[ExprKind],
    next_param: &mut u64,
    helpers: &ExpandedHelperMap,
) -> CoreResult<(Vec<CoreParameter>, Vec<PendingRelationConstraint>)> {
    let mut params = Vec::new();
    let mut relations = Vec::new();
    let mut index = 1usize;
    while index < items.len() {
        let clause_name = expr_name(&items[index])
            .ok()
            .map(|name| normalize_keyword(&name));
        if clause_name.as_deref() == Some(":relations") {
            let relation_values = expr_list_items(
                items.get(index + 1).ok_or_else(|| {
                    CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`:relations` missing value in params clause.",
                    )
                })?,
                "param relations",
            )?;
            for relation in relation_values {
                relations.push(parse_expanded_relation_constraint(&relation)?);
            }
            index += 2;
            continue;
        }

        let decl = items.get(index).ok_or_else(|| {
            CompilerError::new(
                CompilerErrorKind::Parse,
                "Param declaration missing in params clause.",
            )
        })?;
        params.push(parse_expanded_param_decl(decl, next_param, helpers)?);
        index += 1;
    }
    Ok((params, relations))
}

fn parse_expanded_relation_constraint(value: &ExprKind) -> CoreResult<PendingRelationConstraint> {
    let items = expr_list_items(value, "relation constraint")?;
    if items.len() != 3 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Relation constraint must be `(< a b)`, `(<= a b)`, `(> a b)`, or `(>= a b)`.",
        ));
    }
    let operator_name = expr_name(&items[0]).unwrap_or_else(|_| items[0].to_string());
    let operator = parse_relation_operator_from_symbol(operator_name.trim())?;
    Ok(PendingRelationConstraint {
        operator,
        left: parse_expanded_relation_operand(&items[1])?,
        right: parse_expanded_relation_operand(&items[2])?,
    })
}

fn parse_expanded_relation_operand(value: &ExprKind) -> CoreResult<PendingRelationOperand> {
    if matches!(value, ExprKind::Atom(atom) if matches!(atom.syn.ty, TokenType::Number(_))) {
        return Ok(PendingRelationOperand::Number(expr_number_value(
            value,
            "relation operand",
        )?));
    }
    Ok(PendingRelationOperand::ParameterKey(
        expr_value_symbol_or_text(value, "relation operand")?,
    ))
}

fn parse_relation_operator_from_symbol(symbol: &str) -> CoreResult<CoreRelationOperator> {
    match symbol {
        "<" => Ok(CoreRelationOperator::LessThan),
        "<=" => Ok(CoreRelationOperator::LessThanOrEqual),
        ">" => Ok(CoreRelationOperator::GreaterThan),
        ">=" => Ok(CoreRelationOperator::GreaterThanOrEqual),
        other => Err(CompilerError::new(
            CompilerErrorKind::UnsupportedFeature,
            format!(
                "Unsupported relation operator `{}`. Supported: <, <=, >, >=.",
                other
            ),
        )),
    }
}

fn resolve_relation_constraints(
    params: &mut [CoreParameter],
    pending_relations: Vec<PendingRelationConstraint>,
) -> CoreResult<CoreProgramConstraints> {
    let param_ids = params
        .iter()
        .map(|param| (param.key.clone(), param.id))
        .collect::<BTreeMap<_, _>>();
    let mut relations = Vec::new();

    for pending in pending_relations {
        let relation = CoreRelationConstraint {
            operator: pending.operator,
            left: resolve_relation_operand(&pending.left, &param_ids)?,
            right: resolve_relation_operand(&pending.right, &param_ids)?,
        };
        for param in params.iter_mut() {
            if relation_uses_param(&relation, param.id) {
                param.constraints.relations.push(relation.clone());
            }
        }
        relations.push(relation);
    }

    Ok(CoreProgramConstraints {
        relations,
        verify_clauses: Vec::new(),
    })
}

fn resolve_relation_operand(
    operand: &PendingRelationOperand,
    param_ids: &BTreeMap<String, ParamId>,
) -> CoreResult<CoreRelationOperand> {
    match operand {
        PendingRelationOperand::Number(value) => Ok(CoreRelationOperand::Number(*value)),
        PendingRelationOperand::ParameterKey(key) => {
            let param_id = param_ids.get(key).copied().ok_or_else(|| {
                CompilerError::new(
                    CompilerErrorKind::Resolve,
                    format!(
                        "Relation operand `{}` does not match any declared parameter key.",
                        key
                    ),
                )
            })?;
            Ok(CoreRelationOperand::Parameter(param_id))
        }
    }
}

fn relation_uses_param(relation: &CoreRelationConstraint, param_id: ParamId) -> bool {
    matches!(relation.left, CoreRelationOperand::Parameter(id) if id == param_id)
        || matches!(relation.right, CoreRelationOperand::Parameter(id) if id == param_id)
}

fn collect_expanded_model_clauses(
    forms: &[ExprKind],
    helpers: &ExpandedHelperMap,
) -> CoreResult<Vec<ExpandedModelClause>> {
    let mut clauses = Vec::new();
    for form in forms {
        push_expanded_model_clauses(form, helpers, &mut clauses)?;
    }
    Ok(clauses)
}

fn push_expanded_model_clauses(
    form: &ExprKind,
    helpers: &ExpandedHelperMap,
    clauses: &mut Vec<ExpandedModelClause>,
) -> CoreResult<()> {
    match form {
        ExprKind::Begin(begin) => {
            for item in &begin.exprs {
                push_expanded_model_clauses(item, helpers, clauses)?;
            }
        }
        ExprKind::Let(let_expr) => {
            let scoped_helpers = model_let_helpers(let_expr, helpers)?;
            push_expanded_model_clauses(&let_expr.body_expr, &scoped_helpers, clauses)?;
        }
        ExprKind::List(_) | ExprKind::Vector(_) => {
            let items = expr_list_items(form, "model clause")?;
            if let Some(head) = items.first().and_then(expr_head_name) {
                match head.as_str() {
                    "begin" => {
                        for item in items.iter().skip(1) {
                            push_expanded_model_clauses(item, helpers, clauses)?;
                        }
                        return Ok(());
                    }
                    "let" | "let*" if items.len() >= 3 => {
                        let scoped_helpers = model_list_let_helpers(&items[1], helpers)?;
                        for item in items.iter().skip(2) {
                            push_expanded_model_clauses(item, &scoped_helpers, clauses)?;
                        }
                        return Ok(());
                    }
                    "part" => {
                        clauses.push(ExpandedModelClause {
                            items,
                            helpers: helpers.clone(),
                            component: Some(ComponentClause {
                                role: ComponentRole::Output,
                                spelling: "part".to_string(),
                            }),
                        });
                        return Ok(());
                    }
                    "feature" => {
                        clauses.push(ExpandedModelClause {
                            items,
                            helpers: helpers.clone(),
                            component: Some(ComponentClause {
                                role: ComponentRole::Output,
                                spelling: "feature".to_string(),
                            }),
                        });
                        return Ok(());
                    }
                    _ => {}
                }
            }
            clauses.push(ExpandedModelClause {
                items,
                helpers: helpers.clone(),
                component: None,
            });
        }
        _ => {
            clauses.push(ExpandedModelClause {
                items: expr_list_items(form, "model clause")?,
                helpers: helpers.clone(),
                component: None,
            });
        }
    }
    Ok(())
}

fn model_let_helpers(let_expr: &Let, helpers: &ExpandedHelperMap) -> CoreResult<ExpandedHelperMap> {
    let mut scoped_helpers = helpers.clone();
    for (name_expr, value_expr) in &let_expr.bindings {
        let name = expr_value_symbol_or_text(name_expr, "model let binding name")?;
        scoped_helpers.insert(name, ExpandedHelper::Value(value_expr.clone()));
    }
    Ok(scoped_helpers)
}

fn model_list_let_helpers(
    bindings_expr: &ExprKind,
    helpers: &ExpandedHelperMap,
) -> CoreResult<ExpandedHelperMap> {
    let mut scoped_helpers = helpers.clone();
    for binding in expr_list_items(bindings_expr, "model let bindings")? {
        let pair = expr_list_items(&binding, "model let binding")?;
        if pair.len() != 2 {
            return Err(CompilerError::new(
                CompilerErrorKind::Parse,
                "Each model `let` binding must be `(name expr)`.",
            ));
        }
        let name = expr_value_symbol_or_text(&pair[0], "model let binding name")?;
        scoped_helpers.insert(name, ExpandedHelper::Value(pair[1].clone()));
    }
    Ok(scoped_helpers)
}

fn parse_expanded_param_decl(
    value: &ExprKind,
    next_param: &mut u64,
    helpers: &ExpandedHelperMap,
) -> CoreResult<CoreParameter> {
    let items = expr_list_items(value, "param declaration")?;
    let kind_name = expr_name(items.first().ok_or_else(|| {
        CompilerError::new(CompilerErrorKind::Parse, "Empty param declaration.")
    })?)?;
    if items.len() < 3 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            format!(
                "Param declaration `{}` needs a key and default value.",
                kind_name
            ),
        ));
    }
    let key = expr_value_symbol_or_text(&items[1], "param key")?;
    let default_expr = expand_helper_value_expr(&items[2], helpers, &BTreeSet::new())?;
    let default_value = match kind_name.as_str() {
        "number" => CoreParameterValue::Number(expr_number_value(&default_expr, "number default")?),
        "toggle" => CoreParameterValue::Boolean(expr_bool_value(&default_expr, "toggle default")?),
        "select" => {
            CoreParameterValue::Choice(expr_value_symbol_or_text(&default_expr, "select default")?)
        }
        "image" => {
            CoreParameterValue::Image(expr_value_symbol_or_text(&default_expr, "image default")?)
        }
        other => {
            return Err(CompilerError::new(
                CompilerErrorKind::UnsupportedFeature,
                format!("Unsupported param kind `{}`.", other),
            ))
        }
    };

    let mut constraints = CoreParameterConstraints::default();
    let mut label = humanize(&key);
    let mut frozen = false;

    let mut index = 3usize;
    while index < items.len() {
        let key_name = normalize_keyword(&expr_name(&items[index])?);
        match key_name.as_str() {
            ":label" => {
                label = expr_value_symbol_or_text(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:label` missing value.")
                    })?,
                    "param label",
                )?;
                index += 2;
            }
            ":min" => {
                constraints.min = Some(expr_number_value(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:min` missing value.")
                    })?,
                    "param min",
                )?);
                index += 2;
            }
            ":max" => {
                constraints.max = Some(expr_number_value(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:max` missing value.")
                    })?,
                    "param max",
                )?);
                index += 2;
            }
            ":step" => {
                constraints.step = Some(expr_number_value(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:step` missing value.")
                    })?,
                    "param step",
                )?);
                index += 2;
            }
            ":unit" => {
                constraints.unit = Some(parse_expanded_param_unit(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:unit` missing value.")
                    })?,
                )?);
                index += 2;
            }
            ":frozen" => {
                frozen = expr_bool_value(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:frozen` missing value.")
                    })?,
                    "param frozen",
                )?;
                index += 2;
            }
            ":options" => {
                let option_values = expr_list_items(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:options` missing value.")
                    })?,
                    "select options",
                )?;
                constraints.choices = option_values
                    .iter()
                    .map(parse_expanded_choice)
                    .collect::<CoreResult<Vec<_>>>()?;
                index += 2;
            }
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported param option `{}`.", other),
                ))
            }
        }
    }

    let kind = match kind_name.as_str() {
        "number" => CoreParameterKind::Number,
        "toggle" => CoreParameterKind::Boolean,
        "select" => CoreParameterKind::Choice,
        "image" => CoreParameterKind::Image,
        _ => unreachable!(),
    };

    let param = CoreParameter {
        id: ParamId::new(*next_param),
        key,
        label,
        kind,
        default_value,
        frozen,
        constraints,
    };
    *next_param += 1;
    Ok(param)
}

fn parse_expanded_param_unit(value: &ExprKind) -> CoreResult<String> {
    parse_param_unit_name(expr_value_symbol_or_text(value, "param unit")?)
}

fn parse_expanded_choice(value: &ExprKind) -> CoreResult<crate::ecky_core_ir::CoreChoice> {
    let items = expr_list_items(value, "select option")?;
    if items.len() != 2 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Select options must be `(label value)` pairs.",
        ));
    }
    Ok(crate::ecky_core_ir::CoreChoice {
        label: expr_value_symbol_or_text(&items[0], "option label")?,
        value: match &items[1] {
            ExprKind::Atom(atom) if matches!(atom.syn.ty, TokenType::Number(_)) => {
                CoreParameterValue::Number(expr_number_value(&items[1], "option number")?)
            }
            _ => CoreParameterValue::Choice(expr_value_symbol_or_text(&items[1], "option value")?),
        },
    })
}

fn parse_expanded_part_decl(
    items: &[ExprKind],
    next_part: &mut u64,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
) -> CoreResult<CorePart> {
    if items.len() < 3 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Each `(part ...)` needs an id and expression.",
        ));
    }
    let key = expr_value_symbol_or_text(&items[1], "part id")?;
    let (label, expr_value) = if items.len() >= 4
        && matches!(&items[2], ExprKind::Atom(Atom { syn }) if matches!(syn.ty, TokenType::StringLiteral(_) | TokenType::Identifier(_) | TokenType::Keyword(_)))
    {
        (
            expr_value_symbol_or_text(&items[2], "part label")?,
            &items[3],
        )
    } else {
        (humanize(&key), &items[2])
    };
    let root = parse_expanded_node(
        expr_value,
        next_node,
        param_ids,
        helpers,
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    )?;
    let part = CorePart {
        id: PartId::new(*next_part),
        key,
        label,
        root,
    };
    *next_part += 1;
    Ok(part)
}

fn parse_expanded_feature_decl(
    items: &[ExprKind],
    next_part: &mut u64,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
) -> CoreResult<(CorePart, CoreFeatureDecl)> {
    if items.len() < 5 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Each `(feature ...)` needs an id, `:role`, and expression body.",
        ));
    }

    let key = expr_value_symbol_or_text(&items[1], "feature id")?;
    let mut role = None;
    let mut param_keys = Vec::new();
    let mut body = None;
    let mut index = 2usize;

    while index < items.len() {
        let keyword = expr_name(&items[index])
            .ok()
            .map(|name| normalize_keyword(&name));
        match keyword.as_deref() {
            Some(":role") => {
                role = Some(expr_value_symbol_or_text(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(
                            CompilerErrorKind::Parse,
                            "`feature :role` missing value.",
                        )
                    })?,
                    "feature role",
                )?);
                index += 2;
            }
            Some(":params") => {
                let values = expr_list_items(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(
                            CompilerErrorKind::Parse,
                            "`feature :params` missing value list.",
                        )
                    })?,
                    "feature params",
                )?;
                param_keys = values
                    .iter()
                    .map(|value| expr_value_symbol_or_text(value, "feature param key"))
                    .collect::<CoreResult<Vec<_>>>()?;
                index += 2;
            }
            _ => {
                if index != items.len() - 1 {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "Feature clause expects a single trailing body expression.",
                    ));
                }
                body = Some(&items[index]);
                index += 1;
            }
        }
    }

    let role = role.ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "Feature clause requires `:role` metadata.",
        )
    })?;
    let body = body.ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "Feature clause requires a body expression.",
        )
    })?;
    let root = parse_expanded_node(
        body,
        next_node,
        param_ids,
        helpers,
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    )?;
    let part = CorePart {
        id: PartId::new(*next_part),
        key: key.clone(),
        label: humanize(&key),
        root,
    };
    *next_part += 1;
    Ok((
        part,
        CoreFeatureDecl {
            feature_id: key,
            role,
            param_keys,
        },
    ))
}

fn parse_expanded_node(
    value: &ExprKind,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<CoreNode> {
    let id = {
        let current = *next_node;
        *next_node += 1;
        NodeId::new(current)
    };

    let (kind, value_kind) = match value {
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::BooleanLiteral(flag) => (
                CoreNodeKind::Literal(CoreLiteral::Boolean(*flag)),
                CoreValueKind::Boolean,
            ),
            TokenType::Number(_number) => (
                CoreNodeKind::Literal(CoreLiteral::Number(expr_number_value(
                    value,
                    "number literal",
                )?)),
                CoreValueKind::Number,
            ),
            TokenType::StringLiteral(text) => (
                CoreNodeKind::Literal(CoreLiteral::Text(text.to_string())),
                CoreValueKind::Text,
            ),
            TokenType::Identifier(symbol) | TokenType::Keyword(symbol) => {
                match symbol.to_string().as_str() {
                    "start" => (
                        CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Start)),
                        CoreValueKind::Any,
                    ),
                    "end" => (
                        CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::End)),
                        CoreValueKind::Any,
                    ),
                    "xy" => (
                        CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Xy)),
                        CoreValueKind::Any,
                    ),
                    "yz" => (
                        CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Yz)),
                        CoreValueKind::Any,
                    ),
                    "xz" => (
                        CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Xz)),
                        CoreValueKind::Any,
                    ),
                    "min" => (
                        CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Min)),
                        CoreValueKind::Any,
                    ),
                    "center" => (
                        CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Center)),
                        CoreValueKind::Any,
                    ),
                    "max" => (
                        CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Max)),
                        CoreValueKind::Any,
                    ),
                    "true" => (
                        CoreNodeKind::Literal(CoreLiteral::Boolean(true)),
                        CoreValueKind::Boolean,
                    ),
                    "false" => (
                        CoreNodeKind::Literal(CoreLiteral::Boolean(false)),
                        CoreValueKind::Boolean,
                    ),
                    name if local_names.contains(name) => (
                        CoreNodeKind::Reference(CoreReference::Local(name.to_string())),
                        CoreValueKind::Any,
                    ),
                    name if node_refs.contains_key(name) => (
                        CoreNodeKind::Reference(CoreReference::Node(*node_refs.get(name).unwrap())),
                        CoreValueKind::Any,
                    ),
                    name if param_ids.contains_key(name) => (
                        CoreNodeKind::Reference(CoreReference::Parameter(
                            *param_ids.get(name).unwrap(),
                        )),
                        CoreValueKind::Any,
                    ),
                    name => {
                        if let Some(ExpandedHelper::Value(helper_expr)) = helpers.get(name) {
                            if helper_stack.contains(name) {
                                return Err(CompilerError::new(
                                    CompilerErrorKind::UnsupportedFeature,
                                    format!(
                                        "Recursive helper value `{}` is not supported by expanded AST compile.",
                                        name
                                    ),
                                ));
                            }
                            let mut next_stack = helper_stack.clone();
                            next_stack.insert(name.to_string());
                            return parse_expanded_node(
                                helper_expr,
                                next_node,
                                param_ids,
                                helpers,
                                node_refs,
                                local_names,
                                &next_stack,
                            );
                        }
                        (
                            CoreNodeKind::Reference(CoreReference::Local(name.to_string())),
                            CoreValueKind::Any,
                        )
                    }
                }
            }
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported Steel value in CAD compiler: {:?}", other),
                ))
            }
        },
        ExprKind::List(_) | ExprKind::Vector(_) => {
            let items = expr_list_items(value, "node expression")?;
            if is_point_literal_expr(&items) {
                match items.len() {
                    2 => (
                        CoreNodeKind::Literal(CoreLiteral::Point2([
                            expr_number_value(&items[0], "point x")?,
                            expr_number_value(&items[1], "point y")?,
                        ])),
                        CoreValueKind::Point2,
                    ),
                    3 => (
                        CoreNodeKind::Literal(CoreLiteral::Point3([
                            expr_number_value(&items[0], "point x")?,
                            expr_number_value(&items[1], "point y")?,
                            expr_number_value(&items[2], "point z")?,
                        ])),
                        CoreValueKind::Point3,
                    ),
                    _ => unreachable!(),
                }
            } else if let Some(head) = items.first() {
                if let Ok(op_name) = expr_name(head).map(|name| normalize_hygienic_op_name(&name)) {
                    if op_name == "build" {
                        let build = parse_expanded_build_node(
                            &items,
                            next_node,
                            param_ids,
                            helpers,
                            local_names,
                            helper_stack,
                        )?;
                        (build, CoreValueKind::Solid)
                    } else if op_name == "hole" {
                        parse_expanded_typed_hole_call(&items[1..], next_node)?
                    } else if op_name == "verify" {
                        return Err(CompilerError::new(
                            CompilerErrorKind::UnsupportedFeature,
                            "`verify` is top-level only in this slice.",
                        ));
                    } else if op_name == "list" {
                        parse_expanded_list_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "append" {
                        parse_expanded_append_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "reverse" {
                        parse_expanded_reverse_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "range" {
                        parse_expanded_range_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "map" {
                        parse_expanded_map_node(
                            &op_name,
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "filter" {
                        parse_expanded_filter_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if matches!(op_name.as_str(), "fold" | "reduce" | "foldl") {
                        parse_expanded_fold_node(
                            &op_name,
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "zip" {
                        parse_expanded_zip_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "enumerate" {
                        parse_expanded_enumerate_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "linspace" {
                        parse_expanded_linspace_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if matches!(op_name.as_str(), "jitter2" | "superellipse-point") {
                        parse_expanded_point_helper_node(
                            &op_name,
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if matches!(
                        op_name.as_str(),
                        "jittered-grid"
                            | "polar-points"
                            | "organic-loop"
                            | "wave-loop"
                            | "voronoi-cells"
                            | "lorenz-points"
                            | "rossler-points"
                            | "logistic-bifurcation-points"
                            | "henon-points"
                    ) {
                        parse_expanded_fancy_list_node(
                            &op_name,
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if matches!(
                        op_name.as_str(),
                        "flat-map" | "concat-map" | "flat_map" | "concat_map"
                    ) {
                        parse_expanded_flat_map_node(
                            &op_name,
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "apply" {
                        parse_expanded_apply_node(
                            &items[1..],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if matches!(
                        head,
                        ExprKind::Atom(atom) if matches!(&atom.syn.ty, TokenType::Keyword(_))
                    ) {
                        (
                            CoreNodeKind::List(
                                items
                                    .iter()
                                    .map(|item| {
                                        parse_expanded_node(
                                            item,
                                            next_node,
                                            param_ids,
                                            helpers,
                                            node_refs,
                                            local_names,
                                            helper_stack,
                                        )
                                    })
                                    .collect::<CoreResult<Vec<_>>>()?,
                            ),
                            CoreValueKind::List,
                        )
                    } else if op_name == "if" && items.len() == 4 {
                        (
                            CoreNodeKind::If {
                                condition: Box::new(parse_expanded_node(
                                    &items[1],
                                    next_node,
                                    param_ids,
                                    helpers,
                                    node_refs,
                                    local_names,
                                    helper_stack,
                                )?),
                                then_branch: Box::new(parse_expanded_node(
                                    &items[2],
                                    next_node,
                                    param_ids,
                                    helpers,
                                    node_refs,
                                    local_names,
                                    helper_stack,
                                )?),
                                else_branch: Box::new(parse_expanded_node(
                                    &items[3],
                                    next_node,
                                    param_ids,
                                    helpers,
                                    node_refs,
                                    local_names,
                                    helper_stack,
                                )?),
                            },
                            CoreValueKind::Any,
                        )
                    } else if op_name == "let" && items.len() == 3 {
                        parse_expanded_let_node(
                            &items[1],
                            &items[2],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if op_name == "let*" && items.len() == 3 {
                        parse_expanded_let_star_node(
                            &items[1],
                            &items[2],
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            local_names,
                            helper_stack,
                        )?
                    } else if local_names.contains(&op_name)
                        || node_refs.contains_key(&op_name)
                        || param_ids.contains_key(&op_name)
                    {
                        (
                            CoreNodeKind::List(
                                items
                                    .iter()
                                    .map(|item| {
                                        parse_expanded_node(
                                            item,
                                            next_node,
                                            param_ids,
                                            helpers,
                                            node_refs,
                                            local_names,
                                            helper_stack,
                                        )
                                    })
                                    .collect::<CoreResult<Vec<_>>>()?,
                            ),
                            CoreValueKind::List,
                        )
                    } else if !local_names.contains(&op_name)
                        && !node_refs.contains_key(&op_name)
                        && !param_ids.contains_key(&op_name)
                        && matches!(helpers.get(&op_name), Some(ExpandedHelper::Function { .. }))
                    {
                        let Some(ExpandedHelper::Function { params, body }) = helpers.get(&op_name)
                        else {
                            unreachable!();
                        };
                        if helper_stack.contains(&op_name) {
                            return Err(CompilerError::new(
                                CompilerErrorKind::UnsupportedFeature,
                                format!(
                                    "Recursive helper function `{}` is not supported by expanded AST compile.",
                                    op_name
                                ),
                            ));
                        }
                        if params.len() != items.len().saturating_sub(1) {
                            return Err(CompilerError::new(
                                CompilerErrorKind::UnsupportedFeature,
                                format!(
                                    "Helper `{}` expected {} arguments, received {}.",
                                    op_name,
                                    params.len(),
                                    items.len().saturating_sub(1)
                                ),
                            ));
                        }
                        let mut next_stack = helper_stack.clone();
                        next_stack.insert(op_name.clone());
                        let mut bindings = Vec::new();
                        let mut nested_locals = local_names.clone();
                        for (param, arg_expr) in params.iter().zip(items.iter().skip(1)) {
                            let value = parse_expanded_node(
                                arg_expr,
                                next_node,
                                param_ids,
                                helpers,
                                node_refs,
                                local_names,
                                helper_stack,
                            )?;
                            bindings.push(CoreBinding {
                                name: param.clone(),
                                value,
                            });
                            nested_locals.insert(param.clone());
                        }
                        let body = parse_expanded_node(
                            body,
                            next_node,
                            param_ids,
                            helpers,
                            node_refs,
                            &nested_locals,
                            &next_stack,
                        )?;
                        let value_kind = body.value_kind;
                        (
                            CoreNodeKind::Let {
                                bindings,
                                body: Box::new(body),
                            },
                            value_kind,
                        )
                    } else {
                        let mut args = Vec::new();
                        let mut keywords = Vec::new();
                        let mut index = 1usize;
                        let mut body_locals = local_names.clone();
                        if matches!(
                            op_name.as_str(),
                            "repeat" | "repeat-union" | "repeat-compound" | "repeat-pick"
                        ) {
                            if let Some(index_symbol) = items.get(1).and_then(expr_identifier) {
                                body_locals.insert(index_symbol);
                            }
                        }
                        while index < items.len() {
                            if let ExprKind::Atom(atom) = &items[index] {
                                if let TokenType::Keyword(name) = &atom.syn.ty {
                                    let normalized = normalize_keyword(&name.to_string());
                                    if normalized.starts_with(':') && index + 1 < items.len() {
                                        let value = parse_expanded_node(
                                            &items[index + 1],
                                            next_node,
                                            param_ids,
                                            helpers,
                                            node_refs,
                                            &body_locals,
                                            helper_stack,
                                        )?;
                                        let keyword_name =
                                            normalized.trim_start_matches(':').to_string();
                                        keywords.push(
                                            selector_payload_for_keyword(&keyword_name, &value)
                                                .map(|selector| {
                                                    CoreKeywordArg::selector(
                                                        keyword_name.clone(),
                                                        value.clone(),
                                                        selector,
                                                    )
                                                })
                                                .unwrap_or_else(|| {
                                                    CoreKeywordArg::expr(
                                                        keyword_name.clone(),
                                                        value,
                                                    )
                                                }),
                                        );
                                        index += 2;
                                        continue;
                                    }
                                }
                            }
                            args.push(parse_expanded_node(
                                &items[index],
                                next_node,
                                param_ids,
                                helpers,
                                node_refs,
                                &body_locals,
                                helper_stack,
                            )?);
                            index += 1;
                        }
                        if op_name == "ring" {
                            parse_ring_alias_call(args, keywords, next_node)?
                        } else {
                            (
                                CoreNodeKind::Call {
                                    op: map_operation(&op_name),
                                    args,
                                    keywords,
                                },
                                infer_value_kind(&op_name),
                            )
                        }
                    }
                } else {
                    (
                        CoreNodeKind::List(
                            items
                                .iter()
                                .map(|item| {
                                    parse_expanded_node(
                                        item,
                                        next_node,
                                        param_ids,
                                        helpers,
                                        node_refs,
                                        local_names,
                                        helper_stack,
                                    )
                                })
                                .collect::<CoreResult<Vec<_>>>()?,
                        ),
                        CoreValueKind::List,
                    )
                }
            } else {
                (CoreNodeKind::List(Vec::new()), CoreValueKind::List)
            }
        }
        ExprKind::If(if_expr) => (
            CoreNodeKind::If {
                condition: Box::new(parse_expanded_node(
                    &if_expr.test_expr,
                    next_node,
                    param_ids,
                    helpers,
                    node_refs,
                    local_names,
                    helper_stack,
                )?),
                then_branch: Box::new(parse_expanded_node(
                    &if_expr.then_expr,
                    next_node,
                    param_ids,
                    helpers,
                    node_refs,
                    local_names,
                    helper_stack,
                )?),
                else_branch: Box::new(parse_expanded_node(
                    &if_expr.else_expr,
                    next_node,
                    param_ids,
                    helpers,
                    node_refs,
                    local_names,
                    helper_stack,
                )?),
            },
            CoreValueKind::Any,
        ),
        ExprKind::Let(let_expr) => parse_expanded_let_struct(
            let_expr,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )?,
        ExprKind::Begin(begin) => {
            let mut items = Vec::new();
            for item in &begin.exprs {
                items.push(parse_expanded_node(
                    item,
                    next_node,
                    param_ids,
                    helpers,
                    node_refs,
                    local_names,
                    helper_stack,
                )?);
            }
            let value_kind = items
                .last()
                .map(|node| node.value_kind)
                .unwrap_or(CoreValueKind::List);
            (CoreNodeKind::Group(items), value_kind)
        }
        other => {
            return Err(CompilerError::new(
                CompilerErrorKind::UnsupportedFeature,
                format!("Unsupported Steel value in CAD compiler: {:?}", other),
            ))
        }
    };

    Ok(core_node_with_span(
        id,
        kind,
        value_kind,
        expr_source_span(value),
    ))
}

fn core_node_with_span(
    id: NodeId,
    kind: CoreNodeKind,
    value_kind: CoreValueKind,
    span: Option<SourceSpan>,
) -> CoreNode {
    let mut node = CoreNode::new(id, kind, value_kind);
    if let Some(span) = span {
        node = node.with_span(span);
    }
    node
}

fn alloc_node_id(next_node: &mut u64) -> NodeId {
    let current = *next_node;
    *next_node += 1;
    NodeId::new(current)
}

fn expr_source_span(value: &ExprKind) -> Option<SourceSpan> {
    let span = value.span();
    if span.start == 0 && span.end == 0 && span.source_id.is_none() {
        return None;
    }
    Some(SourceSpan::new(
        span.source_id().map(|id| SourceFileId::new(id.0 as u64)),
        span.start(),
        span.end(),
    ))
}

fn parse_expanded_let_struct(
    let_expr: &Let,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let mut bindings = Vec::new();
    let mut nested_locals = local_names.clone();
    for (name_expr, value_expr) in &let_expr.bindings {
        let name = expr_value_symbol_or_text(name_expr, "let binding name")?;
        let value = parse_expanded_node(
            value_expr,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )?;
        bindings.push(CoreBinding {
            name: name.clone(),
            value,
        });
        nested_locals.insert(name);
    }
    let body = parse_expanded_node(
        &let_expr.body_expr,
        next_node,
        param_ids,
        helpers,
        node_refs,
        &nested_locals,
        helper_stack,
    )?;
    let value_kind = body.value_kind;
    Ok((
        CoreNodeKind::Let {
            bindings,
            body: Box::new(body),
        },
        value_kind,
    ))
}

fn parse_expanded_list_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let items = args
        .iter()
        .map(|arg| {
            parse_expanded_node(
                arg,
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                helper_stack,
            )
        })
        .collect::<CoreResult<Vec<_>>>()?;
    let value_kind = infer_list_value_kind(&items);
    Ok((CoreNodeKind::List(items), value_kind))
}

fn parse_expanded_append_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let mut combined = Vec::new();
    for arg in args {
        let node = parse_expanded_node(
            arg,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )?;
        combined.extend(extract_list_items(node, "`append`", next_node)?);
    }
    let value_kind = infer_list_value_kind(&combined);
    Ok((CoreNodeKind::List(combined), value_kind))
}

fn parse_expanded_reverse_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.len() != 1 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "`reverse` expects exactly one list.",
        ));
    }
    let node = parse_expanded_node(
        &args[0],
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    let mut items = extract_list_items(node, "`reverse`", next_node)?;
    items.reverse();
    let value_kind = infer_list_value_kind(&items);
    Ok((CoreNodeKind::List(items), value_kind))
}

fn parse_expanded_range_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let (start_expr, end_expr) = match args {
        [end] => (None, end),
        [start, end] => (Some(start), end),
        _ => {
            return Err(CompilerError::new(
                CompilerErrorKind::Parse,
                "`range` expects one or two integer bounds.",
            ))
        }
    };

    let literal_start = match start_expr {
        Some(start) => parse_optional_integer_literal(start, "`range` start")?,
        None => Some(0),
    };
    let literal_end = parse_optional_integer_literal(end_expr, "`range` end")?;
    if let (Some(start), Some(end)) = (literal_start, literal_end) {
        if end < start {
            return Err(CompilerError::new(
                CompilerErrorKind::TypeMismatch,
                "`range` end must be greater than or equal to start.",
            ));
        }
        let items = (start..end)
            .map(|value| {
                core_node_with_span(
                    alloc_node_id(next_node),
                    CoreNodeKind::Literal(CoreLiteral::Number(value as f64)),
                    CoreValueKind::Number,
                    None,
                )
            })
            .collect::<Vec<_>>();
        return Ok((CoreNodeKind::List(items), CoreValueKind::List));
    }

    let start = match start_expr {
        Some(start) => parse_expanded_node(
            start,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )?,
        None => number_literal_node(0.0, next_node, expr_source_span(end_expr)),
    };
    let end = parse_expanded_node(
        end_expr,
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    Ok((
        CoreNodeKind::Range {
            start: Box::new(start),
            end: Box::new(end),
        },
        CoreValueKind::List,
    ))
}

fn parse_expanded_map_node(
    op_name: &str,
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.len() < 2 {
        return Err(sequence_arity_error(
            &format!("`{}`", op_name),
            "function and at least one list",
            args.len(),
            args.first().and_then(expr_source_span),
        ));
    }
    let parsed_sources = args[1..]
        .iter()
        .map(|arg| {
            parse_expanded_node(
                arg,
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                helper_stack,
            )
        })
        .collect::<CoreResult<Vec<_>>>()?;
    let mut static_sources = Vec::new();
    let mut all_static = true;
    for source in &parsed_sources {
        match extract_list_items(
            clone_node_with_fresh_ids(source, next_node),
            &format!("`{}` source", op_name),
            next_node,
        ) {
            Ok(items) => static_sources.push(items),
            Err(_err) if source.value_kind == CoreValueKind::List => {
                all_static = false;
                break;
            }
            Err(err) => return Err(err),
        }
    }
    if !all_static {
        return parse_expanded_dynamic_map_node(
            op_name,
            &args[0],
            parsed_sources,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        );
    }

    let mapped = zip_sequence_sources(static_sources)
        .into_iter()
        .map(|items| {
            compile_sequence_callable_application(
                op_name,
                &args[0],
                items,
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                helper_stack,
            )
        })
        .collect::<CoreResult<Vec<_>>>()?;
    let value_kind = infer_list_value_kind(&mapped);
    Ok((CoreNodeKind::List(mapped), value_kind))
}

fn parse_expanded_dynamic_map_node(
    op_name: &str,
    callable: &ExprKind,
    sources: Vec<CoreNode>,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let (params, body_expr, nested_stack) = match callable {
        ExprKind::LambdaFunction(lambda) => {
            if lambda.rest || lambda.kwargs {
                return Err(sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "fixed-arity function",
                    "variadic function",
                    expr_source_span(callable),
                ));
            }
            let params = lambda
                .args
                .iter()
                .map(|arg| {
                    expr_identifier(arg).ok_or_else(|| {
                        sequence_callable_kind_error(
                            &format!("`{}` lambda parameter", op_name),
                            "symbol",
                            &expr_actual_kind_label(arg),
                            expr_source_span(arg),
                        )
                    })
                })
                .collect::<CoreResult<Vec<_>>>()?;
            (params, lambda.body.clone(), helper_stack.clone())
        }
        func if expr_identifier(func)
            .and_then(|name| helpers.get(&name).map(|helper| (name, helper)))
            .is_some() =>
        {
            let (name, helper) = expr_identifier(func)
                .and_then(|helper_name| {
                    helpers
                        .get(&helper_name)
                        .map(|helper| (helper_name, helper))
                })
                .unwrap();
            let ExpandedHelper::Function { params, body } = helper else {
                return Err(sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "function",
                    "value",
                    expr_source_span(callable),
                ));
            };
            if helper_stack.contains(&name) {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Recursive helper function `{}` is not supported.", name),
                )
                .with_span(expr_source_span(callable).unwrap_or(SourceSpan::new(None, 0, 0))));
            }
            let mut nested_stack = helper_stack.clone();
            nested_stack.insert(name);
            (params.clone(), body.clone(), nested_stack)
        }
        _ => {
            return Err(sequence_callable_kind_error(
                &format!("`{}`", op_name),
                "lambda or helper function for dynamic list source",
                &expr_actual_kind_label(callable),
                expr_source_span(callable),
            ))
        }
    };
    if params.len() != sources.len() {
        return Err(sequence_callable_arity_error(
            &format!("`{}`", op_name),
            sources.len(),
            params.len(),
            expr_source_span(callable),
        ));
    }

    let mut nested_locals = local_names.clone();
    for param in &params {
        nested_locals.insert(param.clone());
    }
    let body = parse_expanded_node(
        &body_expr,
        next_node,
        param_ids,
        helpers,
        node_refs,
        &nested_locals,
        &nested_stack,
    )?;
    Ok((
        CoreNodeKind::Map {
            params,
            sources,
            body: Box::new(body),
        },
        CoreValueKind::List,
    ))
}

fn parse_expanded_apply_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.len() < 2 {
        return Err(sequence_arity_error(
            "`apply`",
            "function and list argument",
            args.len(),
            args.first().and_then(expr_source_span),
        ));
    }
    let target_name = expr_identifier(&args[0])
        .map(|name| normalize_hygienic_op_name(&name))
        .ok_or_else(|| {
            sequence_callable_kind_error(
                "`apply`",
                "global CAD operation",
                &expr_actual_kind_label(&args[0]),
                expr_source_span(&args[0]),
            )
        })?;
    if local_names.contains(&target_name)
        || node_refs.contains_key(&target_name)
        || param_ids.contains_key(&target_name)
    {
        return Err(sequence_callable_kind_error(
            "`apply`",
            "global CAD operation",
            "reference",
            expr_source_span(&args[0]),
        ));
    }
    if !is_apply_splice_operation(&target_name) {
        return Err(CompilerError::new(
            CompilerErrorKind::UnsupportedFeature,
            format!("`apply` currently supports CAD variadic operations, got `{target_name}`."),
        )
        .with_span(expr_source_span(&args[0]).unwrap_or(SourceSpan::new(None, 0, 0))));
    }

    let fixed_args = args[1..args.len() - 1]
        .iter()
        .map(|arg| {
            parse_expanded_node(
                arg,
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                helper_stack,
            )
        })
        .collect::<CoreResult<Vec<_>>>()?;
    let list = parse_expanded_node(
        args.last().expect("apply list arg"),
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    if list.value_kind != CoreValueKind::List {
        return Err(sequence_type_mismatch_error(
            "`apply` final argument",
            "list",
            core_value_kind_label(list.value_kind),
            list.span,
        ));
    }
    Ok((
        CoreNodeKind::Apply {
            op: map_operation(&target_name),
            args: fixed_args,
            list: Box::new(list),
        },
        infer_value_kind(&target_name),
    ))
}

fn parse_expanded_filter_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.len() != 2 {
        return Err(sequence_arity_error(
            "`filter`",
            "predicate and one list",
            args.len(),
            args.first().and_then(expr_source_span),
        ));
    }
    let mut sources = collect_sequence_sources(
        "filter",
        &args[1..],
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    let items = sources.pop().expect("filter source list");
    let filtered = items
        .into_iter()
        .map(|item| {
            let keep = compile_sequence_callable_application(
                "filter",
                &args[0],
                vec![clone_node_with_fresh_ids(&item, next_node)],
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                helper_stack,
            )?;
            Ok((item, evaluate_sequence_boolean("filter", &keep)?))
        })
        .collect::<CoreResult<Vec<_>>>()?
        .into_iter()
        .filter_map(|(item, keep)| keep.then_some(item))
        .collect::<Vec<_>>();
    let value_kind = infer_list_value_kind(&filtered);
    Ok((CoreNodeKind::List(filtered), value_kind))
}

fn parse_expanded_fold_node(
    op_name: &str,
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.len() != 3 {
        return Err(sequence_arity_error(
            &format!("`{}`", op_name),
            "function, initial value, and one list",
            args.len(),
            args.first().and_then(expr_source_span),
        ));
    }
    let mut sources = collect_sequence_sources(
        op_name,
        &args[2..],
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    let items = sources.pop().expect("fold source list");
    let mut accum = parse_expanded_node(
        &args[1],
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    for item in items {
        accum = compile_sequence_callable_application(
            op_name,
            &args[0],
            vec![item, accum],
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )?;
    }
    let value_kind = accum.value_kind;
    Ok((accum.kind, value_kind))
}

fn parse_expanded_zip_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.is_empty() {
        return Err(sequence_arity_error("`zip`", "at least one list", 0, None));
    }
    let zipped = zip_sequence_sources(collect_sequence_sources(
        "zip",
        args,
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?)
    .into_iter()
    .map(|items| {
        let value_kind = infer_list_value_kind(&items);
        core_node_with_span(
            alloc_node_id(next_node),
            CoreNodeKind::List(items),
            value_kind,
            None,
        )
    })
    .collect::<Vec<_>>();
    Ok((CoreNodeKind::List(zipped), CoreValueKind::List))
}

fn parse_expanded_enumerate_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.len() != 1 {
        return Err(sequence_arity_error(
            "`enumerate`",
            "one list",
            args.len(),
            args.first().and_then(expr_source_span),
        ));
    }
    let mut sources = collect_sequence_sources(
        "enumerate",
        args,
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    let items = sources.pop().expect("enumerate source list");
    let enumerated = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let pair = vec![
                number_literal_node(index as f64, next_node, expr_source_span(&args[0])),
                item,
            ];
            core_node_with_span(
                alloc_node_id(next_node),
                CoreNodeKind::List(pair),
                CoreValueKind::List,
                expr_source_span(&args[0]),
            )
        })
        .collect::<Vec<_>>();
    Ok((CoreNodeKind::List(enumerated), CoreValueKind::List))
}

fn parse_expanded_linspace_node(
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.len() != 3 {
        return Err(sequence_arity_error(
            "`linspace`",
            "start, end, and count",
            args.len(),
            args.first().and_then(expr_source_span),
        ));
    }
    let count = parse_integer_literal(&args[2], "`linspace` count")?;
    if count < 1 {
        return Err(sequence_type_mismatch_error(
            "`linspace` count",
            "positive integer",
            &count.to_string(),
            expr_source_span(&args[2]),
        ));
    }

    let start_literal = expr_number_value(&args[0], "`linspace` start").ok();
    let end_literal = expr_number_value(&args[1], "`linspace` end").ok();
    let items = if count == 1 {
        if let Some(start) = start_literal {
            vec![number_literal_node(
                start,
                next_node,
                expr_source_span(&args[0]),
            )]
        } else {
            vec![parse_expanded_node(
                &args[0],
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                helper_stack,
            )?]
        }
    } else if let (Some(start), Some(end)) = (start_literal, end_literal) {
        (0..count)
            .map(|index| {
                let t = index as f64 / (count - 1) as f64;
                let value = start + (end - start) * t;
                number_literal_node(value, next_node, expr_source_span(&args[0]))
            })
            .collect::<Vec<_>>()
    } else {
        (0..count)
            .map(|index| {
                let start = parse_expanded_node(
                    &args[0],
                    next_node,
                    param_ids,
                    helpers,
                    node_refs,
                    local_names,
                    helper_stack,
                )?;
                let end = parse_expanded_node(
                    &args[1],
                    next_node,
                    param_ids,
                    helpers,
                    node_refs,
                    local_names,
                    helper_stack,
                )?;
                let t = number_literal_node(
                    index as f64 / (count - 1) as f64,
                    next_node,
                    expr_source_span(&args[2]),
                );
                Ok(core_node_with_span(
                    alloc_node_id(next_node),
                    CoreNodeKind::Call {
                        op: CoreOperation::Custom("lerp".to_string()),
                        args: vec![start, end, t],
                        keywords: Vec::new(),
                    },
                    CoreValueKind::Number,
                    expr_source_span(&args[0]).or(expr_source_span(&args[1])),
                ))
            })
            .collect::<CoreResult<Vec<_>>>()?
    };
    Ok((CoreNodeKind::List(items), CoreValueKind::List))
}

fn parse_expanded_point_helper_node(
    op_name: &str,
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let parse = |expr: &ExprKind, next_node: &mut u64| {
        parse_expanded_node(
            expr,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )
    };
    let point = match op_name {
        "jitter2" => {
            if args.len() != 4 {
                return Err(sequence_arity_error(
                    "`jitter2`",
                    "x, y, amount, and seed",
                    args.len(),
                    args.first().and_then(expr_source_span),
                ));
            }
            jitter_point_nodes(
                parse(&args[0], next_node)?,
                parse(&args[1], next_node)?,
                parse(&args[2], next_node)?,
                parse(&args[3], next_node)?,
                next_node,
                args.first().and_then(expr_source_span),
            )
        }
        "superellipse-point" => {
            if args.len() != 4 {
                return Err(sequence_arity_error(
                    "`superellipse-point`",
                    "rx, ry, n, and t",
                    args.len(),
                    args.first().and_then(expr_source_span),
                ));
            }
            let span = args.first().and_then(expr_source_span);
            let angle = mul_number_node(
                number_literal_node(std::f64::consts::TAU, next_node, span),
                parse(&args[3], next_node)?,
                next_node,
                span,
            );
            let exponent = div_number_node(
                number_literal_node(2.0, next_node, span),
                parse(&args[2], next_node)?,
                next_node,
                span,
            );
            vec![
                mul_number_node(
                    parse(&args[0], next_node)?,
                    call_number_node(
                        "signed-pow",
                        vec![
                            call_number_node("cos", vec![angle.clone()], next_node, span),
                            exponent.clone(),
                        ],
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                ),
                mul_number_node(
                    parse(&args[1], next_node)?,
                    call_number_node(
                        "signed-pow",
                        vec![
                            call_number_node("sin", vec![angle], next_node, span),
                            exponent,
                        ],
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                ),
            ]
        }
        _ => unreachable!(),
    };
    Ok((CoreNodeKind::List(point), CoreValueKind::Point2))
}

fn parse_expanded_fancy_list_node(
    op_name: &str,
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let span = args.first().and_then(expr_source_span);
    let parse = |expr: &ExprKind, next_node: &mut u64| {
        parse_expanded_node(
            expr,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )
    };
    let mut points = Vec::new();
    match op_name {
        "polar-points" => {
            if args.len() != 2 {
                return Err(sequence_arity_error(
                    "`polar-points`",
                    "count and radius",
                    args.len(),
                    span,
                ));
            }
            let count = parse_positive_count(&args[0], "`polar-points` count")?;
            for index in 0..count {
                let angle = number_literal_node(
                    std::f64::consts::TAU * index as f64 / count as f64,
                    next_node,
                    span,
                );
                points.push(point2_node(
                    mul_number_node(
                        parse(&args[1], next_node)?,
                        call_number_node("cos", vec![angle.clone()], next_node, span),
                        next_node,
                        span,
                    ),
                    mul_number_node(
                        parse(&args[1], next_node)?,
                        call_number_node("sin", vec![angle], next_node, span),
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                ));
            }
        }
        "organic-loop" => {
            if args.len() != 4 {
                return Err(sequence_arity_error(
                    "`organic-loop`",
                    "count, radius, amount, and seed",
                    args.len(),
                    span,
                ));
            }
            let count = parse_positive_count(&args[0], "`organic-loop` count")?;
            for index in 0..count {
                let angle = number_literal_node(
                    std::f64::consts::TAU * index as f64 / count as f64,
                    next_node,
                    span,
                );
                let radius = add_number_node(
                    parse(&args[1], next_node)?,
                    mul_number_node(
                        parse(&args[2], next_node)?,
                        call_number_node(
                            "hash-signed",
                            vec![
                                number_literal_node(index as f64, next_node, span),
                                number_literal_node(count as f64, next_node, span),
                                parse(&args[3], next_node)?,
                            ],
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                );
                points.push(point2_node(
                    mul_number_node(
                        radius.clone(),
                        call_number_node("cos", vec![angle.clone()], next_node, span),
                        next_node,
                        span,
                    ),
                    mul_number_node(
                        radius,
                        call_number_node("sin", vec![angle], next_node, span),
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                ));
            }
        }
        "wave-loop" => {
            if args.len() != 6 {
                return Err(sequence_arity_error(
                    "`wave-loop`",
                    "count, rx, ry, amp, waves, and seed",
                    args.len(),
                    span,
                ));
            }
            let count = parse_positive_count(&args[0], "`wave-loop` count")?;
            for index in 0..count {
                let angle = number_literal_node(
                    std::f64::consts::TAU * index as f64 / count as f64,
                    next_node,
                    span,
                );
                let wave_phase = add_number_node(
                    mul_number_node(parse(&args[4], next_node)?, angle.clone(), next_node, span),
                    mul_number_node(
                        number_literal_node(std::f64::consts::TAU, next_node, span),
                        call_number_node(
                            "hash01",
                            vec![
                                number_literal_node(index as f64, next_node, span),
                                parse(&args[4], next_node)?,
                                parse(&args[5], next_node)?,
                            ],
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                );
                let wave = mul_number_node(
                    parse(&args[3], next_node)?,
                    call_number_node("sin", vec![wave_phase], next_node, span),
                    next_node,
                    span,
                );
                points.push(point2_node(
                    mul_number_node(
                        add_number_node(parse(&args[1], next_node)?, wave.clone(), next_node, span),
                        call_number_node("cos", vec![angle.clone()], next_node, span),
                        next_node,
                        span,
                    ),
                    mul_number_node(
                        add_number_node(parse(&args[2], next_node)?, wave, next_node, span),
                        call_number_node("sin", vec![angle], next_node, span),
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                ));
            }
        }
        "jittered-grid" | "voronoi-cells" => {
            if args.len() != 6 {
                return Err(sequence_arity_error(
                    &format!("`{}`", op_name),
                    "rows, cols, dx, dy, amount, and seed",
                    args.len(),
                    span,
                ));
            }
            let rows = parse_positive_count(&args[0], &format!("`{}` rows", op_name))?;
            let cols = parse_positive_count(&args[1], &format!("`{}` cols", op_name))?;
            for row in 0..rows {
                for col in 0..cols {
                    let x = mul_number_node(
                        number_literal_node(col as f64 - (cols - 1) as f64 / 2.0, next_node, span),
                        parse(&args[2], next_node)?,
                        next_node,
                        span,
                    );
                    let y = mul_number_node(
                        number_literal_node(row as f64 - (rows - 1) as f64 / 2.0, next_node, span),
                        parse(&args[3], next_node)?,
                        next_node,
                        span,
                    );
                    let seed = add_number_node(
                        parse(&args[5], next_node)?,
                        number_literal_node((row * 1009 + col) as f64, next_node, span),
                        next_node,
                        span,
                    );
                    let mut jittered = jitter_point_nodes(
                        x,
                        y,
                        parse(&args[4], next_node)?,
                        seed,
                        next_node,
                        span,
                    );
                    let jy = jittered.pop().expect("jitter2 y");
                    let jx = jittered.pop().expect("jitter2 x");
                    points.push(point2_node(jx, jy, next_node, span));
                }
            }
        }
        "lorenz-points" => {
            if args.len() != 3 {
                return Err(sequence_arity_error(
                    "`lorenz-points`",
                    "count, dt, and scale",
                    args.len(),
                    span,
                ));
            }
            let count = parse_positive_count(&args[0], "`lorenz-points` count")?;
            let mut x = number_literal_node(0.1, next_node, span);
            let mut y = number_literal_node(0.0, next_node, span);
            let mut z = number_literal_node(0.0, next_node, span);
            for _ in 0..count {
                let sigma = number_literal_node(10.0, next_node, span);
                let rho = number_literal_node(28.0, next_node, span);
                let beta = div_number_node(
                    number_literal_node(8.0, next_node, span),
                    number_literal_node(3.0, next_node, span),
                    next_node,
                    span,
                );
                let dx = mul_number_node(
                    sigma,
                    sub_number_node(y.clone(), x.clone(), next_node, span),
                    next_node,
                    span,
                );
                let dy = sub_number_node(
                    mul_number_node(
                        x.clone(),
                        sub_number_node(rho, z.clone(), next_node, span),
                        next_node,
                        span,
                    ),
                    y.clone(),
                    next_node,
                    span,
                );
                let dz = sub_number_node(
                    mul_number_node(x.clone(), y.clone(), next_node, span),
                    mul_number_node(beta, z.clone(), next_node, span),
                    next_node,
                    span,
                );
                x = add_number_node(
                    x,
                    mul_number_node(parse(&args[1], next_node)?, dx, next_node, span),
                    next_node,
                    span,
                );
                y = add_number_node(
                    y,
                    mul_number_node(parse(&args[1], next_node)?, dy, next_node, span),
                    next_node,
                    span,
                );
                z = add_number_node(
                    z,
                    mul_number_node(parse(&args[1], next_node)?, dz, next_node, span),
                    next_node,
                    span,
                );
                let scale = parse(&args[2], next_node)?;
                points.push(bounded_point3_node(
                    mul_number_node(
                        scale.clone(),
                        div_number_node(
                            x.clone(),
                            number_literal_node(30.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    mul_number_node(
                        scale.clone(),
                        div_number_node(
                            y.clone(),
                            number_literal_node(30.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    mul_number_node(
                        scale.clone(),
                        div_number_node(
                            z.clone(),
                            number_literal_node(50.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    scale,
                    next_node,
                    span,
                ));
            }
        }
        "rossler-points" => {
            if args.len() != 3 {
                return Err(sequence_arity_error(
                    "`rossler-points`",
                    "count, dt, and scale",
                    args.len(),
                    span,
                ));
            }
            let count = parse_positive_count(&args[0], "`rossler-points` count")?;
            let mut x = number_literal_node(0.1, next_node, span);
            let mut y = number_literal_node(0.0, next_node, span);
            let mut z = number_literal_node(0.0, next_node, span);
            for _ in 0..count {
                let dx = neg_number_node(
                    add_number_node(y.clone(), z.clone(), next_node, span),
                    next_node,
                    span,
                );
                let dy = add_number_node(
                    x.clone(),
                    mul_number_node(
                        number_literal_node(0.2, next_node, span),
                        y.clone(),
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                );
                let dz = add_number_node(
                    number_literal_node(0.2, next_node, span),
                    mul_number_node(
                        z.clone(),
                        sub_number_node(
                            x.clone(),
                            number_literal_node(5.7, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                );
                x = add_number_node(
                    x,
                    mul_number_node(parse(&args[1], next_node)?, dx, next_node, span),
                    next_node,
                    span,
                );
                y = add_number_node(
                    y,
                    mul_number_node(parse(&args[1], next_node)?, dy, next_node, span),
                    next_node,
                    span,
                );
                z = add_number_node(
                    z,
                    mul_number_node(parse(&args[1], next_node)?, dz, next_node, span),
                    next_node,
                    span,
                );
                let scale = parse(&args[2], next_node)?;
                points.push(bounded_point3_node(
                    mul_number_node(
                        scale.clone(),
                        div_number_node(
                            x.clone(),
                            number_literal_node(15.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    mul_number_node(
                        scale.clone(),
                        div_number_node(
                            y.clone(),
                            number_literal_node(15.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    mul_number_node(
                        scale.clone(),
                        div_number_node(
                            z.clone(),
                            number_literal_node(30.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    scale,
                    next_node,
                    span,
                ));
            }
        }
        "logistic-bifurcation-points" => {
            if args.len() != 4 {
                return Err(sequence_arity_error(
                    "`logistic-bifurcation-points`",
                    "r-count, samples, transient, and scale",
                    args.len(),
                    span,
                ));
            }
            let r_count = parse_positive_count(&args[0], "`logistic-bifurcation-points` r-count")?;
            let samples = parse_positive_count(&args[1], "`logistic-bifurcation-points` samples")?;
            let transient =
                parse_nonnegative_count(&args[2], "`logistic-bifurcation-points` transient")?;
            for ri in 0..r_count {
                let r = if r_count == 1 {
                    number_literal_node(2.5, next_node, span)
                } else {
                    add_number_node(
                        number_literal_node(2.5, next_node, span),
                        mul_number_node(
                            number_literal_node(1.5, next_node, span),
                            div_number_node(
                                number_literal_node(ri as f64, next_node, span),
                                number_literal_node((r_count - 1) as f64, next_node, span),
                                next_node,
                                span,
                            ),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    )
                };
                let mut x = add_number_node(
                    number_literal_node(0.2, next_node, span),
                    mul_number_node(
                        number_literal_node(0.6, next_node, span),
                        call_number_node(
                            "hash01",
                            vec![
                                number_literal_node(ri as f64, next_node, span),
                                number_literal_node(samples as f64, next_node, span),
                                number_literal_node(transient as f64, next_node, span),
                            ],
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    next_node,
                    span,
                );
                for _ in 0..transient {
                    x = logistic_step_node(r.clone(), x, next_node, span);
                }
                for _ in 0..samples {
                    x = logistic_step_node(r.clone(), x, next_node, span);
                    let scale = parse(&args[3], next_node)?;
                    let x_pos = sub_number_node(
                        mul_number_node(
                            scale.clone(),
                            sub_number_node(
                                mul_number_node(
                                    number_literal_node(2.0, next_node, span),
                                    div_number_node(
                                        sub_number_node(
                                            r.clone(),
                                            number_literal_node(2.5, next_node, span),
                                            next_node,
                                            span,
                                        ),
                                        number_literal_node(1.5, next_node, span),
                                        next_node,
                                        span,
                                    ),
                                    next_node,
                                    span,
                                ),
                                number_literal_node(1.0, next_node, span),
                                next_node,
                                span,
                            ),
                            next_node,
                            span,
                        ),
                        number_literal_node(0.0, next_node, span),
                        next_node,
                        span,
                    );
                    let y_pos = mul_number_node(
                        scale.clone(),
                        sub_number_node(
                            mul_number_node(
                                number_literal_node(2.0, next_node, span),
                                x.clone(),
                                next_node,
                                span,
                            ),
                            number_literal_node(1.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    );
                    points.push(bounded_point2_node(x_pos, y_pos, scale, next_node, span));
                }
            }
        }
        "henon-points" => {
            if args.len() != 2 {
                return Err(sequence_arity_error(
                    "`henon-points`",
                    "count and scale",
                    args.len(),
                    span,
                ));
            }
            let count = parse_positive_count(&args[0], "`henon-points` count")?;
            let mut x = number_literal_node(0.1, next_node, span);
            let mut y = number_literal_node(0.0, next_node, span);
            for _ in 0..count {
                let nx = add_number_node(
                    sub_number_node(
                        number_literal_node(1.0, next_node, span),
                        mul_number_node(
                            number_literal_node(1.4, next_node, span),
                            mul_number_node(x.clone(), x.clone(), next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    y,
                    next_node,
                    span,
                );
                let ny = mul_number_node(
                    number_literal_node(0.3, next_node, span),
                    x,
                    next_node,
                    span,
                );
                x = nx;
                y = ny;
                let scale = parse(&args[1], next_node)?;
                points.push(bounded_point2_node(
                    mul_number_node(
                        scale.clone(),
                        div_number_node(
                            x.clone(),
                            number_literal_node(2.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    mul_number_node(
                        scale.clone(),
                        div_number_node(
                            y.clone(),
                            number_literal_node(2.0, next_node, span),
                            next_node,
                            span,
                        ),
                        next_node,
                        span,
                    ),
                    scale,
                    next_node,
                    span,
                ));
            }
        }
        _ => unreachable!(),
    }
    Ok((CoreNodeKind::List(points), CoreValueKind::List))
}

fn parse_positive_count(value: &ExprKind, context: &str) -> CoreResult<usize> {
    let count = parse_integer_literal(value, context)?;
    if count < 1 {
        return Err(sequence_type_mismatch_error(
            context,
            "positive integer",
            &count.to_string(),
            expr_source_span(value),
        ));
    }
    Ok(count as usize)
}

fn parse_nonnegative_count(value: &ExprKind, context: &str) -> CoreResult<usize> {
    let count = parse_integer_literal(value, context)?;
    if count < 0 {
        return Err(sequence_type_mismatch_error(
            context,
            "nonnegative integer",
            &count.to_string(),
            expr_source_span(value),
        ));
    }
    Ok(count as usize)
}

fn jitter_point_nodes(
    x: CoreNode,
    y: CoreNode,
    amount: CoreNode,
    seed: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> Vec<CoreNode> {
    let x_hash = call_number_node(
        "hash-signed",
        vec![x.clone(), y.clone(), seed.clone()],
        next_node,
        span,
    );
    let y_hash = call_number_node(
        "hash-signed",
        vec![
            add_number_node(
                x.clone(),
                number_literal_node(19.19, next_node, span),
                next_node,
                span,
            ),
            add_number_node(
                y.clone(),
                number_literal_node(7.73, next_node, span),
                next_node,
                span,
            ),
            add_number_node(
                seed,
                number_literal_node(31.0, next_node, span),
                next_node,
                span,
            ),
        ],
        next_node,
        span,
    );
    vec![
        add_number_node(
            x,
            mul_number_node(amount.clone(), x_hash, next_node, span),
            next_node,
            span,
        ),
        add_number_node(
            y,
            mul_number_node(amount, y_hash, next_node, span),
            next_node,
            span,
        ),
    ]
}

fn point2_node(
    x: CoreNode,
    y: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    core_node_with_span(
        alloc_node_id(next_node),
        CoreNodeKind::List(vec![x, y]),
        CoreValueKind::Point2,
        span,
    )
}

fn bounded_point2_node(
    x: CoreNode,
    y: CoreNode,
    scale: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    point2_node(
        clamp_to_scale_node(x, scale.clone(), next_node, span),
        clamp_to_scale_node(y, scale, next_node, span),
        next_node,
        span,
    )
}

fn point3_node(
    x: CoreNode,
    y: CoreNode,
    z: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    core_node_with_span(
        alloc_node_id(next_node),
        CoreNodeKind::List(vec![x, y, z]),
        CoreValueKind::Point3,
        span,
    )
}

fn bounded_point3_node(
    x: CoreNode,
    y: CoreNode,
    z: CoreNode,
    scale: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    point3_node(
        clamp_to_scale_node(x, scale.clone(), next_node, span),
        clamp_to_scale_node(y, scale.clone(), next_node, span),
        clamp_to_scale_node(z, scale, next_node, span),
        next_node,
        span,
    )
}

fn clamp_to_scale_node(
    value: CoreNode,
    scale: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    call_number_node(
        "clamp",
        vec![
            value,
            neg_number_node(scale.clone(), next_node, span),
            scale,
        ],
        next_node,
        span,
    )
}

fn call_number_node(
    name: &str,
    args: Vec<CoreNode>,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    core_node_with_span(
        alloc_node_id(next_node),
        CoreNodeKind::Call {
            op: CoreOperation::Custom(name.to_string()),
            args,
            keywords: Vec::new(),
        },
        CoreValueKind::Number,
        span,
    )
}

fn add_number_node(
    left: CoreNode,
    right: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    call_number_node("+", vec![left, right], next_node, span)
}

fn sub_number_node(
    left: CoreNode,
    right: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    call_number_node("-", vec![left, right], next_node, span)
}

fn neg_number_node(value: CoreNode, next_node: &mut u64, span: Option<SourceSpan>) -> CoreNode {
    call_number_node("-", vec![value], next_node, span)
}

fn mul_number_node(
    left: CoreNode,
    right: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    call_number_node("*", vec![left, right], next_node, span)
}

fn div_number_node(
    left: CoreNode,
    right: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    call_number_node("/", vec![left, right], next_node, span)
}

fn logistic_step_node(
    r: CoreNode,
    x: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    mul_number_node(
        r,
        mul_number_node(
            x.clone(),
            sub_number_node(
                number_literal_node(1.0, next_node, span),
                x,
                next_node,
                span,
            ),
            next_node,
            span,
        ),
        next_node,
        span,
    )
}

fn parse_expanded_flat_map_node(
    op_name: &str,
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if args.len() < 2 {
        return Err(sequence_arity_error(
            &format!("`{}`", op_name),
            "function and at least one list",
            args.len(),
            args.first().and_then(expr_source_span),
        ));
    }
    let mut flattened = Vec::new();
    for items in zip_sequence_sources(collect_sequence_sources(
        op_name,
        &args[1..],
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?) {
        let result = compile_sequence_callable_application(
            op_name,
            &args[0],
            items,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )?;
        flattened.extend(extract_list_items(
            result,
            &format!("`{}` result", op_name),
            next_node,
        )?);
    }
    let value_kind = infer_list_value_kind(&flattened);
    Ok((CoreNodeKind::List(flattened), value_kind))
}

fn collect_sequence_sources(
    op_name: &str,
    args: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<Vec<Vec<CoreNode>>> {
    args.iter()
        .map(|arg| {
            let node = parse_expanded_node(
                arg,
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                helper_stack,
            )?;
            extract_list_items(node, &format!("`{}` source", op_name), next_node)
        })
        .collect::<CoreResult<Vec<_>>>()
}

fn zip_sequence_sources(sources: Vec<Vec<CoreNode>>) -> Vec<Vec<CoreNode>> {
    let shortest = sources.iter().map(Vec::len).min().unwrap_or(0);
    let mut iterators = sources.into_iter().map(Vec::into_iter).collect::<Vec<_>>();
    let mut zipped = Vec::with_capacity(shortest);
    for _ in 0..shortest {
        zipped.push(
            iterators
                .iter_mut()
                .map(|items| items.next().expect("zip rows stay in bounds"))
                .collect::<Vec<_>>(),
        );
    }
    zipped
}

fn compile_sequence_callable_application(
    op_name: &str,
    callable: &ExprKind,
    args: Vec<CoreNode>,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<CoreNode> {
    match callable {
        ExprKind::LambdaFunction(lambda) => {
            if lambda.rest || lambda.kwargs {
                return Err(sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "fixed-arity function",
                    "variadic function",
                    expr_source_span(callable),
                ));
            }
            let params = lambda
                .args
                .iter()
                .map(|arg| {
                    expr_identifier(arg).ok_or_else(|| {
                        sequence_callable_kind_error(
                            &format!("`{}` lambda parameter", op_name),
                            "symbol",
                            &expr_actual_kind_label(arg),
                            expr_source_span(arg),
                        )
                    })
                })
                .collect::<CoreResult<Vec<_>>>()?;
            if params.len() != args.len() {
                return Err(sequence_callable_arity_error(
                    &format!("`{}`", op_name),
                    args.len(),
                    params.len(),
                    expr_source_span(callable),
                ));
            }
            compile_sequence_function_body(
                callable,
                params,
                &lambda.body,
                args,
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                helper_stack,
            )
        }
        func if expr_identifier(func)
            .and_then(|name| helpers.get(&name).map(|helper| (name, helper)))
            .is_some() =>
        {
            let (name, helper) = expr_identifier(func)
                .and_then(|helper_name| {
                    helpers
                        .get(&helper_name)
                        .map(|helper| (helper_name, helper))
                })
                .unwrap();
            let ExpandedHelper::Function { params, body } = helper else {
                return Err(sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "function",
                    "value",
                    expr_source_span(callable),
                ));
            };
            if helper_stack.contains(&name) {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Recursive helper function `{}` is not supported.", name),
                )
                .with_span(expr_source_span(callable).unwrap_or(SourceSpan::new(None, 0, 0))));
            }
            if params.len() != args.len() {
                return Err(sequence_callable_arity_error(
                    &format!("`{}`", op_name),
                    args.len(),
                    params.len(),
                    expr_source_span(callable),
                ));
            }
            let mut nested_stack = helper_stack.clone();
            nested_stack.insert(name);
            compile_sequence_function_body(
                callable,
                params.clone(),
                body,
                args,
                next_node,
                param_ids,
                helpers,
                node_refs,
                local_names,
                &nested_stack,
            )
        }
        ExprKind::Atom(_) => {
            let Some(name) = expr_identifier(callable) else {
                return Err(sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "function",
                    &expr_actual_kind_label(callable),
                    expr_source_span(callable),
                ));
            };
            if matches!(helpers.get(&name), Some(ExpandedHelper::Value(_))) {
                return Err(sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "function",
                    "helper value",
                    expr_source_span(callable),
                ));
            }
            if local_names.contains(&name)
                || node_refs.contains_key(&name)
                || param_ids.contains_key(&name)
            {
                return Err(sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "global function",
                    "reference",
                    expr_source_span(callable),
                ));
            }
            compile_named_sequence_application(&name, callable, args, next_node)
        }
        _ => Err(sequence_callable_kind_error(
            &format!("`{}`", op_name),
            "function",
            &expr_actual_kind_label(callable),
            expr_source_span(callable),
        )),
    }
}

fn compile_sequence_function_body(
    callable: &ExprKind,
    params: Vec<String>,
    body_expr: &ExprKind,
    args: Vec<CoreNode>,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<CoreNode> {
    let mut nested_locals = local_names.clone();
    for param in &params {
        nested_locals.insert(param.clone());
    }
    let body = parse_expanded_node(
        body_expr,
        next_node,
        param_ids,
        helpers,
        node_refs,
        &nested_locals,
        helper_stack,
    )?;
    let value_kind = body.value_kind;
    let bindings = params
        .into_iter()
        .zip(args)
        .map(|(name, value)| CoreBinding { name, value })
        .collect::<Vec<_>>();
    Ok(core_node_with_span(
        alloc_node_id(next_node),
        CoreNodeKind::Let {
            bindings,
            body: Box::new(body),
        },
        value_kind,
        expr_source_span(callable).or(expr_source_span(body_expr)),
    ))
}

fn compile_named_sequence_application(
    name: &str,
    callable: &ExprKind,
    args: Vec<CoreNode>,
    next_node: &mut u64,
) -> CoreResult<CoreNode> {
    let span = expr_source_span(callable);
    if name == "list" {
        let value_kind = infer_list_value_kind(&args);
        return Ok(core_node_with_span(
            alloc_node_id(next_node),
            CoreNodeKind::List(args),
            value_kind,
            span,
        ));
    }
    Ok(core_node_with_span(
        alloc_node_id(next_node),
        CoreNodeKind::Call {
            op: map_operation(name),
            args,
            keywords: Vec::new(),
        },
        infer_value_kind(name),
        span,
    ))
}

fn extract_list_items(
    node: CoreNode,
    context: &str,
    next_node: &mut u64,
) -> CoreResult<Vec<CoreNode>> {
    let CoreNode {
        kind,
        value_kind,
        span,
        ..
    } = node;
    match kind {
        CoreNodeKind::List(items) => Ok(items),
        CoreNodeKind::Let { bindings, body } => {
            extract_list_items(*body, context, next_node).map(|items| {
                items
                    .into_iter()
                    .map(|item| wrap_bindings_around_item(&bindings, item, next_node, span))
                    .collect()
            })
        }
        _ => Err(sequence_type_mismatch_error(
            context,
            "list",
            core_value_kind_label(value_kind),
            span,
        )),
    }
}

fn wrap_bindings_around_item(
    bindings: &[CoreBinding],
    item: CoreNode,
    next_node: &mut u64,
    span: Option<SourceSpan>,
) -> CoreNode {
    let value_kind = item.value_kind;
    let cloned_bindings = bindings
        .iter()
        .map(|binding| CoreBinding {
            name: binding.name.clone(),
            value: clone_node_with_fresh_ids(&binding.value, next_node),
        })
        .collect::<Vec<_>>();
    core_node_with_span(
        alloc_node_id(next_node),
        CoreNodeKind::Let {
            bindings: cloned_bindings,
            body: Box::new(item),
        },
        value_kind,
        span,
    )
}

fn clone_node_with_fresh_ids(node: &CoreNode, next_node: &mut u64) -> CoreNode {
    let kind = match &node.kind {
        CoreNodeKind::Literal(literal) => CoreNodeKind::Literal(literal.clone()),
        CoreNodeKind::Reference(reference) => CoreNodeKind::Reference(reference.clone()),
        CoreNodeKind::Build { bindings, result } => CoreNodeKind::Build {
            bindings: bindings
                .iter()
                .map(|binding| CoreShapeBinding {
                    name: binding.name.clone(),
                    value: clone_node_with_fresh_ids(&binding.value, next_node),
                })
                .collect(),
            result: Box::new(clone_node_with_fresh_ids(result, next_node)),
        },
        CoreNodeKind::Let { bindings, body } => CoreNodeKind::Let {
            bindings: bindings
                .iter()
                .map(|binding| CoreBinding {
                    name: binding.name.clone(),
                    value: clone_node_with_fresh_ids(&binding.value, next_node),
                })
                .collect(),
            body: Box::new(clone_node_with_fresh_ids(body, next_node)),
        },
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => CoreNodeKind::If {
            condition: Box::new(clone_node_with_fresh_ids(condition, next_node)),
            then_branch: Box::new(clone_node_with_fresh_ids(then_branch, next_node)),
            else_branch: Box::new(clone_node_with_fresh_ids(else_branch, next_node)),
        },
        CoreNodeKind::Call { op, args, keywords } => CoreNodeKind::Call {
            op: op.clone(),
            args: args
                .iter()
                .map(|arg| clone_node_with_fresh_ids(arg, next_node))
                .collect(),
            keywords: keywords
                .iter()
                .map(|keyword| match keyword.selector_payload() {
                    Some(selector) => CoreKeywordArg::selector(
                        keyword.name.clone(),
                        clone_node_with_fresh_ids(keyword.source_node(), next_node),
                        selector.clone(),
                    ),
                    None => CoreKeywordArg::expr(
                        keyword.name.clone(),
                        clone_node_with_fresh_ids(keyword.source_node(), next_node),
                    ),
                })
                .collect(),
        },
        CoreNodeKind::Range { start, end } => CoreNodeKind::Range {
            start: Box::new(clone_node_with_fresh_ids(start, next_node)),
            end: Box::new(clone_node_with_fresh_ids(end, next_node)),
        },
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => CoreNodeKind::Map {
            params: params.clone(),
            sources: sources
                .iter()
                .map(|source| clone_node_with_fresh_ids(source, next_node))
                .collect(),
            body: Box::new(clone_node_with_fresh_ids(body, next_node)),
        },
        CoreNodeKind::Apply { op, args, list } => CoreNodeKind::Apply {
            op: op.clone(),
            args: args
                .iter()
                .map(|arg| clone_node_with_fresh_ids(arg, next_node))
                .collect(),
            list: Box::new(clone_node_with_fresh_ids(list, next_node)),
        },
        CoreNodeKind::List(items) => CoreNodeKind::List(
            items
                .iter()
                .map(|item| clone_node_with_fresh_ids(item, next_node))
                .collect(),
        ),
        CoreNodeKind::Group(items) => CoreNodeKind::Group(
            items
                .iter()
                .map(|item| clone_node_with_fresh_ids(item, next_node))
                .collect(),
        ),
    };
    core_node_with_span(alloc_node_id(next_node), kind, node.value_kind, node.span)
}

fn evaluate_sequence_boolean(op_name: &str, node: &CoreNode) -> CoreResult<bool> {
    evaluate_core_boolean(op_name, node, &BTreeMap::new())
}

fn evaluate_core_boolean(
    op_name: &str,
    node: &CoreNode,
    env: &BTreeMap<String, CoreNode>,
) -> CoreResult<bool> {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Boolean(flag)) => Ok(*flag),
        CoreNodeKind::Reference(CoreReference::Local(name)) => evaluate_core_boolean(
            op_name,
            env.get(name).ok_or_else(|| {
                sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "bound local reference",
                    "unbound local",
                    node.span,
                )
            })?,
            env,
        ),
        CoreNodeKind::Let { bindings, body } => {
            let mut nested = env.clone();
            for binding in bindings {
                nested.insert(binding.name.clone(), binding.value.clone());
            }
            evaluate_core_boolean(op_name, body, &nested)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            if evaluate_core_boolean(op_name, condition, env)? {
                evaluate_core_boolean(op_name, then_branch, env)
            } else {
                evaluate_core_boolean(op_name, else_branch, env)
            }
        }
        CoreNodeKind::Call { op, args, .. } => {
            let Some(name) = core_custom_operation_name(op) else {
                return Err(sequence_type_mismatch_error(
                    &format!("`{}`", op_name),
                    "boolean",
                    &node_actual_kind_label(node),
                    node.span,
                ));
            };
            match name {
                "not" => {
                    if args.len() != 1 {
                        return Err(sequence_arity_error(
                            "`not`",
                            "one boolean",
                            args.len(),
                            node.span,
                        ));
                    }
                    Ok(!evaluate_core_boolean(op_name, &args[0], env)?)
                }
                "and" => args.iter().try_fold(true, |acc, arg| {
                    Ok(acc && evaluate_core_boolean(op_name, arg, env)?)
                }),
                "or" => args.iter().try_fold(false, |acc, arg| {
                    Ok(acc || evaluate_core_boolean(op_name, arg, env)?)
                }),
                "=" => {
                    if args.len() != 2 {
                        return Err(sequence_arity_error(
                            "`=`",
                            "two values",
                            args.len(),
                            node.span,
                        ));
                    }
                    if let (Ok(left), Ok(right)) = (
                        evaluate_core_number(op_name, &args[0], env),
                        evaluate_core_number(op_name, &args[1], env),
                    ) {
                        return Ok((left - right).abs() <= f64::EPSILON);
                    }
                    Ok(evaluate_core_stringish(op_name, &args[0], env)?
                        == evaluate_core_stringish(op_name, &args[1], env)?)
                }
                ">" => {
                    compare_core_numbers(op_name, args, env, node.span, |left, right| left > right)
                }
                ">=" => {
                    compare_core_numbers(op_name, args, env, node.span, |left, right| left >= right)
                }
                "<" => {
                    compare_core_numbers(op_name, args, env, node.span, |left, right| left < right)
                }
                "<=" => {
                    compare_core_numbers(op_name, args, env, node.span, |left, right| left <= right)
                }
                "even?" => unary_core_number_predicate(op_name, args, env, node.span, |value| {
                    value.fract().abs() <= f64::EPSILON && (value as i64) % 2 == 0
                }),
                "odd?" => unary_core_number_predicate(op_name, args, env, node.span, |value| {
                    value.fract().abs() <= f64::EPSILON && (value as i64) % 2 != 0
                }),
                "zero?" => unary_core_number_predicate(op_name, args, env, node.span, |value| {
                    value.abs() <= f64::EPSILON
                }),
                "null?" | "empty?" => {
                    if args.len() != 1 {
                        return Err(sequence_arity_error(
                            &format!("`{}`", name),
                            "one list",
                            args.len(),
                            node.span,
                        ));
                    }
                    Ok(matches!(args[0].kind, CoreNodeKind::List(ref items) if items.is_empty()))
                }
                "list?" => {
                    if args.len() != 1 {
                        return Err(sequence_arity_error(
                            "`list?`",
                            "one value",
                            args.len(),
                            node.span,
                        ));
                    }
                    Ok(matches!(args[0].kind, CoreNodeKind::List(_)))
                }
                _ => Err(sequence_type_mismatch_error(
                    &format!("`{}`", op_name),
                    "boolean",
                    &node_actual_kind_label(node),
                    node.span,
                )),
            }
        }
        _ => Err(sequence_type_mismatch_error(
            &format!("`{}`", op_name),
            "boolean",
            &node_actual_kind_label(node),
            node.span,
        )),
    }
}

fn evaluate_core_number(
    op_name: &str,
    node: &CoreNode,
    env: &BTreeMap<String, CoreNode>,
) -> CoreResult<f64> {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Number(number)) => Ok(*number),
        CoreNodeKind::Reference(CoreReference::Local(name)) => evaluate_core_number(
            op_name,
            env.get(name).ok_or_else(|| {
                sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "bound local reference",
                    "unbound local",
                    node.span,
                )
            })?,
            env,
        ),
        CoreNodeKind::Let { bindings, body } => {
            let mut nested = env.clone();
            for binding in bindings {
                nested.insert(binding.name.clone(), binding.value.clone());
            }
            evaluate_core_number(op_name, body, &nested)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            if evaluate_core_boolean(op_name, condition, env)? {
                evaluate_core_number(op_name, then_branch, env)
            } else {
                evaluate_core_number(op_name, else_branch, env)
            }
        }
        CoreNodeKind::Call { op, args, .. } => {
            let Some(name) = core_custom_operation_name(op) else {
                return Err(sequence_type_mismatch_error(
                    &format!("`{}`", op_name),
                    "number",
                    &node_actual_kind_label(node),
                    node.span,
                ));
            };
            match name {
                "+" => args.iter().try_fold(0.0, |acc, arg| {
                    Ok(acc + evaluate_core_number(op_name, arg, env)?)
                }),
                "-" => {
                    if args.is_empty() {
                        return Err(sequence_arity_error(
                            "`-`",
                            "at least one number",
                            0,
                            node.span,
                        ));
                    }
                    if args.len() == 1 {
                        Ok(-evaluate_core_number(op_name, &args[0], env)?)
                    } else {
                        let first = evaluate_core_number(op_name, &args[0], env)?;
                        args[1..].iter().try_fold(first, |acc, arg| {
                            Ok(acc - evaluate_core_number(op_name, arg, env)?)
                        })
                    }
                }
                "*" => args.iter().try_fold(1.0, |acc, arg| {
                    Ok(acc * evaluate_core_number(op_name, arg, env)?)
                }),
                "/" => {
                    if args.len() != 2 {
                        return Err(sequence_arity_error(
                            "`/`",
                            "two numbers",
                            args.len(),
                            node.span,
                        ));
                    }
                    Ok(evaluate_core_number(op_name, &args[0], env)?
                        / evaluate_core_number(op_name, &args[1], env)?)
                }
                "min" => args.iter().try_fold(f64::INFINITY, |acc, arg| {
                    Ok(acc.min(evaluate_core_number(op_name, arg, env)?))
                }),
                "max" => args.iter().try_fold(f64::NEG_INFINITY, |acc, arg| {
                    Ok(acc.max(evaluate_core_number(op_name, arg, env)?))
                }),
                "clamp" => {
                    if args.len() != 3 {
                        return Err(sequence_arity_error(
                            "`clamp`",
                            "value, min, and max",
                            args.len(),
                            node.span,
                        ));
                    }
                    let value = evaluate_core_number(op_name, &args[0], env)?;
                    let min = evaluate_core_number(op_name, &args[1], env)?;
                    let max = evaluate_core_number(op_name, &args[2], env)?;
                    Ok(value.max(min).min(max))
                }
                "lerp" => {
                    if args.len() != 3 {
                        return Err(sequence_arity_error(
                            "`lerp`",
                            "start, end, and t",
                            args.len(),
                            node.span,
                        ));
                    }
                    let start = evaluate_core_number(op_name, &args[0], env)?;
                    let end = evaluate_core_number(op_name, &args[1], env)?;
                    let t = evaluate_core_number(op_name, &args[2], env)?;
                    Ok(start + (end - start) * t)
                }
                "smoothstep" => {
                    if args.len() != 3 {
                        return Err(sequence_arity_error(
                            "`smoothstep`",
                            "edge0, edge1, and x",
                            args.len(),
                            node.span,
                        ));
                    }
                    let edge0 = evaluate_core_number(op_name, &args[0], env)?;
                    let edge1 = evaluate_core_number(op_name, &args[1], env)?;
                    let x = evaluate_core_number(op_name, &args[2], env)?;
                    if (edge0 - edge1).abs() <= f64::EPSILON {
                        return Err(sequence_type_mismatch_error(
                            "`smoothstep`",
                            "distinct edge values",
                            "equal edges",
                            node.span,
                        ));
                    }
                    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                    Ok(t * t * (3.0 - 2.0 * t))
                }
                "abs" => unary_core_number_op(op_name, args, env, node.span, f64::abs),
                "sin" => unary_core_number_op(op_name, args, env, node.span, f64::sin),
                "cos" => unary_core_number_op(op_name, args, env, node.span, f64::cos),
                "tan" => unary_core_number_op(op_name, args, env, node.span, f64::tan),
                "atan" => unary_core_number_op(op_name, args, env, node.span, f64::atan),
                "atan2" => {
                    if args.len() != 2 {
                        return Err(sequence_arity_error(
                            "`atan2`",
                            "y and x",
                            args.len(),
                            node.span,
                        ));
                    }
                    Ok(evaluate_core_number(op_name, &args[0], env)?
                        .atan2(evaluate_core_number(op_name, &args[1], env)?))
                }
                "deg" => {
                    unary_core_number_op(op_name, args, env, node.span, |value| value.to_radians())
                }
                "deg->rad" => {
                    unary_core_number_op(op_name, args, env, node.span, |value| value.to_radians())
                }
                "rad" => {
                    unary_core_number_op(op_name, args, env, node.span, |value| value.to_degrees())
                }
                "rad->deg" => {
                    unary_core_number_op(op_name, args, env, node.span, |value| value.to_degrees())
                }
                "floor" => unary_core_number_op(op_name, args, env, node.span, f64::floor),
                "signed-pow" => {
                    binary_core_number_op(op_name, args, env, node.span, |value, exp| {
                        value.signum() * value.abs().powf(exp)
                    })
                }
                "hash01" => ternary_core_number_op(
                    op_name,
                    args,
                    env,
                    node.span,
                    ecky_deterministic::hash01,
                ),
                "hash-signed" => ternary_core_number_op(
                    op_name,
                    args,
                    env,
                    node.span,
                    ecky_deterministic::hash_signed,
                ),
                "noise2" => ternary_core_number_op(
                    op_name,
                    args,
                    env,
                    node.span,
                    ecky_deterministic::noise2,
                ),
                "voronoi2" => ternary_core_number_op(
                    op_name,
                    args,
                    env,
                    node.span,
                    ecky_deterministic::voronoi2,
                ),
                "cell-distance2" => ternary_core_number_op(
                    op_name,
                    args,
                    env,
                    node.span,
                    ecky_deterministic::cell_distance2,
                ),
                "fbm2" => {
                    if args.len() != 6 {
                        return Err(sequence_arity_error(
                            "`fbm2`",
                            "x, y, seed, octaves, lacunarity, and gain",
                            args.len(),
                            node.span,
                        ));
                    }
                    Ok(ecky_deterministic::fbm2(
                        evaluate_core_number(op_name, &args[0], env)?,
                        evaluate_core_number(op_name, &args[1], env)?,
                        evaluate_core_number(op_name, &args[2], env)?,
                        evaluate_core_number(op_name, &args[3], env)?,
                        evaluate_core_number(op_name, &args[4], env)?,
                        evaluate_core_number(op_name, &args[5], env)?,
                    ))
                }
                _ => Err(sequence_type_mismatch_error(
                    &format!("`{}`", op_name),
                    "number",
                    &node_actual_kind_label(node),
                    node.span,
                )),
            }
        }
        _ => Err(sequence_type_mismatch_error(
            &format!("`{}`", op_name),
            "number",
            &node_actual_kind_label(node),
            node.span,
        )),
    }
}

fn evaluate_core_stringish(
    op_name: &str,
    node: &CoreNode,
    env: &BTreeMap<String, CoreNode>,
) -> CoreResult<String> {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(text.clone()),
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => Ok(match symbol {
            CoreSymbol::Start => "start".to_string(),
            CoreSymbol::End => "end".to_string(),
            CoreSymbol::Xy => "xy".to_string(),
            CoreSymbol::Yz => "yz".to_string(),
            CoreSymbol::Xz => "xz".to_string(),
            CoreSymbol::Min => "min".to_string(),
            CoreSymbol::Center => "center".to_string(),
            CoreSymbol::Max => "max".to_string(),
        }),
        CoreNodeKind::Reference(CoreReference::Local(name)) => evaluate_core_stringish(
            op_name,
            env.get(name).ok_or_else(|| {
                sequence_callable_kind_error(
                    &format!("`{}`", op_name),
                    "bound local reference",
                    "unbound local",
                    node.span,
                )
            })?,
            env,
        ),
        CoreNodeKind::Let { bindings, body } => {
            let mut nested = env.clone();
            for binding in bindings {
                nested.insert(binding.name.clone(), binding.value.clone());
            }
            evaluate_core_stringish(op_name, body, &nested)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            if evaluate_core_boolean(op_name, condition, env)? {
                evaluate_core_stringish(op_name, then_branch, env)
            } else {
                evaluate_core_stringish(op_name, else_branch, env)
            }
        }
        _ => Err(sequence_type_mismatch_error(
            &format!("`{}`", op_name),
            "text",
            &node_actual_kind_label(node),
            node.span,
        )),
    }
}

fn compare_core_numbers(
    op_name: &str,
    args: &[CoreNode],
    env: &BTreeMap<String, CoreNode>,
    span: Option<SourceSpan>,
    compare: impl Fn(f64, f64) -> bool,
) -> CoreResult<bool> {
    if args.len() != 2 {
        return Err(sequence_arity_error(
            &format!("`{}`", op_name),
            "two numbers",
            args.len(),
            span,
        ));
    }
    Ok(compare(
        evaluate_core_number(op_name, &args[0], env)?,
        evaluate_core_number(op_name, &args[1], env)?,
    ))
}

fn unary_core_number_op(
    op_name: &str,
    args: &[CoreNode],
    env: &BTreeMap<String, CoreNode>,
    span: Option<SourceSpan>,
    op: impl Fn(f64) -> f64,
) -> CoreResult<f64> {
    if args.len() != 1 {
        return Err(sequence_arity_error(
            &format!("`{}`", op_name),
            "one number",
            args.len(),
            span,
        ));
    }
    Ok(op(evaluate_core_number(op_name, &args[0], env)?))
}

fn binary_core_number_op(
    op_name: &str,
    args: &[CoreNode],
    env: &BTreeMap<String, CoreNode>,
    span: Option<SourceSpan>,
    op: impl Fn(f64, f64) -> f64,
) -> CoreResult<f64> {
    if args.len() != 2 {
        return Err(sequence_arity_error(
            &format!("`{}`", op_name),
            "two numbers",
            args.len(),
            span,
        ));
    }
    Ok(op(
        evaluate_core_number(op_name, &args[0], env)?,
        evaluate_core_number(op_name, &args[1], env)?,
    ))
}

fn ternary_core_number_op(
    op_name: &str,
    args: &[CoreNode],
    env: &BTreeMap<String, CoreNode>,
    span: Option<SourceSpan>,
    op: impl Fn(f64, f64, f64) -> f64,
) -> CoreResult<f64> {
    if args.len() != 3 {
        return Err(sequence_arity_error(
            &format!("`{}`", op_name),
            "three numbers",
            args.len(),
            span,
        ));
    }
    Ok(op(
        evaluate_core_number(op_name, &args[0], env)?,
        evaluate_core_number(op_name, &args[1], env)?,
        evaluate_core_number(op_name, &args[2], env)?,
    ))
}

fn unary_core_number_predicate(
    op_name: &str,
    args: &[CoreNode],
    env: &BTreeMap<String, CoreNode>,
    span: Option<SourceSpan>,
    predicate: impl Fn(f64) -> bool,
) -> CoreResult<bool> {
    if args.len() != 1 {
        return Err(sequence_arity_error(
            &format!("`{}`", op_name),
            "one number",
            args.len(),
            span,
        ));
    }
    Ok(predicate(evaluate_core_number(op_name, &args[0], env)?))
}

fn number_literal_node(value: f64, next_node: &mut u64, span: Option<SourceSpan>) -> CoreNode {
    core_node_with_span(
        alloc_node_id(next_node),
        CoreNodeKind::Literal(CoreLiteral::Number(value)),
        CoreValueKind::Number,
        span,
    )
}

fn core_custom_operation_name(op: &CoreOperation) -> Option<&str> {
    match op {
        CoreOperation::Custom(name) => Some(name.as_str()),
        _ => None,
    }
}

fn sequence_arity_error(
    subject: &str,
    expected: &str,
    actual: usize,
    span: Option<SourceSpan>,
) -> CompilerError {
    let err = CompilerError::new(
        CompilerErrorKind::Parse,
        format!("{subject} expected {expected}, got {actual} argument(s)."),
    );
    if let Some(span) = span {
        err.with_span(span)
    } else {
        err
    }
}

fn sequence_callable_arity_error(
    subject: &str,
    expected: usize,
    actual: usize,
    span: Option<SourceSpan>,
) -> CompilerError {
    let err = CompilerError::new(
        CompilerErrorKind::TypeMismatch,
        format!("{subject} expected arity {expected}, got arity {actual}."),
    );
    if let Some(span) = span {
        err.with_span(span)
    } else {
        err
    }
}

fn sequence_type_mismatch_error(
    subject: &str,
    expected: &str,
    actual: &str,
    span: Option<SourceSpan>,
) -> CompilerError {
    let err = CompilerError::new(
        CompilerErrorKind::TypeMismatch,
        format!("{subject} expected {expected}, got {actual}."),
    );
    if let Some(span) = span {
        err.with_span(span)
    } else {
        err
    }
}

fn sequence_callable_kind_error(
    subject: &str,
    expected: &str,
    actual: &str,
    span: Option<SourceSpan>,
) -> CompilerError {
    let err = CompilerError::new(
        CompilerErrorKind::UnsupportedFeature,
        format!("{subject} expected {expected}, got {actual}."),
    );
    if let Some(span) = span {
        err.with_span(span)
    } else {
        err
    }
}

fn expr_actual_kind_label(value: &ExprKind) -> String {
    match value {
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::BooleanLiteral(_) => "boolean".to_string(),
            TokenType::Number(_) => "number".to_string(),
            TokenType::StringLiteral(_) => "text".to_string(),
            TokenType::Identifier(_) | TokenType::Keyword(_) => "symbol".to_string(),
            other => format!("{:?}", other),
        },
        ExprKind::LambdaFunction(_) => "function".to_string(),
        ExprKind::List(_) | ExprKind::Vector(_) => "list".to_string(),
        ExprKind::If(_) => "conditional".to_string(),
        ExprKind::Let(_) => "let".to_string(),
        ExprKind::Begin(_) => "group".to_string(),
        ExprKind::Quote(_) => "quoted value".to_string(),
        ExprKind::Define(_) => "definition".to_string(),
        ExprKind::Return(_) => "return".to_string(),
        ExprKind::Macro(_) => "macro".to_string(),
        ExprKind::SyntaxRules(_) => "syntax-rules".to_string(),
        ExprKind::Set(_) => "set!".to_string(),
        ExprKind::Require(_) => "require".to_string(),
    }
}

fn node_actual_kind_label(node: &CoreNode) -> String {
    if node.value_kind != CoreValueKind::Any {
        return core_value_kind_label(node.value_kind).to_string();
    }
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Number(_)) => "number".to_string(),
        CoreNodeKind::Literal(CoreLiteral::Boolean(_)) => "boolean".to_string(),
        CoreNodeKind::Literal(CoreLiteral::Text(_)) => "text".to_string(),
        CoreNodeKind::Literal(CoreLiteral::Symbol(_)) => "symbol".to_string(),
        CoreNodeKind::Literal(CoreLiteral::Point2(_)) => "point2".to_string(),
        CoreNodeKind::Literal(CoreLiteral::Point3(_)) => "point3".to_string(),
        CoreNodeKind::Reference(_) => "reference".to_string(),
        CoreNodeKind::Build { .. } => "solid".to_string(),
        CoreNodeKind::Let { body, .. } => node_actual_kind_label(body),
        CoreNodeKind::If { .. } => "conditional".to_string(),
        CoreNodeKind::Call { op, .. } => core_custom_operation_name(op)
            .map(|name| format!("call `{}`", name))
            .unwrap_or_else(|| "call".to_string()),
        CoreNodeKind::Range { .. } => "list".to_string(),
        CoreNodeKind::Map { .. } => "list".to_string(),
        CoreNodeKind::Apply { op, .. } => core_custom_operation_name(op)
            .map(|name| format!("apply `{}`", name))
            .unwrap_or_else(|| "apply".to_string()),
        CoreNodeKind::List(_) => "list".to_string(),
        CoreNodeKind::Group(_) => "group".to_string(),
    }
}

fn core_value_kind_label(kind: CoreValueKind) -> &'static str {
    match kind {
        CoreValueKind::Any => "value",
        CoreValueKind::Number => "number",
        CoreValueKind::Boolean => "boolean",
        CoreValueKind::Text => "text",
        CoreValueKind::List => "list",
        CoreValueKind::Point2 => "point2",
        CoreValueKind::Point3 => "point3",
        CoreValueKind::Sketch => "sketch",
        CoreValueKind::Path => "path",
        CoreValueKind::Frame => "frame",
        CoreValueKind::Compound => "compound",
        CoreValueKind::Solid => "solid",
    }
}

fn infer_list_value_kind(items: &[CoreNode]) -> CoreValueKind {
    match items.first().map(|node| node.value_kind) {
        Some(CoreValueKind::Point2) => CoreValueKind::Point2,
        Some(CoreValueKind::Point3) => CoreValueKind::Point3,
        _ => CoreValueKind::List,
    }
}

fn parse_optional_integer_literal(value: &ExprKind, context: &str) -> CoreResult<Option<i64>> {
    if !matches!(value, ExprKind::Atom(atom) if matches!(atom.syn.ty, TokenType::Number(_))) {
        return Ok(None);
    }
    parse_integer_literal(value, context).map(Some)
}

fn parse_integer_literal(value: &ExprKind, context: &str) -> CoreResult<i64> {
    let number = expr_number_value(value, context)?;
    if number.fract().abs() > f64::EPSILON {
        return Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected an integer, received {}.", context, number),
        )
        .with_span(expr_source_span(value).unwrap_or(SourceSpan::new(None, 0, 0))));
    }
    Ok(number as i64)
}

fn parse_expanded_let_node(
    bindings_expr: &ExprKind,
    body_expr: &ExprKind,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let binding_items = expr_list_items(bindings_expr, "let bindings")?;
    let mut bindings = Vec::new();
    let mut nested_locals = local_names.clone();
    for binding in binding_items {
        let pair = expr_list_items(&binding, "let binding")?;
        if pair.len() != 2 {
            return Err(CompilerError::new(
                CompilerErrorKind::Parse,
                "Each `let` binding must be `(name expr)`.",
            ));
        }
        let name = expr_value_symbol_or_text(&pair[0], "let binding name")?;
        let value = parse_expanded_node(
            &pair[1],
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        )?;
        bindings.push(CoreBinding {
            name: name.clone(),
            value,
        });
        nested_locals.insert(name);
    }
    let body = parse_expanded_node(
        body_expr,
        next_node,
        param_ids,
        helpers,
        node_refs,
        &nested_locals,
        helper_stack,
    )?;
    let value_kind = body.value_kind;
    Ok((
        CoreNodeKind::Let {
            bindings,
            body: Box::new(body),
        },
        value_kind,
    ))
}

fn parse_expanded_let_star_node(
    bindings_expr: &ExprKind,
    body_expr: &ExprKind,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let binding_items = expr_list_items(bindings_expr, "let* bindings")?;
    let body = build_expanded_let_star_body(
        &binding_items,
        body_expr,
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    Ok((body.kind, body.value_kind))
}

fn build_expanded_let_star_body(
    bindings: &[ExprKind],
    body_expr: &ExprKind,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<CoreNode> {
    let Some((first, rest)) = bindings.split_first() else {
        return parse_expanded_node(
            body_expr,
            next_node,
            param_ids,
            helpers,
            node_refs,
            local_names,
            helper_stack,
        );
    };
    let pair = expr_list_items(first, "let* binding")?;
    if pair.len() != 2 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Each `let*` binding must be `(name expr)`.",
        ));
    }
    let name = expr_value_symbol_or_text(&pair[0], "let* binding name")?;
    let value = parse_expanded_node(
        &pair[1],
        next_node,
        param_ids,
        helpers,
        node_refs,
        local_names,
        helper_stack,
    )?;
    let mut nested_locals = local_names.clone();
    nested_locals.insert(name.clone());
    let body = build_expanded_let_star_body(
        rest,
        body_expr,
        next_node,
        param_ids,
        helpers,
        node_refs,
        &nested_locals,
        helper_stack,
    )?;
    let value_kind = body.value_kind;
    Ok(core_node_with_span(
        alloc_node_id(next_node),
        CoreNodeKind::Let {
            bindings: vec![CoreBinding { name, value }],
            body: Box::new(body),
        },
        value_kind,
        expr_source_span(body_expr),
    ))
}

fn parse_expanded_typed_hole_call(
    args: &[ExprKind],
    next_node: &mut u64,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let mut type_name = None;
    let mut goal = None;
    let mut index = 0usize;

    while index < args.len() {
        let key = normalize_keyword(&expr_name(&args[index])?);
        let value = args.get(index + 1).ok_or_else(|| {
            CompilerError::new(
                CompilerErrorKind::Parse,
                format!("Typed hole option `{}` missing value.", key),
            )
        })?;
        match key.as_str() {
            ":type" => type_name = Some(expr_value_symbol_or_text(value, "hole type")?),
            ":goal" => goal = Some(expr_value_symbol_or_text(value, "hole goal")?),
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported typed hole option `{}`.", other),
                ))
            }
        }
        index += 2;
    }

    typed_hole_call(type_name, goal, next_node)
}

fn parse_expanded_build_node(
    items: &[ExprKind],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    helpers: &ExpandedHelperMap,
    local_names: &BTreeSet<String>,
    helper_stack: &BTreeSet<String>,
) -> CoreResult<CoreNodeKind> {
    if items.len() < 2 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "`build` expects shape bindings and a result.",
        ));
    }
    let mut bindings = Vec::new();
    let mut seen = BTreeMap::new();
    let mut result = None;
    for form in items.iter().skip(1) {
        let stmt = expr_list_items(form, "build statement")?;
        let head = expr_name(stmt.first().ok_or_else(|| {
            CompilerError::new(CompilerErrorKind::Parse, "Empty build statement.")
        })?)?;
        match head.as_str() {
            "shape" => {
                if result.is_some() {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`build` cannot define shapes after `(result ...)`.",
                    ));
                }
                if stmt.len() != 3 {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`shape` expects a name and expression.",
                    ));
                }
                let name = expr_value_symbol_or_text(&stmt[1], "shape name")?;
                if seen.contains_key(&name) {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        format!("`build` cannot rebind shape `{}`.", name),
                    ));
                }
                let value = parse_expanded_node(
                    &stmt[2],
                    next_node,
                    param_ids,
                    helpers,
                    &seen,
                    local_names,
                    helper_stack,
                )?;
                seen.insert(name.clone(), value.id);
                bindings.push(CoreShapeBinding { name, value });
            }
            "result" => {
                if result.is_some() {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`build` requires exactly one `(result ...)` clause.",
                    ));
                }
                if stmt.len() != 2 {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`result` expects exactly one expression.",
                    ));
                }
                result = Some(Box::new(parse_expanded_node(
                    &stmt[1],
                    next_node,
                    param_ids,
                    helpers,
                    &seen,
                    local_names,
                    helper_stack,
                )?));
            }
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported `build` clause `{}`.", other),
                ))
            }
        }
    }
    let result = result.ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "`build` requires exactly one `(result ...)` clause.",
        )
    })?;
    Ok(CoreNodeKind::Build { bindings, result })
}

pub(crate) fn expr_list_items(value: &ExprKind, context: &str) -> CoreResult<Vec<ExprKind>> {
    match value {
        ExprKind::List(list) => Ok(list.args.iter().cloned().collect()),
        ExprKind::Vector(vector) => Ok(vector.args.iter().cloned().collect()),
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected a list, received {:?}", context, other),
        )),
    }
}

fn expr_name(value: &ExprKind) -> CoreResult<String> {
    match value {
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::Identifier(name) | TokenType::Keyword(name) => Ok(name.to_string()),
            TokenType::StringLiteral(text) => Ok(text.to_string()),
            other => Err(CompilerError::new(
                CompilerErrorKind::TypeMismatch,
                format!("Expected symbol, received {:?}", other),
            )),
        },
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("Expected symbol, received {:?}", other),
        )),
    }
}

fn normalize_hygienic_op_name(name: &str) -> String {
    name.rsplit("__%#__").next().unwrap_or(name).to_string()
}

pub(crate) fn expr_identifier(value: &ExprKind) -> Option<String> {
    match value {
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::Identifier(name) => Some(name.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn expr_value_symbol_or_text(value: &ExprKind, context: &str) -> CoreResult<String> {
    match value {
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::Identifier(name) | TokenType::Keyword(name) => Ok(name.to_string()),
            TokenType::StringLiteral(text) => Ok(text.to_string()),
            other => Err(CompilerError::new(
                CompilerErrorKind::TypeMismatch,
                format!("{} expected symbol or text, received {:?}", context, other),
            )),
        },
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected symbol or text, received {:?}", context, other),
        )),
    }
}

fn expr_number_value(value: &ExprKind, context: &str) -> CoreResult<f64> {
    match value {
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::Number(number) => {
                number.resolve().to_string().parse::<f64>().map_err(|_| {
                    CompilerError::new(
                        CompilerErrorKind::TypeMismatch,
                        format!("{} expected a number, received {:?}", context, atom.syn.ty),
                    )
                })
            }
            other => Err(CompilerError::new(
                CompilerErrorKind::TypeMismatch,
                format!("{} expected a number, received {:?}", context, other),
            )),
        },
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected a number, received {:?}", context, other),
        )),
    }
}

fn expr_bool_value(value: &ExprKind, context: &str) -> CoreResult<bool> {
    match value {
        ExprKind::Atom(atom) => match &atom.syn.ty {
            TokenType::BooleanLiteral(flag) => Ok(*flag),
            TokenType::Identifier(name) if name.to_string() == "true" => Ok(true),
            TokenType::Identifier(name) if name.to_string() == "false" => Ok(false),
            other => Err(CompilerError::new(
                CompilerErrorKind::TypeMismatch,
                format!("{} expected a boolean, received {:?}", context, other),
            )),
        },
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected a boolean, received {:?}", context, other),
        )),
    }
}

fn is_point_literal_expr(items: &[ExprKind]) -> bool {
    matches!(items.len(), 2 | 3)
        && items.iter().all(|item| {
            matches!(item, ExprKind::Atom(atom) if matches!(atom.syn.ty, TokenType::Number(_)))
        })
}

fn seed_symbol_bindings(engine: &mut steel_core::steel_vm::engine::Engine, source: &str) {
    let binding_re =
        Regex::new(r#"\((number|toggle|select|image|part|shape)\s+([A-Za-z][A-Za-z0-9_-]*)"#)
            .expect("binding regex");
    for capture in binding_re.captures_iter(source) {
        if let Some(name) = capture.get(2).map(|m| m.as_str()) {
            engine.register_value(name, SteelVal::SymbolV(name.into()));
        }
    }
}

fn compiler_error(kind: CompilerErrorKind, err: impl std::fmt::Display) -> CompilerError {
    CompilerError::new(kind, err.to_string())
}

fn core_err_to_app(err: CompilerError) -> AppError {
    match err.kind {
        CompilerErrorKind::Parse => AppError::parse(err.to_string()),
        CompilerErrorKind::Resolve | CompilerErrorKind::TypeMismatch => {
            AppError::validation(err.to_string())
        }
        CompilerErrorKind::UnsupportedFeature => {
            AppError::new(AppErrorCode::Validation, err.to_string())
        }
        CompilerErrorKind::Backend | CompilerErrorKind::Internal => {
            AppError::internal(err.to_string())
        }
    }
}

fn should_fallback_to_legacy(source: &str, err: &AppError) -> bool {
    let message = err.message.to_lowercase();
    !source.contains("(require ")
        && !source.contains("(define ")
        && !source.contains("(lambda ")
        && !source.contains("(define-syntax ")
        && !message.contains("set!")
        && !message.contains("blocked")
        && !message.contains("before steel lowering")
        && !message.contains("source budget")
}

fn parse_program(value: &SteelVal) -> CoreResult<CoreProgram> {
    let forms = list_items(value, "model root")?;
    let head = symbol_name(forms.first().ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel source returned an empty list.",
        )
    })?)?;
    if head != "model" {
        return Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            "Steel source must evaluate to `(model ...)` or bind `model-root` to one.",
        ));
    }

    let mut params = Vec::new();
    let mut pending_relations = Vec::new();
    let mut raw_parts = Vec::new();
    let mut part_spellings: BTreeMap<usize, String> = BTreeMap::new();
    let mut next_param = 1u64;
    let mut next_part = 1u64;
    let mut next_node = 1u64;
    let mut feature_decls = BTreeMap::new();
    let mut verify_clauses = Vec::new();

    for form in forms.into_iter().skip(1) {
        let items = list_items(&form, "model clause")?;
        let clause =
            symbol_name(items.first().ok_or_else(|| {
                CompilerError::new(CompilerErrorKind::Parse, "Empty model clause.")
            })?)?;
        match clause.as_str() {
            "params" => {
                let (mut parsed_params, mut parsed_relations) =
                    parse_params_clause(&items, &mut next_param)?;
                params.append(&mut parsed_params);
                pending_relations.append(&mut parsed_relations);
            }
            "verify" => verify_clauses.push(parse_verify_clause(&items)?),
            "part" | "feature" => {
                part_spellings.insert(raw_parts.len(), clause.to_string());
                raw_parts.push(items);
            }
            "meta" => {}
            "map" | "range" => return Err(model_level_sequence_form_error(&clause)),
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported top-level model clause `{}`.", other),
                ))
            }
        }
    }

    if raw_parts.is_empty() {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel model needs at least one `(part ...)` clause.",
        ));
    }

    let param_ids = params
        .iter()
        .map(|param| (param.key.clone(), param.id))
        .collect::<BTreeMap<_, _>>();
    let mut constraints = resolve_relation_constraints(&mut params, pending_relations)?;
    constraints.verify_clauses = verify_clauses;
    let mut parts = Vec::new();
    let mut part_components: BTreeMap<String, ComponentClause> = BTreeMap::new();
    for (clause_idx, part_clause) in raw_parts.iter().enumerate() {
        let clause_name =
            symbol_name(part_clause.first().ok_or_else(|| {
                CompilerError::new(CompilerErrorKind::Parse, "Empty model clause.")
            })?)?;
        let spelling = part_spellings
            .get(&clause_idx)
            .map(|s| s.as_str())
            .unwrap_or(clause_name.as_str());
        match clause_name.as_str() {
            "part" => {
                let part =
                    parse_part_decl(part_clause, &mut next_part, &mut next_node, &param_ids)?;
                part_components.insert(
                    part.key.clone(),
                    ComponentClause {
                        role: ComponentRole::Output,
                        spelling: spelling.to_string(),
                    },
                );
                parts.push(part);
            }
            "feature" => {
                let (part, decl) =
                    parse_feature_decl(part_clause, &mut next_part, &mut next_node, &param_ids)?;
                part_components.insert(
                    part.key.clone(),
                    ComponentClause {
                        role: ComponentRole::Output,
                        spelling: spelling.to_string(),
                    },
                );
                feature_decls.insert(part.key.clone(), decl);
                parts.push(part);
            }
            _ => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported top-level model clause `{}`.", clause_name),
                ))
            }
        }
    }

    if parts.is_empty() {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel model needs at least one `(part ...)` clause.",
        ));
    }

    // Component clause tagging (role + spelling) stays on the parse-layer clauses;
    // emit derives part/feature spelling from per-program feature_decls. No global state.
    let _ = part_components;

    Ok(CoreProgram::new(ProgramId::new(1), params, parts)
        .with_feature_decls(feature_decls)
        .with_constraints(constraints))
}

fn parse_verify_clause(items: &[SteelVal]) -> CoreResult<CoreVerifyClause> {
    if items.len() != 4 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Verify clause expects `(tag ...)`, `(metric ...)`, and `(expect ...)`.",
        ));
    }

    Ok(CoreVerifyClause {
        tag: parse_verify_section(&items[1], "tag")?,
        metric: parse_verify_section(&items[2], "metric")?,
        expect: parse_verify_section(&items[3], "expect")?,
    })
}

fn parse_verify_section(value: &SteelVal, expected: &str) -> CoreResult<CoreVerifySection> {
    let items = list_items(value, "verify section")?;
    let name = symbol_name(items.first().ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            format!("Verify `{expected}` section is empty."),
        )
    })?)?;
    if name != expected {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            format!("Verify clause expected `({expected} ...)`, found `({name} ...)`."),
        ));
    }

    Ok(CoreVerifySection {
        items: items[1..]
            .iter()
            .map(parse_verify_value)
            .collect::<CoreResult<Vec<_>>>()?,
    })
}

fn parse_verify_value(value: &SteelVal) -> CoreResult<CoreVerifyValue> {
    match value {
        SteelVal::SymbolV(symbol) => Ok(CoreVerifyValue::Symbol(symbol.to_string())),
        SteelVal::StringV(text) => Ok(CoreVerifyValue::Text(text.to_string())),
        SteelVal::IntV(number) => Ok(CoreVerifyValue::Number(*number as f64)),
        SteelVal::NumV(number) => Ok(CoreVerifyValue::Number(*number)),
        SteelVal::BoolV(flag) => Ok(CoreVerifyValue::Boolean(*flag)),
        SteelVal::ListV(_) | SteelVal::VectorV(_) => Ok(CoreVerifyValue::List(
            list_items(value, "verify list")?
                .iter()
                .map(parse_verify_value)
                .collect::<CoreResult<Vec<_>>>()?,
        )),
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!(
                "verify value expected literal or list, received {:?}",
                other
            ),
        )),
    }
}

fn parse_params_clause(
    items: &[SteelVal],
    next_param: &mut u64,
) -> CoreResult<(Vec<CoreParameter>, Vec<PendingRelationConstraint>)> {
    let mut params = Vec::new();
    let mut relations = Vec::new();
    let mut index = 1usize;
    while index < items.len() {
        let clause_name = symbol_name(&items[index])
            .ok()
            .map(|name| normalize_keyword(&name));
        if clause_name.as_deref() == Some(":relations") {
            let relation_values = list_items(
                items.get(index + 1).ok_or_else(|| {
                    CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`:relations` missing value in params clause.",
                    )
                })?,
                "param relations",
            )?;
            for relation in relation_values {
                relations.push(parse_relation_constraint(&relation)?);
            }
            index += 2;
            continue;
        }

        let decl = items.get(index).ok_or_else(|| {
            CompilerError::new(
                CompilerErrorKind::Parse,
                "Param declaration missing in params clause.",
            )
        })?;
        params.push(parse_param_decl(decl, next_param)?);
        index += 1;
    }
    Ok((params, relations))
}

fn parse_relation_constraint(value: &SteelVal) -> CoreResult<PendingRelationConstraint> {
    let items = list_items(value, "relation constraint")?;
    if items.len() != 3 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Relation constraint must be `(< a b)`, `(<= a b)`, `(> a b)`, or `(>= a b)`.",
        ));
    }
    let operator = parse_relation_operator_from_symbol(&symbol_name(&items[0]).map_err(|_| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "Relation operator must be one of `<`, `<=`, `>`, `>=`.",
        )
    })?)?;
    Ok(PendingRelationConstraint {
        operator,
        left: parse_relation_operand(&items[1])?,
        right: parse_relation_operand(&items[2])?,
    })
}

fn parse_relation_operand(value: &SteelVal) -> CoreResult<PendingRelationOperand> {
    if matches!(value, SteelVal::IntV(_) | SteelVal::NumV(_)) {
        return Ok(PendingRelationOperand::Number(number_value(
            value,
            "relation operand",
        )?));
    }
    Ok(PendingRelationOperand::ParameterKey(value_symbol_or_text(
        value,
        "relation operand",
    )?))
}

fn parse_param_decl(value: &SteelVal, next_param: &mut u64) -> CoreResult<CoreParameter> {
    let items = list_items(value, "param declaration")?;
    let kind_name = symbol_name(items.first().ok_or_else(|| {
        CompilerError::new(CompilerErrorKind::Parse, "Empty param declaration.")
    })?)?;
    if items.len() < 3 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            format!(
                "Param declaration `{}` needs a key and default value.",
                kind_name
            ),
        ));
    }
    let key = value_symbol_or_text(&items[1], "param key")?;
    let default_value = match kind_name.as_str() {
        "number" => CoreParameterValue::Number(number_value(&items[2], "number default")?),
        "toggle" => CoreParameterValue::Boolean(bool_value(&items[2], "toggle default")?),
        "select" => CoreParameterValue::Choice(value_symbol_or_text(&items[2], "select default")?),
        "image" => CoreParameterValue::Image(value_symbol_or_text(&items[2], "image default")?),
        other => {
            return Err(CompilerError::new(
                CompilerErrorKind::UnsupportedFeature,
                format!("Unsupported param kind `{}`.", other),
            ))
        }
    };

    let mut constraints = CoreParameterConstraints::default();
    let mut label = humanize(&key);
    let mut frozen = false;

    let mut index = 3usize;
    while index < items.len() {
        let key_name = normalize_keyword(&symbol_name(&items[index])?);
        match key_name.as_str() {
            ":label" => {
                label = value_symbol_or_text(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:label` missing value.")
                    })?,
                    "param label",
                )?;
                index += 2;
            }
            ":min" => {
                constraints.min = Some(number_value(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:min` missing value.")
                    })?,
                    "param min",
                )?);
                index += 2;
            }
            ":max" => {
                constraints.max = Some(number_value(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:max` missing value.")
                    })?,
                    "param max",
                )?);
                index += 2;
            }
            ":step" => {
                constraints.step = Some(number_value(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:step` missing value.")
                    })?,
                    "param step",
                )?);
                index += 2;
            }
            ":unit" => {
                constraints.unit = Some(parse_param_unit(items.get(index + 1).ok_or_else(
                    || CompilerError::new(CompilerErrorKind::Parse, "`:unit` missing value."),
                )?)?);
                index += 2;
            }
            ":frozen" => {
                frozen = bool_value(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:frozen` missing value.")
                    })?,
                    "param frozen",
                )?;
                index += 2;
            }
            ":options" => {
                let option_values = list_items(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(CompilerErrorKind::Parse, "`:options` missing value.")
                    })?,
                    "select options",
                )?;
                constraints.choices = option_values
                    .iter()
                    .map(parse_choice)
                    .collect::<CoreResult<Vec<_>>>()?;
                index += 2;
            }
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported param option `{}`.", other),
                ))
            }
        }
    }

    let kind = match kind_name.as_str() {
        "number" => CoreParameterKind::Number,
        "toggle" => CoreParameterKind::Boolean,
        "select" => CoreParameterKind::Choice,
        "image" => CoreParameterKind::Image,
        _ => unreachable!(),
    };

    let param = CoreParameter {
        id: ParamId::new(*next_param),
        key,
        label,
        kind,
        default_value,
        frozen,
        constraints,
    };
    *next_param += 1;
    Ok(param)
}

fn parse_param_unit(value: &SteelVal) -> CoreResult<String> {
    parse_param_unit_name(value_symbol_or_text(value, "param unit")?)
}

fn parse_param_unit_name(unit: String) -> CoreResult<String> {
    match unit.as_str() {
        "length" | "angle" | "ratio" | "count" | "text" => Ok(unit),
        other => Err(CompilerError::new(
            CompilerErrorKind::UnsupportedFeature,
            format!("Unsupported param unit `{}`.", other),
        )),
    }
}

fn parse_choice(value: &SteelVal) -> CoreResult<crate::ecky_core_ir::CoreChoice> {
    let items = list_items(value, "select option")?;
    if items.len() != 2 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Select options must be `(label value)` pairs.",
        ));
    }
    Ok(crate::ecky_core_ir::CoreChoice {
        label: value_symbol_or_text(&items[0], "option label")?,
        value: match &items[1] {
            SteelVal::IntV(_) | SteelVal::NumV(_) => {
                CoreParameterValue::Number(number_value(&items[1], "option number")?)
            }
            _ => CoreParameterValue::Choice(value_symbol_or_text(&items[1], "option value")?),
        },
    })
}

fn parse_part_decl(
    items: &[SteelVal],
    next_part: &mut u64,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
) -> CoreResult<CorePart> {
    if items.len() < 3 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Each `(part ...)` needs an id and expression.",
        ));
    }
    let key = value_symbol_or_text(&items[1], "part id")?;
    let (label, expr_value) =
        if items.len() >= 4 && matches!(&items[2], SteelVal::StringV(_) | SteelVal::SymbolV(_)) {
            (value_symbol_or_text(&items[2], "part label")?, &items[3])
        } else {
            (humanize(&key), &items[2])
        };
    let root = parse_node(
        expr_value,
        next_node,
        param_ids,
        &BTreeMap::new(),
        &BTreeSet::new(),
    )?;
    let part = CorePart {
        id: PartId::new(*next_part),
        key,
        label,
        root,
    };
    *next_part += 1;
    Ok(part)
}

fn parse_feature_decl(
    items: &[SteelVal],
    next_part: &mut u64,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
) -> CoreResult<(CorePart, CoreFeatureDecl)> {
    if items.len() < 5 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Each `(feature ...)` needs an id, `:role`, and expression body.",
        ));
    }

    let key = value_symbol_or_text(&items[1], "feature id")?;
    let mut role = None;
    let mut param_keys = Vec::new();
    let mut body = None;
    let mut index = 2usize;

    while index < items.len() {
        let keyword = symbol_name(&items[index])
            .ok()
            .map(|name| normalize_keyword(&name));
        match keyword.as_deref() {
            Some(":role") => {
                role = Some(value_symbol_or_text(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(
                            CompilerErrorKind::Parse,
                            "`feature :role` missing value.",
                        )
                    })?,
                    "feature role",
                )?);
                index += 2;
            }
            Some(":params") => {
                let values = list_items(
                    items.get(index + 1).ok_or_else(|| {
                        CompilerError::new(
                            CompilerErrorKind::Parse,
                            "`feature :params` missing value list.",
                        )
                    })?,
                    "feature params",
                )?;
                param_keys = values
                    .iter()
                    .map(|value| value_symbol_or_text(value, "feature param key"))
                    .collect::<CoreResult<Vec<_>>>()?;
                index += 2;
            }
            _ => {
                if index != items.len() - 1 {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "Feature clause expects a single trailing body expression.",
                    ));
                }
                body = Some(&items[index]);
                index += 1;
            }
        }
    }

    let role = role.ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "Feature clause requires `:role` metadata.",
        )
    })?;
    let body = body.ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "Feature clause requires a body expression.",
        )
    })?;
    let root = parse_node(
        body,
        next_node,
        param_ids,
        &BTreeMap::new(),
        &BTreeSet::new(),
    )?;
    let part = CorePart {
        id: PartId::new(*next_part),
        key: key.clone(),
        label: humanize(&key),
        root,
    };
    *next_part += 1;
    Ok((
        part,
        CoreFeatureDecl {
            feature_id: key,
            role,
            param_keys,
        },
    ))
}

fn parse_node(
    value: &SteelVal,
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    node_refs: &BTreeMap<String, NodeId>,
    local_names: &BTreeSet<String>,
) -> CoreResult<CoreNode> {
    let id = {
        let current = *next_node;
        *next_node += 1;
        NodeId::new(current)
    };

    let (kind, value_kind) = match value {
        SteelVal::BoolV(flag) => (
            CoreNodeKind::Literal(CoreLiteral::Boolean(*flag)),
            CoreValueKind::Boolean,
        ),
        SteelVal::IntV(_) | SteelVal::NumV(_) => (
            CoreNodeKind::Literal(CoreLiteral::Number(number_value(value, "number literal")?)),
            CoreValueKind::Number,
        ),
        SteelVal::StringV(text) => (
            CoreNodeKind::Literal(CoreLiteral::Text(text.to_string())),
            CoreValueKind::Text,
        ),
        SteelVal::SymbolV(symbol) => match symbol.to_string().as_str() {
            "start" => (
                CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Start)),
                CoreValueKind::Any,
            ),
            "end" => (
                CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::End)),
                CoreValueKind::Any,
            ),
            "xy" => (
                CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Xy)),
                CoreValueKind::Any,
            ),
            "yz" => (
                CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Yz)),
                CoreValueKind::Any,
            ),
            "xz" => (
                CoreNodeKind::Literal(CoreLiteral::Symbol(CoreSymbol::Xz)),
                CoreValueKind::Any,
            ),
            name if local_names.contains(name) => (
                CoreNodeKind::Reference(CoreReference::Local(name.to_string())),
                CoreValueKind::Any,
            ),
            name if node_refs.contains_key(name) => (
                CoreNodeKind::Reference(CoreReference::Node(*node_refs.get(name).unwrap())),
                CoreValueKind::Any,
            ),
            name if param_ids.contains_key(name) => (
                CoreNodeKind::Reference(CoreReference::Parameter(*param_ids.get(name).unwrap())),
                CoreValueKind::Any,
            ),
            name => (
                CoreNodeKind::Reference(CoreReference::Local(name.to_string())),
                CoreValueKind::Any,
            ),
        },
        SteelVal::ListV(_) | SteelVal::VectorV(_) => {
            let items = list_items(value, "node expression")?;
            if is_point_literal(&items) {
                match items.len() {
                    2 => (
                        CoreNodeKind::Literal(CoreLiteral::Point2([
                            number_value(&items[0], "point x")?,
                            number_value(&items[1], "point y")?,
                        ])),
                        CoreValueKind::Point2,
                    ),
                    3 => (
                        CoreNodeKind::Literal(CoreLiteral::Point3([
                            number_value(&items[0], "point x")?,
                            number_value(&items[1], "point y")?,
                            number_value(&items[2], "point z")?,
                        ])),
                        CoreValueKind::Point3,
                    ),
                    _ => unreachable!(),
                }
            } else if let Some(head) = items.first() {
                if let Ok(op_name) = symbol_name(head) {
                    if normalize_keyword(&op_name).starts_with(':') {
                        (
                            CoreNodeKind::List(
                                items
                                    .iter()
                                    .map(|item| {
                                        parse_node(
                                            item,
                                            next_node,
                                            param_ids,
                                            node_refs,
                                            local_names,
                                        )
                                    })
                                    .collect::<CoreResult<Vec<_>>>()?,
                            ),
                            CoreValueKind::List,
                        )
                    } else if op_name == "build" {
                        let build = parse_build_node(&items, next_node, param_ids, local_names)?;
                        (build, CoreValueKind::Solid)
                    } else if op_name == "hole" {
                        parse_typed_hole_call(&items[1..], next_node)?
                    } else if op_name == "verify" {
                        return Err(CompilerError::new(
                            CompilerErrorKind::UnsupportedFeature,
                            "`verify` is top-level only in this slice.",
                        ));
                    } else if op_name == "if" && items.len() == 4 {
                        (
                            CoreNodeKind::If {
                                condition: Box::new(parse_node(
                                    &items[1],
                                    next_node,
                                    param_ids,
                                    node_refs,
                                    local_names,
                                )?),
                                then_branch: Box::new(parse_node(
                                    &items[2],
                                    next_node,
                                    param_ids,
                                    node_refs,
                                    local_names,
                                )?),
                                else_branch: Box::new(parse_node(
                                    &items[3],
                                    next_node,
                                    param_ids,
                                    node_refs,
                                    local_names,
                                )?),
                            },
                            CoreValueKind::Any,
                        )
                    } else {
                        let mut args = Vec::new();
                        let mut keywords = Vec::new();
                        let mut index = 1usize;
                        let mut body_locals = local_names.clone();
                        if matches!(
                            op_name.as_str(),
                            "repeat" | "repeat-union" | "repeat-compound" | "repeat-pick"
                        ) {
                            if let Some(index_symbol) = items.get(1).and_then(|node| match node {
                                SteelVal::SymbolV(symbol) => Some(symbol.to_string()),
                                _ => None,
                            }) {
                                body_locals.insert(index_symbol);
                            }
                        }
                        while index < items.len() {
                            if let Ok(name) = symbol_name(&items[index]) {
                                let normalized = normalize_keyword(&name);
                                if normalized.starts_with(':') && index + 1 < items.len() {
                                    let value = parse_node(
                                        &items[index + 1],
                                        next_node,
                                        param_ids,
                                        node_refs,
                                        &body_locals,
                                    )?;
                                    let keyword_name =
                                        normalized.trim_start_matches(':').to_string();
                                    keywords.push(
                                        selector_payload_for_keyword(&keyword_name, &value)
                                            .map(|selector| {
                                                CoreKeywordArg::selector(
                                                    keyword_name.clone(),
                                                    value.clone(),
                                                    selector,
                                                )
                                            })
                                            .unwrap_or_else(|| {
                                                CoreKeywordArg::expr(keyword_name.clone(), value)
                                            }),
                                    );
                                    index += 2;
                                    continue;
                                }
                            }
                            args.push(parse_node(
                                &items[index],
                                next_node,
                                param_ids,
                                node_refs,
                                &body_locals,
                            )?);
                            index += 1;
                        }
                        if op_name == "ring" {
                            parse_ring_alias_call(args, keywords, next_node)?
                        } else {
                            (
                                CoreNodeKind::Call {
                                    op: map_operation(&op_name),
                                    args,
                                    keywords,
                                },
                                infer_value_kind(&op_name),
                            )
                        }
                    }
                } else {
                    (
                        CoreNodeKind::List(
                            items
                                .iter()
                                .map(|item| {
                                    parse_node(item, next_node, param_ids, node_refs, local_names)
                                })
                                .collect::<CoreResult<Vec<_>>>()?,
                        ),
                        CoreValueKind::List,
                    )
                }
            } else {
                (CoreNodeKind::List(Vec::new()), CoreValueKind::List)
            }
        }
        other => {
            return Err(CompilerError::new(
                CompilerErrorKind::UnsupportedFeature,
                format!("Unsupported Steel value in CAD compiler: {:?}", other),
            ))
        }
    };

    Ok(CoreNode::new(id, kind, value_kind))
}

fn parse_typed_hole_call(
    args: &[SteelVal],
    next_node: &mut u64,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let mut type_name = None;
    let mut goal = None;
    let mut index = 0usize;

    while index < args.len() {
        let key = normalize_keyword(&symbol_name(&args[index])?);
        let value = args.get(index + 1).ok_or_else(|| {
            CompilerError::new(
                CompilerErrorKind::Parse,
                format!("Typed hole option `{}` missing value.", key),
            )
        })?;
        match key.as_str() {
            ":type" => type_name = Some(value_symbol_or_text(value, "hole type")?),
            ":goal" => goal = Some(value_symbol_or_text(value, "hole goal")?),
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported typed hole option `{}`.", other),
                ))
            }
        }
        index += 2;
    }

    typed_hole_call(type_name, goal, next_node)
}

fn parse_ring_alias_call(
    args: Vec<CoreNode>,
    keywords: Vec<CoreKeywordArg>,
    next_node: &mut u64,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    if !keywords.is_empty() {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "`ring` does not accept keyword arguments.",
        ));
    }
    if !(args.len() == 2 || args.len() == 3) {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "`ring` expects outer-radius inner-radius and optional segments.",
        ));
    }

    let mut outer_args = vec![args[0].clone()];
    let mut inner_args = vec![args[1].clone()];
    if let Some(segments) = args.get(2) {
        outer_args.push(segments.clone());
        inner_args.push(segments.clone());
    }

    let outer = CoreNode::new(
        alloc_node_id(next_node),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Circle),
            args: outer_args,
            keywords: Vec::new(),
        },
        CoreValueKind::Sketch,
    );
    let inner = CoreNode::new(
        alloc_node_id(next_node),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Circle),
            args: inner_args,
            keywords: Vec::new(),
        },
        CoreValueKind::Sketch,
    );

    Ok((
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Profile),
            args: Vec::new(),
            keywords: vec![
                CoreKeywordArg::expr("outer".to_string(), outer),
                CoreKeywordArg::expr("holes".to_string(), inner),
            ],
        },
        CoreValueKind::Sketch,
    ))
}

fn typed_hole_call(
    type_name: Option<String>,
    goal: Option<String>,
    next_node: &mut u64,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let type_name = type_name.ok_or_else(|| {
        CompilerError::new(CompilerErrorKind::Parse, "Typed hole requires `:type`.")
    })?;
    let value_kind = typed_hole_value_kind(&type_name)?;
    let mut keywords = vec![CoreKeywordArg::expr(
        "type".to_string(),
        CoreNode::new(
            alloc_node_id(next_node),
            CoreNodeKind::Literal(CoreLiteral::Text(type_name)),
            CoreValueKind::Text,
        ),
    )];
    if let Some(goal) = goal {
        keywords.push(CoreKeywordArg::expr(
            "goal".to_string(),
            CoreNode::new(
                alloc_node_id(next_node),
                CoreNodeKind::Literal(CoreLiteral::Text(goal)),
                CoreValueKind::Text,
            ),
        ));
    }
    Ok((
        CoreNodeKind::Call {
            op: CoreOperation::Custom("hole".to_string()),
            args: vec![],
            keywords,
        },
        value_kind,
    ))
}

fn typed_hole_value_kind(type_name: &str) -> CoreResult<CoreValueKind> {
    match type_name.to_ascii_lowercase().as_str() {
        "solid" => Ok(CoreValueKind::Solid),
        "sketch" => Ok(CoreValueKind::Sketch),
        "path" => Ok(CoreValueKind::Path),
        "shape" => Ok(CoreValueKind::Solid),
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!(
                "Typed hole `:type` expected solid, sketch, path, or shape; got `{}`.",
                other
            ),
        )),
    }
}

fn parse_build_node(
    items: &[SteelVal],
    next_node: &mut u64,
    param_ids: &BTreeMap<String, ParamId>,
    local_names: &BTreeSet<String>,
) -> CoreResult<CoreNodeKind> {
    if items.len() < 2 {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "`build` expects shape bindings and a result.",
        ));
    }
    let mut bindings = Vec::new();
    let mut seen = BTreeMap::new();
    let mut result = None;
    for form in items.iter().skip(1) {
        let stmt = list_items(form, "build statement")?;
        let head = symbol_name(stmt.first().ok_or_else(|| {
            CompilerError::new(CompilerErrorKind::Parse, "Empty build statement.")
        })?)?;
        match head.as_str() {
            "shape" => {
                if result.is_some() {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`build` cannot define shapes after `(result ...)`.",
                    ));
                }
                if stmt.len() != 3 {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`shape` expects a name and expression.",
                    ));
                }
                let name = value_symbol_or_text(&stmt[1], "shape name")?;
                if seen.contains_key(&name) {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        format!("`build` cannot rebind shape `{}`.", name),
                    ));
                }
                let value = parse_node(&stmt[2], next_node, param_ids, &seen, local_names)?;
                seen.insert(name.clone(), value.id);
                bindings.push(CoreShapeBinding { name, value });
            }
            "result" => {
                if result.is_some() {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`build` requires exactly one `(result ...)` clause.",
                    ));
                }
                if stmt.len() != 2 {
                    return Err(CompilerError::new(
                        CompilerErrorKind::Parse,
                        "`result` expects exactly one expression.",
                    ));
                }
                result = Some(Box::new(parse_node(
                    &stmt[1],
                    next_node,
                    param_ids,
                    &seen,
                    local_names,
                )?));
            }
            other => {
                return Err(CompilerError::new(
                    CompilerErrorKind::UnsupportedFeature,
                    format!("Unsupported `build` clause `{}`.", other),
                ))
            }
        }
    }
    let result = result.ok_or_else(|| {
        CompilerError::new(
            CompilerErrorKind::Parse,
            "`build` requires exactly one `(result ...)` clause.",
        )
    })?;
    Ok(CoreNodeKind::Build { bindings, result })
}

fn list_items(value: &SteelVal, context: &str) -> CoreResult<Vec<SteelVal>> {
    match value {
        SteelVal::ListV(list) => Ok(list.iter().cloned().collect()),
        SteelVal::VectorV(vector) => Ok(vector.iter().cloned().collect()),
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected a list, received {:?}", context, other),
        )),
    }
}

fn symbol_name(value: &SteelVal) -> CoreResult<String> {
    match value {
        SteelVal::SymbolV(symbol) => Ok(symbol.to_string()),
        SteelVal::StringV(text) => Ok(text.to_string()),
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("Expected symbol, received {:?}", other),
        )),
    }
}

fn value_symbol_or_text(value: &SteelVal, context: &str) -> CoreResult<String> {
    match value {
        SteelVal::SymbolV(symbol) => Ok(symbol.to_string()),
        SteelVal::StringV(text) => Ok(text.to_string()),
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected symbol or text, received {:?}", context, other),
        )),
    }
}

fn number_value(value: &SteelVal, context: &str) -> CoreResult<f64> {
    match value {
        SteelVal::IntV(n) => Ok(*n as f64),
        SteelVal::NumV(n) => Ok(*n),
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected a number, received {:?}", context, other),
        )),
    }
}

fn bool_value(value: &SteelVal, context: &str) -> CoreResult<bool> {
    match value {
        SteelVal::BoolV(flag) => Ok(*flag),
        other => Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!("{} expected a boolean, received {:?}", context, other),
        )),
    }
}

fn is_point_literal(items: &[SteelVal]) -> bool {
    matches!(items.len(), 2 | 3)
        && items
            .iter()
            .all(|item| matches!(item, SteelVal::IntV(_) | SteelVal::NumV(_)))
}

fn map_operation(name: &str) -> CoreOperation {
    match name {
        "box" => CoreOperation::Primitive(CorePrimitive::Box),
        "sphere" => CoreOperation::Primitive(CorePrimitive::Sphere),
        "cylinder" => CoreOperation::Primitive(CorePrimitive::Cylinder),
        "cone" => CoreOperation::Primitive(CorePrimitive::Cone),
        "circle" => CoreOperation::Primitive(CorePrimitive::Circle),
        "rectangle" => CoreOperation::Primitive(CorePrimitive::Rectangle),
        "rounded-rect" | "rounded_rect" => {
            CoreOperation::Primitive(CorePrimitive::RoundedRectangle)
        }
        "rounded-polygon" | "rounded_polygon" => {
            CoreOperation::Primitive(CorePrimitive::RoundedPolygon)
        }
        "polygon" => CoreOperation::Primitive(CorePrimitive::Polygon),
        "profile" => CoreOperation::Primitive(CorePrimitive::Profile),
        "make-face" => CoreOperation::Primitive(CorePrimitive::MakeFace),
        "text" => CoreOperation::Primitive(CorePrimitive::Text),
        "svg" => CoreOperation::Primitive(CorePrimitive::Svg),
        "import-stl" => CoreOperation::Primitive(CorePrimitive::Stl),
        "union" | "fuse" => CoreOperation::Boolean(CoreBooleanOp::Union),
        "difference" | "cut" => CoreOperation::Boolean(CoreBooleanOp::Difference),
        "intersection" | "common" => CoreOperation::Boolean(CoreBooleanOp::Intersection),
        "xor" => CoreOperation::Boolean(CoreBooleanOp::Xor),
        "translate" => CoreOperation::Transform(CoreTransformOp::Translate),
        "rotate" => CoreOperation::Transform(CoreTransformOp::Rotate),
        "scale" => CoreOperation::Transform(CoreTransformOp::Scale),
        "mirror" => CoreOperation::Transform(CoreTransformOp::Mirror),
        "extrude" => CoreOperation::Surface(CoreSurfaceOp::Extrude),
        "revolve" => CoreOperation::Surface(CoreSurfaceOp::Revolve),
        "loft" => CoreOperation::Surface(CoreSurfaceOp::Loft),
        "sweep" => CoreOperation::Surface(CoreSurfaceOp::Sweep),
        "shell" => CoreOperation::Surface(CoreSurfaceOp::Shell),
        "offset" => CoreOperation::Surface(CoreSurfaceOp::Offset),
        "offset-rounded" | "offset_rounded" => CoreOperation::Surface(CoreSurfaceOp::OffsetRounded),
        "fillet" => CoreOperation::Surface(CoreSurfaceOp::Fillet),
        "chamfer" => CoreOperation::Surface(CoreSurfaceOp::Chamfer),
        "taper" => CoreOperation::Surface(CoreSurfaceOp::Taper),
        "twist" => CoreOperation::Surface(CoreSurfaceOp::Twist),
        "polyline" | "path" => CoreOperation::Path(CorePathOp::Polyline),
        "bezier-path" => CoreOperation::Path(CorePathOp::BezierPath),
        "bspline" => CoreOperation::Path(CorePathOp::Bspline),
        "linear-array" => CoreOperation::Array(CoreArrayOp::LinearArray),
        "radial-array" => CoreOperation::Array(CoreArrayOp::RadialArray),
        "grid-array" => CoreOperation::Array(CoreArrayOp::GridArray),
        "arc-array" => CoreOperation::Array(CoreArrayOp::ArcArray),
        "repeat" => CoreOperation::Array(CoreArrayOp::Repeat),
        "repeat-union" => CoreOperation::Array(CoreArrayOp::RepeatUnion),
        "repeat-compound" => CoreOperation::Array(CoreArrayOp::RepeatCompound),
        "repeat-pick" => CoreOperation::Array(CoreArrayOp::RepeatPick),
        "plane" => CoreOperation::Frame(CoreFrameOp::Plane),
        "location" => CoreOperation::Frame(CoreFrameOp::Location),
        "path-frame" => CoreOperation::Frame(CoreFrameOp::PathFrame),
        "place" => CoreOperation::Frame(CoreFrameOp::Place),
        "clip-box" => CoreOperation::Frame(CoreFrameOp::ClipBox),
        "compound" => CoreOperation::Meta(CoreMetaOp::Group),
        _ => CoreOperation::Custom(name.to_string()),
    }
}

fn infer_value_kind(name: &str) -> CoreValueKind {
    match name {
        "+" | "-" | "*" | "/" | "min" | "max" | "clamp" | "lerp" | "smoothstep" | "sin" | "cos"
        | "tan" | "atan" | "atan2" | "deg" | "rad" | "deg->rad" | "rad->deg" | "abs" | "floor"
        | "signed-pow" | "hash01" | "hash-signed" | "noise2" | "fbm2" | "voronoi2"
        | "cell-distance2" => CoreValueKind::Number,
        "not" | "and" | "or" | "=" | ">" | ">=" | "<" | "<=" | "even?" | "odd?" | "zero?"
        | "null?" | "empty?" | "list?" => CoreValueKind::Boolean,
        "list"
        | "append"
        | "reverse"
        | "range"
        | "map"
        | "filter"
        | "zip"
        | "enumerate"
        | "linspace"
        | "flat-map"
        | "concat-map"
        | "flat_map"
        | "concat_map"
        | "jittered-grid"
        | "polar-points"
        | "organic-loop"
        | "wave-loop"
        | "voronoi-cells"
        | "lorenz-points"
        | "rossler-points"
        | "logistic-bifurcation-points"
        | "henon-points" => CoreValueKind::List,
        "jitter2" | "superellipse-point" => CoreValueKind::Point2,
        "circle" | "ring" | "rectangle" | "rounded-rect" | "rounded_rect" | "rounded-polygon"
        | "rounded_polygon" | "polygon" | "profile" | "make-face" | "text" | "svg" | "offset"
        | "offset-rounded" => CoreValueKind::Sketch,
        "bezier-path" | "path" | "polyline" => CoreValueKind::Path,
        "bspline" => CoreValueKind::Sketch,
        "plane" | "location" | "path-frame" => CoreValueKind::Frame,
        "compound" | "repeat-compound" => CoreValueKind::Compound,
        _ => CoreValueKind::Solid,
    }
}

fn is_apply_splice_operation(name: &str) -> bool {
    matches!(
        name,
        "union" | "fuse" | "difference" | "cut" | "intersection" | "common" | "compound"
    )
}

fn emit_program(program: &CoreProgram) -> String {
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut out = String::from("(model");
    if !program.parameters.is_empty() || !program.constraints.relations.is_empty() {
        out.push_str("\n  (params");
        for param in &program.parameters {
            out.push_str("\n    ");
            out.push_str(&emit_param(param));
        }
        if !program.constraints.relations.is_empty() {
            out.push_str("\n    :relations (");
            out.push_str(
                &program
                    .constraints
                    .relations
                    .iter()
                    .map(|relation| emit_relation_constraint(relation, &param_names))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            out.push(')');
        }
        out.push(')');
    }
    for verify_clause in &program.constraints.verify_clauses {
        out.push_str("\n  ");
        out.push_str(&emit_verify_clause(verify_clause));
    }
    for part in &program.parts {
        if let Some(feature_decl) = program.feature_decls.get(&part.key) {
            out.push_str("\n  (feature ");
            out.push_str(&feature_decl.feature_id);
            out.push_str(" :role ");
            out.push_str(&emit_string(&feature_decl.role));
            if !feature_decl.param_keys.is_empty() {
                out.push_str(" :params (");
                out.push_str(&feature_decl.param_keys.join(" "));
                out.push(')');
            }
            out.push(' ');
            out.push_str(&emit_node(&part.root, &param_names, &BTreeMap::new()));
            out.push(')');
            continue;
        }

        out.push_str("\n  (part ");
        out.push_str(&part.key);
        if part.label != humanize(&part.key) {
            out.push(' ');
            out.push_str(&emit_string(&part.label));
        }
        out.push(' ');
        out.push_str(&emit_node(&part.root, &param_names, &BTreeMap::new()));
        out.push(')');
    }
    out.push_str("\n)");
    out
}

fn emit_verify_clause(clause: &CoreVerifyClause) -> String {
    format!(
        "(verify {} {} {})",
        emit_verify_section("tag", &clause.tag),
        emit_verify_section("metric", &clause.metric),
        emit_verify_section("expect", &clause.expect)
    )
}

fn emit_verify_section(name: &str, section: &CoreVerifySection) -> String {
    if section.items.is_empty() {
        return format!("({name})");
    }

    format!(
        "({} {})",
        name,
        section
            .items
            .iter()
            .map(emit_verify_value)
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn emit_verify_value(value: &CoreVerifyValue) -> String {
    match value {
        CoreVerifyValue::Symbol(symbol) => symbol.clone(),
        CoreVerifyValue::Number(number) => emit_number(*number),
        CoreVerifyValue::Boolean(flag) => {
            if *flag {
                "#t".to_string()
            } else {
                "#f".to_string()
            }
        }
        CoreVerifyValue::Text(text) => emit_string(text),
        CoreVerifyValue::List(items) => format!(
            "({})",
            items
                .iter()
                .map(emit_verify_value)
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}

fn emit_param(param: &CoreParameter) -> String {
    let mut out = match param.kind {
        CoreParameterKind::Number => format!(
            "(number {} {}",
            param.key,
            emit_param_value(&param.default_value)
        ),
        CoreParameterKind::Boolean => format!(
            "(toggle {} {}",
            param.key,
            emit_param_value(&param.default_value)
        ),
        CoreParameterKind::Choice => format!(
            "(select {} {}",
            param.key,
            emit_param_value(&param.default_value)
        ),
        CoreParameterKind::Image => format!(
            "(image {} {}",
            param.key,
            emit_param_value(&param.default_value)
        ),
        CoreParameterKind::Text => format!(
            "(text-param {} {}",
            param.key,
            emit_param_value(&param.default_value)
        ),
    };
    if param.label != humanize(&param.key) {
        out.push_str(" :label ");
        out.push_str(&emit_string(&param.label));
    }
    if let Some(min) = param.constraints.min {
        out.push_str(&format!(" :min {}", emit_number(min)));
    }
    if let Some(max) = param.constraints.max {
        out.push_str(&format!(" :max {}", emit_number(max)));
    }
    if let Some(step) = param.constraints.step {
        out.push_str(&format!(" :step {}", emit_number(step)));
    }
    if let Some(unit) = &param.constraints.unit {
        out.push_str(" :unit ");
        out.push_str(&emit_string(unit));
    }
    if param.frozen {
        out.push_str(" :frozen #t");
    }
    if !param.constraints.choices.is_empty() {
        out.push_str(" :options (");
        let rendered = param
            .constraints
            .choices
            .iter()
            .map(|choice| {
                format!(
                    "({} {})",
                    emit_string(&choice.label),
                    emit_param_value(&choice.value)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&rendered);
        out.push(')');
    }
    out.push(')');
    out
}

fn emit_param_value(value: &CoreParameterValue) -> String {
    match value {
        CoreParameterValue::Number(n) => emit_number(*n),
        CoreParameterValue::Boolean(flag) => {
            if *flag {
                "#t".to_string()
            } else {
                "#f".to_string()
            }
        }
        CoreParameterValue::Text(text)
        | CoreParameterValue::Choice(text)
        | CoreParameterValue::Image(text) => emit_string(text),
    }
}

fn emit_relation_constraint(
    relation: &CoreRelationConstraint,
    param_names: &BTreeMap<u64, String>,
) -> String {
    format!(
        "({} {} {})",
        relation.operator.as_str(),
        emit_relation_operand(&relation.left, param_names),
        emit_relation_operand(&relation.right, param_names)
    )
}

fn emit_relation_operand(
    operand: &CoreRelationOperand,
    param_names: &BTreeMap<u64, String>,
) -> String {
    match operand {
        CoreRelationOperand::Parameter(param_id) => param_names
            .get(&param_id.raw())
            .cloned()
            .unwrap_or_else(|| format!("p{}", param_id.raw())),
        CoreRelationOperand::Number(value) => emit_number(*value),
    }
}

fn emit_node(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    node_names: &BTreeMap<u64, String>,
) -> String {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Number(n)) => emit_number(*n),
        CoreNodeKind::Literal(CoreLiteral::Boolean(flag)) => {
            if *flag {
                "#t".to_string()
            } else {
                "#f".to_string()
            }
        }
        CoreNodeKind::Literal(CoreLiteral::Text(text)) => emit_string(text),
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => match symbol {
            CoreSymbol::Start => "start".to_string(),
            CoreSymbol::End => "end".to_string(),
            CoreSymbol::Xy => "xy".to_string(),
            CoreSymbol::Yz => "yz".to_string(),
            CoreSymbol::Xz => "xz".to_string(),
            CoreSymbol::Min => "min".to_string(),
            CoreSymbol::Center => "center".to_string(),
            CoreSymbol::Max => "max".to_string(),
        },
        CoreNodeKind::Literal(CoreLiteral::Point2(point)) => {
            format!("({} {})", emit_number(point[0]), emit_number(point[1]))
        }
        CoreNodeKind::Literal(CoreLiteral::Point3(point)) => format!(
            "({} {} {})",
            emit_number(point[0]),
            emit_number(point[1]),
            emit_number(point[2])
        ),
        CoreNodeKind::Reference(CoreReference::Local(name)) => name.clone(),
        CoreNodeKind::Reference(CoreReference::Parameter(id)) => param_names
            .get(&id.raw())
            .cloned()
            .unwrap_or_else(|| "param-ref".to_string()),
        CoreNodeKind::Reference(CoreReference::Node(id)) => node_names
            .get(&id.raw())
            .cloned()
            .unwrap_or_else(|| "node-ref".to_string()),
        CoreNodeKind::Reference(CoreReference::Part(_)) => "part-ref".to_string(),
        CoreNodeKind::Build { bindings, result } => {
            let mut nested = node_names.clone();
            for binding in bindings {
                nested.insert(binding.value.id.raw(), binding.name.clone());
            }
            let rendered = bindings
                .iter()
                .map(|binding| {
                    format!(
                        "(shape {} {})",
                        binding.name,
                        emit_node(&binding.value, param_names, &nested)
                    )
                })
                .chain(std::iter::once(format!(
                    "(result {})",
                    emit_node(result, param_names, &nested)
                )))
                .collect::<Vec<_>>()
                .join(" ");
            format!("(build {})", rendered)
        }
        CoreNodeKind::Call { op, args, keywords } => {
            let mut items = vec![emit_operation(op)];
            items.extend(
                args.iter()
                    .map(|arg| emit_node(arg, param_names, node_names)),
            );
            for keyword in keywords {
                items.push(format!(":{}", keyword.name));
                items.push(emit_node(keyword.source_node(), param_names, node_names));
            }
            format!("({})", items.join(" "))
        }
        CoreNodeKind::Range { start, end } => format!(
            "(range {} {})",
            emit_node(start, param_names, node_names),
            emit_node(end, param_names, node_names)
        ),
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => {
            let params = params.join(" ");
            let mut items = vec![format!(
                "(lambda ({}) {})",
                params,
                emit_node(body, param_names, node_names)
            )];
            items.extend(
                sources
                    .iter()
                    .map(|source| emit_node(source, param_names, node_names)),
            );
            format!("(map {})", items.join(" "))
        }
        CoreNodeKind::Apply { op, args, list } => {
            let mut items = vec!["apply".to_string(), emit_operation(op)];
            items.extend(
                args.iter()
                    .map(|arg| emit_node(arg, param_names, node_names)),
            );
            items.push(emit_node(list, param_names, node_names));
            format!("({})", items.join(" "))
        }
        CoreNodeKind::List(items) => {
            format!(
                "({})",
                items
                    .iter()
                    .map(|item| emit_node(item, param_names, node_names))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        CoreNodeKind::Group(items) => {
            format!(
                "({})",
                items
                    .iter()
                    .map(|item| emit_node(item, param_names, node_names))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        CoreNodeKind::Let { bindings, body } => {
            let rendered = bindings
                .iter()
                .map(|binding| {
                    format!(
                        "({} {})",
                        binding.name,
                        emit_node(&binding.value, param_names, node_names)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "(let ({}) {})",
                rendered,
                emit_node(body, param_names, node_names)
            )
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => format!(
            "(if {} {} {})",
            emit_node(condition, param_names, node_names),
            emit_node(then_branch, param_names, node_names),
            emit_node(else_branch, param_names, node_names)
        ),
    }
}

fn emit_operation(op: &CoreOperation) -> String {
    match op {
        CoreOperation::Primitive(CorePrimitive::Box) => "box".to_string(),
        CoreOperation::Primitive(CorePrimitive::Sphere) => "sphere".to_string(),
        CoreOperation::Primitive(CorePrimitive::Cylinder) => "cylinder".to_string(),
        CoreOperation::Primitive(CorePrimitive::Cone) => "cone".to_string(),
        CoreOperation::Primitive(CorePrimitive::Circle) => "circle".to_string(),
        CoreOperation::Primitive(CorePrimitive::Rectangle) => "rectangle".to_string(),
        CoreOperation::Primitive(CorePrimitive::RoundedRectangle) => "rounded-rect".to_string(),
        CoreOperation::Primitive(CorePrimitive::RoundedPolygon) => "rounded-polygon".to_string(),
        CoreOperation::Primitive(CorePrimitive::Polygon) => "polygon".to_string(),
        CoreOperation::Primitive(CorePrimitive::Profile) => "profile".to_string(),
        CoreOperation::Primitive(CorePrimitive::MakeFace) => "make-face".to_string(),
        CoreOperation::Primitive(CorePrimitive::Text) => "text".to_string(),
        CoreOperation::Primitive(CorePrimitive::Svg) => "svg".to_string(),
        CoreOperation::Primitive(CorePrimitive::Stl) => "import-stl".to_string(),
        CoreOperation::Boolean(CoreBooleanOp::Union) => "union".to_string(),
        CoreOperation::Boolean(CoreBooleanOp::Difference) => "difference".to_string(),
        CoreOperation::Boolean(CoreBooleanOp::Intersection) => "intersection".to_string(),
        CoreOperation::Boolean(CoreBooleanOp::Xor) => "xor".to_string(),
        CoreOperation::Transform(CoreTransformOp::Translate) => "translate".to_string(),
        CoreOperation::Transform(CoreTransformOp::Rotate) => "rotate".to_string(),
        CoreOperation::Transform(CoreTransformOp::Scale) => "scale".to_string(),
        CoreOperation::Transform(CoreTransformOp::Mirror) => "mirror".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Extrude) => "extrude".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Revolve) => "revolve".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Loft) => "loft".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Sweep) => "sweep".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Shell) => "shell".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Offset) => "offset".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::OffsetRounded) => "offset-rounded".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => "fillet".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => "chamfer".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Taper) => "taper".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Twist) => "twist".to_string(),
        CoreOperation::Path(CorePathOp::Polyline) => "path".to_string(),
        CoreOperation::Path(CorePathOp::BezierPath) => "bezier-path".to_string(),
        CoreOperation::Path(CorePathOp::Bspline) => "bspline".to_string(),
        CoreOperation::Array(CoreArrayOp::LinearArray) => "linear-array".to_string(),
        CoreOperation::Array(CoreArrayOp::RadialArray) => "radial-array".to_string(),
        CoreOperation::Array(CoreArrayOp::GridArray) => "grid-array".to_string(),
        CoreOperation::Array(CoreArrayOp::ArcArray) => "arc-array".to_string(),
        CoreOperation::Array(CoreArrayOp::Repeat) => "repeat".to_string(),
        CoreOperation::Array(CoreArrayOp::RepeatUnion) => "repeat-union".to_string(),
        CoreOperation::Array(CoreArrayOp::RepeatCompound) => "repeat-compound".to_string(),
        CoreOperation::Array(CoreArrayOp::RepeatPick) => "repeat-pick".to_string(),
        CoreOperation::Frame(CoreFrameOp::Plane) => "plane".to_string(),
        CoreOperation::Frame(CoreFrameOp::Location) => "location".to_string(),
        CoreOperation::Frame(CoreFrameOp::PathFrame) => "path-frame".to_string(),
        CoreOperation::Frame(CoreFrameOp::Place) => "place".to_string(),
        CoreOperation::Frame(CoreFrameOp::ClipBox) => "clip-box".to_string(),
        CoreOperation::Meta(CoreMetaOp::Group) => "compound".to_string(),
        CoreOperation::Meta(CoreMetaOp::Comment) => "meta".to_string(),
        CoreOperation::Meta(CoreMetaOp::Annotate) => "build".to_string(),
        CoreOperation::Custom(name) => name.clone(),
    }
}

fn emit_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

fn emit_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn humanize(key: &str) -> String {
    let mut out = String::new();
    for (index, chunk) in key
        .split(['-', '_'])
        .filter(|chunk| !chunk.is_empty())
        .enumerate()
    {
        if index > 0 {
            out.push(' ');
        }
        let mut chars = chunk.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
        }
        out.extend(chars);
    }
    if out.is_empty() {
        key.to_string()
    } else {
        out
    }
}

fn normalize_keyword(name: &str) -> String {
    if let Some(stripped) = name.strip_prefix("#:") {
        format!(":{}", stripped)
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_core_ir::{
        CoreArrayOp, CoreFrameOp, CoreNodeKind, CoreOperation, CorePathOp, CorePrimitive,
        CoreReference, CoreSurfaceOp, CoreSymbol,
    };

    #[test]
    fn blocks_mutation_surface() {
        let err =
            compile_to_core_program("(define x 1) (set! x 2) (model (part body (box 1 1 1)))")
                .expect_err("set! blocked");
        assert!(err.to_string().contains("set!"));
    }

    #[test]
    fn rejects_deep_source_before_steel_lowering() {
        let depth = ECKY_SOURCE_MAX_PAREN_DEPTH + 1;
        let source = format!("{}{}", "(".repeat(depth), ")".repeat(depth));
        let err = compile_to_core_program(&source).expect_err("deep source rejected");

        assert_eq!(err.kind, CompilerErrorKind::UnsupportedFeature);
        assert!(err.message.contains("before Steel lowering"), "{}", err);
        assert!(err.message.contains("nesting depth"), "{}", err);
    }

    #[test]
    fn source_budget_errors_do_not_fallback_to_legacy() {
        let depth = ECKY_SOURCE_MAX_PAREN_DEPTH + 1;
        let source = format!("{}{}", "(".repeat(depth), ")".repeat(depth));
        let result = try_compile_to_core_program(&source).expect("budget error is authoritative");
        let err = result.expect_err("deep source rejected");

        assert!(
            err.message.contains("before Steel lowering"),
            "{}",
            err.message
        );
    }

    #[test]
    fn compiles_wide_let_source_on_guarded_stack() {
        let bindings = (0..220)
            .map(|index| format!("(v{index} {index}.0)"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!("(model (part body (let* ({bindings}) (box v1 v2 v3))))");
        let program = compile_to_core_program(&source).expect("wide let compiles");

        assert_eq!(program.parts.len(), 1);
    }

    #[test]
    fn expanded_ast_gate_allows_structural_surface_forms() {
        assert!(can_use_expanded_ast(
            "(model (params (toggle raised true)) (part body (let ((lift (if raised 3 1))) (translate 0 0 lift (box 10 10 10)))))"
        ));
        assert!(can_use_expanded_ast(
            "(define base-radius 14) (model (params (number radius base-radius)) (part body (circle radius)))"
        ));
        assert!(can_use_expanded_ast(
            "(define (cup-body radius height) (extrude (circle radius) height)) (model (part body (cup-body 12 30)))"
        ));
        assert!(can_use_expanded_ast(
            "(model (part body (build (shape pts (map (lambda (i) (list i (+ i 1))) (range 0 3))) (result (polygon pts)))))"
        ));
    }

    #[test]
    fn compiles_let_and_if_source_via_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            "(model (params (toggle raised true)) (part body (let ((lift (if raised 3 1))) (translate 0 0 lift (box 10 10 10)))))",
        )
        .expect("compile");
        let root = &program.parts[0].root;
        let CoreNodeKind::Let { bindings, body } = &root.kind else {
            panic!("expected let node, got {:?}", root.kind);
        };
        assert_eq!(bindings.len(), 1);
        assert!(matches!(bindings[0].value.kind, CoreNodeKind::If { .. }));
        assert!(matches!(body.kind, CoreNodeKind::Call { .. }));
    }

    #[test]
    fn compiles_let_star_source_via_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            "(model (part body (let* ((a 2) (b (+ a 1))) (translate b 0 0 (box 10 10 10)))))",
        )
        .expect("compile");
        let root = &program.parts[0].root;
        let CoreNodeKind::Let { bindings, body } = &root.kind else {
            panic!("expected outer let node, got {:?}", root.kind);
        };
        assert_eq!(bindings.len(), 1);
        assert!(
            bindings[0].name.contains('a'),
            "expected hygienic outer binding for a, got {}",
            bindings[0].name
        );
        let CoreNodeKind::Let {
            bindings: inner_bindings,
            body: inner_body,
        } = &body.kind
        else {
            panic!("expected nested let node, got {:?}", body.kind);
        };
        assert_eq!(inner_bindings.len(), 1);
        assert!(
            inner_bindings[0].name.contains('b'),
            "expected hygienic inner binding for b, got {}",
            inner_bindings[0].name
        );
        let CoreNodeKind::Call { .. } = &inner_body.kind else {
            panic!("expected translate call, got {:?}", inner_body.kind);
        };
    }

    #[test]
    fn splices_begin_model_clauses_via_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (begin
                (params (number width 12))
                (meta source "fixture")
                (part body (box width 2 3))
                (part cap (sphere 4))))
            "#,
        )
        .expect("compile");

        assert_eq!(program.parameters.len(), 1);
        assert_eq!(program.parameters[0].key, "width");
        assert_eq!(program.parts.len(), 2);
        assert_eq!(program.parts[0].key, "body");
        assert_eq!(program.parts[1].key, "cap");
    }

    #[test]
    fn splices_let_model_clauses_via_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (let ((default-width 18)
                    (body-depth 5))
                (params (number width default-width))
                (part body (box width body-depth 3))))
            "#,
        )
        .expect("compile");

        assert_eq!(program.parameters.len(), 1);
        assert!(matches!(
            program.parameters[0].default_value,
            crate::ecky_core_ir::CoreParameterValue::Number(18.0)
        ));
        assert_eq!(program.parts.len(), 1);
        let CoreNodeKind::Call { op, args, .. } = &program.parts[0].root.kind else {
            panic!("expected box call, got {:?}", program.parts[0].root.kind);
        };
        assert!(matches!(op, CoreOperation::Primitive(CorePrimitive::Box)));
        assert!(matches!(
            args[0].kind,
            CoreNodeKind::Reference(CoreReference::Parameter(_))
        ));
        assert!(matches!(
            args[1].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(5.0))
        ));
    }

    #[test]
    fn splices_let_star_model_clauses_via_runtime_path() {
        let program = compile_to_core_program_via_runtime(
            r#"
            (model
              (let* ((default-width 11)
                     (body-depth (+ default-width 4)))
                (params (number width default-width))
                (part body (box width body-depth 3))))
            "#,
        )
        .expect("compile");

        assert_eq!(program.parameters.len(), 1);
        assert!(matches!(
            program.parameters[0].default_value,
            crate::ecky_core_ir::CoreParameterValue::Number(11.0)
        ));
        assert_eq!(program.parts.len(), 1);
        let CoreNodeKind::Call { args, .. } = &program.parts[0].root.kind else {
            panic!("expected box call, got {:?}", program.parts[0].root.kind);
        };
        assert!(matches!(
            args[1].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(15.0))
        ));
    }

    #[test]
    fn splices_model_clauses_when_runtime_path_is_forced_by_user_macro() {
        let program = compile_to_core_program(
            r#"
            (define-syntax passthrough
              (syntax-rules ()
                [(_ expr) expr]))
            (model
              (let* ((default-width 9)
                     (body-depth (+ default-width 6)))
                (params (number width default-width))
                (part body (passthrough (box width body-depth 3)))))
            "#,
        )
        .expect("compile");

        assert_eq!(program.parameters.len(), 1);
        assert!(matches!(
            program.parameters[0].default_value,
            crate::ecky_core_ir::CoreParameterValue::Number(9.0)
        ));
        let CoreNodeKind::Call { args, .. } = &program.parts[0].root.kind else {
            panic!("expected box call, got {:?}", program.parts[0].root.kind);
        };
        assert!(matches!(
            args[1].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(15.0))
        ));
    }

    #[test]
    fn compiles_model_level_let_star_with_computed_param_default() {
        let program = compile_to_core_program(
            r#"
            (model
              (let* ((default-r 20)
                     (default-h (* default-r 3)))
                (params (number radius default-r :label "Radius")
                        (number height default-h :label "Height"))
                (part body (cylinder radius height 48))))
            "#,
        )
        .expect("compile");

        assert_eq!(program.parameters.len(), 2);
        assert!(matches!(
            program.parameters[0].default_value,
            crate::ecky_core_ir::CoreParameterValue::Number(20.0)
        ));
        assert!(matches!(
            program.parameters[1].default_value,
            crate::ecky_core_ir::CoreParameterValue::Number(60.0)
        ));
        assert_eq!(program.parts.len(), 1);
    }

    #[test]
    fn spliced_model_clauses_preserve_unsupported_clause_error() {
        let err = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (begin
                (bogus clause)
                (part body (box 1 1 1))))
            "#,
        )
        .expect_err("unsupported clause");

        assert!(err
            .to_string()
            .contains("Unsupported top-level model clause `bogus`."));
    }

    #[test]
    fn model_level_map_reports_clause_boundary() {
        let err = compile_to_core_program(
            r#"
            (model
              (map (lambda (i) (part body (box i 1 1))) (range 1 3)))
            "#,
        )
        .expect_err("model-level map must be rejected");

        let message = err.to_string();
        assert!(message.contains("Model children are clauses"), "{message}");
        assert!(
            message.contains(
                "Supported direct clauses: `params`, `verify`, `part`, `feature`, `meta`."
            ),
            "{message}"
        );
        assert!(
            message.contains("Supported wrappers: `begin`, `let`, `let*`."),
            "{message}"
        );
        assert!(
            message.contains("`map` belongs inside `(part ...)` geometry/list expressions."),
            "{message}"
        );
    }

    #[test]
    fn model_level_range_reports_clause_boundary() {
        let err = compile_to_core_program(
            r#"
            (model
              (range 1 3))
            "#,
        )
        .expect_err("model-level range must be rejected");

        let message = err.to_string();
        assert!(message.contains("Model children are clauses"), "{message}");
        assert!(
            message.contains(
                "Supported direct clauses: `params`, `verify`, `part`, `feature`, `meta`."
            ),
            "{message}"
        );
        assert!(
            message.contains("Supported wrappers: `begin`, `let`, `let*`."),
            "{message}"
        );
        assert!(
            message.contains("`range` belongs inside `(part ...)` geometry/list expressions."),
            "{message}"
        );
    }

    #[test]
    fn compiles_simple_value_define_via_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            "(define base-radius 14) (model (params (number radius base-radius)) (part body (circle radius)))",
        )
        .expect("compile");

        assert_eq!(program.parameters.len(), 1);
        assert!(matches!(
            program.parameters[0].default_value,
            crate::ecky_core_ir::CoreParameterValue::Number(14.0)
        ));
    }

    #[test]
    fn compiles_simple_function_define_via_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            "(define (cup-body radius height) (extrude (circle radius) height)) (model (part body (cup-body 12 30)))",
        )
        .expect("compile");

        let root = &program.parts[0].root;
        let CoreNodeKind::Let { bindings, body } = &root.kind else {
            panic!(
                "expected helper call to lower into let, got {:?}",
                root.kind
            );
        };
        assert_eq!(bindings.len(), 2);
        assert!(bindings[0].name.contains("radius"));
        assert!(bindings[1].name.contains("height"));
        let CoreNodeKind::Call { op, args, .. } = &body.kind else {
            panic!("expected extrude call, got {:?}", body.kind);
        };
        assert!(matches!(op, CoreOperation::Surface(CoreSurfaceOp::Extrude)));
        let CoreNodeKind::Call {
            op: circle_op,
            args: circle_args,
            ..
        } = &args[0].kind
        else {
            panic!("expected circle call, got {:?}", args[0].kind);
        };
        assert!(matches!(
            circle_op,
            CoreOperation::Primitive(CorePrimitive::Circle)
        ));
        assert!(matches!(
            circle_args[0].kind,
            CoreNodeKind::Reference(CoreReference::Local(_))
        ));
        assert!(matches!(
            args[1].kind,
            CoreNodeKind::Reference(CoreReference::Local(_))
        ));
    }

    #[test]
    fn parses_path_frame_keywords() {
        let program = compile_to_core_program(
            "(model (part body (build (shape rail (path (list 0 0 0) (list 10 0 10))) (shape frame (path-frame rail :at end)) (result frame))))",
        )
        .expect("compile");
        let root = &program.parts[0].root;
        let CoreNodeKind::Build { bindings, result } = &root.kind else {
            panic!("expected build node, got {:?}", root.kind);
        };
        assert_eq!(bindings.len(), 2);
        let CoreNodeKind::Call { op, args, keywords } = &bindings[1].value.kind else {
            panic!("expected call");
        };
        assert!(matches!(op, CoreOperation::Frame(CoreFrameOp::PathFrame)));
        assert!(matches!(
            args[0].kind,
            CoreNodeKind::Reference(CoreReference::Node(_))
        ));
        assert_eq!(keywords.len(), 1);
        assert!(matches!(
            keywords[0].source_node().kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Symbol(CoreSymbol::End))
        ));
        assert!(matches!(
            result.kind,
            CoreNodeKind::Reference(CoreReference::Node(_))
        ));
    }

    #[test]
    fn emits_typed_path_and_linear_array_ops() {
        let program = compile_to_core_program(
            "(model (part body (linear-array 2 10 0 0 (polyline (list 0 0 0) (list 1 0 0)))))",
        )
        .expect("compile");
        let root = &program.parts[0].root;
        let CoreNodeKind::Call { op, args, .. } = &root.kind else {
            panic!("expected call");
        };
        assert!(matches!(op, CoreOperation::Array(CoreArrayOp::LinearArray)));
        let CoreNodeKind::Call { op: path_op, .. } = &args[4].kind else {
            panic!("expected path call");
        };
        assert!(matches!(path_op, CoreOperation::Path(CorePathOp::Polyline)));
    }

    #[test]
    fn rounded_rect_alias_infers_sketch_kind_before_typecheck() {
        let program =
            compile_to_core_program("(model (part body (extrude (rounded_rect 20 10 2) 5)))")
                .expect("rounded_rect alias should infer sketch kind");

        let CoreNodeKind::Call { op, args, .. } = &program.parts[0].root.kind else {
            panic!(
                "expected extrude call, got {:?}",
                program.parts[0].root.kind
            );
        };
        assert!(matches!(op, CoreOperation::Surface(CoreSurfaceOp::Extrude)));
        let CoreNodeKind::Call { op: profile_op, .. } = &args[0].kind else {
            panic!("expected rounded rect profile call, got {:?}", args[0].kind);
        };
        assert!(matches!(
            profile_op,
            CoreOperation::Primitive(CorePrimitive::RoundedRectangle)
        ));
    }

    #[test]
    fn ring_alias_infers_sketch_kind_before_typecheck() {
        let program = compile_to_core_program("(model (part body (extrude (ring 20 10 64) 5)))")
            .expect("ring alias should infer sketch kind");

        let CoreNodeKind::Call { op, args, .. } = &program.parts[0].root.kind else {
            panic!(
                "expected extrude call, got {:?}",
                program.parts[0].root.kind
            );
        };
        assert!(matches!(op, CoreOperation::Surface(CoreSurfaceOp::Extrude)));
        let CoreNodeKind::Call {
            op: profile_op,
            keywords,
            ..
        } = &args[0].kind
        else {
            panic!("expected profile call, got {:?}", args[0].kind);
        };
        assert!(matches!(
            profile_op,
            CoreOperation::Primitive(CorePrimitive::Profile)
        ));
        assert_eq!(keywords.len(), 2);
        assert_eq!(keywords[0].name, "outer");
        assert_eq!(keywords[1].name, "holes");
    }

    #[test]
    fn polygon_rejects_known_3d_point_lists_before_lowering() {
        let literal_err = compile_to_core_program(
            r#"
            (model
              (part body
                (extrude
                  (polygon ((0 0 0) (1 0 0) (0 1 0)))
                  1)))
            "#,
        )
        .expect_err("polygon must reject literal 3D points");

        let message = literal_err.to_string();
        assert!(message.contains("polygon"), "{message}");
        assert!(message.contains("2D point list"), "{message}");
        assert!(message.contains("point3 list"), "{message}");

        let helper_err = compile_to_core_program(
            r#"
            (model
              (part body
                (extrude
                  (polygon (lorenz-points 4 0.01 10))
                  1)))
            "#,
        )
        .expect_err("polygon must reject helper-expanded 3D points");

        let message = helper_err.to_string();
        assert!(message.contains("polygon"), "{message}");
        assert!(message.contains("2D point list"), "{message}");
        assert!(message.contains("point3 list"), "{message}");
    }

    #[test]
    fn path_rejects_known_2d_point_lists_before_lowering() {
        for op in ["path", "polyline"] {
            let source = format!(
                r#"
                (model
                  (part body
                    ({} ((0 0) (1 0)))))
                "#,
                op
            );
            let err = compile_to_core_program(&source)
                .expect_err(&format!("{op} must reject literal 2D points"));

            let message = err.to_string();
            assert!(message.contains("path"), "{message}");
            assert!(message.contains("3D point list"), "{message}");
            assert!(message.contains("point2 list"), "{message}");
        }

        let helper_err = compile_to_core_program(
            r#"
            (model
              (part body
                (path (organic-loop 8 10 1 2))))
            "#,
        )
        .expect_err("path must reject helper-expanded 2D points");

        let message = helper_err.to_string();
        assert!(message.contains("path"), "{message}");
        assert!(message.contains("3D point list"), "{message}");
        assert!(message.contains("point2 list"), "{message}");
    }

    #[test]
    fn bspline_rejects_known_3d_point_lists_before_lowering() {
        let err = compile_to_core_program(
            r#"
            (model
              (part body
                (extrude
                  (bspline ((0 0 0) (1 0 0) (1 1 0) (0 1 0)))
                  1)))
            "#,
        )
        .expect_err("bspline sketch path must reject 3D points");

        let message = err.to_string();
        assert!(message.contains("bspline"), "{message}");
        assert!(message.contains("2D point list"), "{message}");
        assert!(message.contains("point3 list"), "{message}");
    }

    #[test]
    fn dynamic_point_lists_remain_permissive() {
        compile_to_core_program(
            r#"
            (model
              (params (number n 2))
              (part body
                (build
                  (shape pts
                    (map
                      (lambda (i)
                        (if (< i 1)
                          n
                          (list i 0 0)))
                      (range 0 n)))
                  (result
                    (path pts)))))
            "#,
        )
        .expect("dynamic list element kind stays unknown");
    }

    #[test]
    fn compiles_range_map_append_reverse_on_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (part body
                (build
                  (shape pts
                    (append
                      (list (list 0 10))
                      (map (lambda (i)
                             (list i (+ i 10)))
                           (range 1 4))
                      (list (list 4 0))
                      (reverse
                        (map (lambda (i)
                               (list i i))
                             (range 1 4)))))
                  (result (polygon pts)))))
            "#,
        )
        .expect("compile");

        let root = &program.parts[0].root;
        let CoreNodeKind::Build { bindings, result } = &root.kind else {
            panic!("expected build node, got {:?}", root.kind);
        };
        assert_eq!(bindings.len(), 1);
        let CoreNodeKind::List(pts_items) = &bindings[0].value.kind else {
            panic!("expected appended list, got {:?}", bindings[0].value.kind);
        };
        assert_eq!(pts_items.len(), 8);
        assert!(pts_items[1..4]
            .iter()
            .all(|node| matches!(node.kind, CoreNodeKind::Let { .. })));
        let CoreNodeKind::Call { op, args, .. } = &result.kind else {
            panic!("expected polygon call, got {:?}", result.kind);
        };
        assert!(matches!(
            op,
            CoreOperation::Primitive(CorePrimitive::Polygon)
        ));
        assert!(matches!(
            args[0].kind,
            CoreNodeKind::Reference(CoreReference::Node(_))
        ));
    }

    #[test]
    fn compiles_generic_sequence_builtins_on_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (part body
                (list
                  (map list (range 0 4) (range 10 12) (range 20 23))
                  (zip (range 0 3) (range 10 12))
                  (enumerate (range 5 7))
                  (linspace 0 10 3)
                  (linspace 4 8 1)
                  (flat-map (lambda (i)
                              (let ((j (+ i 10)))
                                (list i j)))
                            (range 1 3))
                  (concat-map list (range 7 9)))))
            "#,
        )
        .expect("compile");

        let root = &program.parts[0].root;
        let CoreNodeKind::List(groups) = &root.kind else {
            panic!("expected root list, got {:?}", root.kind);
        };
        assert_eq!(groups.len(), 7);

        let CoreNodeKind::List(mapped) = &groups[0].kind else {
            panic!("expected mapped list, got {:?}", groups[0].kind);
        };
        assert_eq!(mapped.len(), 2);
        assert!(mapped
            .iter()
            .all(|node| matches!(node.kind, CoreNodeKind::List(_))));

        let CoreNodeKind::List(zipped) = &groups[1].kind else {
            panic!("expected zipped list, got {:?}", groups[1].kind);
        };
        assert_eq!(zipped.len(), 2);
        assert!(zipped
            .iter()
            .all(|node| matches!(node.kind, CoreNodeKind::List(_))));

        let CoreNodeKind::List(enumerated) = &groups[2].kind else {
            panic!("expected enumerated list, got {:?}", groups[2].kind);
        };
        assert_eq!(enumerated.len(), 2);
        let CoreNodeKind::List(first_pair) = &enumerated[0].kind else {
            panic!("expected enumerate pair, got {:?}", enumerated[0].kind);
        };
        assert!(matches!(
            first_pair[0].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(0.0))
        ));
        assert!(matches!(
            first_pair[1].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(5.0))
        ));

        let CoreNodeKind::List(linspace) = &groups[3].kind else {
            panic!("expected linspace list, got {:?}", groups[3].kind);
        };
        assert_eq!(linspace.len(), 3);
        assert!(matches!(
            linspace[0].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(0.0))
        ));
        assert!(matches!(
            linspace[1].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(5.0))
        ));
        assert!(matches!(
            linspace[2].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(10.0))
        ));

        let CoreNodeKind::List(singleton_linspace) = &groups[4].kind else {
            panic!("expected singleton linspace list, got {:?}", groups[4].kind);
        };
        assert_eq!(singleton_linspace.len(), 1);
        assert!(matches!(
            singleton_linspace[0].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(4.0))
        ));

        let CoreNodeKind::List(flattened) = &groups[5].kind else {
            panic!("expected flat-map list, got {:?}", groups[5].kind);
        };
        assert_eq!(flattened.len(), 4);
        assert!(flattened
            .iter()
            .all(|node| matches!(node.kind, CoreNodeKind::Let { .. })));

        let CoreNodeKind::List(concat_mapped) = &groups[6].kind else {
            panic!("expected concat-map list, got {:?}", groups[6].kind);
        };
        assert_eq!(concat_mapped.len(), 2);
        assert!(matches!(
            concat_mapped[0].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(7.0))
        ));
        assert!(matches!(
            concat_mapped[1].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(8.0))
        ));
    }

    #[test]
    fn compiles_static_zip_destructuring_in_map_lambda() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (part body
                (extrude
                  (polygon
                    (map
                      (lambda ((x y))
                        (list x y))
                      (zip (range 0 3) (range 10 13))))
                  2)))
            "#,
        )
        .expect("compile static zip destructuring");

        let CoreNodeKind::Call { op, args, .. } = &program.parts[0].root.kind else {
            panic!("expected extrude, got {:?}", program.parts[0].root.kind);
        };
        assert!(matches!(op, CoreOperation::Surface(CoreSurfaceOp::Extrude)));
        let CoreNodeKind::Call {
            op: polygon_op,
            args: polygon_args,
            ..
        } = &args[0].kind
        else {
            panic!("expected polygon, got {:?}", args[0].kind);
        };
        assert!(matches!(
            polygon_op,
            CoreOperation::Primitive(CorePrimitive::Polygon)
        ));
        let CoreNodeKind::List(mapped) = &polygon_args[0].kind else {
            panic!("expected mapped point list, got {:?}", polygon_args[0].kind);
        };
        assert_eq!(mapped.len(), 3);
        for item in mapped {
            let CoreNodeKind::Let { bindings, body } = &item.kind else {
                panic!("expected destructuring let item, got {:?}", item.kind);
            };
            assert_eq!(bindings.len(), 2);
            assert!(bindings[0].name.contains("x"), "{}", bindings[0].name);
            assert!(bindings[1].name.contains("y"), "{}", bindings[1].name);
            assert!(matches!(body.kind, CoreNodeKind::List(_)));
        }
    }

    #[test]
    fn compiles_static_enumerate_destructuring_in_map_lambda() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (part body
                (apply union
                  (map
                    (lambda ((index value))
                      (translate (* index 4) value 0 (box 1 1 1)))
                    (enumerate (range 1 4))))))
            "#,
        )
        .expect("compile static enumerate destructuring");

        let CoreNodeKind::Apply { op, list, .. } = &program.parts[0].root.kind else {
            panic!("expected apply union, got {:?}", program.parts[0].root.kind);
        };
        assert!(matches!(op, CoreOperation::Boolean(CoreBooleanOp::Union)));
        let CoreNodeKind::List(mapped) = &list.kind else {
            panic!("expected mapped solids, got {:?}", list.kind);
        };
        assert_eq!(mapped.len(), 3);
        assert!(mapped
            .iter()
            .all(|item| matches!(item.kind, CoreNodeKind::Let { .. })));
    }

    #[test]
    fn dynamic_map_rejects_lambda_destructuring_with_clear_message() {
        let err = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (params (number n 3))
              (part body
                (map
                  (lambda ((x y)) (list x y))
                  (map (lambda (i) (list i i)) (range 0 n)))))
            "#,
        )
        .expect_err("dynamic destructuring requires runtime tuple access");

        let message = err.to_string();
        assert!(message.contains("`map` lambda parameter"), "{message}");
        assert!(message.contains("`zip` or static `enumerate`"), "{message}");
        assert!(message.contains("destructuring"), "{message}");
    }

    #[test]
    fn compiles_deterministic_fancy_helpers_into_portable_points() {
        let program = compile_to_core_program(
            r#"
            (model
              (params (number seed 7 :label "Seed" :min 0 :max 99))
              (part body
                (extrude
                  (polygon (organic-loop 12 20 3 seed))
                  4)))
            "#,
        )
        .expect("compile");

        assert_eq!(program.parts.len(), 1);
    }

    #[test]
    fn parses_param_unit_metadata() {
        let program = compile_to_core_program(
            r#"
            (model
              (params
                (number width 12 :unit length)
                (number sweep 90 :unit angle)
                (number scale 0.5 :unit ratio)
                (number teeth 16 :unit count)
                (select material "PLA" :unit text :options (("PLA" "PLA") ("PETG" "PETG"))))
              (part body (box width 10 2)))
            "#,
        )
        .expect("compile");

        let units = program
            .parameters
            .iter()
            .map(|param| param.constraints.unit.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            units,
            vec![
                Some("length"),
                Some("angle"),
                Some("ratio"),
                Some("count"),
                Some("text")
            ]
        );
    }

    #[test]
    fn parses_runtime_param_unit_metadata() {
        let program = compile_to_core_program_via_runtime(
            r#"
            (model
              (params (number width 12 :unit "length"))
              (part body (box width 10 2)))
            "#,
        )
        .expect("compile");

        assert_eq!(
            program.parameters[0].constraints.unit.as_deref(),
            Some("length")
        );
    }

    #[test]
    fn emits_param_unit_metadata() {
        let source = compile_to_legacy_source(
            r#"
            (model
              (params (number sweep 90 :unit angle))
              (part body (rotate 0 0 sweep (box 10 10 2))))
            "#,
        )
        .expect("compile");

        assert!(
            source.contains("(number sweep 90 :unit \"angle\")"),
            "{source}"
        );
    }

    #[test]
    fn parses_param_relation_metadata() {
        let program = compile_to_core_program(
            r#"
            (model
              (params
                (number lens_bore_d 8)
                (number tunnel_aperture_h 10)
                :relations ((< lens_bore_d tunnel_aperture_h)))
              (part body (box lens_bore_d 2 3)))
            "#,
        )
        .expect("compile");

        assert_eq!(program.constraints.relations.len(), 1);
        assert_eq!(program.parameters[0].constraints.relations.len(), 1);
        assert_eq!(program.parameters[1].constraints.relations.len(), 1);
    }

    #[test]
    fn emits_param_relation_metadata() {
        let source = compile_to_legacy_source(
            r#"
            (model
              (params
                (number lens_bore_d 8)
                (number tunnel_aperture_h 10)
                :relations ((< lens_bore_d tunnel_aperture_h)))
              (part body (box lens_bore_d 2 3)))
            "#,
        )
        .expect("compile");

        assert!(
            source.contains(":relations ((< lens_bore_d tunnel_aperture_h))"),
            "{source}"
        );
    }

    #[test]
    fn parses_verify_clause_metadata() {
        let program = compile_to_core_program(
            r#"
            (model
              (verify
                (tag body_shell)
                (metric min_wall_thickness "body")
                (expect (>= value 2)))
              (part body (box 10 10 10)))
            "#,
        )
        .expect("compile");

        assert_eq!(program.constraints.verify_clauses.len(), 1);
        assert_eq!(
            program.constraints.verify_clauses[0],
            CoreVerifyClause {
                tag: CoreVerifySection {
                    items: vec![CoreVerifyValue::Symbol("body_shell".into())],
                },
                metric: CoreVerifySection {
                    items: vec![
                        CoreVerifyValue::Symbol("min_wall_thickness".into()),
                        CoreVerifyValue::Text("body".into()),
                    ],
                },
                expect: CoreVerifySection {
                    items: vec![CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol(">=".into()),
                        CoreVerifyValue::Symbol("value".into()),
                        CoreVerifyValue::Number(2.0),
                    ])],
                },
            }
        );
    }

    #[test]
    fn parses_runtime_verify_clause_metadata() {
        let program = compile_to_core_program_via_runtime(
            r#"
            (model
              (verify
                (tag body_shell)
                (metric min_wall_thickness "body")
                (expect (>= value 2)))
              (part body (box 10 10 10)))
            "#,
        )
        .expect("compile");

        assert_eq!(program.constraints.verify_clauses.len(), 1);
        assert_eq!(
            program.constraints.verify_clauses[0].metric.items,
            vec![
                CoreVerifyValue::Symbol("min_wall_thickness".into()),
                CoreVerifyValue::Text("body".into()),
            ]
        );
    }

    #[test]
    fn emits_verify_clause_metadata() {
        let source = compile_to_legacy_source(
            r#"
            (model
              (verify
                (tag body_shell)
                (metric min_wall_thickness "body")
                (expect (>= value 2)))
              (part body (box 10 10 10)))
            "#,
        )
        .expect("compile");

        assert!(
            source.contains(
                "(verify (tag body_shell) (metric min_wall_thickness \"body\") (expect (>= value 2)))"
            ),
            "{source}"
        );
    }

    #[test]
    fn roundtrips_verify_clause_metadata() {
        let source = compile_to_legacy_source(
            r#"
            (model
              (verify
                (tag body_shell)
                (metric min_wall_thickness "body")
                (expect (>= value 2)))
              (part body (box 10 10 10)))
            "#,
        )
        .expect("compile");

        let reparsed = compile_to_core_program(&source).expect("reparse emitted verify");
        assert_eq!(reparsed.constraints.verify_clauses.len(), 1);
        assert_eq!(
            reparsed.constraints.verify_clauses[0].expect.items,
            vec![CoreVerifyValue::List(vec![
                CoreVerifyValue::Symbol(">=".into()),
                CoreVerifyValue::Symbol("value".into()),
                CoreVerifyValue::Number(2.0),
            ])]
        );
    }

    #[test]
    fn parses_feature_metadata() {
        let program = compile_to_core_program(
            r#"
            (model
              (params (number width 12))
              (feature body :role shell :params (width) (box width 8 6)))
            "#,
        )
        .expect("compile");

        let feature = program.feature_decls.get("body").expect("feature metadata");
        assert_eq!(feature.feature_id, "body");
        assert_eq!(feature.role, "shell");
        assert_eq!(feature.param_keys, vec!["width"]);
    }

    #[test]
    fn parses_runtime_feature_metadata() {
        let program = compile_to_core_program_via_runtime(
            r#"
            (model
              (params (number width 12))
              (feature body :role "shell" :params (width) (box width 8 6)))
            "#,
        )
        .expect("compile");

        let feature = program.feature_decls.get("body").expect("feature metadata");
        assert_eq!(feature.feature_id, "body");
        assert_eq!(feature.role, "shell");
        assert_eq!(feature.param_keys, vec!["width"]);
    }

    #[test]
    fn emits_feature_metadata() {
        let source = compile_to_legacy_source(
            r#"
            (model
              (params (number width 12))
              (feature body :role shell :params (width) (box width 8 6)))
            "#,
        )
        .expect("compile");

        assert!(
            source.contains("(feature body :role \"shell\" :params (width)"),
            "{source}"
        );
    }

    #[test]
    fn parses_verify_metadata() {
        let program = compile_to_core_program(
            r#"
            (model
              (verify
                (tag front_entrance body.front_window_1)
                (metric front_overlap (projection-overlap lid.front_skirt body.front_window_1 :axis x))
                (expect front_overlap (> 3)))
              (part body (box 10 8 6)))
            "#,
        )
        .expect("compile");

        assert_eq!(program.constraints.verify_clauses.len(), 1);
        let clause = &program.constraints.verify_clauses[0];
        assert_eq!(
            clause.tag.items,
            vec![
                CoreVerifyValue::Symbol("front_entrance".into()),
                CoreVerifyValue::Symbol("body.front_window_1".into()),
            ]
        );
        assert_eq!(
            clause.metric.items,
            vec![
                CoreVerifyValue::Symbol("front_overlap".into()),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol("projection-overlap".into()),
                    CoreVerifyValue::Symbol("lid.front_skirt".into()),
                    CoreVerifyValue::Symbol("body.front_window_1".into()),
                    CoreVerifyValue::Symbol("#:axis".into()),
                    CoreVerifyValue::Symbol("x".into()),
                ]),
            ]
        );
        assert_eq!(
            clause.expect.items,
            vec![
                CoreVerifyValue::Symbol("front_overlap".into()),
                CoreVerifyValue::List(vec![
                    CoreVerifyValue::Symbol(">".into()),
                    CoreVerifyValue::Number(3.0),
                ]),
            ]
        );
    }

    #[test]
    fn parses_runtime_verify_metadata() {
        let program = compile_to_core_program_via_runtime(
            r#"
            (model
              (verify
                (tag front_entrance body.front_window_1)
                (metric front_overlap (projection-overlap lid.front_skirt body.front_window_1 :axis x))
                (expect front_overlap (> 3)))
              (part body (box 10 8 6)))
            "#,
        )
        .expect("compile");

        assert_eq!(program.constraints.verify_clauses.len(), 1);
    }

    #[test]
    fn emits_verify_metadata() {
        let source = compile_to_legacy_source(
            r#"
            (model
              (verify
                (tag front_entrance body.front_window_1)
                (metric front_overlap (projection-overlap lid.front_skirt body.front_window_1 :axis x))
                (expect front_overlap (> 3)))
              (part body (box 10 8 6)))
            "#,
        )
        .expect("compile");

        assert!(
            source.contains(
                "(verify (tag front_entrance body.front_window_1) (metric front_overlap (projection-overlap lid.front_skirt body.front_window_1 #:axis x)) (expect front_overlap (> 3)))"
            ),
            "{source}"
        );
    }

    #[test]
    fn rejects_empty_verify_metadata() {
        let err = compile_to_core_program(
            r#"
            (model
              (verify)
              (part body (box 10 8 6)))
            "#,
        )
        .expect_err("empty verify rejected");

        assert!(err
            .to_string()
            .contains("Verify clause expects `(tag ...)`, `(metric ...)`, and `(expect ...)`."));
    }

    #[test]
    fn rejects_nested_verify_metadata() {
        let err = compile_to_core_program(
            r#"
            (model
              (part body
                (union
                  (verify
                    (tag front_entrance body.front_window_1)
                    (metric front_overlap (projection-overlap lid.front_skirt body.front_window_1 :axis x))
                    (expect front_overlap (> 3)))
                  (box 10 8 6))))
            "#,
        )
        .expect_err("nested verify rejected");

        assert!(matches!(
            err.kind,
            CompilerErrorKind::UnsupportedFeature | CompilerErrorKind::Parse
        ));
    }

    #[test]
    fn compiles_organic_loop_bspline_fixture_with_seeded_closed_profile() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (params (number seed 17 :label "Seed" :min 0 :max 99))
              (part body
                (let ((rim (organic-loop 24 22 4 seed)))
                  (extrude
                    (bspline rim :closed #t)
                    6))))
            "#,
        )
        .expect("compile organic bspline fixture");

        assert_eq!(program.parameters.len(), 1);
        assert_eq!(program.parts.len(), 1);
        let CoreNodeKind::Let { bindings, body } = &program.parts[0].root.kind else {
            panic!(
                "expected seeded rim let, got {:?}",
                program.parts[0].root.kind
            );
        };
        assert_eq!(bindings.len(), 1);
        assert!(bindings[0].name.contains("rim"), "{}", bindings[0].name);
        assert_point_list(&bindings[0].value, 24, CoreValueKind::Point2, "organic rim");
        assert_eq!(count_custom_calls(&bindings[0].value, "hash-signed"), 48);

        let CoreNodeKind::Call { op, args, .. } = &body.kind else {
            panic!("expected extrude body, got {:?}", body.kind);
        };
        assert!(matches!(op, CoreOperation::Surface(CoreSurfaceOp::Extrude)));
        let CoreNodeKind::Call {
            op: bspline_op,
            args: bspline_args,
            keywords,
        } = &args[0].kind
        else {
            panic!("expected bspline profile, got {:?}", args[0].kind);
        };
        assert!(matches!(
            bspline_op,
            CoreOperation::Path(CorePathOp::Bspline)
        ));
        assert!(matches!(
            bspline_args[0].kind,
            CoreNodeKind::Reference(CoreReference::Local(ref name)) if name == &bindings[0].name
        ));
        let closed = keywords
            .iter()
            .find(|keyword| keyword.name == "closed")
            .expect("closed keyword");
        assert!(matches!(
            closed.source_node().kind,
            CoreNodeKind::Literal(CoreLiteral::Boolean(true))
        ));
    }

    #[test]
    fn compiles_voronoi_cell_fixture_for_sites_and_perforated_panel() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (params
                (number seed 23 :label "Seed" :min 0 :max 99)
                (number cell-count 12 :label "Cell count" :min 4 :max 24 :step 1))
              (part sites
                (extrude
                  (polygon (voronoi-cells 3 4 14 12 2 seed))
                  1))
              (part panel
                (build
                  (shape plate (box 72 48 4 :align '(center center min)))
                  (result
                    (difference
                      plate
                      (apply union
                        (map
                          (lambda (cell)
                            (let* ((col (- cell (* 4 (floor (/ cell 4)))))
                                   (row (floor (/ cell 4)))
                                   (x (* (- col 1.5) 14))
                                   (y (* (- row 1.0) 12))
                                   (jx (+ x (* 2.4 (hash-signed col row seed))))
                                   (jy (+ y (* 2.4 (hash-signed (+ col 19.19) (+ row 7.73) seed))))
                                   (r (+ 2.2 (* 1.1 (voronoi2 (/ jx 14.0) (/ jy 12.0) seed)))))
                              (translate jx jy 0
                                (cylinder r 8 24))))
                          (range 0 cell-count))))))))
            "#,
        )
        .expect("compile voronoi cell fixture");

        assert_eq!(program.parts.len(), 2);
        let CoreNodeKind::Call {
            op: site_extrude,
            args: site_args,
            ..
        } = &program.parts[0].root.kind
        else {
            panic!(
                "expected sites extrude, got {:?}",
                program.parts[0].root.kind
            );
        };
        assert!(matches!(
            site_extrude,
            CoreOperation::Surface(CoreSurfaceOp::Extrude)
        ));
        let CoreNodeKind::Call {
            op: polygon_op,
            args: polygon_args,
            ..
        } = &site_args[0].kind
        else {
            panic!("expected polygon sites, got {:?}", site_args[0].kind);
        };
        assert!(matches!(
            polygon_op,
            CoreOperation::Primitive(CorePrimitive::Polygon)
        ));
        assert_point_list(&polygon_args[0], 12, CoreValueKind::Point2, "voronoi sites");
        assert_eq!(count_custom_calls(&polygon_args[0], "hash-signed"), 24);

        let CoreNodeKind::Build { bindings, result } = &program.parts[1].root.kind else {
            panic!("expected panel build, got {:?}", program.parts[1].root.kind);
        };
        assert_eq!(bindings.len(), 1);
        let CoreNodeKind::Call {
            op: difference_op,
            args: difference_args,
            ..
        } = &result.kind
        else {
            panic!("expected panel difference, got {:?}", result.kind);
        };
        assert!(matches!(
            difference_op,
            CoreOperation::Boolean(CoreBooleanOp::Difference)
        ));
        let CoreNodeKind::Apply {
            op: apply_op, list, ..
        } = &difference_args[1].kind
        else {
            panic!(
                "expected apply union cutters, got {:?}",
                difference_args[1].kind
            );
        };
        assert!(matches!(
            apply_op,
            CoreOperation::Boolean(CoreBooleanOp::Union)
        ));
        let CoreNodeKind::Map {
            params,
            sources,
            body,
        } = &list.kind
        else {
            panic!("expected mapped cutters, got {:?}", list.kind);
        };
        assert_eq!(params.len(), 1);
        assert!(params[0].contains("cell"), "{}", params[0]);
        assert_eq!(sources.len(), 1);
        assert!(matches!(sources[0].kind, CoreNodeKind::Range { .. }));
        assert_eq!(count_custom_calls(body, "hash-signed"), 2);
        assert_eq!(count_custom_calls(body, "voronoi2"), 1);
    }

    #[test]
    fn compiles_chaotic_point_helpers_with_literal_counts_on_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (part body
                (list
                  (lorenz-points 4 0.01 10)
                  (rossler-points 5 0.05 8)
                  (logistic-bifurcation-points 3 2 4 6)
                  (henon-points 7 9))))
            "#,
        )
        .expect("compile");

        let root = &program.parts[0].root;
        let CoreNodeKind::List(groups) = &root.kind else {
            panic!("expected root list, got {:?}", root.kind);
        };
        assert_eq!(groups.len(), 4);

        let CoreNodeKind::List(lorenz) = &groups[0].kind else {
            panic!("expected lorenz points, got {:?}", groups[0].kind);
        };
        assert_eq!(lorenz.len(), 4);
        assert_eq!(lorenz[0].value_kind, CoreValueKind::Point3);

        let CoreNodeKind::List(rossler) = &groups[1].kind else {
            panic!("expected rossler points, got {:?}", groups[1].kind);
        };
        assert_eq!(rossler.len(), 5);
        assert_eq!(rossler[0].value_kind, CoreValueKind::Point3);

        let CoreNodeKind::List(logistic) = &groups[2].kind else {
            panic!("expected logistic points, got {:?}", groups[2].kind);
        };
        assert_eq!(logistic.len(), 6);
        assert_eq!(logistic[0].value_kind, CoreValueKind::Point2);

        let CoreNodeKind::List(henon) = &groups[3].kind else {
            panic!("expected henon points, got {:?}", groups[3].kind);
        };
        assert_eq!(henon.len(), 7);
        assert_eq!(henon[0].value_kind, CoreValueKind::Point2);
    }

    #[test]
    fn compiles_chaotic_helpers_into_bounded_geometry_fixtures() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (part attractor-path
                (path (lorenz-points 9 0.01 18)))
              (part henon-ridge
                (extrude
                  (bspline (henon-points 16 12) :closed #f)
                  2))
              (part chaotic-cloud
                (list
                  (rossler-points 7 0.05 14)
                  (logistic-bifurcation-points 4 3 6 10))))
            "#,
        )
        .expect("compile chaotic geometry fixtures");

        assert_eq!(program.parts.len(), 3);
        let CoreNodeKind::Call {
            op: path_op,
            args: path_args,
            ..
        } = &program.parts[0].root.kind
        else {
            panic!(
                "expected attractor path call, got {:?}",
                program.parts[0].root.kind
            );
        };
        assert!(matches!(path_op, CoreOperation::Path(CorePathOp::Polyline)));
        assert_point_list(&path_args[0], 9, CoreValueKind::Point3, "lorenz path");
        assert_eq!(count_custom_calls(&path_args[0], "clamp"), 27);

        let CoreNodeKind::Call {
            op: ridge_op,
            args: ridge_args,
            ..
        } = &program.parts[1].root.kind
        else {
            panic!(
                "expected henon ridge extrude, got {:?}",
                program.parts[1].root.kind
            );
        };
        assert!(matches!(
            ridge_op,
            CoreOperation::Surface(CoreSurfaceOp::Extrude)
        ));
        let CoreNodeKind::Call {
            op: ridge_profile_op,
            args: ridge_profile_args,
            ..
        } = &ridge_args[0].kind
        else {
            panic!("expected henon bspline, got {:?}", ridge_args[0].kind);
        };
        assert!(matches!(
            ridge_profile_op,
            CoreOperation::Path(CorePathOp::Bspline)
        ));
        assert_point_list(
            &ridge_profile_args[0],
            16,
            CoreValueKind::Point2,
            "henon ridge",
        );
        assert_eq!(count_custom_calls(&ridge_profile_args[0], "clamp"), 32);

        let CoreNodeKind::List(groups) = &program.parts[2].root.kind else {
            panic!(
                "expected chaotic cloud lists, got {:?}",
                program.parts[2].root.kind
            );
        };
        assert_eq!(groups.len(), 2);
        assert_point_list(&groups[0], 7, CoreValueKind::Point3, "rossler cloud");
        assert_point_list(&groups[1], 12, CoreValueKind::Point2, "logistic cloud");
        assert_eq!(count_custom_calls(&groups[0], "clamp"), 21);
        assert_eq!(count_custom_calls(&groups[1], "clamp"), 24);
    }

    #[test]
    fn compiles_filter_fold_and_reduce_builtins_on_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (part body
                (list
                  (filter (lambda (i) (< i 3)) (range 0 5))
                  (fold + 0 (range 1 4))
                  (reduce + 0 (range 1 4)))))
            "#,
        )
        .expect("compile");

        let root = &program.parts[0].root;
        let CoreNodeKind::List(groups) = &root.kind else {
            panic!("expected root list, got {:?}", root.kind);
        };
        assert_eq!(groups.len(), 3);

        let CoreNodeKind::List(filtered) = &groups[0].kind else {
            panic!("expected filtered list, got {:?}", groups[0].kind);
        };
        assert_eq!(filtered.len(), 3);
        assert!(matches!(
            filtered[0].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(0.0))
        ));
        assert!(matches!(
            filtered[2].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(2.0))
        ));

        assert_eq!(
            emit_node(&groups[1], &BTreeMap::new(), &BTreeMap::new()),
            "(+ 3 (+ 2 (+ 1 0)))"
        );
        assert_eq!(
            emit_node(&groups[2], &BTreeMap::new(), &BTreeMap::new()),
            "(+ 3 (+ 2 (+ 1 0)))"
        );
    }

    #[test]
    fn reports_sequence_diagnostics_with_op_context() {
        let map_type_err =
            compile_to_core_program_from_expanded_ast("(model (part body (map (lambda (x) x) 1)))")
                .expect_err("map type mismatch");
        assert!(map_type_err.to_string().contains("`map`"));
        assert!(map_type_err.to_string().contains("expected list"));
        assert!(map_type_err.to_string().contains("got number"));

        let map_arity_err = compile_to_core_program_from_expanded_ast(
            "(model (part body (map (lambda (x) x) (range 0 2) (range 0 2))))",
        )
        .expect_err("map arity mismatch");
        assert!(map_arity_err.to_string().contains("`map`"));
        assert!(map_arity_err.to_string().contains("expected arity 2"));
        assert!(map_arity_err.to_string().contains("got arity 1"));

        let filter_type_err = compile_to_core_program_from_expanded_ast(
            "(model (part body (filter (lambda (x) x) (range 0 2))))",
        )
        .expect_err("filter boolean mismatch");
        assert!(filter_type_err.to_string().contains("`filter`"));
        assert!(filter_type_err.to_string().contains("expected boolean"));
        assert!(filter_type_err.to_string().contains("got number"));
    }

    #[test]
    fn compiles_dynamic_tooth_apply_map_on_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (params
                (number num-teeth 4)
                (number pitch 5)
                (number dz 20)
                (number length 80))
              (part teeth
                (build
                  (shape tooth
                    (box 2 4 1))
                  (shape num-teeth
                    (max 0 (floor (/ length pitch))))
                  (result
                    (apply union
                      (map
                        (lambda (i)
                          (let* ((x (* (+ i 0.5) pitch))
                                 (slope (* (/ dz length)
                                           (sin (* pi (/ x length)))))
                                 (angle-deg (rad (atan slope)))
                                 (angle2-deg (rad (atan2 dz length))))
                            (translate x 0 0
                              (rotate 0 (+ angle-deg angle2-deg) 0 tooth))))
                        (range 0 num-teeth)))))))
            "#,
        )
        .expect("compile dynamic tooth math");

        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();
        let rendered = emit_node(&program.parts[0].root, &param_names, &BTreeMap::new());
        assert!(rendered.contains("(apply union"), "{rendered}");
        assert!(rendered.contains("(map (lambda ("), "{rendered}");
        assert!(rendered.contains("(range 0 num-teeth)"), "{rendered}");
        assert!(rendered.contains("(floor (/ length pitch))"), "{rendered}");
        assert!(rendered.contains("(atan "), "{rendered}");
        assert!(rendered.contains("(atan2 dz length)"), "{rendered}");
    }

    #[test]
    fn compiles_align_and_plane_location_on_expanded_ast_path() {
        let program = compile_to_core_program_from_expanded_ast(
            r#"
            (model
              (part body
                (build
                  (shape base-plane
                    (plane :origin '(1 2 3) :x '(1 0 0) :normal '(0 0 1)))
                  (shape peg
                    (box 4 6 8 :align '(min center max)))
                  (result
                    (place
                      (location base-plane :offset '(5 0 0) :rotate '(0 90 0))
                      peg)))))
            "#,
        )
        .expect("compile");

        let root = &program.parts[0].root;
        let CoreNodeKind::Build { bindings, result } = &root.kind else {
            panic!("expected build, got {:?}", root.kind);
        };
        assert_eq!(bindings.len(), 2);
        let CoreNodeKind::Call { op, keywords, .. } = &bindings[0].value.kind else {
            panic!("expected plane call, got {:?}", bindings[0].value.kind);
        };
        assert!(matches!(op, CoreOperation::Frame(CoreFrameOp::Plane)));
        assert_eq!(keywords.len(), 3);

        let CoreNodeKind::Call { op, keywords, .. } = &bindings[1].value.kind else {
            panic!("expected box call, got {:?}", bindings[1].value.kind);
        };
        assert!(matches!(op, CoreOperation::Primitive(CorePrimitive::Box)));
        assert_eq!(keywords.len(), 1);

        let rendered = emit_node(root, &BTreeMap::new(), &BTreeMap::new());
        assert!(rendered.contains("(plane :origin (1 2 3) :x (1 0 0) :normal (0 0 1))"));
        assert!(rendered.contains("(box 4 6 8 :align (min center max))"));
        assert!(rendered.contains("(location base-plane :offset (5 0 0) :rotate (0 90 0))"));

        let CoreNodeKind::Call { op, .. } = &result.kind else {
            panic!("expected place call, got {:?}", result.kind);
        };
        assert!(matches!(op, CoreOperation::Frame(CoreFrameOp::Place)));
    }

    #[test]
    fn compiles_typed_hole_placeholder_in_expression_position() {
        let program = compile_to_core_program(
            r#"
            (model
              (part body
                (difference
                  (hole :type Solid :goal "snap clip outer body")
                  (box 1 1 1))))
            "#,
        )
        .expect("compile typed hole");

        let CoreNodeKind::Call { op, args, .. } = &program.parts[0].root.kind else {
            panic!("expected difference call");
        };
        assert!(matches!(
            op,
            CoreOperation::Boolean(CoreBooleanOp::Difference)
        ));
        assert_eq!(args[0].value_kind, CoreValueKind::Solid);
        let CoreNodeKind::Call {
            op: hole_op,
            keywords,
            ..
        } = &args[0].kind
        else {
            panic!("expected hole call");
        };
        assert!(matches!(hole_op, CoreOperation::Custom(name) if name == "hole"));
        assert_eq!(keywords.len(), 2);
    }

    #[test]
    fn compiles_helical_ridge_as_custom_surface_with_args_and_keywords() {
        let program = compile_to_core_program(
            r#"
            (model
              (part thread
                (helical-ridge
                  :radius 18
                  :pitch 3
                  :height 24
                  :base-width 1.2
                  :crest-width 0.35
                  :depth 0.6
                  :female #t
                  :clearance 0.15
                  :lefthand #t)))
            "#,
        )
        .expect("compile helical ridge");

        let root = &program.parts[0].root;
        let CoreNodeKind::Call { op, args, keywords } = &root.kind else {
            panic!("expected helical-ridge call, got {:?}", root.kind);
        };

        assert!(matches!(op, CoreOperation::Custom(name) if name == "helical-ridge"));
        assert_eq!(root.value_kind, CoreValueKind::Solid);
        assert!(
            root.span.is_some(),
            "helical-ridge call should keep source span"
        );
        assert!(args.is_empty());

        let names = keywords
            .iter()
            .map(|keyword| keyword.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "radius",
                "pitch",
                "height",
                "base-width",
                "crest-width",
                "depth",
                "female",
                "clearance",
                "lefthand"
            ]
        );

        let numeric_values = keywords
            .iter()
            .filter_map(|keyword| match &keyword.source_node().kind {
                CoreNodeKind::Literal(CoreLiteral::Number(value)) => Some(*value),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(numeric_values, vec![18.0, 3.0, 24.0, 1.2, 0.35, 0.6, 0.15]);
        assert_eq!(
            emit_node(root, &BTreeMap::new(), &BTreeMap::new()),
            "(helical-ridge :radius 18 :pitch 3 :height 24 :base-width 1.2 :crest-width 0.35 :depth 0.6 :female #t :clearance 0.15 :lefthand #t)"
        );
        assert!(
            keywords
                .iter()
                .all(|keyword| keyword.source_node().span.is_some()),
            "keyword value nodes should keep source spans"
        );
    }

    #[test]
    fn compiles_exact_edge_selector_keyword_payload() {
        let program = compile_to_core_program(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "target-id:body:edge:0:0-0-0_0-0-10"
                  (box 10 10 10))))
            "#,
        )
        .expect("compile");
        let CoreNodeKind::Call { keywords, .. } = &program.parts[0].root.kind else {
            panic!("expected call");
        };
        assert_eq!(keywords.len(), 1);
        assert_eq!(
            keywords[0].selector_payload(),
            Some(CoreSelectorPayload::EdgeTargetIds(vec![
                "body:edge:0:0-0-0_0-0-10".into()
            ]))
            .as_ref()
        );
    }

    #[test]
    fn compiles_coarse_edge_selector_keyword_payload() {
        let program = compile_to_core_program(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "left+vertical"
                  (box 10 10 10))))
            "#,
        )
        .expect("compile");
        let CoreNodeKind::Call { keywords, .. } = &program.parts[0].root.kind else {
            panic!("expected call");
        };
        assert_eq!(
            keywords[0].selector_payload(),
            Some(CoreSelectorPayload::EdgeClauses(vec![
                crate::ecky_core_ir::CoreEdgeSelectorClause::Boundary {
                    axis: crate::ecky_core_ir::CoreEdgeAxis::X,
                    bound: crate::ecky_core_ir::CoreEdgeBound::Min,
                },
                crate::ecky_core_ir::CoreEdgeSelectorClause::Axis(
                    crate::ecky_core_ir::CoreEdgeAxis::Z,
                ),
            ]))
            .as_ref()
        );
    }

    #[test]
    fn compiles_exact_face_selector_keyword_payload() {
        let program = compile_to_core_program(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "target-id:body:face:0:0-0-10:400"
                  (box 10 10 10))))
            "#,
        )
        .expect("compile");
        let CoreNodeKind::Call { keywords, .. } = &program.parts[0].root.kind else {
            panic!("expected call");
        };
        assert_eq!(keywords.len(), 1);
        assert_eq!(
            keywords[0].selector_payload(),
            Some(CoreSelectorPayload::FaceTargetIds(vec![
                "body:face:0:0-0-10:400".into()
            ]))
            .as_ref()
        );
    }

    #[test]
    fn compiles_coarse_face_selector_keyword_payload() {
        let program = compile_to_core_program(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "top"
                  (box 10 10 10))))
            "#,
        )
        .expect("compile");
        let CoreNodeKind::Call { keywords, .. } = &program.parts[0].root.kind else {
            panic!("expected call");
        };
        assert_eq!(keywords.len(), 1);
        assert_eq!(
            keywords[0].selector_payload(),
            Some(CoreSelectorPayload::FaceClauses(vec![
                crate::ecky_core_ir::CoreFaceSelectorClause::Boundary {
                    axis: crate::ecky_core_ir::CoreEdgeAxis::Z,
                    bound: crate::ecky_core_ir::CoreEdgeBound::Max,
                },
            ]))
            .as_ref()
        );
    }

    #[test]
    fn compiles_richer_face_selector_keyword_payload() {
        let program = compile_to_core_program(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "planar+normal-z+area-max"
                  (box 10 10 10))))
            "#,
        )
        .expect("compile");
        let CoreNodeKind::Call { keywords, .. } = &program.parts[0].root.kind else {
            panic!("expected call");
        };
        assert_eq!(keywords.len(), 1);
        assert_eq!(
            keywords[0].selector_payload(),
            Some(CoreSelectorPayload::FaceClauses(vec![
                crate::ecky_core_ir::CoreFaceSelectorClause::Planar,
                crate::ecky_core_ir::CoreFaceSelectorClause::Normal(
                    crate::ecky_core_ir::CoreEdgeAxis::Z,
                ),
                crate::ecky_core_ir::CoreFaceSelectorClause::Area(
                    crate::ecky_core_ir::CoreFaceAreaRank::Max,
                ),
            ]))
            .as_ref()
        );
    }

    #[test]
    fn clone_node_with_fresh_ids_preserves_selector_payload() {
        let program = compile_to_core_program(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "target-id:body:edge:0:0-0-0_0-0-10"
                  (box 10 10 10))))
            "#,
        )
        .expect("compile");
        let mut next_node = 10_000;
        let cloned = clone_node_with_fresh_ids(&program.parts[0].root, &mut next_node);
        let CoreNodeKind::Call { keywords, .. } = &cloned.kind else {
            panic!("expected call");
        };
        assert_eq!(
            keywords[0].selector_payload(),
            Some(CoreSelectorPayload::EdgeTargetIds(vec![
                "body:edge:0:0-0-0_0-0-10".into()
            ]))
            .as_ref()
        );
    }

    #[test]
    fn typed_hole_type_mismatch_fails_typecheck() {
        let err = compile_to_core_program(
            r#"
            (model
              (part body
                (extrude (hole :type solid :goal "wrong profile") 5)))
            "#,
        )
        .expect_err("solid hole cannot be extruded as sketch");

        let message = err.to_string();
        assert!(message.contains("extrude"), "{message}");
        assert!(message.contains("sketch"), "{message}");
        assert!(message.contains("solid"), "{message}");
    }

    #[test]
    fn component_clause_roles_cover_model_part_feature() {
        // T1 representation: model -> Root, part/feature -> Output, define-component -> Library (T2).
        let root = ComponentClause {
            role: ComponentRole::Root,
            spelling: "model".to_string(),
        };
        let part = ComponentClause {
            role: ComponentRole::Output,
            spelling: "part".to_string(),
        };
        let feature = ComponentClause {
            role: ComponentRole::Output,
            spelling: "feature".to_string(),
        };
        assert_eq!(root.role, ComponentRole::Root);
        assert_eq!(part.role, ComponentRole::Output);
        assert_eq!(feature.role, ComponentRole::Output);
        assert_ne!(ComponentRole::Library, ComponentRole::Output);
    }

    #[test]
    fn component_emit_preserves_part_spelling() {
        let source = "(model (part my_part (box 1 1 1)))";
        let program = compile_to_core_program(source).expect("compile");
        let emitted = emit_program(&program);

        assert!(
            emitted.contains("(part my_part"),
            "emitted should preserve 'part' spelling: {emitted}"
        );
        assert!(
            !emitted.contains("(feature my_part"),
            "should not emit as feature"
        );
    }

    #[test]
    fn component_emit_preserves_feature_spelling() {
        let source = r#"
            (model
              (part body (box 1 1 1))
              (feature my_feature :role "chamfer" (box 1 1 1)))
        "#;
        let program = compile_to_core_program(source).expect("compile");
        let emitted = emit_program(&program);

        assert!(
            emitted.contains("(feature my_feature"),
            "emitted should preserve 'feature' spelling: {emitted}"
        );
    }

    #[test]
    fn component_roundtrip_preserves_spellings() {
        let source = r#"
            (model
              (part shell (box 2 2 2))
              (part core (box 1 1 1))
              (feature trim :role "finish" (box 1 1 1)))
        "#;
        let program = compile_to_core_program(source).expect("compile");
        let emitted = emit_program(&program);
        let program2 = compile_to_core_program(&emitted).expect("recompile");

        // Core structure must be identical
        assert_eq!(
            program.parts.len(),
            program2.parts.len(),
            "part count must match"
        );
        assert_eq!(
            program.parts[0].key, program2.parts[0].key,
            "part 0 key must match"
        );
        assert_eq!(
            program.parts[1].key, program2.parts[1].key,
            "part 1 key must match"
        );
        assert_eq!(
            program.parts[2].key, program2.parts[2].key,
            "part 2 key must match"
        );

        // Emitted source must preserve spellings
        let emitted2 = emit_program(&program2);
        assert!(
            emitted.contains("(part shell"),
            "first emit should have 'part shell'"
        );
        assert!(
            emitted2.contains("(part shell"),
            "second emit should preserve 'part shell'"
        );
        assert!(
            emitted.contains("(feature trim"),
            "first emit should have 'feature trim'"
        );
        assert!(
            emitted2.contains("(feature trim"),
            "second emit should preserve 'feature trim'"
        );
    }

    // --- T2: define-component and instantiation ---

    fn component_let_bindings(node: &CoreNode) -> &[CoreBinding] {
        let CoreNodeKind::Let { bindings, .. } = &node.kind else {
            panic!("expected component expansion let, got {:?}", node.kind);
        };
        bindings
    }

    fn binding_number(bindings: &[CoreBinding], name: &str) -> f64 {
        let demangle = |raw: &str| -> String {
            let raw = normalize_hygienic_op_name(raw);
            raw.trim_start_matches('#')
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .to_string()
        };
        let binding = bindings
            .iter()
            .find(|binding| demangle(&binding.name) == name)
            .unwrap_or_else(|| panic!("missing binding `{name}` in {bindings:?}"));
        match &binding.value.kind {
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(value)) => *value,
            other => panic!("binding `{name}` is not a number literal: {other:?}"),
        }
    }

    fn collect_node_ids(node: &CoreNode, seen: &mut Vec<u64>) {
        seen.push(node.id.raw());
        match &node.kind {
            CoreNodeKind::Call { args, .. } => {
                for arg in args {
                    collect_node_ids(arg, seen);
                }
            }
            CoreNodeKind::Let { bindings, body } => {
                for binding in bindings {
                    collect_node_ids(&binding.value, seen);
                }
                collect_node_ids(body, seen);
            }
            CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
                for item in items {
                    collect_node_ids(item, seen);
                }
            }
            CoreNodeKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_node_ids(condition, seen);
                collect_node_ids(then_branch, seen);
                collect_node_ids(else_branch, seen);
            }
            _ => {}
        }
    }

    #[test]
    fn define_component_instantiation_applies_defaults_and_overrides() {
        let program = compile_to_core_program(
            r#"
            (define-component knuckle
              ((number pin_d 8) (number clearance 0.3))
              (difference
                (cylinder (* 2 pin_d) 10 96)
                (cylinder (+ pin_d clearance) 12 96)))
            (model
              (part hinge_a (knuckle :pin_d 6))
              (part hinge_b (knuckle)))
            "#,
        )
        .expect("compile");

        assert_eq!(program.parts.len(), 2);
        assert_eq!(program.parts[0].key, "hinge_a");
        assert_eq!(program.parts[1].key, "hinge_b");

        let a_bindings = component_let_bindings(&program.parts[0].root);
        assert_eq!(binding_number(a_bindings, "pin_d"), 6.0);
        assert_eq!(binding_number(a_bindings, "clearance"), 0.3);

        let b_bindings = component_let_bindings(&program.parts[1].root);
        assert_eq!(binding_number(b_bindings, "pin_d"), 8.0);
        assert_eq!(binding_number(b_bindings, "clearance"), 0.3);
    }

    #[test]
    fn define_component_inside_model_is_supported() {
        let program = compile_to_core_program(
            r#"
            (model
              (define-component stub ((number size 4)) (box size size size))
              (part body (stub :size 2)))
            "#,
        )
        .expect("compile");

        assert_eq!(program.parts.len(), 1);
        let bindings = component_let_bindings(&program.parts[0].root);
        assert_eq!(binding_number(bindings, "size"), 2.0);
    }

    #[test]
    fn define_component_signature_accepts_param_metadata() {
        let program = compile_to_core_program(
            r#"
            (define-component knob
              ((number d 10 :label "Diameter" :min 4 :max 20 :step 0.5))
              (cylinder d 5 64))
            (model (part body (knob :d 12)))
            "#,
        )
        .expect("compile");
        let bindings = component_let_bindings(&program.parts[0].root);
        assert_eq!(binding_number(bindings, "d"), 12.0);
    }

    #[test]
    fn component_instantiation_rejects_unknown_keyword() {
        let err = compile_to_core_program(
            r#"
            (define-component knuckle ((number pin_d 8)) (cylinder pin_d 10 96))
            (model (part a (knuckle :bogus 1)))
            "#,
        )
        .expect_err("unknown keyword rejected");
        let message = err.to_string();
        assert!(message.contains("knuckle"), "{message}");
        assert!(message.contains("bogus"), "{message}");
        assert!(
            message.contains("pin_d"),
            "signature must be named: {message}"
        );
    }

    #[test]
    fn component_instantiation_rejects_missing_required_argument() {
        let err = compile_to_core_program(
            r#"
            (define-component knuckle ((number pin_d)) (cylinder pin_d 10 96))
            (model (part a (knuckle)))
            "#,
        )
        .expect_err("missing required rejected");
        let message = err.to_string();
        assert!(message.contains("knuckle"), "{message}");
        assert!(message.contains("pin_d"), "{message}");
    }

    #[test]
    fn component_instantiation_rejects_positional_arguments() {
        let err = compile_to_core_program(
            r#"
            (define-component knuckle ((number pin_d 8)) (cylinder pin_d 10 96))
            (model (part a (knuckle 6)))
            "#,
        )
        .expect_err("positional args rejected");
        let message = err.to_string();
        assert!(message.contains("knuckle"), "{message}");
        assert!(message.contains("keyword"), "{message}");
    }

    #[test]
    fn component_body_rejects_free_variables() {
        let err = compile_to_core_program(
            r#"
            (model
              (params (number width 12))
              (define-component leak ((number d 2)) (box d d width))
              (part a (leak)))
            "#,
        )
        .expect_err("free variable rejected");
        let message = err.to_string();
        assert!(message.contains("leak"), "{message}");
        assert!(message.contains("width"), "{message}");
    }

    #[test]
    fn component_body_allows_local_bindings_and_builtins() {
        let program = compile_to_core_program(
            r#"
            (define-component lug ((number d 4))
              (let* ((r (/ d 2))
                     (h (* r 3)))
                (translate 0 0 (max r 1) (cylinder r h 48))))
            (model (part body (lug :d 6)))
            "#,
        )
        .expect("locals and builtins are not free variables");
        assert_eq!(program.parts.len(), 1);
    }

    #[test]
    fn components_nest_and_allocate_fresh_node_ids() {
        let program = compile_to_core_program(
            r#"
            (define-component pin ((number d 2)) (cylinder d 10 48))
            (define-component pair ((number d 2))
              (union
                (pin :d d)
                (translate 5 0 0 (pin :d d))))
            (model
              (part left (pair :d 3))
              (part right (pair)))
            "#,
        )
        .expect("nested components compile");

        assert_eq!(program.parts.len(), 2);
        let mut ids = Vec::new();
        collect_node_ids(&program.parts[0].root, &mut ids);
        collect_node_ids(&program.parts[1].root, &mut ids);
        let unique: BTreeSet<u64> = ids.iter().copied().collect();
        assert_eq!(
            unique.len(),
            ids.len(),
            "expanded node ids must be fresh per instance"
        );
    }

    #[test]
    fn component_expansion_anchors_at_call_site() {
        let program = compile_to_core_program(
            r#"
            (define-component pin ((number d 2)) (cylinder d 10 48))
            (model (part a (pin :d 3)) (part b (pin)))
            "#,
        )
        .expect("compile");
        assert!(
            program.parts[0].root.span.is_some(),
            "expansion root must carry the call-site span"
        );
        assert!(program.parts[1].root.span.is_some());
        assert_ne!(
            program.parts[0].root.id, program.parts[1].root.id,
            "each instantiation expands with its own nodes"
        );
    }

    #[test]
    fn component_self_recursion_errors_deterministically() {
        let err = compile_to_core_program(
            r#"
            (define-component loop ((number d 2)) (union (loop :d d)))
            (model (part a (loop)))
            "#,
        )
        .expect_err("self recursion rejected");
        let message = err.to_string();
        assert!(message.contains("loop"), "{message}");
        assert!(
            message.contains("cycle") || message.contains("recursi"),
            "{message}"
        );
    }

    #[test]
    fn component_mutual_recursion_errors_deterministically() {
        let err = compile_to_core_program(
            r#"
            (define-component ping ((number d 2)) (union (pong :d d)))
            (define-component pong ((number d 2)) (union (ping :d d)))
            (model (part a (ping)))
            "#,
        )
        .expect_err("mutual recursion rejected");
        let message = err.to_string();
        assert!(
            message.contains("cycle") || message.contains("recursi"),
            "{message}"
        );
    }

    #[test]
    fn component_nesting_depth_is_capped() {
        let mut source = String::new();
        source.push_str("(define-component c0 ((number d 2)) (box d d d))\n");
        for level in 1..=33 {
            source.push_str(&format!(
                "(define-component c{} ((number d 2)) (union (c{} :d d)))\n",
                level,
                level - 1
            ));
        }
        source.push_str("(model (part a (c33)))");
        let err = compile_to_core_program(&source).expect_err("depth cap enforced");
        let message = err.to_string();
        assert!(message.contains("depth"), "{message}");
    }

    fn parity_fold_node(node: &CoreNode, env: &BTreeMap<String, CoreNode>) -> CoreNode {
        if let Ok(value) = evaluate_core_number("parity", node, env) {
            return CoreNode::new(
                NodeId::new(0),
                CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(value)),
                CoreValueKind::Number,
            );
        }
        let kind = match &node.kind {
            CoreNodeKind::Let { bindings, body } => {
                let mut nested = env.clone();
                for binding in bindings {
                    nested.insert(binding.name.clone(), binding.value.clone());
                }
                return parity_fold_node(body, &nested);
            }
            CoreNodeKind::Reference(CoreReference::Local(name)) if env.contains_key(name) => {
                return parity_fold_node(&env[name].clone(), env);
            }
            CoreNodeKind::Call { op, args, keywords } => CoreNodeKind::Call {
                op: op.clone(),
                args: args.iter().map(|arg| parity_fold_node(arg, env)).collect(),
                keywords: keywords.clone(),
            },
            CoreNodeKind::List(items) => CoreNodeKind::List(
                items
                    .iter()
                    .map(|item| parity_fold_node(item, env))
                    .collect(),
            ),
            other => other.clone(),
        };
        let value_kind = node.value_kind;
        CoreNode::new(NodeId::new(0), kind, value_kind)
    }

    // --- T3: verify clause travel ---

    fn verify_tags(program: &CoreProgram) -> Vec<String> {
        program
            .constraints
            .verify_clauses
            .iter()
            .filter_map(|clause| match clause.tag.items.first() {
                Some(CoreVerifyValue::Symbol(tag)) => Some(tag.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn component_verify_clauses_travel_with_each_instantiation() {
        let program = compile_to_core_program(
            r#"
            (define-component knuckle
              ((number pin_d 8) (number clearance 0.3))
              (verify
                (tag clearance_check)
                (metric min_wall_thickness "body")
                (expect (>= value 1)))
              (cylinder (+ pin_d clearance) 10 96))
            (model
              (part hinge_a (knuckle :pin_d 6))
              (part hinge_b (knuckle)))
            "#,
        )
        .expect("compile");

        let tags = verify_tags(&program);
        assert!(
            tags.contains(&"hinge_a/clearance_check".to_string()),
            "expected namespaced tag for hinge_a, got {tags:?}"
        );
        assert!(
            tags.contains(&"hinge_b/clearance_check".to_string()),
            "expected namespaced tag for hinge_b, got {tags:?}"
        );
        assert_eq!(program.constraints.verify_clauses.len(), 2);
    }

    #[test]
    fn nested_component_verify_clauses_travel_to_instantiating_part() {
        let program = compile_to_core_program(
            r#"
            (define-component pin ((number d 2))
              (verify (tag pin_ok) (metric min_wall_thickness "body") (expect (>= value 1)))
              (cylinder d 10 48))
            (define-component pair ((number d 2))
              (union (pin :d d) (translate 5 0 0 (pin :d d))))
            (model (part left (pair :d 3)))
            "#,
        )
        .expect("compile");

        let tags = verify_tags(&program);
        assert_eq!(
            tags,
            vec!["left/pin_ok".to_string()],
            "nested verify must namespace by the instantiating part and dedupe identical clauses"
        );
    }

    #[test]
    fn component_verify_clauses_match_on_both_compile_paths() {
        let source = r#"
            (define-component pin ((number d 2))
              (verify (tag pin_ok) (metric min_wall_thickness "body") (expect (>= value 1)))
              (cylinder d 10 48))
            (model (part a (pin :d 3)))
        "#;
        let lowered = lower_component_definitions_source(source).expect("lowering");
        let expanded = compile_to_core_program_from_expanded_ast(&lowered).expect("expanded");
        let runtime = compile_to_core_program_via_runtime(&lowered).expect("runtime");
        assert_eq!(
            expanded.constraints.verify_clauses,
            runtime.constraints.verify_clauses
        );
        assert_eq!(verify_tags(&expanded), vec!["a/pin_ok".to_string()]);
    }

    #[test]
    fn top_level_verify_clauses_are_unchanged_by_component_lowering() {
        let program = compile_to_core_program(
            r#"
            (define-component stub ((number size 4)) (box size size size))
            (model
              (verify
                (tag body_shell)
                (metric min_wall_thickness "body")
                (expect (>= value 2)))
              (part body (stub :size 2)))
            "#,
        )
        .expect("compile");
        assert_eq!(verify_tags(&program), vec!["body_shell".to_string()]);
    }

    #[test]
    fn component_compile_paths_agree_after_static_fold() {
        let source = r#"
            (define-component knuckle
              ((number pin_d 8) (number clearance 0.3))
              (difference
                (cylinder (* 2 pin_d) 10 96)
                (cylinder (+ pin_d clearance) 12 96)))
            (model
              (part hinge_a (knuckle :pin_d 6))
              (part hinge_b (knuckle)))
        "#;
        let lowered = lower_component_definitions_source(source).expect("lowering");
        let expanded = compile_to_core_program_from_expanded_ast(&lowered).expect("expanded path");
        let runtime = compile_to_core_program_via_runtime(&lowered).expect("runtime path");

        assert_eq!(expanded.parts.len(), runtime.parts.len());
        assert_eq!(
            format!("{:?}", expanded.parameters),
            format!("{:?}", runtime.parameters)
        );
        for (left, right) in expanded.parts.iter().zip(runtime.parts.iter()) {
            assert_eq!(left.key, right.key);
            let env = BTreeMap::new();
            let folded_left = parity_fold_node(&left.root, &env);
            let folded_right = parity_fold_node(&right.root, &env);
            assert_eq!(
                format!("{:#?}", folded_left),
                format!("{:#?}", folded_right),
                "part `{}` must fold to the same structure on both paths",
                left.key
            );
        }
    }

    fn assert_point_list(node: &CoreNode, len: usize, kind: CoreValueKind, label: &str) {
        let CoreNodeKind::List(points) = &node.kind else {
            panic!("expected {label} point list, got {:?}", node.kind);
        };
        assert_eq!(points.len(), len, "{label} point count");
        assert!(
            points.iter().all(|point| point.value_kind == kind),
            "{label} point kind"
        );
    }

    fn count_custom_calls(node: &CoreNode, name: &str) -> usize {
        let here = match &node.kind {
            CoreNodeKind::Call {
                op: CoreOperation::Custom(op_name),
                ..
            } if op_name == name => 1,
            _ => 0,
        };
        here + node_children(node)
            .into_iter()
            .map(|child| count_custom_calls(child, name))
            .sum::<usize>()
    }

    fn node_children(node: &CoreNode) -> Vec<&CoreNode> {
        match &node.kind {
            CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => Vec::new(),
            CoreNodeKind::Build { bindings, result } => bindings
                .iter()
                .map(|binding| &binding.value)
                .chain(std::iter::once(result.as_ref()))
                .collect(),
            CoreNodeKind::Let { bindings, body } => bindings
                .iter()
                .map(|binding| &binding.value)
                .chain(std::iter::once(body.as_ref()))
                .collect(),
            CoreNodeKind::If {
                condition,
                then_branch,
                else_branch,
            } => vec![
                condition.as_ref(),
                then_branch.as_ref(),
                else_branch.as_ref(),
            ],
            CoreNodeKind::Call { args, keywords, .. } => args
                .iter()
                .chain(keywords.iter().map(|keyword| keyword.source_node()))
                .collect(),
            CoreNodeKind::Range { start, end } => vec![start.as_ref(), end.as_ref()],
            CoreNodeKind::Map { sources, body, .. } => sources
                .iter()
                .chain(std::iter::once(body.as_ref()))
                .collect(),
            CoreNodeKind::Apply { args, list, .. } => {
                args.iter().chain(std::iter::once(list.as_ref())).collect()
            }
            CoreNodeKind::List(items) | CoreNodeKind::Group(items) => items.iter().collect(),
        }
    }

    // T0: Compatibility lock tests — freeze current behavior before component work

    #[test]
    fn fixture_lock_captures_stable_node_keys_for_all_parts() {
        let snapshot_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/component-unification/fixture-lock.snap");
        let fixtures_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../model-runtime/examples");

        let mut entries: Vec<_> = std::fs::read_dir(&fixtures_dir)
            .expect("fixture dir exists")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "ecky")
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|e| e.path());

        let mut snapshot_content = String::new();
        for entry in entries {
            let path = entry.path();
            let fixture_name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let source = std::fs::read_to_string(&path).expect("read fixture");

            match compile_to_core_program(&source) {
                Ok(program) => {
                    snapshot_content.push_str(&format!("fixture: {}\n", fixture_name));
                    for part in &program.parts {
                        if let Some(span) = part.root.span {
                            let start = span.start as usize;
                            let end = span.end as usize;
                            if start < end && end <= source.len() {
                                if source.is_char_boundary(start) && source.is_char_boundary(end) {
                                    use sha2::{Digest, Sha256};
                                    let mut hasher = Sha256::new();
                                    hasher.update(b"freecad-part-root|");
                                    hasher.update(part.key.as_bytes());
                                    hasher.update(b"|");
                                    hasher.update(&source.as_bytes()[start..end]);
                                    let stable_key = format!("sha256:{:x}", hasher.finalize());
                                    snapshot_content.push_str(&format!(
                                        "  part: {} key: {}\n",
                                        part.key, stable_key
                                    ));
                                } else {
                                    snapshot_content.push_str(&format!(
                                        "  part: {} key: invalid-boundary\n",
                                        part.key
                                    ));
                                }
                            } else {
                                snapshot_content
                                    .push_str(&format!("  part: {} key: invalid-span\n", part.key));
                            }
                        } else {
                            snapshot_content
                                .push_str(&format!("  part: {} key: no-span\n", part.key));
                        }
                    }
                }
                Err(_) => {
                    snapshot_content.push_str(&format!("skip: {}\n", fixture_name));
                }
            }
        }

        if !snapshot_path.exists() {
            std::fs::create_dir_all(snapshot_path.parent().unwrap()).expect("create snapshot dir");
            std::fs::write(&snapshot_path, &snapshot_content).expect("write snapshot");
            panic!(
                "snapshot materialized at {} — rerun test",
                snapshot_path.display()
            );
        }

        let expected = std::fs::read_to_string(&snapshot_path).expect("read snapshot");
        if snapshot_content != expected {
            panic!(
                "fixture-lock snapshot mismatch:\n\nExpected:\n{}\n\nGot:\n{}",
                expected, snapshot_content
            );
        }
    }

    #[test]
    fn emit_spelling_lock_preserves_clause_heads_in_roundtrip() {
        let fixtures_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../model-runtime/examples");

        let mut entries: Vec<_> = std::fs::read_dir(&fixtures_dir)
            .expect("fixture dir exists")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "ecky")
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let path = entry.path();
            let fixture_name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let source = std::fs::read_to_string(&path).expect("read fixture");

            if let Ok(program) = compile_to_core_program(&source) {
                let re_emitted = emit_program(&program);

                // Extract top-level clause heads from both source and re-emitted
                let extract_clause_heads = |text: &str| -> Vec<String> {
                    let mut heads = Vec::new();
                    let mut depth = 0;
                    let mut current_word = String::new();
                    let mut in_word = false;

                    for ch in text.chars() {
                        match ch {
                            '(' => {
                                depth += 1;
                                in_word = false;
                                current_word.clear();
                            }
                            ')' => {
                                if depth == 2 && !current_word.is_empty() {
                                    heads.push(current_word.clone());
                                }
                                depth -= 1;
                                current_word.clear();
                                in_word = false;
                            }
                            ' ' | '\t' | '\n' | '\r' => {
                                in_word = false;
                            }
                            _ => {
                                if depth == 2 {
                                    if !in_word {
                                        in_word = true;
                                        current_word.clear();
                                    }
                                    current_word.push(ch);
                                }
                            }
                        }
                    }
                    heads
                };

                let source_heads = extract_clause_heads(&source);
                let re_emitted_heads = extract_clause_heads(&re_emitted);

                assert_eq!(
                    source_heads, re_emitted_heads,
                    "fixture {} clause head mismatch (model/part/feature/params/verify must be preserved)",
                    fixture_name
                );
            }
        }
    }

    #[test]
    fn core_program_digest_lock_captures_structural_stability() {
        let snapshot_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/component-unification/core-digest.snap");
        let fixtures_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../model-runtime/examples");

        let mut entries: Vec<_> = std::fs::read_dir(&fixtures_dir)
            .expect("fixture dir exists")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "ecky")
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|e| e.path());

        let mut snapshot_content = String::new();
        for entry in entries {
            let path = entry.path();
            let fixture_name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let source = std::fs::read_to_string(&path).expect("read fixture");

            match compile_to_core_program(&source) {
                Ok(program) => {
                    // SourceFileId comes from Steel's process-global source registry and
                    // depends on how many sources other tests parsed first; mask it so the
                    // digest only locks program structure.
                    let debug_repr = Regex::new(r"SourceFileId\(\s*\d+,?\s*\)")
                        .unwrap()
                        .replace_all(&format!("{:#?}", program), "SourceFileId(_)")
                        .into_owned();
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    hasher.update(debug_repr.as_bytes());
                    let digest = format!("{:x}", hasher.finalize());
                    snapshot_content.push_str(&format!("{}: {}\n", fixture_name, digest));
                }
                Err(_) => {
                    snapshot_content.push_str(&format!("{}: skip\n", fixture_name));
                }
            }
        }

        if !snapshot_path.exists() {
            std::fs::create_dir_all(snapshot_path.parent().unwrap()).expect("create snapshot dir");
            std::fs::write(&snapshot_path, &snapshot_content).expect("write snapshot");
            panic!(
                "snapshot materialized at {} — rerun test",
                snapshot_path.display()
            );
        }

        let expected = std::fs::read_to_string(&snapshot_path).expect("read snapshot");
        if snapshot_content != expected {
            panic!(
                "core-digest snapshot mismatch:\n\nExpected:\n{}\n\nGot:\n{}",
                expected, snapshot_content
            );
        }
    }
}
