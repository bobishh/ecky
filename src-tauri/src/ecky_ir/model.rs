use std::collections::BTreeMap;

use lexpr::Value;

#[cfg(test)]
use crate::ecky_core_ir::{
    CoreArrayOp, CoreBooleanOp, CoreFrameOp, CoreLiteral, CoreMetaOp, CoreNode, CoreNodeKind,
    CoreOperation, CorePart, CorePathOp, CorePrimitive, CoreReference, CoreSelectorPayload,
    CoreSurfaceOp, CoreSymbol, CoreTransformOp,
};
use crate::ecky_core_ir::{
    CoreParameter, CoreParameterKind, CoreParameterValue, CoreProgram, CoreValueKind,
};
use crate::models::{
    AppResult, DesignParams, ParamValue, ParsedParamsResult, SelectOption, SelectValue, UiField,
};

#[cfg(test)]
use super::edge_ops::{
    edge_selector_spec_from_core_payload, face_selector_spec_from_core_payload,
    parse_edge_selector_spec, parse_face_selector_spec, EdgeSelectorSpec, FaceSelectorSpec,
};
#[cfg(not(test))]
use super::edge_ops::{
    parse_edge_selector_spec, parse_face_selector_spec, EdgeSelectorSpec, FaceSelectorSpec,
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
pub(crate) enum IrSelectorExpr {
    Edge(EdgeSelectorSpec),
    Face(FaceSelectorSpec),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum IrExpr {
    Number(f64),
    Boolean(bool),
    String(String),
    Symbol(String),
    Keyword(String),
    Selector(IrSelectorExpr),
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
            "Current `.ecky` runtime only supports scalar atoms and proper lists.",
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

pub(crate) fn expr_parse_edge_selector_spec(
    value: &IrExpr,
    context: &str,
) -> AppResult<EdgeSelectorSpec> {
    match value {
        IrExpr::Selector(IrSelectorExpr::Edge(selector)) => Ok(selector.clone()),
        IrExpr::Selector(IrSelectorExpr::Face(_)) => Err(validation(format!(
            "Expected edge selector for {}.",
            context
        ))),
        _ => Err(validation(format!(
            "Expected typed edge selector for {}.",
            context
        ))),
    }
}

pub(crate) fn expr_parse_face_selector_spec(
    value: &IrExpr,
    context: &str,
) -> AppResult<FaceSelectorSpec> {
    match value {
        IrExpr::Selector(IrSelectorExpr::Face(selector)) => Ok(selector.clone()),
        IrExpr::Selector(IrSelectorExpr::Edge(_)) => Err(validation(format!(
            "Expected face selector for {}.",
            context
        ))),
        _ => Err(validation(format!(
            "Expected typed face selector for {}.",
            context
        ))),
    }
}

fn materialize_edge_selector_expr(value: IrExpr) -> AppResult<IrExpr> {
    match value {
        IrExpr::Selector(IrSelectorExpr::Edge(selector)) => {
            Ok(IrExpr::Selector(IrSelectorExpr::Edge(selector)))
        }
        IrExpr::Selector(IrSelectorExpr::Face(_)) => {
            Err(validation("Expected edge selector for edge selection."))
        }
        other => Ok(IrExpr::Selector(IrSelectorExpr::Edge(
            parse_edge_selector_spec(&expr_parse_stringish(&other, "edge selection")?)?,
        ))),
    }
}

fn materialize_face_selector_expr(value: IrExpr) -> AppResult<IrExpr> {
    match value {
        IrExpr::Selector(IrSelectorExpr::Face(selector)) => {
            Ok(IrExpr::Selector(IrSelectorExpr::Face(selector)))
        }
        IrExpr::Selector(IrSelectorExpr::Edge(_)) => {
            Err(validation("Expected face selector for face selection."))
        }
        other => Ok(IrExpr::Selector(IrSelectorExpr::Face(
            parse_face_selector_spec(&expr_parse_stringish(&other, "face selection")?)?,
        ))),
    }
}

fn materialize_keyword_selector(name: &str, value: IrExpr) -> AppResult<IrExpr> {
    match name {
        "edges" => materialize_edge_selector_expr(value),
        "faces" => materialize_face_selector_expr(value),
        _ => Ok(value),
    }
}

pub(crate) fn materialize_selector_nodes(value: IrExpr) -> AppResult<IrExpr> {
    match value {
        IrExpr::List(items) => {
            let mut rewritten = items
                .into_iter()
                .map(materialize_selector_nodes)
                .collect::<AppResult<Vec<_>>>()?;
            if rewritten.first().and_then(IrExpr::as_symbol).is_some() {
                let mut index = 1usize;
                while index + 1 < rewritten.len() {
                    if let Some(keyword) = expr_keyword_name(&rewritten[index]) {
                        rewritten[index + 1] =
                            materialize_keyword_selector(keyword, rewritten[index + 1].dup())?;
                        index += 2;
                        continue;
                    }
                    index += 1;
                }
            }
            Ok(IrExpr::List(rewritten))
        }
        other => Ok(other),
    }
}

#[cfg(test)]
fn ir_expr_from_core_selector_payload(payload: &CoreSelectorPayload) -> AppResult<IrExpr> {
    match payload {
        CoreSelectorPayload::EdgeAll
        | CoreSelectorPayload::EdgeClauses(_)
        | CoreSelectorPayload::EdgeTag(_)
        | CoreSelectorPayload::EdgeTargetIds(_) => Ok(IrExpr::Selector(IrSelectorExpr::Edge(
            edge_selector_spec_from_core_payload(payload)?,
        ))),
        CoreSelectorPayload::FaceClauses(_)
        | CoreSelectorPayload::FaceTag(_)
        | CoreSelectorPayload::FaceTargetIds(_) => Ok(IrExpr::Selector(IrSelectorExpr::Face(
            face_selector_spec_from_core_payload(payload)?,
        ))),
    }
}

pub(super) fn inline_let_expr(value: &IrExpr) -> AppResult<IrExpr> {
    inline_let_expr_with_scope(value, &BTreeMap::new())
}

fn let_scope_with_bindings(
    bindings: &[IrExpr],
    scope: &BTreeMap<String, IrExpr>,
    sequential: bool,
) -> AppResult<BTreeMap<String, IrExpr>> {
    let mut next_scope = scope.clone();
    for binding in bindings {
        let pair = expr_list_items(binding, "let binding")?;
        if pair.len() != 2 && pair.len() != 4 {
            return Err(validation(
                "Each `let`/`let*` binding must be `(name expr)`.",
            ));
        }
        if pair.len() == 4 {
            let value_kind_keyword = expr_keyword_name(&pair[2]).filter(|k| *k == "value-kind");
            let value_kind_tag = pair[3].as_symbol().and_then(parse_value_kind_tag);
            if value_kind_keyword.is_none() || value_kind_tag.is_none() {
                return Err(validation(
                    "Typed `let`/`let*` bindings must use `:value-kind <tag>` metadata.",
                ));
            }
        }
        let name = expr_parse_stringish(&pair[0], "let binding name")?;
        let binding_scope = if sequential { &next_scope } else { scope };
        let bound = inline_let_expr_with_scope(&pair[1], binding_scope)?;
        next_scope.insert(name, bound);
    }
    Ok(next_scope)
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
                Some("let") | Some("let*") => {
                    if items.len() != 3 {
                        return Err(validation("`let`/`let*` expects bindings and a body."));
                    }
                    let bindings = expr_list_items(&items[1], "let bindings")?;
                    let local_scope =
                        let_scope_with_bindings(bindings, scope, head.as_symbol() == Some("let*"))?;
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
    pub(super) value_kind: Option<CoreValueKind>,
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
    pub(super) value_kind: Option<CoreValueKind>,
}

#[cfg(test)]
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
        return Err(validation("`.ecky` source must start with `(model ...)`."));
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
                    "Top-level node `{}` is not supported by current `.ecky` runtime.",
                    other
                )))
            }
        }
    }

    if parts.is_empty() {
        return Err(validation(
            "`.ecky` models need at least one `(part ...)` node.",
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
                "Param kind `{}` is not supported by current `.ecky` runtime.",
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
        expr: materialize_selector_nodes(IrExpr::from_value(&expr)?)?,
        value_kind: None,
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
                if stmt.len() != 3 && stmt.len() != 5 {
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
                let value_kind = if stmt.len() == 5 {
                    expr_keyword_name(&stmt[3])
                        .filter(|k| *k == "value-kind")
                        .and_then(|_| stmt[4].as_symbol())
                        .and_then(parse_value_kind_tag)
                } else {
                    None
                };
                bindings.push(TypedBuildBinding {
                    name,
                    expr: stmt[2].dup(),
                    value_kind,
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

fn parsed_params_from_ir_params(params: &[IrParam]) -> ParsedParamsResult {
    ParsedParamsResult {
        fields: params.iter().map(|param| param.field.clone()).collect(),
        params: params
            .iter()
            .map(|param| (param.field.key().to_string(), param.default_value.clone()))
            .collect(),
    }
}

pub(super) fn parsed_params_from_model(model: &IrModel) -> ParsedParamsResult {
    parsed_params_from_ir_params(&model.params)
}

pub(crate) fn parsed_params_from_core_program(
    program: &CoreProgram,
) -> AppResult<ParsedParamsResult> {
    let params = program
        .parameters
        .iter()
        .map(core_param_to_ir_param)
        .collect::<AppResult<Vec<_>>>()?;
    Ok(parsed_params_from_ir_params(&params))
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

pub(crate) fn core_program_param_defaults(
    program: &CoreProgram,
) -> AppResult<BTreeMap<String, ParamValue>> {
    program
        .parameters
        .iter()
        .map(|param| {
            Ok((
                param.key.clone(),
                core_param_value_to_param_value(&param.default_value)?,
            ))
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn core_part_to_ir_part(
    part: &CorePart,
    param_names: &BTreeMap<u64, String>,
) -> AppResult<IrPart> {
    let mut used_local_names = BTreeMap::new();
    Ok(IrPart {
        part_id: part.key.clone(),
        label: part.label.clone(),
        expr: materialize_selector_nodes(core_node_to_ir_expr(
            &part.root,
            param_names,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut used_local_names,
        )?)?,
        value_kind: Some(part.root.value_kind),
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

#[cfg(test)]
pub(crate) fn core_node_to_ir_expr(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    refs: &BTreeMap<u64, String>,
    locals: &BTreeMap<String, String>,
    used_local_names: &mut BTreeMap<String, usize>,
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
            CoreSymbol::Min => "min",
            CoreSymbol::Center => "center",
            CoreSymbol::Max => "max",
        })),
        CoreNodeKind::Literal(CoreLiteral::Point2([x, y])) => {
            Ok(IrExpr::list(vec![IrExpr::number(*x), IrExpr::number(*y)]))
        }
        CoreNodeKind::Literal(CoreLiteral::Point3([x, y, z])) => Ok(IrExpr::list(vec![
            IrExpr::number(*x),
            IrExpr::number(*y),
            IrExpr::number(*z),
        ])),
        CoreNodeKind::Reference(CoreReference::Local(name)) => Ok(IrExpr::symbol(
            locals.get(name).cloned().unwrap_or_else(|| name.clone()),
        )),
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
            let mut nested_locals = locals.clone();
            for binding in bindings {
                let ir_name = allocate_legacy_local_name(&binding.name, used_local_names);
                let mut shape_items = vec![
                    IrExpr::symbol("shape"),
                    IrExpr::symbol(ir_name.clone()),
                    core_node_to_ir_expr(
                        &binding.value,
                        param_names,
                        &nested,
                        &nested_locals,
                        used_local_names,
                    )?,
                ];
                if binding.value.value_kind != CoreValueKind::Any {
                    shape_items.push(IrExpr::keyword("value-kind"));
                    shape_items.push(IrExpr::symbol(core_value_kind_tag(
                        binding.value.value_kind,
                    )));
                }
                items.push(IrExpr::list(shape_items));
                nested.insert(binding.value.id.raw(), ir_name.clone());
                nested_locals.insert(binding.name.clone(), ir_name);
            }
            items.push(IrExpr::list(vec![
                IrExpr::symbol("result"),
                core_node_to_ir_expr(
                    result,
                    param_names,
                    &nested,
                    &nested_locals,
                    used_local_names,
                )?,
            ]));
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested_locals = locals.clone();
            let ir_binding_names = bindings
                .iter()
                .map(|binding| {
                    (
                        binding.name.clone(),
                        allocate_legacy_local_name(&binding.name, used_local_names),
                    )
                })
                .collect::<Vec<_>>();
            let binding_values = bindings
                .iter()
                .zip(ir_binding_names.iter())
                .map(|(binding, (_, ir_name))| {
                    let mut pair = vec![
                        IrExpr::symbol(ir_name.clone()),
                        core_node_to_ir_expr(
                            &binding.value,
                            param_names,
                            refs,
                            locals,
                            used_local_names,
                        )?,
                    ];
                    if binding.value.value_kind != CoreValueKind::Any {
                        pair.push(IrExpr::keyword("value-kind"));
                        pair.push(IrExpr::symbol(core_value_kind_tag(
                            binding.value.value_kind,
                        )));
                    }
                    Ok(IrExpr::list(pair))
                })
                .collect::<AppResult<Vec<_>>>()?;
            for (original_name, ir_name) in ir_binding_names {
                nested_locals.insert(original_name, ir_name);
            }
            Ok(IrExpr::list(vec![
                IrExpr::symbol("let"),
                IrExpr::list(binding_values),
                core_node_to_ir_expr(body, param_names, refs, &nested_locals, used_local_names)?,
            ]))
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => Ok(IrExpr::list(vec![
            IrExpr::symbol("if"),
            core_node_to_ir_expr(condition, param_names, refs, locals, used_local_names)?,
            core_node_to_ir_expr(then_branch, param_names, refs, locals, used_local_names)?,
            core_node_to_ir_expr(else_branch, param_names, refs, locals, used_local_names)?,
        ])),
        CoreNodeKind::Call { op, args, keywords } => {
            let mut items = vec![IrExpr::symbol(core_operation_name(op))];
            for arg in args {
                items.push(core_node_to_ir_expr(
                    arg,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
            }
            for keyword in keywords {
                items.push(IrExpr::keyword(keyword.name.clone()));
                items.push(match (keyword.name.as_str(), keyword.selector_payload()) {
                    ("edges", None) => {
                        return Err(validation(
                            "CoreProgram `:edges` keyword requires selector payload.",
                        ))
                    }
                    ("faces", None) => {
                        return Err(validation(
                            "CoreProgram `:faces` keyword requires selector payload.",
                        ))
                    }
                    (
                        "edges",
                        Some(
                            CoreSelectorPayload::FaceClauses(_)
                            | CoreSelectorPayload::FaceTargetIds(_),
                        ),
                    ) => {
                        return Err(validation(
                            "CoreProgram `:edges` keyword requires edge selector payload.",
                        ))
                    }
                    (
                        "faces",
                        Some(
                            CoreSelectorPayload::EdgeAll
                            | CoreSelectorPayload::EdgeClauses(_)
                            | CoreSelectorPayload::EdgeTargetIds(_),
                        ),
                    ) => {
                        return Err(validation(
                            "CoreProgram `:faces` keyword requires face selector payload.",
                        ))
                    }
                    (_, Some(selector)) => ir_expr_from_core_selector_payload(selector)?,
                    (_, None) => core_node_to_ir_expr(
                        keyword.source_node(),
                        param_names,
                        refs,
                        locals,
                        used_local_names,
                    )?,
                });
            }
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::Range { start, end } => Ok(IrExpr::list(vec![
            IrExpr::symbol("range"),
            core_node_to_ir_expr(start, param_names, refs, locals, used_local_names)?,
            core_node_to_ir_expr(end, param_names, refs, locals, used_local_names)?,
        ])),
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => {
            let mut nested_locals = locals.clone();
            let mut ir_params = Vec::new();
            for param in params {
                let ir_name = allocate_legacy_local_name(param, used_local_names);
                nested_locals.insert(param.clone(), ir_name.clone());
                ir_params.push(IrExpr::symbol(ir_name));
            }
            let mut items = vec![
                IrExpr::symbol("map"),
                IrExpr::list(vec![
                    IrExpr::symbol("lambda"),
                    IrExpr::list(ir_params),
                    core_node_to_ir_expr(
                        body,
                        param_names,
                        refs,
                        &nested_locals,
                        used_local_names,
                    )?,
                ]),
            ];
            for source in sources {
                items.push(core_node_to_ir_expr(
                    source,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
            }
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::Apply { op, args, list } => {
            let mut items = vec![
                IrExpr::symbol("apply"),
                IrExpr::symbol(core_operation_name(op)),
            ];
            for arg in args {
                items.push(core_node_to_ir_expr(
                    arg,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
            }
            items.push(core_node_to_ir_expr(
                list,
                param_names,
                refs,
                locals,
                used_local_names,
            )?);
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::List(items) => Ok(IrExpr::list(
            items
                .iter()
                .map(|item| core_node_to_ir_expr(item, param_names, refs, locals, used_local_names))
                .collect::<AppResult<Vec<_>>>()?,
        )),
        CoreNodeKind::Group(items) => Ok(IrExpr::list(
            items
                .iter()
                .map(|item| core_node_to_ir_expr(item, param_names, refs, locals, used_local_names))
                .collect::<AppResult<Vec<_>>>()?,
        )),
    }
}

pub(crate) fn allocate_legacy_local_name(name: &str, used: &mut BTreeMap<String, usize>) -> String {
    let mut base = name.trim_start_matches('#').trim().replace('#', "");
    if base.is_empty() {
        base = "value".to_string();
    }
    let mut normalized = String::with_capacity(base.len());
    for ch in base.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => normalized.push(ch),
            _ => normalized.push('_'),
        }
    }
    if normalized.is_empty() {
        normalized.push_str("value");
    }
    if normalized
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
    {
        normalized.insert_str(0, "v_");
    }

    let slot = used.entry(normalized.clone()).or_insert(0);
    *slot += 1;
    if *slot == 1 {
        normalized
    } else {
        format!("{}_{}", normalized, *slot)
    }
}

#[cfg(test)]
fn core_value_kind_tag(kind: CoreValueKind) -> &'static str {
    match kind {
        CoreValueKind::Any => "any",
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

pub(super) fn parse_value_kind_tag(tag: &str) -> Option<CoreValueKind> {
    match tag {
        "any" => Some(CoreValueKind::Any),
        "number" => Some(CoreValueKind::Number),
        "boolean" => Some(CoreValueKind::Boolean),
        "text" => Some(CoreValueKind::Text),
        "list" => Some(CoreValueKind::List),
        "point2" => Some(CoreValueKind::Point2),
        "point3" => Some(CoreValueKind::Point3),
        "sketch" => Some(CoreValueKind::Sketch),
        "path" => Some(CoreValueKind::Path),
        "frame" => Some(CoreValueKind::Frame),
        "compound" => Some(CoreValueKind::Compound),
        "solid" => Some(CoreValueKind::Solid),
        _ => None,
    }
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_scheme::compile_to_core_program;

    fn collect_symbols(expr: &IrExpr, out: &mut Vec<String>) {
        match expr {
            IrExpr::Symbol(symbol) => out.push(symbol.clone()),
            IrExpr::Selector(_) => {}
            IrExpr::List(items) => {
                for item in items {
                    collect_symbols(item, out);
                }
            }
            _ => {}
        }
    }

    fn contains_edge_selector(expr: &IrExpr) -> bool {
        match expr {
            IrExpr::Selector(IrSelectorExpr::Edge(_)) => true,
            IrExpr::List(items) => items.iter().any(contains_edge_selector),
            _ => false,
        }
    }

    fn contains_face_selector(expr: &IrExpr) -> bool {
        match expr {
            IrExpr::Selector(IrSelectorExpr::Face(_)) => true,
            IrExpr::List(items) => items.iter().any(contains_face_selector),
            _ => false,
        }
    }

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

    #[test]
    fn inline_let_expr_keeps_parallel_binding_references_unresolved() {
        let value = IrExpr::from_value(&ir_parse("(let ((a 2) (b (+ a 1))) b)").expect("parse"))
            .expect("expr");
        let rewritten = inline_let_expr(&value).expect("inline let");
        let items = rewritten.as_list().expect("rewritten list");
        assert_eq!(items[0].as_symbol(), Some("+"));
        assert_eq!(items[1].as_symbol(), Some("a"));
    }

    #[test]
    fn inline_let_expr_expands_let_star_sequentially() {
        let value = IrExpr::from_value(&ir_parse("(let* ((a 2) (b (+ a 1))) b)").expect("parse"))
            .expect("expr");
        let rewritten = inline_let_expr(&value).expect("inline let*");
        let items = rewritten.as_list().expect("rewritten list");
        assert_eq!(items[0].as_symbol(), Some("+"));
        assert_eq!(items[1].as_f64(), Some(2.0));
    }

    #[test]
    fn inline_let_expr_accepts_typed_binding_metadata() {
        let value = IrExpr::list(vec![
            IrExpr::symbol("let"),
            IrExpr::list(vec![IrExpr::list(vec![
                IrExpr::symbol("r"),
                IrExpr::number(4.0),
                IrExpr::keyword("value-kind"),
                IrExpr::symbol("number"),
            ])]),
            IrExpr::list(vec![IrExpr::symbol("circle"), IrExpr::symbol("r")]),
        ]);
        let rewritten = inline_let_expr(&value).expect("inline typed let");
        assert_eq!(
            rewritten,
            IrExpr::list(vec![IrExpr::symbol("circle"), IrExpr::number(4.0)])
        );
    }

    #[test]
    fn core_program_to_model_sanitizes_hygienic_local_symbols() {
        let program = compile_to_core_program(
            "(define (track total)\n  (let* ((total-l (+ total 2)) (total-l2 (+ total-l 3)))\n    (translate total-l2 0 0 (box total-l2 10 10))))\n(model (params (number total 20)) (part body (track total)))",
        )
        .expect("program");
        let model = core_program_to_model(&program).expect("model");
        let mut symbols = Vec::new();
        collect_symbols(&model.parts[0].expr, &mut symbols);
        assert!(
            symbols.iter().all(|symbol| !symbol.contains("##")),
            "hygienic symbols leaked into legacy bridge: {:?}",
            symbols
        );
        assert!(
            symbols.iter().any(|symbol| symbol.contains("total-l2")),
            "expected readable local symbol in {:?}",
            symbols
        );
    }

    #[test]
    fn parse_model_materializes_edge_selector_node() {
        let model = parse_model(
            "(model (part body (fillet 1 :edges \"target-id:body:edge:0:0-0-0_1-0-0\" (box 1 1 1))))",
        )
        .expect("model");
        let items = model.parts[0].expr.as_list().expect("call");
        match &items[3] {
            IrExpr::Selector(IrSelectorExpr::Edge(selector)) => {
                assert_eq!(
                    selector.target_ids(),
                    Some(&["body:edge:0:0-0-0_1-0-0".to_string()][..])
                );
            }
            other => panic!("expected edge selector node, got {other:?}"),
        }
    }

    #[test]
    fn parse_model_materializes_face_selector_node() {
        let model = parse_model(
            "(model (part body (shell 1 :faces \"target-id:body:face:0:0-0-1:1\" (box 1 1 1))))",
        )
        .expect("model");
        let items = model.parts[0].expr.as_list().expect("call");
        match &items[3] {
            IrExpr::Selector(IrSelectorExpr::Face(selector)) => {
                assert_eq!(selector.target_ids(), &["body:face:0:0-0-1:1".to_string()]);
            }
            other => panic!("expected face selector node, got {other:?}"),
        }
    }

    #[test]
    fn expr_parse_edge_selector_spec_rejects_raw_string_expr() {
        let err = expr_parse_edge_selector_spec(&IrExpr::string("left+vertical"), "edge selection")
            .expect_err("raw string should fail");
        assert!(err.to_string().contains("Expected typed edge selector"));
    }

    #[test]
    fn expr_parse_face_selector_spec_rejects_raw_string_expr() {
        let err = expr_parse_face_selector_spec(
            &IrExpr::string("target-id:body:face:0:0-0-1:1"),
            "face selection",
        )
        .expect_err("raw string should fail");
        assert!(err.to_string().contains("Expected typed face selector"));
    }

    #[test]
    fn core_program_to_model_materializes_selector_nodes() {
        let program = compile_to_core_program(
            "(model (part body (fillet 1 :edges \"target-id:body:edge:0:0-0-0_1-0-0\" (box 1 1 1))))",
        )
        .expect("program");
        let model = core_program_to_model(&program).expect("model");
        assert!(
            contains_edge_selector(&model.parts[0].expr),
            "expected selector in {:?}",
            model.parts[0].expr
        );
    }

    #[test]
    fn core_program_to_model_materializes_face_selector_nodes() {
        let program = compile_to_core_program(
            "(model (part body (shell 1 :faces \"target-id:body:face:0:0-0-1:1\" (box 1 1 1))))",
        )
        .expect("program");
        let model = core_program_to_model(&program).expect("model");
        assert!(
            contains_face_selector(&model.parts[0].expr),
            "expected face selector in {:?}",
            model.parts[0].expr
        );
    }

    #[test]
    fn core_program_to_model_rejects_missing_selector_payload_on_edges_keyword() {
        let mut program = compile_to_core_program(
            "(model (part body (fillet 1 :edges \"left+vertical\" (box 1 1 1))))",
        )
        .expect("program");
        let CoreNodeKind::Call { keywords, .. } = &mut program.parts[0].root.kind else {
            panic!("expected call");
        };
        keywords[0].set_selector_payload(None);

        let err = match core_program_to_model(&program) {
            Ok(_) => panic!("missing selector payload should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("CoreProgram `:edges` keyword requires selector payload"),
            "{err}"
        );
    }

    #[test]
    fn core_program_to_model_rejects_wrong_kind_selector_payload_on_edges_keyword() {
        let mut program = compile_to_core_program(
            "(model (part body (fillet 1 :edges \"left+vertical\" (box 1 1 1))))",
        )
        .expect("program");
        let CoreNodeKind::Call { keywords, .. } = &mut program.parts[0].root.kind else {
            panic!("expected call");
        };
        keywords[0].set_selector_payload(Some(CoreSelectorPayload::FaceTargetIds(vec![
            "body:face:0:0-0-1:1".into(),
        ])));

        let err = match core_program_to_model(&program) {
            Ok(_) => panic!("wrong-kind selector payload should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("CoreProgram `:edges` keyword requires edge selector payload"),
            "{err}"
        );
    }
}
