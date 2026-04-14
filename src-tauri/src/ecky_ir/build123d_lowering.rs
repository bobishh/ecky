use std::collections::{BTreeMap, BTreeSet};

use crate::models::{AppResult, ParamValue};

use super::model::{
    expr_head_symbol as head_symbol, expr_keyword_name as keyword_name,
    expr_list_items as list_items, expr_parse_stringish as parse_stringish, parse_model,
    parse_typed_build_expr as parse_build_expr, IrExpr as Value, IrModel,
};
use super::shared::{unsupported, validation};

trait IrExprVecExt {
    fn to_vec(&self) -> Option<Vec<Value>>;
}

impl IrExprVecExt for Value {
    fn to_vec(&self) -> Option<Vec<Value>> {
        self.as_list()
            .map(|items| items.iter().map(Value::dup).collect())
    }
}

pub fn lower_to_build123d(source: &str) -> AppResult<String> {
    let model = parse_model(source)?;
    lower_model_to_build123d(&model)
}

pub(crate) fn lower_model_to_build123d(model: &IrModel) -> AppResult<String> {
    let mut lowerer = ExprLowerer::new(&model);
    lowerer.lower_model()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum B123dGeomKind {
    Sketch2d,
    Solid3d,
    Path3d,
}

impl B123dGeomKind {
    fn noun(&self) -> &'static str {
        match self {
            Self::Sketch2d => "2D sketch",
            Self::Solid3d => "3D solid",
            Self::Path3d => "3D path",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LoweredListKind {
    Point2d,
    Point3d,
    Pair,
    Triple,
    Scalar,
    Empty,
    Mixed,
}

impl LoweredListKind {
    fn noun(&self) -> &'static str {
        match self {
            Self::Point2d => "2D point list",
            Self::Point3d => "3D point list",
            Self::Pair => "pair list",
            Self::Triple => "triple list",
            Self::Scalar => "scalar list",
            Self::Empty => "empty list",
            Self::Mixed => "mixed list",
        }
    }
}

#[cfg(test)]
#[path = "build123d_lowering_tests.rs"]
mod build123d_lowering_tests;

#[derive(Clone, Debug)]
struct LoweredGeom {
    var: String,
    kind: B123dGeomKind,
}

#[derive(Clone, Debug)]
struct LoweredList {
    items: Vec<Value>,
    kind: LoweredListKind,
    source_op: Option<String>,
}

impl LoweredList {
    fn new(items: Vec<Value>, kind: LoweredListKind, source_op: Option<String>) -> Self {
        Self {
            items,
            kind,
            source_op,
        }
    }

    fn source_suffix(&self) -> String {
        match self.source_op.as_deref() {
            Some(op) => format!(" from list op `{}`", op),
            None => " from list literal".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
enum LoweredBinding {
    Geom(LoweredGeom),
    List(LoweredList),
    Frame(String),
    Number(String),
    Boolean(String),
    Stringish(String),
}

#[derive(Clone, Debug)]
struct LoweringScope<'a> {
    params: &'a BTreeMap<String, ParamValue>,
    locals: Vec<BTreeMap<String, LoweredBinding>>,
}

impl<'a> LoweringScope<'a> {
    fn new(params: &'a BTreeMap<String, ParamValue>) -> Self {
        Self {
            params,
            locals: Vec::new(),
        }
    }

    fn with_frame(&self, frame: BTreeMap<String, LoweredBinding>) -> Self {
        let mut locals = self.locals.clone();
        locals.push(frame);
        Self {
            params: self.params,
            locals,
        }
    }

    fn resolve_binding(&self, symbol: &str) -> Option<&LoweredBinding> {
        self.locals.iter().rev().find_map(|frame| frame.get(symbol))
    }
}

fn lower_num_expr(value: &Value, scope: &LoweringScope<'_>) -> AppResult<String> {
    if let Some(n) = value.as_f64() {
        return Ok(fmt_f64(n));
    }
    if let Some(sym) = value.as_symbol() {
        if let Some(binding) = scope.resolve_binding(sym) {
            return match binding {
                LoweredBinding::Number(expr) => Ok(expr.clone()),
                _ => Err(unsupported(format!(
                    "Symbol `{}` is not a numeric binding in this context.",
                    sym
                ))),
            };
        }
        return match scope.params.get(sym) {
            Some(ParamValue::Number(d)) => {
                Ok(format!("float(params.get({:?}, {}))", sym, fmt_f64(*d)))
            }
            Some(_) => Err(unsupported(format!(
                "Symbol `{}` is not a numeric parameter.",
                sym
            ))),
            None => Err(validation(format!("Unknown symbol `{}`.", sym))),
        };
    }
    let items = list_items(value, "numeric expression")?;
    let op = head_symbol(&items, "numeric expression")?;
    let args = &items[1..];
    match op {
        "if" => {
            if args.len() != 3 {
                return Err(validation(
                    "Numeric `if` expects condition, then-expr, else-expr.",
                ));
            }
            let cond = lower_bool_expr(&args[0], scope)?;
            let then_expr = lower_num_expr(&args[1], scope)?;
            let else_expr = lower_num_expr(&args[2], scope)?;
            Ok(format!("({then_expr} if {cond} else {else_expr})"))
        }
        "intersect-x" => {
            if args.len() != 2 {
                return Err(validation("`intersect-x` expects shape and z."));
            }
            let shape = match args[0].as_symbol() {
                Some(sym) => match scope.resolve_binding(sym) {
                    Some(LoweredBinding::Geom(geom)) => geom.var.clone(),
                    Some(_) => {
                        return Err(unsupported(format!(
                            "Symbol `{}` is not a geometry binding in `intersect-x`.",
                            sym
                        )))
                    }
                    None => return Err(validation(format!("Unknown symbol `{}`.", sym))),
                },
                None => return Err(validation("`intersect-x` shape must be a geometry symbol.")),
            };
            let z = lower_num_expr(&args[1], scope)?;
            Ok(format!("_ecky_intersect_x({shape}, {z})"))
        }
        "+" => {
            if args.is_empty() {
                return Ok("0.0".to_string());
            }
            let parts = lower_num_list(args, scope)?;
            Ok(format!("({})", parts.join(" + ")))
        }
        "-" => {
            if args.is_empty() {
                return Err(validation("`-` expects at least one argument."));
            }
            if args.len() == 1 {
                return Ok(format!("(-{})", lower_num_expr(&args[0], scope)?));
            }
            let first = lower_num_expr(&args[0], scope)?;
            let rest = lower_num_list(&args[1..], scope)?;
            Ok(format!("({} - {})", first, rest.join(" - ")))
        }
        "*" => {
            if args.is_empty() {
                return Ok("1.0".to_string());
            }
            let parts = lower_num_list(args, scope)?;
            Ok(format!("({})", parts.join(" * ")))
        }
        "/" => {
            if args.len() != 2 {
                return Err(validation("`/` expects exactly two arguments."));
            }
            let a = lower_num_expr(&args[0], scope)?;
            let b = lower_num_expr(&args[1], scope)?;
            Ok(format!("({a} / {b})"))
        }
        "min" => {
            let parts = lower_num_list(args, scope)?;
            Ok(format!("min({})", parts.join(", ")))
        }
        "max" => {
            let parts = lower_num_list(args, scope)?;
            Ok(format!("max({})", parts.join(", ")))
        }
        "clamp" => {
            if args.len() != 3 {
                return Err(validation("`clamp` expects value, min, max."));
            }
            let v = lower_num_expr(&args[0], scope)?;
            let lo = lower_num_expr(&args[1], scope)?;
            let hi = lower_num_expr(&args[2], scope)?;
            Ok(format!("max({lo}, min({hi}, {v}))"))
        }
        "lerp" => {
            if args.len() != 3 {
                return Err(validation("`lerp` expects start, end, t."));
            }
            let s = lower_num_expr(&args[0], scope)?;
            let e = lower_num_expr(&args[1], scope)?;
            let t = lower_num_expr(&args[2], scope)?;
            Ok(format!("(({s}) + (({e}) - ({s})) * ({t}))"))
        }
        "smoothstep" => {
            if args.len() != 3 {
                return Err(validation("`smoothstep` expects edge0, edge1, x."));
            }
            let e0 = lower_num_expr(&args[0], scope)?;
            let e1 = lower_num_expr(&args[1], scope)?;
            let x = lower_num_expr(&args[2], scope)?;
            Ok(format!(
                "(lambda _t: _t*_t*(3.0-2.0*_t))\
                 (max(0.0, min(1.0, ({x} - {e0}) / ({e1} - {e0}))))"
            ))
        }
        "sin" => {
            if args.len() != 1 {
                return Err(validation("`sin` expects one argument."));
            }
            Ok(format!("math.sin({})", lower_num_expr(&args[0], scope)?))
        }
        "cos" => {
            if args.len() != 1 {
                return Err(validation("`cos` expects one argument."));
            }
            Ok(format!("math.cos({})", lower_num_expr(&args[0], scope)?))
        }
        "tan" => {
            if args.len() != 1 {
                return Err(validation("`tan` expects one argument."));
            }
            Ok(format!("math.tan({})", lower_num_expr(&args[0], scope)?))
        }
        "atan" => {
            if args.len() != 1 {
                return Err(validation("`atan` expects one argument."));
            }
            Ok(format!("math.atan({})", lower_num_expr(&args[0], scope)?))
        }
        "abs" => {
            if args.len() != 1 {
                return Err(validation("`abs` expects one argument."));
            }
            Ok(format!("abs({})", lower_num_expr(&args[0], scope)?))
        }
        "floor" => {
            if args.len() != 1 {
                return Err(validation("`floor` expects one argument."));
            }
            Ok(format!("math.floor({})", lower_num_expr(&args[0], scope)?))
        }
        "deg" => {
            if args.len() != 1 {
                return Err(validation("`deg` expects one argument."));
            }
            Ok(format!(
                "math.radians({})",
                lower_num_expr(&args[0], scope)?
            ))
        }
        "rad" => {
            if args.len() != 1 {
                return Err(validation("`rad` expects one argument."));
            }
            Ok(format!(
                "math.degrees({})",
                lower_num_expr(&args[0], scope)?
            ))
        }
        other => Err(unsupported(format!(
            "Numeric expression `{}` is not supported by the build123d lowerer.",
            other
        ))),
    }
}

fn lower_num_list(args: &[Value], scope: &LoweringScope<'_>) -> AppResult<Vec<String>> {
    args.iter().map(|a| lower_num_expr(a, scope)).collect()
}

fn infer_list_item_kind(value: &Value, scope: &LoweringScope<'_>) -> LoweredListKind {
    if lower_point_2d_expr(value, scope).is_ok() {
        return LoweredListKind::Point2d;
    }
    if lower_point_3d_expr(value, scope).is_ok() {
        return LoweredListKind::Point3d;
    }

    let Some(items) = value.to_vec() else {
        return LoweredListKind::Scalar;
    };

    if head_symbol(&items, "list item").ok() == Some("let") && items.len() == 3 {
        if let Ok(child_scope) = lower_scalar_let_scope(&items[1], scope) {
            return infer_list_item_kind(&items[2], &child_scope);
        }
    }

    let arity = if head_symbol(&items, "list item").ok() == Some("list") {
        items.len().saturating_sub(1)
    } else {
        items.len()
    };

    match arity {
        0 => LoweredListKind::Empty,
        1 => LoweredListKind::Scalar,
        2 => LoweredListKind::Pair,
        3 => LoweredListKind::Triple,
        _ => LoweredListKind::Mixed,
    }
}

fn infer_list_kind(items: &[Value], scope: &LoweringScope<'_>) -> LoweredListKind {
    let Some(first) = items.first() else {
        return LoweredListKind::Empty;
    };
    let first_kind = infer_list_item_kind(first, scope);
    if items[1..]
        .iter()
        .any(|item| infer_list_item_kind(item, scope) != first_kind)
    {
        return LoweredListKind::Mixed;
    }
    first_kind
}

fn binding_kind_noun(binding: &LoweredBinding) -> &'static str {
    match binding {
        LoweredBinding::Geom(geom) => geom.kind.noun(),
        LoweredBinding::List(list) => list.kind.noun(),
        LoweredBinding::Frame(_) => "frame",
        LoweredBinding::Number(_) => "number",
        LoweredBinding::Boolean(_) => "boolean",
        LoweredBinding::Stringish(_) => "string-like value",
    }
}

fn lower_scalar_binding(value: &Value, scope: &LoweringScope<'_>) -> AppResult<LoweredBinding> {
    let number_err = match lower_num_expr(value, scope) {
        Ok(number) => return Ok(LoweredBinding::Number(number)),
        Err(err) => err,
    };
    let bool_err = match lower_bool_expr(value, scope) {
        Ok(boolean) => return Ok(LoweredBinding::Boolean(boolean)),
        Err(err) => err,
    };
    if let Ok(stringish) = lower_stringish_expr(value, scope) {
        return Ok(LoweredBinding::Stringish(stringish));
    }
    if number_err.code == crate::models::AppErrorCode::Validation {
        return Err(number_err);
    }
    if bool_err.code == crate::models::AppErrorCode::Validation {
        return Err(bool_err);
    }
    Err(number_err)
}

fn lower_scalar_let_scope<'a>(
    bindings_value: &Value,
    scope: &LoweringScope<'a>,
) -> AppResult<LoweringScope<'a>> {
    let bindings = list_items(bindings_value, "let bindings")?;
    let mut frame = BTreeMap::new();
    let mut child_scope = scope.clone();
    for binding in bindings {
        let pair = list_items(binding, "binding pair")?;
        if pair.len() != 2 {
            return Err(validation("Each binding must be `(name expr)`."));
        }
        let name = pair[0]
            .as_symbol()
            .ok_or_else(|| validation("Binding name must be a symbol."))?;
        let lowered = lower_scalar_binding(&pair[1], &child_scope)?;
        frame.insert(name.to_string(), lowered);
        child_scope = scope.with_frame(frame.clone());
    }
    Ok(child_scope)
}

fn lower_point_2d_expr(value: &Value, scope: &LoweringScope<'_>) -> AppResult<(String, String)> {
    let items = value
        .to_vec()
        .ok_or_else(|| validation("Expected a proper list for point."))?;
    if head_symbol(&items, "2D point expression").ok() == Some("list") && items.len() == 3 {
        return Ok((
            lower_num_expr(&items[1], scope)?,
            lower_num_expr(&items[2], scope)?,
        ));
    }
    if head_symbol(&items, "2D point expression").ok() == Some("let") && items.len() == 3 {
        let child_scope = lower_scalar_let_scope(&items[1], scope)?;
        return lower_point_2d_expr(&items[2], &child_scope);
    }
    if items.len() == 2 {
        return Ok((
            lower_num_expr(&items[0], scope)?,
            lower_num_expr(&items[1], scope)?,
        ));
    }
    Err(validation("Points must be (x y) pairs."))
}

fn lower_point_3d_expr(
    value: &Value,
    scope: &LoweringScope<'_>,
) -> AppResult<(String, String, String)> {
    let items = value
        .to_vec()
        .ok_or_else(|| validation("Expected a proper list for point."))?;
    if head_symbol(&items, "3D point expression").ok() == Some("list") && items.len() == 4 {
        return Ok((
            lower_num_expr(&items[1], scope)?,
            lower_num_expr(&items[2], scope)?,
            lower_num_expr(&items[3], scope)?,
        ));
    }
    if head_symbol(&items, "3D point expression").ok() == Some("let") && items.len() == 3 {
        let child_scope = lower_scalar_let_scope(&items[1], scope)?;
        return lower_point_3d_expr(&items[2], &child_scope);
    }
    if items.len() == 3 {
        return Ok((
            lower_num_expr(&items[0], scope)?,
            lower_num_expr(&items[1], scope)?,
            lower_num_expr(&items[2], scope)?,
        ));
    }
    Err(validation("3D points must be (x y z) triples."))
}

fn fmt_f64(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}.0", n as i64)
    } else {
        // Use Rust's default Display which gives enough precision
        format!("{}", n)
    }
}

fn lower_bool_expr(value: &Value, scope: &LoweringScope<'_>) -> AppResult<String> {
    if let Some(b) = value.as_bool() {
        return Ok(if b { "True".into() } else { "False".into() });
    }
    if let Some(sym) = value.as_symbol() {
        if let Some(binding) = scope.resolve_binding(sym) {
            return match binding {
                LoweredBinding::Boolean(expr) => Ok(expr.clone()),
                _ => Err(unsupported(format!(
                    "Symbol `{}` is not a boolean binding in this context.",
                    sym
                ))),
            };
        }
        return match scope.params.get(sym) {
            Some(ParamValue::Boolean(b)) => Ok(format!(
                "bool(params.get({:?}, {}))",
                sym,
                if *b { "True" } else { "False" }
            )),
            Some(_) => Err(unsupported(format!(
                "Symbol `{}` is not a boolean parameter.",
                sym
            ))),
            None => Err(validation(format!("Unknown symbol `{}`.", sym))),
        };
    }
    let items = list_items(value, "boolean expression")?;
    let op = head_symbol(&items, "boolean expression")?;
    let args = &items[1..];
    match op {
        "if" => {
            if args.len() != 3 {
                return Err(validation(
                    "Boolean `if` expects condition, then-expr, else-expr.",
                ));
            }
            let cond = lower_bool_expr(&args[0], scope)?;
            let then_expr = lower_bool_expr(&args[1], scope)?;
            let else_expr = lower_bool_expr(&args[2], scope)?;
            Ok(format!("({then_expr} if {cond} else {else_expr})"))
        }
        "not" => {
            if args.len() != 1 {
                return Err(validation("`not` expects one argument."));
            }
            Ok(format!("(not {})", lower_bool_expr(&args[0], scope)?))
        }
        "and" => {
            let parts = args
                .iter()
                .map(|a| lower_bool_expr(a, scope))
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!("({})", parts.join(" and ")))
        }
        "or" => {
            let parts = args
                .iter()
                .map(|a| lower_bool_expr(a, scope))
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!("({})", parts.join(" or ")))
        }
        "=" => {
            if args.len() != 2 {
                return Err(validation("`=` expects exactly two arguments."));
            }
            if let (Ok(a), Ok(b)) = (
                lower_num_expr(&args[0], scope),
                lower_num_expr(&args[1], scope),
            ) {
                return Ok(format!("({a} == {b})"));
            }
            let a = lower_stringish_expr(&args[0], scope)?;
            let b = lower_stringish_expr(&args[1], scope)?;
            Ok(format!("({a} == {b})"))
        }
        ">" | ">=" | "<" | "<=" => {
            if args.len() != 2 {
                return Err(validation(format!(
                    "`{}` expects exactly two arguments.",
                    op
                )));
            }
            let a = lower_num_expr(&args[0], scope)?;
            let b = lower_num_expr(&args[1], scope)?;
            Ok(format!("({a} {op} {b})"))
        }
        other => Err(unsupported(format!(
            "Boolean operator `{}` is not supported by the build123d lowerer.",
            other
        ))),
    }
}

fn lower_stringish_expr(value: &Value, scope: &LoweringScope<'_>) -> AppResult<String> {
    if let Some(text) = value.as_str() {
        return Ok(format!("{:?}", text));
    }
    if let Some(sym) = value.as_symbol() {
        if let Some(binding) = scope.resolve_binding(sym) {
            return match binding {
                LoweredBinding::Stringish(expr)
                | LoweredBinding::Number(expr)
                | LoweredBinding::Boolean(expr) => Ok(format!("str({expr})")),
                LoweredBinding::Geom(_) | LoweredBinding::Frame(_) | LoweredBinding::List(_) => {
                    Err(unsupported(format!(
                        "Symbol `{}` is not a string-like binding in this context.",
                        sym
                    )))
                }
            };
        }
        return match scope.params.get(sym) {
            Some(ParamValue::String(s)) => Ok(format!("str(params.get({:?}, {:?}))", sym, s)),
            Some(ParamValue::Number(n)) => {
                Ok(format!("str(params.get({:?}, {}))", sym, fmt_f64(*n)))
            }
            Some(ParamValue::Boolean(b)) => Ok(format!(
                "str(params.get({:?}, {}))",
                sym,
                if *b { "True" } else { "False" }
            )),
            _ => Ok(format!("{:?}", sym)),
        };
    }
    Err(validation("Expected a string-like value."))
}

// ===========================================================================
// Build123d expression-based lowering
// ===========================================================================

/// A lowered Python expression tree — not yet assigned to a variable.
#[derive(Clone, Debug)]
enum PyExpr {
    /// `func(args..., key=val...)`
    Call {
        func: String,
        args: Vec<PyExpr>,
        kwargs: Vec<(String, PyExpr)>,
    },
    /// `operand op operand op ...`
    BinOp {
        op: &'static str,
        operands: Vec<PyExpr>,
    },
    /// `prefix * inner`
    Transform { prefix: String, inner: Box<PyExpr> },
    /// Reference to an already-assigned variable.
    Var(String),
    /// Inline literal/expression (used directly, never assigned).
    Inline(String),
    /// Pre-emitted imperative block (for-loops, runtime guards, list comps).
    /// `lines` are emitted verbatim; `result_var` holds the final value.
    Imperative {
        lines: Vec<String>,
        result_var: String,
    },
}

#[derive(Clone, Debug)]
struct LoweredNode {
    expr: PyExpr,
    kind: B123dGeomKind,
}

enum ShellLoweringPlan {
    BooleanInner(Value),
    SolidOffsetPlanarFaces,
}

#[derive(Debug, PartialEq)]
enum PathFrameAnchor {
    Start,
    End,
    Position(Value),
}

#[derive(Debug, PartialEq)]
struct PathFrameCall {
    path: Value,
    at: PathFrameAnchor,
    up: Option<Value>,
}

#[derive(Debug, PartialEq)]
struct PlaceCall {
    frame: Value,
    geometry: Value,
    offset: Option<Value>,
    rotate: Option<Value>,
}

#[derive(Debug, PartialEq)]
struct ClipBoxCall {
    geometry: Value,
    x: Value,
    y: Value,
    z: Value,
}

#[derive(Debug, PartialEq)]
struct LinearArrayCall {
    count: Value,
    dx: Value,
    dy: Value,
    dz: Value,
    geometry: Value,
}

#[derive(Debug, Default, PartialEq)]
struct ParsedCallArgs {
    positional: Vec<Value>,
    keywords: BTreeMap<String, Value>,
}

impl ParsedCallArgs {
    fn parse(node: &str, args: &[Value], allowed_keywords: &[&str]) -> AppResult<Self> {
        let allowed = allowed_keywords.iter().copied().collect::<BTreeSet<_>>();
        let mut positional = Vec::new();
        let mut keywords = BTreeMap::new();
        let mut index = 0;

        while index < args.len() {
            if let Some(name) = keyword_name(&args[index]) {
                if !allowed.contains(name) {
                    return Err(validation(format!(
                        "`{}` does not recognize option `:{}`.",
                        node, name
                    )));
                }
                if index + 1 >= args.len() {
                    return Err(validation(format!("Keyword `:{}` needs a value.", name)));
                }
                if keywords
                    .insert(name.replace('-', "_"), args[index + 1].dup())
                    .is_some()
                {
                    return Err(validation(format!(
                        "`{}` received duplicate `:{}`.",
                        node, name
                    )));
                }
                index += 2;
                continue;
            }
            positional.push(args[index].dup());
            index += 1;
        }

        Ok(Self {
            positional,
            keywords,
        })
    }
}

fn parse_path_frame_call(args: &[Value]) -> AppResult<PathFrameCall> {
    let parsed = ParsedCallArgs::parse("path-frame", args, &["at", "up"])?;
    if parsed.positional.len() != 1 {
        return Err(validation("`path-frame` expects a path."));
    }
    let at = match parsed.keywords.get("at") {
        Some(value) => match value.as_symbol().or_else(|| value.as_str()) {
            Some("start") => PathFrameAnchor::Start,
            Some("end") => PathFrameAnchor::End,
            _ => PathFrameAnchor::Position(value.dup()),
        },
        None => PathFrameAnchor::End,
    };
    Ok(PathFrameCall {
        path: parsed.positional[0].dup(),
        at,
        up: parsed.keywords.get("up").map(Value::dup),
    })
}

fn parse_place_call(args: &[Value]) -> AppResult<PlaceCall> {
    let parsed = ParsedCallArgs::parse("place", args, &["offset", "rotate"])?;
    if parsed.positional.len() != 2 {
        return Err(validation(
            "`place` expects a frame, a geometry node, and optional `:offset` / `:rotate`.",
        ));
    }
    Ok(PlaceCall {
        frame: parsed.positional[0].dup(),
        geometry: parsed.positional[1].dup(),
        offset: parsed.keywords.get("offset").map(Value::dup),
        rotate: parsed.keywords.get("rotate").map(Value::dup),
    })
}

fn parse_clip_box_call(args: &[Value]) -> AppResult<ClipBoxCall> {
    let parsed = ParsedCallArgs::parse("clip-box", args, &["x", "y", "z"])?;
    if parsed.positional.len() != 1 {
        return Err(validation(
            "`clip-box` expects a solid and `:x` / `:y` / `:z` ranges.",
        ));
    }
    Ok(ClipBoxCall {
        geometry: parsed.positional[0].dup(),
        x: parsed
            .keywords
            .get("x")
            .map(Value::dup)
            .ok_or_else(|| validation("`clip-box` requires `:x`."))?,
        y: parsed
            .keywords
            .get("y")
            .map(Value::dup)
            .ok_or_else(|| validation("`clip-box` requires `:y`."))?,
        z: parsed
            .keywords
            .get("z")
            .map(Value::dup)
            .ok_or_else(|| validation("`clip-box` requires `:z`."))?,
    })
}

fn parse_linear_array_call(args: &[Value]) -> AppResult<LinearArrayCall> {
    let parsed = ParsedCallArgs::parse("linear-array", args, &[])?;
    if parsed.positional.len() != 5 {
        return Err(validation(
            "`linear-array` expects count, dx, dy, dz, and a mesh.",
        ));
    }
    Ok(LinearArrayCall {
        count: parsed.positional[0].dup(),
        dx: parsed.positional[1].dup(),
        dy: parsed.positional[2].dup(),
        dz: parsed.positional[3].dup(),
        geometry: parsed.positional[4].dup(),
    })
}

// ---- Linearizer: PyExpr tree → flat Python assignment statements -----------

struct Linearizer {
    counter: usize,
    lines: Vec<String>,
}

impl Linearizer {
    fn new() -> Self {
        Self {
            counter: 0,
            lines: Vec::new(),
        }
    }

    fn next_var(&mut self) -> String {
        let v = format!("_v{}", self.counter);
        self.counter += 1;
        v
    }

    fn emit(&mut self, line: String) {
        self.lines.push(line);
    }

    /// Linearize an expression, returning the variable name (or inline string)
    /// that holds the result.
    fn linearize(&mut self, expr: &PyExpr) -> String {
        match expr {
            PyExpr::Var(name) => name.clone(),
            PyExpr::Inline(s) => s.clone(),
            PyExpr::Call { func, args, kwargs } => {
                let lin_args: Vec<String> = args.iter().map(|a| self.linearize(a)).collect();
                let mut all_parts = lin_args;
                for (k, v) in kwargs {
                    let lin_v = self.linearize(v);
                    all_parts.push(format!("{k}={lin_v}"));
                }
                let var = self.next_var();
                self.emit(format!("{var} = {func}({})", all_parts.join(", ")));
                var
            }
            PyExpr::BinOp { op, operands } => {
                let vars: Vec<String> = operands.iter().map(|o| self.linearize(o)).collect();
                let var = self.next_var();
                self.emit(format!("{var} = {}", vars.join(&format!(" {op} "))));
                var
            }
            PyExpr::Transform { prefix, inner } => {
                let inner_var = self.linearize(inner);
                let var = self.next_var();
                self.emit(format!(
                    "{var} = _ecky_apply_transform({prefix}, {inner_var})"
                ));
                var
            }
            PyExpr::Imperative { lines, result_var } => {
                for line in lines {
                    self.emit(line.clone());
                }
                result_var.clone()
            }
        }
    }
}

// ---- Serializer: assemble final Python program ----------------------------

fn b123d_preamble() -> Vec<String> {
    vec![
        "from build123d import *".into(),
        "from build123d import exporters".into(),
        "import math".into(),
        String::new(),
        "def _ecky_intersect_x(shape, z):".into(),
        "    try:".into(),
        "        pts = shape.find_intersection_points(Axis(origin=(0,0,z), direction=(1,0,0)))".into(),
        "        if not pts: return 0.0".into(),
        "        pt = pts[-1][0] if isinstance(pts[-1], (list, tuple)) else pts[-1]".into(),
        "        return pt.X".into(),
        "    except:".into(),
        "        return 0.0".into(),
        String::new(),
        "def _ecky_face(shape):".into(),
        "    try:".into(),
        "        faces = shape.faces()".into(),
        "        if len(faces) == 1: return faces[0]".into(),
        "        if len(faces) > 1: return shape".into(),
        "    except Exception:".into(),
        "        pass".into(),
        "    try:".into(),
        "        return make_face(Wire(shape.edges()))".into(),
        "    except Exception:".into(),
        "        return shape".into(),
        String::new(),
        "def _ecky_wire_from_segments(*segments):".into(),
        "    edges = []".into(),
        "    for segment in segments:".into(),
        "        try:".into(),
        "            edges.extend(list(segment.edges()))".into(),
        "            continue".into(),
        "        except Exception:".into(),
        "            pass".into(),
        "        try:".into(),
        "            edges.append(segment)".into(),
        "        except Exception:".into(),
        "            pass".into(),
        "    return Wire(edges)".into(),
        String::new(),
        "def _ecky_face_from_wires(*wires):".into(),
        "    edges = []".into(),
        "    for wire in wires:".into(),
        "        try:".into(),
        "            edges.extend(list(wire.edges()))".into(),
        "        except Exception:".into(),
        "            pass".into(),
        "    if not edges:".into(),
        "        return Compound(children=[])".into(),
        "    try:".into(),
        "        return make_face(Wire(edges))".into(),
        "    except Exception:".into(),
        "        return _ecky_face(_ecky_compound(*wires))".into(),
        String::new(),
        "def _ecky_face_with_holes(outer_wire, *hole_wires):".into(),
        "    return Face(outer_wire, list(hole_wires))".into(),
        String::new(),
        "def _ecky_apply_transform(transform, shape):".into(),
        "    try:".into(),
        "        solids = _ecky_collect_solids(shape)".into(),
        "        if len(solids) == 1:".into(),
        "            return transform * solids[0]".into(),
        "        if len(solids) > 1:".into(),
        "            return Compound(children=[transform * solid for solid in solids])".into(),
        "    except Exception:".into(),
        "        pass".into(),
        "    try:".into(),
        "        return transform * shape".into(),
        "    except Exception:".into(),
        "        return Compound(children=[])".into(),
        String::new(),
        "def _ecky_solid(shape):".into(),
        "    try:".into(),
        "        solids = list(shape.solids())".into(),
        "        if len(solids) == 1: return Compound(children=[solids[0]])".into(),
        "        if len(solids) > 1: return Compound(children=solids)".into(),
        "    except Exception:".into(),
        "        pass".into(),
        "    return shape".into(),
        String::new(),
        "def _ecky_has_solids(shape):".into(),
        "    try: return len(list(shape.solids())) > 0".into(),
        "    except Exception: return False".into(),
        String::new(),
        "def _ecky_collect_solids(shape):".into(),
        "    try: return list(shape.solids())".into(),
        "    except Exception: return []".into(),
        String::new(),
        "def _ecky_compound(*shapes):".into(),
        "    solids = []".into(),
        "    for shape in shapes: solids.extend(_ecky_collect_solids(shape))".into(),
        "    return Compound(children=solids)".into(),
        String::new(),
        "def _ecky_difference_solid(base, *cuts):".into(),
        "    if not _ecky_has_solids(base): return Compound(children=[])".into(),
        "    out = _ecky_solid(base)".into(),
        "    for cut in cuts:".into(),
        "        if not _ecky_has_solids(cut): continue".into(),
        "        out = out - _ecky_solid(cut)".into(),
        "    return _ecky_solid(out)".into(),
        String::new(),
        "def _ecky_intersection_solid(*shapes):".into(),
        "    non_empty = [shape for shape in shapes if _ecky_has_solids(shape)]".into(),
        "    if not non_empty: return Compound(children=[])".into(),
        "    out = _ecky_solid(non_empty[0])".into(),
        "    for shape in non_empty[1:]:".into(),
        "        out = out & _ecky_solid(shape)".into(),
        "        if not _ecky_has_solids(out): return Compound(children=[])".into(),
        "    return _ecky_solid(out)".into(),
        String::new(),
        "def _ecky_fuse_many(*shapes):".into(),
        "    solids = []".into(),
        "    for shape in shapes: solids.extend(_ecky_collect_solids(shape))".into(),
        "    if not solids: return Compound(children=[])".into(),
        "    if len(solids) == 1: return Compound(children=[solids[0]])".into(),
        "    return _ecky_solid(solids[0].fuse(*solids[1:]))".into(),
        String::new(),
        "def _ecky_cut_many(base, *cuts):".into(),
        "    base_solids = _ecky_collect_solids(base)".into(),
        "    cut_solids = []".into(),
        "    for cut in cuts: cut_solids.extend(_ecky_collect_solids(cut))".into(),
        "    if not base_solids: return Compound(children=[])".into(),
        "    if not cut_solids: return base_solids[0] if len(base_solids) == 1 else Compound(children=base_solids)".into(),
        "    cutter = cut_solids[0] if len(cut_solids) == 1 else Compound(children=cut_solids)".into(),
        "    out = []".into(),
        "    for solid in base_solids:".into(),
        "        out.extend(_ecky_collect_solids(solid - cutter))".into(),
        "    if not out: return Compound(children=[])".into(),
        "    return out[0] if len(out) == 1 else Compound(children=out)".into(),
        String::new(),
        "def _ecky_common_many(*shapes):".into(),
        "    buckets = [_ecky_collect_solids(shape) for shape in shapes]".into(),
        "    if any(len(bucket) == 0 for bucket in buckets): return Compound(children=[])".into(),
        "    current = buckets[0]".into(),
        "    for bucket in buckets[1:]:".into(),
        "        out = []".into(),
        "        for left in current:".into(),
        "            hit = left.intersect(*bucket)".into(),
        "            out.extend(_ecky_collect_solids(hit))".into(),
        "        current = out".into(),
        "        if not current: return Compound(children=[])".into(),
        "    return current[0] if len(current) == 1 else Compound(children=current)".into(),
        String::new(),
        "def _ecky_path_frame(path, at='end', up=None):".into(),
        "    if at == 'start': position = 0.0".into(),
        "    elif at == 'end': position = 1.0".into(),
        "    else: position = float(at)".into(),
        "    kwargs = {'position_mode': PositionMode.PARAMETER, 'frame_method': FrameMethod.FRENET}".into(),
        "    if up is not None: kwargs['x_dir'] = Vector(*up)".into(),
        "    return path.location_at(position, **kwargs)".into(),
        String::new(),
        "def _ecky_place(frame, shape, offset=(0,0,0), rotate=(0,0,0)):".into(),
        "    ox, oy, oz = offset".into(),
        "    rx, ry, rz = rotate".into(),
        "    return frame * Pos(ox, oy, oz) * Rot(rx, ry, rz) * shape".into(),
        String::new(),
        "def _ecky_clip_box(shape, xmin, xmax, ymin, ymax, zmin, zmax):".into(),
        "    solids = _ecky_collect_solids(shape)".into(),
        "    if not solids: return Compound(children=[])".into(),
        "    xmin, xmax = min(xmin, xmax), max(xmin, xmax)".into(),
        "    ymin, ymax = min(ymin, ymax), max(ymin, ymax)".into(),
        "    zmin, zmax = min(zmin, zmax), max(zmin, zmax)".into(),
        "    clip = Pos((xmin + xmax) / 2.0, (ymin + ymax) / 2.0, (zmin + zmax) / 2.0) * Box(xmax - xmin, ymax - ymin, zmax - zmin, align=(Align.CENTER, Align.CENTER, Align.CENTER))".into(),
        "    return _ecky_common_many(shape, clip)".into(),
        String::new(),
    ]
}

fn serialize_b123d_program(linearized_lines: Vec<String>, part_entries: Vec<String>) -> String {
    let mut out = b123d_preamble();
    out.extend(linearized_lines);
    out.push(String::new());
    out.push(format!("_ecky_parts = [{}]", part_entries.join(", ")));
    out.join("\n")
}

// ---- Expression lowerer ---------------------------------------------------

struct ExprLowerer<'a> {
    model: &'a IrModel,
    lin: Linearizer,
    imp_counter: usize,
}

impl<'a> ExprLowerer<'a> {
    fn new(model: &'a IrModel) -> Self {
        Self {
            model,
            lin: Linearizer::new(),
            imp_counter: 0,
        }
    }

    fn next_imp_var(&mut self) -> String {
        let v = format!("_b{}", self.imp_counter);
        self.imp_counter += 1;
        v
    }

    fn stabilize_solid_result(
        &mut self,
        expr: PyExpr,
        kind: B123dGeomKind,
    ) -> (PyExpr, B123dGeomKind) {
        if kind != B123dGeomKind::Solid3d {
            return (expr, kind);
        }
        let raw = self.lin.linearize(&expr);
        let result = self.next_imp_var();
        self.lin.emit(format!("{result} = _ecky_solid({raw})"));
        (PyExpr::Var(result), kind)
    }

    fn lower_geom_expr_locally(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<(Vec<String>, String, B123dGeomKind)> {
        let mut nested = ExprLowerer {
            model: self.model,
            lin: Linearizer::new(),
            imp_counter: self.imp_counter,
        };
        let node = nested.lower_geom_expr(value, scope)?;
        let result_var = nested.lin.linearize(&node.expr);
        self.imp_counter = nested.imp_counter;
        Ok((nested.lin.lines, result_var, node.kind))
    }

    fn param_defaults(&self) -> BTreeMap<String, ParamValue> {
        self.model
            .params
            .iter()
            .map(|p| (p.field.key().to_string(), p.default_value.clone()))
            .collect()
    }

    fn lower_model(&mut self) -> AppResult<String> {
        let defaults = self.param_defaults();
        let scope = LoweringScope::new(&defaults);
        let parts: Vec<(String, Value)> = self
            .model
            .parts
            .iter()
            .map(|p| (p.part_id.clone(), p.expr.dup()))
            .collect();

        let mut part_entries: Vec<String> = Vec::new();
        for (part_id, expr) in &parts {
            let node = self.lower_geom_expr(expr, &scope)?;
            let var = self.lin.linearize(&node.expr);
            part_entries.push(format!("({:?}, {})", part_id, var));
        }

        Ok(serialize_b123d_program(
            std::mem::take(&mut self.lin.lines),
            part_entries,
        ))
    }

    fn parse_properties(&self, args: &[Value]) -> AppResult<(Vec<Value>, BTreeMap<String, Value>)> {
        let mut positional = Vec::new();
        let mut properties = BTreeMap::new();
        let mut i = 0;
        while i < args.len() {
            if let Some(name) = keyword_name(&args[i]) {
                if i + 1 >= args.len() {
                    return Err(validation(format!("Keyword `:{}` needs a value.", name)));
                }
                let key = name.replace('-', "_");
                properties.insert(key, args[i + 1].dup());
                i += 2;
                continue;
            }
            positional.push(args[i].dup());
            i += 1;
        }
        Ok((positional, properties))
    }

    fn try_materialize_list_binding(
        &self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<Option<LoweredList>> {
        if let Some(sym) = value.as_symbol() {
            return Ok(match scope.resolve_binding(sym) {
                Some(LoweredBinding::List(list)) => Some(list.clone()),
                _ => None,
            });
        }

        let Some(items) = value.to_vec() else {
            return Ok(None);
        };

        if head_symbol(&items, "list expression").ok() == Some("let") {
            if items.len() != 3 {
                return Err(validation("List `let` expects bindings and a body."));
            }
            let child_scope = lower_scalar_let_scope(&items[1], scope)?;
            return self.try_materialize_list_binding(&items[2], &child_scope);
        }

        if head_symbol(&items, "list expression").ok() == Some("list") {
            let materialized = items[1..].iter().map(Value::dup).collect::<Vec<_>>();
            return Ok(Some(LoweredList::new(
                materialized.clone(),
                infer_list_kind(&materialized, scope),
                Some("list".to_string()),
            )));
        }

        if items.first().and_then(Value::as_symbol).is_some() {
            return Ok(None);
        }

        let materialized = items.iter().map(Value::dup).collect::<Vec<_>>();
        Ok(Some(LoweredList::new(
            materialized.clone(),
            infer_list_kind(&materialized, scope),
            None,
        )))
    }

    fn require_list_kind(
        &self,
        list: LoweredList,
        cad_op: &str,
        expected_kind: LoweredListKind,
        subject: Option<String>,
    ) -> AppResult<LoweredList> {
        if list.kind == expected_kind {
            return Ok(list);
        }

        let suffix = list.source_suffix();
        let message = if let Some(subject) = subject {
            format!(
                "CAD op `{}` expected {} but {} resolved to {}{}.",
                cad_op,
                expected_kind.noun(),
                subject,
                list.kind.noun(),
                suffix
            )
        } else {
            format!(
                "CAD op `{}` expected {} but received {}{}.",
                cad_op,
                expected_kind.noun(),
                list.kind.noun(),
                suffix
            )
        };
        Err(unsupported(message))
    }

    fn materialize_list_for_cad_op(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
        cad_op: &str,
        expected_kind: LoweredListKind,
    ) -> AppResult<LoweredList> {
        if let Some(sym) = value.as_symbol() {
            return match scope.resolve_binding(sym) {
                Some(LoweredBinding::List(list)) => self.require_list_kind(
                    list.clone(),
                    cad_op,
                    expected_kind,
                    Some(format!("symbol `{}`", sym)),
                ),
                Some(binding) => Err(unsupported(format!(
                    "CAD op `{}` expected {} but symbol `{}` resolved to {}.",
                    cad_op,
                    expected_kind.noun(),
                    sym,
                    binding_kind_noun(binding)
                ))),
                None => Err(validation(format!("Unknown symbol `{}`.", sym))),
            };
        }

        if let Some(list) = self.try_materialize_list_binding(value, scope)? {
            return self.require_list_kind(list, cad_op, expected_kind, None);
        }

        if let Some(items) = value.to_vec() {
            if let Some(actual_op) = items.first().and_then(Value::as_symbol) {
                if let Ok((_, _, actual_kind)) = self.lower_geom_expr_locally(value, scope) {
                    return Err(unsupported(format!(
                        "CAD op `{}` expected {} but CAD op `{}` produced {}.",
                        cad_op,
                        expected_kind.noun(),
                        actual_op,
                        actual_kind.noun()
                    )));
                }
                return Err(unsupported(format!(
                    "CAD op `{}` expected {} but list op `{}` did not materialize to that kind.",
                    cad_op,
                    expected_kind.noun(),
                    actual_op
                )));
            }
        }

        Err(validation(format!(
            "CAD op `{}` expected {}.",
            cad_op,
            expected_kind.noun()
        )))
    }

    fn materialize_list_len(
        &self,
        value: &Value,
        scope: &LoweringScope<'_>,
        context: &str,
    ) -> AppResult<usize> {
        if let Some(list) = self.try_materialize_list_binding(value, scope)? {
            return Ok(list.items.len());
        }
        Ok(list_items(value, context)?.len())
    }

    fn lower_points_2d_args(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
        cad_op: &str,
    ) -> AppResult<String> {
        let points =
            self.materialize_list_for_cad_op(value, scope, cad_op, LoweredListKind::Point2d)?;
        let mut entries = Vec::new();
        for point in &points.items {
            let (x, y) = lower_point_2d_expr(point, scope)?;
            entries.push(format!("({x}, {y})"));
        }
        Ok(entries.join(", "))
    }

    fn lower_points_3d_args(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
        cad_op: &str,
    ) -> AppResult<String> {
        let points =
            self.materialize_list_for_cad_op(value, scope, cad_op, LoweredListKind::Point3d)?;
        let mut entries = Vec::new();
        for point in &points.items {
            let (x, y, z) = lower_point_3d_expr(point, scope)?;
            entries.push(format!("({x}, {y}, {z})"));
        }
        Ok(entries.join(", "))
    }

    fn lower_count(&self, value: &Value, scope: &LoweringScope<'_>) -> AppResult<String> {
        if let Some(n) = value.as_f64() {
            return Ok(format!("{}", n.round().max(1.0) as usize));
        }
        let expr = lower_num_expr(value, scope)?;
        Ok(format!("int({})", expr))
    }

    fn lower_tangent_scalars(
        &self,
        value: &Value,
        scope: &LoweringScope<'_>,
        point_count: usize,
    ) -> AppResult<String> {
        let scalar_items = list_items(value, "tangent scalars")?;
        if scalar_items.is_empty() {
            return Err(validation("`tangent-scalars` cannot be empty."));
        }

        if scalar_items
            .iter()
            .all(|item| item.as_f64().is_some() || item.as_symbol().is_some())
        {
            let scalars = scalar_items
                .iter()
                .map(|item| lower_num_expr(item, scope))
                .collect::<AppResult<Vec<_>>>()?;
            if scalars.len() != 2 && scalars.len() != point_count {
                return Err(validation(format!(
                    "`tangent-scalars` expects 2 entries or one per point ({}).",
                    point_count
                )));
            }
            return Ok(format!("[{}]", scalars.join(", ")));
        }

        let mut scalar_entries = Vec::new();
        let scalar_pair_count = scalar_items.len();
        for item in scalar_items {
            let pair = list_items(item, "scalar pair")?;
            if pair.len() != 2 {
                return Err(validation("Each tangent scalar entry must be a pair."));
            }
            let s1 = lower_num_expr(&pair[0], scope)?;
            let s2 = lower_num_expr(&pair[1], scope)?;
            if s1 == s2 {
                scalar_entries.push(s1);
                continue;
            }
            if scalar_pair_count == 1 {
                scalar_entries.push(s1);
                scalar_entries.push(s2);
                continue;
            }
            return Err(validation(
                "`tangent-scalars` pairs are ambiguous here. Use a flat scalar list like `(1.75 1)` or duplicated pairs like `((1 1) (2 2))`.",
            ));
        }
        if scalar_entries.len() != 2 && scalar_entries.len() != point_count {
            return Err(validation(format!(
                "`tangent-scalars` expects 2 entries or one per point ({}).",
                point_count
            )));
        }
        Ok(format!("[{}]", scalar_entries.join(", ")))
    }

    fn shell_negate_value(&self, value: &Value) -> Value {
        Value::list(vec![Value::symbol("-"), value.dup()])
    }

    fn shell_subtract_wall(&self, value: &Value, wall: &Value) -> Value {
        Value::list(vec![Value::symbol("-"), value.dup(), wall.dup()])
    }

    fn shell_offset_sketch(&self, sketch: &Value, wall: &Value) -> Value {
        Value::list(vec![
            Value::symbol("offset"),
            self.shell_negate_value(wall),
            sketch.dup(),
        ])
    }

    fn shell_target_value(&self, node: &str, args: Vec<Value>) -> Value {
        let mut items = Vec::with_capacity(args.len() + 1);
        items.push(Value::symbol(node));
        items.extend(args);
        Value::list(items)
    }

    fn plan_shell_target(&self, target: &Value, wall: &Value) -> AppResult<ShellLoweringPlan> {
        let target_items = list_items(target, "shell target")?;
        let target_node = head_symbol(&target_items, "shell target")?;
        let target_args = &target_items[1..];

        let plan = match target_node {
            "cylinder" => {
                if target_args.len() < 2 || target_args.len() > 3 {
                    return Err(validation(
                        "`shell` cylinder expects radius, height, and optional segments.",
                    ));
                }
                let mut inner_args = vec![
                    self.shell_subtract_wall(&target_args[0], wall),
                    target_args[1].dup(),
                ];
                if let Some(segments) = target_args.get(2) {
                    inner_args.push(segments.dup());
                }
                ShellLoweringPlan::BooleanInner(self.shell_target_value("cylinder", inner_args))
            }
            "cone" => {
                if target_args.len() < 3 || target_args.len() > 4 {
                    return Err(validation(
                        "`shell` cone expects bottom radius, top radius, height.",
                    ));
                }
                let mut inner_args = vec![
                    self.shell_subtract_wall(&target_args[0], wall),
                    self.shell_subtract_wall(&target_args[1], wall),
                    target_args[2].dup(),
                ];
                if let Some(segments) = target_args.get(3) {
                    inner_args.push(segments.dup());
                }
                ShellLoweringPlan::BooleanInner(self.shell_target_value("cone", inner_args))
            }
            "sphere" => {
                if target_args.is_empty() || target_args.len() > 3 {
                    return Err(validation("`shell` sphere expects radius."));
                }
                let mut inner_args = vec![self.shell_subtract_wall(&target_args[0], wall)];
                inner_args.extend(target_args[1..].iter().map(Value::dup));
                ShellLoweringPlan::BooleanInner(self.shell_target_value("sphere", inner_args))
            }
            "extrude" => {
                if target_args.len() != 2 {
                    return Err(validation("`shell` extrude expects a sketch and height."));
                }
                ShellLoweringPlan::BooleanInner(self.shell_target_value(
                    "extrude",
                    vec![
                        self.shell_offset_sketch(&target_args[0], wall),
                        target_args[1].dup(),
                    ],
                ))
            }
            "revolve" => {
                if target_args.len() < 2 || target_args.len() > 3 {
                    return Err(validation(
                        "`shell` revolve expects a sketch, angle, and optional segments.",
                    ));
                }
                ShellLoweringPlan::SolidOffsetPlanarFaces
            }
            "sweep" => {
                if target_args.len() != 2 {
                    return Err(validation("`shell` sweep expects a sketch and a path."));
                }
                ShellLoweringPlan::BooleanInner(self.shell_target_value(
                    "sweep",
                    vec![
                        self.shell_offset_sketch(&target_args[0], wall),
                        target_args[1].dup(),
                    ],
                ))
            }
            "loft" => {
                if target_args.len() < 3 {
                    return Err(validation(
                        "`shell` loft expects height, and at least two sketches.",
                    ));
                }
                let mut inner_args = vec![target_args[0].dup()];
                inner_args.extend(
                    target_args[1..]
                        .iter()
                        .map(|sketch| self.shell_offset_sketch(sketch, wall)),
                );
                ShellLoweringPlan::BooleanInner(self.shell_target_value("loft", inner_args))
            }
            "twist" => {
                if !(target_args.len() == 3 || target_args.len() == 4) {
                    return Err(validation(
                        "`shell` twist expects height, angle, sketch or height, angle, segments, sketch.",
                    ));
                }
                let sketch_index = if target_args.len() == 3 {
                    2usize
                } else {
                    3usize
                };
                let mut inner_args = target_args.iter().map(Value::dup).collect::<Vec<_>>();
                inner_args[sketch_index] =
                    self.shell_offset_sketch(&target_args[sketch_index], wall);
                ShellLoweringPlan::BooleanInner(self.shell_target_value("twist", inner_args))
            }
            other => {
                return Err(unsupported(format!(
                    "Node `shell` with target `{}` is not yet supported by the build123d lowerer. \
                     Use the EckyRust backend for this model.",
                    other
                )));
            }
        };

        Ok(plan)
    }

    fn lower_sketch_operand(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredNode> {
        if let Ok(geom) = self.lower_sketch_expr(value, scope) {
            return Ok(LoweredNode {
                expr: PyExpr::Call {
                    func: "_ecky_face".into(),
                    args: vec![geom.expr],
                    kwargs: vec![],
                },
                kind: B123dGeomKind::Sketch2d,
            });
        }
        let points = self.lower_points_2d_args(value, scope, "wire")?;
        Ok(LoweredNode {
            expr: PyExpr::Call {
                func: "_ecky_face".into(),
                args: vec![PyExpr::Call {
                    func: "Polygon".into(),
                    args: vec![PyExpr::Inline(format!("[{points}]"))],
                    kwargs: vec![],
                }],
                kwargs: vec![],
            },
            kind: B123dGeomKind::Sketch2d,
        })
    }

    fn lower_wire_operand(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<PyExpr> {
        let mut wire_operands = Vec::new();
        if Self::collect_make_face_wires(value, &mut wire_operands) && wire_operands.len() > 1 {
            let lowered: AppResult<Vec<PyExpr>> = wire_operands
                .into_iter()
                .map(|item| self.lower_wire_operand(&item, scope))
                .collect();
            return Ok(PyExpr::Call {
                func: "_ecky_wire_from_segments".into(),
                args: lowered?,
                kwargs: vec![],
            });
        }
        if let Ok(sketch) = self.lower_sketch_expr(value, scope) {
            return Ok(PyExpr::Call {
                func: "_ecky_wire_from_segments".into(),
                args: vec![sketch.expr],
                kwargs: vec![],
            });
        }
        let points = self.lower_points_2d_args(value, scope, "wire")?;
        Ok(PyExpr::Call {
            func: "_ecky_wire_from_segments".into(),
            args: vec![PyExpr::Call {
                func: "Polygon".into(),
                args: vec![PyExpr::Inline(format!("[{points}]"))],
                kwargs: vec![],
            }],
            kwargs: vec![],
        })
    }

    fn lower_wire_collection(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<Vec<PyExpr>> {
        let items = list_items(value, "wire collection")?;
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let is_node = items
            .first()
            .and_then(|v| v.as_symbol())
            .map(|s| !s.starts_with(':'))
            .unwrap_or(false);
        if is_node {
            return Ok(vec![self.lower_wire_operand(value, scope)?]);
        }
        let is_single_loop = items
            .first()
            .and_then(|v| v.to_vec())
            .map(|pair| pair.len() == 2)
            .unwrap_or(false);
        if is_single_loop {
            return Ok(vec![self.lower_wire_operand(value, scope)?]);
        }
        items
            .iter()
            .map(|item| self.lower_wire_operand(item, scope))
            .collect()
    }

    fn lower_openings(&mut self, value: &Value, scope: &LoweringScope<'_>) -> AppResult<PyExpr> {
        let items = list_items(value, "offset openings")?;
        if items.is_empty() {
            return Ok(PyExpr::Inline("[]".into()));
        }
        let is_node = items
            .first()
            .and_then(Value::as_symbol)
            .map(|s| !s.starts_with(':'))
            .unwrap_or(false);
        if is_node {
            return Ok(self.lower_sketch_operand(value, scope)?.expr);
        }
        let is_single_loop = items
            .first()
            .and_then(|v| v.to_vec())
            .map(|pair| {
                pair.len() == 2
                    && pair
                        .iter()
                        .all(|v| v.as_f64().is_some() || v.as_symbol().is_some())
            })
            .unwrap_or(false);
        if is_single_loop {
            return Ok(self.lower_sketch_operand(value, scope)?.expr);
        }
        let mut openings: Vec<PyExpr> = Vec::new();
        for item in items {
            openings.push(self.lower_sketch_operand(item, scope)?.expr);
        }
        if openings.len() == 1 {
            return Ok(openings.remove(0));
        }
        // Linearize each opening and build a list expression
        let vars: Vec<String> = openings.iter().map(|e| self.lin.linearize(e)).collect();
        Ok(PyExpr::Inline(format!("[{}]", vars.join(", "))))
    }

    fn lower_vec3(
        &self,
        value: &Value,
        scope: &LoweringScope<'_>,
        context: &str,
    ) -> AppResult<(String, String, String)> {
        let triple = list_items(value, context)?;
        if triple.len() != 3 {
            return Err(validation(format!("{} must be `(x y z)`.", context)));
        }
        Ok((
            lower_num_expr(&triple[0], scope)?,
            lower_num_expr(&triple[1], scope)?,
            lower_num_expr(&triple[2], scope)?,
        ))
    }

    fn lower_range_pair(
        &self,
        value: &Value,
        scope: &LoweringScope<'_>,
        context: &str,
    ) -> AppResult<(String, String)> {
        let pair = list_items(value, context)?;
        if pair.len() != 2 {
            return Err(validation(format!("{} must be `(min max)`.", context)));
        }
        Ok((
            lower_num_expr(&pair[0], scope)?,
            lower_num_expr(&pair[1], scope)?,
        ))
    }

    fn collect_make_face_wires(value: &Value, out: &mut Vec<Value>) -> bool {
        let Some(items) = value.to_vec() else {
            out.push(value.dup());
            return true;
        };
        let Some(head) = items.first().and_then(Value::as_symbol) else {
            out.push(value.dup());
            return true;
        };
        if !matches!(head, "union" | "fuse") {
            out.push(value.dup());
            return true;
        }
        if items.len() < 2 {
            return false;
        }
        for item in &items[1..] {
            if !Self::collect_make_face_wires(item, out) {
                return false;
            }
        }
        true
    }

    fn lower_frame_expr(&mut self, value: &Value, scope: &LoweringScope<'_>) -> AppResult<String> {
        if let Some(sym) = value.as_symbol() {
            return match scope.resolve_binding(sym) {
                Some(LoweredBinding::Frame(frame)) => Ok(frame.clone()),
                Some(_) => Err(unsupported(format!(
                    "Symbol `{}` is not a frame binding in this context.",
                    sym
                ))),
                None => Err(validation(format!("Unknown symbol `{}`.", sym))),
            };
        }

        let items = list_items(value, "frame expression")?;
        let node = head_symbol(&items, "frame expression")?;
        let args = &items[1..];
        match node {
            "path-frame" => {
                let call = parse_path_frame_call(args)?;
                let path = self.lower_geom_expr(&call.path, scope)?;
                match path.kind {
                    B123dGeomKind::Path3d | B123dGeomKind::Sketch2d => {}
                    _ => {
                        return Err(unsupported(format!(
                            "Node `path-frame` expected a path-like shape but received {}.",
                            path.kind.noun()
                        )))
                    }
                }
                let path_var = self.lin.linearize(&path.expr);
                let at = match call.at {
                    PathFrameAnchor::Start => "\"start\"".to_string(),
                    PathFrameAnchor::End => "\"end\"".to_string(),
                    PathFrameAnchor::Position(value) => lower_num_expr(&value, scope)?,
                };
                let up = if let Some(value) = call.up {
                    let (x, y, z) = self.lower_vec3(&value, scope, "`path-frame :up`")?;
                    format!("({x}, {y}, {z})")
                } else {
                    "None".to_string()
                };
                let result = self.next_imp_var();
                self.lin.emit(format!(
                    "{result} = _ecky_path_frame({path_var}, {at}, {up})"
                ));
                Ok(result)
            }
            _ => Err(unsupported(format!(
                "Node `{}` is not supported in frame context by the build123d lowerer.",
                node
            ))),
        }
    }

    fn lower_binding_value(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredBinding> {
        if let Ok(frame) = self.lower_frame_expr(value, scope) {
            return Ok(LoweredBinding::Frame(frame));
        }
        if let Ok(geom) = self.lower_geom_expr(value, scope) {
            let var = self.lin.linearize(&geom.expr);
            return Ok(LoweredBinding::Geom(LoweredGeom {
                var,
                kind: geom.kind,
            }));
        }
        if let Some(list) = self.try_materialize_list_binding(value, scope)? {
            return Ok(LoweredBinding::List(list));
        }
        let number_err = match lower_num_expr(value, scope) {
            Ok(number) => return Ok(LoweredBinding::Number(number)),
            Err(err) => err,
        };
        let bool_err = match lower_bool_expr(value, scope) {
            Ok(boolean) => return Ok(LoweredBinding::Boolean(boolean)),
            Err(err) => err,
        };
        if let Ok(stringish) = lower_stringish_expr(value, scope) {
            return Ok(LoweredBinding::Stringish(stringish));
        }
        if number_err.code == crate::models::AppErrorCode::Validation {
            return Err(number_err);
        }
        if bool_err.code == crate::models::AppErrorCode::Validation {
            return Err(bool_err);
        }
        Err(number_err)
    }

    fn lower_sketch_expr(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredNode> {
        let geom = self.lower_geom_expr(value, scope)?;
        if geom.kind != B123dGeomKind::Sketch2d {
            return Err(unsupported(format!(
                "Node expected a 2D sketch but received {}.",
                geom.kind.noun()
            )));
        }
        Ok(geom)
    }

    fn lower_solid_expr(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredNode> {
        let geom = self.lower_geom_expr(value, scope)?;
        if geom.kind != B123dGeomKind::Solid3d {
            return Err(unsupported(format!(
                "Node expected a 3D solid but received {}.",
                geom.kind.noun()
            )));
        }
        Ok(geom)
    }

    fn lower_path_expr(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredNode> {
        let geom = self.lower_geom_expr(value, scope)?;
        if geom.kind != B123dGeomKind::Path3d {
            return Err(unsupported(format!(
                "Node expected a 3D path but received {}.",
                geom.kind.noun()
            )));
        }
        Ok(geom)
    }

    fn same_kind(&self, node: &str, nodes: &[LoweredNode]) -> AppResult<B123dGeomKind> {
        let first = nodes
            .first()
            .ok_or_else(|| validation(format!("Node `{}` expects at least one operand.", node)))?;
        for n in &nodes[1..] {
            if n.kind != first.kind {
                return Err(unsupported(format!(
                    "Node `{}` requires matching geometry kinds, got {} and {}.",
                    node,
                    first.kind.noun(),
                    n.kind.noun()
                )));
            }
        }
        Ok(first.kind.clone())
    }

    fn lower_geom_list(
        &mut self,
        args: &[Value],
        scope: &LoweringScope<'_>,
    ) -> AppResult<Vec<LoweredNode>> {
        args.iter()
            .map(|arg| self.lower_geom_expr(arg, scope))
            .collect()
    }

    fn lower_geom_expr(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredNode> {
        if let Some(sym) = value.as_symbol() {
            return match scope.resolve_binding(sym) {
                Some(LoweredBinding::Geom(geom)) => Ok(LoweredNode {
                    expr: PyExpr::Var(geom.var.clone()),
                    kind: geom.kind.clone(),
                }),
                Some(LoweredBinding::List(list)) => Err(unsupported(format!(
                    "Symbol `{}` is a {} and cannot be used as geometry here.",
                    sym,
                    list.kind.noun()
                ))),
                Some(_) => Err(unsupported(format!(
                    "Symbol `{}` is not a geometry binding in this context.",
                    sym
                ))),
                None => Err(validation(format!("Unknown symbol `{}`.", sym))),
            };
        }
        let items = value
            .to_vec()
            .ok_or_else(|| validation("Expected a proper list for geometry node."))?;
        let node = head_symbol(&items, "geometry node")?;
        let args = &items[1..];

        let (expr, kind) = match node {
            "def" => {
                return Err(unsupported(
                    "`def` is not supported by Ecky IR v0. Use lexical `let` for immutable local bindings.",
                ));
            }
            "build" => {
                let build = parse_build_expr(value)?;
                let mut child_scope = scope.clone();
                for binding in &build.bindings {
                    let lowered = self.lower_binding_value(&binding.expr, &child_scope)?;
                    let local_name = format!("_{}", binding.name.replace('-', "_"));
                    match &lowered {
                        LoweredBinding::Geom(geom) => {
                            self.lin.emit(format!("{local_name} = {}", geom.var))
                        }
                        LoweredBinding::List(_) => {}
                        LoweredBinding::Frame(expr)
                        | LoweredBinding::Number(expr)
                        | LoweredBinding::Boolean(expr)
                        | LoweredBinding::Stringish(expr) => {
                            self.lin.emit(format!("{local_name} = {expr}"));
                        }
                    }
                    let stored = match lowered {
                        LoweredBinding::Geom(mut geom) => {
                            geom.var = local_name.clone();
                            LoweredBinding::Geom(geom)
                        }
                        LoweredBinding::List(list) => LoweredBinding::List(list),
                        LoweredBinding::Frame(_) => LoweredBinding::Frame(local_name.clone()),
                        LoweredBinding::Number(_) => LoweredBinding::Number(local_name.clone()),
                        LoweredBinding::Boolean(_) => LoweredBinding::Boolean(local_name.clone()),
                        LoweredBinding::Stringish(_) => {
                            LoweredBinding::Stringish(local_name.clone())
                        }
                    };
                    let mut frame = BTreeMap::new();
                    frame.insert(binding.name.clone(), stored);
                    child_scope = child_scope.with_frame(frame);
                }
                return self.lower_geom_expr(&build.result, &child_scope);
            }
            "let" => {
                if args.len() < 2 {
                    return Err(validation("`let` expects bindings and a body."));
                }
                let bindings = list_items(&args[0], "let bindings")?;
                let mut frame = BTreeMap::new();

                for b in bindings {
                    let pair = list_items(b, "binding pair")?;
                    if pair.len() != 2 {
                        return Err(validation("Each binding must be `(name expr)`."));
                    }
                    let name = pair[0]
                        .as_symbol()
                        .ok_or_else(|| validation("Binding name must be a symbol."))?;
                    let binding = self.lower_binding_value(&pair[1], scope)?;
                    let local_name = format!("_{}", name.replace('-', "_"));
                    match &binding {
                        LoweredBinding::Geom(geom) => {
                            self.lin.emit(format!("{local_name} = {}", geom.var));
                        }
                        LoweredBinding::List(_) => {}
                        LoweredBinding::Frame(expr) => {
                            self.lin.emit(format!("{local_name} = {expr}"));
                        }
                        LoweredBinding::Number(expr)
                        | LoweredBinding::Boolean(expr)
                        | LoweredBinding::Stringish(expr) => {
                            self.lin.emit(format!("{local_name} = {expr}"));
                        }
                    }
                    let local_binding = match binding {
                        LoweredBinding::Geom(mut geom) => {
                            geom.var = local_name.clone();
                            LoweredBinding::Geom(geom)
                        }
                        LoweredBinding::List(list) => LoweredBinding::List(list),
                        LoweredBinding::Frame(_) => LoweredBinding::Frame(local_name.clone()),
                        LoweredBinding::Number(_) => LoweredBinding::Number(local_name.clone()),
                        LoweredBinding::Boolean(_) => LoweredBinding::Boolean(local_name.clone()),
                        LoweredBinding::Stringish(_) => {
                            LoweredBinding::Stringish(local_name.clone())
                        }
                    };
                    frame.insert(name.to_string(), local_binding);
                }
                let child_scope = scope.with_frame(frame);
                return self.lower_geom_expr(&args[1], &child_scope);
            }
            // -- Primitives (Step 1) --
            "box" => {
                if args.len() != 3 {
                    return Err(validation("`box` expects width, depth, and height."));
                }
                let w = lower_num_expr(&args[0], scope)?;
                let d = lower_num_expr(&args[1], scope)?;
                let h = lower_num_expr(&args[2], scope)?;
                (
                    PyExpr::Call {
                        func: "Box".into(),
                        args: vec![PyExpr::Inline(w), PyExpr::Inline(d), PyExpr::Inline(h)],
                        kwargs: vec![(
                            "align".into(),
                            PyExpr::Inline("(Align.CENTER, Align.CENTER, Align.MIN)".into()),
                        )],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "cylinder" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(validation(
                        "`cylinder` expects radius, height, and optional segments.",
                    ));
                }
                let r = lower_num_expr(&args[0], scope)?;
                let h = lower_num_expr(&args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "Cylinder".into(),
                        args: vec![PyExpr::Inline(r), PyExpr::Inline(h)],
                        kwargs: vec![(
                            "align".into(),
                            PyExpr::Inline("(Align.CENTER, Align.CENTER, Align.MIN)".into()),
                        )],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "sphere" => {
                if args.is_empty() || args.len() > 3 {
                    return Err(validation("`sphere` expects radius and optional segments."));
                }
                let r = lower_num_expr(&args[0], scope)?;
                (
                    PyExpr::Call {
                        func: "Sphere".into(),
                        args: vec![PyExpr::Inline(r)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "cone" => {
                if args.len() < 3 || args.len() > 4 {
                    return Err(validation(
                        "`cone` expects bottom radius, top radius, height, and optional segments.",
                    ));
                }
                let br = lower_num_expr(&args[0], scope)?;
                let tr = lower_num_expr(&args[1], scope)?;
                let h = lower_num_expr(&args[2], scope)?;
                (
                    PyExpr::Call {
                        func: "Cone".into(),
                        args: vec![PyExpr::Inline(br), PyExpr::Inline(tr), PyExpr::Inline(h)],
                        kwargs: vec![(
                            "align".into(),
                            PyExpr::Inline("(Align.CENTER, Align.CENTER, Align.MIN)".into()),
                        )],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "circle" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(validation("`circle` expects radius and optional segments."));
                }
                let r = lower_num_expr(&args[0], scope)?;
                (
                    PyExpr::Call {
                        func: "Circle".into(),
                        args: vec![PyExpr::Inline(r)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "rounded_rect" | "rounded-rect" => {
                if args.len() < 3 || args.len() > 4 {
                    return Err(validation(
                        "`rounded_rect` expects width, height, corner radius, and optional segments.",
                    ));
                }
                let w = lower_num_expr(&args[0], scope)?;
                let h = lower_num_expr(&args[1], scope)?;
                let r = lower_num_expr(&args[2], scope)?;
                (
                    PyExpr::Call {
                        func: "RectangleRounded".into(),
                        args: vec![PyExpr::Inline(w), PyExpr::Inline(h), PyExpr::Inline(r)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "polygon" => {
                if args.len() != 1 {
                    return Err(validation("`polygon` expects a single point list."));
                }
                let points = self.lower_points_2d_args(&args[0], scope, "polygon")?;
                (
                    PyExpr::Call {
                        func: "Polygon".into(),
                        args: vec![PyExpr::Inline(points)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            // -- Booleans (Step 2) --
            "union" | "fuse" => {
                if args.len() < 2 {
                    return Err(validation(format!(
                        "`{}` expects at least two operands.",
                        node
                    )));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind(node, &operands)?;
                if kind == B123dGeomKind::Solid3d {
                    (
                        PyExpr::Call {
                            func: "_ecky_fuse_many".into(),
                            args: operands.into_iter().map(|n| n.expr).collect(),
                            kwargs: vec![],
                        },
                        kind,
                    )
                } else {
                    self.stabilize_solid_result(
                        PyExpr::BinOp {
                            op: "+",
                            operands: operands.into_iter().map(|n| n.expr).collect(),
                        },
                        kind,
                    )
                }
            }
            "compound" => {
                if args.is_empty() {
                    return Err(validation("`compound` expects at least one solid operand."));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind("compound", &operands)?;
                if kind != B123dGeomKind::Solid3d {
                    return Err(unsupported(
                        "Node `compound` currently supports 3D solids only.".to_string(),
                    ));
                }
                (
                    PyExpr::Call {
                        func: "_ecky_compound".into(),
                        args: operands.into_iter().map(|n| n.expr).collect(),
                        kwargs: vec![],
                    },
                    kind,
                )
            }
            "difference" | "cut" => {
                if args.len() < 2 {
                    return Err(validation(format!(
                        "`{}` expects at least two operands.",
                        node
                    )));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind(node, &operands)?;
                if kind == B123dGeomKind::Solid3d {
                    let exprs: Vec<PyExpr> = operands.into_iter().map(|n| n.expr).collect();
                    let (first, rest) = exprs.split_first().ok_or_else(|| {
                        validation(format!("`{}` expects at least two operands.", node))
                    })?;
                    return Ok(LoweredNode {
                        expr: PyExpr::Call {
                            func: "_ecky_cut_many".into(),
                            args: std::iter::once(first.clone())
                                .chain(rest.iter().cloned())
                                .collect(),
                            kwargs: vec![],
                        },
                        kind,
                    });
                }
                self.stabilize_solid_result(
                    PyExpr::BinOp {
                        op: "-",
                        operands: operands.into_iter().map(|n| n.expr).collect(),
                    },
                    kind,
                )
            }
            "intersection" | "common" => {
                if args.len() < 2 {
                    return Err(validation(format!(
                        "`{}` expects at least two operands.",
                        node
                    )));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind(node, &operands)?;
                if kind == B123dGeomKind::Solid3d {
                    return Ok(LoweredNode {
                        expr: PyExpr::Call {
                            func: "_ecky_common_many".into(),
                            args: operands.into_iter().map(|n| n.expr).collect(),
                            kwargs: vec![],
                        },
                        kind,
                    });
                }
                self.stabilize_solid_result(
                    PyExpr::BinOp {
                        op: "&",
                        operands: operands.into_iter().map(|n| n.expr).collect(),
                    },
                    kind,
                )
            }
            "xor" => {
                if args.len() < 2 {
                    return Err(validation("`xor` expects at least two operands."));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind("xor", &operands)?;
                let exprs: Vec<PyExpr> = operands.into_iter().map(|n| n.expr).collect();
                let sum = PyExpr::BinOp {
                    op: "+",
                    operands: exprs.clone(),
                };
                let inter = PyExpr::BinOp {
                    op: "&",
                    operands: exprs,
                };
                self.stabilize_solid_result(
                    PyExpr::BinOp {
                        op: "-",
                        operands: vec![sum, inter],
                    },
                    kind,
                )
            }
            // -- Transforms (Step 3) --
            "translate" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`translate` expects x, y, z, and a geometry node.",
                    ));
                }
                let x = lower_num_expr(&args[0], scope)?;
                let y = lower_num_expr(&args[1], scope)?;
                let z = lower_num_expr(&args[2], scope)?;
                let inner = self.lower_geom_expr(&args[3], scope)?;
                (
                    PyExpr::Transform {
                        prefix: format!("Pos({x}, {y}, {z})"),
                        inner: Box::new(inner.expr),
                    },
                    inner.kind,
                )
            }
            "rotate" => {
                if args.len() != 4 {
                    return Err(validation("`rotate` expects x, y, z, and a geometry node."));
                }
                let rx = lower_num_expr(&args[0], scope)?;
                let ry = lower_num_expr(&args[1], scope)?;
                let rz = lower_num_expr(&args[2], scope)?;
                let inner = self.lower_geom_expr(&args[3], scope)?;
                (
                    PyExpr::Transform {
                        prefix: format!("Rot({rx}, {ry}, {rz})"),
                        inner: Box::new(inner.expr),
                    },
                    inner.kind,
                )
            }
            "scale" => {
                if args.len() != 4 {
                    return Err(validation("`scale` expects x, y, z, and a geometry node."));
                }
                let sx = lower_num_expr(&args[0], scope)?;
                let sy = lower_num_expr(&args[1], scope)?;
                let sz = lower_num_expr(&args[2], scope)?;
                let inner = self.lower_geom_expr(&args[3], scope)?;
                let inner_var = self.lin.linearize(&inner.expr);
                let result = self.next_imp_var();
                let lines = vec![
                    format!("_sx, _sy, _sz = {sx}, {sy}, {sz}"),
                    "if not (abs(_sx - _sy) < 1e-9 and abs(_sy - _sz) < 1e-9): \
                     raise ValueError(f'build123d lowerer: non-uniform scale not supported \
                     ({{_sx}}, {{_sy}}, {{_sz}}).')"
                        .to_string(),
                    format!("{result} = {inner_var}.scale(_sx)"),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    inner.kind,
                )
            }
            "mirror" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`mirror` expects axis, offset, and a geometry node.",
                    ));
                }
                let axis = parse_stringish(&args[0], "mirror axis")?;
                let offset_val = lower_num_expr(&args[1], scope)?;
                let inner = self.lower_geom_expr(&args[2], scope)?;
                let plane = match axis.as_str() {
                    "x" => "Plane.YZ",
                    "y" => "Plane.XZ",
                    "z" => "Plane.XY",
                    other => {
                        return Err(validation(format!(
                            "Unsupported mirror axis `{}`. Use `x`, `y`, or `z`.",
                            other
                        )));
                    }
                };
                (
                    PyExpr::Call {
                        func: "mirror".into(),
                        args: vec![inner.expr],
                        kwargs: vec![(
                            "about".into(),
                            PyExpr::Inline(format!("{plane}.offset({offset_val})")),
                        )],
                    },
                    inner.kind,
                )
            }
            "place" => {
                let call = parse_place_call(args)?;
                let frame = self.lower_frame_expr(&call.frame, scope)?;
                let geom = self.lower_geom_expr(&call.geometry, scope)?;
                let geom_var = self.lin.linearize(&geom.expr);
                let offset = if let Some(value) = call.offset {
                    let (x, y, z) = self.lower_vec3(&value, scope, "`place :offset`")?;
                    format!("({x}, {y}, {z})")
                } else {
                    "(0.0, 0.0, 0.0)".to_string()
                };
                let rotate = if let Some(value) = call.rotate {
                    let (x, y, z) = self.lower_vec3(&value, scope, "`place :rotate`")?;
                    format!("({x}, {y}, {z})")
                } else {
                    "(0.0, 0.0, 0.0)".to_string()
                };
                let result = self.next_imp_var();
                let lines = vec![format!(
                    "{result} = _ecky_place({frame}, {geom_var}, {offset}, {rotate})"
                )];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    geom.kind,
                )
            }
            "clip-box" => {
                let call = parse_clip_box_call(args)?;
                let geom = self.lower_solid_expr(&call.geometry, scope)?;
                let geom_var = self.lin.linearize(&geom.expr);
                let (xmin, xmax) = self.lower_range_pair(&call.x, scope, "`clip-box :x`")?;
                let (ymin, ymax) = self.lower_range_pair(&call.y, scope, "`clip-box :y`")?;
                let (zmin, zmax) = self.lower_range_pair(&call.z, scope, "`clip-box :z`")?;
                let result = self.next_imp_var();
                let lines = vec![format!(
                    "{result} = _ecky_clip_box({geom_var}, {xmin}, {xmax}, {ymin}, {ymax}, {zmin}, {zmax})"
                )];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            // -- Sketch-to-solid (Step 4) --
            "extrude" => {
                if args.len() != 2 {
                    return Err(validation("`extrude` expects a sketch and height."));
                }
                let sketch = self.lower_sketch_expr(&args[0], scope)?;
                let h = lower_num_expr(&args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "extrude".into(),
                        args: vec![
                            PyExpr::Call {
                                func: "_ecky_face".into(),
                                args: vec![sketch.expr],
                                kwargs: vec![],
                            },
                            PyExpr::Inline(h),
                        ],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "revolve" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(validation(
                        "`revolve` expects a sketch, angle, and optional segments.",
                    ));
                }
                let sketch = self.lower_sketch_expr(&args[0], scope)?;
                let angle = lower_num_expr(&args[1], scope)?;
                let face = PyExpr::Call {
                    func: "_ecky_face".into(),
                    args: vec![sketch.expr],
                    kwargs: vec![],
                };
                let positioned = PyExpr::Transform {
                    prefix: "Rot(90, 0, 0)".into(),
                    inner: Box::new(face),
                };
                (
                    PyExpr::Call {
                        func: "revolve".into(),
                        args: vec![positioned],
                        kwargs: vec![
                            ("axis".into(), PyExpr::Inline("Axis.Z".into())),
                            ("revolution_arc".into(), PyExpr::Inline(angle)),
                        ],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "make-face" => {
                if args.len() != 1 {
                    return Err(validation("`make-face` expects a single wire node."));
                }
                let mut wire_operands = Vec::new();
                if Self::collect_make_face_wires(&args[0], &mut wire_operands)
                    && wire_operands.len() > 1
                {
                    let lowered: AppResult<Vec<PyExpr>> = wire_operands
                        .into_iter()
                        .map(|value| self.lower_sketch_expr(&value, scope).map(|node| node.expr))
                        .collect();
                    (
                        PyExpr::Call {
                            func: "_ecky_face_from_wires".into(),
                            args: lowered?,
                            kwargs: vec![],
                        },
                        B123dGeomKind::Sketch2d,
                    )
                } else {
                    let wire = self.lower_sketch_expr(&args[0], scope)?;
                    (
                        PyExpr::Call {
                            func: "_ecky_face".into(),
                            args: vec![wire.expr],
                            kwargs: vec![],
                        },
                        B123dGeomKind::Sketch2d,
                    )
                }
            }
            "loft" => {
                if args.len() < 3 {
                    return Err(validation(
                        "`loft` expects height, and at least two sketches.",
                    ));
                }
                let height = lower_num_expr(&args[0], scope)?;
                let mut positioned = Vec::new();
                for (i, sketch_expr) in args[1..].iter().enumerate() {
                    let sketch = self.lower_sketch_expr(sketch_expr, scope)?;
                    let t = i as f64 / (args.len() - 2) as f64;
                    positioned.push(PyExpr::Transform {
                        prefix: format!("Pos(0, 0, ({height}) * {t})"),
                        inner: Box::new(sketch.expr),
                    });
                }
                let sketch_vars: Vec<String> =
                    positioned.iter().map(|e| self.lin.linearize(e)).collect();
                (
                    PyExpr::Call {
                        func: "loft".into(),
                        args: vec![PyExpr::Inline(format!("[{}]", sketch_vars.join(", ")))],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "taper" => {
                if !(args.len() == 3 || args.len() == 4) {
                    return Err(validation(
                        "`taper` expects height, scale, sketch or height, scale-x, scale-y, sketch.",
                    ));
                }
                let height = lower_num_expr(&args[0], scope)?;
                let (scale_x, scale_y, sketch_index) = if args.len() == 3 {
                    let s = lower_num_expr(&args[1], scope)?;
                    (s.clone(), s, 2usize)
                } else {
                    (
                        lower_num_expr(&args[1], scope)?,
                        lower_num_expr(&args[2], scope)?,
                        3usize,
                    )
                };
                let sketch = self.lower_sketch_expr(&args[sketch_index], scope)?;
                let sketch_var = self.lin.linearize(&sketch.expr);
                let bottom = self.next_imp_var();
                let scaled = self.next_imp_var();
                let result = self.next_imp_var();
                let lines = vec![
                    format!("{bottom} = {sketch_var}"),
                    format!("_tsx, _tsy = {scale_x}, {scale_y}"),
                    format!(
                        "if abs(_tsx - _tsy) < 1e-9: {scaled} = Pos(0, 0, {height}) * {sketch_var}.scale(_tsx)"
                    ),
                    "else: raise ValueError('build123d lowerer: non-uniform taper scale not supported')".into(),
                    format!("{result} = loft([{bottom}, {scaled}])"),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "twist" => {
                if !(args.len() == 3 || args.len() == 4) {
                    return Err(validation(
                        "`twist` expects height, angle, sketch or height, angle, segments, sketch.",
                    ));
                }
                let height = lower_num_expr(&args[0], scope)?;
                let angle = lower_num_expr(&args[1], scope)?;
                let (segments, sketch_index) = if args.len() == 3 {
                    ("12".to_string(), 2usize)
                } else {
                    (self.lower_count(&args[2], scope)?, 3usize)
                };
                let sketch = self.lower_sketch_expr(&args[sketch_index], scope)?;
                let sketch_var = self.lin.linearize(&sketch.expr);
                let sections = self.next_imp_var();
                let result = self.next_imp_var();
                let lines = vec![
                    format!(
                        "{sections} = [Pos(0, 0, {height} * _ti / {segments}) * Rot(0, 0, {angle} * _ti / {segments}) * {sketch_var} for _ti in range({segments} + 1)]"
                    ),
                    format!("{result} = loft({sections})"),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "sweep" => {
                if args.len() != 2 {
                    return Err(validation("`sweep` expects a sketch and a path."));
                }
                let section = self.lower_sketch_expr(&args[0], scope)?;
                let path_geom = self.lower_path_expr(&args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "sweep".into(),
                        args: vec![PyExpr::Call {
                            func: "_ecky_face".into(),
                            args: vec![section.expr],
                            kwargs: vec![],
                        }],
                        kwargs: vec![("path".into(), path_geom.expr)],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            // -- Paths & sketches (Step 6) --
            "path" => {
                let points_value = if args.len() == 1 {
                    args[0].dup()
                } else {
                    Value::list(args.iter().map(Value::dup).collect())
                };
                let points = self.materialize_list_for_cad_op(
                    &points_value,
                    scope,
                    "path",
                    LoweredListKind::Point3d,
                )?;
                let mut point_strs = Vec::new();
                let mut planar_on_xy = true;
                for arg in &points.items {
                    let (x, y, z) = lower_point_3d_expr(arg, scope)?;
                    if z != "0.0" {
                        planar_on_xy = false;
                    }
                    point_strs.push(format!("({x}, {y}, {z})"));
                }
                if point_strs.len() < 2 {
                    return Err(validation("`path` expects at least two points."));
                }
                let kind = if planar_on_xy {
                    B123dGeomKind::Sketch2d
                } else {
                    B123dGeomKind::Path3d
                };
                (
                    PyExpr::Call {
                        func: "Polyline".into(),
                        args: point_strs.into_iter().map(PyExpr::Inline).collect(),
                        kwargs: vec![],
                    },
                    kind,
                )
            }
            "bezier-path" => {
                if args.is_empty() {
                    return Err(validation(
                        "`bezier-path` expects points and optional segments.",
                    ));
                }
                let points_str = self.lower_points_3d_args(&args[0], scope, "bezier-path")?;
                let pts = self.next_imp_var();
                let result = self.next_imp_var();
                let lines = vec![
                    format!("{pts} = [{points_str}]"),
                    format!(
                        "{result} = Bezier({pts}[0], {pts}[1], {pts}[2], {pts}[3])"
                    ),
                    format!("for _bi in range(3, len({pts})-1, 3):"),
                    format!(
                        "    {result} = {result} + Bezier({pts}[_bi], {pts}[_bi+1], {pts}[_bi+2], {pts}[_bi+3])"
                    ),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    B123dGeomKind::Path3d,
                )
            }
            "profile" => {
                let mut outer_wires: Vec<PyExpr> = Vec::new();
                let mut hole_wires: Vec<PyExpr> = Vec::new();
                if args.first().and_then(keyword_name).is_some() {
                    if args.len() % 2 != 0 {
                        return Err(validation(
                            "`profile` clauses must use keyword/value pairs for `:outer` and `:holes`.",
                        ));
                    }
                    let mut index = 0usize;
                    while index < args.len() {
                        let name = keyword_name(&args[index]).ok_or_else(|| {
                            validation(
                                "`profile` clauses must use keywords like `:outer` and `:holes`.",
                            )
                        })?;
                        let value = &args[index + 1];
                        match name {
                            "outer" => {
                                outer_wires.extend(self.lower_wire_collection(value, scope)?);
                            }
                            "holes" => {
                                hole_wires.extend(self.lower_wire_collection(value, scope)?);
                            }
                            other => {
                                return Err(validation(format!(
                                    "`profile` does not recognize clause `:{}`.",
                                    other
                                )))
                            }
                        }
                        index += 2;
                    }
                } else {
                    for form in args {
                        let pair = list_items(form, "profile clause")?;
                        if pair.len() != 2 {
                            return Err(validation(
                                "`profile` clauses must look like `(:outer ...)` or `(:holes ...)`.",
                            ));
                        }
                        let name = keyword_name(&pair[0]).ok_or_else(|| {
                            validation(
                                "`profile` clauses must use keywords like `:outer` and `:holes`.",
                            )
                        })?;
                        match name {
                            "outer" => {
                                outer_wires.extend(self.lower_wire_collection(&pair[1], scope)?);
                            }
                            "holes" => {
                                hole_wires.extend(self.lower_wire_collection(&pair[1], scope)?);
                            }
                            other => {
                                return Err(validation(format!(
                                    "`profile` does not recognize clause `:{}`.",
                                    other
                                )))
                            }
                        }
                    }
                }
                if outer_wires.is_empty() {
                    return Err(validation("`profile` needs at least one outer loop."));
                }
                let mut result_expr = if hole_wires.is_empty() {
                    PyExpr::Call {
                        func: "_ecky_face".into(),
                        args: vec![outer_wires.remove(0)],
                        kwargs: vec![],
                    }
                } else {
                    if outer_wires.len() != 1 {
                        return Err(unsupported(
                            "`profile` with holes currently expects a single outer loop.",
                        ));
                    }
                    PyExpr::Call {
                        func: "_ecky_face_with_holes".into(),
                        args: std::iter::once(outer_wires.remove(0))
                            .chain(hole_wires)
                            .collect(),
                        kwargs: vec![],
                    }
                };
                for wire in outer_wires {
                    result_expr = PyExpr::BinOp {
                        op: "+",
                        operands: vec![
                            result_expr,
                            PyExpr::Call {
                                func: "_ecky_face".into(),
                                args: vec![wire],
                                kwargs: vec![],
                            },
                        ],
                    };
                }
                (result_expr, B123dGeomKind::Sketch2d)
            }
            // -- Modifiers (Step 5) --
            "offset" | "offset-rounded" => {
                let (pos_args, properties) = self.parse_properties(args)?;
                if pos_args.len() != 2 {
                    return Err(validation(format!(
                        "`{}` expects distance, optional keyword properties, and a sketch.",
                        node
                    )));
                }
                let distance = lower_num_expr(&pos_args[0], scope)?;
                let sketch = self.lower_sketch_expr(&pos_args[1], scope)?;
                let mut kwargs: Vec<(String, PyExpr)> =
                    vec![("amount".into(), PyExpr::Inline(distance))];
                if let Some(openings) = properties.get("openings") {
                    kwargs.push(("openings".into(), self.lower_openings(openings, scope)?));
                }
                (
                    PyExpr::Call {
                        func: "offset".into(),
                        args: vec![sketch.expr],
                        kwargs,
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "fillet" | "chamfer" => {
                let (pos_args, properties) = self.parse_properties(args)?;
                if pos_args.len() < 2 {
                    return Err(validation(format!(
                        "`{}` expects radius and a geometry node.",
                        node
                    )));
                }
                let radius = lower_num_expr(&pos_args[0], scope)?;
                let edge_select = if let Some(value) = properties.get("edges") {
                    parse_stringish(value, "edge selection")?
                } else {
                    "all".to_string()
                };
                let body = self.lower_solid_expr(&pos_args[1], scope)?;
                let body_var = self.lin.linearize(&body.expr);
                let edges_expr = match edge_select.as_str() {
                    "all" => format!("{body_var}.edges()"),
                    "top" => format!("{body_var}.edges().group_by(Axis.Z)[-1]"),
                    "bottom" => format!("{body_var}.edges().group_by(Axis.Z)[0]"),
                    "vertical" => format!("{body_var}.edges().filter_by(Axis.Z)"),
                    other => {
                        return Err(validation(format!(
                            "Unknown edge selector `{}`. Use `all`, `top`, `bottom`, or `vertical`.",
                            other
                        )));
                    }
                };
                (
                    PyExpr::Call {
                        func: node.to_string(),
                        args: vec![PyExpr::Inline(edges_expr), PyExpr::Inline(radius)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "if" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`if` expects condition, then-shape, else-shape.",
                    ));
                }
                let cond = lower_bool_expr(&args[0], scope)?;
                let then_geom = self.lower_geom_expr(&args[1], scope)?;
                let else_geom = self.lower_geom_expr(&args[2], scope)?;
                if then_geom.kind != else_geom.kind {
                    return Err(unsupported(format!(
                        "Node `if` requires matching branch kinds, got {} and {}.",
                        then_geom.kind.noun(),
                        else_geom.kind.noun()
                    )));
                }
                let (then_lines, then_result, _) = self.lower_geom_expr_locally(&args[1], scope)?;
                let (else_lines, else_result, _) = self.lower_geom_expr_locally(&args[2], scope)?;
                let result = self.next_imp_var();
                let mut lines = vec![format!("if {cond}:")];
                lines.extend(then_lines.into_iter().map(|line| format!("    {line}")));
                lines.push(format!("    {result} = {then_result}"));
                lines.push("else:".to_string());
                lines.extend(else_lines.into_iter().map(|line| format!("    {line}")));
                lines.push(format!("    {result} = {else_result}"));
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    then_geom.kind,
                )
            }
            "rounded-polygon" | "rounded_polygon" => {
                if args.len() < 2 {
                    return Err(validation(
                        "`rounded-polygon` expects point list and corner radius.",
                    ));
                }
                let points = self.lower_points_2d_args(&args[0], scope, "rounded-polygon")?;
                let radius = lower_num_expr(&args[1], scope)?;
                let poly = PyExpr::Call {
                    func: "Polygon".into(),
                    args: vec![PyExpr::Inline(format!("[{points}]"))],
                    kwargs: vec![],
                };
                let poly_var = self.lin.linearize(&poly);
                (
                    PyExpr::Call {
                        func: "fillet".into(),
                        args: vec![
                            PyExpr::Inline(format!("{poly_var}.vertices()")),
                            PyExpr::Inline(radius),
                        ],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "bspline" => {
                let (pos_args, properties) = self.parse_properties(args)?;
                if pos_args.is_empty() {
                    return Err(validation("`bspline` expects point list."));
                }
                let point_count =
                    self.materialize_list_len(&pos_args[0], scope, "bspline points")?;
                let points = self.lower_points_2d_args(&pos_args[0], scope, "bspline")?;
                let closed = if let Some(value) = properties.get("closed") {
                    lower_bool_expr(value, scope)?
                } else if pos_args.len() > 1 {
                    lower_bool_expr(&pos_args[1], scope)?
                } else {
                    "False".to_string()
                };
                let mut kwargs: Vec<(String, PyExpr)> =
                    vec![("periodic".into(), PyExpr::Inline(closed))];
                if let Some(value) = properties.get("tangents") {
                    let tangent_count =
                        self.materialize_list_len(value, scope, "bspline tangents")?;
                    if tangent_count != 2 && tangent_count != point_count {
                        return Err(validation(format!(
                            "`tangents` expects 2 entries or one per point ({}).",
                            point_count
                        )));
                    }
                    let t = self.lower_points_2d_args(value, scope, "bspline tangents")?;
                    kwargs.push(("tangents".into(), PyExpr::Inline(format!("[{t}]"))));
                }
                if let Some(value) = properties.get("tangent_scalars") {
                    let s = self.lower_tangent_scalars(value, scope, point_count)?;
                    kwargs.push(("tangent_scalars".into(), PyExpr::Inline(s)));
                }
                (
                    PyExpr::Call {
                        func: "Spline".into(),
                        args: vec![PyExpr::Inline(format!("[{points}]"))],
                        kwargs,
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "text" => {
                if args.len() < 2 {
                    return Err(validation("`text` expects string and size."));
                }
                let txt = lower_stringish_expr(&args[0], scope)?;
                let size = lower_num_expr(&args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "Text".into(),
                        args: vec![PyExpr::Inline(txt)],
                        kwargs: vec![("font_size".into(), PyExpr::Inline(size))],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "svg" => {
                if args.len() != 1 {
                    return Err(validation("`svg` expects a file path."));
                }
                let path = lower_stringish_expr(&args[0], scope)?;
                (
                    PyExpr::Call {
                        func: "import_svg".into(),
                        args: vec![PyExpr::Inline(path)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "import-stl" | "import_stl" => {
                if args.len() != 1 {
                    return Err(validation("`import-stl` expects a file path."));
                }
                let path = lower_stringish_expr(&args[0], scope)?;
                (
                    PyExpr::Call {
                        func: "import_stl".into(),
                        args: vec![PyExpr::Inline(path)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            // -- Arrays (Step 7) --
            "linear-array" => {
                let call = parse_linear_array_call(args)?;
                let count = self.lower_count(&call.count, scope)?;
                let dx = lower_num_expr(&call.dx, scope)?;
                let dy = lower_num_expr(&call.dy, scope)?;
                let dz = lower_num_expr(&call.dz, scope)?;
                let base = self.lower_geom_expr(&call.geometry, scope)?;
                let base_var = self.lin.linearize(&base.expr);
                let result = self.next_imp_var();
                let loop_var = "__ecky_la_i";
                let lines = vec![
                    format!("{result} = {base_var}"),
                    format!("for {loop_var} in range(1, {count}):"),
                    format!(
                        "    {result} = {result} + Pos({dx} * {loop_var}, {dy} * {loop_var}, {dz} * {loop_var}) * {base_var}"
                    ),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    base.kind,
                )
            }
            "radial-array" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`radial-array` expects count, step degrees, radius, and a mesh.",
                    ));
                }
                let count = self.lower_count(&args[0], scope)?;
                let step_deg = lower_num_expr(&args[1], scope)?;
                let radius = lower_num_expr(&args[2], scope)?;
                let base = self.lower_geom_expr(&args[3], scope)?;
                let base_var = self.lin.linearize(&base.expr);
                let translated = self.next_imp_var();
                let result = self.next_imp_var();
                let loop_var = "__ecky_ra_i";
                let lines = vec![
                    format!("{translated} = Pos({radius}, 0, 0) * {base_var}"),
                    format!("{result} = {translated}"),
                    format!("for {loop_var} in range(1, {count}):"),
                    format!(
                        "    {result} = {result} + Rot(0, 0, {step_deg} * {loop_var}) * {translated}"
                    ),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    base.kind,
                )
            }
            "grid-array" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`grid-array` expects rows, cols, dx, dy, and a mesh.",
                    ));
                }
                let rows = self.lower_count(&args[0], scope)?;
                let cols = self.lower_count(&args[1], scope)?;
                let dx = lower_num_expr(&args[2], scope)?;
                let dy = lower_num_expr(&args[3], scope)?;
                let base = self.lower_geom_expr(&args[4], scope)?;
                let base_var = self.lin.linearize(&base.expr);
                let result = self.next_imp_var();
                let row_var = "__ecky_ga_r";
                let col_var = "__ecky_ga_c";
                let lines = vec![
                    format!("{result} = {base_var}"),
                    format!("for {row_var} in range({rows}):"),
                    format!("    for {col_var} in range({cols}):"),
                    format!(
                        "        if {row_var} != 0 or {col_var} != 0: {result} = {result} + Pos({dx} * {col_var}, {dy} * {row_var}, 0) * {base_var}"
                    ),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    base.kind,
                )
            }
            "arc-array" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`arc-array` expects count, radius, start degrees, end degrees, and a mesh.",
                    ));
                }
                let count = self.lower_count(&args[0], scope)?;
                let radius = lower_num_expr(&args[1], scope)?;
                let start_deg = lower_num_expr(&args[2], scope)?;
                let end_deg = lower_num_expr(&args[3], scope)?;
                let base = self.lower_geom_expr(&args[4], scope)?;
                let base_var = self.lin.linearize(&base.expr);
                let first = self.next_imp_var();
                let result = self.next_imp_var();
                let step_var = "__ecky_arc_step";
                let loop_var = "__ecky_aa_i";
                let lines = vec![
                    format!(
                        "{step_var} = (({end_deg}) - ({start_deg})) / max(1, {count} - 1)"
                    ),
                    format!(
                        "{first} = Rot(0, 0, {start_deg}) * Pos({radius}, 0, 0) * {base_var}"
                    ),
                    format!("{result} = {first}"),
                    format!("for {loop_var} in range(1, {count}):"),
                    format!(
                        "    {result} = {result} + Rot(0, 0, ({start_deg}) + {step_var} * {loop_var}) * Pos({radius}, 0, 0) * {base_var}"
                    ),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    base.kind,
                )
            }
            "repeat-union" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`repeat-union` expects index symbol, count, and a body.",
                    ));
                }
                let index = args[0]
                    .as_symbol()
                    .ok_or_else(|| validation("`repeat-union` index must be a symbol."))?;
                let count = lower_num_expr(&args[1], scope)?;
                let count_var = self.next_imp_var();
                let result = self.next_imp_var();
                let loop_var = format!("__ecky_ru_{}", index.replace('-', "_"));
                let local_name = format!("_{}", index.replace('-', "_"));
                let mut frame = BTreeMap::new();
                frame.insert(
                    index.to_string(),
                    LoweredBinding::Number(local_name.clone()),
                );
                let child_scope = scope.with_frame(frame);
                let (body_lines, body_result, body_kind) =
                    self.lower_geom_expr_locally(&args[2], &child_scope)?;
                let mut lines = vec![
                    format!("{result} = None"),
                    format!("{count_var} = max(0, int(math.floor({count})))"),
                    format!("for {loop_var} in range({count_var}):"),
                    format!("    {local_name} = float({loop_var})"),
                ];
                lines.extend(body_lines.into_iter().map(|line| format!("    {line}")));
                lines.push(format!(
                    "    {result} = {body_result} if {result} is None else {result} + {body_result}"
                ));
                lines.push(format!(
                    "if {result} is None: raise ValueError('repeat-union produced no geometry')"
                ));
                self.stabilize_solid_result(
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    body_kind,
                )
            }
            "repeat-pick" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`repeat-pick` expects index symbol, count, predicate, and body.",
                    ));
                }
                let index = args[0]
                    .as_symbol()
                    .ok_or_else(|| validation("`repeat-pick` index must be a symbol."))?;
                let count = lower_num_expr(&args[1], scope)?;
                let count_var = self.next_imp_var();
                let result = self.next_imp_var();
                let loop_var = format!("__ecky_rp_{}", index.replace('-', "_"));
                let local_name = format!("_{}", index.replace('-', "_"));
                let mut frame = BTreeMap::new();
                frame.insert(
                    index.to_string(),
                    LoweredBinding::Number(local_name.clone()),
                );
                let child_scope = scope.with_frame(frame);
                let predicate = lower_bool_expr(&args[2], &child_scope)?;
                let (body_lines, body_result, body_kind) =
                    self.lower_geom_expr_locally(&args[3], &child_scope)?;
                let mut lines = vec![
                    format!("{result} = None"),
                    format!("{count_var} = max(0, int(math.floor({count})))"),
                    format!("for {loop_var} in range({count_var}):"),
                    format!("    {local_name} = float({loop_var})"),
                    format!("    if {predicate}:"),
                ];
                lines.extend(body_lines.into_iter().map(|line| format!("        {line}")));
                lines.push(format!("        {result} = {body_result}"));
                lines.push(format!(
                    "if {result} is None: raise ValueError('repeat-pick found no matching geometry')"
                ));
                self.stabilize_solid_result(
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    body_kind,
                )
            }
            "repeat-compound" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`repeat-compound` expects index symbol, count, and a body.",
                    ));
                }
                let index = args[0]
                    .as_symbol()
                    .ok_or_else(|| validation("`repeat-compound` index must be a symbol."))?;
                let count = lower_num_expr(&args[1], scope)?;
                let count_var = self.next_imp_var();
                let solids_var = self.next_imp_var();
                let result = self.next_imp_var();
                let loop_var = format!("__ecky_rc_{}", index.replace('-', "_"));
                let local_name = format!("_{}", index.replace('-', "_"));
                let mut frame = BTreeMap::new();
                frame.insert(
                    index.to_string(),
                    LoweredBinding::Number(local_name.clone()),
                );
                let child_scope = scope.with_frame(frame);
                let (body_lines, body_result, body_kind) =
                    self.lower_geom_expr_locally(&args[2], &child_scope)?;
                if body_kind != B123dGeomKind::Solid3d {
                    return Err(unsupported(
                        "Node `repeat-compound` currently supports 3D solids only.".to_string(),
                    ));
                }
                let mut lines = vec![
                    format!("{solids_var} = []"),
                    format!("{count_var} = max(0, int(math.floor({count})))"),
                    format!("for {loop_var} in range({count_var}):"),
                    format!("    {local_name} = float({loop_var})"),
                ];
                lines.extend(body_lines.into_iter().map(|line| format!("    {line}")));
                lines.push(format!(
                    "    {solids_var}.extend(_ecky_collect_solids({body_result}))"
                ));
                lines.push(format!("{result} = Compound(children={solids_var})"));
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            // -- Shell (Step 9) --
            "shell" => {
                if args.len() != 2 {
                    return Err(validation(
                        "`shell` expects wall thickness and a geometry node.",
                    ));
                }
                let wall = lower_num_expr(&args[0], scope)?;
                let outer = self.lower_solid_expr(&args[1], scope)?;
                match self.plan_shell_target(&args[1], &args[0])? {
                    ShellLoweringPlan::BooleanInner(inner_target) => {
                        let inner = self.lower_solid_expr(&inner_target, scope)?;
                        (
                            PyExpr::BinOp {
                                op: "-",
                                operands: vec![outer.expr, inner.expr],
                            },
                            B123dGeomKind::Solid3d,
                        )
                    }
                    ShellLoweringPlan::SolidOffsetPlanarFaces => {
                        let outer_var = self.lin.linearize(&outer.expr);
                        let result = self.next_imp_var();
                        let lines = vec![format!(
                            "{result} = offset({outer_var}, amount=-({wall}), openings={outer_var}.faces().filter_by(GeomType.PLANE))"
                        )];
                        (
                            PyExpr::Imperative {
                                lines,
                                result_var: result,
                            },
                            B123dGeomKind::Solid3d,
                        )
                    }
                }
            }
            "wall-pattern" | "pattern" => {
                return Err(unsupported(
                    "Node `wall-pattern` is not yet supported by the build123d lowerer. \
                     Use the EckyRust backend for this model or implement the pattern geometrically.",
                ));
            }
            other => {
                return Err(unsupported(format!(
                    "Node `{}` is not yet supported by the build123d lowerer. \
                     Use the EckyRust backend for this model.",
                    other
                )));
            }
        };

        Ok(LoweredNode { expr, kind })
    }
}
