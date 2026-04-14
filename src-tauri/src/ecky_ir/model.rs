use std::collections::BTreeMap;

use lexpr::Value;

use crate::ecky_core_ir::{
    CoreArrayOp, CoreBooleanOp, CoreFrameOp, CoreKeywordArg, CoreLiteral, CoreMetaOp, CoreNode,
    CoreNodeKind, CoreOperation, CoreParameter, CoreParameterKind, CoreParameterValue, CorePart,
    CorePathOp, CorePrimitive, CoreProgram, CoreReference, CoreSurfaceOp, CoreSymbol,
    CoreTransformOp,
};
use crate::models::{
    AppResult, DesignParams, ParamValue, ParsedParamsResult, SelectOption, SelectValue, UiField,
};

use super::shared::{unsupported, validation};
use super::syntax::{
    head_symbol, ir_parse, keyword_name, list_items, parse_number_value, parse_stringish,
};

pub(crate) struct IrModel {
    pub(super) params: Vec<IrParam>,
    pub(super) parts: Vec<IrPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum IrExpr {
    Number(f64),
    Boolean(bool),
    String(String),
    Symbol(String),
    Keyword(String),
    List(Vec<IrExpr>),
}

impl IrExpr {
    pub(super) fn from_value(value: &Value) -> AppResult<Self> {
        if let Some(number) = value.as_f64() {
            return Ok(Self::Number(number));
        }
        if let Some(flag) = value.as_bool() {
            return Ok(Self::Boolean(flag));
        }
        if let Some(text) = value.as_str() {
            return Ok(Self::String(text.to_string()));
        }
        if let Some(symbol) = value.as_symbol() {
            return Ok(Self::Symbol(symbol.to_string()));
        }
        if let Some(keyword) = value.as_keyword() {
            return Ok(Self::Keyword(keyword.to_string()));
        }
        if let Some(items) = value.to_vec() {
            return Ok(Self::List(
                items
                    .iter()
                    .map(Self::from_value)
                    .collect::<AppResult<Vec<_>>>()?,
            ));
        }
        Err(validation(
            "Ecky IR v0 only supports scalar atoms and proper lists.",
        ))
    }

    pub(super) fn dup(&self) -> Self {
        self.clone()
    }

    pub(super) fn number(value: f64) -> Self {
        Self::Number(value)
    }

    pub(super) fn boolean(value: bool) -> Self {
        Self::Boolean(value)
    }

    pub(super) fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    pub(super) fn symbol(name: impl Into<String>) -> Self {
        Self::Symbol(name.into())
    }

    pub(super) fn keyword(name: impl Into<String>) -> Self {
        Self::Keyword(name.into())
    }

    pub(super) fn list(items: Vec<IrExpr>) -> Self {
        Self::List(items)
    }

    pub(super) fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub(super) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(flag) => Some(*flag),
            _ => None,
        }
    }

    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub(super) fn as_symbol(&self) -> Option<&str> {
        match self {
            Self::Symbol(symbol) => Some(symbol),
            _ => None,
        }
    }

    pub(super) fn as_keyword(&self) -> Option<&str> {
        match self {
            Self::Keyword(keyword) => Some(keyword),
            _ => None,
        }
    }

    pub(super) fn as_list(&self) -> Option<&[IrExpr]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }
}

pub(super) fn expr_list_items<'a>(value: &'a IrExpr, context: &str) -> AppResult<&'a [IrExpr]> {
    value
        .as_list()
        .ok_or_else(|| validation(format!("Expected a proper list for {}.", context)))
}

pub(super) fn expr_head_symbol<'a>(items: &'a [IrExpr], context: &str) -> AppResult<&'a str> {
    items
        .first()
        .and_then(IrExpr::as_symbol)
        .ok_or_else(|| validation(format!("Expected a symbolic head for {}.", context)))
}

pub(super) fn expr_keyword_name(value: &IrExpr) -> Option<&str> {
    value
        .as_keyword()
        .or_else(|| {
            value.as_symbol().and_then(|symbol| {
                symbol
                    .strip_prefix("#:")
                    .or_else(|| symbol.strip_prefix(':'))
            })
        })
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.strip_prefix("#:").or_else(|| text.strip_prefix(':')))
        })
}

pub(super) fn expr_parse_stringish(value: &IrExpr, context: &str) -> AppResult<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    if let Some(symbol) = value.as_symbol() {
        return Ok(symbol.to_string());
    }
    Err(validation(format!("Expected text for {}.", context)))
}

pub(super) fn inline_let_expr(value: &IrExpr) -> AppResult<IrExpr> {
    inline_let_expr_with_scope(value, &BTreeMap::new())
}

fn inline_let_expr_with_scope(
    value: &IrExpr,
    scope: &BTreeMap<String, IrExpr>,
) -> AppResult<IrExpr> {
    match value {
        IrExpr::Symbol(symbol) => Ok(scope.get(symbol).cloned().unwrap_or_else(|| value.clone())),
        IrExpr::List(items) => {
            let Some(head) = items.first() else {
                return Ok(value.clone());
            };
            match head.as_symbol() {
                Some("let") => {
                    if items.len() != 3 {
                        return Err(validation("`let` expects bindings and a body."));
                    }
                    let bindings = expr_list_items(&items[1], "let bindings")?;
                    let mut local_scope = scope.clone();
                    for binding in bindings {
                        let pair = expr_list_items(binding, "let binding")?;
                        if pair.len() != 2 {
                            return Err(validation("Each `let` binding must be `(name expr)`."));
                        }
                        let name = expr_parse_stringish(&pair[0], "let binding name")?;
                        let bound = inline_let_expr_with_scope(&pair[1], &local_scope)?;
                        local_scope.insert(name, bound);
                    }
                    inline_let_expr_with_scope(&items[2], &local_scope)
                }
                Some("build") => {
                    let mut rewritten = vec![head.clone()];
                    for stmt in items.iter().skip(1) {
                        if let Some(stmt_items) = stmt.as_list() {
                            if let Some(stmt_head) = stmt_items.first().and_then(IrExpr::as_symbol)
                            {
                                match stmt_head {
                                    "shape" if stmt_items.len() == 3 => {
                                        rewritten.push(IrExpr::list(vec![
                                            stmt_items[0].clone(),
                                            stmt_items[1].clone(),
                                            inline_let_expr_with_scope(&stmt_items[2], scope)?,
                                        ]));
                                        continue;
                                    }
                                    "result" if stmt_items.len() == 2 => {
                                        rewritten.push(IrExpr::list(vec![
                                            stmt_items[0].clone(),
                                            inline_let_expr_with_scope(&stmt_items[1], scope)?,
                                        ]));
                                        continue;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        rewritten.push(inline_let_expr_with_scope(stmt, scope)?);
                    }
                    Ok(IrExpr::list(rewritten))
                }
                _ => {
                    let mut rewritten = Vec::with_capacity(items.len());
                    rewritten.push(head.clone());
                    for item in items.iter().skip(1) {
                        rewritten.push(inline_let_expr_with_scope(item, scope)?);
                    }
                    Ok(IrExpr::list(rewritten))
                }
            }
        }
        _ => Ok(value.clone()),
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct BuildExpr {
    pub(super) bindings: Vec<BuildBinding>,
    pub(super) result: Value,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct BuildBinding {
    pub(super) name: String,
    pub(super) expr: Value,
}

#[derive(Debug)]
pub(super) struct TypedBuildExpr {
    pub(super) bindings: Vec<TypedBuildBinding>,
    pub(super) result: IrExpr,
}

#[derive(Debug)]
pub(super) struct TypedBuildBinding {
    pub(super) name: String,
    pub(super) expr: IrExpr,
}

#[derive(Debug, Clone)]
pub(crate) struct IrParam {
    pub(super) field: UiField,
    pub(super) default_value: ParamValue,
}

#[derive(Debug)]
pub(crate) struct IrPart {
    pub(super) part_id: String,
    pub(super) label: String,
    pub(super) expr: IrExpr,
}

pub(crate) fn core_program_to_model(program: &CoreProgram) -> AppResult<IrModel> {
    let params = program
        .parameters
        .iter()
        .map(core_param_to_ir_param)
        .collect::<AppResult<Vec<_>>>()?;
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let parts = program
        .parts
        .iter()
        .map(|part| core_part_to_ir_part(part, &param_names))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(IrModel { params, parts })
}
pub(super) fn parse_model(source: &str) -> AppResult<IrModel> {
    let value = ir_parse(source)?;
    let forms = list_items(&value, "model")?;
    if head_symbol(&forms, "model root")? != "model" {
        return Err(validation(
            "Ecky IR v0 source must start with `(model ...)`.",
        ));
    }

    let mut params = Vec::new();
    let mut parts = Vec::new();
    for form in forms.into_iter().skip(1) {
        let items = list_items(&form, "model form")?;
        match head_symbol(&items, "model form")? {
            "params" => {
                for decl in items.into_iter().skip(1) {
                    params.push(parse_param_decl(&decl)?);
                }
            }
            "part" => parts.push(parse_part_decl(&items)?),
            "meta" => {}
            other => {
                return Err(unsupported(format!(
                    "Top-level node `{}` is not supported by Ecky IR v0.",
                    other
                )))
            }
        }
    }

    if parts.is_empty() {
        return Err(validation(
            "Ecky IR v0 models need at least one `(part ...)` node.",
        ));
    }

    Ok(IrModel { params, parts })
}

pub(super) fn parse_param_decl(value: &Value) -> AppResult<IrParam> {
    let items = list_items(value, "param declaration")?;
    let kind = head_symbol(&items, "param declaration")?;
    if items.len() < 3 {
        return Err(validation(format!(
            "Param declaration `{}` needs at least a name and default value.",
            kind
        )));
    }
    let key = parse_stringish(&items[1], "param key")?;
    let mut label = humanize_key(&key);
    let mut min = None;
    let mut max = None;
    let mut step = None;
    let mut options = Vec::new();
    let mut frozen = false;
    let default_atom = &items[2];
    let mut index = 3usize;
    while index + 1 < items.len() {
        let Some(name) = keyword_name(&items[index]) else {
            break;
        };
        let option_value = &items[index + 1];
        match name {
            "label" => label = parse_stringish(option_value, "param label")?,
            "min" => min = Some(parse_number_value(option_value, "param min")?),
            "max" => max = Some(parse_number_value(option_value, "param max")?),
            "step" => step = Some(parse_number_value(option_value, "param step")?),
            "frozen" => frozen = option_value.as_bool().unwrap_or(false),
            "options" => {
                let option_forms = list_items(option_value, "select options")?;
                options = option_forms
                    .iter()
                    .map(parse_select_option)
                    .collect::<AppResult<Vec<_>>>()?;
            }
            _ => {}
        }
        index += 2;
    }

    let (field, default_value) = match kind {
        "number" => (
            UiField::Number {
                key,
                label,
                min,
                max,
                step,
                min_from: None,
                max_from: None,
                frozen,
            },
            ParamValue::Number(parse_number_value(default_atom, "number default")?),
        ),
        "select" => {
            if options.is_empty() {
                return Err(validation("Select params require `:options` entries."));
            }
            let default_value = if let Some(text) = default_atom.as_str() {
                ParamValue::String(text.to_string())
            } else if let Some(number) = default_atom.as_f64() {
                ParamValue::Number(number)
            } else {
                return Err(validation("Select defaults must be string or number."));
            };
            (
                UiField::Select {
                    key,
                    label,
                    options,
                    frozen,
                },
                default_value,
            )
        }
        "toggle" => (
            UiField::Checkbox { key, label, frozen },
            ParamValue::Boolean(default_atom.as_bool().unwrap_or(false)),
        ),
        "image" => (
            UiField::Image { key, label, frozen },
            ParamValue::String(default_atom.as_str().unwrap_or_default().to_string()),
        ),
        other => {
            return Err(unsupported(format!(
                "Param kind `{}` is not supported by Ecky IR v0.",
                other
            )))
        }
    };

    Ok(IrParam {
        field,
        default_value,
    })
}

pub(super) fn parse_select_option(value: &Value) -> AppResult<SelectOption> {
    let pair = list_items(value, "select option")?;
    if pair.len() != 2 {
        return Err(validation("Select options must be `(label value)` pairs."));
    }
    let label = parse_stringish(&pair[0], "select option label")?;
    let value = if let Some(text) = pair[1].as_str() {
        SelectValue::String(text.to_string())
    } else if let Some(symbol) = pair[1].as_symbol() {
        SelectValue::String(symbol.to_string())
    } else if let Some(number) = pair[1].as_f64() {
        SelectValue::Number(number)
    } else {
        return Err(validation("Select option values must be string or number."));
    };
    Ok(SelectOption { label, value })
}

pub(super) fn parse_part_decl(items: &[Value]) -> AppResult<IrPart> {
    if items.len() < 3 {
        return Err(validation("Each `(part ...)` needs an id and expression."));
    }
    let part_id = parse_stringish(&items[1], "part id")?;
    let (label, expr) =
        if items.len() >= 4 && (items[2].as_str().is_some() || items[2].as_symbol().is_some()) {
            (parse_stringish(&items[2], "part label")?, items[3].clone())
        } else {
            (humanize_key(&part_id), items[2].clone())
        };
    Ok(IrPart {
        part_id,
        label,
        expr: IrExpr::from_value(&expr)?,
    })
}

#[allow(dead_code)]
pub(super) fn parse_build_expr(value: &Value) -> AppResult<BuildExpr> {
    let items = list_items(value, "build expression")?;
    if head_symbol(&items, "build expression")? != "build" {
        return Err(validation("Expected a `(build ...)` expression."));
    }
    if items.len() < 2 {
        return Err(validation(
            "`build` expects one or more `(shape ...)` bindings and a `(result ...)`.",
        ));
    }

    let mut bindings = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut result = None;

    for form in items.into_iter().skip(1) {
        let stmt = list_items(&form, "build statement")?;
        match head_symbol(&stmt, "build statement")? {
            "shape" => {
                if result.is_some() {
                    return Err(validation(
                        "`build` cannot define more shapes after `(result ...)`.",
                    ));
                }
                if stmt.len() != 3 {
                    return Err(validation(
                        "`shape` expects a binding name and an expression.",
                    ));
                }
                let name = parse_stringish(&stmt[1], "shape binding name")?;
                if !seen.insert(name.clone()) {
                    return Err(validation(format!(
                        "`build` cannot rebind shape `{}`.",
                        name
                    )));
                }
                bindings.push(BuildBinding {
                    name,
                    expr: stmt[2].clone(),
                });
            }
            "result" => {
                if stmt.len() != 2 {
                    return Err(validation("`result` expects exactly one expression."));
                }
                if result.is_some() {
                    return Err(validation("`build` requires exactly one `(result ...)`."));
                }
                result = Some(stmt[1].clone());
            }
            other => {
                return Err(validation(format!(
                    "`build` only accepts `(shape ...)` and `(result ...)`, got `{}`.",
                    other
                )));
            }
        }
    }

    Ok(BuildExpr {
        bindings,
        result: result.ok_or_else(|| validation("`build` requires exactly one `(result ...)`."))?,
    })
}

pub(super) fn parse_typed_build_expr(value: &IrExpr) -> AppResult<TypedBuildExpr> {
    let items = expr_list_items(value, "build expression")?;
    if expr_head_symbol(items, "build expression")? != "build" {
        return Err(validation("Expected a `(build ...)` expression."));
    }
    if items.len() < 2 {
        return Err(validation(
            "`build` expects one or more `(shape ...)` bindings and a `(result ...)`.",
        ));
    }

    let mut bindings = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut result = None;

    for form in items.iter().skip(1) {
        let stmt = expr_list_items(form, "build statement")?;
        match expr_head_symbol(stmt, "build statement")? {
            "shape" => {
                if result.is_some() {
                    return Err(validation(
                        "`build` cannot define more shapes after `(result ...)`.",
                    ));
                }
                if stmt.len() != 3 {
                    return Err(validation(
                        "`shape` expects a binding name and an expression.",
                    ));
                }
                let name = expr_parse_stringish(&stmt[1], "shape binding name")?;
                if !seen.insert(name.clone()) {
                    return Err(validation(format!(
                        "`build` cannot rebind shape `{}`.",
                        name
                    )));
                }
                bindings.push(TypedBuildBinding {
                    name,
                    expr: stmt[2].dup(),
                });
            }
            "result" => {
                if stmt.len() != 2 {
                    return Err(validation("`result` expects exactly one expression."));
                }
                if result.is_some() {
                    return Err(validation("`build` requires exactly one `(result ...)`."));
                }
                result = Some(stmt[1].dup());
            }
            other => {
                return Err(validation(format!(
                    "`build` only accepts `(shape ...)` and `(result ...)`, got `{}`.",
                    other
                )));
            }
        }
    }

    Ok(TypedBuildExpr {
        bindings,
        result: result.ok_or_else(|| validation("`build` requires exactly one `(result ...)`."))?,
    })
}

pub(super) fn humanize_key(key: &str) -> String {
    key.split(['_', '-', '.'])
        .filter(|token| !token.is_empty())
        .map(|token| {
            let mut chars = token.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn build_param_env(
    model: &IrModel,
    parameters: &DesignParams,
) -> BTreeMap<String, ParamValue> {
    let mut env = BTreeMap::new();
    for param in &model.params {
        env.insert(param.field.key().to_string(), param.default_value.clone());
    }
    for (key, value) in parameters {
        env.insert(key.clone(), value.clone());
    }
    env
}

pub(super) fn parsed_params_from_model(model: &IrModel) -> ParsedParamsResult {
    ParsedParamsResult {
        fields: model
            .params
            .iter()
            .map(|param| param.field.clone())
            .collect(),
        params: model
            .params
            .iter()
            .map(|param| (param.field.key().to_string(), param.default_value.clone()))
            .collect(),
    }
}

fn core_param_to_ir_param(param: &CoreParameter) -> AppResult<IrParam> {
    let field = match param.kind {
        CoreParameterKind::Number => UiField::Number {
            key: param.key.clone(),
            label: param.label.clone(),
            min: param.constraints.min,
            max: param.constraints.max,
            step: param.constraints.step,
            min_from: None,
            max_from: None,
            frozen: param.frozen,
        },
        CoreParameterKind::Choice => UiField::Select {
            key: param.key.clone(),
            label: param.label.clone(),
            options: param
                .constraints
                .choices
                .iter()
                .map(|choice| {
                    Ok(SelectOption {
                        label: choice.label.clone(),
                        value: match &choice.value {
                            CoreParameterValue::Number(n) => SelectValue::Number(*n),
                            CoreParameterValue::Choice(text)
                            | CoreParameterValue::Text(text)
                            | CoreParameterValue::Image(text) => SelectValue::String(text.clone()),
                            CoreParameterValue::Boolean(_) => return Err(validation(
                                "Select options cannot use boolean values in the legacy IR bridge.",
                            )),
                        },
                    })
                })
                .collect::<AppResult<Vec<_>>>()?,
            frozen: param.frozen,
        },
        CoreParameterKind::Boolean => UiField::Checkbox {
            key: param.key.clone(),
            label: param.label.clone(),
            frozen: param.frozen,
        },
        CoreParameterKind::Image => UiField::Image {
            key: param.key.clone(),
            label: param.label.clone(),
            frozen: param.frozen,
        },
        CoreParameterKind::Text => {
            return Err(unsupported(
                "Text params are not yet supported by the legacy IR bridge.",
            ))
        }
    };

    Ok(IrParam {
        field,
        default_value: core_param_value_to_param_value(&param.default_value)?,
    })
}

fn core_part_to_ir_part(part: &CorePart, param_names: &BTreeMap<u64, String>) -> AppResult<IrPart> {
    Ok(IrPart {
        part_id: part.key.clone(),
        label: part.label.clone(),
        expr: core_node_to_ir_expr(&part.root, param_names, &BTreeMap::new())?,
    })
}

fn core_param_value_to_param_value(value: &CoreParameterValue) -> AppResult<ParamValue> {
    match value {
        CoreParameterValue::Number(n) => Ok(ParamValue::Number(*n)),
        CoreParameterValue::Boolean(flag) => Ok(ParamValue::Boolean(*flag)),
        CoreParameterValue::Text(text)
        | CoreParameterValue::Choice(text)
        | CoreParameterValue::Image(text) => Ok(ParamValue::String(text.clone())),
    }
}

fn core_node_to_ir_expr(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    refs: &BTreeMap<u64, String>,
) -> AppResult<IrExpr> {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Number(n)) => Ok(IrExpr::number(*n)),
        CoreNodeKind::Literal(CoreLiteral::Boolean(flag)) => Ok(IrExpr::boolean(*flag)),
        CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(IrExpr::string(text.clone())),
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => Ok(IrExpr::symbol(match symbol {
            CoreSymbol::Start => "start",
            CoreSymbol::End => "end",
            CoreSymbol::Xy => "xy",
            CoreSymbol::Yz => "yz",
            CoreSymbol::Xz => "xz",
        })),
        CoreNodeKind::Literal(CoreLiteral::Point2([x, y])) => {
            Ok(IrExpr::list(vec![IrExpr::number(*x), IrExpr::number(*y)]))
        }
        CoreNodeKind::Literal(CoreLiteral::Point3([x, y, z])) => Ok(IrExpr::list(vec![
            IrExpr::number(*x),
            IrExpr::number(*y),
            IrExpr::number(*z),
        ])),
        CoreNodeKind::Reference(CoreReference::Local(name)) => Ok(IrExpr::symbol(name.clone())),
        CoreNodeKind::Reference(CoreReference::Node(id)) => refs
            .get(&id.raw())
            .map(|name| IrExpr::symbol(name.clone()))
            .ok_or_else(|| unsupported(format!("Unsupported Core node reference {:?}.", id))),
        CoreNodeKind::Reference(CoreReference::Parameter(id)) => param_names
            .get(&id.raw())
            .map(|name| IrExpr::symbol(name.clone()))
            .ok_or_else(|| unsupported(format!("Unsupported Core parameter reference {:?}.", id))),
        CoreNodeKind::Reference(other) => Err(unsupported(format!(
            "Unsupported Core IR reference in legacy bridge: {:?}.",
            other
        ))),
        CoreNodeKind::Build { bindings, result } => {
            let mut items = vec![IrExpr::symbol("build")];
            let mut nested = refs.clone();
            for binding in bindings {
                nested.insert(binding.value.id.raw(), binding.name.clone());
                items.push(IrExpr::list(vec![
                    IrExpr::symbol("shape"),
                    IrExpr::symbol(binding.name.clone()),
                    core_node_to_ir_expr(&binding.value, param_names, &nested)?,
                ]));
            }
            items.push(IrExpr::list(vec![
                IrExpr::symbol("result"),
                core_node_to_ir_expr(result, param_names, &nested)?,
            ]));
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::Let { bindings, body } => {
            let binding_values = bindings
                .iter()
                .map(|binding| {
                    Ok(IrExpr::list(vec![
                        IrExpr::symbol(binding.name.clone()),
                        core_node_to_ir_expr(&binding.value, param_names, refs)?,
                    ]))
                })
                .collect::<AppResult<Vec<_>>>()?;
            Ok(IrExpr::list(vec![
                IrExpr::symbol("let"),
                IrExpr::list(binding_values),
                core_node_to_ir_expr(body, param_names, refs)?,
            ]))
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => Ok(IrExpr::list(vec![
            IrExpr::symbol("if"),
            core_node_to_ir_expr(condition, param_names, refs)?,
            core_node_to_ir_expr(then_branch, param_names, refs)?,
            core_node_to_ir_expr(else_branch, param_names, refs)?,
        ])),
        CoreNodeKind::Call { op, args, keywords } => {
            let mut items = vec![IrExpr::symbol(core_operation_name(op))];
            for arg in args {
                items.push(core_node_to_ir_expr(arg, param_names, refs)?);
            }
            for CoreKeywordArg { name, value } in keywords {
                items.push(IrExpr::keyword(name.clone()));
                items.push(core_node_to_ir_expr(value, param_names, refs)?);
            }
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::List(items) => Ok(IrExpr::list(
            items
                .iter()
                .map(|item| core_node_to_ir_expr(item, param_names, refs))
                .collect::<AppResult<Vec<_>>>()?,
        )),
        CoreNodeKind::Group(items) => Ok(IrExpr::list(
            items
                .iter()
                .map(|item| core_node_to_ir_expr(item, param_names, refs))
                .collect::<AppResult<Vec<_>>>()?,
        )),
    }
}

fn core_operation_name(op: &CoreOperation) -> String {
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
        CoreOperation::Frame(CoreFrameOp::PathFrame) => "path-frame".to_string(),
        CoreOperation::Frame(CoreFrameOp::Place) => "place".to_string(),
        CoreOperation::Frame(CoreFrameOp::ClipBox) => "clip-box".to_string(),
        CoreOperation::Meta(CoreMetaOp::Group) => "compound".to_string(),
        CoreOperation::Meta(CoreMetaOp::Comment) => "meta".to_string(),
        CoreOperation::Meta(CoreMetaOp::Annotate) => "build".to_string(),
        CoreOperation::Custom(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_build_expr_accepts_shapes_then_result() {
        let value = ir_parse("(build (shape track (box 10 10 10)) (shape cutout (cylinder 2 8)) (result (cut track cutout)))")
            .expect("parse");
        let build = parse_build_expr(&value).expect("build");
        assert_eq!(build.bindings.len(), 2);
        assert_eq!(build.bindings[0].name, "track");
        assert_eq!(build.bindings[1].name, "cutout");
    }

    #[test]
    fn parse_build_expr_rejects_missing_result() {
        let value = ir_parse("(build (shape track (box 10 10 10)))").expect("parse");
        let err = parse_build_expr(&value).expect_err("missing result");
        assert!(err
            .to_string()
            .contains("requires exactly one `(result ...)`"));
    }

    #[test]
    fn parse_build_expr_rejects_rebinding() {
        let value = ir_parse(
            "(build (shape track (box 10 10 10)) (shape track (cylinder 2 8)) (result track))",
        )
        .expect("parse");
        let err = parse_build_expr(&value).expect_err("rebind");
        assert!(err.to_string().contains("cannot rebind shape `track`"));
    }

    #[test]
    fn parse_typed_build_expr_reads_part_expr_without_lexpr_value() {
        let model =
            parse_model("(model (part body (build (shape track (box 10 10 10)) (result track))))")
                .expect("model");
        let build = parse_typed_build_expr(&model.parts[0].expr).expect("typed build");
        assert_eq!(build.bindings.len(), 1);
        assert_eq!(build.bindings[0].name, "track");
        assert_eq!(build.result.as_symbol(), Some("track"));
    }
}
