use rustpython_ast::Visitor;
use rustpython_parser::ast::{self, Constant, Expr, Stmt};
use rustpython_parser::{parse, Mode};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};
use uuid::Uuid;

use super::session::{build_runtime_snapshot, write_last_snapshot};
use crate::models::{
    validate_design_output, validate_design_params, validate_model_manifest, validate_ui_spec,
    AppError, AppResult, AppState, ArtifactBundle, DesignParams, Message, MessageRole,
    MessageStatus, ModelManifest, ParamValue, ParsedParamsResult, SelectOption, SelectValue,
    UiField, UiSpec,
};
use crate::{db, persist_thread_summary};

fn field_label(key: &str) -> String {
    key.replace(['_', '-'], " ")
}

fn default_field_label(key: &str, explicit: Option<String>) -> String {
    explicit.unwrap_or_else(|| field_label(key))
}

fn create_field(key: &str, value: &ParamValue) -> UiField {
    let label = field_label(key);
    match value {
        ParamValue::String(text) => UiField::Select {
            key: key.to_string(),
            label,
            options: vec![SelectOption {
                label: text.clone(),
                value: SelectValue::String(text.clone()),
            }],
            frozen: false,
        },
        ParamValue::Number(_) | ParamValue::Null => UiField::Number {
            key: key.to_string(),
            label,
            min: None,
            max: None,
            step: None,
            min_from: None,
            max_from: None,
            frozen: false,
        },
        ParamValue::Boolean(_) => UiField::Checkbox {
            key: key.to_string(),
            label,
            frozen: false,
        },
    }
}

fn parse_string_constant(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Constant(expr_const) => match &expr_const.value {
            Constant::Str(text) => Some(text.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn parse_number_constant(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Constant(expr_const) => match &expr_const.value {
            Constant::Int(value) => {
                let numeric: i64 = value.try_into().ok()?;
                Some(numeric as f64)
            }
            Constant::Float(value) => Some(*value),
            _ => None,
        },
        Expr::UnaryOp(unary) => match unary.op {
            ast::UnaryOp::USub => parse_number_constant(&unary.operand).map(|value| -value),
            ast::UnaryOp::UAdd => parse_number_constant(&unary.operand),
            _ => None,
        },
        _ => None,
    }
}

fn extract_value(expr: &Expr) -> ParamValue {
    match expr {
        Expr::Constant(expr_const) => match &expr_const.value {
            Constant::Str(text) => ParamValue::String(text.to_string()),
            Constant::Int(_) | Constant::Float(_) => {
                ParamValue::Number(parse_number_constant(expr).unwrap_or(0.0))
            }
            Constant::Bool(value) => ParamValue::Boolean(*value),
            Constant::None => ParamValue::Null,
            _ => ParamValue::Number(0.0),
        },
        Expr::UnaryOp(_) => parse_number_constant(expr)
            .map(ParamValue::Number)
            .unwrap_or(ParamValue::Number(0.0)),
        _ => ParamValue::Number(0.0),
    }
}

fn select_value_to_param(value: &SelectValue) -> ParamValue {
    match value {
        SelectValue::String(text) => ParamValue::String(text.clone()),
        SelectValue::Number(number) => ParamValue::Number(*number),
    }
}

fn select_label(value: &SelectValue) -> String {
    match value {
        SelectValue::String(text) => text.clone(),
        SelectValue::Number(number) => {
            if number.fract().abs() < f64::EPSILON {
                format!("{}", *number as i64)
            } else {
                number.to_string()
            }
        }
    }
}

fn parse_select_value(expr: &Expr) -> Option<SelectValue> {
    match expr {
        Expr::Constant(expr_const) => match &expr_const.value {
            Constant::Str(text) => Some(SelectValue::String(text.to_string())),
            Constant::Int(_) | Constant::Float(_) => {
                parse_number_constant(expr).map(SelectValue::Number)
            }
            _ => None,
        },
        Expr::UnaryOp(_) => parse_number_constant(expr).map(SelectValue::Number),
        _ => None,
    }
}

fn call_keyword<'a>(call: &'a ast::ExprCall, name: &str) -> Option<&'a Expr> {
    call.keywords.iter().find_map(|keyword| {
        keyword
            .arg
            .as_ref()
            .filter(|arg| arg.as_str() == name)
            .map(|_| &keyword.value)
    })
}

fn parse_select_option(expr: &Expr) -> Option<SelectOption> {
    if let Some(value) = parse_select_value(expr) {
        return Some(SelectOption {
            label: select_label(&value),
            value,
        });
    }

    let Expr::Dict(dict) = expr else {
        return None;
    };

    let mut label = None;
    let mut value = None;
    for (index, key_expr) in dict.keys.iter().enumerate() {
        let Some(key_expr) = key_expr else {
            continue;
        };
        let Some(key) = parse_string_constant(key_expr) else {
            continue;
        };
        let Some(item) = dict.values.get(index) else {
            continue;
        };
        match key.as_str() {
            "label" => label = parse_string_constant(item),
            "value" => value = parse_select_value(item),
            _ => {}
        }
    }

    let value = value?;
    Some(SelectOption {
        label: label.unwrap_or_else(|| select_label(&value)),
        value,
    })
}

fn parse_select_options(expr: &Expr) -> Option<Vec<SelectOption>> {
    match expr {
        Expr::List(list) => list.elts.iter().map(parse_select_option).collect(),
        Expr::Tuple(tuple) => tuple.elts.iter().map(parse_select_option).collect(),
        _ => None,
    }
}

fn parse_control_kind(call: &ast::ExprCall) -> Option<&str> {
    match &*call.func {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attr) => Some(attr.attr.as_str()),
        _ => None,
    }
}

fn parse_framework_control(call: &ast::ExprCall) -> Result<(UiField, ParamValue), String> {
    let Some(kind) = parse_control_kind(call) else {
        return Err("Control entry must call number/select/toggle.".to_string());
    };
    if call.args.len() < 2 {
        return Err(format!("Control '{}' must provide key and default.", kind));
    }

    let key = parse_string_constant(&call.args[0])
        .ok_or_else(|| format!("Control '{}' key must be a string literal.", kind))?;
    let label = default_field_label(
        &key,
        call_keyword(call, "label").and_then(parse_string_constant),
    );

    match kind {
        "number" => {
            let default = parse_number_constant(&call.args[1])
                .ok_or_else(|| format!("Number control '{}' default must be numeric.", key))?;
            let field = UiField::Number {
                key: key.clone(),
                label,
                min: call_keyword(call, "min").and_then(parse_number_constant),
                max: call_keyword(call, "max").and_then(parse_number_constant),
                step: call_keyword(call, "step").and_then(parse_number_constant),
                min_from: None,
                max_from: None,
                frozen: false,
            };
            Ok((field, ParamValue::Number(default)))
        }
        "select" => {
            let default = parse_select_value(&call.args[1]).ok_or_else(|| {
                format!(
                    "Select control '{}' default must be a string or number literal.",
                    key
                )
            })?;
            let options = call_keyword(call, "options")
                .and_then(parse_select_options)
                .ok_or_else(|| format!("Select control '{}' must declare literal options.", key))?;
            if options.is_empty() {
                return Err(format!(
                    "Select control '{}' must declare at least one option.",
                    key
                ));
            }
            let field = UiField::Select {
                key: key.clone(),
                label,
                options,
                frozen: false,
            };
            Ok((field, select_value_to_param(&default)))
        }
        "toggle" => {
            let default = match extract_value(&call.args[1]) {
                ParamValue::Boolean(value) => value,
                _ => {
                    return Err(format!(
                        "Toggle control '{}' default must be a boolean literal.",
                        key
                    ))
                }
            };
            let field = UiField::Checkbox {
                key: key.clone(),
                label,
                frozen: false,
            };
            Ok((field, ParamValue::Boolean(default)))
        }
        _ => Err(format!(
            "Unsupported control helper '{}'. Use number/select/toggle only.",
            kind
        )),
    }
}

fn parse_controls_value(value: &Expr) -> Result<ParsedParamsResult, String> {
    let entries = match value {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return Err("CONTROLS must be a literal list or tuple.".to_string()),
    };

    let mut fields = Vec::new();
    let mut params = DesignParams::new();
    for entry in entries {
        let Expr::Call(call) = entry else {
            return Err("Every CONTROLS entry must be a helper call.".to_string());
        };
        let (field, default) = parse_framework_control(call)?;
        let key = field.key().to_string();
        if params.contains_key(&key) {
            return Err(format!("Control key '{}' is declared more than once.", key));
        }
        params.insert(key, default);
        fields.push(field);
    }

    Ok(ParsedParamsResult { fields, params })
}

fn assigned_name_expr<'a>(stmt: &'a Stmt, target_name: &str) -> Option<&'a Expr> {
    match stmt {
        Stmt::Assign(assign) => assign
            .targets
            .iter()
            .any(|target| matches!(target, Expr::Name(name) if name.id.as_str() == target_name))
            .then_some(&assign.value),
        Stmt::AnnAssign(assign) => {
            if matches!(&*assign.target, Expr::Name(name) if name.id.as_str() == target_name) {
                assign.value.as_deref()
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_raw_params_identifier(name: &str) -> bool {
    matches!(name, "params" | "parameters")
}

fn raw_params_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) if is_raw_params_identifier(name.id.as_str()) => Some(name.id.as_str()),
        _ => None,
    }
}

fn is_build_context_call(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "BuildContext",
        Expr::Attribute(attr) => attr.attr.as_str() == "BuildContext",
        _ => false,
    }
}

#[derive(Default)]
struct FrameworkParamsValidator {
    saw_bind_call: bool,
    violations: Vec<String>,
}

impl FrameworkParamsValidator {
    fn push_violation(&mut self, message: String) {
        if !self.violations.iter().any(|existing| existing == &message) {
            self.violations.push(message);
        }
    }

    fn finish(self) -> Result<(), String> {
        if let Some(message) = self.violations.first() {
            return Err(message.clone());
        }
        if !self.saw_bind_call {
            return Err(
                "Framework macros must bind raw params through registry.bind(params) before building geometry."
                    .to_string(),
            );
        }
        Ok(())
    }
}

impl Visitor for FrameworkParamsValidator {
    fn visit_expr_call(&mut self, node: ast::ExprCall) {
        let is_bind_call =
            matches!(&*node.func, Expr::Attribute(attr) if attr.attr.as_str() == "bind");
        let is_build_context = is_build_context_call(&node.func);
        let ast::ExprCall {
            func,
            args,
            keywords,
            ..
        } = node;

        self.visit_expr(*func);
        for (index, arg) in args.into_iter().enumerate() {
            if is_bind_call && index == 0 && raw_params_name(&arg).is_some() {
                self.saw_bind_call = true;
                continue;
            }
            self.visit_expr(arg);
        }
        for keyword in keywords {
            let ast::Keyword { arg, value, .. } = keyword;
            let allowed_build_context_params = is_build_context
                && arg
                    .as_ref()
                    .map(|name| name.as_str() == "params")
                    .unwrap_or(false)
                && raw_params_name(&value).is_some();
            if allowed_build_context_params {
                continue;
            }
            self.visit_expr(value);
        }
    }

    fn visit_expr_attribute(&mut self, node: ast::ExprAttribute) {
        let ast::ExprAttribute { value, attr, .. } = node;
        if let Some(name) = raw_params_name(&value) {
            self.push_violation(format!(
                "Raw {} attribute access ({}.{}) is not allowed in framework macros. Use registry.bind(...) and cfg instead.",
                name, name, attr
            ));
            return;
        }
        self.visit_expr(*value);
    }

    fn visit_expr_subscript(&mut self, node: ast::ExprSubscript) {
        let ast::ExprSubscript { value, slice, .. } = node;
        if let Some(name) = raw_params_name(&value) {
            self.push_violation(format!(
                "Raw {} indexing is not allowed in framework macros. Use registry.bind(...) and cfg instead.",
                name
            ));
            self.visit_expr(*slice);
            return;
        }
        self.visit_expr(*value);
        self.visit_expr(*slice);
    }

    fn visit_expr_name(&mut self, node: ast::ExprName) {
        if is_raw_params_identifier(node.id.as_str()) {
            self.push_violation(format!(
                "Raw {} access is not allowed in framework macros. Bind params once and read cfg everywhere else.",
                node.id
            ));
        }
    }

    fn visit_arguments(&mut self, node: ast::Arguments) {
        for arg in node.posonlyargs {
            if let Some(annotation) = arg.def.annotation {
                self.visit_expr(*annotation);
            }
            if let Some(default) = arg.default {
                self.visit_expr(*default);
            }
        }
        for arg in node.args {
            if let Some(annotation) = arg.def.annotation {
                self.visit_expr(*annotation);
            }
            if let Some(default) = arg.default {
                self.visit_expr(*default);
            }
        }
        if let Some(vararg) = node.vararg {
            if let Some(annotation) = vararg.annotation {
                self.visit_expr(*annotation);
            }
        }
        for arg in node.kwonlyargs {
            if let Some(annotation) = arg.def.annotation {
                self.visit_expr(*annotation);
            }
            if let Some(default) = arg.default {
                self.visit_expr(*default);
            }
        }
        if let Some(kwarg) = node.kwarg {
            if let Some(annotation) = kwarg.annotation {
                self.visit_expr(*annotation);
            }
        }
    }

    fn visit_keyword(&mut self, node: ast::Keyword) {
        self.visit_expr(node.value);
    }

    fn visit_withitem(&mut self, node: ast::WithItem) {
        self.visit_expr(node.context_expr);
        if let Some(optional_vars) = node.optional_vars {
            self.visit_expr(*optional_vars);
        }
    }

    fn visit_match_case(&mut self, node: ast::MatchCase) {
        if let Some(guard) = node.guard {
            self.visit_expr(*guard);
        }
        for stmt in node.body {
            self.visit_stmt(stmt);
        }
    }

    fn visit_comprehension(&mut self, node: ast::Comprehension) {
        self.visit_expr(node.target);
        self.visit_expr(node.iter);
        for expr in node.ifs {
            self.visit_expr(expr);
        }
    }
}

fn validate_framework_macro_shape(module: &ast::ModModule) -> Result<(), String> {
    let mut validator = FrameworkParamsValidator::default();
    for stmt in module.body.clone() {
        validator.visit_stmt(stmt);
    }
    validator.finish()
}

fn module_uses_cad_sdk(module: &ast::ModModule) -> bool {
    module.body.iter().any(|stmt| match stmt {
        Stmt::Import(import_stmt) => import_stmt
            .names
            .iter()
            .any(|alias| alias.name.as_str() == "cad_sdk"),
        Stmt::ImportFrom(import_from) => import_from
            .module
            .as_ref()
            .map(|module| module.as_str() == "cad_sdk")
            .unwrap_or(false),
        _ => false,
    })
}

fn parse_framework_controls_from_ast(ast: &ast::Mod) -> Result<Option<ParsedParamsResult>, String> {
    let ast::Mod::Module(module) = ast else {
        return Ok(None);
    };

    let mut parsed = None;
    let mut saw_controls = false;
    for stmt in &module.body {
        if let Some(value) = assigned_name_expr(stmt, "CONTROLS") {
            saw_controls = true;
            parsed = Some(parse_controls_value(value)?);
        }
    }

    if let Some(result) = parsed {
        validate_framework_macro_shape(module)?;
        return Ok(Some(result));
    }
    if saw_controls || module_uses_cad_sdk(module) {
        return Err("Framework macros must define a valid literal CONTROLS list.".to_string());
    }
    Ok(None)
}

pub fn derive_framework_controls(macro_code: &str) -> AppResult<Option<ParsedParamsResult>> {
    let ast = parse(macro_code, Mode::Module, "<embedded>")
        .map_err(|err| AppError::parse(format!("Failed to parse macro: {}", err)))?;
    parse_framework_controls_from_ast(&ast)
        .map_err(|err| AppError::validation(format!("CAD framework contract violation: {}", err)))
}

pub fn coerce_param_for_field(
    field: &UiField,
    candidate: Option<&ParamValue>,
    fallback: Option<&ParamValue>,
) -> ParamValue {
    match field {
        UiField::Checkbox { .. } => candidate
            .and_then(|value| match value {
                ParamValue::Boolean(flag) => Some(ParamValue::Boolean(*flag)),
                _ => None,
            })
            .or_else(|| {
                fallback.and_then(|value| match value {
                    ParamValue::Boolean(flag) => Some(ParamValue::Boolean(*flag)),
                    _ => None,
                })
            })
            .unwrap_or(ParamValue::Boolean(false)),
        UiField::Select { options, .. } => candidate
            .filter(|value| {
                options
                    .iter()
                    .any(|option| value.matches_select_value(&option.value))
            })
            .cloned()
            .or_else(|| {
                fallback
                    .filter(|value| {
                        options
                            .iter()
                            .any(|option| value.matches_select_value(&option.value))
                    })
                    .cloned()
            })
            .or_else(|| {
                options
                    .first()
                    .map(|option| select_value_to_param(&option.value))
            })
            .unwrap_or(ParamValue::Null),
        UiField::Range { .. } | UiField::Number { .. } => candidate
            .and_then(|value| match value {
                ParamValue::Number(number) => Some(ParamValue::Number(*number)),
                _ => None,
            })
            .or_else(|| {
                fallback.and_then(|value| match value {
                    ParamValue::Number(number) => Some(ParamValue::Number(*number)),
                    _ => None,
                })
            })
            .unwrap_or(ParamValue::Number(0.0)),
        UiField::Image { .. } => candidate
            .and_then(|value| match value {
                ParamValue::String(s) => Some(ParamValue::String(s.clone())),
                _ => None,
            })
            .or_else(|| {
                fallback.and_then(|value| match value {
                    ParamValue::String(s) => Some(ParamValue::String(s.clone())),
                    _ => None,
                })
            })
            .unwrap_or_else(|| ParamValue::String("".to_string())),
    }
}

pub fn reconcile_framework_params(
    fields: &[UiField],
    current: &DesignParams,
    defaults: &DesignParams,
) -> DesignParams {
    let mut params = DesignParams::new();
    for field in fields {
        let key = field.key().to_string();
        let value = coerce_param_for_field(field, current.get(&key), defaults.get(&key));
        params.insert(key, value);
    }
    params
}

fn process_params_value(value: &Expr, fields: &mut Vec<UiField>, params: &mut DesignParams) {
    if let Expr::Dict(dict) = value {
        for (index, key_opt) in dict.keys.iter().enumerate() {
            if let Some(Expr::Constant(const_key)) = key_opt {
                if let Constant::Str(key) = &const_key.value {
                    if let Some(val_expr) = dict.values.get(index) {
                        let inferred = extract_value(val_expr);
                        params.insert(key.to_string(), inferred.clone());
                        fields.push(create_field(key, &inferred));
                    }
                }
            }
        }
    } else if let Expr::Call(call) = value {
        if let Expr::Name(func_name) = &*call.func {
            if func_name.id.as_str() == "dict" {
                for keyword in &call.keywords {
                    if let Some(arg_id) = &keyword.arg {
                        let key = arg_id.as_str().to_string();
                        let inferred = extract_value(&keyword.value);
                        params.insert(key.clone(), inferred.clone());
                        fields.push(create_field(&key, &inferred));
                    }
                }
            }
        }
    }
}

fn scan_expr_for_params_get(expr: &Expr, fields: &mut Vec<UiField>, params: &mut DesignParams) {
    match expr {
        Expr::Call(call) => {
            if let Expr::Attribute(attr) = &*call.func {
                if let Expr::Name(obj_name) = &*attr.value {
                    if obj_name.id.as_str() == "params" && attr.attr.as_str() == "get" {
                        if call.args.len() >= 2 {
                            if let Expr::Constant(const_key) = &call.args[0] {
                                if let Constant::Str(key) = &const_key.value {
                                    if !params.contains_key(key.as_str()) {
                                        let inferred = extract_value(&call.args[1]);
                                        params.insert(key.to_string(), inferred.clone());
                                        fields.push(create_field(key, &inferred));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for arg in &call.args {
                scan_expr_for_params_get(arg, fields, params);
            }
            for keyword in &call.keywords {
                scan_expr_for_params_get(&keyword.value, fields, params);
            }
        }
        Expr::BinOp(bin_op) => {
            scan_expr_for_params_get(&bin_op.left, fields, params);
            scan_expr_for_params_get(&bin_op.right, fields, params);
        }
        Expr::Dict(dict) => {
            for value in &dict.values {
                scan_expr_for_params_get(value, fields, params);
            }
        }
        Expr::List(list) => {
            for value in &list.elts {
                scan_expr_for_params_get(value, fields, params);
            }
        }
        Expr::Tuple(tuple) => {
            for value in &tuple.elts {
                scan_expr_for_params_get(value, fields, params);
            }
        }
        _ => {}
    }
}

fn scan_stmt_for_params_get(stmt: &Stmt, fields: &mut Vec<UiField>, params: &mut DesignParams) {
    match stmt {
        Stmt::Assign(assign) => scan_expr_for_params_get(&assign.value, fields, params),
        Stmt::AnnAssign(assign) => {
            if let Some(value) = &assign.value {
                scan_expr_for_params_get(value, fields, params);
            }
        }
        Stmt::Expr(expr) => scan_expr_for_params_get(&expr.value, fields, params),
        Stmt::For(for_stmt) => {
            for stmt in &for_stmt.body {
                scan_stmt_for_params_get(stmt, fields, params);
            }
            scan_expr_for_params_get(&for_stmt.iter, fields, params);
        }
        Stmt::If(if_stmt) => {
            for stmt in &if_stmt.body {
                scan_stmt_for_params_get(stmt, fields, params);
            }
            for stmt in &if_stmt.orelse {
                scan_stmt_for_params_get(stmt, fields, params);
            }
            scan_expr_for_params_get(&if_stmt.test, fields, params);
        }
        Stmt::With(with_stmt) => {
            for stmt in &with_stmt.body {
                scan_stmt_for_params_get(stmt, fields, params);
            }
        }
        Stmt::FunctionDef(function) => {
            for stmt in &function.body {
                scan_stmt_for_params_get(stmt, fields, params);
            }
        }
        _ => {}
    }
}

#[tauri::command]
#[specta::specta]
pub fn parse_macro_params(macro_code: String) -> ParsedParamsResult {
    let ast = match parse(&macro_code, Mode::Module, "<embedded>") {
        Ok(parsed) => parsed,
        Err(_) => {
            return ParsedParamsResult {
                fields: Vec::new(),
                params: DesignParams::new(),
            }
        }
    };

    match parse_framework_controls_from_ast(&ast) {
        Ok(Some(parsed)) => return parsed,
        Err(_) => {
            return ParsedParamsResult {
                fields: Vec::new(),
                params: DesignParams::new(),
            }
        }
        Ok(None) => {}
    }

    let mut fields = Vec::new();
    let mut params = DesignParams::new();
    if let ast::Mod::Module(module) = ast {
        for stmt in &module.body {
            match stmt {
                Stmt::Assign(assign) => {
                    let is_params = assign.targets.iter().any(
                        |target| matches!(target, Expr::Name(name) if name.id.as_str() == "params"),
                    );
                    if is_params {
                        process_params_value(&assign.value, &mut fields, &mut params);
                    }
                }
                Stmt::AnnAssign(assign) => {
                    if let Expr::Name(name) = &*assign.target {
                        if name.id.as_str() == "params" {
                            if let Some(value) = &assign.value {
                                process_params_value(value, &mut fields, &mut params);
                            }
                        }
                    }
                }
                _ => {}
            }
            scan_stmt_for_params_get(stmt, &mut fields, &mut params);
        }
    }

    let mut unique_fields = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();
    for field in fields {
        if seen_keys.insert(field.key().to_string()) {
            unique_fields.push(field);
        }
    }

    ParsedParamsResult {
        fields: unique_fields,
        params,
    }
}

use crate::services::design as design_service;

#[tauri::command]
#[specta::specta]
pub async fn add_manual_version(
    thread_id: String,
    title: String,
    version_name: String,
    macro_code: String,
    parameters: DesignParams,
    ui_spec: UiSpec,
    artifact_bundle: Option<ArtifactBundle>,
    model_manifest: Option<ModelManifest>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<String> {
    design_service::add_manual_version(
        thread_id,
        title,
        version_name,
        macro_code,
        parameters,
        ui_spec,
        artifact_bundle,
        model_manifest,
        None,
        None,
        &state,
        &app,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn add_imported_model_version(
    thread_id: String,
    title: String,
    artifact_bundle: ArtifactBundle,
    model_manifest: ModelManifest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<String> {
    validate_model_manifest(&model_manifest)?;
    if artifact_bundle.model_id != model_manifest.model_id {
        return Err(AppError::validation(
            "Imported model manifest does not match artifact bundle model id.",
        ));
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let db = state.db.lock().await;

    let thread_traits = if db::get_thread_title(&db, &thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .is_none()
    {
        Some(crate::generate_genie_traits())
    } else {
        None
    };
    db::create_or_update_thread(&db, &thread_id, &title, now, thread_traits.as_ref())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let msg_id = Uuid::new_v4().to_string();
    let label = model_manifest.document.document_label.trim();
    let content = if label.is_empty() {
        "Imported FreeCAD model.".to_string()
    } else {
        format!("Imported FreeCAD model: {}.", label)
    };
    let msg = Message {
        id: msg_id.clone(),
        role: MessageRole::Assistant,
        content,
        status: MessageStatus::Success,
        output: None,
        usage: None,
        artifact_bundle: Some(artifact_bundle.clone()),
        model_manifest: Some(model_manifest.clone()),
        agent_origin: None,
        image_data: None,
        attachment_images: Vec::new(),
        timestamp: now,
    };

    db::add_message(&db, &thread_id, &msg).map_err(|err| AppError::persistence(err.to_string()))?;
    let _ = persist_thread_summary(&db, &thread_id, &title);
    let snapshot = build_runtime_snapshot(
        None,
        Some(thread_id.clone()),
        Some(msg_id.clone()),
        Some(artifact_bundle),
        Some(model_manifest),
        None,
    );
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
    }
    write_last_snapshot(&app, Some(&snapshot));

    Ok(msg_id)
}

#[tauri::command]
#[specta::specta]
pub async fn update_ui_spec(
    message_id: String,
    ui_spec: UiSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    validate_ui_spec(&ui_spec)?;

    let (updated_output, updated_thread_id, artifact_bundle, model_manifest) = {
        let db = state.db.lock().await;
        let (mut current_output, current_thread_id) =
            db::get_message_output_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .ok_or_else(|| {
                    AppError::not_found("Message output not found for uiSpec update.")
                })?;
        if let Some(parsed) = derive_framework_controls(&current_output.macro_code)? {
            let derived = UiSpec {
                fields: parsed.fields.clone(),
            };
            if derived != ui_spec {
                return Err(AppError::validation(
                    "uiSpec is derived from CONTROLS for framework macros. Edit CONTROLS in the macro instead.",
                ));
            }
        }
        current_output.ui_spec = ui_spec;
        validate_design_output(&current_output)?;
        db::update_message_ui_spec(&db, &message_id, &current_output.ui_spec)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        let (artifact_bundle, model_manifest, _) =
            db::get_message_runtime_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .unwrap_or((None, None, current_thread_id.clone()));
        (
            current_output,
            current_thread_id,
            artifact_bundle,
            model_manifest,
        )
    };

    {
        let snapshot = build_runtime_snapshot(
            Some(updated_output.clone()),
            Some(updated_thread_id.clone()),
            Some(message_id.clone()),
            artifact_bundle,
            model_manifest,
            None,
        );
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
        write_last_snapshot(&app, Some(&snapshot));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_parameters(
    message_id: String,
    parameters: DesignParams,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    let (updated_output, updated_thread_id, artifact_bundle, model_manifest) = {
        let db = state.db.lock().await;
        let (mut current_output, current_thread_id) =
            db::get_message_output_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .ok_or_else(|| {
                    AppError::not_found("Message output not found for parameter update.")
                })?;
        validate_design_params(&parameters, &current_output.ui_spec)?;
        current_output.initial_params = parameters;
        validate_design_output(&current_output)?;
        db::update_message_parameters(&db, &message_id, &current_output.initial_params)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        let (artifact_bundle, model_manifest, _) =
            db::get_message_runtime_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .unwrap_or((None, None, current_thread_id.clone()));
        (
            current_output,
            current_thread_id,
            artifact_bundle,
            model_manifest,
        )
    };

    {
        let snapshot = build_runtime_snapshot(
            Some(updated_output.clone()),
            Some(updated_thread_id.clone()),
            Some(message_id.clone()),
            artifact_bundle,
            model_manifest,
            None,
        );
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
        write_last_snapshot(&app, Some(&snapshot));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_version_runtime(
    message_id: String,
    artifact_bundle: ArtifactBundle,
    model_manifest: ModelManifest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    validate_model_manifest(&model_manifest)?;
    if artifact_bundle.model_id != model_manifest.model_id {
        return Err(AppError::validation(
            "Model manifest does not match artifact bundle model id.",
        ));
    }

    let (current_output, current_thread_id) = {
        let db = state.db.lock().await;
        db::update_message_artifact_bundle(&db, &message_id, &artifact_bundle)
            .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;
        db::update_message_model_manifest(&db, &message_id, &model_manifest)
            .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;

        let current_output = db::get_message_output_and_thread(&db, &message_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .map(|(output, _)| output);
        let (_, _, current_thread_id) = db::get_message_runtime_and_thread(&db, &message_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .ok_or_else(|| AppError::not_found("Message runtime not found for update."))?;
        (current_output, current_thread_id)
    };

    {
        let snapshot = build_runtime_snapshot(
            current_output,
            Some(current_thread_id),
            Some(message_id),
            Some(artifact_bundle),
            Some(model_manifest),
            None,
        );
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
        write_last_snapshot(&app, Some(&snapshot));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_framework_controls_parses_literal_controls() {
        let macro_code = r#"
from cad_sdk import number, select, toggle, ControlRegistry

CONTROLS = [
    number("width", 105.0, min=60, max=180, step=1, label="Width"),
    select("style", "slit", options=["slit", "holes"], label="Style"),
    toggle("enable_tie_ears", True, label="Tie Ears"),
]

registry = ControlRegistry(CONTROLS)
cfg = registry.bind(params)
"#;

        let parsed = derive_framework_controls(macro_code)
            .expect("framework parse should succeed")
            .expect("framework controls should exist");

        assert_eq!(parsed.fields.len(), 3);
        assert_eq!(parsed.params.get("width"), Some(&ParamValue::Number(105.0)));
        assert_eq!(
            parsed.params.get("style"),
            Some(&ParamValue::String("slit".to_string()))
        );
        assert_eq!(
            parsed.params.get("enable_tie_ears"),
            Some(&ParamValue::Boolean(true))
        );
    }

    #[test]
    fn parse_macro_params_prefers_framework_controls() {
        let macro_code = r#"
from cad_sdk import number, ControlRegistry

CONTROLS = [
    number("width", 42.0, label="Width"),
]

registry = ControlRegistry(CONTROLS)
cfg = registry.bind(params)
legacy_width = 10.0
"#;

        let parsed = parse_macro_params(macro_code.to_string());
        assert_eq!(parsed.fields.len(), 1);
        assert_eq!(parsed.params.get("width"), Some(&ParamValue::Number(42.0)));
    }

    #[test]
    fn parse_macro_params_supports_legacy_macros() {
        let macro_code = r#"
params = {
    "width": 55.0,
    "enabled": True,
}

width = params.get("width", 40.0)
enabled = params.get("enabled", False)
"#;

        let parsed = parse_macro_params(macro_code.to_string());
        assert_eq!(parsed.params.get("width"), Some(&ParamValue::Number(55.0)));
        assert_eq!(
            parsed.params.get("enabled"),
            Some(&ParamValue::Boolean(true))
        );
        assert_eq!(parsed.fields.len(), 2);
    }

    #[test]
    fn derive_framework_controls_rejects_missing_controls() {
        let macro_code = r#"
from cad_sdk import number

width = 10
"#;

        let err = derive_framework_controls(macro_code).expect_err("should reject");
        assert!(err.message.contains("CAD framework contract violation"));
    }

    #[test]
    fn derive_framework_controls_rejects_raw_params_access() {
        let macro_code = r#"
from cad_sdk import number, ControlRegistry

CONTROLS = [
    number("width", 10.0),
]

registry = ControlRegistry(CONTROLS)
cfg = registry.bind(params)
width = params.get("width", 10.0)
"#;

        let err = derive_framework_controls(macro_code).expect_err("should reject raw params");
        assert!(err.message.contains("Raw params attribute access"));
    }

    #[test]
    fn derive_framework_controls_requires_bind_call() {
        let macro_code = r#"
from cad_sdk import number

CONTROLS = [
    number("width", 10.0),
]
"#;

        let err = derive_framework_controls(macro_code).expect_err("should require bind");
        assert!(err.message.contains("registry.bind(params)"));
    }
}
