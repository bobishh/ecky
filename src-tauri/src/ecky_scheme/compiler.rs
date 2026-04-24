#![allow(clippy::result_large_err, clippy::too_many_arguments)]

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use steel_core::parser::ast::{Atom, Define, ExprKind, Let};
use steel_core::parser::parser::Parser;
use steel_core::parser::tokens::TokenType;
use steel_core::rvals::SteelVal;

use crate::contracts::{AppError, AppErrorCode, AppResult};
use crate::ecky_core_ir::{
    CompilerError, CompilerErrorKind, CoreArrayOp, CoreBinding, CoreBooleanOp, CoreFrameOp,
    CoreKeywordArg, CoreLiteral, CoreMetaOp, CoreNode, CoreNodeKind, CoreOperation, CoreParameter,
    CoreParameterConstraints, CoreParameterKind, CoreParameterValue, CorePart, CorePathOp,
    CorePrimitive, CoreProgram, CoreReference, CoreResult, CoreShapeBinding, CoreSurfaceOp,
    CoreSymbol, CoreTransformOp, CoreValueKind, NodeId, ParamId, PartId, ProgramId, SourceFileId,
    SourceSpan,
};
use crate::ecky_deterministic;

use super::bootstrap;

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
    bootstrap::validate_user_source(source)
        .map_err(|err| CompilerError::new(CompilerErrorKind::Parse, err))?;
    reject_model_level_sequence_forms(source)?;

    if can_use_expanded_ast(source) {
        if let Ok(program) = compile_to_core_program_from_expanded_ast(source) {
            return verify_compiled_core_program(program);
        }
    }

    compile_to_core_program_via_runtime(source).and_then(verify_compiled_core_program)
}

fn verify_compiled_core_program(program: CoreProgram) -> CoreResult<CoreProgram> {
    crate::ecky_core_ir::verify_core_program(&program)?;
    Ok(program)
}

fn can_use_expanded_ast(source: &str) -> bool {
    !source.contains("(define-syntax ") && !source.contains("(set! ")
}

fn compile_to_core_program_via_runtime(source: &str) -> CoreResult<CoreProgram> {
    let mut engine = bootstrap::new_engine();
    let runtime_source = rewrite_runtime_model_clause_wrappers(source)?;
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
            "Model children are clauses, not sequence expressions. Supported direct clauses: `params`, `part`, `meta`. Supported wrappers: `begin`, `let`, `let*`. `{}` belongs inside `(part ...)` geometry/list expressions.",
            name
        ),
    )
}

fn compile_to_core_program_from_expanded_ast(source: &str) -> CoreResult<CoreProgram> {
    let mut engine = bootstrap::new_engine();
    let wrapped = wrap_expanded_ast_source(source);
    let forms = engine
        .emit_expanded_ast_without_optimizations(&wrapped, None)
        .map_err(|err| compiler_error(CompilerErrorKind::Parse, err))?;
    let decoded = forms.iter().map(decode_expanded_expr).collect::<Vec<_>>();
    parse_expanded_program(&decoded)
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

fn expr_head_name(value: &ExprKind) -> Option<String> {
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
    let mut next_param = 1u64;
    let mut next_part = 1u64;
    let mut next_node = 1u64;

    let clauses = collect_expanded_model_clauses(&forms[1..], helpers)?;
    let mut raw_parts = Vec::new();

    for clause_form in clauses {
        let items = clause_form.items;
        let clause =
            expr_name(items.first().ok_or_else(|| {
                CompilerError::new(CompilerErrorKind::Parse, "Empty model clause.")
            })?)?;
        match clause.as_str() {
            "params" => {
                for decl in items.into_iter().skip(1) {
                    params.push(parse_expanded_param_decl(
                        &decl,
                        &mut next_param,
                        &clause_form.helpers,
                    )?);
                }
            }
            "part" => raw_parts.push(ExpandedModelClause {
                items,
                helpers: clause_form.helpers,
            }),
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
    let parts = raw_parts
        .iter()
        .map(|items| {
            parse_expanded_part_decl(
                &items.items,
                &mut next_part,
                &mut next_node,
                &param_ids,
                &items.helpers,
            )
        })
        .collect::<CoreResult<Vec<_>>>()?;

    if parts.is_empty() {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel model needs at least one `(part ...)` clause.",
        ));
    }

    Ok(CoreProgram::new(ProgramId::new(1), params, parts))
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
                    _ => {}
                }
            }
            clauses.push(ExpandedModelClause {
                items,
                helpers: helpers.clone(),
            });
        }
        _ => {
            clauses.push(ExpandedModelClause {
                items: expr_list_items(form, "model clause")?,
                helpers: helpers.clone(),
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
                                        keywords.push(CoreKeywordArg {
                                            name: normalized.trim_start_matches(':').to_string(),
                                            value: parse_expanded_node(
                                                &items[index + 1],
                                                next_node,
                                                param_ids,
                                                helpers,
                                                node_refs,
                                                &body_locals,
                                                helper_stack,
                                            )?,
                                        });
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
                        (
                            CoreNodeKind::Call {
                                op: map_operation(&op_name),
                                args,
                                keywords,
                            },
                            infer_value_kind(&op_name),
                        )
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
            Err(err) if source.value_kind == CoreValueKind::List => {
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
                item,
                number_literal_node(index as f64, next_node, expr_source_span(&args[0])),
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
                .map(|keyword| CoreKeywordArg {
                    name: keyword.name.clone(),
                    value: clone_node_with_fresh_ids(&keyword.value, next_node),
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

fn expr_list_items(value: &ExprKind, context: &str) -> CoreResult<Vec<ExprKind>> {
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

fn expr_identifier(value: &ExprKind) -> Option<String> {
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
    let mut raw_parts = Vec::new();
    let mut next_param = 1u64;
    let mut next_part = 1u64;
    let mut next_node = 1u64;

    for form in forms.into_iter().skip(1) {
        let items = list_items(&form, "model clause")?;
        let clause =
            symbol_name(items.first().ok_or_else(|| {
                CompilerError::new(CompilerErrorKind::Parse, "Empty model clause.")
            })?)?;
        match clause.as_str() {
            "params" => {
                for decl in items.into_iter().skip(1) {
                    params.push(parse_param_decl(&decl, &mut next_param)?);
                }
            }
            "part" => raw_parts.push(items),
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
    let parts = raw_parts
        .iter()
        .map(|items| parse_part_decl(items, &mut next_part, &mut next_node, &param_ids))
        .collect::<CoreResult<Vec<_>>>()?;

    if parts.is_empty() {
        return Err(CompilerError::new(
            CompilerErrorKind::Parse,
            "Steel model needs at least one `(part ...)` clause.",
        ));
    }

    Ok(CoreProgram::new(ProgramId::new(1), params, parts))
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
                                    keywords.push(CoreKeywordArg {
                                        name: normalized.trim_start_matches(':').to_string(),
                                        value: parse_node(
                                            &items[index + 1],
                                            next_node,
                                            param_ids,
                                            node_refs,
                                            &body_locals,
                                        )?,
                                    });
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
                        (
                            CoreNodeKind::Call {
                                op: map_operation(&op_name),
                                args,
                                keywords,
                            },
                            infer_value_kind(&op_name),
                        )
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

fn typed_hole_call(
    type_name: Option<String>,
    goal: Option<String>,
    next_node: &mut u64,
) -> CoreResult<(CoreNodeKind, CoreValueKind)> {
    let type_name = type_name.ok_or_else(|| {
        CompilerError::new(CompilerErrorKind::Parse, "Typed hole requires `:type`.")
    })?;
    let value_kind = typed_hole_value_kind(&type_name)?;
    let mut keywords = vec![CoreKeywordArg {
        name: "type".to_string(),
        value: CoreNode::new(
            alloc_node_id(next_node),
            CoreNodeKind::Literal(CoreLiteral::Text(type_name)),
            CoreValueKind::Text,
        ),
    }];
    if let Some(goal) = goal {
        keywords.push(CoreKeywordArg {
            name: "goal".to_string(),
            value: CoreNode::new(
                alloc_node_id(next_node),
                CoreNodeKind::Literal(CoreLiteral::Text(goal)),
                CoreValueKind::Text,
            ),
        });
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
        "fillet" => CoreOperation::Surface(CoreSurfaceOp::Fillet),
        "chamfer" => CoreOperation::Surface(CoreSurfaceOp::Chamfer),
        "twist" => CoreOperation::Surface(CoreSurfaceOp::Twist),
        "polyline" | "path" => CoreOperation::Path(CorePathOp::Polyline),
        "bezier-path" => CoreOperation::Path(CorePathOp::BezierPath),
        "bspline" => CoreOperation::Path(CorePathOp::Bspline),
        "linear-array" => CoreOperation::Array(CoreArrayOp::LinearArray),
        "radial-array" => CoreOperation::Array(CoreArrayOp::RadialArray),
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
        "circle" | "rectangle" | "rounded-rect" | "rounded-polygon" | "rounded_polygon"
        | "polygon" | "profile" | "make-face" | "text" | "svg" | "offset-rounded" => {
            CoreValueKind::Sketch
        }
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
    if !program.parameters.is_empty() {
        out.push_str("\n  (params");
        for param in &program.parameters {
            out.push_str("\n    ");
            out.push_str(&emit_param(param));
        }
        out.push(')');
    }
    for part in &program.parts {
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
                items.push(emit_node(&keyword.value, param_names, node_names));
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
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => "fillet".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => "chamfer".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Twist) => "twist".to_string(),
        CoreOperation::Path(CorePathOp::Polyline) => "path".to_string(),
        CoreOperation::Path(CorePathOp::BezierPath) => "bezier-path".to_string(),
        CoreOperation::Path(CorePathOp::Bspline) => "bspline".to_string(),
        CoreOperation::Array(CoreArrayOp::LinearArray) => "linear-array".to_string(),
        CoreOperation::Array(CoreArrayOp::RadialArray) => "radial-array".to_string(),
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
            message.contains("Supported direct clauses: `params`, `part`, `meta`."),
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
            message.contains("Supported direct clauses: `params`, `part`, `meta`."),
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
            keywords[0].value.kind,
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
              (part body
                (build
                  (shape pts
                    (map
                      (lambda (i) (list i 0))
                      (range 0 2)))
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
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(5.0))
        ));
        assert!(matches!(
            first_pair[1].kind,
            CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(0.0))
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
}
