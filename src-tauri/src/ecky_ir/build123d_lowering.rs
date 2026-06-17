use std::collections::{BTreeMap, BTreeSet};

use crate::ecky_core_ir::{
    CoreArrayOp, CoreBooleanOp, CoreFrameOp, CoreLiteral, CoreMetaOp, CoreNode, CoreNodeKind,
    CoreOperation, CorePathOp, CorePrimitive, CoreProgram, CoreReference, CoreSelectorPayload,
    CoreShapeBinding, CoreSurfaceOp, CoreSymbol, CoreTransformOp, CoreValueKind,
};
use crate::models::{AppResult, ParamValue};

use super::edge_ops::{
    edge_selector_spec_from_core_payload, face_selector_spec_from_core_payload,
    parse_edge_selector_spec,
};
use super::model::{
    allocate_legacy_local_name, core_program_param_defaults, expr_head_symbol as head_symbol,
    expr_keyword_name as keyword_name, expr_list_items as list_items,
    expr_parse_edge_selector_spec, expr_parse_face_selector_spec,
    expr_parse_stringish as parse_stringish, materialize_selector_nodes, parse_model,
    parse_typed_build_expr as parse_build_expr, parse_value_kind_tag, IrExpr as Value, IrModel,
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

fn core_selector_payload_to_ir_value(payload: &CoreSelectorPayload) -> AppResult<Value> {
    match payload {
        CoreSelectorPayload::EdgeAll
        | CoreSelectorPayload::EdgeClauses(_)
        | CoreSelectorPayload::EdgeTag(_)
        | CoreSelectorPayload::EdgeTargetIds(_) => Ok(Value::Selector(
            crate::ecky_ir::model::IrSelectorExpr::Edge(edge_selector_spec_from_core_payload(
                payload,
            )?),
        )),
        CoreSelectorPayload::FaceClauses(_)
        | CoreSelectorPayload::FaceTag(_)
        | CoreSelectorPayload::FaceTargetIds(_) => Ok(Value::Selector(
            crate::ecky_ir::model::IrSelectorExpr::Face(face_selector_spec_from_core_payload(
                payload,
            )?),
        )),
    }
}

#[cfg(test)]
mod typed_hole_tests {
    use super::lower_core_program_to_build123d;

    fn typed_hole_cases() -> [(&'static str, &'static str, &'static str); 4] {
        [
            (
                "solid",
                "solid cutout",
                r#"(model
                    (part shell
                      (difference
                        (box 1 1 1)
                        (hole :type solid :goal "solid cutout"))))"#,
            ),
            (
                "sketch",
                "sketch profile",
                r#"(model
                    (part body
                      (extrude
                        (hole :type sketch :goal "sketch profile")
                        5)))"#,
            ),
            (
                "path",
                "path spine",
                r#"(model
                    (part rail
                      (sweep
                        (circle 1)
                        (hole :type path :goal "path spine"))))"#,
            ),
            (
                "shape",
                "generic shape",
                r#"(model
                    (part body
                      (translate
                        1 0 0
                        (hole :type shape :goal "generic shape"))))"#,
            ),
        ]
    }

    #[test]
    fn lower_core_program_rejects_typed_hole_kinds() {
        for (type_name, goal, source) in typed_hole_cases() {
            let program = crate::ecky_scheme::compile_to_core_program(source)
                .unwrap_or_else(|err| panic!("{type_name} hole should compile: {err}"));
            let err = match lower_core_program_to_build123d(&program) {
                Ok(script) => panic!("{type_name} hole lowered into script: {script}"),
                Err(err) => err,
            };
            let message = err.to_string();

            assert!(message.contains("Typed hole"), "{message}");
            assert!(
                message.contains(&format!("requested type `{type_name}`")),
                "{message}"
            );
            assert!(message.contains(goal), "{message}");
            assert!(
                message.contains("must be filled before render/lowering"),
                "{message}"
            );
        }
    }
}

pub fn lower_to_build123d(source: &str) -> AppResult<String> {
    match parse_model(source) {
        Ok(model) => lower_model_to_build123d(&model),
        Err(_) => {
            let program = crate::ecky_scheme::compile_to_core_program(source)
                .map_err(|err| {
                    crate::contracts::AuthoringDiagnostic::surface(
                        crate::contracts::AuthoringReason::ParseSyntax,
                        err.to_string(),
                    ).to_app_error()
                })?;
            lower_core_program_to_build123d(&program)
        }
    }
}

pub(crate) fn lower_model_to_build123d(model: &IrModel) -> AppResult<String> {
    let defaults = model
        .params
        .iter()
        .map(|param| (param.field.key().to_string(), param.default_value.clone()))
        .collect::<BTreeMap<_, _>>();
    let parts = model
        .parts
        .iter()
        .map(|part| (part.part_id.clone(), part.expr.dup(), part.value_kind))
        .collect::<Vec<_>>();
    lower_parts_to_build123d(&defaults, &parts)
}

pub(crate) fn lower_core_program_to_build123d(program: &CoreProgram) -> AppResult<String> {
    let defaults = core_program_param_defaults(program)?;
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut lowerer = ExprLowerer::new(&defaults);
    lowerer.lower_core_parts(&program.parts, &param_names)
}

fn lower_parts_to_build123d(
    defaults: &BTreeMap<String, ParamValue>,
    parts: &[(String, Value, Option<CoreValueKind>)],
) -> AppResult<String> {
    let mut lowerer = ExprLowerer::new(defaults);
    lowerer.lower_parts(parts)
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeListKind {
    Number,
    Geom(B123dGeomKind),
}

#[derive(Clone, Debug)]
struct LoweredRuntimeList {
    var: String,
    kind: RuntimeListKind,
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
    RuntimeList(LoweredRuntimeList),
    Frame(String),
    Number(String),
    Boolean(String),
    Stringish(String),
}

#[derive(Clone, Debug)]
struct LoweringScope<'a> {
    params: &'a BTreeMap<String, ParamValue>,
    locals: Vec<BTreeMap<String, LoweredBinding>>,
    current_part_id: Option<String>,
}

impl<'a> LoweringScope<'a> {
    fn new(params: &'a BTreeMap<String, ParamValue>) -> Self {
        Self {
            params,
            locals: Vec::new(),
            current_part_id: None,
        }
    }

    fn with_frame(&self, frame: BTreeMap<String, LoweredBinding>) -> Self {
        let mut locals = self.locals.clone();
        locals.push(frame);
        Self {
            params: self.params,
            locals,
            current_part_id: self.current_part_id.clone(),
        }
    }

    fn with_part_id(&self, part_id: &str) -> Self {
        Self {
            params: self.params,
            locals: self.locals.clone(),
            current_part_id: Some(part_id.to_string()),
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
        match sym {
            "pi" => return Ok("math.pi".to_string()),
            "tau" => return Ok("(2.0 * math.pi)".to_string()),
            _ => {}
        }
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
    let op = head_symbol(items, "numeric expression")?;
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
        "atan2" => {
            if args.len() != 2 {
                return Err(validation("`atan2` expects y and x."));
            }
            Ok(format!(
                "math.atan2({}, {})",
                lower_num_expr(&args[0], scope)?,
                lower_num_expr(&args[1], scope)?
            ))
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
        "deg" | "deg->rad" => {
            if args.len() != 1 {
                return Err(validation("`deg`/`deg->rad` expects one argument."));
            }
            Ok(format!(
                "math.radians({})",
                lower_num_expr(&args[0], scope)?
            ))
        }
        "rad" | "rad->deg" => {
            if args.len() != 1 {
                return Err(validation("`rad`/`rad->deg` expects one argument."));
            }
            Ok(format!(
                "math.degrees({})",
                lower_num_expr(&args[0], scope)?
            ))
        }
        "signed-pow" => {
            if args.len() != 2 {
                return Err(validation("`signed-pow` expects value and exponent."));
            }
            Ok(format!(
                "_ecky_signed_pow({}, {})",
                lower_num_expr(&args[0], scope)?,
                lower_num_expr(&args[1], scope)?
            ))
        }
        "hash01" | "hash-signed" | "noise2" | "voronoi2" | "cell-distance2" => {
            if args.len() != 3 {
                return Err(validation(format!("`{}` expects x, y, and seed.", op)));
            }
            let func = match op {
                "hash01" => "_ecky_hash01",
                "hash-signed" => "_ecky_hash_signed",
                "noise2" => "_ecky_noise2",
                "voronoi2" => "_ecky_voronoi2",
                "cell-distance2" => "_ecky_cell_distance2",
                _ => unreachable!(),
            };
            Ok(format!(
                "{func}({}, {}, {})",
                lower_num_expr(&args[0], scope)?,
                lower_num_expr(&args[1], scope)?,
                lower_num_expr(&args[2], scope)?
            ))
        }
        "fbm2" => {
            if args.len() != 6 {
                return Err(validation(
                    "`fbm2` expects x, y, seed, octaves, lacunarity, and gain.",
                ));
            }
            Ok(format!(
                "_ecky_fbm2({}, {}, {}, {}, {}, {})",
                lower_num_expr(&args[0], scope)?,
                lower_num_expr(&args[1], scope)?,
                lower_num_expr(&args[2], scope)?,
                lower_num_expr(&args[3], scope)?,
                lower_num_expr(&args[4], scope)?,
                lower_num_expr(&args[5], scope)?
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

    if matches!(head_symbol(&items, "list item").ok(), Some("let" | "let*")) && items.len() == 3 {
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
        LoweredBinding::RuntimeList(list) => list.kind.noun(),
        LoweredBinding::Frame(_) => "frame",
        LoweredBinding::Number(_) => "number",
        LoweredBinding::Boolean(_) => "boolean",
        LoweredBinding::Stringish(_) => "string-like value",
    }
}

impl RuntimeListKind {
    fn noun(&self) -> &'static str {
        match self {
            Self::Number => "runtime number list",
            Self::Geom(kind) => match kind {
                B123dGeomKind::Sketch2d => "runtime 2D sketch list",
                B123dGeomKind::Solid3d => "runtime 3D solid list",
                B123dGeomKind::Path3d => "runtime 3D path list",
            },
        }
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
        if pair.len() != 2 && pair.len() != 4 {
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
    if matches!(
        head_symbol(&items, "2D point expression").ok(),
        Some("let" | "let*")
    ) && items.len() == 3
    {
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
    if matches!(
        head_symbol(&items, "3D point expression").ok(),
        Some("let" | "let*")
    ) && items.len() == 3
    {
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

fn extract_let_binding_hint(pair: &[Value]) -> Option<CoreValueKind> {
    if pair.len() == 4 {
        keyword_name(&pair[2])
            .filter(|k| *k == "value-kind")
            .and_then(|_| pair[3].as_symbol())
            .and_then(parse_value_kind_tag)
    } else {
        None
    }
}

fn core_symbol_name(symbol: &CoreSymbol) -> &'static str {
    match symbol {
        CoreSymbol::Start => "start",
        CoreSymbol::End => "end",
        CoreSymbol::Xy => "xy",
        CoreSymbol::Yz => "yz",
        CoreSymbol::Xz => "xz",
        CoreSymbol::Min => "min",
        CoreSymbol::Center => "center",
        CoreSymbol::Max => "max",
    }
}

fn core_value_kind_tag_local(kind: CoreValueKind) -> &'static str {
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

fn core_operation_name_local(op: &CoreOperation) -> String {
    match op {
        CoreOperation::Primitive(CorePrimitive::Box) => "box".to_string(),
        CoreOperation::Primitive(CorePrimitive::Sphere) => "sphere".to_string(),
        CoreOperation::Primitive(CorePrimitive::Cylinder) => "cylinder".to_string(),
        CoreOperation::Primitive(CorePrimitive::Cone) => "cone".to_string(),
        CoreOperation::Primitive(CorePrimitive::Torus) => "torus".to_string(),
        CoreOperation::Primitive(CorePrimitive::Wedge) => "wedge".to_string(),
        CoreOperation::Primitive(CorePrimitive::Ellipse) => "ellipse".to_string(),
        CoreOperation::Primitive(CorePrimitive::Slot) => "slot-overall".to_string(),
        CoreOperation::Primitive(CorePrimitive::SlotArc) => "slot-arc".to_string(),
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
        CoreOperation::Surface(CoreSurfaceOp::Draft) => "draft".to_string(),
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

fn fmt_f64(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}.0", n as i64)
    } else {
        // Use Rust's default Display which gives enough precision
        format!("{}", n)
    }
}

fn python_local_ident(symbol: &str, prefix: &str) -> String {
    let mut out = String::from(prefix);
    let mut wrote_any = false;

    for ch in symbol.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => {
                out.push(ch);
                wrote_any = true;
            }
            '-' => {
                out.push('_');
                wrote_any = true;
            }
            other => {
                use std::fmt::Write as _;
                let _ = write!(out, "_u{:x}_", other as u32);
                wrote_any = true;
            }
        }
    }

    if !wrote_any {
        out.push_str("value");
    }

    out
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
    let op = head_symbol(items, "boolean expression")?;
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
                LoweredBinding::Geom(_)
                | LoweredBinding::Frame(_)
                | LoweredBinding::List(_)
                | LoweredBinding::RuntimeList(_) => Err(unsupported(format!(
                    "Symbol `{}` is not a string-like binding in this context.",
                    sym
                ))),
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
struct PlaneCall {
    origin: Option<Value>,
    x: Option<Value>,
    normal: Option<Value>,
}

#[derive(Debug, PartialEq)]
struct LocationCall {
    frame: Value,
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

struct SampledRadialLoftCall {
    binders: [String; 3],
    height: Value,
    z_steps: Value,
    theta_steps: Value,
    radius: Value,
    z_map: Option<Value>,
}

struct HelicalRidgeCall {
    radius: Value,
    pitch: Value,
    height: Value,
    base_width: Value,
    crest_width: Value,
    depth: Value,
    female: Option<Value>,
    clearance: Option<Value>,
    lefthand: Option<Value>,
}

struct ThreadCall {
    iso: Option<Value>,
    radius: Option<Value>,
    pitch: Option<Value>,
    length: Value,
    depth: Option<Value>,
    base_width: Option<Value>,
    crest_width: Option<Value>,
    female: Option<Value>,
    clearance: Option<Value>,
    lefthand: Option<Value>,
}

fn parse_thread_call(args: &[Value]) -> AppResult<ThreadCall> {
    let parsed = ParsedCallArgs::parse(
        "thread",
        args,
        &[
            "iso",
            "radius",
            "pitch",
            "length",
            "depth",
            "base-width",
            "crest-width",
            "female",
            "clearance",
            "lefthand",
        ],
    )?;
    if !parsed.positional.is_empty() {
        return Err(validation(
            "`thread` expects keyword options: either `:iso \"M6\"` or `:radius`/`:pitch`/`:depth`, plus `:length` and optional `:base-width`, `:crest-width`, `:female`, `:clearance`, `:lefthand`.",
        ));
    }
    Ok(ThreadCall {
        iso: parsed.keywords.get("iso").map(Value::dup),
        radius: parsed.keywords.get("radius").map(Value::dup),
        pitch: parsed.keywords.get("pitch").map(Value::dup),
        length: parsed
            .keywords
            .get("length")
            .map(Value::dup)
            .ok_or_else(|| validation("`thread` requires `:length`."))?,
        depth: parsed.keywords.get("depth").map(Value::dup),
        base_width: parsed.keywords.get("base_width").map(Value::dup),
        crest_width: parsed.keywords.get("crest_width").map(Value::dup),
        female: parsed.keywords.get("female").map(Value::dup),
        clearance: parsed.keywords.get("clearance").map(Value::dup),
        lefthand: parsed.keywords.get("lefthand").map(Value::dup),
    })
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

fn typed_hole_error(args: &[Value]) -> String {
    let parsed = ParsedCallArgs::parse("hole", args, &["type", "goal"]);
    let (type_name, goal) = match parsed {
        Ok(parsed) => (
            parsed
                .keywords
                .get("type")
                .and_then(|value| parse_stringish(value, "hole type").ok()),
            parsed
                .keywords
                .get("goal")
                .and_then(|value| parse_stringish(value, "hole goal").ok()),
        ),
        Err(_) => (None, None),
    };
    match (type_name, goal) {
        (Some(type_name), Some(goal)) => format!(
            "Typed hole requested type `{}` with goal `{}` must be filled before render/lowering.",
            type_name, goal
        ),
        (Some(type_name), None) => format!(
            "Typed hole requested type `{}` must be filled before render/lowering.",
            type_name
        ),
        (None, Some(goal)) => format!(
            "Typed hole with goal `{}` must be filled before render/lowering.",
            goal
        ),
        (None, None) => "Typed hole must be filled before render/lowering.".to_string(),
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

fn parse_plane_call(args: &[Value]) -> AppResult<PlaneCall> {
    let parsed = ParsedCallArgs::parse("plane", args, &["origin", "x", "normal"])?;
    if !parsed.positional.is_empty() {
        return Err(validation(
            "`plane` expects only `:origin`, `:x`, and `:normal` options.",
        ));
    }
    Ok(PlaneCall {
        origin: parsed.keywords.get("origin").map(Value::dup),
        x: parsed.keywords.get("x").map(Value::dup),
        normal: parsed.keywords.get("normal").map(Value::dup),
    })
}

fn parse_location_call(args: &[Value]) -> AppResult<LocationCall> {
    let parsed = ParsedCallArgs::parse("location", args, &["offset", "rotate"])?;
    if parsed.positional.len() != 1 {
        return Err(validation(
            "`location` expects a plane/frame and optional `:offset` / `:rotate`.",
        ));
    }
    Ok(LocationCall {
        frame: parsed.positional[0].dup(),
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

fn parse_sampled_radial_loft_call(args: &[Value]) -> AppResult<SampledRadialLoftCall> {
    if args.is_empty() {
        return Err(validation(
            "`sampled-radial-loft` expects binder names plus keyword/value options.",
        ));
    }
    let binders = list_items(&args[0], "`sampled-radial-loft` binders")?;
    if binders.len() != 3 {
        return Err(validation(
            "`sampled-radial-loft` binders must be `(theta z fz)`.",
        ));
    }
    let parsed = ParsedCallArgs::parse(
        "sampled-radial-loft",
        &args[1..],
        &["height", "z-steps", "theta-steps", "radius", "z-map"],
    )?;
    if !parsed.positional.is_empty() {
        return Err(validation(
            "`sampled-radial-loft` expects only keyword/value options after the binder list.",
        ));
    }
    Ok(SampledRadialLoftCall {
        binders: [
            parse_stringish(&binders[0], "`sampled-radial-loft` theta binder")?,
            parse_stringish(&binders[1], "`sampled-radial-loft` z binder")?,
            parse_stringish(&binders[2], "`sampled-radial-loft` fz binder")?,
        ],
        height: parsed
            .keywords
            .get("height")
            .map(Value::dup)
            .ok_or_else(|| validation("`sampled-radial-loft` requires `:height`."))?,
        z_steps: parsed
            .keywords
            .get("z_steps")
            .map(Value::dup)
            .ok_or_else(|| validation("`sampled-radial-loft` requires `:z-steps`."))?,
        theta_steps: parsed
            .keywords
            .get("theta_steps")
            .map(Value::dup)
            .ok_or_else(|| validation("`sampled-radial-loft` requires `:theta-steps`."))?,
        radius: parsed
            .keywords
            .get("radius")
            .map(Value::dup)
            .ok_or_else(|| validation("`sampled-radial-loft` requires `:radius`."))?,
        z_map: parsed.keywords.get("z_map").map(Value::dup),
    })
}

fn parse_helical_ridge_call(args: &[Value]) -> AppResult<HelicalRidgeCall> {
    let parsed = ParsedCallArgs::parse(
        "helical-ridge",
        args,
        &[
            "radius",
            "pitch",
            "height",
            "base-width",
            "crest-width",
            "depth",
            "female",
            "clearance",
            "lefthand",
        ],
    )?;
    if !parsed.positional.is_empty() {
        return Err(validation(
            "`helical-ridge` expects keyword options: `:radius`, `:pitch`, `:height`, `:base-width`, `:crest-width`, and `:depth`.",
        ));
    }
    Ok(HelicalRidgeCall {
        radius: parsed
            .keywords
            .get("radius")
            .map(Value::dup)
            .ok_or_else(|| validation("`helical-ridge` requires `:radius`."))?,
        pitch: parsed
            .keywords
            .get("pitch")
            .map(Value::dup)
            .ok_or_else(|| validation("`helical-ridge` requires `:pitch`."))?,
        height: parsed
            .keywords
            .get("height")
            .map(Value::dup)
            .ok_or_else(|| validation("`helical-ridge` requires `:height`."))?,
        base_width: parsed
            .keywords
            .get("base_width")
            .map(Value::dup)
            .ok_or_else(|| validation("`helical-ridge` requires `:base-width`."))?,
        crest_width: parsed
            .keywords
            .get("crest_width")
            .map(Value::dup)
            .ok_or_else(|| validation("`helical-ridge` requires `:crest-width`."))?,
        depth: parsed
            .keywords
            .get("depth")
            .map(Value::dup)
            .ok_or_else(|| validation("`helical-ridge` requires `:depth`."))?,
        female: parsed.keywords.get("female").map(Value::dup),
        clearance: parsed.keywords.get("clearance").map(Value::dup),
        lefthand: parsed.keywords.get("lefthand").map(Value::dup),
    })
}

fn parse_lambda_expr(value: &Value) -> AppResult<(Vec<String>, Value)> {
    let items = list_items(value, "lambda expression")?;
    if head_symbol(items, "lambda expression")? != "lambda" || items.len() != 3 {
        return Err(validation("`map` expects `(lambda (args ...) body)`."));
    }
    let params = list_items(&items[1], "lambda parameter list")?
        .iter()
        .map(|param| {
            param
                .as_symbol()
                .map(str::to_string)
                .ok_or_else(|| validation("Lambda parameters must be symbols."))
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok((params, items[2].dup()))
}

fn parse_align_axis(value: &Value, node: &str) -> AppResult<&'static str> {
    match value.as_symbol().or_else(|| value.as_str()) {
        Some("min") => Ok("Align.MIN"),
        Some("center") => Ok("Align.CENTER"),
        Some("max") => Ok("Align.MAX"),
        Some(other) => Err(validation(format!(
            "`{} :align` expects `min`, `center`, or `max`, got `{}`.",
            node, other
        ))),
        None => Err(validation(format!(
            "`{} :align` expects `(x y z)` axis symbols.",
            node
        ))),
    }
}

fn parse_align_tuple(value: Option<&Value>, node: &str, default: &str) -> AppResult<String> {
    let Some(value) = value else {
        return Ok(default.to_string());
    };
    let items = {
        let parsed = list_items(value, "align tuple")?;
        if parsed.len() == 2 && parsed.first().and_then(Value::as_symbol) == Some("quote") {
            list_items(&parsed[1], "align tuple")?
        } else {
            parsed
        }
    };
    if items.len() != 3 {
        return Err(validation(format!("`{} :align` expects `(x y z)`.", node)));
    }
    Ok(format!(
        "({}, {}, {})",
        parse_align_axis(&items[0], node)?,
        parse_align_axis(&items[1], node)?,
        parse_align_axis(&items[2], node)?,
    ))
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
        "from _ecky_build123d_helpers import *".into(),
        "import math".into(),
        "def _ecky_fract01(value):\n    value = float(value)\n    wrapped = value - math.floor(value)\n    return max(0.0, min(1.0, wrapped))".into(),
        "def _ecky_hash01(x, y, seed):\n    raw = math.sin(float(x) * 127.1 + float(y) * 311.7 + float(seed) * 74.7) * 43758.5453123\n    return _ecky_fract01(raw)".into(),
        "def _ecky_hash_signed(x, y, seed):\n    return _ecky_hash01(x, y, seed) * 2.0 - 1.0".into(),
        "def _ecky_smoothstep01(x):\n    t = max(0.0, min(1.0, float(x)))\n    return t * t * (3.0 - 2.0 * t)".into(),
        "def _ecky_noise2(x, y, seed):\n    x0 = math.floor(float(x)); y0 = math.floor(float(y))\n    xf = float(x) - x0; yf = float(y) - y0\n    n00 = _ecky_hash01(x0, y0, seed); n10 = _ecky_hash01(x0 + 1.0, y0, seed)\n    n01 = _ecky_hash01(x0, y0 + 1.0, seed); n11 = _ecky_hash01(x0 + 1.0, y0 + 1.0, seed)\n    sx = _ecky_smoothstep01(xf); sy = _ecky_smoothstep01(yf)\n    ix0 = n00 + (n10 - n00) * sx; ix1 = n01 + (n11 - n01) * sx\n    return max(0.0, min(1.0, ix0 + (ix1 - ix0) * sy))".into(),
        "def _ecky_fbm2(x, y, seed, octaves, lacunarity, gain):\n    octaves = max(1, min(12, int(round(float(octaves)))))\n    lacunarity = max(0.0001, float(lacunarity)); gain = max(0.0, min(1.0, float(gain)))\n    amp = 0.5; freq = 1.0; total = 0.0; norm = 0.0\n    for octave in range(octaves):\n        total += _ecky_noise2(float(x) * freq, float(y) * freq, float(seed) + octave * 17.0) * amp\n        norm += amp; amp *= gain; freq *= lacunarity\n    return 0.0 if norm <= 1e-12 else max(0.0, min(1.0, total / norm))".into(),
        "def _ecky_cell_distance2(x, y, seed):\n    cx = math.floor(float(x)); cy = math.floor(float(y)); best = float('inf')\n    for oy in (-1, 0, 1):\n        for ox in (-1, 0, 1):\n            gx = cx + ox; gy = cy + oy\n            px = gx + _ecky_hash01(gx, gy, seed)\n            py = gy + _ecky_hash01(gx + 19.19, gy + 7.73, float(seed) + 31.0)\n            best = min(best, math.hypot(float(x) - px, float(y) - py))\n    return max(0.0, min(1.0, best / math.sqrt(2.0)))".into(),
        "def _ecky_voronoi2(x, y, seed):\n    return max(0.0, min(1.0, 1.0 - _ecky_cell_distance2(x, y, seed)))".into(),
        "def _ecky_signed_pow(value, exponent):\n    value = float(value); exponent = float(exponent)\n    return math.copysign(abs(value) ** exponent, value)".into(),
        "def _ecky_helical_ridge(radius, pitch, height, base_width, crest_width, depth, female=False, clearance=0.0, lefthand=False):\n    radius = float(radius); pitch = float(pitch); height = float(height)\n    base_width = float(base_width); crest_width = float(crest_width); depth = float(depth)\n    clearance = max(0.0, float(clearance))\n    female = bool(female); lefthand = bool(lefthand)\n    if radius <= 0.0: raise ValueError('helical-ridge radius must be positive')\n    if pitch <= 0.0: raise ValueError('helical-ridge pitch must be positive')\n    if height <= 0.0: raise ValueError('helical-ridge height must be positive')\n    if base_width <= 0.0: raise ValueError('helical-ridge base-width must be positive')\n    if crest_width <= 0.0: raise ValueError('helical-ridge crest-width must be positive')\n    if depth <= 0.0: raise ValueError('helical-ridge depth must be positive')\n    envelope_clearance = clearance if female else 0.0\n    path_radius = radius\n    base_half = (base_width + 2.0 * envelope_clearance) * 0.5\n    crest_half = (crest_width + 2.0 * envelope_clearance) * 0.5\n    ridge_depth = depth + envelope_clearance\n    path = Edge.make_helix(pitch=pitch, height=height, radius=path_radius, center=(0, 0, 0), normal=(0, 0, 1), lefthand=lefthand)\n    profile = Polyline((path_radius, 0, -base_half), (path_radius + ridge_depth, 0, -crest_half), (path_radius + ridge_depth, 0, crest_half), (path_radius, 0, base_half), close=True)\n    return _ecky_solid(sweep(_ecky_face(profile), path=path, is_frenet=True))".into(),
        "def _ecky_thread(radius, pitch, length, depth, base_width, crest_width, female=False, clearance=0.0, lefthand=False):\n    radius = float(radius); depth = float(depth)\n    overlap = min(0.3, radius * 0.5, depth)\n    ridge = _ecky_helical_ridge(radius - overlap, pitch, length, base_width, crest_width, depth + overlap, female=female, clearance=clearance, lefthand=lefthand)\n    if bool(female):\n        return ridge\n    core = Cylinder(radius, float(length), align=(Align.CENTER, Align.CENTER, Align.MIN))\n    return _ecky_solid(core + ridge)".into(),
        "def _ecky_draft(solid, angle, neutral_z=0.0):\n    solid = _ecky_solid(solid)\n    faces = [f for f in solid.faces() if abs(float(f.normal_at().Z)) < 1.0e-6]\n    if not faces:\n        return solid\n    plane = Plane(origin=(0, 0, float(neutral_z)), z_dir=(0, 0, 1))\n    return _ecky_solid(draft(faces, neutral_plane=plane, angle=float(angle)))".into(),
        "def _ecky_regular_polygon(sides, radius, rotation=0.0):\n    sides = int(round(float(sides))); radius = float(radius); rot = math.radians(float(rotation))\n    if sides < 3: raise ValueError('regular-polygon needs at least 3 sides')\n    if radius <= 0.0: raise ValueError('regular-polygon radius must be positive')\n    pts = [(radius * math.cos(rot + 2.0 * math.pi * i / sides), radius * math.sin(rot + 2.0 * math.pi * i / sides)) for i in range(sides)]\n    return Polygon(*pts, align=None)".into(),
        "def _ecky_trapezoid(bottom, top, height, skew=0.0):\n    bottom = float(bottom); top = float(top); height = float(height); skew = float(skew)\n    if bottom <= 0.0 or top <= 0.0: raise ValueError('trapezoid bottom and top must be positive')\n    if height <= 0.0: raise ValueError('trapezoid height must be positive')\n    half_h = height / 2.0\n    pts = [(-bottom / 2.0, -half_h), (bottom / 2.0, -half_h), (top / 2.0 + skew, half_h), (-top / 2.0 + skew, half_h)]\n    return Polygon(*pts, align=None)".into(),
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
    param_defaults: &'a BTreeMap<String, ParamValue>,
    lin: Linearizer,
    imp_counter: usize,
    local_name_counts: BTreeMap<String, usize>,
}

impl<'a> ExprLowerer<'a> {
    fn new(param_defaults: &'a BTreeMap<String, ParamValue>) -> Self {
        Self {
            param_defaults,
            lin: Linearizer::new(),
            imp_counter: 0,
            local_name_counts: BTreeMap::new(),
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
            param_defaults: self.param_defaults,
            lin: Linearizer::new(),
            imp_counter: self.imp_counter,
            local_name_counts: self.local_name_counts.clone(),
        };
        let node = nested.lower_geom_expr(value, scope)?;
        let result_var = nested.lin.linearize(&node.expr);
        self.imp_counter = nested.imp_counter;
        self.local_name_counts = nested.local_name_counts;
        Ok((nested.lin.lines, result_var, node.kind))
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_core_value_locally(
        &mut self,
        node: &CoreNode,
        hint: Option<CoreValueKind>,
        param_names: &BTreeMap<u64, String>,
        refs: &BTreeMap<u64, String>,
        locals: &BTreeMap<String, String>,
        used_local_names: &BTreeMap<String, usize>,
        scope: &LoweringScope<'_>,
    ) -> AppResult<(Vec<String>, LoweredBinding)> {
        let mut nested = ExprLowerer {
            param_defaults: self.param_defaults,
            lin: Linearizer::new(),
            imp_counter: self.imp_counter,
            local_name_counts: self.local_name_counts.clone(),
        };
        let mut child_refs = refs.clone();
        let mut child_locals = locals.clone();
        let mut child_used_local_names = used_local_names.clone();
        let binding = nested.lower_core_value_hinted(
            node,
            hint,
            param_names,
            &mut child_refs,
            &mut child_locals,
            &mut child_used_local_names,
            scope,
        )?;
        self.imp_counter = nested.imp_counter;
        self.local_name_counts = nested.local_name_counts;
        Ok((nested.lin.lines, binding))
    }

    fn lower_core_list_value(
        &self,
        node: &CoreNode,
        param_names: &BTreeMap<u64, String>,
        refs: &BTreeMap<u64, String>,
        locals: &BTreeMap<String, String>,
        used_local_names: &mut BTreeMap<String, usize>,
    ) -> AppResult<Value> {
        match &node.kind {
            CoreNodeKind::Literal(CoreLiteral::Number(number)) => Ok(Value::number(*number)),
            CoreNodeKind::Literal(CoreLiteral::Boolean(flag)) => Ok(Value::boolean(*flag)),
            CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(Value::string(text.clone())),
            CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => {
                Ok(Value::symbol(core_symbol_name(symbol)))
            }
            CoreNodeKind::Literal(CoreLiteral::Point2([x, y])) => {
                Ok(Value::list(vec![Value::number(*x), Value::number(*y)]))
            }
            CoreNodeKind::Literal(CoreLiteral::Point3([x, y, z])) => Ok(Value::list(vec![
                Value::number(*x),
                Value::number(*y),
                Value::number(*z),
            ])),
            CoreNodeKind::Reference(CoreReference::Local(name)) => Ok(Value::symbol(
                locals.get(name).cloned().unwrap_or_else(|| name.clone()),
            )),
            CoreNodeKind::Reference(CoreReference::Node(id)) => refs
                .get(&id.raw())
                .cloned()
                .map(Value::symbol)
                .ok_or_else(|| {
                    unsupported(format!(
                        "Unsupported Core node reference {:?} in list value.",
                        id
                    ))
                }),
            CoreNodeKind::Reference(CoreReference::Parameter(id)) => param_names
                .get(&id.raw())
                .cloned()
                .map(Value::symbol)
                .ok_or_else(|| {
                    unsupported(format!("Unsupported Core parameter reference {:?}.", id))
                }),
            CoreNodeKind::List(items) => Ok(Value::list(
                items
                    .iter()
                    .map(|item| {
                        self.lower_core_list_value(
                            item,
                            param_names,
                            refs,
                            locals,
                            used_local_names,
                        )
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            )),
            _ => self.lower_core_node_to_value(node, param_names, refs, locals, used_local_names),
        }
    }

    fn lower_parts(
        &mut self,
        parts: &[(String, Value, Option<CoreValueKind>)],
    ) -> AppResult<String> {
        let scope = LoweringScope::new(self.param_defaults);
        let mut part_entries: Vec<String> = Vec::new();
        for (part_id, expr, value_kind) in parts {
            let part_scope = scope.with_part_id(part_id);
            let node = self.lower_geom_expr_hinted(expr, &part_scope, *value_kind)?;
            let var = self.lin.linearize(&node.expr);
            part_entries.push(format!("({:?}, {})", part_id, var));
        }

        Ok(serialize_b123d_program(
            std::mem::take(&mut self.lin.lines),
            part_entries,
        ))
    }

    fn lower_core_parts(
        &mut self,
        parts: &[crate::ecky_core_ir::CorePart],
        param_names: &BTreeMap<u64, String>,
    ) -> AppResult<String> {
        let scope = LoweringScope::new(self.param_defaults);
        let mut part_entries: Vec<String> = Vec::new();
        for part in parts {
            let part_scope = scope.with_part_id(&part.key);
            let mut refs = BTreeMap::new();
            let mut locals = BTreeMap::new();
            let mut used_local_names = BTreeMap::new();
            let node = self.lower_core_geom_node(
                &part.root,
                param_names,
                &mut refs,
                &mut locals,
                &mut used_local_names,
                &part_scope,
            )?;
            let var = self.lin.linearize(&node.expr);
            part_entries.push(format!("({:?}, {})", part.key, var));
        }

        Ok(serialize_b123d_program(
            std::mem::take(&mut self.lin.lines),
            part_entries,
        ))
    }

    fn lower_core_geom_node(
        &mut self,
        node: &CoreNode,
        param_names: &BTreeMap<u64, String>,
        refs: &mut BTreeMap<u64, String>,
        locals: &mut BTreeMap<String, String>,
        used_local_names: &mut BTreeMap<String, usize>,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredNode> {
        match self.lower_core_value_hinted(
            node,
            Some(node.value_kind),
            param_names,
            refs,
            locals,
            used_local_names,
            scope,
        )? {
            LoweredBinding::Geom(geom) => Ok(LoweredNode {
                expr: PyExpr::Var(geom.var),
                kind: geom.kind,
            }),
            other => Err(unsupported(format!(
                "Core node expected geometry but resolved to {}.",
                binding_kind_noun(&other)
            ))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_core_value_hinted(
        &mut self,
        node: &CoreNode,
        hint: Option<CoreValueKind>,
        param_names: &BTreeMap<u64, String>,
        refs: &mut BTreeMap<u64, String>,
        locals: &mut BTreeMap<String, String>,
        used_local_names: &mut BTreeMap<String, usize>,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredBinding> {
        match &node.kind {
            CoreNodeKind::Literal(CoreLiteral::Number(number)) => {
                let value = Value::number(*number);
                self.lower_binding_value_hinted(&value, scope, hint)
            }
            CoreNodeKind::Literal(CoreLiteral::Boolean(flag)) => {
                let value = Value::boolean(*flag);
                self.lower_binding_value_hinted(&value, scope, hint)
            }
            CoreNodeKind::Literal(CoreLiteral::Text(text)) => {
                let value = Value::string(text.clone());
                self.lower_binding_value_hinted(&value, scope, hint)
            }
            CoreNodeKind::Reference(CoreReference::Local(name)) => {
                let value =
                    Value::symbol(locals.get(name).cloned().unwrap_or_else(|| name.clone()));
                self.lower_binding_value_hinted(&value, scope, hint)
            }
            CoreNodeKind::Reference(CoreReference::Node(id)) => {
                let name = refs.get(&id.raw()).cloned().ok_or_else(|| {
                    unsupported(format!(
                        "Unsupported Core node reference {:?} in hinted value.",
                        id
                    ))
                })?;
                let value = Value::symbol(name);
                self.lower_binding_value_hinted(&value, scope, hint)
            }
            CoreNodeKind::Reference(CoreReference::Parameter(id)) => {
                let name = param_names.get(&id.raw()).cloned().ok_or_else(|| {
                    unsupported(format!("Unsupported Core parameter reference {:?}.", id))
                })?;
                let value = Value::symbol(name);
                self.lower_binding_value_hinted(&value, scope, hint)
            }
            CoreNodeKind::List(items) => {
                let value = Value::list(
                    items
                        .iter()
                        .map(|item| {
                            self.lower_core_list_value(
                                item,
                                param_names,
                                refs,
                                locals,
                                used_local_names,
                            )
                        })
                        .collect::<AppResult<Vec<_>>>()?,
                );
                self.lower_binding_value_hinted(&value, scope, hint)
            }
            CoreNodeKind::If {
                condition,
                then_branch,
                else_branch,
            } if !matches!(
                hint.or(Some(node.value_kind)),
                Some(
                    CoreValueKind::Any
                        | CoreValueKind::List
                        | CoreValueKind::Point2
                        | CoreValueKind::Point3
                ) | None
            ) =>
            {
                let (cond_lines, cond_binding) = self.lower_core_value_locally(
                    condition,
                    Some(CoreValueKind::Boolean),
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                    scope,
                )?;
                let cond_expr = match cond_binding {
                    LoweredBinding::Boolean(expr) => expr,
                    other => {
                        return Err(unsupported(format!(
                            "Core `if` condition resolved to {} instead of boolean.",
                            binding_kind_noun(&other)
                        )))
                    }
                };
                let result_hint = hint.or(Some(node.value_kind));
                let (then_lines, then_binding) = self.lower_core_value_locally(
                    then_branch,
                    result_hint.or(Some(then_branch.value_kind)),
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                    scope,
                )?;
                let (else_lines, else_binding) = self.lower_core_value_locally(
                    else_branch,
                    result_hint.or(Some(else_branch.value_kind)),
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                    scope,
                )?;
                let mut lines = cond_lines;
                lines.push(format!("if {cond_expr}:"));
                match (then_binding, else_binding) {
                    (LoweredBinding::Geom(then_geom), LoweredBinding::Geom(else_geom)) => {
                        if then_geom.kind != else_geom.kind {
                            return Err(unsupported(format!(
                                "Node `if` requires matching branch kinds, got {} and {}.",
                                then_geom.kind.noun(),
                                else_geom.kind.noun()
                            )));
                        }
                        let result = self.next_imp_var();
                        lines.extend(then_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {}", then_geom.var));
                        lines.push("else:".to_string());
                        lines.extend(else_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {}", else_geom.var));
                        Ok(LoweredBinding::Geom(LoweredGeom {
                            var: self.lin.linearize(&PyExpr::Imperative {
                                lines,
                                result_var: result.clone(),
                            }),
                            kind: then_geom.kind,
                        }))
                    }
                    (LoweredBinding::Number(then_expr), LoweredBinding::Number(else_expr)) => {
                        let result = self.next_python_binding_ident("_if_value");
                        lines.extend(then_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {then_expr}"));
                        lines.push("else:".to_string());
                        lines.extend(else_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {else_expr}"));
                        for line in lines {
                            self.lin.emit(line);
                        }
                        Ok(LoweredBinding::Number(result))
                    }
                    (LoweredBinding::Boolean(then_expr), LoweredBinding::Boolean(else_expr)) => {
                        let result = self.next_python_binding_ident("_if_value");
                        lines.extend(then_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {then_expr}"));
                        lines.push("else:".to_string());
                        lines.extend(else_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {else_expr}"));
                        for line in lines {
                            self.lin.emit(line);
                        }
                        Ok(LoweredBinding::Boolean(result))
                    }
                    (
                        LoweredBinding::Stringish(then_expr),
                        LoweredBinding::Stringish(else_expr),
                    ) => {
                        let result = self.next_python_binding_ident("_if_value");
                        lines.extend(then_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {then_expr}"));
                        lines.push("else:".to_string());
                        lines.extend(else_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {else_expr}"));
                        for line in lines {
                            self.lin.emit(line);
                        }
                        Ok(LoweredBinding::Stringish(result))
                    }
                    (LoweredBinding::Frame(then_expr), LoweredBinding::Frame(else_expr)) => {
                        let result = self.next_python_binding_ident("_if_value");
                        lines.extend(then_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {then_expr}"));
                        lines.push("else:".to_string());
                        lines.extend(else_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {else_expr}"));
                        for line in lines {
                            self.lin.emit(line);
                        }
                        Ok(LoweredBinding::Frame(result))
                    }
                    (
                        LoweredBinding::RuntimeList(then_list),
                        LoweredBinding::RuntimeList(else_list),
                    ) => {
                        if then_list.kind != else_list.kind {
                            return Err(unsupported(format!(
                                "Node `if` requires matching branch kinds, got {} and {}.",
                                then_list.kind.noun(),
                                else_list.kind.noun()
                            )));
                        }
                        let result = self.next_python_binding_ident("_if_value");
                        lines.extend(then_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {}", then_list.var));
                        lines.push("else:".to_string());
                        lines.extend(else_lines.into_iter().map(|line| format!("    {line}")));
                        lines.push(format!("    {result} = {}", else_list.var));
                        for line in lines {
                            self.lin.emit(line);
                        }
                        Ok(LoweredBinding::RuntimeList(LoweredRuntimeList {
                            var: result,
                            kind: then_list.kind,
                        }))
                    }
                    (then_binding, else_binding) => Err(unsupported(format!(
                        "Node `if` requires matching branch kinds, got {} and {}.",
                        binding_kind_noun(&then_binding),
                        binding_kind_noun(&else_binding)
                    ))),
                }
            }
            CoreNodeKind::Build { bindings, result } => {
                let mut child_scope = scope.clone();
                let mut child_refs = refs.clone();
                let mut child_locals = locals.clone();
                let mut child_used_local_names = used_local_names.clone();

                for binding in bindings {
                    let lowered = self.lower_core_value_hinted(
                        &binding.value,
                        Some(binding.value.value_kind),
                        param_names,
                        &mut child_refs,
                        &mut child_locals,
                        &mut child_used_local_names,
                        &child_scope,
                    )?;
                    let ir_name =
                        allocate_legacy_local_name(&binding.name, &mut child_used_local_names);
                    let stored = self.emit_and_store_binding(&ir_name, lowered);
                    let mut frame = BTreeMap::new();
                    frame.insert(ir_name.clone(), stored);
                    child_scope = child_scope.with_frame(frame);
                    child_refs.insert(binding.value.id.raw(), ir_name.clone());
                    child_locals.insert(binding.name.clone(), ir_name);
                }

                self.lower_core_value_hinted(
                    result,
                    hint.or(Some(result.value_kind)),
                    param_names,
                    &mut child_refs,
                    &mut child_locals,
                    &mut child_used_local_names,
                    &child_scope,
                )
            }
            CoreNodeKind::Let { bindings, body } => {
                let mut child_refs = refs.clone();
                let mut child_locals = locals.clone();
                let mut child_used_local_names = used_local_names.clone();
                let ir_binding_names = bindings
                    .iter()
                    .map(|binding| {
                        (
                            binding.name.clone(),
                            allocate_legacy_local_name(&binding.name, &mut child_used_local_names),
                            binding.value.id.raw(),
                        )
                    })
                    .collect::<Vec<_>>();
                let mut frame = BTreeMap::new();

                for (binding, (_, ir_name, node_id)) in bindings.iter().zip(ir_binding_names.iter())
                {
                    let lowered = self.lower_core_value_hinted(
                        &binding.value,
                        Some(binding.value.value_kind),
                        param_names,
                        refs,
                        locals,
                        used_local_names,
                        scope,
                    )?;
                    let stored = self.emit_and_store_binding(ir_name, lowered);
                    frame.insert(ir_name.clone(), stored);
                    child_refs.insert(*node_id, ir_name.clone());
                }
                for (original_name, ir_name, _) in ir_binding_names {
                    child_locals.insert(original_name, ir_name);
                }
                let child_scope = scope.with_frame(frame);
                self.lower_core_value_hinted(
                    body,
                    hint.or(Some(body.value_kind)),
                    param_names,
                    &mut child_refs,
                    &mut child_locals,
                    &mut child_used_local_names,
                    &child_scope,
                )
            }
            CoreNodeKind::Group(items) => {
                let (last, prefix) = items
                    .split_last()
                    .ok_or_else(|| validation("Core group sequence cannot be empty."))?;
                for item in prefix {
                    let _ = self.lower_core_value_hinted(
                        item,
                        Some(item.value_kind),
                        param_names,
                        refs,
                        locals,
                        used_local_names,
                        scope,
                    )?;
                }
                self.lower_core_value_hinted(
                    last,
                    hint.or(Some(last.value_kind)),
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                    scope,
                )
            }
            _ => {
                let value = self.lower_core_node_to_value(
                    node,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?;
                self.lower_binding_value_hinted(&value, scope, hint)
            }
        }
    }

    fn lower_core_node_to_value(
        &self,
        node: &CoreNode,
        param_names: &BTreeMap<u64, String>,
        refs: &BTreeMap<u64, String>,
        locals: &BTreeMap<String, String>,
        used_local_names: &mut BTreeMap<String, usize>,
    ) -> AppResult<Value> {
        match &node.kind {
            CoreNodeKind::Literal(CoreLiteral::Number(number)) => Ok(Value::number(*number)),
            CoreNodeKind::Literal(CoreLiteral::Boolean(flag)) => Ok(Value::boolean(*flag)),
            CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(Value::string(text.clone())),
            CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => {
                Ok(Value::symbol(core_symbol_name(symbol)))
            }
            CoreNodeKind::Literal(CoreLiteral::Point2([x, y])) => {
                Ok(Value::list(vec![Value::number(*x), Value::number(*y)]))
            }
            CoreNodeKind::Literal(CoreLiteral::Point3([x, y, z])) => Ok(Value::list(vec![
                Value::number(*x),
                Value::number(*y),
                Value::number(*z),
            ])),
            CoreNodeKind::Reference(CoreReference::Local(name)) => Ok(Value::symbol(
                locals.get(name).cloned().unwrap_or_else(|| name.clone()),
            )),
            CoreNodeKind::Reference(CoreReference::Node(id)) => refs
                .get(&id.raw())
                .cloned()
                .map(Value::symbol)
                .ok_or_else(|| {
                    unsupported(format!(
                        "Unsupported Core node reference {:?} in value bridge.",
                        id
                    ))
                }),
            CoreNodeKind::Reference(CoreReference::Parameter(id)) => param_names
                .get(&id.raw())
                .cloned()
                .map(Value::symbol)
                .ok_or_else(|| {
                    unsupported(format!("Unsupported Core parameter reference {:?}.", id))
                }),
            CoreNodeKind::Reference(other) => Err(unsupported(format!(
                "Unsupported Core IR reference in build123d lowerer: {:?}.",
                other
            ))),
            CoreNodeKind::Build { bindings, result } => {
                let mut nested_refs = refs.clone();
                let mut nested_locals = locals.clone();
                let mut items = vec![Value::symbol("build")];
                let ir_binding_names = bindings
                    .iter()
                    .map(|binding: &CoreShapeBinding| {
                        (
                            binding.name.clone(),
                            allocate_legacy_local_name(&binding.name, used_local_names),
                            binding.value.id.raw(),
                        )
                    })
                    .collect::<Vec<_>>();
                for (binding, (original_name, ir_name, node_id)) in
                    bindings.iter().zip(ir_binding_names.iter())
                {
                    let mut shape_items = vec![
                        Value::symbol("shape"),
                        Value::symbol(ir_name.clone()),
                        self.lower_core_node_to_value(
                            &binding.value,
                            param_names,
                            &nested_refs,
                            &nested_locals,
                            used_local_names,
                        )?,
                    ];
                    if binding.value.value_kind != CoreValueKind::Any {
                        shape_items.push(Value::keyword("value-kind"));
                        shape_items.push(Value::symbol(core_value_kind_tag_local(
                            binding.value.value_kind,
                        )));
                    }
                    items.push(Value::list(shape_items));
                    nested_refs.insert(*node_id, ir_name.clone());
                    nested_locals.insert(original_name.clone(), ir_name.clone());
                }
                items.push(Value::list(vec![
                    Value::symbol("result"),
                    self.lower_core_node_to_value(
                        result,
                        param_names,
                        &nested_refs,
                        &nested_locals,
                        used_local_names,
                    )?,
                ]));
                Ok(Value::list(items))
            }
            CoreNodeKind::Let { bindings, body } => {
                let mut nested_refs = refs.clone();
                let mut nested_locals = locals.clone();
                let ir_binding_names = bindings
                    .iter()
                    .map(|binding| {
                        (
                            binding.name.clone(),
                            allocate_legacy_local_name(&binding.name, used_local_names),
                            binding.value.id.raw(),
                        )
                    })
                    .collect::<Vec<_>>();
                let binding_values = bindings
                    .iter()
                    .zip(ir_binding_names.iter())
                    .map(|(binding, (_, ir_name, node_id))| {
                        nested_refs.insert(*node_id, ir_name.clone());
                        let mut pair = vec![
                            Value::symbol(ir_name.clone()),
                            self.lower_core_node_to_value(
                                &binding.value,
                                param_names,
                                refs,
                                locals,
                                used_local_names,
                            )?,
                        ];
                        if binding.value.value_kind != CoreValueKind::Any {
                            pair.push(Value::keyword("value-kind"));
                            pair.push(Value::symbol(core_value_kind_tag_local(
                                binding.value.value_kind,
                            )));
                        }
                        Ok(Value::list(pair))
                    })
                    .collect::<AppResult<Vec<_>>>()?;
                for (original_name, ir_name, _) in ir_binding_names {
                    nested_locals.insert(original_name, ir_name);
                }
                Ok(Value::list(vec![
                    Value::symbol("let"),
                    Value::list(binding_values),
                    self.lower_core_node_to_value(
                        body,
                        param_names,
                        &nested_refs,
                        &nested_locals,
                        used_local_names,
                    )?,
                ]))
            }
            CoreNodeKind::If {
                condition,
                then_branch,
                else_branch,
            } => Ok(Value::list(vec![
                Value::symbol("if"),
                self.lower_core_node_to_value(
                    condition,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?,
                self.lower_core_node_to_value(
                    then_branch,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?,
                self.lower_core_node_to_value(
                    else_branch,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?,
            ])),
            CoreNodeKind::Call { op, args, keywords } => {
                let mut items = vec![Value::symbol(core_operation_name_local(op))];
                for arg in args {
                    items.push(self.lower_core_node_to_value(
                        arg,
                        param_names,
                        refs,
                        locals,
                        used_local_names,
                    )?);
                }
                for keyword in keywords {
                    items.push(Value::keyword(keyword.name.clone()));
                    items.push(match (keyword.name.as_str(), keyword.selector_payload()) {
                        ("created-by", None) => {
                            return Err(validation(format!(
                                "`{}` does not recognize option `:created-by`.",
                                core_operation_name_local(op)
                            )))
                        }
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
                        (_, Some(selector)) => core_selector_payload_to_ir_value(selector)?,
                        (_, None) => self.lower_core_node_to_value(
                            keyword.source_node(),
                            param_names,
                            refs,
                            locals,
                            used_local_names,
                        )?,
                    });
                }
                materialize_selector_nodes(Value::list(items))
            }
            CoreNodeKind::Range { start, end } => Ok(Value::list(vec![
                Value::symbol("range"),
                self.lower_core_node_to_value(start, param_names, refs, locals, used_local_names)?,
                self.lower_core_node_to_value(end, param_names, refs, locals, used_local_names)?,
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
                    ir_params.push(Value::symbol(ir_name));
                }
                let mut items = vec![
                    Value::symbol("map"),
                    Value::list(vec![
                        Value::symbol("lambda"),
                        Value::list(ir_params),
                        self.lower_core_node_to_value(
                            body,
                            param_names,
                            refs,
                            &nested_locals,
                            used_local_names,
                        )?,
                    ]),
                ];
                for source in sources {
                    items.push(self.lower_core_node_to_value(
                        source,
                        param_names,
                        refs,
                        locals,
                        used_local_names,
                    )?);
                }
                Ok(Value::list(items))
            }
            CoreNodeKind::Apply { op, args, list } => {
                let mut items = vec![
                    Value::symbol("apply"),
                    Value::symbol(core_operation_name_local(op)),
                ];
                for arg in args {
                    items.push(self.lower_core_node_to_value(
                        arg,
                        param_names,
                        refs,
                        locals,
                        used_local_names,
                    )?);
                }
                items.push(self.lower_core_node_to_value(
                    list,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
                Ok(Value::list(items))
            }
            CoreNodeKind::List(items) | CoreNodeKind::Group(items) => Ok(Value::list(
                items
                    .iter()
                    .map(|item| {
                        self.lower_core_node_to_value(
                            item,
                            param_names,
                            refs,
                            locals,
                            used_local_names,
                        )
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            )),
        }
    }

    fn emit_and_store_binding(&mut self, name: &str, binding: LoweredBinding) -> LoweredBinding {
        let local_name = self.next_python_binding_ident(name);
        match &binding {
            LoweredBinding::Geom(geom) => self.lin.emit(format!("{local_name} = {}", geom.var)),
            LoweredBinding::List(_) | LoweredBinding::RuntimeList(_) => {}
            LoweredBinding::Frame(expr)
            | LoweredBinding::Number(expr)
            | LoweredBinding::Boolean(expr)
            | LoweredBinding::Stringish(expr) => {
                self.lin.emit(format!("{local_name} = {expr}"));
            }
        }
        match binding {
            LoweredBinding::Geom(mut geom) => {
                geom.var = local_name;
                LoweredBinding::Geom(geom)
            }
            LoweredBinding::List(list) => LoweredBinding::List(list),
            LoweredBinding::RuntimeList(list) => LoweredBinding::RuntimeList(list),
            LoweredBinding::Frame(_) => LoweredBinding::Frame(local_name),
            LoweredBinding::Number(_) => LoweredBinding::Number(local_name),
            LoweredBinding::Boolean(_) => LoweredBinding::Boolean(local_name),
            LoweredBinding::Stringish(_) => LoweredBinding::Stringish(local_name),
        }
    }

    fn next_python_binding_ident(&mut self, name: &str) -> String {
        let base = python_local_ident(name, "_");
        let slot = self.local_name_counts.entry(base.clone()).or_insert(0);
        *slot += 1;
        if *slot == 1 {
            base
        } else {
            format!("{}_{}", base, *slot)
        }
    }

    fn lower_geom_expr_hinted(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
        hint: Option<CoreValueKind>,
    ) -> AppResult<LoweredNode> {
        if matches!(
            hint,
            Some(
                CoreValueKind::Solid
                    | CoreValueKind::Sketch
                    | CoreValueKind::Compound
                    | CoreValueKind::Path
            )
        ) {
            return self.lower_legacy_group_sequence_or_geom(value, scope);
        }
        self.lower_geom_expr(value, scope)
    }

    fn lower_legacy_group_sequence_or_geom(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredNode> {
        let Some(items) = value.as_list() else {
            return self.lower_geom_expr(value, scope);
        };
        if items.is_empty() || items.first().and_then(Value::as_symbol).is_some() {
            return self.lower_geom_expr(value, scope);
        }

        let (last, prefix) = items
            .split_last()
            .ok_or_else(|| validation("Geometry group sequence cannot be empty."))?;
        for item in prefix {
            let _ = self.lower_legacy_group_sequence_or_geom(item, scope)?;
        }
        self.lower_legacy_group_sequence_or_geom(last, scope)
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

        if matches!(
            head_symbol(&items, "list expression").ok(),
            Some("let" | "let*")
        ) {
            if items.len() != 3 {
                return Err(validation("List `let`/`let*` expects bindings and a body."));
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
        _scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredList> {
        if list.kind == expected_kind {
            return Ok(list);
        }
        if expected_kind == LoweredListKind::Point2d && list.kind == LoweredListKind::Pair {
            return Ok(LoweredList::new(
                list.items,
                LoweredListKind::Point2d,
                list.source_op,
            ));
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
                    scope,
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
            return self.require_list_kind(list, cad_op, expected_kind, None, scope);
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

    fn lower_runtime_list_expr(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredRuntimeList> {
        if let Some(sym) = value.as_symbol() {
            return match scope.resolve_binding(sym) {
                Some(LoweredBinding::RuntimeList(list)) => Ok(list.clone()),
                Some(binding) => Err(unsupported(format!(
                    "Symbol `{}` is a {} and cannot be used as a runtime list.",
                    sym,
                    binding_kind_noun(binding)
                ))),
                None => Err(validation(format!("Unknown symbol `{}`.", sym))),
            };
        }

        let items = value
            .to_vec()
            .ok_or_else(|| validation("Expected a proper list for runtime list expression."))?;
        let node = head_symbol(&items, "runtime list expression")?;
        let args = &items[1..];
        match node {
            "range" => {
                let (start, end) = match args {
                    [end] => ("0.0".to_string(), lower_num_expr(end, scope)?),
                    [start, end] => (lower_num_expr(start, scope)?, lower_num_expr(end, scope)?),
                    _ => return Err(validation("`range` expects one or two bounds.")),
                };
                let result = self.next_imp_var();
                self.lin.emit(format!(
                    "{result} = list(range(int(math.floor({start})), int(math.floor({end}))))"
                ));
                Ok(LoweredRuntimeList {
                    var: result,
                    kind: RuntimeListKind::Number,
                })
            }
            "map" => self.lower_runtime_map_list(args, scope),
            "let" | "let*" => {
                if args.len() != 2 {
                    return Err(validation(
                        "Runtime list `let`/`let*` expects bindings and body.",
                    ));
                }
                let child_scope = lower_scalar_let_scope(&args[0], scope)?;
                self.lower_runtime_list_expr(&args[1], &child_scope)
            }
            other => Err(unsupported(format!(
                "Runtime list expression `{}` is not supported by the build123d lowerer.",
                other
            ))),
        }
    }

    fn lower_runtime_map_list(
        &mut self,
        args: &[Value],
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredRuntimeList> {
        if args.len() < 2 {
            return Err(validation(
                "`map` expects function and at least one source list.",
            ));
        }
        let (params, body) = parse_lambda_expr(&args[0])?;
        if params.len() != args.len() - 1 {
            return Err(validation(format!(
                "`map` lambda expects {} source list(s), got {}.",
                params.len(),
                args.len() - 1
            )));
        }
        let sources = args[1..]
            .iter()
            .map(|source| self.lower_runtime_list_expr(source, scope))
            .collect::<AppResult<Vec<_>>>()?;
        for source in &sources {
            if source.kind != RuntimeListKind::Number {
                return Err(unsupported(format!(
                    "`map` currently supports numeric runtime source lists, got {}.",
                    source.kind.noun()
                )));
            }
        }

        let result = self.next_imp_var();
        let mut frame = BTreeMap::new();
        let mut lines = vec![format!("{result} = []")];
        if params.len() == 1 {
            let loop_var = python_local_ident(&params[0], "__ecky_map_");
            let local_name = python_local_ident(&params[0], "_");
            frame.insert(
                params[0].clone(),
                LoweredBinding::Number(local_name.clone()),
            );
            lines.push(format!("for {loop_var} in {}:", sources[0].var));
            lines.push(format!("    {local_name} = float({loop_var})"));
        } else {
            let tuple_var = self.next_imp_var();
            let source_vars = sources
                .iter()
                .map(|source| source.var.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("for {tuple_var} in zip({source_vars}):"));
            for (index, param) in params.iter().enumerate() {
                let local_name = python_local_ident(param, "_");
                frame.insert(param.clone(), LoweredBinding::Number(local_name.clone()));
                lines.push(format!("    {local_name} = float({tuple_var}[{index}])"));
            }
        }
        let child_scope = scope.with_frame(frame);
        let (body_lines, body_result, body_kind) =
            self.lower_geom_expr_locally(&body, &child_scope)?;
        lines.extend(body_lines.into_iter().map(|line| format!("    {line}")));
        if body_kind == B123dGeomKind::Solid3d {
            lines.push(format!(
                "    {result}.extend(_ecky_collect_solids({body_result}))"
            ));
        } else {
            lines.push(format!("    {result}.append({body_result})"));
        }
        for line in lines {
            self.lin.emit(line);
        }
        Ok(LoweredRuntimeList {
            var: result,
            kind: RuntimeListKind::Geom(body_kind),
        })
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
        let target_node = head_symbol(target_items, "shell target")?;
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
            "sampled-radial-loft" => {
                let call = parse_sampled_radial_loft_call(target_args)?;
                let mut inner_args = vec![target_args[0].dup()];
                inner_args.push(Value::keyword("height"));
                inner_args.push(call.height.dup());
                inner_args.push(Value::keyword("z-steps"));
                inner_args.push(call.z_steps.dup());
                inner_args.push(Value::keyword("theta-steps"));
                inner_args.push(call.theta_steps.dup());
                inner_args.push(Value::keyword("radius"));
                inner_args.push(self.shell_subtract_wall(&call.radius, wall));
                if let Some(z_map) = call.z_map {
                    inner_args.push(Value::keyword("z-map"));
                    inner_args.push(z_map);
                }
                ShellLoweringPlan::BooleanInner(
                    self.shell_target_value("sampled-radial-loft", inner_args),
                )
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
        let node = head_symbol(items, "frame expression")?;
        let args = &items[1..];
        match node {
            "plane" => {
                let call = parse_plane_call(args)?;
                let origin = if let Some(value) = call.origin {
                    let (x, y, z) = self.lower_vec3(&value, scope, "`plane :origin`")?;
                    format!("({x}, {y}, {z})")
                } else {
                    "(0.0, 0.0, 0.0)".to_string()
                };
                let x_dir = if let Some(value) = call.x {
                    let (x, y, z) = self.lower_vec3(&value, scope, "`plane :x`")?;
                    format!("({x}, {y}, {z})")
                } else {
                    "(1.0, 0.0, 0.0)".to_string()
                };
                let z_dir = if let Some(value) = call.normal {
                    let (x, y, z) = self.lower_vec3(&value, scope, "`plane :normal`")?;
                    format!("({x}, {y}, {z})")
                } else {
                    "(0.0, 0.0, 1.0)".to_string()
                };
                Ok(format!(
                    "Plane(origin={origin}, x_dir={x_dir}, z_dir={z_dir})"
                ))
            }
            "location" => {
                let call = parse_location_call(args)?;
                let frame = self.lower_frame_expr(&call.frame, scope)?;
                let offset = if let Some(value) = call.offset {
                    let (x, y, z) = self.lower_vec3(&value, scope, "`location :offset`")?;
                    format!("({x}, {y}, {z})")
                } else {
                    "(0.0, 0.0, 0.0)".to_string()
                };
                let rotate = if let Some(value) = call.rotate {
                    let (x, y, z) = self.lower_vec3(&value, scope, "`location :rotate`")?;
                    format!("({x}, {y}, {z})")
                } else {
                    "(0.0, 0.0, 0.0)".to_string()
                };
                let result = self.next_imp_var();
                self.lin.emit(format!(
                    "{result} = _ecky_location({frame}, {offset}, {rotate})"
                ));
                Ok(result)
            }
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

    fn lower_binding_value_hinted(
        &mut self,
        value: &Value,
        scope: &LoweringScope<'_>,
        hint: Option<CoreValueKind>,
    ) -> AppResult<LoweredBinding> {
        match hint {
            Some(CoreValueKind::Number) => {
                return Ok(LoweredBinding::Number(lower_num_expr(value, scope)?))
            }
            Some(CoreValueKind::Boolean) => {
                return Ok(LoweredBinding::Boolean(lower_bool_expr(value, scope)?))
            }
            Some(CoreValueKind::Text) => {
                return Ok(LoweredBinding::Stringish(lower_stringish_expr(
                    value, scope,
                )?))
            }
            Some(CoreValueKind::Frame) => {
                return Ok(LoweredBinding::Frame(self.lower_frame_expr(value, scope)?))
            }
            Some(
                CoreValueKind::Solid
                | CoreValueKind::Sketch
                | CoreValueKind::Compound
                | CoreValueKind::Path,
            ) => {
                let geom = self.lower_geom_expr_hinted(value, scope, hint)?;
                let var = self.lin.linearize(&geom.expr);
                return Ok(LoweredBinding::Geom(LoweredGeom {
                    var,
                    kind: geom.kind,
                }));
            }
            Some(CoreValueKind::List) => {
                if let Some(list) = self.try_materialize_list_binding(value, scope)? {
                    return Ok(LoweredBinding::List(list));
                }
                if let Ok(list) = self.lower_runtime_list_expr(value, scope) {
                    return Ok(LoweredBinding::RuntimeList(list));
                }
            }
            Some(CoreValueKind::Any | CoreValueKind::Point2 | CoreValueKind::Point3) | None => {}
        }
        self.lower_binding_value(value, scope)
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
        if let Ok(list) = self.lower_runtime_list_expr(value, scope) {
            return Ok(LoweredBinding::RuntimeList(list));
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

    fn lower_apply_geom(
        &mut self,
        args: &[Value],
        scope: &LoweringScope<'_>,
    ) -> AppResult<LoweredNode> {
        if args.len() < 2 {
            return Err(validation(
                "`apply` expects an operation and a runtime list.",
            ));
        }
        let op = args[0]
            .as_symbol()
            .ok_or_else(|| validation("`apply` operation must be a symbol."))?;
        let fixed_args = &args[1..args.len() - 1];
        let runtime_list = self.lower_runtime_list_expr(args.last().expect("apply list"), scope)?;
        let RuntimeListKind::Geom(list_kind) = runtime_list.kind.clone() else {
            return Err(unsupported(format!(
                "`apply {}` expected a geometry list, got {}.",
                op,
                runtime_list.kind.noun()
            )));
        };
        let fixed = self.lower_geom_list(fixed_args, scope)?;
        for operand in &fixed {
            if operand.kind != list_kind {
                return Err(unsupported(format!(
                    "`apply {}` requires matching geometry kinds, got {} and {}.",
                    op,
                    operand.kind.noun(),
                    list_kind.noun()
                )));
            }
        }
        let kind = fixed
            .first()
            .map(|operand| operand.kind.clone())
            .unwrap_or_else(|| list_kind.clone());
        if kind != B123dGeomKind::Solid3d {
            return Err(unsupported(format!(
                "`apply {}` currently supports 3D solids on build123d.",
                op
            )));
        }
        let fixed_vars = fixed
            .iter()
            .map(|node| self.lin.linearize(&node.expr))
            .collect::<Vec<_>>();
        let result = self.next_imp_var();
        let mut call_args = fixed_vars.clone();
        call_args.push(format!("*{}", runtime_list.var));
        match op {
            "union" | "fuse" => {
                let needed = 2usize.saturating_sub(fixed_vars.len());
                if needed > 0 {
                    self.lin.emit(format!(
                        "if len({}) < {}: raise ValueError('apply {} produced too few geometry operands')",
                        runtime_list.var, needed, op
                    ));
                }
                self.lin.emit(format!(
                    "{result} = _ecky_fuse_many({})",
                    call_args.join(", ")
                ));
            }
            "compound" => {
                let needed = 1usize.saturating_sub(fixed_vars.len());
                if needed > 0 {
                    self.lin.emit(format!(
                        "if len({}) < {}: raise ValueError('apply compound produced no geometry')",
                        runtime_list.var, needed
                    ));
                }
                self.lin.emit(format!(
                    "{result} = _ecky_compound({})",
                    call_args.join(", ")
                ));
            }
            "difference" | "cut" => {
                if fixed_vars.is_empty() {
                    return Err(validation(
                        "`apply difference` requires a fixed base operand.",
                    ));
                }
                self.lin.emit(format!(
                    "if not {}: raise ValueError('apply {} produced no cutters')",
                    runtime_list.var, op
                ));
                self.lin.emit(format!(
                    "{result} = _ecky_cut_many({})",
                    call_args.join(", ")
                ));
            }
            "intersection" | "common" => {
                let needed = 2usize.saturating_sub(fixed_vars.len());
                if needed > 0 {
                    self.lin.emit(format!(
                        "if len({}) < {}: raise ValueError('apply {} produced too few geometry operands')",
                        runtime_list.var, needed, op
                    ));
                }
                self.lin.emit(format!(
                    "{result} = _ecky_common_many({})",
                    call_args.join(", ")
                ));
            }
            other => {
                return Err(unsupported(format!(
                    "`apply {}` is not supported by the build123d lowerer.",
                    other
                )))
            }
        }
        Ok(LoweredNode {
            expr: PyExpr::Var(result),
            kind,
        })
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
                Some(LoweredBinding::RuntimeList(list)) => Err(unsupported(format!(
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
                    "`def` is not supported by current `.ecky` runtime. Use lexical `let` for immutable local bindings.",
                ));
            }
            "build" => {
                let build = parse_build_expr(value)?;
                let mut child_scope = scope.clone();
                for binding in &build.bindings {
                    let lowered = self.lower_binding_value_hinted(
                        &binding.expr,
                        &child_scope,
                        binding.value_kind,
                    )?;
                    let stored = self.emit_and_store_binding(&binding.name, lowered);
                    let mut frame = BTreeMap::new();
                    frame.insert(binding.name.clone(), stored);
                    child_scope = child_scope.with_frame(frame);
                }
                return self.lower_legacy_group_sequence_or_geom(&build.result, &child_scope);
            }
            "let" => {
                if args.len() < 2 {
                    return Err(validation("`let` expects bindings and a body."));
                }
                let bindings = list_items(&args[0], "let bindings")?;
                let mut frame = BTreeMap::new();

                for b in bindings {
                    let pair = list_items(b, "binding pair")?;
                    if pair.len() != 2 && pair.len() != 4 {
                        return Err(validation("Each binding must be `(name expr)`."));
                    }
                    let name = pair[0]
                        .as_symbol()
                        .ok_or_else(|| validation("Binding name must be a symbol."))?;
                    let hint = extract_let_binding_hint(pair);
                    let binding = self.lower_binding_value_hinted(&pair[1], scope, hint)?;
                    let local_binding = self.emit_and_store_binding(name, binding);
                    frame.insert(name.to_string(), local_binding);
                }
                let child_scope = scope.with_frame(frame);
                return self.lower_legacy_group_sequence_or_geom(&args[1], &child_scope);
            }
            "let*" => {
                if args.len() < 2 {
                    return Err(validation("`let*` expects bindings and a body."));
                }
                let bindings = list_items(&args[0], "let* bindings")?;
                let mut child_scope = scope.clone();

                for b in bindings {
                    let pair = list_items(b, "binding pair")?;
                    if pair.len() != 2 && pair.len() != 4 {
                        return Err(validation("Each binding must be `(name expr)`."));
                    }
                    let name = pair[0]
                        .as_symbol()
                        .ok_or_else(|| validation("Binding name must be a symbol."))?;
                    let hint = extract_let_binding_hint(pair);
                    let binding = self.lower_binding_value_hinted(&pair[1], &child_scope, hint)?;
                    let local_binding = self.emit_and_store_binding(name, binding);
                    let mut frame = BTreeMap::new();
                    frame.insert(name.to_string(), local_binding);
                    child_scope = child_scope.with_frame(frame);
                }
                return self.lower_legacy_group_sequence_or_geom(&args[1], &child_scope);
            }
            "apply" => {
                return self.lower_apply_geom(args, scope);
            }
            "hole" => return Err(validation(typed_hole_error(args))),
            // -- Primitives (Step 1) --
            "box" => {
                let parsed = ParsedCallArgs::parse("box", args, &["align"])?;
                if parsed.positional.len() != 3 {
                    return Err(validation("`box` expects width, depth, and height."));
                }
                let w = lower_num_expr(&parsed.positional[0], scope)?;
                let d = lower_num_expr(&parsed.positional[1], scope)?;
                let h = lower_num_expr(&parsed.positional[2], scope)?;
                let align = parse_align_tuple(
                    parsed.keywords.get("align"),
                    "box",
                    "(Align.CENTER, Align.CENTER, Align.MIN)",
                )?;
                (
                    PyExpr::Call {
                        func: "Box".into(),
                        args: vec![PyExpr::Inline(w), PyExpr::Inline(d), PyExpr::Inline(h)],
                        kwargs: vec![("align".into(), PyExpr::Inline(align))],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "cylinder" => {
                let parsed = ParsedCallArgs::parse("cylinder", args, &["align"])?;
                if parsed.positional.len() < 2 || parsed.positional.len() > 3 {
                    return Err(validation(
                        "`cylinder` expects radius, height, and optional segments.",
                    ));
                }
                let r = lower_num_expr(&parsed.positional[0], scope)?;
                let h = lower_num_expr(&parsed.positional[1], scope)?;
                let align = parse_align_tuple(
                    parsed.keywords.get("align"),
                    "cylinder",
                    "(Align.CENTER, Align.CENTER, Align.MIN)",
                )?;
                (
                    PyExpr::Call {
                        func: "Cylinder".into(),
                        args: vec![PyExpr::Inline(r), PyExpr::Inline(h)],
                        kwargs: vec![("align".into(), PyExpr::Inline(align))],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "sphere" => {
                let parsed = ParsedCallArgs::parse("sphere", args, &["align"])?;
                if parsed.positional.is_empty() || parsed.positional.len() > 3 {
                    return Err(validation("`sphere` expects radius and optional segments."));
                }
                let r = lower_num_expr(&parsed.positional[0], scope)?;
                let align = parse_align_tuple(
                    parsed.keywords.get("align"),
                    "sphere",
                    "(Align.CENTER, Align.CENTER, Align.CENTER)",
                )?;
                (
                    PyExpr::Call {
                        func: "Sphere".into(),
                        args: vec![PyExpr::Inline(r)],
                        kwargs: vec![("align".into(), PyExpr::Inline(align))],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "cone" => {
                let parsed = ParsedCallArgs::parse("cone", args, &["align"])?;
                if parsed.positional.len() < 3 || parsed.positional.len() > 4 {
                    return Err(validation(
                        "`cone` expects bottom radius, top radius, height, and optional segments.",
                    ));
                }
                let br = lower_num_expr(&parsed.positional[0], scope)?;
                let tr = lower_num_expr(&parsed.positional[1], scope)?;
                let h = lower_num_expr(&parsed.positional[2], scope)?;
                let align = parse_align_tuple(
                    parsed.keywords.get("align"),
                    "cone",
                    "(Align.CENTER, Align.CENTER, Align.MIN)",
                )?;
                (
                    PyExpr::Call {
                        func: "Cone".into(),
                        args: vec![PyExpr::Inline(br), PyExpr::Inline(tr), PyExpr::Inline(h)],
                        kwargs: vec![("align".into(), PyExpr::Inline(align))],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "wedge" => {
                let parsed = ParsedCallArgs::parse("wedge", args, &["align"])?;
                if parsed.positional.len() != 7 {
                    return Err(validation(
                        "`wedge` expects dx, dy, dz, xmin, zmin, xmax, zmax.",
                    ));
                }
                let dims = parsed
                    .positional
                    .iter()
                    .map(|expr| lower_num_expr(expr, scope))
                    .collect::<AppResult<Vec<_>>>()?;
                let align = parse_align_tuple(
                    parsed.keywords.get("align"),
                    "wedge",
                    "(Align.CENTER, Align.CENTER, Align.CENTER)",
                )?;
                (
                    PyExpr::Call {
                        func: "Wedge".into(),
                        args: dims.into_iter().map(PyExpr::Inline).collect(),
                        kwargs: vec![("align".into(), PyExpr::Inline(align))],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "torus" => {
                let parsed = ParsedCallArgs::parse("torus", args, &["align"])?;
                if parsed.positional.len() != 2 {
                    return Err(validation(
                        "`torus` expects major radius and minor radius.",
                    ));
                }
                let major = lower_num_expr(&parsed.positional[0], scope)?;
                let minor = lower_num_expr(&parsed.positional[1], scope)?;
                let align = parse_align_tuple(
                    parsed.keywords.get("align"),
                    "torus",
                    "(Align.CENTER, Align.CENTER, Align.CENTER)",
                )?;
                (
                    PyExpr::Call {
                        func: "Torus".into(),
                        args: vec![PyExpr::Inline(major), PyExpr::Inline(minor)],
                        kwargs: vec![("align".into(), PyExpr::Inline(align))],
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
            "slot-overall" | "slot_overall" => {
                if args.len() != 2 {
                    return Err(validation("`slot-overall` expects length and width."));
                }
                let length = lower_num_expr(&args[0], scope)?;
                let width = lower_num_expr(&args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "SlotOverall".into(),
                        args: vec![PyExpr::Inline(length), PyExpr::Inline(width)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "slot-center-to-center" | "slot_center_to_center" => {
                if args.len() != 2 {
                    return Err(validation(
                        "`slot-center-to-center` expects center separation and width.",
                    ));
                }
                let sep = lower_num_expr(&args[0], scope)?;
                let width = lower_num_expr(&args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "SlotCenterToCenter".into(),
                        args: vec![PyExpr::Inline(sep), PyExpr::Inline(width)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "slot-arc" | "slot_arc" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`slot-arc` expects radius, start angle, end angle, width.",
                    ));
                }
                let radius = lower_num_expr(&args[0], scope)?;
                let start = lower_num_expr(&args[1], scope)?;
                let end = lower_num_expr(&args[2], scope)?;
                let width = lower_num_expr(&args[3], scope)?;
                (
                    PyExpr::Call {
                        func: "SlotArc".into(),
                        args: vec![
                            PyExpr::Inline(format!(
                                "CenterArc((0, 0), {radius}, {start}, ({end}) - ({start}))"
                            )),
                            PyExpr::Inline(width),
                        ],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "slot-center-point" | "slot_center_point" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`slot-center-point` expects cx, cy, px, py, width.",
                    ));
                }
                let cx = lower_num_expr(&args[0], scope)?;
                let cy = lower_num_expr(&args[1], scope)?;
                let px = lower_num_expr(&args[2], scope)?;
                let py = lower_num_expr(&args[3], scope)?;
                let width = lower_num_expr(&args[4], scope)?;
                (
                    PyExpr::Call {
                        func: "SlotCenterPoint".into(),
                        args: vec![
                            PyExpr::Inline(format!("({cx}, {cy})")),
                            PyExpr::Inline(format!("({px}, {py})")),
                            PyExpr::Inline(width),
                        ],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "ellipse" => {
                if args.len() != 2 {
                    return Err(validation("`ellipse` expects x radius and y radius."));
                }
                let rx = lower_num_expr(&args[0], scope)?;
                let ry = lower_num_expr(&args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "Ellipse".into(),
                        args: vec![PyExpr::Inline(rx), PyExpr::Inline(ry)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "rectangle" => {
                if args.len() != 2 {
                    return Err(validation("`rectangle` expects width and height."));
                }
                let w = lower_num_expr(&args[0], scope)?;
                let h = lower_num_expr(&args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "Rectangle".into(),
                        args: vec![PyExpr::Inline(w), PyExpr::Inline(h)],
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
                        func: "_ecky_polygon".into(),
                        args: vec![PyExpr::Inline(points)],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "regular-polygon" | "regular_polygon" => {
                let parsed = ParsedCallArgs::parse("regular-polygon", args, &["rotation"])?;
                if parsed.positional.len() != 2 {
                    return Err(validation(
                        "`regular-polygon` expects sides and radius, plus optional `:rotation`.",
                    ));
                }
                let sides = lower_num_expr(&parsed.positional[0], scope)?;
                let radius = lower_num_expr(&parsed.positional[1], scope)?;
                let rotation = match parsed.keywords.get("rotation") {
                    Some(value) => lower_num_expr(value, scope)?,
                    None => "0.0".to_string(),
                };
                (
                    PyExpr::Call {
                        func: "_ecky_regular_polygon".into(),
                        args: vec![
                            PyExpr::Inline(sides),
                            PyExpr::Inline(radius),
                            PyExpr::Inline(rotation),
                        ],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Sketch2d,
                )
            }
            "trapezoid" => {
                let parsed = ParsedCallArgs::parse("trapezoid", args, &["skew"])?;
                if parsed.positional.len() != 3 {
                    return Err(validation(
                        "`trapezoid` expects bottom, top, and height, plus optional `:skew`.",
                    ));
                }
                let bottom = lower_num_expr(&parsed.positional[0], scope)?;
                let top = lower_num_expr(&parsed.positional[1], scope)?;
                let height = lower_num_expr(&parsed.positional[2], scope)?;
                let skew = match parsed.keywords.get("skew") {
                    Some(value) => lower_num_expr(value, scope)?,
                    None => "0.0".to_string(),
                };
                (
                    PyExpr::Call {
                        func: "_ecky_trapezoid".into(),
                        args: vec![
                            PyExpr::Inline(bottom),
                            PyExpr::Inline(top),
                            PyExpr::Inline(height),
                            PyExpr::Inline(skew),
                        ],
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
                    format!(
                        "if abs(_sx - _sy) < 1e-9 and abs(_sy - _sz) < 1e-9: {result} = {inner_var}.scale(_sx)"
                    ),
                    format!(
                        "else: {result} = _ecky_non_uniform_scale({inner_var}, _sx, _sy, _sz)"
                    ),
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
                let parsed = ParsedCallArgs::parse("extrude", args, &["symmetric"])?;
                if parsed.positional.len() != 2 {
                    return Err(validation("`extrude` expects a sketch and height."));
                }
                let sketch = self.lower_sketch_expr(&parsed.positional[0], scope)?;
                let h = lower_num_expr(&parsed.positional[1], scope)?;
                let symmetric = if let Some(value) = parsed.keywords.get("symmetric") {
                    lower_bool_expr(value, scope)?
                } else {
                    "False".to_string()
                };
                (
                    PyExpr::Call {
                        func: "_ecky_extrude".into(),
                        args: vec![sketch.expr, PyExpr::Inline(h), PyExpr::Inline(symmetric)],
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
                        func: "_ecky_loft".into(),
                        args: vec![PyExpr::Inline(format!("[{}]", sketch_vars.join(", ")))],
                        kwargs: vec![],
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "draft" => {
                let (pos_args, properties) = self.parse_properties(args)?;
                if pos_args.len() != 2 {
                    return Err(validation(
                        "`draft` expects an angle and a solid (plus optional `:neutral-z`).",
                    ));
                }
                let angle = lower_num_expr(&pos_args[0], scope)?;
                let neutral_z = match properties.get("neutral-z").or_else(|| properties.get("neutral_z")) {
                    Some(value) => lower_num_expr(value, scope)?,
                    None => "0.0".to_string(),
                };
                let solid = self.lower_solid_expr(&pos_args[1], scope)?;
                (
                    PyExpr::Call {
                        func: "_ecky_draft".into(),
                        args: vec![solid.expr, PyExpr::Inline(angle)],
                        kwargs: vec![("neutral_z".into(), PyExpr::Inline(neutral_z))],
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
                    format!(
                        "else: {scaled} = Pos(0, 0, {height}) * _ecky_non_uniform_scale({sketch_var}, _tsx, _tsy, 1.0)"
                    ),
                    format!("{result} = _ecky_loft([{bottom}, {scaled}])"),
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
                    format!("{result} = _ecky_loft({sections})"),
                ];
                (
                    PyExpr::Imperative {
                        lines,
                        result_var: result,
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "sampled-radial-loft" => {
                let call = parse_sampled_radial_loft_call(args)?;
                let height = lower_num_expr(&call.height, scope)?;
                let z_steps = self.lower_count(&call.z_steps, scope)?;
                let theta_steps = self.lower_count(&call.theta_steps, scope)?;
                let result = self.next_imp_var();
                let sections = self.next_imp_var();
                let z_steps_var = self.next_imp_var();
                let theta_steps_var = self.next_imp_var();
                let theta_var = self.next_imp_var();
                let z_var = self.next_imp_var();
                let fz_var = self.next_imp_var();
                let section_z_var = self.next_imp_var();
                let radius_var = self.next_imp_var();
                let points_var = self.next_imp_var();
                let zi_var = format!("{result}_zi");
                let ti_var = format!("{result}_ti");
                let mut frame = BTreeMap::new();
                frame.insert(
                    call.binders[0].clone(),
                    LoweredBinding::Number(theta_var.clone()),
                );
                frame.insert(
                    call.binders[1].clone(),
                    LoweredBinding::Number(z_var.clone()),
                );
                frame.insert(
                    call.binders[2].clone(),
                    LoweredBinding::Number(fz_var.clone()),
                );
                let child_scope = scope.with_frame(frame);
                let radius_expr = lower_num_expr(&call.radius, &child_scope)?;
                let z_map_expr = call
                    .z_map
                    .as_ref()
                    .map(|value| lower_num_expr(value, &child_scope))
                    .transpose()?
                    .unwrap_or_else(|| z_var.clone());
                (
                    PyExpr::Imperative {
                        lines: vec![
                            format!("{z_steps_var} = max(1, int(round(float({z_steps}))))"),
                            format!(
                                "{theta_steps_var} = max(3, int(round(float({theta_steps}))))"
                            ),
                            format!("{sections} = []"),
                            format!("for {zi_var} in range({z_steps_var} + 1):"),
                            format!(
                                "    {fz_var} = 0.0 if {z_steps_var} <= 0 else float({zi_var}) / float({z_steps_var})"
                            ),
                            format!("    {z_var} = ({height}) * {fz_var}"),
                            format!("    {section_z_var} = float({z_map_expr})"),
                            format!("    {points_var} = []"),
                            format!("    for {ti_var} in range({theta_steps_var}):"),
                            format!(
                                "        {theta_var} = (2.0 * math.pi * float({ti_var})) / float({theta_steps_var})"
                            ),
                            format!("        {radius_var} = float({radius_expr})"),
                            format!(
                                "        if {radius_var} <= 0.0: raise ValueError('sampled-radial-loft radius must stay positive')"
                            ),
                            format!(
                                "        {points_var}.append(({radius_var} * math.cos({theta_var}), {radius_var} * math.sin({theta_var})))"
                            ),
                            format!(
                                "    {sections}.append(Pos(0, 0, {section_z_var}) * _ecky_face(Polygon({points_var})))"
                            ),
                            format!("{result} = _ecky_loft({sections})"),
                        ],
                        result_var: result,
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "helical-ridge" => {
                let call = parse_helical_ridge_call(args)?;
                let mut kwargs = Vec::new();
                if let Some(female) = call.female.as_ref() {
                    kwargs.push((
                        "female".into(),
                        PyExpr::Inline(lower_bool_expr(female, scope)?),
                    ));
                }
                if let Some(clearance) = call.clearance.as_ref() {
                    kwargs.push((
                        "clearance".into(),
                        PyExpr::Inline(lower_num_expr(clearance, scope)?),
                    ));
                }
                if let Some(lefthand) = call.lefthand.as_ref() {
                    kwargs.push((
                        "lefthand".into(),
                        PyExpr::Inline(lower_bool_expr(lefthand, scope)?),
                    ));
                }
                (
                    PyExpr::Call {
                        func: "_ecky_helical_ridge".into(),
                        args: vec![
                            PyExpr::Inline(lower_num_expr(&call.radius, scope)?),
                            PyExpr::Inline(lower_num_expr(&call.pitch, scope)?),
                            PyExpr::Inline(lower_num_expr(&call.height, scope)?),
                            PyExpr::Inline(lower_num_expr(&call.base_width, scope)?),
                            PyExpr::Inline(lower_num_expr(&call.crest_width, scope)?),
                            PyExpr::Inline(lower_num_expr(&call.depth, scope)?),
                        ],
                        kwargs,
                    },
                    B123dGeomKind::Solid3d,
                )
            }
            "thread" => {
                let call = parse_thread_call(args)?;
                let len = lower_num_expr(&call.length, scope)?;
                let (r, p, d) = if let Some(iso) = call.iso.as_ref() {
                    let designation = iso
                        .as_str()
                        .ok_or_else(|| validation("`thread :iso` expects a string like \"M6\"."))?;
                    let (radius, pitch, depth) =
                        crate::ecky_core_ir::iso_metric_thread_core(designation).ok_or_else(|| {
                            validation(format!(
                                "`thread` unknown ISO designation `{designation}` (try M3, M4, M5, M6, M8, M10, M12, M16, M20)."
                            ))
                        })?;
                    (format!("{radius}"), format!("{pitch}"), format!("{depth}"))
                } else {
                    let radius = call
                        .radius
                        .as_ref()
                        .ok_or_else(|| validation("`thread` requires `:radius` (or `:iso`)."))?;
                    let pitch = call
                        .pitch
                        .as_ref()
                        .ok_or_else(|| validation("`thread` requires `:pitch` (or `:iso`)."))?;
                    let depth = call
                        .depth
                        .as_ref()
                        .ok_or_else(|| validation("`thread` requires `:depth` (or `:iso`)."))?;
                    (
                        lower_num_expr(radius, scope)?,
                        lower_num_expr(pitch, scope)?,
                        lower_num_expr(depth, scope)?,
                    )
                };
                let bw = match call.base_width.as_ref() {
                    Some(v) => lower_num_expr(v, scope)?,
                    None => format!("({p}) * 0.75"),
                };
                let cw = match call.crest_width.as_ref() {
                    Some(v) => lower_num_expr(v, scope)?,
                    None => format!("({p}) * 0.25"),
                };
                let mut kwargs = Vec::new();
                if let Some(female) = call.female.as_ref() {
                    kwargs.push((
                        "female".into(),
                        PyExpr::Inline(lower_bool_expr(female, scope)?),
                    ));
                }
                if let Some(clearance) = call.clearance.as_ref() {
                    kwargs.push((
                        "clearance".into(),
                        PyExpr::Inline(lower_num_expr(clearance, scope)?),
                    ));
                }
                if let Some(lefthand) = call.lefthand.as_ref() {
                    kwargs.push((
                        "lefthand".into(),
                        PyExpr::Inline(lower_bool_expr(lefthand, scope)?),
                    ));
                }
                (
                    PyExpr::Call {
                        func: "_ecky_thread".into(),
                        args: vec![
                            PyExpr::Inline(r),
                            PyExpr::Inline(p),
                            PyExpr::Inline(len),
                            PyExpr::Inline(d),
                            PyExpr::Inline(bw),
                            PyExpr::Inline(cw),
                        ],
                        kwargs,
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
            "rib" | "groove" => {
                if args.len() != 3 {
                    return Err(validation(format!(
                        "`{node}` expects a solid, a profile, and a path."
                    )));
                }
                let solid = self.lower_solid_expr(&args[0], scope)?;
                let section = self.lower_sketch_expr(&args[1], scope)?;
                let path_geom = self.lower_path_expr(&args[2], scope)?;
                let swept = PyExpr::Call {
                    func: "sweep".into(),
                    args: vec![PyExpr::Call {
                        func: "_ecky_face".into(),
                        args: vec![section.expr],
                        kwargs: vec![],
                    }],
                    kwargs: vec![("path".into(), path_geom.expr)],
                };
                let op = if node == "rib" { "+" } else { "-" };
                (
                    PyExpr::BinOp {
                        op,
                        operands: vec![solid.expr, swept],
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
                let point_count =
                    self.materialize_list_len(&args[0], scope, "bezier-path points")?;
                if point_count < 4 || (point_count - 1) % 3 != 0 {
                    return Err(validation(format!(
                        "`bezier-path` expects 3n+1 control points (4, 7, 10, ...), got {}.",
                        point_count
                    )));
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
                if properties.contains_key("to-radius") || properties.contains_key("to_radius") {
                    return Err(validation(
                        "`fillet :to-radius` (tapered, variable-radius fillet) is not supported on the build123d backend — build123d `fillet()` takes a single radius. Use the native or freecad backend for tapered fillets.",
                    ));
                }
                let radius = lower_num_expr(&pos_args[0], scope)?;
                let selector = if let Some(value) = properties.get("edges") {
                    expr_parse_edge_selector_spec(value, "edge selection")?
                } else {
                    parse_edge_selector_spec("all")?
                };
                let body = self.lower_solid_expr(&pos_args[1], scope)?;
                let body_var = self.lin.linearize(&body.expr);
                let edges_expr = if selector.target_ids().is_some() {
                    let Some(part_id) = scope.current_part_id.as_deref() else {
                        return Err(validation(
                            "Exact edge target-id selectors require a part context in build123d lowering.",
                        ));
                    };
                    format!(
                        "_ecky_select_edges({body_var}, {}, {:?})",
                        selector.python_payload_literal(),
                        part_id
                    )
                } else {
                    format!(
                        "_ecky_select_edges({body_var}, {})",
                        selector.python_payload_literal()
                    )
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
                let loop_var = python_local_ident(index, "__ecky_ru_");
                let local_name = python_local_ident(index, "_");
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
                let loop_var = python_local_ident(index, "__ecky_rp_");
                let local_name = python_local_ident(index, "_");
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
                let loop_var = python_local_ident(index, "__ecky_rc_");
                let local_name = python_local_ident(index, "_");
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
                let (pos_args, properties) = self.parse_properties(args)?;
                if pos_args.len() != 2 {
                    return Err(validation(
                        "`shell` expects wall thickness and a geometry node.",
                    ));
                }
                let wall = lower_num_expr(&pos_args[0], scope)?;
                let outer = self.lower_solid_expr(&pos_args[1], scope)?;
                if let Some(face_select) = properties.get("faces") {
                    let parsed =
                        expr_parse_face_selector_spec(face_select, "shell face selection")?;
                    let Some(part_id) = scope.current_part_id.as_deref() else {
                        return Err(validation(
                            "Build123d `shell :faces` exact selectors require a part context.",
                        ));
                    };
                    let outer_var = self.lin.linearize(&outer.expr);
                    let result = self.next_imp_var();
                    let lines = vec![format!(
                        "{result} = offset({outer_var}, amount=-({wall}), openings=_ecky_select_shell_faces({outer_var}, {}, {:?}))",
                        parsed.python_payload_literal(),
                        part_id
                    )];
                    (
                        PyExpr::Imperative {
                            lines,
                            result_var: result,
                        },
                        B123dGeomKind::Solid3d,
                    )
                } else {
                    match self.plan_shell_target(&pos_args[1], &pos_args[0])? {
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
