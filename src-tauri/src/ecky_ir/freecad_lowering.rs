#![allow(clippy::needless_return)]

use std::collections::{BTreeMap, BTreeSet};

use crate::contracts::AppResult;
use crate::ecky_core_ir::{
    CoreArrayOp, CoreBooleanOp, CoreFrameOp, CoreLiteral, CoreMetaOp, CoreNode, CoreNodeKind,
    CoreOperation, CorePathOp, CorePrimitive, CoreProgram, CoreReference, CoreSelectorPayload,
    CoreSurfaceOp, CoreSymbol, CoreTransformOp, CoreValueKind,
};
use crate::models::ParamValue;

use super::edge_ops::{
    edge_selector_spec_from_core_payload, face_selector_spec_from_core_payload,
    parse_edge_selector_spec,
};
use super::model::{
    allocate_legacy_local_name, core_program_param_defaults, expr_head_symbol, expr_keyword_name,
    expr_list_items, expr_parse_edge_selector_spec, expr_parse_face_selector_spec,
    expr_parse_stringish, materialize_selector_nodes, parse_value_kind_tag, IrExpr, IrModel,
};
use super::shared::{unsupported, validation};

pub fn lower_to_freecad(source: &str) -> AppResult<String> {
    let model = super::model::parse_model(source)?;
    lower_model_to_freecad(&model)
}

pub(crate) fn lower_model_to_freecad(model: &IrModel) -> AppResult<String> {
    let defaults = model
        .params
        .iter()
        .map(|param| (param.field.key().to_string(), param.default_value.clone()))
        .collect::<BTreeMap<_, _>>();
    let parts = model
        .parts
        .iter()
        .map(|part| (part.part_id.clone(), part.expr.clone()))
        .collect::<Vec<_>>();
    lower_parts_to_freecad(&defaults, &parts)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn lower_core_program_to_freecad(program: &CoreProgram) -> AppResult<String> {
    let defaults = core_program_param_defaults(program)?;
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let parts = program
        .parts
        .iter()
        .map(|part| {
            let mut used_local_names = BTreeMap::new();
            Ok((
                part.key.clone(),
                core_node_to_ir_expr_local(
                    &part.root,
                    &param_names,
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &mut used_local_names,
                )?,
            ))
        })
        .collect::<AppResult<Vec<_>>>()?;
    lower_parts_to_freecad(&defaults, &parts)
}

fn lower_parts_to_freecad(
    defaults: &BTreeMap<String, ParamValue>,
    parts: &[(String, IrExpr)],
) -> AppResult<String> {
    let scope = LoweringScope::new(defaults.clone());
    let mut lowerer = FreecadLowerer::new();
    let mut lowered_parts = Vec::new();

    for (part_id, expr) in parts {
        lowerer.current_part_id = Some(part_id.clone());
        let node = lowerer.lower_geom_expr(expr, &scope)?;
        let part_var = lowerer.stabilize_part(node);
        lowered_parts.push((part_id.clone(), part_var));
    }

    Ok(lowerer.serialize_program(lowered_parts))
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

fn core_selector_payload_to_ir_expr_local(payload: &CoreSelectorPayload) -> AppResult<IrExpr> {
    match payload {
        CoreSelectorPayload::EdgeAll
        | CoreSelectorPayload::EdgeClauses(_)
        | CoreSelectorPayload::EdgeTag(_)
        | CoreSelectorPayload::EdgeTargetIds(_) => Ok(IrExpr::Selector(
            crate::ecky_ir::model::IrSelectorExpr::Edge(edge_selector_spec_from_core_payload(
                payload,
            )?),
        )),
        CoreSelectorPayload::FaceClauses(_)
        | CoreSelectorPayload::FaceTag(_)
        | CoreSelectorPayload::FaceTargetIds(_) => Ok(IrExpr::Selector(
            crate::ecky_ir::model::IrSelectorExpr::Face(face_selector_spec_from_core_payload(
                payload,
            )?),
        )),
    }
}

fn core_node_to_ir_expr_local(
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
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => {
            Ok(IrExpr::symbol(core_symbol_name(symbol)))
        }
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
            "Unsupported Core IR reference in FreeCAD lowerer: {:?}.",
            other
        ))),
        CoreNodeKind::Build { bindings, result } => {
            let mut items = vec![IrExpr::symbol("build")];
            let mut nested_refs = refs.clone();
            let mut nested_locals = locals.clone();
            for binding in bindings {
                let ir_name = allocate_legacy_local_name(&binding.name, used_local_names);
                let mut shape_items = vec![
                    IrExpr::symbol("shape"),
                    IrExpr::symbol(ir_name.clone()),
                    core_node_to_ir_expr_local(
                        &binding.value,
                        param_names,
                        &nested_refs,
                        &nested_locals,
                        used_local_names,
                    )?,
                ];
                if binding.value.value_kind != CoreValueKind::Any {
                    shape_items.push(IrExpr::keyword("value-kind"));
                    shape_items.push(IrExpr::symbol(core_value_kind_tag_local(
                        binding.value.value_kind,
                    )));
                }
                items.push(IrExpr::list(shape_items));
                nested_refs.insert(binding.value.id.raw(), ir_name.clone());
                nested_locals.insert(binding.name.clone(), ir_name);
            }
            items.push(IrExpr::list(vec![
                IrExpr::symbol("result"),
                core_node_to_ir_expr_local(
                    result,
                    param_names,
                    &nested_refs,
                    &nested_locals,
                    used_local_names,
                )?,
            ]));
            Ok(IrExpr::list(items))
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
                        IrExpr::symbol(ir_name.clone()),
                        core_node_to_ir_expr_local(
                            &binding.value,
                            param_names,
                            refs,
                            locals,
                            used_local_names,
                        )?,
                    ];
                    if binding.value.value_kind != CoreValueKind::Any {
                        pair.push(IrExpr::keyword("value-kind"));
                        pair.push(IrExpr::symbol(core_value_kind_tag_local(
                            binding.value.value_kind,
                        )));
                    }
                    Ok(IrExpr::list(pair))
                })
                .collect::<AppResult<Vec<_>>>()?;
            for (original_name, ir_name, _) in ir_binding_names {
                nested_locals.insert(original_name, ir_name);
            }
            Ok(IrExpr::list(vec![
                IrExpr::symbol("let"),
                IrExpr::list(binding_values),
                core_node_to_ir_expr_local(
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
        } => Ok(IrExpr::list(vec![
            IrExpr::symbol("if"),
            core_node_to_ir_expr_local(condition, param_names, refs, locals, used_local_names)?,
            core_node_to_ir_expr_local(then_branch, param_names, refs, locals, used_local_names)?,
            core_node_to_ir_expr_local(else_branch, param_names, refs, locals, used_local_names)?,
        ])),
        CoreNodeKind::Call { op, args, keywords } => {
            let mut items = vec![IrExpr::symbol(core_operation_name_local(op))];
            for arg in args {
                items.push(core_node_to_ir_expr_local(
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
                    (_, Some(selector)) => core_selector_payload_to_ir_expr_local(selector)?,
                    (_, None) => core_node_to_ir_expr_local(
                        keyword.source_node(),
                        param_names,
                        refs,
                        locals,
                        used_local_names,
                    )?,
                });
            }
            materialize_selector_nodes(IrExpr::list(items))
        }
        CoreNodeKind::Range { start, end } => Ok(IrExpr::list(vec![
            IrExpr::symbol("range"),
            core_node_to_ir_expr_local(start, param_names, refs, locals, used_local_names)?,
            core_node_to_ir_expr_local(end, param_names, refs, locals, used_local_names)?,
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
                    core_node_to_ir_expr_local(
                        body,
                        param_names,
                        refs,
                        &nested_locals,
                        used_local_names,
                    )?,
                ]),
            ];
            for source in sources {
                items.push(core_node_to_ir_expr_local(
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
                IrExpr::symbol(core_operation_name_local(op)),
            ];
            for arg in args {
                items.push(core_node_to_ir_expr_local(
                    arg,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
            }
            items.push(core_node_to_ir_expr_local(
                list,
                param_names,
                refs,
                locals,
                used_local_names,
            )?);
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => Ok(IrExpr::list(
            items
                .iter()
                .map(|item| {
                    core_node_to_ir_expr_local(item, param_names, refs, locals, used_local_names)
                })
                .collect::<AppResult<Vec<_>>>()?,
        )),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum GeomKind {
    Sketch2d,
    Solid3d,
    Path3d,
    Frame,
}

impl GeomKind {
    fn noun(&self) -> &'static str {
        match self {
            Self::Sketch2d => "2D sketch",
            Self::Solid3d => "3D solid",
            Self::Path3d => "3D path",
            Self::Frame => "frame",
        }
    }
}

#[derive(Clone, Debug)]
enum LoweredBinding {
    Geom { var: String, kind: GeomKind },
    RuntimeList(LoweredRuntimeList),
    Number(String),
    Boolean(String),
    Stringish(String),
    Frame(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeListKind {
    Number,
    Geom(GeomKind),
}

#[derive(Clone, Debug)]
struct LoweredRuntimeList {
    var: String,
    kind: RuntimeListKind,
}

impl RuntimeListKind {
    fn noun(&self) -> &'static str {
        match self {
            Self::Number => "runtime number list",
            Self::Geom(kind) => match kind {
                GeomKind::Sketch2d => "runtime 2D sketch list",
                GeomKind::Solid3d => "runtime 3D solid list",
                GeomKind::Path3d => "runtime 3D path list",
                GeomKind::Frame => "runtime frame list",
            },
        }
    }
}

#[derive(Clone, Debug)]
struct LoweringScope {
    params: BTreeMap<String, ParamValue>,
    locals: Vec<BTreeMap<String, LoweredBinding>>,
}

impl LoweringScope {
    fn new(params: BTreeMap<String, ParamValue>) -> Self {
        Self {
            params,
            locals: Vec::new(),
        }
    }

    fn with_frame(&self, frame: BTreeMap<String, LoweredBinding>) -> Self {
        let mut locals = self.locals.clone();
        locals.push(frame);
        Self {
            params: self.params.clone(),
            locals,
        }
    }

    fn resolve(&self, symbol: &str) -> Option<&LoweredBinding> {
        self.locals.iter().rev().find_map(|frame| frame.get(symbol))
    }
}

#[derive(Debug)]
struct LoweredNode {
    expr: String,
    kind: GeomKind,
}

#[derive(Debug, Default)]
struct ParsedCallArgs {
    positional: Vec<IrExpr>,
    keywords: BTreeMap<String, IrExpr>,
}

struct SampledRadialLoftCall {
    binders: [String; 3],
    height: IrExpr,
    z_steps: IrExpr,
    theta_steps: IrExpr,
    radius: IrExpr,
    z_map: Option<IrExpr>,
}

struct HelicalRidgeCall {
    radius: IrExpr,
    pitch: IrExpr,
    height: IrExpr,
    base_width: IrExpr,
    crest_width: IrExpr,
    depth: IrExpr,
    female: Option<IrExpr>,
    clearance: Option<IrExpr>,
    lefthand: Option<IrExpr>,
}

impl ParsedCallArgs {
    fn parse(node: &str, args: &[IrExpr], allowed_keywords: &[&str]) -> AppResult<Self> {
        let allowed = allowed_keywords.iter().copied().collect::<BTreeSet<_>>();
        let mut positional = Vec::new();
        let mut keywords = BTreeMap::new();
        let mut index = 0usize;

        while index < args.len() {
            if let Some(name) = expr_keyword_name(&args[index]) {
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
                    .insert(name.replace('-', "_"), args[index + 1].clone())
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
            positional.push(args[index].clone());
            index += 1;
        }

        Ok(Self {
            positional,
            keywords,
        })
    }
}

fn parse_sampled_radial_loft_call(args: &[IrExpr]) -> AppResult<SampledRadialLoftCall> {
    if args.is_empty() {
        return Err(validation(
            "`sampled-radial-loft` expects binder names plus keyword/value options.",
        ));
    }
    let binders = expr_list_items(&args[0], "`sampled-radial-loft` binders")?;
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
            expr_parse_stringish(&binders[0], "`sampled-radial-loft` theta binder")?,
            expr_parse_stringish(&binders[1], "`sampled-radial-loft` z binder")?,
            expr_parse_stringish(&binders[2], "`sampled-radial-loft` fz binder")?,
        ],
        height: parsed
            .keywords
            .get("height")
            .cloned()
            .ok_or_else(|| validation("`sampled-radial-loft` requires `:height`."))?,
        z_steps: parsed
            .keywords
            .get("z_steps")
            .cloned()
            .ok_or_else(|| validation("`sampled-radial-loft` requires `:z-steps`."))?,
        theta_steps: parsed
            .keywords
            .get("theta_steps")
            .cloned()
            .ok_or_else(|| validation("`sampled-radial-loft` requires `:theta-steps`."))?,
        radius: parsed
            .keywords
            .get("radius")
            .cloned()
            .ok_or_else(|| validation("`sampled-radial-loft` requires `:radius`."))?,
        z_map: parsed.keywords.get("z_map").cloned(),
    })
}

fn parse_helical_ridge_call(args: &[IrExpr]) -> AppResult<HelicalRidgeCall> {
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
            .cloned()
            .ok_or_else(|| validation("`helical-ridge` requires `:radius`."))?,
        pitch: parsed
            .keywords
            .get("pitch")
            .cloned()
            .ok_or_else(|| validation("`helical-ridge` requires `:pitch`."))?,
        height: parsed
            .keywords
            .get("height")
            .cloned()
            .ok_or_else(|| validation("`helical-ridge` requires `:height`."))?,
        base_width: parsed
            .keywords
            .get("base_width")
            .cloned()
            .ok_or_else(|| validation("`helical-ridge` requires `:base-width`."))?,
        crest_width: parsed
            .keywords
            .get("crest_width")
            .cloned()
            .ok_or_else(|| validation("`helical-ridge` requires `:crest-width`."))?,
        depth: parsed
            .keywords
            .get("depth")
            .cloned()
            .ok_or_else(|| validation("`helical-ridge` requires `:depth`."))?,
        female: parsed.keywords.get("female").cloned(),
        clearance: parsed.keywords.get("clearance").cloned(),
        lefthand: parsed.keywords.get("lefthand").cloned(),
    })
}

fn describe_expr(value: &IrExpr) -> String {
    if let Some(sym) = value.as_symbol() {
        return format!("symbol `{}`", sym);
    }
    if value.as_f64().is_some() {
        return "number literal".to_string();
    }
    if value.as_bool().is_some() {
        return "boolean literal".to_string();
    }
    if value.as_str().is_some() {
        return "string literal".to_string();
    }
    if let Some(items) = value.as_list() {
        if let Ok(head) = expr_head_symbol(items, "expression") {
            return format!("expression `{}`", head);
        }
        return "list expression".to_string();
    }
    "expression".to_string()
}

fn typed_hole_error(args: &[IrExpr]) -> String {
    let parsed = ParsedCallArgs::parse("hole", args, &["type", "goal"]);
    let (type_name, goal) = match parsed {
        Ok(parsed) => (
            parsed
                .keywords
                .get("type")
                .and_then(|value| expr_parse_stringish(value, "hole type").ok()),
            parsed
                .keywords
                .get("goal")
                .and_then(|value| expr_parse_stringish(value, "hole goal").ok()),
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

struct FreecadLowerer {
    lines: Vec<String>,
    counter: usize,
    current_part_id: Option<String>,
}

impl FreecadLowerer {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            counter: 0,
            current_part_id: None,
        }
    }

    fn next_var(&mut self) -> String {
        let var = format!("_f{}", self.counter);
        self.counter += 1;
        var
    }

    fn emit(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    fn stabilize_part(&mut self, node: LoweredNode) -> String {
        match node.kind {
            GeomKind::Solid3d => node.expr,
            _ => node.expr,
        }
    }

    fn serialize_program(&self, parts: Vec<(String, String)>) -> String {
        let mut out = freecad_preamble();
        out.extend(self.lines.iter().cloned());
        out.push(String::new());
        out.push("_ecky_parts = [".to_string());
        for (index, (part_id, expr)) in parts.iter().enumerate() {
            let suffix = if index + 1 == parts.len() { "" } else { "," };
            out.push(format!("    ({:?}, {} ){}", part_id, expr, suffix));
        }
        out.push("]".to_string());
        out.join("\n")
    }

    fn lower_geom_expr_locally(
        &self,
        value: &IrExpr,
        scope: &LoweringScope,
    ) -> AppResult<(Vec<String>, String, GeomKind, usize)> {
        let mut nested = FreecadLowerer {
            lines: Vec::new(),
            counter: self.counter,
            current_part_id: self.current_part_id.clone(),
        };
        let node = nested.lower_geom_expr(value, scope)?;
        Ok((nested.lines, node.expr, node.kind, nested.counter))
    }

    fn lower_binding_value_hinted(
        &mut self,
        value: &IrExpr,
        scope: &LoweringScope,
        hint: Option<CoreValueKind>,
    ) -> AppResult<LoweredBinding> {
        match hint {
            Some(CoreValueKind::Number) => {
                return Ok(LoweredBinding::Number(self.lower_num_expr(value, scope)?))
            }
            Some(CoreValueKind::Boolean) => {
                return Ok(LoweredBinding::Boolean(self.lower_bool_expr(value, scope)?))
            }
            Some(CoreValueKind::Text) => {
                return Ok(LoweredBinding::Stringish(
                    self.lower_stringish_expr(value, scope)?,
                ))
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
                let node = self.lower_geom_expr(value, scope)?;
                return Ok(LoweredBinding::Geom {
                    var: node.expr,
                    kind: node.kind,
                });
            }
            Some(CoreValueKind::List) => {
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
        value: &IrExpr,
        scope: &LoweringScope,
    ) -> AppResult<LoweredBinding> {
        let geom_err = match self.lower_geom_expr(value, scope) {
            Ok(node) => {
                return Ok(LoweredBinding::Geom {
                    var: node.expr,
                    kind: node.kind,
                })
            }
            Err(err) => err,
        };
        if let Ok(frame) = self.lower_frame_expr(value, scope) {
            return Ok(LoweredBinding::Frame(frame));
        }
        if let Ok(list) = self.lower_runtime_list_expr(value, scope) {
            return Ok(LoweredBinding::RuntimeList(list));
        }
        let num_err = match self.lower_num_expr(value, scope) {
            Ok(number) => return Ok(LoweredBinding::Number(number)),
            Err(err) => err,
        };
        let bool_err = match self.lower_bool_expr(value, scope) {
            Ok(boolean) => return Ok(LoweredBinding::Boolean(boolean)),
            Err(err) => err,
        };
        if let Ok(stringish) = self.lower_stringish_expr(value, scope) {
            return Ok(LoweredBinding::Stringish(stringish));
        }
        if matches!(geom_err.code, crate::models::AppErrorCode::Validation) {
            return Err(geom_err);
        }
        if matches!(num_err.code, crate::models::AppErrorCode::Validation) {
            return Err(num_err);
        }
        if matches!(bool_err.code, crate::models::AppErrorCode::Validation) {
            return Err(bool_err);
        }
        Err(validation(format!(
            "Could not lower binding value from {}.",
            describe_expr(value)
        )))
    }

    fn lower_scalar_binding(
        &self,
        value: &IrExpr,
        scope: &LoweringScope,
    ) -> AppResult<LoweredBinding> {
        let num_err = match self.lower_num_expr(value, scope) {
            Ok(number) => return Ok(LoweredBinding::Number(number)),
            Err(err) => err,
        };
        let bool_err = match self.lower_bool_expr(value, scope) {
            Ok(boolean) => return Ok(LoweredBinding::Boolean(boolean)),
            Err(err) => err,
        };
        if let Ok(stringish) = self.lower_stringish_expr(value, scope) {
            return Ok(LoweredBinding::Stringish(stringish));
        }
        if matches!(num_err.code, crate::models::AppErrorCode::Validation) {
            return Err(num_err);
        }
        if matches!(bool_err.code, crate::models::AppErrorCode::Validation) {
            return Err(bool_err);
        }
        Err(num_err)
    }

    fn lower_scalar_let_scope(
        &self,
        bindings_value: &IrExpr,
        scope: &LoweringScope,
    ) -> AppResult<LoweringScope> {
        let bindings = expr_list_items(bindings_value, "let bindings")?;
        let mut frame = BTreeMap::new();
        let mut child_scope = scope.clone();
        for binding in bindings {
            let pair = expr_list_items(binding, "binding pair")?;
            if pair.len() != 2 && pair.len() != 4 {
                return Err(validation("Each binding must be `(name expr)`."));
            }
            let name = pair[0]
                .as_symbol()
                .ok_or_else(|| validation("Binding name must be a symbol."))?;
            let lowered = self.lower_scalar_binding(&pair[1], &child_scope)?;
            frame.insert(name.to_string(), lowered);
            child_scope = scope.with_frame(frame.clone());
        }
        Ok(child_scope)
    }

    fn lower_num_expr(&self, value: &IrExpr, scope: &LoweringScope) -> AppResult<String> {
        if let Some(n) = value.as_f64() {
            return Ok(fmt_f64(n));
        }
        if let Some(sym) = value.as_symbol() {
            if let Some(binding) = scope.resolve(sym) {
                return match binding {
                    LoweredBinding::Number(expr) => Ok(expr.clone()),
                    _ => Err(unsupported(format!(
                        "Symbol `{}` is not a numeric binding in this context.",
                        sym
                    ))),
                };
            }
            return match scope.params.get(sym) {
                Some(ParamValue::Number(n)) => {
                    Ok(format!("float(params.get({:?}, {}))", sym, fmt_f64(*n)))
                }
                Some(ParamValue::Null) => Err(unsupported(format!(
                    "Symbol `{}` is not a numeric parameter.",
                    sym
                ))),
                Some(_) => Err(unsupported(format!(
                    "Symbol `{}` is not a numeric parameter.",
                    sym
                ))),
                None => Err(validation(format!("Unknown symbol `{}`.", sym))),
            };
        }

        let items = expr_list_items(value, "numeric expression")?;
        let op = expr_head_symbol(items, "numeric expression")?;
        let args = &items[1..];
        if matches!(op, "let" | "let*") {
            if args.len() != 2 {
                return Err(validation(
                    "Numeric `let`/`let*` expects bindings and body.",
                ));
            }
            let child_scope = self.lower_scalar_let_scope(&args[0], scope)?;
            return self.lower_num_expr(&args[1], &child_scope);
        }
        match op {
            "if" => {
                if args.len() != 3 {
                    return Err(validation("`if` expects condition, then-expr, else-expr."));
                }
                Ok(format!(
                    "({} if {} else {})",
                    self.lower_num_expr(&args[1], scope)?,
                    self.lower_bool_expr(&args[0], scope)?,
                    self.lower_num_expr(&args[2], scope)?,
                ))
            }
            "+" => {
                let parts = args
                    .iter()
                    .map(|arg| self.lower_num_expr(arg, scope))
                    .collect::<AppResult<Vec<_>>>()?;
                Ok(if parts.is_empty() {
                    "0.0".to_string()
                } else {
                    format!("({})", parts.join(" + "))
                })
            }
            "-" => {
                if args.is_empty() {
                    return Err(validation("`-` expects at least one argument."));
                }
                if args.len() == 1 {
                    return Ok(format!("(-{})", self.lower_num_expr(&args[0], scope)?));
                }
                let parts = args
                    .iter()
                    .map(|arg| self.lower_num_expr(arg, scope))
                    .collect::<AppResult<Vec<_>>>()?;
                Ok(format!("({})", parts.join(" - ")))
            }
            "*" => {
                let parts = args
                    .iter()
                    .map(|arg| self.lower_num_expr(arg, scope))
                    .collect::<AppResult<Vec<_>>>()?;
                Ok(if parts.is_empty() {
                    "1.0".to_string()
                } else {
                    format!("({})", parts.join(" * "))
                })
            }
            "/" => {
                if args.len() != 2 {
                    return Err(validation("`/` expects exactly two arguments."));
                }
                Ok(format!(
                    "({} / {})",
                    self.lower_num_expr(&args[0], scope)?,
                    self.lower_num_expr(&args[1], scope)?,
                ))
            }
            "min" | "max" => {
                let parts = args
                    .iter()
                    .map(|arg| self.lower_num_expr(arg, scope))
                    .collect::<AppResult<Vec<_>>>()?;
                Ok(format!("{}({})", op, parts.join(", ")))
            }
            "clamp" => {
                if args.len() != 3 {
                    return Err(validation("`clamp` expects value, min, max."));
                }
                Ok(format!(
                    "max({}, min({}, {}))",
                    self.lower_num_expr(&args[1], scope)?,
                    self.lower_num_expr(&args[2], scope)?,
                    self.lower_num_expr(&args[0], scope)?,
                ))
            }
            "lerp" => {
                if args.len() != 3 {
                    return Err(validation("`lerp` expects start, end, t."));
                }
                let s = self.lower_num_expr(&args[0], scope)?;
                let e = self.lower_num_expr(&args[1], scope)?;
                let t = self.lower_num_expr(&args[2], scope)?;
                Ok(format!("(({s}) + (({e}) - ({s})) * ({t}))"))
            }
            "smoothstep" => {
                if args.len() != 3 {
                    return Err(validation("`smoothstep` expects edge0, edge1, x."));
                }
                let e0 = self.lower_num_expr(&args[0], scope)?;
                let e1 = self.lower_num_expr(&args[1], scope)?;
                let x = self.lower_num_expr(&args[2], scope)?;
                Ok(format!(
                    "(lambda _t: _t*_t*(3.0-2.0*_t))(max(0.0, min(1.0, ({x} - {e0}) / ({e1} - {e0}))))"
                ))
            }
            "sin" => {
                if args.len() != 1 {
                    return Err(validation("`sin` expects one argument."));
                }
                Ok(format!(
                    "math.sin({})",
                    self.lower_num_expr(&args[0], scope)?
                ))
            }
            "cos" => {
                if args.len() != 1 {
                    return Err(validation("`cos` expects one argument."));
                }
                Ok(format!(
                    "math.cos({})",
                    self.lower_num_expr(&args[0], scope)?
                ))
            }
            "tan" => {
                if args.len() != 1 {
                    return Err(validation("`tan` expects one argument."));
                }
                Ok(format!(
                    "math.tan({})",
                    self.lower_num_expr(&args[0], scope)?
                ))
            }
            "atan" => {
                if args.len() != 1 {
                    return Err(validation("`atan` expects one argument."));
                }
                Ok(format!(
                    "math.atan({})",
                    self.lower_num_expr(&args[0], scope)?
                ))
            }
            "atan2" => {
                if args.len() != 2 {
                    return Err(validation("`atan2` expects y and x."));
                }
                Ok(format!(
                    "math.atan2({}, {})",
                    self.lower_num_expr(&args[0], scope)?,
                    self.lower_num_expr(&args[1], scope)?
                ))
            }
            "abs" => {
                if args.len() != 1 {
                    return Err(validation("`abs` expects one argument."));
                }
                Ok(format!("abs({})", self.lower_num_expr(&args[0], scope)?))
            }
            "floor" => {
                if args.len() != 1 {
                    return Err(validation("`floor` expects one argument."));
                }
                Ok(format!(
                    "math.floor({})",
                    self.lower_num_expr(&args[0], scope)?
                ))
            }
            "deg" | "deg->rad" => {
                if args.len() != 1 {
                    return Err(validation("`deg`/`deg->rad` expects one argument."));
                }
                Ok(format!(
                    "math.radians({})",
                    self.lower_num_expr(&args[0], scope)?
                ))
            }
            "rad" | "rad->deg" => {
                if args.len() != 1 {
                    return Err(validation("`rad`/`rad->deg` expects one argument."));
                }
                Ok(format!(
                    "math.degrees({})",
                    self.lower_num_expr(&args[0], scope)?
                ))
            }
            "signed-pow" => {
                if args.len() != 2 {
                    return Err(validation("`signed-pow` expects value and exponent."));
                }
                Ok(format!(
                    "_ecky_signed_pow({}, {})",
                    self.lower_num_expr(&args[0], scope)?,
                    self.lower_num_expr(&args[1], scope)?
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
                    self.lower_num_expr(&args[0], scope)?,
                    self.lower_num_expr(&args[1], scope)?,
                    self.lower_num_expr(&args[2], scope)?
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
                    self.lower_num_expr(&args[0], scope)?,
                    self.lower_num_expr(&args[1], scope)?,
                    self.lower_num_expr(&args[2], scope)?,
                    self.lower_num_expr(&args[3], scope)?,
                    self.lower_num_expr(&args[4], scope)?,
                    self.lower_num_expr(&args[5], scope)?
                ))
            }
            other => Err(unsupported(format!(
                "Numeric expression `{}` is not supported by the FreeCAD lowerer.",
                other
            ))),
        }
    }

    fn lower_bool_expr(&self, value: &IrExpr, scope: &LoweringScope) -> AppResult<String> {
        if let Some(flag) = value.as_bool() {
            return Ok(if flag { "True".into() } else { "False".into() });
        }
        if let Some(sym) = value.as_symbol() {
            if let Some(binding) = scope.resolve(sym) {
                return match binding {
                    LoweredBinding::Boolean(expr) => Ok(expr.clone()),
                    _ => Err(unsupported(format!(
                        "Symbol `{}` is not a boolean binding in this context.",
                        sym
                    ))),
                };
            }
            return match scope.params.get(sym) {
                Some(ParamValue::Boolean(flag)) => Ok(format!(
                    "bool(params.get({:?}, {}))",
                    sym,
                    if *flag { "True" } else { "False" }
                )),
                Some(ParamValue::Null) => Err(unsupported(format!(
                    "Symbol `{}` is not a boolean parameter.",
                    sym
                ))),
                Some(_) => Err(unsupported(format!(
                    "Symbol `{}` is not a boolean parameter.",
                    sym
                ))),
                None => Err(validation(format!("Unknown symbol `{}`.", sym))),
            };
        }

        let items = expr_list_items(value, "boolean expression")?;
        let op = expr_head_symbol(items, "boolean expression")?;
        let args = &items[1..];
        if matches!(op, "let" | "let*") {
            if args.len() != 2 {
                return Err(validation(
                    "Boolean `let`/`let*` expects bindings and body.",
                ));
            }
            let child_scope = self.lower_scalar_let_scope(&args[0], scope)?;
            return self.lower_bool_expr(&args[1], &child_scope);
        }
        match op {
            "if" => {
                if args.len() != 3 {
                    return Err(validation("Boolean `if` expects condition, then, else."));
                }
                Ok(format!(
                    "({} if {} else {})",
                    self.lower_bool_expr(&args[1], scope)?,
                    self.lower_bool_expr(&args[0], scope)?,
                    self.lower_bool_expr(&args[2], scope)?,
                ))
            }
            "not" => {
                if args.len() != 1 {
                    return Err(validation("`not` expects one argument."));
                }
                Ok(format!("(not {})", self.lower_bool_expr(&args[0], scope)?))
            }
            "and" | "or" => {
                let parts = args
                    .iter()
                    .map(|arg| self.lower_bool_expr(arg, scope))
                    .collect::<AppResult<Vec<_>>>()?;
                Ok(format!("({})", parts.join(&format!(" {} ", op))))
            }
            "=" | ">" | ">=" | "<" | "<=" => {
                if args.len() != 2 {
                    return Err(validation(format!(
                        "`{}` expects exactly two arguments.",
                        op
                    )));
                }
                if op == "=" {
                    if let (Ok(a), Ok(b)) = (
                        self.lower_num_expr(&args[0], scope),
                        self.lower_num_expr(&args[1], scope),
                    ) {
                        return Ok(format!("({} == {})", a, b));
                    }
                    let a = self.lower_stringish_expr(&args[0], scope)?;
                    let b = self.lower_stringish_expr(&args[1], scope)?;
                    return Ok(format!("({} == {})", a, b));
                }
                Ok(format!(
                    "({} {} {})",
                    self.lower_num_expr(&args[0], scope)?,
                    op,
                    self.lower_num_expr(&args[1], scope)?,
                ))
            }
            other => Err(unsupported(format!(
                "Boolean operator `{}` is not supported by the FreeCAD lowerer.",
                other
            ))),
        }
    }

    fn lower_stringish_expr(&self, value: &IrExpr, scope: &LoweringScope) -> AppResult<String> {
        if let Some(text) = value.as_str() {
            return Ok(format!("{:?}", text));
        }
        if let Some(sym) = value.as_symbol() {
            if let Some(binding) = scope.resolve(sym) {
                return match binding {
                    LoweredBinding::Stringish(expr)
                    | LoweredBinding::Number(expr)
                    | LoweredBinding::Boolean(expr) => Ok(format!("str({})", expr)),
                    _ => Err(unsupported(format!(
                        "Symbol `{}` is not a string-like binding in this context.",
                        sym
                    ))),
                };
            }
            return match scope.params.get(sym) {
                Some(ParamValue::String(text)) => {
                    Ok(format!("str(params.get({:?}, {:?}))", sym, text))
                }
                Some(ParamValue::Number(n)) => {
                    Ok(format!("str(params.get({:?}, {}))", sym, fmt_f64(*n)))
                }
                Some(ParamValue::Boolean(flag)) => Ok(format!(
                    "str(params.get({:?}, {}))",
                    sym,
                    if *flag { "True" } else { "False" }
                )),
                Some(ParamValue::Null) => Ok("None".to_string()),
                None => Ok(format!("{:?}", sym)),
            };
        }
        Err(validation("Expected text or symbol."))
    }

    fn lower_vec3(
        &self,
        value: &IrExpr,
        scope: &LoweringScope,
    ) -> AppResult<(String, String, String)> {
        let items = expr_list_items(value, "3D point expression")?;
        match items.len() {
            3 => Ok((
                self.lower_num_expr(&items[0], scope)?,
                self.lower_num_expr(&items[1], scope)?,
                self.lower_num_expr(&items[2], scope)?,
            )),
            _ => Err(validation("3D points must be `(x y z)` triples.")),
        }
    }

    fn lower_point2(&self, value: &IrExpr, scope: &LoweringScope) -> AppResult<String> {
        let items = expr_list_items(value, "2D point expression")?;
        if expr_head_symbol(items, "2D point expression").ok() == Some("list") && items.len() == 3 {
            return Ok(format!(
                "App.Vector({}, {}, 0.0)",
                self.lower_num_expr(&items[1], scope)?,
                self.lower_num_expr(&items[2], scope)?,
            ));
        }
        if matches!(
            expr_head_symbol(items, "2D point expression").ok(),
            Some("let" | "let*")
        ) && items.len() == 3
        {
            let child_scope = self.lower_scalar_let_scope(&items[1], scope)?;
            return self.lower_point2(&items[2], &child_scope);
        }
        match items.len() {
            2 => Ok(format!(
                "App.Vector({}, {}, 0.0)",
                self.lower_num_expr(&items[0], scope)?,
                self.lower_num_expr(&items[1], scope)?,
            )),
            _ => Err(validation("2D points must be `(x y)` pairs.")),
        }
    }

    fn lower_point3(&self, value: &IrExpr, scope: &LoweringScope) -> AppResult<String> {
        let items = expr_list_items(value, "3D point expression")?;
        if expr_head_symbol(items, "3D point expression").ok() == Some("list") && items.len() == 4 {
            return Ok(format!(
                "App.Vector({}, {}, {})",
                self.lower_num_expr(&items[1], scope)?,
                self.lower_num_expr(&items[2], scope)?,
                self.lower_num_expr(&items[3], scope)?,
            ));
        }
        if matches!(
            expr_head_symbol(items, "3D point expression").ok(),
            Some("let" | "let*")
        ) && items.len() == 3
        {
            let child_scope = self.lower_scalar_let_scope(&items[1], scope)?;
            return self.lower_point3(&items[2], &child_scope);
        }
        match items.len() {
            3 => Ok(format!(
                "App.Vector({}, {}, {})",
                self.lower_num_expr(&items[0], scope)?,
                self.lower_num_expr(&items[1], scope)?,
                self.lower_num_expr(&items[2], scope)?,
            )),
            _ => Err(validation("3D points must be `(x y z)` triples.")),
        }
    }

    fn lower_align_tuple(
        &self,
        value: Option<&IrExpr>,
        node: &str,
        default: (&'static str, &'static str, &'static str),
    ) -> AppResult<String> {
        let Some(value) = value else {
            return Ok(format!(
                "(\"{}\", \"{}\", \"{}\")",
                default.0, default.1, default.2
            ));
        };
        let unwrapped = if let Some(items) = value.as_list() {
            if items.first().and_then(IrExpr::as_symbol) == Some("quote") && items.len() == 2 {
                expr_list_items(&items[1], "align tuple")?
            } else {
                items
            }
        } else {
            expr_list_items(value, "align tuple")?
        };
        if unwrapped.len() != 3 {
            return Err(validation(format!("`{} :align` expects `(x y z)`.", node)));
        }
        let mut axis = Vec::new();
        for item in unwrapped {
            let name = item
                .as_symbol()
                .or_else(|| item.as_str())
                .ok_or_else(|| validation(format!("`{} :align` expects symbols.", node)))?;
            match name {
                "min" | "center" | "max" => axis.push(format!("{:?}", name)),
                other => {
                    return Err(validation(format!(
                        "`{} :align` expects `min`, `center`, or `max`, got `{}`.",
                        node, other
                    )))
                }
            }
        }
        Ok(format!("({}, {}, {})", axis[0], axis[1], axis[2]))
    }

    fn lower_edge_selector(&self, value: Option<&IrExpr>) -> AppResult<String> {
        let parsed = match value {
            Some(value) => expr_parse_edge_selector_spec(value, "edge selection")?,
            None => parse_edge_selector_spec("all")?,
        };
        Ok(parsed.python_payload_literal().to_string())
    }

    fn lower_face_selector(&self, value: Option<&IrExpr>) -> AppResult<Option<String>> {
        let Some(value) = value else {
            return Ok(None);
        };
        let parsed = expr_parse_face_selector_spec(value, "face selection")?;
        Ok(Some(parsed.python_payload_literal().to_string()))
    }

    fn lower_range_pair(
        &self,
        value: &IrExpr,
        scope: &LoweringScope,
        node: &str,
    ) -> AppResult<(String, String)> {
        let items = expr_list_items(value, node)?;
        if items.len() != 2 {
            return Err(validation(format!("`{}` expects `(min max)`.", node)));
        }
        Ok((
            self.lower_num_expr(&items[0], scope)?,
            self.lower_num_expr(&items[1], scope)?,
        ))
    }

    fn lower_frame_expr(&mut self, value: &IrExpr, scope: &LoweringScope) -> AppResult<String> {
        if let Some(sym) = value.as_symbol() {
            if let Some(binding) = scope.resolve(sym) {
                return match binding {
                    LoweredBinding::Frame(expr) => Ok(expr.clone()),
                    LoweredBinding::Geom { var, kind } if *kind == GeomKind::Frame => {
                        Ok(var.clone())
                    }
                    _ => Err(unsupported(format!(
                        "Symbol `{}` is not a frame binding in this context.",
                        sym
                    ))),
                };
            }
        }
        let items = expr_list_items(value, "frame expression")?;
        let op = expr_head_symbol(items, "frame expression")?;
        let args = &items[1..];
        match op {
            "plane" => {
                let parsed = ParsedCallArgs::parse("plane", args, &["origin", "x", "normal"])?;
                let origin = parsed
                    .keywords
                    .get("origin")
                    .map(|v| self.lower_vec3(v, scope))
                    .transpose()?;
                let x = parsed
                    .keywords
                    .get("x")
                    .map(|v| self.lower_vec3(v, scope))
                    .transpose()?;
                let normal = parsed
                    .keywords
                    .get("normal")
                    .map(|v| self.lower_vec3(v, scope))
                    .transpose()?;
                Ok(format!(
                    "{{'kind': 'plane', 'origin': {}, 'x': {}, 'normal': {}}}",
                    origin
                        .map(|(x, y, z)| format!("({}, {}, {})", x, y, z))
                        .unwrap_or_else(|| "None".to_string()),
                    x.map(|(x, y, z)| format!("({}, {}, {})", x, y, z))
                        .unwrap_or_else(|| "None".to_string()),
                    normal
                        .map(|(x, y, z)| format!("({}, {}, {})", x, y, z))
                        .unwrap_or_else(|| "None".to_string())
                ))
            }
            "location" => {
                let parsed = ParsedCallArgs::parse("location", args, &["offset", "rotate"])?;
                if parsed.positional.len() != 1 {
                    return Err(validation(
                        "`location` expects a plane/frame and optional `:offset` / `:rotate`.",
                    ));
                }
                let offset = parsed
                    .keywords
                    .get("offset")
                    .map(|v| self.lower_vec3(v, scope))
                    .transpose()?;
                let rotate = parsed
                    .keywords
                    .get("rotate")
                    .map(|v| self.lower_vec3(v, scope))
                    .transpose()?;
                Ok(format!(
                    "{{'kind': 'location', 'frame': {}, 'offset': {}, 'rotate': {}}}",
                    self.lower_frame_expr(&parsed.positional[0], scope)?,
                    offset
                        .map(|(x, y, z)| format!("({}, {}, {})", x, y, z))
                        .unwrap_or_else(|| "None".to_string()),
                    rotate
                        .map(|(x, y, z)| format!("({}, {}, {})", x, y, z))
                        .unwrap_or_else(|| "None".to_string())
                ))
            }
            "path-frame" => {
                let parsed = ParsedCallArgs::parse("path-frame", args, &["at", "up"])?;
                if parsed.positional.len() != 1 {
                    return Err(validation("`path-frame` expects a path."));
                }
                let at = match parsed.keywords.get("at") {
                    Some(value) => self
                        .lower_num_expr(value, scope)
                        .or_else(|_| self.lower_stringish_expr(value, scope))?,
                    None => "\"end\"".to_string(),
                };
                let up = parsed
                    .keywords
                    .get("up")
                    .map(|v| self.lower_vec3(v, scope))
                    .transpose()?;
                Ok(format!(
                    "{{'kind': 'path-frame', 'path': {}, 'at': {}, 'up': {}}}",
                    self.lower_geom_expr(&parsed.positional[0], scope)?.expr,
                    at,
                    up.map(|(x, y, z)| format!("({}, {}, {})", x, y, z))
                        .unwrap_or_else(|| "None".to_string())
                ))
            }
            other => Err(unsupported(format!(
                "Frame node `{}` is not supported by the FreeCAD lowerer.",
                other
            ))),
        }
    }

    fn lower_geom_list(
        &mut self,
        values: &[IrExpr],
        scope: &LoweringScope,
    ) -> AppResult<Vec<LoweredNode>> {
        values
            .iter()
            .map(|value| self.lower_geom_expr(value, scope))
            .collect()
    }

    fn lower_runtime_list_expr(
        &mut self,
        value: &IrExpr,
        scope: &LoweringScope,
    ) -> AppResult<LoweredRuntimeList> {
        if let Some(sym) = value.as_symbol() {
            return match scope.resolve(sym) {
                Some(LoweredBinding::RuntimeList(list)) => Ok(list.clone()),
                Some(_) => Err(unsupported(format!(
                    "Symbol `{}` is not a runtime list binding.",
                    sym
                ))),
                None => Err(validation(format!("Unknown symbol `{}`.", sym))),
            };
        }

        let items = expr_list_items(value, "runtime list expression")?;
        let op = expr_head_symbol(items, "runtime list expression")?;
        let args = &items[1..];
        match op {
            "range" => {
                let (start, end) = match args {
                    [end] => ("0.0".to_string(), self.lower_num_expr(end, scope)?),
                    [start, end] => (
                        self.lower_num_expr(start, scope)?,
                        self.lower_num_expr(end, scope)?,
                    ),
                    _ => return Err(validation("`range` expects one or two bounds.")),
                };
                let result = self.next_var();
                self.emit(format!(
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
                let child_scope = self.lower_scalar_let_scope(&args[0], scope)?;
                self.lower_runtime_list_expr(&args[1], &child_scope)
            }
            other => Err(unsupported(format!(
                "Runtime list expression `{}` is not supported by the FreeCAD lowerer.",
                other
            ))),
        }
    }

    fn lower_runtime_map_list(
        &mut self,
        args: &[IrExpr],
        scope: &LoweringScope,
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

        let result = self.next_var();
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
            let tuple_var = self.next_var();
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
        let (body_lines, body_result, body_kind, next_counter) =
            self.lower_geom_expr_locally(&body, &child_scope)?;
        lines.extend(body_lines.into_iter().map(|line| format!("    {line}")));
        lines.push(format!("    {result}.append({body_result})"));
        self.lines.extend(lines);
        self.counter = self.counter.max(next_counter);
        Ok(LoweredRuntimeList {
            var: result,
            kind: RuntimeListKind::Geom(body_kind),
        })
    }

    fn lower_apply_geom(
        &mut self,
        args: &[IrExpr],
        scope: &LoweringScope,
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
        let fixed_vars = fixed
            .iter()
            .map(|node| node.expr.clone())
            .collect::<Vec<_>>();
        let mut call_args = fixed_vars.clone();
        call_args.push(format!("*{}", runtime_list.var));
        let result = self.next_var();
        match op {
            "union" | "fuse" => {
                let needed = 2usize.saturating_sub(fixed_vars.len());
                if needed > 0 {
                    self.emit(format!(
                        "if len({}) < {}: raise ValueError('apply {} produced too few geometry operands')",
                        runtime_list.var, needed, op
                    ));
                }
                let helper = match kind {
                    GeomKind::Solid3d => "_ecky_fuse_many",
                    _ => "_ecky_union_many",
                };
                self.emit(format!("{result} = {helper}({})", call_args.join(", ")));
            }
            "compound" => {
                let needed = 1usize.saturating_sub(fixed_vars.len());
                if needed > 0 {
                    self.emit(format!(
                        "if len({}) < {}: raise ValueError('apply compound produced no geometry')",
                        runtime_list.var, needed
                    ));
                }
                self.emit(format!(
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
                self.emit(format!(
                    "if not {}: raise ValueError('apply {} produced no cutters')",
                    runtime_list.var, op
                ));
                self.emit(format!(
                    "{result} = _ecky_cut_many({})",
                    call_args.join(", ")
                ));
            }
            "intersection" | "common" => {
                let needed = 2usize.saturating_sub(fixed_vars.len());
                if needed > 0 {
                    self.emit(format!(
                        "if len({}) < {}: raise ValueError('apply {} produced too few geometry operands')",
                        runtime_list.var, needed, op
                    ));
                }
                self.emit(format!(
                    "{result} = _ecky_common_many({})",
                    call_args.join(", ")
                ));
            }
            other => {
                return Err(unsupported(format!(
                    "`apply {}` is not supported by the FreeCAD lowerer.",
                    other
                )))
            }
        }
        Ok(LoweredNode { expr: result, kind })
    }

    fn lower_geom_expr(&mut self, value: &IrExpr, scope: &LoweringScope) -> AppResult<LoweredNode> {
        if let Some(sym) = value.as_symbol() {
            return match scope.resolve(sym) {
                Some(LoweredBinding::Geom { var, kind }) => Ok(LoweredNode {
                    expr: var.clone(),
                    kind: kind.clone(),
                }),
                Some(LoweredBinding::Frame(expr)) => Ok(LoweredNode {
                    expr: expr.clone(),
                    kind: GeomKind::Frame,
                }),
                Some(_) => Err(unsupported(format!(
                    "Symbol `{}` is not a geometry binding in this context.",
                    sym
                ))),
                None => Err(validation(format!("Unknown symbol `{}`.", sym))),
            };
        }

        let items = expr_list_items(value, "geometry node")?;
        let op = expr_head_symbol(items, "geometry node")?;
        let args = &items[1..];

        match op {
            "let" => {
                if args.len() < 2 {
                    return Err(validation("`let` expects bindings and a body."));
                }
                let bindings = expr_list_items(&args[0], "let bindings")?;
                let mut frame = BTreeMap::new();
                let let_scope = scope.clone();
                for binding in bindings {
                    let pair = expr_list_items(binding, "binding pair")?;
                    if pair.len() != 2 && pair.len() != 4 {
                        return Err(validation("Each binding must be `(name expr)`."));
                    }
                    let name = expr_parse_stringish(&pair[0], "binding name")?;
                    let hint = extract_let_binding_hint(pair);
                    let lowered = self
                        .lower_binding_value_hinted(&pair[1], &let_scope, hint)
                        .map_err(|err| {
                            validation(format!("`let` binding `{}` failed: {}", name, err.message))
                        })?;
                    let local_name = python_local_ident(&name, "_");
                    match &lowered {
                        LoweredBinding::Geom { var, .. } => {
                            self.emit(format!("{local_name} = {var}"))
                        }
                        LoweredBinding::RuntimeList(_) => {}
                        LoweredBinding::Number(expr)
                        | LoweredBinding::Boolean(expr)
                        | LoweredBinding::Stringish(expr)
                        | LoweredBinding::Frame(expr) => {
                            self.emit(format!("{local_name} = {expr}"))
                        }
                    }
                    let stored = match lowered {
                        LoweredBinding::Geom { kind, .. } => LoweredBinding::Geom {
                            var: local_name.clone(),
                            kind,
                        },
                        LoweredBinding::RuntimeList(list) => LoweredBinding::RuntimeList(list),
                        LoweredBinding::Number(_) => LoweredBinding::Number(local_name.clone()),
                        LoweredBinding::Boolean(_) => LoweredBinding::Boolean(local_name.clone()),
                        LoweredBinding::Stringish(_) => {
                            LoweredBinding::Stringish(local_name.clone())
                        }
                        LoweredBinding::Frame(_) => LoweredBinding::Frame(local_name.clone()),
                    };
                    frame.insert(name, stored);
                }
                let child_scope = scope.with_frame(frame);
                return self.lower_geom_expr(&args[1], &child_scope);
            }
            "let*" => {
                if args.len() < 2 {
                    return Err(validation("`let*` expects bindings and a body."));
                }
                let bindings = expr_list_items(&args[0], "let* bindings")?;
                let mut child_scope = scope.clone();
                let mut frame = BTreeMap::new();
                for binding in bindings {
                    let pair = expr_list_items(binding, "binding pair")?;
                    if pair.len() != 2 && pair.len() != 4 {
                        return Err(validation("Each binding must be `(name expr)`."));
                    }
                    let name = expr_parse_stringish(&pair[0], "binding name")?;
                    let hint = extract_let_binding_hint(pair);
                    let lowered = self
                        .lower_binding_value_hinted(&pair[1], &child_scope, hint)
                        .map_err(|err| {
                            validation(format!("`let*` binding `{}` failed: {}", name, err.message))
                        })?;
                    let local_name = python_local_ident(&name, "_");
                    match &lowered {
                        LoweredBinding::Geom { var, .. } => {
                            self.emit(format!("{local_name} = {var}"))
                        }
                        LoweredBinding::RuntimeList(_) => {}
                        LoweredBinding::Number(expr)
                        | LoweredBinding::Boolean(expr)
                        | LoweredBinding::Stringish(expr)
                        | LoweredBinding::Frame(expr) => {
                            self.emit(format!("{local_name} = {expr}"))
                        }
                    }
                    let stored = match lowered {
                        LoweredBinding::Geom { kind, .. } => LoweredBinding::Geom {
                            var: local_name.clone(),
                            kind,
                        },
                        LoweredBinding::RuntimeList(list) => LoweredBinding::RuntimeList(list),
                        LoweredBinding::Number(_) => LoweredBinding::Number(local_name.clone()),
                        LoweredBinding::Boolean(_) => LoweredBinding::Boolean(local_name.clone()),
                        LoweredBinding::Stringish(_) => {
                            LoweredBinding::Stringish(local_name.clone())
                        }
                        LoweredBinding::Frame(_) => LoweredBinding::Frame(local_name.clone()),
                    };
                    frame.insert(name, stored.clone());
                    child_scope = child_scope.with_frame(frame.clone());
                }
                return self.lower_geom_expr(&args[1], &child_scope);
            }
            "build" => {
                let (bindings, result) = parse_build_expr(value)?;
                let mut child_scope = scope.clone();
                for binding in bindings {
                    let lowered = self
                        .lower_binding_value_hinted(&binding.expr, &child_scope, binding.value_kind)
                        .map_err(|err| {
                            validation(format!(
                                "`build` binding `{}` failed: {}",
                                binding.name, err.message
                            ))
                        })?;
                    let local_name = python_local_ident(&binding.name, "_");
                    match &lowered {
                        LoweredBinding::Geom { var, .. } => {
                            self.emit(format!("{local_name} = {var}"))
                        }
                        LoweredBinding::RuntimeList(_) => {}
                        LoweredBinding::Number(expr)
                        | LoweredBinding::Boolean(expr)
                        | LoweredBinding::Stringish(expr)
                        | LoweredBinding::Frame(expr) => {
                            self.emit(format!("{local_name} = {expr}"))
                        }
                    }
                    let stored = match lowered {
                        LoweredBinding::Geom { kind, .. } => LoweredBinding::Geom {
                            var: local_name.clone(),
                            kind,
                        },
                        LoweredBinding::RuntimeList(list) => LoweredBinding::RuntimeList(list),
                        LoweredBinding::Number(_) => LoweredBinding::Number(local_name.clone()),
                        LoweredBinding::Boolean(_) => LoweredBinding::Boolean(local_name.clone()),
                        LoweredBinding::Stringish(_) => {
                            LoweredBinding::Stringish(local_name.clone())
                        }
                        LoweredBinding::Frame(_) => LoweredBinding::Frame(local_name.clone()),
                    };
                    let mut frame = BTreeMap::new();
                    frame.insert(binding.name, stored);
                    child_scope = child_scope.with_frame(frame);
                }
                return self.lower_geom_expr(&result, &child_scope);
            }
            "apply" => {
                return self.lower_apply_geom(args, scope);
            }
            "hole" => return Err(validation(typed_hole_error(args))),
            "repeat-union" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`repeat-union` expects index symbol, count, and a body.",
                    ));
                }
                let index = args[0]
                    .as_symbol()
                    .ok_or_else(|| validation("`repeat-union` index must be a symbol."))?;
                let count = self.lower_num_expr(&args[1], scope)?;
                let count_var = self.next_var();
                let items_var = self.next_var();
                let result = self.next_var();
                let loop_var = python_local_ident(index, "__ecky_ru_");
                let local_name = python_local_ident(index, "_");
                let mut frame = BTreeMap::new();
                frame.insert(
                    index.to_string(),
                    LoweredBinding::Number(local_name.clone()),
                );
                let child_scope = scope.with_frame(frame);
                let (body_lines, body_result, body_kind, next_counter) =
                    self.lower_geom_expr_locally(&args[2], &child_scope)?;
                let helper = match body_kind {
                    GeomKind::Solid3d => "_ecky_fuse_many",
                    _ => "_ecky_union_many",
                };
                let mut lines = vec![
                    format!("{items_var} = []"),
                    format!("{count_var} = max(0, int(math.floor({count})))"),
                    format!("for {loop_var} in range({count_var}):"),
                    format!("    {local_name} = float({loop_var})"),
                ];
                lines.extend(body_lines.into_iter().map(|line| format!("    {line}")));
                lines.push(format!("    {items_var}.append({body_result})"));
                lines.push(format!(
                    "if not {items_var}: raise ValueError('repeat-union produced no geometry')"
                ));
                lines.push(format!("{result} = {helper}(*{items_var})"));
                self.lines.extend(lines);
                self.counter = self.counter.max(next_counter);
                return Ok(LoweredNode {
                    expr: result,
                    kind: body_kind,
                });
            }
            "repeat-pick" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`repeat-pick` expects index symbol, count, predicate, and a body.",
                    ));
                }
                let index = args[0]
                    .as_symbol()
                    .ok_or_else(|| validation("`repeat-pick` index must be a symbol."))?;
                let count = self.lower_num_expr(&args[1], scope)?;
                let count_var = self.next_var();
                let items_var = self.next_var();
                let result = self.next_var();
                let loop_var = python_local_ident(index, "__ecky_rp_");
                let local_name = python_local_ident(index, "_");
                let mut frame = BTreeMap::new();
                frame.insert(
                    index.to_string(),
                    LoweredBinding::Number(local_name.clone()),
                );
                let child_scope = scope.with_frame(frame);
                let predicate = self.lower_bool_expr(&args[2], &child_scope)?;
                let (body_lines, body_result, body_kind, next_counter) =
                    self.lower_geom_expr_locally(&args[3], &child_scope)?;
                let mut lines = vec![
                    format!("{items_var} = []"),
                    format!("{count_var} = max(0, int(math.floor({count})))"),
                    format!("for {loop_var} in range({count_var}):"),
                    format!("    {local_name} = float({loop_var})"),
                    format!("    if {predicate}:"),
                ];
                lines.extend(body_lines.into_iter().map(|line| format!("        {line}")));
                lines.push(format!("        {items_var}.append({body_result})"));
                lines.push(format!(
                    "if not {items_var}: raise ValueError('repeat-pick found no matching geometry')"
                ));
                lines.push(format!("{result} = {items_var}[-1]"));
                self.lines.extend(lines);
                self.counter = self.counter.max(next_counter);
                return Ok(LoweredNode {
                    expr: result,
                    kind: body_kind,
                });
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
                let count = self.lower_num_expr(&args[1], scope)?;
                let count_var = self.next_var();
                let items_var = self.next_var();
                let result = self.next_var();
                let loop_var = python_local_ident(index, "__ecky_rc_");
                let local_name = python_local_ident(index, "_");
                let mut frame = BTreeMap::new();
                frame.insert(
                    index.to_string(),
                    LoweredBinding::Number(local_name.clone()),
                );
                let child_scope = scope.with_frame(frame);
                let (body_lines, body_result, body_kind, next_counter) =
                    self.lower_geom_expr_locally(&args[2], &child_scope)?;
                let mut lines = vec![
                    format!("{items_var} = []"),
                    format!("{count_var} = max(0, int(math.floor({count})))"),
                    format!("for {loop_var} in range({count_var}):"),
                    format!("    {local_name} = float({loop_var})"),
                ];
                lines.extend(body_lines.into_iter().map(|line| format!("    {line}")));
                lines.push(format!("    {items_var}.append({body_result})"));
                lines.push(format!(
                    "{result} = _ecky_compound(*{items_var}) if {items_var} else Part.Shape()"
                ));
                self.lines.extend(lines);
                self.counter = self.counter.max(next_counter);
                return Ok(LoweredNode {
                    expr: result,
                    kind: body_kind,
                });
            }
            "box" => {
                let parsed = ParsedCallArgs::parse("box", args, &["align"])?;
                if parsed.positional.len() != 3 {
                    return Err(validation("`box` expects width, depth, and height."));
                }
                let w = self.lower_num_expr(&parsed.positional[0], scope)?;
                let d = self.lower_num_expr(&parsed.positional[1], scope)?;
                let h = self.lower_num_expr(&parsed.positional[2], scope)?;
                let align = self.lower_align_tuple(
                    parsed.keywords.get("align"),
                    "box",
                    ("center", "center", "min"),
                )?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_box({w}, {d}, {h}, {align})"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "sphere" => {
                let parsed = ParsedCallArgs::parse("sphere", args, &["align"])?;
                if parsed.positional.is_empty() {
                    return Err(validation("`sphere` expects radius."));
                }
                let r = self.lower_num_expr(&parsed.positional[0], scope)?;
                let align = self.lower_align_tuple(
                    parsed.keywords.get("align"),
                    "sphere",
                    ("center", "center", "center"),
                )?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_sphere({r}, {align})"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "cylinder" => {
                let parsed = ParsedCallArgs::parse("cylinder", args, &["align"])?;
                if parsed.positional.len() < 2 {
                    return Err(validation("`cylinder` expects radius and height."));
                }
                let r = self.lower_num_expr(&parsed.positional[0], scope)?;
                let h = self.lower_num_expr(&parsed.positional[1], scope)?;
                let align = self.lower_align_tuple(
                    parsed.keywords.get("align"),
                    "cylinder",
                    ("center", "center", "min"),
                )?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_cylinder({r}, {h}, {align})"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "cone" => {
                let parsed = ParsedCallArgs::parse("cone", args, &["align"])?;
                if parsed.positional.len() < 3 {
                    return Err(validation(
                        "`cone` expects bottom radius, top radius, and height.",
                    ));
                }
                let br = self.lower_num_expr(&parsed.positional[0], scope)?;
                let tr = self.lower_num_expr(&parsed.positional[1], scope)?;
                let h = self.lower_num_expr(&parsed.positional[2], scope)?;
                let align = self.lower_align_tuple(
                    parsed.keywords.get("align"),
                    "cone",
                    ("center", "center", "min"),
                )?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_cone({br}, {tr}, {h}, {align})"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "circle" => {
                if args.is_empty() {
                    return Err(validation("`circle` expects radius."));
                }
                let r = self.lower_num_expr(&args[0], scope)?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_circle({r})"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "rounded_rect" | "rounded-rect" => {
                if args.len() < 3 || args.len() > 4 {
                    return Err(validation(
                        "`rounded_rect` expects width, height, corner radius, and optional segments.",
                    ));
                }
                let w = self.lower_num_expr(&args[0], scope)?;
                let h = self.lower_num_expr(&args[1], scope)?;
                let r = self.lower_num_expr(&args[2], scope)?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_rounded_rect({w}, {h}, {r})"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "rounded_polygon" | "rounded-polygon" => {
                if args.len() < 2 {
                    return Err(validation(
                        "`rounded-polygon` expects point list and corner radius.",
                    ));
                }
                let points = self.lower_point_list(&args[0], scope, false)?;
                let radius = self.lower_num_expr(&args[1], scope)?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_rounded_polygon([{points}], {radius})"
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "polygon" => {
                if args.len() != 1 {
                    return Err(validation("`polygon` expects a point list."));
                }
                let points = self.lower_point_list(&args[0], scope, false)?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_polygon([{points}])"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "path" => {
                let points = if args.len() == 1 {
                    args[0].clone()
                } else {
                    IrExpr::list(args.to_vec())
                };
                let point_items = expr_list_items(&points, "path points")?;
                let planar = point_items.iter().all(|item| {
                    let item_list = item.as_list().unwrap_or(&[]);
                    item_list.len() == 2
                        || item_list.len() == 3
                            && self
                                .lower_num_expr(&item_list[2], scope)
                                .map(|value| value == "0.0")
                                .unwrap_or(false)
                });
                let lowered = self.lower_point_list(&points, scope, true)?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_path([{lowered}])"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: if planar {
                        GeomKind::Sketch2d
                    } else {
                        GeomKind::Path3d
                    },
                });
            }
            "bezier-path" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(validation(
                        "`bezier-path` expects points and optional segments.",
                    ));
                }
                let points = self.lower_point_list(&args[0], scope, true)?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_bezier_path([{points}])"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Path3d,
                });
            }
            "bspline" => {
                let parsed = ParsedCallArgs::parse(
                    "bspline",
                    args,
                    &["closed", "tangents", "tangent-scalars"],
                )?;
                if parsed.positional.is_empty() {
                    return Err(validation("`bspline` expects a point list."));
                }
                let points = self.lower_point_list(&parsed.positional[0], scope, false)?;
                let closed = parsed
                    .keywords
                    .get("closed")
                    .map(|value| self.lower_bool_expr(value, scope))
                    .transpose()?
                    .unwrap_or_else(|| "False".to_string());
                let tangents = parsed
                    .keywords
                    .get("tangents")
                    .map(|value| self.lower_point_list(value, scope, false))
                    .transpose()?;
                let tangent_scalars = parsed
                    .keywords
                    .get("tangent_scalars")
                    .map(|value| self.lower_num_list(value, scope))
                    .transpose()?;
                let result = self.next_var();
                let mut call = format!("{result} = _ecky_bspline([{points}], closed={closed}");
                if let Some(tangents) = tangents {
                    call.push_str(&format!(", tangents=[{tangents}]"));
                }
                if let Some(scalars) = tangent_scalars {
                    call.push_str(&format!(", tangent_scalars=[{}]", scalars.join(", ")));
                }
                call.push(')');
                self.emit(call);
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "profile" => {
                let mut outer_wires: Vec<String> = Vec::new();
                let mut hole_wires: Vec<String> = Vec::new();
                if args.first().and_then(expr_keyword_name).is_some() {
                    if args.len() % 2 != 0 {
                        return Err(validation(
                            "`profile` clauses must use keyword/value pairs for `:outer` and `:holes`.",
                        ));
                    }
                    let mut index = 0usize;
                    while index < args.len() {
                        let name = expr_keyword_name(&args[index]).ok_or_else(|| {
                            validation(
                                "`profile` clauses must use keywords like `:outer` and `:holes`.",
                            )
                        })?;
                        let value = &args[index + 1];
                        match name {
                            "outer" => {
                                outer_wires.extend(self.lower_wire_collection(value, scope)?)
                            }
                            "holes" => hole_wires.extend(self.lower_wire_collection(value, scope)?),
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
                        let pair = expr_list_items(form, "profile clause")?;
                        if pair.len() != 2 {
                            return Err(validation(
                                "`profile` clauses must look like `(:outer ...)` or `(:holes ...)`.",
                            ));
                        }
                        let name = expr_keyword_name(&pair[0]).ok_or_else(|| {
                            validation(
                                "`profile` clauses must use keywords like `:outer` and `:holes`.",
                            )
                        })?;
                        match name {
                            "outer" => {
                                outer_wires.extend(self.lower_wire_collection(&pair[1], scope)?)
                            }
                            "holes" => {
                                hole_wires.extend(self.lower_wire_collection(&pair[1], scope)?)
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
                if outer_wires.len() != 1 {
                    return Err(unsupported(
                        "`profile` currently expects a single outer loop on FreeCAD.".to_string(),
                    ));
                }
                let result = self.next_var();
                if hole_wires.is_empty() {
                    self.emit(format!("{result} = _ecky_face({})", outer_wires[0]));
                } else {
                    self.emit(format!(
                        "{result} = _ecky_face_with_holes({}, [{}])",
                        outer_wires[0],
                        hole_wires.join(", ")
                    ));
                }
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "make-face" => {
                if args.len() != 1 {
                    return Err(validation("`make-face` expects a single wire node."));
                }
                let sketch = self.lower_geom_expr(&args[0], scope)?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_face({})", sketch.expr));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "union" | "fuse" => {
                if args.len() < 2 {
                    return Err(validation(format!(
                        "`{}` expects at least two operands.",
                        op
                    )));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind(&operands)?;
                let result = self.next_var();
                let args = operands
                    .into_iter()
                    .map(|node| node.expr)
                    .collect::<Vec<_>>()
                    .join(", ");
                match kind {
                    GeomKind::Solid3d => self.emit(format!("{result} = _ecky_fuse_many({args})")),
                    _ => self.emit(format!("{result} = _ecky_union_many({args})")),
                }
                return Ok(LoweredNode { expr: result, kind });
            }
            "compound" => {
                if args.is_empty() {
                    return Err(validation("`compound` expects at least one operand."));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind(&operands)?;
                let result = self.next_var();
                let args = operands
                    .into_iter()
                    .map(|node| node.expr)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.emit(format!("{result} = _ecky_compound({args})"));
                return Ok(LoweredNode { expr: result, kind });
            }
            "difference" | "cut" => {
                if args.len() < 2 {
                    return Err(validation(format!(
                        "`{}` expects at least two operands.",
                        op
                    )));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind(&operands)?;
                let result = self.next_var();
                let args = operands
                    .into_iter()
                    .map(|node| node.expr)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.emit(format!("{result} = _ecky_cut_many({args})"));
                return Ok(LoweredNode { expr: result, kind });
            }
            "intersection" | "common" => {
                if args.len() < 2 {
                    return Err(validation(format!(
                        "`{}` expects at least two operands.",
                        op
                    )));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind(&operands)?;
                let result = self.next_var();
                let args = operands
                    .into_iter()
                    .map(|node| node.expr)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.emit(format!("{result} = _ecky_common_many({args})"));
                return Ok(LoweredNode { expr: result, kind });
            }
            "xor" => {
                if args.len() < 2 {
                    return Err(validation("`xor` expects at least two operands."));
                }
                let operands = self.lower_geom_list(args, scope)?;
                let kind = self.same_kind(&operands)?;
                if matches!(kind, GeomKind::Path3d) {
                    return Err(unsupported(
                        "`xor` currently supports sketch and solid operands on FreeCAD."
                            .to_string(),
                    ));
                }
                let result = self.next_var();
                let args = operands
                    .into_iter()
                    .map(|node| node.expr)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.emit(format!("{result} = _ecky_xor_many({args})"));
                return Ok(LoweredNode { expr: result, kind });
            }
            "translate" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`translate` expects x, y, z, and a geometry node.",
                    ));
                }
                let x = self.lower_num_expr(&args[0], scope)?;
                let y = self.lower_num_expr(&args[1], scope)?;
                let z = self.lower_num_expr(&args[2], scope)?;
                let inner = self.lower_geom_expr(&args[3], scope)?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_translate({}, ({x}, {y}, {z}))",
                    inner.expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: inner.kind,
                });
            }
            "rotate" => {
                if args.len() != 4 {
                    return Err(validation("`rotate` expects x, y, z, and a geometry node."));
                }
                let rx = self.lower_num_expr(&args[0], scope)?;
                let ry = self.lower_num_expr(&args[1], scope)?;
                let rz = self.lower_num_expr(&args[2], scope)?;
                let inner = self.lower_geom_expr(&args[3], scope)?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_rotate({}, ({rx}, {ry}, {rz}))",
                    inner.expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: inner.kind,
                });
            }
            "scale" => {
                if args.len() != 4 {
                    return Err(validation("`scale` expects x, y, z, and a geometry node."));
                }
                let sx = self.lower_num_expr(&args[0], scope)?;
                let sy = self.lower_num_expr(&args[1], scope)?;
                let sz = self.lower_num_expr(&args[2], scope)?;
                let inner = self.lower_geom_expr(&args[3], scope)?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_scale({}, ({sx}, {sy}, {sz}))",
                    inner.expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: inner.kind,
                });
            }
            "mirror" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`mirror` expects axis, offset, and a geometry node.",
                    ));
                }
                let axis = expr_parse_stringish(&args[0], "mirror axis")?;
                let offset = self.lower_num_expr(&args[1], scope)?;
                let inner = self.lower_geom_expr(&args[2], scope)?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_mirror({}, {:?}, {})",
                    inner.expr, axis, offset
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: inner.kind,
                });
            }
            "extrude" => {
                let parsed = ParsedCallArgs::parse("extrude", args, &["symmetric"])?;
                if parsed.positional.len() != 2 {
                    return Err(validation("`extrude` expects a sketch and height."));
                }
                let sketch = self.lower_geom_expr(&parsed.positional[0], scope)?;
                let h = self.lower_num_expr(&parsed.positional[1], scope)?;
                let symmetric = parsed
                    .keywords
                    .get("symmetric")
                    .map(|value| self.lower_bool_expr(value, scope))
                    .transpose()?
                    .unwrap_or_else(|| "False".to_string());
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_extrude({}, {}, {})",
                    sketch.expr, h, symmetric
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "revolve" => {
                if args.len() < 2 {
                    return Err(validation("`revolve` expects a sketch and an angle."));
                }
                let sketch = self.lower_geom_expr(&args[0], scope)?;
                let angle = self.lower_num_expr(&args[1], scope)?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_revolve({}, {})",
                    sketch.expr, angle
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "loft" => {
                if args.len() < 3 {
                    return Err(validation(
                        "`loft` expects height and at least two section sketches.",
                    ));
                }
                let height = self.lower_num_expr(&args[0], scope)?;
                let section_count = args.len() - 1;
                let mut sections = Vec::with_capacity(section_count);
                for (index, sketch_expr) in args[1..].iter().enumerate() {
                    let sketch = self.lower_geom_expr(sketch_expr, scope)?;
                    if sketch.kind != GeomKind::Sketch2d {
                        return Err(unsupported(format!(
                            "`loft` section {} must be a 2D sketch, got {}.",
                            index + 1,
                            sketch.kind.noun()
                        )));
                    }
                    let placed = if index == 0 {
                        sketch.expr
                    } else {
                        let ratio = index as f64 / (section_count - 1) as f64;
                        let section = self.next_var();
                        self.emit(format!(
                            "{section} = _ecky_translate({}, (0.0, 0.0, ({height}) * {}))",
                            sketch.expr,
                            fmt_f64(ratio)
                        ));
                        section
                    };
                    sections.push(placed);
                }
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_loft([{}])", sections.join(", ")));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "taper" => {
                if !(args.len() == 3 || args.len() == 4) {
                    return Err(validation(
                        "`taper` expects height, scale, sketch or height, scale-x, scale-y, sketch.",
                    ));
                }
                let height = self.lower_num_expr(&args[0], scope)?;
                let (sx, sy, sketch_index) = if args.len() == 3 {
                    let scale = self.lower_num_expr(&args[1], scope)?;
                    (scale.clone(), scale, 2usize)
                } else {
                    (
                        self.lower_num_expr(&args[1], scope)?,
                        self.lower_num_expr(&args[2], scope)?,
                        3usize,
                    )
                };
                let sketch = self.lower_geom_expr(&args[sketch_index], scope)?;
                if sketch.kind != GeomKind::Sketch2d {
                    return Err(unsupported(format!(
                        "`taper` expects a 2D sketch, got {}.",
                        sketch.kind.noun()
                    )));
                }
                let sx_var = self.next_var();
                let sy_var = self.next_var();
                let scaled = self.next_var();
                let top = self.next_var();
                let result = self.next_var();
                self.emit(format!("{sx_var} = {sx}"));
                self.emit(format!("{sy_var} = {sy}"));
                self.emit(format!(
                    "if abs(float({sx_var}) - float({sy_var})) >= 1e-9: raise ValueError('FreeCAD lowerer: non-uniform taper not supported.')"
                ));
                self.emit(format!(
                    "{scaled} = _ecky_scale({}, ({sx_var}, {sy_var}, 1.0))",
                    sketch.expr
                ));
                self.emit(format!(
                    "{top} = _ecky_translate({scaled}, (0.0, 0.0, {height}))"
                ));
                self.emit(format!("{result} = _ecky_loft([{}, {top}])", sketch.expr));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "twist" => {
                if !(args.len() == 3 || args.len() == 4) {
                    return Err(validation(
                        "`twist` expects height, angle, sketch or height, angle, segments, sketch.",
                    ));
                }
                let height = self.lower_num_expr(&args[0], scope)?;
                let angle = self.lower_num_expr(&args[1], scope)?;
                let (segments, sketch_index) = if args.len() == 3 {
                    ("12".to_string(), 2usize)
                } else {
                    (self.lower_count_expr(&args[2], scope, 1)?, 3usize)
                };
                let sketch = self.lower_geom_expr(&args[sketch_index], scope)?;
                if sketch.kind != GeomKind::Sketch2d {
                    return Err(unsupported(format!(
                        "`twist` expects a 2D sketch, got {}.",
                        sketch.kind.noun()
                    )));
                }
                let segments_var = self.next_var();
                let sections = self.next_var();
                let section = self.next_var();
                let moved = self.next_var();
                let result = self.next_var();
                self.emit(format!("{segments_var} = {segments}"));
                self.emit(format!("{sections} = []"));
                self.emit(format!("for _ecky_i in range({segments_var} + 1):"));
                self.emit(format!(
                    "    _ecky_ratio = 0.0 if {segments_var} == 0 else float(_ecky_i) / float({segments_var})"
                ));
                self.emit(format!(
                    "    {section} = _ecky_rotate({}, (0.0, 0.0, ({angle}) * _ecky_ratio))",
                    sketch.expr
                ));
                self.emit(format!(
                    "    {moved} = _ecky_translate({section}, (0.0, 0.0, ({height}) * _ecky_ratio))"
                ));
                self.emit(format!("    {sections}.append({moved})"));
                self.emit(format!("{result} = _ecky_loft({sections})"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "sampled-radial-loft" => {
                let call = parse_sampled_radial_loft_call(args)?;
                let height = self.lower_num_expr(&call.height, scope)?;
                let z_steps = self.lower_count_expr(&call.z_steps, scope, 1)?;
                let theta_steps = self.lower_count_expr(&call.theta_steps, scope, 3)?;
                let sections = self.next_var();
                let result = self.next_var();
                let z_steps_var = self.next_var();
                let theta_steps_var = self.next_var();
                let theta_var = self.next_var();
                let z_var = self.next_var();
                let fz_var = self.next_var();
                let section_z_var = self.next_var();
                let radius_var = self.next_var();
                let points_var = self.next_var();
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
                let radius_expr = self.lower_num_expr(&call.radius, &child_scope)?;
                let z_map_expr = call
                    .z_map
                    .as_ref()
                    .map(|value| self.lower_num_expr(value, &child_scope))
                    .transpose()?
                    .unwrap_or_else(|| z_var.clone());
                self.emit(format!("{z_steps_var} = {z_steps}"));
                self.emit(format!("{theta_steps_var} = {theta_steps}"));
                self.emit(format!("{sections} = []"));
                self.emit(format!("for {zi_var} in range({z_steps_var} + 1):"));
                self.emit(format!(
                    "    {fz_var} = 0.0 if {z_steps_var} <= 0 else float({zi_var}) / float({z_steps_var})"
                ));
                self.emit(format!("    {z_var} = ({height}) * {fz_var}"));
                self.emit(format!("    {section_z_var} = float({z_map_expr})"));
                self.emit(format!("    {points_var} = []"));
                self.emit(format!("    for {ti_var} in range({theta_steps_var}):"));
                self.emit(format!(
                    "        {theta_var} = (2.0 * math.pi * float({ti_var})) / float({theta_steps_var})"
                ));
                self.emit(format!("        {radius_var} = float({radius_expr})"));
                self.emit(format!(
                    "        if {radius_var} <= 0.0: raise ValueError('sampled-radial-loft radius must stay positive')"
                ));
                self.emit(format!(
                    "        {points_var}.append(App.Vector({radius_var} * math.cos({theta_var}), {radius_var} * math.sin({theta_var}), {section_z_var}))"
                ));
                self.emit(format!("    {points_var}.append({points_var}[0])"));
                self.emit(format!(
                    "    {sections}.append(Part.Wire(Part.makePolygon({points_var})))"
                ));
                self.emit(format!("{result} = _ecky_loft({sections})"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "helical-ridge" => {
                let call = parse_helical_ridge_call(args)?;
                let radius = self.lower_num_expr(&call.radius, scope)?;
                let pitch = self.lower_num_expr(&call.pitch, scope)?;
                let height = self.lower_num_expr(&call.height, scope)?;
                let base_width = self.lower_num_expr(&call.base_width, scope)?;
                let crest_width = self.lower_num_expr(&call.crest_width, scope)?;
                let depth = self.lower_num_expr(&call.depth, scope)?;
                let female = call
                    .female
                    .as_ref()
                    .map(|value| self.lower_bool_expr(value, scope))
                    .transpose()?
                    .unwrap_or_else(|| "False".to_string());
                let clearance = call
                    .clearance
                    .as_ref()
                    .map(|value| self.lower_num_expr(value, scope))
                    .transpose()?
                    .unwrap_or_else(|| "0.0".to_string());
                let lefthand = call
                    .lefthand
                    .as_ref()
                    .map(|value| self.lower_bool_expr(value, scope))
                    .transpose()?
                    .unwrap_or_else(|| "False".to_string());
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_helical_ridge({radius}, {pitch}, {height}, {base_width}, {crest_width}, {depth}, female={female}, clearance={clearance}, lefthand={lefthand})"
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "shell" => {
                let parsed = ParsedCallArgs::parse(op, args, &["faces"])?;
                if parsed.positional.len() != 2 {
                    return Err(validation(
                        "`shell` expects wall thickness and a geometry node.",
                    ));
                }
                let wall = self.lower_num_expr(&parsed.positional[0], scope)?;
                let solid = self.lower_geom_expr(&parsed.positional[1], scope)?;
                let selector = self.lower_face_selector(parsed.keywords.get("faces"))?;
                let object_name = self.current_part_id.clone().ok_or_else(|| {
                    validation("FreeCAD lowerer lost current part id for shell face selector.")
                })?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_shell({}, {}, {}, {:?})",
                    solid.expr,
                    wall,
                    selector.as_deref().unwrap_or("None"),
                    object_name
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "sweep" => {
                if args.len() != 2 {
                    return Err(validation("`sweep` expects a profile and a path."));
                }
                let profile = self.lower_geom_expr(&args[0], scope)?;
                let path = self.lower_geom_expr(&args[1], scope)?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_sweep({}, {})",
                    profile.expr, path.expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "offset" | "offset-rounded" => {
                let parsed = ParsedCallArgs::parse(op, args, &["openings"])?;
                if parsed.positional.len() != 2 {
                    return Err(validation(format!(
                        "`{}` expects distance, optional `:openings`, and a sketch.",
                        op
                    )));
                }
                let distance = self.lower_num_expr(&parsed.positional[0], scope)?;
                let sketch = self.lower_geom_expr(&parsed.positional[1], scope)?;
                if sketch.kind != GeomKind::Sketch2d {
                    return Err(unsupported(format!(
                        "`{}` expects a 2D sketch, got {}.",
                        op,
                        sketch.kind.noun()
                    )));
                }
                let openings = parsed
                    .keywords
                    .get("openings")
                    .map(|value| self.lower_wire_collection(value, scope))
                    .transpose()?;
                let openings_expr = openings
                    .map(|items| format!("[{}]", items.join(", ")))
                    .unwrap_or_else(|| "None".to_string());
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_offset({}, {}, {})",
                    sketch.expr, distance, openings_expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "text" => {
                if args.len() < 2 {
                    return Err(validation("`text` expects string and size."));
                }
                let txt = self.lower_stringish_expr(&args[0], scope)?;
                let size = self.lower_num_expr(&args[1], scope)?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_text({}, {})", txt, size));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "svg" => {
                if args.is_empty() || args.len() > 4 {
                    return Err(validation(
                        "`svg` expects a file path, optional target width/height, and optional fit mode.",
                    ));
                }
                let path = self.lower_stringish_expr(&args[0], scope)?;
                let target_width = args
                    .get(1)
                    .map(|value| self.lower_num_expr(value, scope))
                    .transpose()?
                    .unwrap_or_else(|| "None".to_string());
                let target_height = args
                    .get(2)
                    .map(|value| self.lower_num_expr(value, scope))
                    .transpose()?
                    .unwrap_or_else(|| "None".to_string());
                let fit_mode = args
                    .get(3)
                    .map(|value| self.lower_stringish_expr(value, scope))
                    .transpose()?
                    .unwrap_or_else(|| "'contain'".to_string());
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_svg({}, {}, {}, {})",
                    path, target_width, target_height, fit_mode
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Sketch2d,
                });
            }
            "import-stl" | "import_stl" => {
                if args.len() != 1 {
                    return Err(validation("`import-stl` expects a file path."));
                }
                let path = self.lower_stringish_expr(&args[0], scope)?;
                let result = self.next_var();
                self.emit(format!("{result} = _ecky_import_stl({})", path));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "fillet" | "chamfer" => {
                let parsed = ParsedCallArgs::parse(op, args, &["edges"])?;
                if parsed.positional.len() != 2 {
                    return Err(validation(
                        "`fillet` and `chamfer` expect radius and a geometry node.",
                    ));
                }
                let radius = self.lower_num_expr(&parsed.positional[0], scope)?;
                let selector = self.lower_edge_selector(parsed.keywords.get("edges"))?;
                let solid = self.lower_geom_expr(&parsed.positional[1], scope)?;
                let result = self.next_var();
                let helper = if op == "fillet" {
                    "_ecky_fillet"
                } else {
                    "_ecky_chamfer"
                };
                let object_name = self.current_part_id.clone().ok_or_else(|| {
                    validation("FreeCAD lowerer lost current part id for edge selector.")
                })?;
                self.emit(format!(
                    "{result} = {}({}, {}, {}, {:?})",
                    helper, solid.expr, radius, selector, object_name
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "clip-box" => {
                let parsed = ParsedCallArgs::parse("clip-box", args, &["x", "y", "z"])?;
                if parsed.positional.len() != 1 {
                    return Err(validation(
                        "`clip-box` expects a geometry node and axis ranges.",
                    ));
                }
                let solid = self.lower_geom_expr(&parsed.positional[0], scope)?;
                let (xmin, xmax) = self.lower_range_pair(
                    parsed
                        .keywords
                        .get("x")
                        .ok_or_else(|| validation("`clip-box` requires `:x`."))?,
                    scope,
                    "clip-box :x",
                )?;
                let (ymin, ymax) = self.lower_range_pair(
                    parsed
                        .keywords
                        .get("y")
                        .ok_or_else(|| validation("`clip-box` requires `:y`."))?,
                    scope,
                    "clip-box :y",
                )?;
                let (zmin, zmax) = self.lower_range_pair(
                    parsed
                        .keywords
                        .get("z")
                        .ok_or_else(|| validation("`clip-box` requires `:z`."))?,
                    scope,
                    "clip-box :z",
                )?;
                let result = self.next_var();
                self.emit(format!(
                    "{result} = _ecky_clip_box({}, {}, {}, {}, {}, {}, {})",
                    solid.expr, xmin, xmax, ymin, ymax, zmin, zmax
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Solid3d,
                });
            }
            "place" => {
                let parsed = ParsedCallArgs::parse("place", args, &["offset", "rotate"])?;
                if parsed.positional.len() != 2 {
                    return Err(validation(
                        "`place` expects a frame, a geometry node, and optional `:offset` / `:rotate`.",
                    ));
                }
                let frame = self.lower_frame_expr(&parsed.positional[0], scope)?;
                let geom = self.lower_geom_expr(&parsed.positional[1], scope)?;
                let offset = parsed
                    .keywords
                    .get("offset")
                    .map(|v| self.lower_vec3(v, scope))
                    .transpose()?;
                let rotate = parsed
                    .keywords
                    .get("rotate")
                    .map(|v| self.lower_vec3(v, scope))
                    .transpose()?;
                let result = self.next_var();
                let offset = offset
                    .map(|(x, y, z)| format!("({}, {}, {})", x, y, z))
                    .unwrap_or_else(|| "(0.0, 0.0, 0.0)".to_string());
                let rotate = rotate
                    .map(|(x, y, z)| format!("({}, {}, {})", x, y, z))
                    .unwrap_or_else(|| "(0.0, 0.0, 0.0)".to_string());
                self.emit(format!(
                    "{result} = _ecky_place({frame}, {}, {offset}, {rotate})",
                    geom.expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: geom.kind,
                });
            }
            "linear-array" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`linear-array` expects count, dx, dy, dz, and a shape.",
                    ));
                }
                let count = self.lower_count_expr(&args[0], scope, 1)?;
                let dx = self.lower_num_expr(&args[1], scope)?;
                let dy = self.lower_num_expr(&args[2], scope)?;
                let dz = self.lower_num_expr(&args[3], scope)?;
                let base = self.lower_geom_expr(&args[4], scope)?;
                let helper = if base.kind == GeomKind::Solid3d {
                    "_ecky_fuse_many"
                } else {
                    "_ecky_union_many"
                };
                let result = self.next_var();
                self.emit(format!("{result} = {}", base.expr));
                self.emit(format!("for __ecky_la_i in range(1, {count}):"));
                self.emit(format!(
                    "    {result} = {helper}({result}, _ecky_translate({}, (({dx}) * __ecky_la_i, ({dy}) * __ecky_la_i, ({dz}) * __ecky_la_i)))",
                    base.expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: base.kind,
                });
            }
            "radial-array" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`radial-array` expects count, step degrees, radius, and a shape.",
                    ));
                }
                let count = self.lower_count_expr(&args[0], scope, 1)?;
                let step_deg = self.lower_num_expr(&args[1], scope)?;
                let radius = self.lower_num_expr(&args[2], scope)?;
                let base = self.lower_geom_expr(&args[3], scope)?;
                let helper = if base.kind == GeomKind::Solid3d {
                    "_ecky_fuse_many"
                } else {
                    "_ecky_union_many"
                };
                let translated = self.next_var();
                let result = self.next_var();
                self.emit(format!(
                    "{translated} = _ecky_translate({}, ({radius}, 0.0, 0.0))",
                    base.expr
                ));
                self.emit(format!("{result} = {translated}"));
                self.emit(format!("for __ecky_ra_i in range(1, {count}):"));
                self.emit(format!(
                    "    {result} = {helper}({result}, _ecky_rotate({translated}, (0.0, 0.0, ({step_deg}) * __ecky_ra_i)))"
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: base.kind,
                });
            }
            "grid-array" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`grid-array` expects rows, cols, dx, dy, and a shape.",
                    ));
                }
                let rows = self.lower_count_expr(&args[0], scope, 1)?;
                let cols = self.lower_count_expr(&args[1], scope, 1)?;
                let dx = self.lower_num_expr(&args[2], scope)?;
                let dy = self.lower_num_expr(&args[3], scope)?;
                let base = self.lower_geom_expr(&args[4], scope)?;
                let helper = if base.kind == GeomKind::Solid3d {
                    "_ecky_fuse_many"
                } else {
                    "_ecky_union_many"
                };
                let result = self.next_var();
                self.emit(format!("{result} = {}", base.expr));
                self.emit(format!("for __ecky_ga_r in range({rows}):"));
                self.emit(format!("    for __ecky_ga_c in range({cols}):"));
                self.emit(format!(
                    "        if __ecky_ga_r != 0 or __ecky_ga_c != 0: {result} = {helper}({result}, _ecky_translate({}, (({dx}) * __ecky_ga_c, ({dy}) * __ecky_ga_r, 0.0)))",
                    base.expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: base.kind,
                });
            }
            "arc-array" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`arc-array` expects count, radius, start degrees, end degrees, and a shape.",
                    ));
                }
                let count = self.lower_count_expr(&args[0], scope, 1)?;
                let radius = self.lower_num_expr(&args[1], scope)?;
                let start_deg = self.lower_num_expr(&args[2], scope)?;
                let end_deg = self.lower_num_expr(&args[3], scope)?;
                let base = self.lower_geom_expr(&args[4], scope)?;
                let helper = if base.kind == GeomKind::Solid3d {
                    "_ecky_fuse_many"
                } else {
                    "_ecky_union_many"
                };
                let first = self.next_var();
                let result = self.next_var();
                self.emit(format!(
                    "__ecky_arc_step = (({end_deg}) - ({start_deg})) / max(1, {count} - 1)"
                ));
                self.emit(format!(
                    "{first} = _ecky_rotate(_ecky_translate({}, ({radius}, 0.0, 0.0)), (0.0, 0.0, {start_deg}))",
                    base.expr
                ));
                self.emit(format!("{result} = {first}"));
                self.emit(format!("for __ecky_aa_i in range(1, {count}):"));
                self.emit(format!(
                    "    {result} = {helper}({result}, _ecky_rotate(_ecky_translate({}, ({radius}, 0.0, 0.0)), (0.0, 0.0, ({start_deg}) + __ecky_arc_step * __ecky_aa_i)))",
                    base.expr
                ));
                return Ok(LoweredNode {
                    expr: result,
                    kind: base.kind,
                });
            }
            "plane" | "location" | "path-frame" => {
                let frame = self.lower_frame_expr(value, scope)?;
                let result = self.next_var();
                self.emit(format!("{result} = {frame}"));
                return Ok(LoweredNode {
                    expr: result,
                    kind: GeomKind::Frame,
                });
            }
            "if" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`if` expects condition, then-shape, else-shape.",
                    ));
                }
                let cond = self.lower_bool_expr(&args[0], scope)?;
                let (then_lines, then_result, then_kind, next_counter) =
                    self.lower_geom_expr_locally(&args[1], scope)?;
                let (else_lines, else_result, else_kind, next_counter2) =
                    self.lower_geom_expr_locally(&args[2], scope)?;
                if then_kind != else_kind {
                    return Err(unsupported(format!(
                        "Node `if` requires matching branch kinds, got {} and {}.",
                        then_kind.noun(),
                        else_kind.noun()
                    )));
                }
                let result = self.next_var();
                let mut lines = vec![format!("if {cond}:")];
                lines.extend(then_lines.into_iter().map(|line| format!("    {line}")));
                lines.push(format!("    {result} = {then_result}"));
                lines.push("else:".to_string());
                lines.extend(else_lines.into_iter().map(|line| format!("    {line}")));
                lines.push(format!("    {result} = {else_result}"));
                self.lines.extend(lines);
                self.counter = self.counter.max(next_counter).max(next_counter2);
                return Ok(LoweredNode {
                    expr: result,
                    kind: then_kind,
                });
            }
            other => {
                return Err(unsupported(format!(
                    "Node `{}` is not yet supported by the FreeCAD lowerer.",
                    other
                )));
            }
        }
    }

    fn lower_point_list(
        &self,
        value: &IrExpr,
        scope: &LoweringScope,
        allow_3d: bool,
    ) -> AppResult<String> {
        let items = expr_list_items(value, "point list")?;
        let mut points = Vec::new();
        for item in items {
            if allow_3d {
                if let Ok(expr) = self.lower_point3(item, scope) {
                    points.push(expr);
                    continue;
                }
            }
            if let Ok(expr) = self.lower_point2(item, scope) {
                points.push(expr);
                continue;
            }
            let item_expr = item
                .as_list()
                .ok_or_else(|| validation("Point must be a list."))?;
            if !allow_3d && item_expr.len() == 3 {
                points.push(self.lower_point2(
                    &IrExpr::list(vec![item_expr[0].clone(), item_expr[1].clone()]),
                    scope,
                )?);
                continue;
            }
            return Err(validation("Points must be 2D pairs or 3D triples."));
        }
        Ok(points.join(", "))
    }

    fn lower_wire_operand(&mut self, value: &IrExpr, scope: &LoweringScope) -> AppResult<String> {
        if let Ok(sketch) = self.lower_geom_expr(value, scope) {
            if sketch.kind != GeomKind::Sketch2d {
                return Err(unsupported(format!(
                    "Expected 2D sketch for wire operand, got {}.",
                    sketch.kind.noun()
                )));
            }
            return Ok(format!("_ecky_as_wire({})", sketch.expr));
        }
        let points = self.lower_point_list(value, scope, false)?;
        Ok(format!("_ecky_polygon([{points}])"))
    }

    fn lower_wire_collection(
        &mut self,
        value: &IrExpr,
        scope: &LoweringScope,
    ) -> AppResult<Vec<String>> {
        let items = expr_list_items(value, "wire collection")?;
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let is_node = items
            .first()
            .and_then(IrExpr::as_symbol)
            .map(|s| !s.starts_with(':'))
            .unwrap_or(false);
        if is_node {
            return Ok(vec![self.lower_wire_operand(value, scope)?]);
        }
        let is_single_loop = items
            .first()
            .and_then(IrExpr::as_list)
            .map(|pair| {
                pair.len() == 2
                    && pair.iter().all(|value| {
                        value.as_f64().is_some()
                            || value.as_symbol().is_some()
                            || value.as_str().is_some()
                    })
            })
            .unwrap_or(false);
        if is_single_loop {
            return Ok(vec![self.lower_wire_operand(value, scope)?]);
        }
        items
            .iter()
            .map(|item| self.lower_wire_operand(item, scope))
            .collect()
    }

    fn lower_num_list(&self, value: &IrExpr, scope: &LoweringScope) -> AppResult<Vec<String>> {
        let items = expr_list_items(value, "numeric list")?;
        items
            .iter()
            .map(|item| self.lower_num_expr(item, scope))
            .collect()
    }

    fn lower_count_expr(
        &self,
        value: &IrExpr,
        scope: &LoweringScope,
        minimum: usize,
    ) -> AppResult<String> {
        if let Some(n) = value.as_f64() {
            return Ok((n.round().max(minimum as f64) as usize).to_string());
        }
        let expr = self.lower_num_expr(value, scope)?;
        Ok(format!("max({}, int(round(float({expr}))))", minimum))
    }

    fn same_kind(&self, operands: &[LoweredNode]) -> AppResult<GeomKind> {
        let kind = operands
            .first()
            .map(|node| node.kind.clone())
            .ok_or_else(|| validation("expected at least one operand"))?;
        if operands.iter().any(|node| node.kind != kind) {
            return Err(unsupported(
                "Mixed geometry kinds are not supported by the FreeCAD lowerer.".to_string(),
            ));
        }
        Ok(kind)
    }
}

fn fmt_f64(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}.0", n as i64)
    } else {
        format!("{}", n)
    }
}

fn extract_let_binding_hint(pair: &[IrExpr]) -> Option<CoreValueKind> {
    if pair.len() == 4 {
        expr_keyword_name(&pair[2])
            .filter(|k| *k == "value-kind")
            .and_then(|_| pair[3].as_symbol())
            .and_then(parse_value_kind_tag)
    } else {
        None
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

fn freecad_preamble() -> Vec<String> {
    vec![
        "import FreeCAD as App".into(),
        "import Part".into(),
        "import math".into(),
        "import os".into(),
        String::new(),
        "def _ecky_fract01(value):\n    value = float(value)\n    wrapped = value - math.floor(value)\n    return max(0.0, min(1.0, wrapped))".into(),
        "def _ecky_hash01(x, y, seed):\n    raw = math.sin(float(x) * 127.1 + float(y) * 311.7 + float(seed) * 74.7) * 43758.5453123\n    return _ecky_fract01(raw)".into(),
        "def _ecky_hash_signed(x, y, seed):\n    return _ecky_hash01(x, y, seed) * 2.0 - 1.0".into(),
        "def _ecky_smoothstep01(x):\n    t = max(0.0, min(1.0, float(x)))\n    return t * t * (3.0 - 2.0 * t)".into(),
        "def _ecky_noise2(x, y, seed):\n    x0 = math.floor(float(x)); y0 = math.floor(float(y))\n    xf = float(x) - x0; yf = float(y) - y0\n    n00 = _ecky_hash01(x0, y0, seed); n10 = _ecky_hash01(x0 + 1.0, y0, seed)\n    n01 = _ecky_hash01(x0, y0 + 1.0, seed); n11 = _ecky_hash01(x0 + 1.0, y0 + 1.0, seed)\n    sx = _ecky_smoothstep01(xf); sy = _ecky_smoothstep01(yf)\n    ix0 = n00 + (n10 - n00) * sx; ix1 = n01 + (n11 - n01) * sx\n    return max(0.0, min(1.0, ix0 + (ix1 - ix0) * sy))".into(),
        "def _ecky_fbm2(x, y, seed, octaves, lacunarity, gain):\n    octaves = max(1, min(12, int(round(float(octaves)))))\n    lacunarity = max(0.0001, float(lacunarity)); gain = max(0.0, min(1.0, float(gain)))\n    amp = 0.5; freq = 1.0; total = 0.0; norm = 0.0\n    for octave in range(octaves):\n        total += _ecky_noise2(float(x) * freq, float(y) * freq, float(seed) + octave * 17.0) * amp\n        norm += amp; amp *= gain; freq *= lacunarity\n    return 0.0 if norm <= 1e-12 else max(0.0, min(1.0, total / norm))".into(),
        "def _ecky_cell_distance2(x, y, seed):\n    cx = math.floor(float(x)); cy = math.floor(float(y)); best = float('inf')\n    for oy in (-1, 0, 1):\n        for ox in (-1, 0, 1):\n            gx = cx + ox; gy = cy + oy\n            px = gx + _ecky_hash01(gx, gy, seed)\n            py = gy + _ecky_hash01(gx + 19.19, gy + 7.73, float(seed) + 31.0)\n            best = min(best, math.hypot(float(x) - px, float(y) - py))\n    return max(0.0, min(1.0, best / math.sqrt(2.0)))".into(),
        "def _ecky_voronoi2(x, y, seed):\n    return max(0.0, min(1.0, 1.0 - _ecky_cell_distance2(x, y, seed)))".into(),
        "def _ecky_signed_pow(value, exponent):\n    value = float(value); exponent = float(exponent)\n    return math.copysign(abs(value) ** exponent, value)".into(),
        String::new(),
        "def _ecky_axis_offset(size, align):".into(),
        "    if align == \"min\":".into(),
        "        return 0.0".into(),
        "    if align == \"center\":".into(),
        "        return -size / 2.0".into(),
        "    return -size".into(),
        String::new(),
        "def _ecky_center_offset(size, align):".into(),
        "    if align == \"min\":".into(),
        "        return size / 2.0".into(),
        "    if align == \"center\":".into(),
        "        return 0.0".into(),
        "    return -size / 2.0".into(),
        String::new(),
        "def _ecky_vector(x, y, z=0.0):".into(),
        "    return App.Vector(float(x), float(y), float(z))".into(),
        String::new(),
        "def _ecky_box(width, depth, height, align=(\"center\", \"center\", \"min\")):\n    shape = Part.makeBox(float(width), float(depth), float(height))\n    shape.translate(App.Vector(\n        _ecky_axis_offset(float(width), align[0]),\n        _ecky_axis_offset(float(depth), align[1]),\n        _ecky_axis_offset(float(height), align[2]),\n    ))\n    return shape".into(),
        String::new(),
        "def _ecky_sphere(radius, align=(\"center\", \"center\", \"center\")):\n    shape = Part.makeSphere(float(radius), App.Vector(\n        _ecky_center_offset(float(radius) * 2.0, align[0]),\n        _ecky_center_offset(float(radius) * 2.0, align[1]),\n        _ecky_center_offset(float(radius) * 2.0, align[2]),\n    ))\n    return shape".into(),
        String::new(),
        "def _ecky_cylinder(radius, height, align=(\"center\", \"center\", \"min\")):\n    shape = Part.makeCylinder(float(radius), float(height), App.Vector(\n        _ecky_center_offset(float(radius) * 2.0, align[0]),\n        _ecky_center_offset(float(radius) * 2.0, align[1]),\n        _ecky_axis_offset(float(height), align[2]),\n    ))\n    return shape".into(),
        String::new(),
        "def _ecky_cone(br, tr, height, align=(\"center\", \"center\", \"min\")):\n    span = max(float(br), float(tr)) * 2.0\n    shape = Part.makeCone(float(br), float(tr), float(height), App.Vector(\n        _ecky_center_offset(span, align[0]),\n        _ecky_center_offset(span, align[1]),\n        _ecky_axis_offset(float(height), align[2]),\n    ))\n    return shape".into(),
        String::new(),
        "def _ecky_circle(radius):\n    circle = Part.Circle(App.Vector(0, 0, 0), App.Vector(0, 0, 1), float(radius))\n    return Part.Wire(circle.toShape())".into(),
        String::new(),
        "def _ecky_rounded_rect(width, height, radius):\n    w = float(width)\n    h = float(height)\n    r = min(float(radius), abs(w) / 2.0, abs(h) / 2.0)\n    x0 = -w / 2.0\n    y0 = -h / 2.0\n    x1 = w / 2.0\n    y1 = h / 2.0\n    if r <= 1e-12:\n        return _ecky_polygon([App.Vector(x0, y0, 0), App.Vector(x1, y0, 0), App.Vector(x1, y1, 0), App.Vector(x0, y1, 0)])\n    edges = [\n        Part.LineSegment(App.Vector(x0 + r, y0, 0), App.Vector(x1 - r, y0, 0)).toShape(),\n        Part.Arc(App.Vector(x1 - r, y0, 0), App.Vector(x1, y0, 0), App.Vector(x1, y0 + r, 0)).toShape(),\n        Part.LineSegment(App.Vector(x1, y0 + r, 0), App.Vector(x1, y1 - r, 0)).toShape(),\n        Part.Arc(App.Vector(x1, y1 - r, 0), App.Vector(x1, y1, 0), App.Vector(x1 - r, y1, 0)).toShape(),\n        Part.LineSegment(App.Vector(x1 - r, y1, 0), App.Vector(x0 + r, y1, 0)).toShape(),\n        Part.Arc(App.Vector(x0 + r, y1, 0), App.Vector(x0, y1, 0), App.Vector(x0, y1 - r, 0)).toShape(),\n        Part.LineSegment(App.Vector(x0, y1 - r, 0), App.Vector(x0, y0 + r, 0)).toShape(),\n        Part.Arc(App.Vector(x0, y0 + r, 0), App.Vector(x0, y0, 0), App.Vector(x0 + r, y0, 0)).toShape(),\n    ]\n    return Part.Wire(edges)".into(),
        String::new(),
        "def _ecky_rounded_polygon(points, radius):\n    pts = list(points)\n    if len(pts) >= 2 and pts[0] == pts[-1]:\n        pts = pts[:-1]\n    if len(pts) < 3:\n        raise ValueError('`rounded-polygon` expects at least three points.')\n    req_r = abs(float(radius))\n    if req_r <= 1e-12:\n        return _ecky_polygon(pts)\n    def _xy(v):\n        return (float(v.x), float(v.y))\n    def _sub(a, b):\n        return (a[0] - b[0], a[1] - b[1])\n    def _add(a, b):\n        return (a[0] + b[0], a[1] + b[1])\n    def _mul(a, scalar):\n        return (a[0] * scalar, a[1] * scalar)\n    def _len(a):\n        return math.hypot(a[0], a[1])\n    def _norm(a):\n        length = _len(a)\n        if length <= 1e-12:\n            raise ValueError('`rounded-polygon` got a zero-length edge.')\n        return (a[0] / length, a[1] / length)\n    corners = []\n    count = len(pts)\n    for index in range(count):\n        prev = _xy(pts[index - 1])\n        curr = _xy(pts[index])\n        nxt = _xy(pts[(index + 1) % count])\n        in_vec = _sub(prev, curr)\n        out_vec = _sub(nxt, curr)\n        len_in = _len(in_vec)\n        len_out = _len(out_vec)\n        if len_in <= 1e-12 or len_out <= 1e-12:\n            raise ValueError('`rounded-polygon` got a zero-length edge.')\n        in_dir = _norm(in_vec)\n        out_dir = _norm(out_vec)\n        dot = max(-1.0, min(1.0, in_dir[0] * out_dir[0] + in_dir[1] * out_dir[1]))\n        theta = math.acos(dot)\n        tan_half = math.tan(theta / 2.0) if theta > 1e-12 else 0.0\n        bisector = _add(in_dir, out_dir)\n        bisector_len = _len(bisector)\n        if tan_half <= 1e-12 or bisector_len <= 1e-12:\n            corners.append((curr, curr, curr, False))\n            continue\n        corner_r = min(req_r, min(len_in, len_out) * tan_half)\n        if corner_r <= 1e-12:\n            corners.append((curr, curr, curr, False))\n            continue\n        tangent = corner_r / tan_half\n        bisector = _mul(bisector, 1.0 / bisector_len)\n        center_dist = corner_r / math.sin(theta / 2.0)\n        p_in = _add(curr, _mul(in_dir, tangent))\n        p_out = _add(curr, _mul(out_dir, tangent))\n        center = _add(curr, _mul(bisector, center_dist))\n        mid_dir = _sub(curr, center)\n        mid_len = _len(mid_dir)\n        if mid_len <= 1e-12:\n            corners.append((curr, curr, curr, False))\n            continue\n        mid = _add(center, _mul(mid_dir, corner_r / mid_len))\n        corners.append((p_in, p_out, mid, True))\n    edges = []\n    for index in range(count):\n        _, p_out, _, _ = corners[index]\n        p_in_next, p_out_next, mid_next, rounded_next = corners[(index + 1) % count]\n        line_start = App.Vector(p_out[0], p_out[1], 0)\n        line_end = App.Vector(p_in_next[0], p_in_next[1], 0)\n        if line_start.distanceToPoint(line_end) > 1e-9:\n            edges.append(Part.LineSegment(line_start, line_end).toShape())\n        if rounded_next:\n            edges.append(\n                Part.Arc(\n                    App.Vector(p_in_next[0], p_in_next[1], 0),\n                    App.Vector(mid_next[0], mid_next[1], 0),\n                    App.Vector(p_out_next[0], p_out_next[1], 0),\n                ).toShape()\n            )\n    if not edges:\n        return _ecky_polygon(pts)\n    return Part.Wire(edges)".into(),
        String::new(),
        "def _ecky_polygon(points):\n    pts = list(points)\n    if len(pts) >= 2 and pts[0] != pts[-1]:\n        pts.append(pts[0])\n    return Part.makePolygon(pts)".into(),
        String::new(),
        "def _ecky_path(points):\n    return Part.makePolygon(list(points))".into(),
        String::new(),
        "def _ecky_bezier_path(points):\n    pts = list(points)\n    if len(pts) < 4 or (len(pts) - 1) % 3 != 0:\n        raise ValueError(f'`bezier-path` expects 3n+1 control points (4, 7, 10, ...), got {len(pts)}.')\n    edges = []\n    for index in range(0, len(pts) - 1, 3):\n        curve = Part.BezierCurve()\n        curve.setPoles(pts[index:index + 4])\n        edges.append(curve.toShape())\n    return Part.Wire(edges)".into(),
        String::new(),
        "def _ecky_bspline(points, closed=False, tangents=None, tangent_scalars=None):\n    pts = list(points)\n    curve = Part.BSplineCurve()\n    kwargs = {}\n    if tangents and len(tangents) >= 2:\n        start = [float(v) for v in tangents[0]]\n        end = [float(v) for v in tangents[-1]]\n        if tangent_scalars and len(tangent_scalars) >= 2:\n            start_scale = float(tangent_scalars[0])\n            end_scale = float(tangent_scalars[-1])\n            start = [value * start_scale for value in start]\n            end = [value * end_scale for value in end]\n        kwargs['InitialTangent'] = App.Vector(*start)\n        kwargs['FinalTangent'] = App.Vector(*end)\n    try:\n        if kwargs:\n            curve.interpolate(pts, **kwargs)\n        else:\n            curve.interpolate(pts)\n    except TypeError:\n        curve.interpolate(pts)\n    if closed and pts:\n        try:\n            curve.setPeriodic()\n        except Exception:\n            try:\n                curve.closed = True\n            except Exception:\n                pass\n    return Part.Wire(curve.toShape())".into(),
        String::new(),
        "def _ecky_default_font_path():\n    explicit = os.environ.get('ECKYCAD_FONT_PATH')\n    candidates = [explicit] if explicit else []\n    candidates.extend([\n        '/System/Library/Fonts/Supplemental/Arial Black.ttf',\n        '/System/Library/Fonts/Supplemental/Impact.ttf',\n        '/System/Library/Fonts/Supplemental/Arial Unicode.ttf',\n        '/System/Library/Fonts/Supplemental/Arial.ttf',\n        '/Library/Fonts/Arial.ttf',\n        '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',\n        'C:/Windows/Fonts/arial.ttf',\n    ])\n    for path in candidates:\n        if path and os.path.exists(path):\n            return path\n    raise RuntimeError('No usable font found for `text`. Set ECKYCAD_FONT_PATH to a .ttf file.')".into(),
        String::new(),
        "def _ecky_as_wire(shape):\n    if getattr(shape, \"ShapeType\", \"\") == \"Wire\":\n        return shape\n    if hasattr(shape, \"Wires\") and getattr(shape, \"Wires\", None):\n        return shape.Wires[0]\n    try:\n        return Part.Wire(shape.Edges)\n    except Exception:\n        return shape".into(),
        String::new(),
        "def _ecky_face(shape):\n    if getattr(shape, \"ShapeType\", \"\") == \"Face\":\n        return shape\n    faces = list(getattr(shape, 'Faces', []) or [])\n    if faces:\n        return faces[0] if len(faces) == 1 else Part.makeCompound(faces)\n    if isinstance(shape, (list, tuple)):\n        return Part.Face(shape)\n    wire = _ecky_as_wire(shape)\n    if getattr(wire, \"ShapeType\", \"\") == \"Face\":\n        return wire\n    return Part.Face(wire)".into(),
        String::new(),
        "def _ecky_face_with_holes(outer, holes):\n    loops = [_ecky_as_wire(outer)]\n    loops.extend(_ecky_as_wire(hole) for hole in list(holes or []))\n    return Part.Face(loops)".into(),
        String::new(),
        "def _ecky_text(value, size):\n    glyphs = Part.makeWireString(str(value), _ecky_default_font_path(), float(size))\n    faces = [_ecky_face(glyph) for glyph in glyphs]\n    if not faces:\n        raise ValueError('text produced no glyph faces')\n    return faces[0] if len(faces) == 1 else Part.makeCompound(faces)".into(),
        String::new(),
        "def _ecky_svg(path, target_width=None, target_height=None, fit_mode='contain'):\n    import importSVG\n    doc = App.ActiveDocument or App.newDocument('EckyCAD')\n    before = {obj.Name for obj in doc.Objects}\n    importSVG.insert(str(path), doc.Name)\n    doc.recompute()\n    imported = [obj for obj in doc.Objects if obj.Name not in before]\n    shapes = []\n    try:\n        for obj in imported:\n            shape = getattr(obj, 'Shape', None)\n            if shape is None:\n                continue\n            try:\n                if shape.isNull():\n                    continue\n            except Exception:\n                pass\n            try:\n                shapes.append(Part.Face(shape))\n            except Exception:\n                shapes.append(shape.copy())\n    finally:\n        for obj in imported:\n            try:\n                doc.removeObject(obj.Name)\n            except Exception:\n                pass\n        doc.recompute()\n    if not shapes:\n        raise ValueError(f'SVG import produced no shapes for {path}')\n    result = shapes[0] if len(shapes) == 1 else Part.makeCompound(shapes)\n    if target_width is None and target_height is None:\n        return result\n    bb = result.BoundBox\n    width = float(bb.XLength)\n    height = float(bb.YLength)\n    mode = str(fit_mode or 'contain').strip().lower()\n    if width <= 1e-9 or height <= 1e-9:\n        raise ValueError(f'SVG import has degenerate bounds for {path}')\n    sx = (float(target_width) / width) if target_width is not None else None\n    sy = (float(target_height) / height) if target_height is not None else None\n    fitted = result.copy()\n    fitted.translate(App.Vector(-((bb.XMin + bb.XMax) / 2.0), -((bb.YMin + bb.YMax) / 2.0), -bb.ZMin))\n    if mode in ('contain', 'fit'):\n        factor = min([scale for scale in (sx, sy) if scale is not None])\n        fitted.scale(factor)\n        return fitted\n    if mode == 'cover':\n        factor = max([scale for scale in (sx, sy) if scale is not None])\n        fitted.scale(factor)\n        return fitted\n    if mode in ('stretch', 'fill'):\n        matrix = App.Matrix()\n        matrix.A11 = sx if sx is not None else sy\n        matrix.A22 = sy if sy is not None else sx\n        matrix.A33 = 1.0\n        return fitted.transformGeometry(matrix)\n    raise ValueError(f'Unsupported SVG fit mode {fit_mode!r}; expected contain, cover, or stretch.')".into(),
        String::new(),
        "def _ecky_import_stl(path, tolerance=0.1):\n    import Mesh\n    mesh = Mesh.Mesh(str(path))\n    shape = Part.Shape()\n    shape.makeShapeFromMesh(mesh.Topology, float(tolerance))\n    solids = []\n    for shell in list(getattr(shape, 'Shells', []) or []):\n        try:\n            solids.append(Part.makeSolid(shell))\n        except Exception:\n            pass\n    if solids:\n        return solids[0] if len(solids) == 1 else Part.makeCompound(solids)\n    existing_solids = list(getattr(shape, 'Solids', []) or [])\n    if existing_solids:\n        return existing_solids[0] if len(existing_solids) == 1 else Part.makeCompound(existing_solids)\n    return shape".into(),
        String::new(),
        "def _ecky_non_null_items(items):\n    kept = []\n    for item in items:\n        if item is None:\n            continue\n        try:\n            if item.isNull():\n                continue\n        except Exception:\n            pass\n        kept.append(item)\n    return kept".into(),
        String::new(),
        "def _ecky_union_many(*items):\n    items = _ecky_non_null_items(items)\n    if not items:\n        raise ValueError('union needs at least one item')\n    if all(getattr(item, \"ShapeType\", \"\") in (\"Solid\", \"CompSolid\") for item in items):\n        result = items[0].copy()\n        for item in items[1:]:\n            result = result.fuse(item)\n        return result\n    edges = []\n    for item in items:\n        if hasattr(item, \"Edges\"):\n            edges.extend(list(item.Edges))\n        else:\n            edges.append(item)\n    try:\n        return Part.Wire(edges)\n    except Exception:\n        return Part.makeCompound(items)".into(),
        String::new(),
        "def _ecky_fuse_many(*items):\n    items = _ecky_non_null_items(items)\n    if not items:\n        return Part.Shape()\n    result = items[0].copy()\n    for item in items[1:]:\n        result = result.fuse(item)\n    return result".into(),
        String::new(),
        "def _ecky_cut_many(*items):\n    items = _ecky_non_null_items(items)\n    if not items:\n        return Part.Shape()\n    result = items[0].copy()\n    for item in items[1:]:\n        result = result.cut(item)\n    return result".into(),
        String::new(),
        "def _ecky_common_many(*items):\n    items = _ecky_non_null_items(items)\n    if not items:\n        return Part.Shape()\n    result = items[0].copy()\n    for item in items[1:]:\n        result = result.common(item)\n    return result".into(),
        String::new(),
        "def _ecky_xor_many(*items):\n    items = _ecky_non_null_items(items)\n    if not items:\n        return Part.Shape()\n    union = items[0].copy()\n    common = items[0].copy()\n    for item in items[1:]:\n        union = union.fuse(item)\n        common = common.common(item)\n    try:\n        if common.isNull():\n            return union\n    except Exception:\n        pass\n    return union.cut(common)".into(),
        String::new(),
        "def _ecky_compound(*items):\n    items = _ecky_non_null_items(items)\n    if not items:\n        return Part.Shape()\n    return Part.makeCompound(list(items))".into(),
        String::new(),
        "def _ecky_translate(shape, offset):\n    result = shape.copy()\n    result.translate(App.Vector(*[float(v) for v in offset]))\n    return result".into(),
        String::new(),
        "def _ecky_rotate(shape, rotate):\n    result = shape.copy()\n    rx, ry, rz = [float(v) for v in rotate]\n    if abs(rx) > 1e-12:\n        result.rotate(App.Vector(0, 0, 0), App.Vector(1, 0, 0), rx)\n    if abs(ry) > 1e-12:\n        result.rotate(App.Vector(0, 0, 0), App.Vector(0, 1, 0), ry)\n    if abs(rz) > 1e-12:\n        result.rotate(App.Vector(0, 0, 0), App.Vector(0, 0, 1), rz)\n    return result".into(),
        String::new(),
        "def _ecky_scale(shape, scale):\n    result = shape.copy()\n    sx, sy, sz = [float(v) for v in scale]\n    if abs(sx - sy) < 1e-9 and abs(sy - sz) < 1e-9:\n        result.scale(sx)\n        return result\n    matrix = App.Matrix()\n    matrix.A11 = sx\n    matrix.A22 = sy\n    matrix.A33 = sz\n    transformed = result.transformGeometry(matrix)\n    return transformed if transformed is not None else result".into(),
        String::new(),
        "def _ecky_mirror(shape, axis, offset):\n    result = shape.copy()\n    axis = axis.lower()\n    point = App.Vector(0, 0, 0)\n    normal = App.Vector(1, 0, 0)\n    if axis == 'y':\n        normal = App.Vector(0, 1, 0)\n    elif axis == 'z':\n        normal = App.Vector(0, 0, 1)\n    point = App.Vector(float(offset), 0, 0)\n    result.mirror(point, normal)\n    return result".into(),
        String::new(),
        "def _ecky_extrude(sketch, height, symmetric=False):\n    face = _ecky_face(sketch)\n    amount = float(height)\n    result = face.copy()\n    if symmetric:\n        result.translate(App.Vector(0, 0, -amount / 2.0))\n    return result.extrude(App.Vector(0, 0, amount))".into(),
        String::new(),
        "def _ecky_revolve(sketch, angle):\n    face = _ecky_face(sketch).copy()\n    face.rotate(App.Vector(0, 0, 0), App.Vector(1, 0, 0), 90.0)\n    return face.revolve(App.Vector(0, 0, 0), App.Vector(0, 0, 1), float(angle))".into(),
        String::new(),
        "def _ecky_loft(sections):\n    if len(sections) < 2:\n        raise ValueError('loft needs at least two sections')\n    wires = [_ecky_as_wire(section) for section in sections]\n    return Part.makeLoft(wires, True)".into(),
        String::new(),
        "def _ecky_sweep(profile, path):\n    profile = _ecky_as_wire(profile)\n    path = _ecky_as_wire(path)\n    return path.makePipeShell([profile], True, False)".into(),
        String::new(),
        "def _ecky_helical_ridge(radius, pitch, height, base_width, crest_width, depth, female=False, clearance=0.0, lefthand=False):\n    radius = float(radius); pitch = float(pitch); height = float(height)\n    base_width = float(base_width); crest_width = float(crest_width); depth = float(depth)\n    clearance = max(0.0, float(clearance))\n    female = bool(female); lefthand = bool(lefthand)\n    if radius <= 0.0: raise ValueError('helical-ridge radius must be positive')\n    if pitch <= 0.0: raise ValueError('helical-ridge pitch must be positive')\n    if height <= 0.0: raise ValueError('helical-ridge height must be positive')\n    if base_width <= 0.0: raise ValueError('helical-ridge base-width must be positive')\n    if crest_width <= 0.0: raise ValueError('helical-ridge crest-width must be positive')\n    if depth <= 0.0: raise ValueError('helical-ridge depth must be positive')\n    envelope_clearance = clearance if female else 0.0\n    path_radius = radius\n    base_half = (base_width + 2.0 * envelope_clearance) * 0.5\n    crest_half = (crest_width + 2.0 * envelope_clearance) * 0.5\n    ridge_depth = depth + envelope_clearance\n    helix = Part.makeHelix(pitch, height, path_radius)\n    if lefthand:\n        helix.mirror(App.Vector(0, 0, 0), App.Vector(0, 1, 0))\n    points = [\n        App.Vector(path_radius, 0, -base_half),\n        App.Vector(path_radius + ridge_depth, 0, -crest_half),\n        App.Vector(path_radius + ridge_depth, 0, crest_half),\n        App.Vector(path_radius, 0, base_half),\n        App.Vector(path_radius, 0, -base_half),\n    ]\n    profile = Part.Wire(Part.makePolygon(points))\n    return Part.Wire(helix).makePipeShell([profile], True, False)".into(),
        String::new(),
        "def _ecky_offset(shape, amount, openings=None):\n    base = shape\n    if openings:\n        base = _ecky_face_with_holes(shape, openings)\n    if getattr(base, 'ShapeType', '') == 'Face':\n        return base.makeOffset2D(float(amount))\n    return _ecky_as_wire(base).makeOffset2D(float(amount))".into(),
        String::new(),
        "def _ecky_planar_faces(shape):\n    faces = []\n    top = None\n    for face in list(getattr(shape, 'Faces', []) or []):\n        surface = getattr(face, 'Surface', None)\n        name = surface.__class__.__name__.lower() if surface is not None else ''\n        if 'plane' not in name:\n            continue\n        box = getattr(face, 'BoundBox', None)\n        zmax = float(box.ZMax) if box is not None else 0.0\n        if top is None or zmax > top + 1e-6:\n            faces = [face]\n            top = zmax\n        elif abs(zmax - top) <= 1e-6:\n            faces.append(face)\n    return faces".into(),
        String::new(),
        "def _ecky_face_target_id(face, face_index, object_name):\n    try:\n        center = getattr(face, 'CenterOfMass', None)\n        area = getattr(face, 'Area', None)\n        if center is not None and area is not None:\n            return f'{object_name}:face:{face_index}:{_ecky_point_signature((float(center.x), float(center.y), float(center.z)))}:{_ecky_number_signature(area)}'\n    except Exception:\n        pass\n    return f'{object_name}:face:{face_index}'".into(),
        String::new(),
        "def _ecky_stable_edge_target_id(target_id):\n    raw = str(target_id or '').strip()\n    marker = ':edge:'\n    if marker not in raw:\n        return raw\n    prefix, payload = raw.split(marker, 1)\n    parts = payload.split(':')\n    if len(parts) >= 2 and parts[0].isdigit():\n        return f'{prefix}{marker}{\":\".join(parts[1:])}'\n    return raw".into(),
        String::new(),
        "def _ecky_stable_face_target_id(target_id):\n    raw = str(target_id or '').strip()\n    marker = ':face:'\n    if marker not in raw:\n        return raw\n    prefix, payload = raw.split(marker, 1)\n    parts = payload.split(':')\n    if len(parts) >= 3 and parts[0].isdigit():\n        return f'{prefix}{marker}{\":\".join(parts[1:])}'\n    return raw".into(),
        String::new(),
        "_ECKY_FACE_SELECTOR_HELP = '`all`, `planar`, `normal-x`, `normal-y`, `normal-z`, `area-min`, `area-max`, `top`, `bottom`, `left`, `right`, `front`, `back`, `x-min`, `x-max`, `y-min`, `y-max`, `z-min`, `z-max`, `target-id:<id>`, `target-ids:<id>|<id>`, or `+` intersections such as `planar+normal-z+z-max`.'".into(),
        String::new(),
        "def _ecky_face_selector_error(selector):\n    raise ValueError(f'Unknown face selector `{selector}`. Use {_ECKY_FACE_SELECTOR_HELP}')".into(),
        String::new(),
        "def _ecky_face_selector_target_ids(selector):\n    if selector is None:\n        return None\n    if not isinstance(selector, dict):\n        raise ValueError(f'Face selector `{selector}` requires typed selector payload.')\n    if _ecky_selector_kind(selector) != 'targetIds':\n        return None\n    target_ids = []\n    for item in selector.get('targetIds') or []:\n        target_id = str(item).strip()\n        if target_id and ':face:' in target_id and target_id not in target_ids:\n            target_ids.append(target_id)\n    if not target_ids:\n        raise ValueError(f'Face selector `{selector}` did not include any face target ids.')\n    return target_ids".into(),
        String::new(),
        "def _ecky_face_selector_clauses(selector):\n    if selector is None:\n        return []\n    if not isinstance(selector, dict):\n        raise ValueError(f'Face selector `{selector}` requires typed selector payload.')\n    kind = _ecky_selector_kind(selector)\n    if kind == 'all':\n        return []\n    if kind != 'clauses':\n        _ecky_face_selector_error(selector)\n    clauses = []\n    for clause in selector.get('clauses') or []:\n        if not isinstance(clause, dict):\n            _ecky_face_selector_error(selector)\n        clause_kind = str(clause.get('kind') or '').strip().lower()\n        if clause_kind == 'boundary':\n            axis = str(clause.get('axis') or '').strip().lower()\n            bound = str(clause.get('bound') or '').strip().lower()\n            if axis not in ('x', 'y', 'z') or bound not in ('min', 'max'):\n                _ecky_face_selector_error(selector)\n            clauses.append(('boundary', axis, bound))\n            continue\n        if clause_kind == 'planar':\n            clauses.append(('planar',))\n            continue\n        if clause_kind == 'normal':\n            axis = str(clause.get('axis') or '').strip().lower()\n            if axis not in ('x', 'y', 'z'):\n                _ecky_face_selector_error(selector)\n            clauses.append(('normal', axis))\n            continue\n        if clause_kind == 'area':\n            rank = str(clause.get('rank') or '').strip().lower()\n            if rank not in ('min', 'max'):\n                _ecky_face_selector_error(selector)\n            clauses.append(('area', rank))\n            continue\n        _ecky_face_selector_error(selector)\n    return clauses".into(),
        String::new(),
        "def _ecky_face_matches_clause(face, box, clause, tol):\n    face_box = getattr(face, 'BoundBox', None)\n    if face_box is None:\n        return False\n    if clause[0] == 'planar':\n        surface = getattr(face, 'Surface', None)\n        name = surface.__class__.__name__.lower() if surface is not None else ''\n        return 'plane' in name\n    if clause[0] == 'normal':\n        surface = getattr(face, 'Surface', None)\n        name = surface.__class__.__name__.lower() if surface is not None else ''\n        if 'plane' not in name:\n            return False\n        axis = clause[1]\n        span = {'x': float(face_box.XLength), 'y': float(face_box.YLength), 'z': float(face_box.ZLength)}[axis]\n        return span <= tol\n    _, axis, bound = clause\n    target = {\n        ('x', 'min'): float(box.XMin),\n        ('x', 'max'): float(box.XMax),\n        ('y', 'min'): float(box.YMin),\n        ('y', 'max'): float(box.YMax),\n        ('z', 'min'): float(box.ZMin),\n        ('z', 'max'): float(box.ZMax),\n    }[(axis, bound)]\n    face_min = {\n        'x': float(face_box.XMin),\n        'y': float(face_box.YMin),\n        'z': float(face_box.ZMin),\n    }[axis]\n    face_max = {\n        'x': float(face_box.XMax),\n        'y': float(face_box.YMax),\n        'z': float(face_box.ZMax),\n    }[axis]\n    return abs(face_min - target) <= tol and abs(face_max - target) <= tol".into(),
        String::new(),
        "def _ecky_filter_faces_by_area(faces, rank):\n    if not faces:\n        return []\n    areas = [float(getattr(face, 'Area', 0.0)) for face in faces]\n    target = min(areas) if rank == 'min' else max(areas)\n    tol = max(abs(float(target)), 1.0) * 1e-6\n    return [face for face, area in zip(faces, areas) if abs(float(area) - target) <= tol]".into(),
        String::new(),
        "def _ecky_select_shell_faces(shape, selector, object_name=None):\n    faces = list(getattr(shape, 'Faces', []) or [])\n    if not faces:\n        raise ValueError('Shape has no faces for shell opening selection.')\n    target_ids = _ecky_face_selector_target_ids(selector)\n    if target_ids is not None:\n        if not object_name:\n            raise ValueError(f'Face selector `{selector}` requires an object name for exact target-id matching.')\n        face_records = []\n        stable_counts = {}\n        for face_index, face in enumerate(faces):\n            target_id = _ecky_face_target_id(face, face_index, object_name)\n            stable_id = _ecky_stable_face_target_id(target_id)\n            face_records.append((face, target_id, stable_id))\n            stable_counts[stable_id] = stable_counts.get(stable_id, 0) + 1\n        selected = []\n        matched = set()\n        for requested_target_id in target_ids:\n            exact = next((record for record in face_records if record[1] == requested_target_id), None)\n            if exact is not None:\n                if exact[1] not in matched:\n                    selected.append(exact[0])\n                    matched.add(exact[1])\n                continue\n            stable_requested = _ecky_stable_face_target_id(requested_target_id)\n            candidates = [record for record in face_records if record[2] == stable_requested]\n            if not candidates:\n                raise ValueError(f'Face selector `{selector}` did not match target ids: {[requested_target_id]}')\n            if len(candidates) > 1 or stable_counts.get(stable_requested, 0) > 1:\n                raise ValueError(f'Face selector `{selector}` ambiguously matched stable face target `{requested_target_id}`.')\n            candidate = candidates[0]\n            if candidate[1] not in matched:\n                selected.append(candidate[0])\n                matched.add(candidate[1])\n        return selected\n    clauses = _ecky_face_selector_clauses(selector)\n    if selector is None:\n        return None\n    if not clauses:\n        return faces\n    box = getattr(shape, 'BoundBox', None)\n    if box is None:\n        raise ValueError('Shape has no BoundBox for shell face selection.')\n    span = max(float(box.XLength), float(box.YLength), float(box.ZLength), 1.0)\n    tol = span * 1e-6\n    selected = list(faces)\n    for clause in clauses:\n        if clause[0] == 'area':\n            selected = _ecky_filter_faces_by_area(selected, clause[1])\n            continue\n        selected = [face for face in selected if _ecky_face_matches_clause(face, box, clause, tol)]\n    if not selected:\n        raise ValueError(f'Face selector `{selector}` matched no shell opening faces.')\n    return selected".into(),
        String::new(),
        "def _ecky_shell(shape, wall, face_selector=None, object_name=None):\n    amount = -abs(float(wall))\n    if amount >= 0.0:\n        raise ValueError('shell expects positive wall thickness')\n    selected_faces = _ecky_select_shell_faces(shape, face_selector, object_name)\n    planar = selected_faces if selected_faces is not None else _ecky_planar_faces(shape)\n    failures = []\n    if planar:\n        for tol in (0.05, 0.1):\n            try:\n                return shape.makeThickness(planar, amount, tol)\n            except Exception as exc:\n                failures.append(f'makeThickness({tol}) -> {exc}')\n        if selected_faces is not None:\n            raise ValueError('shell failed: ' + '; '.join(failures))\n    if selected_faces is not None:\n        raise ValueError(f'Face selector `{face_selector}` matched no shell opening faces.')\n    for tol in (0.05, 0.1):\n        try:\n            inner = shape.makeOffsetShape(amount, tol, join=2, fill=True)\n            return shape.cut(inner)\n        except Exception as exc:\n            failures.append(f'makeOffsetShape({tol}) -> {exc}')\n    raise ValueError('shell failed: ' + '; '.join(failures))".into(),
        String::new(),
        "def _ecky_edge_axis_span(edge):\n    box = getattr(edge, 'BoundBox', None)\n    if box is None:\n        raise ValueError('Edge has no BoundBox.')\n    return float(box.XLength), float(box.YLength), float(box.ZLength)".into(),
        String::new(),
        "_ECKY_EDGE_SELECTOR_HELP = '`all`, `top`, `bottom`, `left`, `right`, `front`, `back`, `vertical`, `axis-x`, `axis-y`, `axis-z`, `x-min`, `x-max`, `y-min`, `y-max`, `z-min`, `z-max`, `target-id:<id>`, `target-ids:<id>|<id>`, or `+` intersections such as `x-min+axis-z`.'".into(),
        String::new(),
        "def _ecky_edge_selector_error(selector):\n    raise ValueError(f'Unknown edge selector `{selector}`. Use {_ECKY_EDGE_SELECTOR_HELP}')".into(),
        String::new(),
        "def _ecky_selector_kind(selector):\n    if isinstance(selector, dict):\n        return str(selector.get('kind') or '').strip()\n    return ''".into(),
        String::new(),
        "def _ecky_selector_target_ids(selector):\n    if selector is None:\n        return None\n    if not isinstance(selector, dict):\n        raise ValueError(f'Edge selector `{selector}` requires typed selector payload.')\n    if _ecky_selector_kind(selector) != 'targetIds':\n        return None\n    target_ids = []\n    for item in selector.get('targetIds') or []:\n        text = str(item).strip()\n        if text and text not in target_ids:\n            target_ids.append(text)\n    if not target_ids:\n        raise ValueError(f'Edge selector `{selector}` did not include any target ids.')\n    return target_ids".into(),
        String::new(),
        "def _ecky_selector_clauses(selector):\n    if selector is None:\n        return []\n    if not isinstance(selector, dict):\n        raise ValueError(f'Edge selector `{selector}` requires typed selector payload.')\n    kind = _ecky_selector_kind(selector)\n    if kind == 'all':\n        return []\n    if kind != 'clauses':\n        _ecky_edge_selector_error(selector)\n    clauses = []\n    for clause in selector.get('clauses') or []:\n        if not isinstance(clause, dict):\n            _ecky_edge_selector_error(selector)\n        clause_kind = str(clause.get('kind') or '').strip()\n        if clause_kind == 'axis':\n            axis = str(clause.get('axis') or '').strip().lower()\n            if axis not in ('x', 'y', 'z'):\n                _ecky_edge_selector_error(selector)\n            clauses.append(('axis', axis))\n            continue\n        if clause_kind == 'boundary':\n            axis = str(clause.get('axis') or '').strip().lower()\n            bound = str(clause.get('bound') or '').strip().lower()\n            if axis not in ('x', 'y', 'z') or bound not in ('min', 'max'):\n                _ecky_edge_selector_error(selector)\n            clauses.append(('boundary', axis, bound))\n            continue\n        _ecky_edge_selector_error(selector)\n    return clauses".into(),
        String::new(),
        "def _ecky_number_signature(value):\n    text = format(float(value), '.3f').rstrip('0').rstrip('.')\n    if text in ('', '-0'):\n        return '0'\n    return text".into(),
        String::new(),
        "def _ecky_point_signature(point):\n    return '-'.join(_ecky_number_signature(coord) for coord in point)".into(),
        String::new(),
        "def _ecky_edge_endpoints(edge):\n    try:\n        vertexes = list(getattr(edge, 'Vertexes', []) or [])\n        if len(vertexes) >= 2:\n            start = vertexes[0].Point\n            end = vertexes[-1].Point\n            return (float(start.x), float(start.y), float(start.z)), (float(end.x), float(end.y), float(end.z))\n    except Exception:\n        pass\n    try:\n        start = edge.valueAt(edge.FirstParameter)\n        end = edge.valueAt(edge.LastParameter)\n        return (float(start.x), float(start.y), float(start.z)), (float(end.x), float(end.y), float(end.z))\n    except Exception:\n        return None".into(),
        String::new(),
        "def _ecky_edge_target_id(edge, edge_index, object_name):\n    endpoints = _ecky_edge_endpoints(edge)\n    if endpoints is None:\n        return f'{object_name}:edge:{edge_index}'\n    first, second = sorted((_ecky_point_signature(endpoints[0]), _ecky_point_signature(endpoints[1])))\n    return f'{object_name}:edge:{edge_index}:{first}_{second}'".into(),
        String::new(),
        "def _ecky_axis_value(vec, axis):\n    if axis == 'x':\n        return float(vec.x)\n    if axis == 'y':\n        return float(vec.y)\n    return float(vec.z)".into(),
        String::new(),
        "def _ecky_edge_matches_clause(edge, box, clause, tol):\n    edge_box = getattr(edge, 'BoundBox', None)\n    if edge_box is None:\n        return False\n    if clause[0] == 'axis':\n        x_span, y_span, z_span = _ecky_edge_axis_span(edge)\n        if clause[1] == 'x':\n            return x_span > tol and y_span <= tol and z_span <= tol\n        if clause[1] == 'y':\n            return y_span > tol and x_span <= tol and z_span <= tol\n        return z_span > tol and x_span <= tol and y_span <= tol\n    _, axis, bound = clause\n    target_box = box if bound == 'min' else box\n    target = {\n        ('x', 'min'): float(box.XMin),\n        ('x', 'max'): float(box.XMax),\n        ('y', 'min'): float(box.YMin),\n        ('y', 'max'): float(box.YMax),\n        ('z', 'min'): float(box.ZMin),\n        ('z', 'max'): float(box.ZMax),\n    }[(axis, bound)]\n    edge_min = {\n        'x': float(edge_box.XMin),\n        'y': float(edge_box.YMin),\n        'z': float(edge_box.ZMin),\n    }[axis]\n    edge_max = {\n        'x': float(edge_box.XMax),\n        'y': float(edge_box.YMax),\n        'z': float(edge_box.ZMax),\n    }[axis]\n    return abs(edge_min - target) <= tol and abs(edge_max - target) <= tol".into(),
        String::new(),
        "def _ecky_select_edges(shape, selector, object_name=None):\n    edges = list(getattr(shape, 'Edges', []) or [])\n    if not edges:\n        raise ValueError('Shape has no edges for fillet/chamfer.')\n    target_ids = _ecky_selector_target_ids(selector)\n    if target_ids is not None:\n        if not object_name:\n            raise ValueError(f'Edge selector `{selector}` requires an object name for exact target-id matching.')\n        edge_records = []\n        stable_counts = {}\n        for edge_index, edge in enumerate(edges):\n            target_id = _ecky_edge_target_id(edge, edge_index, object_name)\n            stable_id = _ecky_stable_edge_target_id(target_id)\n            edge_records.append((edge, target_id, stable_id))\n            stable_counts[stable_id] = stable_counts.get(stable_id, 0) + 1\n        selected = []\n        matched = set()\n        for requested_target_id in target_ids:\n            exact = next((record for record in edge_records if record[1] == requested_target_id), None)\n            if exact is not None:\n                if exact[1] not in matched:\n                    selected.append(exact[0])\n                    matched.add(exact[1])\n                continue\n            stable_requested = _ecky_stable_edge_target_id(requested_target_id)\n            candidates = [record for record in edge_records if record[2] == stable_requested]\n            if not candidates:\n                raise ValueError(f'Edge selector `{selector}` did not match target ids: {[requested_target_id]}')\n            if len(candidates) > 1 or stable_counts.get(stable_requested, 0) > 1:\n                raise ValueError(f'Edge selector `{selector}` ambiguously matched stable edge target `{requested_target_id}`.')\n            candidate = candidates[0]\n            if candidate[1] not in matched:\n                selected.append(candidate[0])\n                matched.add(candidate[1])\n        return selected\n    clauses = _ecky_selector_clauses(selector)\n    if not clauses:\n        return edges\n    box = getattr(shape, 'BoundBox', None)\n    if box is None:\n        raise ValueError('Shape has no BoundBox for edge selection.')\n    span = max(float(box.XLength), float(box.YLength), float(box.ZLength), 1.0)\n    tol = span * 1e-6\n    selected = [edge for edge in edges if all(_ecky_edge_matches_clause(edge, box, clause, tol) for clause in clauses)]\n    if not selected:\n        raise ValueError(f'Edge selector `{selector}` matched no edges.')\n    return selected".into(),
        String::new(),
        "def _ecky_edge_signature(edge):\n    box = getattr(edge, 'BoundBox', None)\n    if box is None:\n        return None\n    try:\n        curve = edge.Curve\n        u0, u1 = edge.ParameterRange\n        mid = curve.value((float(u0) + float(u1)) / 2.0)\n        point = (float(mid.x), float(mid.y), float(mid.z))\n    except Exception:\n        point = (\n            (float(box.XMin) + float(box.XMax)) / 2.0,\n            (float(box.YMin) + float(box.YMax)) / 2.0,\n            (float(box.ZMin) + float(box.ZMax)) / 2.0,\n        )\n    return {\n        'point': point,\n        'span': (float(box.XLength), float(box.YLength), float(box.ZLength)),\n    }".into(),
        String::new(),
        "def _ecky_match_edge(shape, signature):\n    best = None\n    best_score = None\n    for edge in list(getattr(shape, 'Edges', []) or []):\n        candidate = _ecky_edge_signature(edge)\n        if candidate is None:\n            continue\n        score = 0.0\n        for a, b in zip(candidate['point'], signature['point']):\n            score += abs(float(a) - float(b))\n        for a, b in zip(candidate['span'], signature['span']):\n            score += abs(float(a) - float(b))\n        if best_score is None or score < best_score:\n            best = edge\n            best_score = score\n    return best".into(),
        String::new(),
        "def _ecky_apply_edge_op(shape, amount, selector, op_name, object_name=None):\n    selected = _ecky_select_edges(shape, selector, object_name)\n    direct = shape.makeFillet if op_name == 'fillet' else shape.makeChamfer\n    try:\n        return direct(amount, selected)\n    except Exception as exc:\n        failure = exc\n    result = shape.copy()\n    applied = 0\n    for signature in [_ecky_edge_signature(edge) for edge in selected]:\n        if signature is None:\n            continue\n        edge = _ecky_match_edge(result, signature)\n        if edge is None:\n            continue\n        try:\n            direct = result.makeFillet if op_name == 'fillet' else result.makeChamfer\n            result = direct(amount, [edge])\n            applied += 1\n        except Exception:\n            continue\n    if applied > 0:\n        return result\n    raise ValueError(f'{op_name} failed for selector `{selector}`: {failure}')".into(),
        String::new(),
        "def _ecky_fillet(shape, radius, selector=None, object_name=None):\n    amount = abs(float(radius))\n    if amount <= 1e-12:\n        return shape.copy()\n    return _ecky_apply_edge_op(shape, amount, selector, 'fillet', object_name)".into(),
        String::new(),
        "def _ecky_chamfer(shape, distance, selector=None, object_name=None):\n    amount = abs(float(distance))\n    if amount <= 1e-12:\n        return shape.copy()\n    return _ecky_apply_edge_op(shape, amount, selector, 'chamfer', object_name)".into(),
        String::new(),
        "def _ecky_clip_box(shape, xmin, xmax, ymin, ymax, zmin, zmax):\n    x0, x1 = sorted((float(xmin), float(xmax)))\n    y0, y1 = sorted((float(ymin), float(ymax)))\n    z0, z1 = sorted((float(zmin), float(zmax)))\n    box = Part.makeBox(x1 - x0, y1 - y0, z1 - z0)\n    box.translate(App.Vector(x0, y0, z0))\n    return shape.common(box)".into(),
        String::new(),
        "def _ecky_vec3(value, default=(0, 0, 0)):\n    if isinstance(value, App.Vector):\n        return App.Vector(float(value.x), float(value.y), float(value.z))\n    if value is None:\n        value = default\n    return App.Vector(*[float(v) for v in value])".into(),
        String::new(),
        "def _ecky_norm(vec, fallback=(1, 0, 0)):\n    value = _ecky_vec3(vec, fallback)\n    length = float(value.Length)\n    if length <= 1e-12:\n        value = _ecky_vec3(fallback)\n        length = float(value.Length)\n    if length <= 1e-12:\n        return App.Vector(1, 0, 0)\n    return App.Vector(value.x / length, value.y / length, value.z / length)".into(),
        String::new(),
        "def _ecky_project_perp(vec, normal):\n    value = _ecky_vec3(vec)\n    axis = _ecky_norm(normal, (0, 0, 1))\n    dot = float(value.dot(axis))\n    return App.Vector(value.x - axis.x * dot, value.y - axis.y * dot, value.z - axis.z * dot)".into(),
        String::new(),
        "def _ecky_basis(x_hint, z_hint):\n    z_axis = _ecky_norm(z_hint, (0, 0, 1))\n    x_axis = _ecky_project_perp(x_hint, z_axis)\n    if float(x_axis.Length) <= 1e-9:\n        fallback = App.Vector(1, 0, 0)\n        if abs(float(fallback.dot(z_axis))) > 0.9:\n            fallback = App.Vector(0, 1, 0)\n        x_axis = _ecky_project_perp(fallback, z_axis)\n    x_axis = _ecky_norm(x_axis, (1, 0, 0))\n    y_axis = z_axis.cross(x_axis)\n    if float(y_axis.Length) <= 1e-9:\n        fallback = App.Vector(0, 1, 0)\n        if abs(float(fallback.dot(z_axis))) > 0.9:\n            fallback = App.Vector(1, 0, 0)\n        x_axis = _ecky_norm(_ecky_project_perp(fallback, z_axis), (1, 0, 0))\n        y_axis = z_axis.cross(x_axis)\n    y_axis = _ecky_norm(y_axis, (0, 1, 0))\n    x_axis = _ecky_norm(y_axis.cross(z_axis), (1, 0, 0))\n    return x_axis, y_axis, z_axis".into(),
        String::new(),
        "def _ecky_euler_placement(offset=(0, 0, 0), rotate=(0, 0, 0)):\n    placement = App.Placement(_ecky_vec3(offset), App.Rotation())\n    rx, ry, rz = [float(v) for v in (rotate or (0, 0, 0))]\n    if abs(rx) > 1e-12:\n        placement = placement.multiply(App.Placement(App.Vector(0, 0, 0), App.Rotation(App.Vector(1, 0, 0), rx)))\n    if abs(ry) > 1e-12:\n        placement = placement.multiply(App.Placement(App.Vector(0, 0, 0), App.Rotation(App.Vector(0, 1, 0), ry)))\n    if abs(rz) > 1e-12:\n        placement = placement.multiply(App.Placement(App.Vector(0, 0, 0), App.Rotation(App.Vector(0, 0, 1), rz)))\n    return placement".into(),
        String::new(),
        "def _ecky_path_sample(path, at):\n    if path is None:\n        return App.Vector(0, 0, 0), App.Vector(0, 0, 1)\n    edges = list(getattr(path, 'Edges', []) or [])\n    vertices = list(getattr(path, 'Vertexes', []) or [])\n    if not edges:\n        if not vertices:\n            return App.Vector(0, 0, 0), App.Vector(0, 0, 1)\n        point = vertices[-1].Point\n        if at == 'start':\n            point = vertices[0].Point\n        return point, App.Vector(0, 0, 1)\n    if at == 'start':\n        edge = edges[0]\n        return vertices[0].Point if vertices else edge.valueAt(edge.FirstParameter), _ecky_norm(edge.tangentAt(edge.FirstParameter), (0, 0, 1))\n    if at == 'end':\n        edge = edges[-1]\n        point = vertices[-1].Point if vertices else edge.valueAt(edge.LastParameter)\n        tangent_param = edge.LastParameter - 1e-9 if edge.LastParameter > edge.FirstParameter else edge.FirstParameter\n        return point, _ecky_norm(edge.tangentAt(tangent_param), (0, 0, 1))\n    try:\n        position = float(at)\n    except Exception:\n        edge = edges[-1]\n        point = vertices[-1].Point if vertices else edge.valueAt(edge.LastParameter)\n        tangent_param = edge.LastParameter - 1e-9 if edge.LastParameter > edge.FirstParameter else edge.FirstParameter\n        return point, _ecky_norm(edge.tangentAt(tangent_param), (0, 0, 1))\n    if position <= 0.0:\n        edge = edges[0]\n        return vertices[0].Point if vertices else edge.valueAt(edge.FirstParameter), _ecky_norm(edge.tangentAt(edge.FirstParameter), (0, 0, 1))\n    if position >= 1.0:\n        edge = edges[-1]\n        point = vertices[-1].Point if vertices else edge.valueAt(edge.LastParameter)\n        tangent_param = edge.LastParameter - 1e-9 if edge.LastParameter > edge.FirstParameter else edge.FirstParameter\n        return point, _ecky_norm(edge.tangentAt(tangent_param), (0, 0, 1))\n    total = sum(float(edge.Length) for edge in edges)\n    if total <= 1e-12:\n        edge = edges[0]\n        return edge.valueAt(edge.FirstParameter), _ecky_norm(edge.tangentAt(edge.FirstParameter), (0, 0, 1))\n    target = total * position\n    walked = 0.0\n    for edge in edges:\n        edge_len = float(edge.Length)\n        if target <= walked + edge_len or edge is edges[-1]:\n            ratio = 0.0 if edge_len <= 1e-12 else max(0.0, min(1.0, (target - walked) / edge_len))\n            param = edge.FirstParameter + ratio * (edge.LastParameter - edge.FirstParameter)\n            point = edge.valueAt(param)\n            tangent = edge.tangentAt(param)\n            return point, _ecky_norm(tangent, (0, 0, 1))\n        walked += edge_len\n    edge = edges[-1]\n    point = vertices[-1].Point if vertices else edge.valueAt(edge.LastParameter)\n    tangent_param = edge.LastParameter - 1e-9 if edge.LastParameter > edge.FirstParameter else edge.FirstParameter\n    return point, _ecky_norm(edge.tangentAt(tangent_param), (0, 0, 1))".into(),
        String::new(),
        "def _ecky_path_point(path, at):\n    return _ecky_path_sample(path, at)[0]".into(),
        String::new(),
        "def _ecky_frame_placement(frame):\n    if not isinstance(frame, dict):\n        return App.Placement()\n    kind = frame.get('kind')\n    if kind == 'plane':\n        origin = _ecky_vec3(frame.get('origin'), (0, 0, 0))\n        normal = frame.get('normal') or (0, 0, 1)\n        x_hint = frame.get('x')\n        if x_hint is not None:\n            raw_x = _ecky_project_perp(x_hint, normal)\n            if float(raw_x.Length) <= 1e-9:\n                raise ValueError(f'`plane :x` must be perpendicular to `:normal` {normal}.')\n            x_axis, y_axis, z_axis = _ecky_basis(raw_x, normal)\n        else:\n            x_axis, y_axis, z_axis = _ecky_basis((1, 0, 0), normal)\n        return App.Placement(origin, App.Rotation(x_axis, y_axis, z_axis))\n    if kind == 'location':\n        base = _ecky_frame_placement(frame.get('frame'))\n        local = _ecky_euler_placement(frame.get('offset') or (0, 0, 0), frame.get('rotate') or (0, 0, 0))\n        return base.multiply(local)\n    if kind == 'path-frame':\n        origin, tangent = _ecky_path_sample(frame.get('path'), frame.get('at', 'end'))\n        if frame.get('up') is not None:\n            raw_up = _ecky_project_perp(frame.get('up'), tangent)\n            if float(raw_up.Length) <= 1e-9:\n                raise ValueError(f'`path-frame :up` must be perpendicular to the path tangent {tangent}.')\n            x_axis, y_axis, z_axis = _ecky_basis(raw_up, tangent)\n        else:\n            x_axis, y_axis, z_axis = _ecky_basis((0, 0, 1), tangent)\n        return App.Placement(origin, App.Rotation(x_axis, y_axis, z_axis))\n    origin = frame.get('origin')\n    if isinstance(origin, tuple) or isinstance(origin, list):\n        return App.Placement(_ecky_vec3(origin), App.Rotation())\n    return App.Placement()".into(),
        String::new(),
        "def _ecky_place(frame, shape, offset=(0, 0, 0), rotate=(0, 0, 0)):\n    result = shape.copy()\n    placement = _ecky_frame_placement(frame).multiply(_ecky_euler_placement(offset, rotate))\n    result.Placement = placement.multiply(result.Placement)\n    return result".into(),
        String::new(),
        "doc = App.ActiveDocument or App.newDocument('EckyCAD')".into(),
        String::new(),
    ]
}

fn parse_build_expr(value: &IrExpr) -> AppResult<(Vec<BuildBinding>, IrExpr)> {
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
    let mut seen = BTreeSet::new();
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
                bindings.push(BuildBinding {
                    name,
                    expr: stmt[2].clone(),
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

    Ok((
        bindings,
        result.ok_or_else(|| validation("`build` requires exactly one `(result ...)`."))?,
    ))
}

fn parse_lambda_expr(value: &IrExpr) -> AppResult<(Vec<String>, IrExpr)> {
    let items = expr_list_items(value, "lambda expression")?;
    if expr_head_symbol(items, "lambda expression")? != "lambda" || items.len() != 3 {
        return Err(validation("`map` expects `(lambda (args ...) body)`."));
    }
    let params = expr_list_items(&items[1], "lambda parameter list")?
        .iter()
        .map(|param| {
            param
                .as_symbol()
                .map(str::to_string)
                .ok_or_else(|| validation("Lambda parameters must be symbols."))
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok((params, items[2].clone()))
}

#[derive(Debug)]
struct BuildBinding {
    name: String,
    expr: IrExpr,
    value_kind: Option<CoreValueKind>,
}

#[cfg(test)]
mod tests {
    use super::{lower_core_program_to_freecad, lower_to_freecad};

    fn surface_fixture(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/cad/surface/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
    }

    fn example_fixture(name: &str) -> String {
        let path = format!(
            "{}/../model-runtime/examples/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
    }

    fn example_fixture_or_fallback(name: &str, fallback: &str) -> String {
        let path = format!(
            "{}/../model-runtime/examples/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        std::fs::read_to_string(&path).unwrap_or_else(|_| fallback.to_string())
    }

    fn fixture_part_ids(source: &str) -> Vec<String> {
        let mut ids = Vec::new();
        let mut tail = source;
        while let Some(idx) = tail.find("(part ") {
            let rest = &tail[idx + "(part ".len()..];
            let Some(end) = rest.find(|ch: char| ch.is_whitespace() || ch == ')') else {
                break;
            };
            if end > 0 {
                ids.push(rest[..end].to_string());
            }
            tail = rest;
        }
        ids
    }

    fn assert_fixture_tuple_names(code: &str, source: &str) {
        let ids = fixture_part_ids(source);
        assert!(!ids.is_empty(), "fixture must declare at least one part");
        for part_id in ids {
            assert!(
                code.contains(&format!(r#"("{}","#, part_id)),
                "missing tuple part id `{}` in lowered output: {}",
                part_id,
                code
            );
        }
    }

    #[test]
    fn freecad_lowering_emits_runner_adapter_and_parts_list() {
        let src = r#"(model (part body (box 10 20 30)))"#;
        let code = crate::ecky_ir::lower_to_freecad(src).expect("lower");
        assert!(code.contains("_ecky_parts"), "missing parts list: {}", code);
        assert!(
            !code.contains("doc.addObject"),
            "runner adapter leaked into lowered source: {}",
            code
        );
        assert!(
            code.contains("_ecky_box"),
            "missing FreeCAD primitive: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_handles_cup_and_body_minimum() {
        let cup = lower_to_freecad(include_str!(
            "../../tests/fixtures/cad/surface/canonical_cup.ecky"
        ))
        .expect("cup");
        assert!(cup.contains("_ecky_bspline"), "cup bspline: {}", cup);
        assert!(cup.contains("_ecky_shell"), "cup shell: {}", cup);
        assert!(cup.contains("_ecky_fillet"), "cup fillet: {}", cup);

        let body = lower_to_freecad(include_str!(
            "../../tests/fixtures/cad/surface/thomas_modular_ramp_body.ecky"
        ))
        .expect("body");
        assert!(body.contains("_ecky_polygon"), "body polygon: {}", body);
        assert!(body.contains("_ecky_extrude"), "body extrude: {}", body);
        assert!(body.contains("_ecky_cut_many"), "body cut: {}", body);
    }

    #[test]
    fn freecad_lowering_supports_align_clip_box_and_place() {
        let src = r#"(model
            (part body
              (build
                (shape base (plane :origin (10 20 30) :x (0 1 0) :normal (0 0 1)))
                (shape peg (box 4 4 4 :align '(min center max)))
                (shape pose (location base :offset (5 0 0) :rotate (0 90 0)))
                (shape clipped (clip-box (place pose peg)
                  :x (20 40)
                  :y (-5 5)
                  :z (-10 20)))
                (result clipped))))"#;
        let code = crate::ecky_ir::lower_to_freecad(src).expect("lower");
        assert!(code.contains("_ecky_box"), "box align: {}", code);
        assert!(
            code.contains("(\"min\", \"center\", \"max\")"),
            "align tuple: {}",
            code
        );
        assert!(code.contains("_ecky_place"), "place helper: {}", code);
        assert!(code.contains("_ecky_clip_box"), "clip-box helper: {}", code);
        assert!(code.contains("'kind': 'plane'"), "plane frame: {}", code);
    }

    #[test]
    fn freecad_lowering_supports_sweep_smoke() {
        let src = r#"(model
            (part body
              (sweep
                (polygon ((0 0) (4 0) (4 2) (0 2)))
                (path ((0 0 0) (0 0 10))))))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(code.contains("_ecky_sweep"), "sweep helper: {}", code);
        assert!(code.contains("_ecky_path"), "path helper: {}", code);
    }

    #[test]
    fn freecad_lowering_supports_helical_ridge_smoke() {
        let src = r#"(model
            (part thread
              (helical-ridge
                :radius 10
                :pitch 2
                :height 18
                :base-width 1.2
                :crest-width 0.4
                :depth 0.7
                :female #t
                :clearance 0.15
                :lefthand #t)))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(
            code.contains("_ecky_helical_ridge("),
            "helper call: {}",
            code
        );
        assert!(code.contains("Part.makeHelix("), "helix helper: {}", code);
        assert!(code.contains("makePipeShell("), "pipe shell: {}", code);
        assert!(
            code.contains("path_radius = radius"),
            "same path radius: {}",
            code
        );
        assert!(
            code.contains("female=True, clearance=0.15, lefthand=True"),
            "female args: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_helicoid_thread_coupon_fixture_keeps_helical_markers_and_tuple_names() {
        let source = example_fixture("helicoid-thread-coupon.ecky");
        let code = lower_to_freecad(&source).expect("lower");

        assert!(
            code.contains("_ecky_helical_ridge("),
            "helical helper missing: {}",
            code
        );
        assert!(code.contains("Part.makeHelix("), "helix marker: {}", code);
        assert!(
            code.contains("female=True, clearance=0.2")
                && code.contains("female=True, clearance=0.35"),
            "clearance variants missing: {}",
            code
        );
        assert!(
            code.contains(r#"("coupon_male_020","#)
                && code.contains(r#"("coupon_female_020","#)
                && code.contains(r#"("coupon_male_035","#)
                && code.contains(r#"("coupon_female_035","#),
            "part tuple names missing: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_magnet_clamp_coupon_fixture_keeps_multi_part_and_tuple_names() {
        let source = example_fixture("magnet-clamp-coupon.ecky");
        let code = lower_to_freecad(&source).expect("lower");

        assert!(
            code.contains("_ecky_cut_many("),
            "difference path: {}",
            code
        );
        assert!(
            code.contains("_ecky_cylinder(3.2, 6.0")
                && code.contains("_ecky_cylinder(3.5, 1.2")
                && code.contains("_ecky_box(60.0, 30.0, 6.0")
                && code.contains("_ecky_box(60.0, 30.0, 1.2"),
            "expected magnet primitive markers missing: {}",
            code
        );
        assert!(
            code.contains(r#"("magnet_clamp_base_n","#)
                && code.contains(r#"("magnet_clamp_base_s","#)
                && code.contains(r#"("magnet_polarity_mask_n","#)
                && code.contains(r#"("magnet_polarity_mask_s","#),
            "part tuple names missing: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_dovetail_box_fixture_keeps_tuple_names_and_boolean_markers() {
        let source = example_fixture_or_fallback(
            "dovetail-box.ecky",
            r#"(model
                (part box_shell
                  (difference
                    (box 80 60 24)
                    (translate 2 2 2 (box 76 56 22))))
                (part lid
                  (translate 0 0 24
                    (box 80 60 2))))"#,
        );
        let code = lower_to_freecad(&source).expect("lower");

        assert_fixture_tuple_names(&code, &source);
        assert!(code.contains("_ecky_box("), "box marker missing: {}", code);
        assert!(
            code.contains("_ecky_cut_many(") || code.contains("_ecky_fuse_many("),
            "boolean marker missing: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_vermicomposter_lid_clearance_fixture_keeps_tuple_names_and_lip_markers() {
        let source = example_fixture_or_fallback(
            "vermicomposter-lid-clearance.ecky",
            r#"(model
                (part lid_clearance_035
                  (difference
                    (box 160 120 8)
                    (translate 0 0 1
                      (grid-array 4 3 30 30 (cylinder 4 8)))))
                (part lid_clearance_055
                  (translate 0 140 0
                    (difference
                      (box 160 120 8)
                      (translate 0 0 1
                        (grid-array 4 3 30 30 (cylinder 5 8)))))))"#,
        );
        let code = lower_to_freecad(&source).expect("lower");

        assert_fixture_tuple_names(&code, &source);
        assert!(
            code.contains("_ecky_cut_many("),
            "cut marker missing: {}",
            code
        );
        assert!(
            code.contains("_ecky_fuse_many(") && code.contains("_ecky_translate("),
            "lid lip/vent markers missing: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_snap_hook_coupon_fixture_keeps_tuple_names_and_curve_markers() {
        let source = example_fixture_or_fallback(
            "snap-hook-coupon.ecky",
            r#"(model
                (part snap_arm
                  (fillet 0.8
                    (sweep
                      (polygon ((0 0) (2.4 0) (2.4 1.6) (0 1.6)))
                      (bezier-path ((0 0 0) (16 0 2) (24 0 6) (36 0 10)))))
                (part snap_mate
                  (difference
                    (box 40 10 12)
                    (translate 2 2 2 (box 36 6 8)))))"#,
        );
        let code = lower_to_freecad(&source).expect("lower");

        assert_fixture_tuple_names(&code, &source);
        assert!(
            code.contains("_ecky_bezier_path(") || code.contains("_ecky_bspline("),
            "curve marker missing: {}",
            code
        );
        assert!(
            code.contains("_ecky_sweep("),
            "sweep marker missing: {}",
            code
        );
        assert!(
            code.contains("_ecky_fillet(") || code.contains("_ecky_chamfer("),
            "edge-finish marker missing: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_supports_circle_smoke() {
        let src = r#"(model
            (part body
              (extrude (circle 10) 8)))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(code.contains("_ecky_circle"), "circle helper: {}", code);
        assert!(code.contains("_ecky_extrude"), "extrude helper: {}", code);
    }

    #[test]
    fn freecad_lowering_supports_rounded_rect_smoke() {
        let src = r#"(model
            (part body
              (extrude (rounded-rect 20 10 2) 8)))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(
            code.contains("_ecky_rounded_rect"),
            "rounded-rect helper: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_supports_rounded_polygon_smoke() {
        let src = r#"(model
            (part body
              (extrude (rounded-polygon ((0 20) (20 0) (0 -20) (-20 0)) 4 8) 8)))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(
            code.contains("_ecky_rounded_polygon"),
            "rounded-polygon helper: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_supports_profile_smoke() {
        let src = r#"(model
            (part body
              (extrude
                (profile
                  :outer (polygon ((0 0) (20 0) (20 20) (0 20)))
                  :holes ((polygon ((6 6) (14 6) (14 14) (6 14)))))
                4)))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(
            code.contains("_ecky_face_with_holes"),
            "profile helper: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_supports_bezier_path_smoke() {
        let src = r#"(model
            (part body
              (sweep
                (polygon ((0 0) (4 0) (4 2) (0 2)))
                (bezier-path ((0 0 0) (8 0 0) (8 8 0) (16 8 0))))))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(
            code.contains("_ecky_bezier_path"),
            "bezier-path helper: {}",
            code
        );
        assert!(code.contains("_ecky_sweep"), "sweep helper: {}", code);
    }

    #[test]
    fn freecad_lowering_supports_offset_smoke() {
        let src = r#"(model
            (part body
              (extrude
                (offset 2 :openings ((polygon ((6 6) (14 6) (14 14) (6 14))))
                  (polygon ((0 0) (20 0) (20 20) (0 20))))
                4)))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(code.contains("_ecky_offset"), "offset helper: {}", code);
        assert!(
            code.contains("_ecky_face_with_holes"),
            "offset openings helper: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_supports_taper_and_twist_smoke() {
        let src = r#"(model
            (part body
              (union
                (taper 20 0.6 (polygon ((0 0) (8 0) (8 8) (0 8))))
                (translate 20 0 0
                  (twist 20 90 8 (polygon ((0 0) (8 0) (8 8) (0 8))))))))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(code.contains("_ecky_loft"), "loft helper: {}", code);
        assert!(code.contains("_ecky_scale"), "taper scale: {}", code);
        assert!(code.contains("_ecky_rotate"), "twist rotate: {}", code);
    }

    #[test]
    fn freecad_lowering_supports_named_arrays_smoke() {
        let src = r#"(model
            (part body
              (union
                (linear-array 3 12 0 0 (box 4 4 4))
                (grid-array 2 2 10 10 (box 2 2 2))
                (radial-array 4 90 20 (cylinder 2 4))
                (arc-array 3 30 0 180 (cylinder 2 4)))))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(code.contains("__ecky_la_i"), "linear-array loop: {}", code);
        assert!(code.contains("__ecky_ga_r"), "grid-array loop: {}", code);
        assert!(code.contains("__ecky_ra_i"), "radial-array loop: {}", code);
        assert!(code.contains("__ecky_aa_i"), "arc-array loop: {}", code);
    }

    #[test]
    fn freecad_lowering_supports_xor_smoke() {
        let src = r#"(model
            (part body
              (xor
                (box 10 10 10)
                (translate 5 0 0 (box 10 10 10)))))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(code.contains("_ecky_xor_many"), "xor helper: {}", code);
    }

    #[test]
    fn freecad_lowering_supports_loft_smoke() {
        let src = r#"(model
            (part body
              (loft 50
                (polygon ((0 0) (12 0) (12 12) (0 12)))
                (polygon ((2 2) (10 2) (10 10) (2 10)))
                (polygon ((4 4) (8 4) (8 8) (4 8))))))"#;
        let code = lower_to_freecad(src).expect("lower");
        assert!(code.contains("_ecky_loft"), "loft helper: {}", code);
        assert!(
            code.contains("_ecky_translate("),
            "section positioning: {}",
            code
        );
        assert!(
            code.contains("(50.0) * 0.5"),
            "middle section height: {}",
            code
        );
        assert!(
            code.contains("(50.0) * 1.0"),
            "top section height: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_supports_sampled_radial_loft_smoke() {
        let src = r#"
            (model
              (part body
                (sampled-radial-loft
                  (theta z fz)
                  :height 40
                  :z-steps 6
                  :theta-steps 24
                  :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                  :z-map (+ z (* fz 2))))))"#;
        let code = crate::ecky_ir::lower_to_freecad(src).expect("lower");
        assert!(code.contains("_zi in range("), "{code}");
        assert!(code.contains("_ti in range("), "{code}");
        assert!(code.contains("App.Vector("), "{code}");
        assert!(code.contains("Part.makePolygon("), "{code}");
        assert!(code.contains("_ecky_loft("), "{code}");
        assert!(code.contains("math.sin("), "{code}");
    }

    #[test]
    fn freecad_lowering_supports_text_svg_and_import_stl() {
        let text = lower_to_freecad(r#"(model (part body (extrude (text "HELLO" 12) 2)))"#)
            .expect("text lower");
        assert!(text.contains("_ecky_text"), "text helper: {}", text);
        assert!(
            text.contains("_ecky_default_font_path"),
            "font helper: {}",
            text
        );
        assert!(
            text.contains("faces = [_ecky_face(glyph) for glyph in glyphs]"),
            "text helper should normalize glyph wires/faces: {}",
            text
        );

        let svg = lower_to_freecad(r#"(model (part body (extrude (svg "/tmp/logo.svg") 2)))"#)
            .expect("svg lower");
        assert!(svg.contains("_ecky_svg"), "svg helper: {}", svg);
        assert!(svg.contains("import importSVG"), "svg import: {}", svg);
        assert!(
            svg.contains("fit_mode='contain'"),
            "svg helper should support fit modes: {}",
            svg
        );
        assert!(svg.contains("mode == 'cover'"), "svg cover mode: {}", svg);
        assert!(
            svg.contains("mode in ('stretch', 'fill')"),
            "svg stretch mode: {}",
            svg
        );

        let fitted_svg = lower_to_freecad(
            r#"(model (part body (extrude (svg "/tmp/logo.svg" 8 5 "contain") 2)))"#,
        )
        .expect("fitted svg lower");
        assert!(
            fitted_svg.contains("_ecky_svg")
                && fitted_svg.contains("/tmp/logo.svg")
                && fitted_svg.contains("8.0, 5.0")
                && fitted_svg.contains("contain"),
            "fitted svg call: {}",
            fitted_svg
        );

        let stl = lower_to_freecad(r#"(model (part body (import-stl "/tmp/sample.stl")))"#)
            .expect("stl lower");
        assert!(stl.contains("_ecky_import_stl"), "stl helper: {}", stl);
        assert!(stl.contains("import Mesh"), "mesh import: {}", stl);
    }

    #[test]
    fn freecad_lowering_supports_repeat_compound_and_repeat_pick() {
        let compound = lower_to_freecad(
            r#"(model
                (part body
                  (repeat-compound i 3
                    (translate (* i 10) 0 0 (cylinder 2 5)))))"#,
        )
        .expect("repeat-compound lower");
        assert!(
            compound.contains("for __ecky_rc_i"),
            "repeat-compound loop: {}",
            compound
        );
        assert!(
            compound.contains("_ecky_compound"),
            "repeat-compound helper: {}",
            compound
        );

        let pick = lower_to_freecad(
            r#"(model
                (part body
                  (repeat-pick i 4 (= i 2)
                    (translate (* i 5) 0 0 (box 2 2 2)))))"#,
        )
        .expect("repeat-pick lower");
        assert!(
            pick.contains("for __ecky_rp_i"),
            "repeat-pick loop: {}",
            pick
        );
        assert!(
            pick.contains("repeat-pick found no matching geometry"),
            "repeat-pick guard: {}",
            pick
        );
    }

    #[test]
    fn freecad_lowering_supports_apply_map_range_cutters() {
        let code = lower_to_freecad(include_str!(
            "../../tests/fixtures/cad/surface/tooth_rotated_cutters_comprehension.ecky"
        ))
        .expect("apply-map-range lower");
        assert!(
            code.contains("for __ecky_map_i in _f"),
            "map loop: {}",
            code
        );
        assert!(
            code.contains("range(int(math.floor(0.0)), int(math.floor("),
            "dynamic range: {}",
            code
        );
        assert!(
            code.contains("_ecky_cut_many(_base, *_f"),
            "spliced cut: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_supports_organic_bspline_loop_fixture() {
        let source = surface_fixture("organic_bspline_loop.ecky");
        let code = crate::ecky_ir::lower_to_freecad(&source).expect("lower");

        assert!(code.contains("_ecky_bspline"), "bspline helper: {}", code);
        assert!(code.contains("closed=True"), "closed loop: {}", code);
        assert!(code.contains("_ecky_hash_signed("), "seeded loop: {}", code);
        assert!(code.contains("_ecky_extrude"), "surface extrude: {}", code);
    }

    #[test]
    fn freecad_lowering_supports_voronoi_perforated_panel_fixture() {
        let source = surface_fixture("voronoi_perforated_panel.ecky");
        let code = lower_to_freecad(&source).expect("lower");

        assert!(
            code.contains("for __ecky_map_cell in "),
            "cell map loop: {}",
            code
        );
        assert!(
            code.contains("_ecky_fuse_many(*"),
            "apply union cutouts: {}",
            code
        );
        assert!(
            code.contains("_ecky_cut_many(_panel, "),
            "panel perforation cut: {}",
            code
        );
        assert!(
            code.contains("_ecky_hash_signed("),
            "seeded cells: {}",
            code
        );
    }

    #[test]
    fn lower_core_program_to_freecad_matches_public_entrypoint_for_comprehension_fixture() {
        let source = include_str!(
            "../../tests/fixtures/cad/surface/tooth_rotated_cutters_comprehension.ecky"
        );
        let program = crate::ecky_scheme::try_compile_to_core_program(source)
            .expect("compiled path")
            .expect("program");
        let direct = lower_core_program_to_freecad(&program).expect("direct");
        let public = crate::ecky_ir::lower_to_freecad(source).expect("public");

        assert_eq!(direct, public);
        assert!(
            direct.contains("for __ecky_map_") && direct.contains(" in _f"),
            "map loop: {}",
            direct
        );
        assert!(
            direct.contains("range(int(math.floor(0.0)), int(math.floor("),
            "dynamic range: {}",
            direct
        );
        assert!(
            direct.contains("_ecky_cut_many(_base, *_f"),
            "spliced cut: {}",
            direct
        );
    }

    #[test]
    fn lower_core_program_to_freecad_rejects_typed_hole_kinds() {
        let cases = [
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
        ];

        for (type_name, goal, source) in cases {
            let program = crate::ecky_scheme::compile_to_core_program(source)
                .unwrap_or_else(|err| panic!("{type_name} hole should compile: {err}"));
            let err = match lower_core_program_to_freecad(&program) {
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

    #[test]
    fn freecad_lowering_accepts_typed_let_wrapped_point_lists_from_core_bridge() {
        let source = r#"(model
            (part body
              (extrude
                (polygon
                  (list
                    (let ((i 1))
                      (list i (+ i 1)))
                    (let ((i 3))
                      (list i (+ i 1)))))
                5)))"#;

        let code = crate::ecky_ir::lower_to_freecad(source).expect("lower");

        assert!(code.contains("_ecky_polygon("), "polygon: {}", code);
        assert!(code.contains("App.Vector(1.0, (1.0 + 1.0), 0.0)"));
        assert!(code.contains("App.Vector(3.0, (3.0 + 1.0), 0.0)"));
    }

    #[test]
    fn lower_core_program_to_freecad_supports_text_params_without_legacy_model_bridge() {
        use crate::ecky_core_ir::{
            CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CoreParameter,
            CoreParameterConstraints, CoreParameterKind, CoreParameterValue, CorePart,
            CorePrimitive, CoreProgram, CoreReference, CoreValueKind, NodeId, ParamId, PartId,
            ProgramId,
        };

        let label_id = ParamId::new(1);
        let root = CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Call {
                op: CoreOperation::Primitive(CorePrimitive::Text),
                args: vec![
                    CoreNode::new(
                        NodeId::new(11),
                        CoreNodeKind::Reference(CoreReference::Parameter(label_id)),
                        CoreValueKind::Text,
                    ),
                    CoreNode::new(
                        NodeId::new(12),
                        CoreNodeKind::Literal(CoreLiteral::Number(10.0)),
                        CoreValueKind::Number,
                    ),
                ],
                keywords: vec![],
            },
            CoreValueKind::Sketch,
        );
        let program = CoreProgram::new(
            ProgramId::new(1),
            vec![CoreParameter {
                id: label_id,
                key: "label".into(),
                label: "Label".into(),
                kind: CoreParameterKind::Text,
                default_value: CoreParameterValue::Text("hello".into()),
                frozen: false,
                constraints: CoreParameterConstraints::default(),
            }],
            vec![CorePart {
                id: PartId::new(2),
                key: "body".into(),
                label: "Body".into(),
                root,
            }],
        );

        let bridge_err = crate::ecky_ir::model::core_program_to_model(&program)
            .err()
            .expect("legacy bridge should reject text params");
        assert!(
            bridge_err
                .details
                .as_deref()
                .unwrap_or(bridge_err.message.as_str())
                .contains("Text params are not yet supported by the legacy IR bridge."),
            "unexpected bridge error: {:?}",
            bridge_err
        );

        let code = lower_core_program_to_freecad(&program).expect("lower");
        assert!(code.contains("_ecky_text("), "text helper: {}", code);
        assert!(
            code.contains(r#"str(params.get("label", "hello"))"#),
            "text param default: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_emits_selector_aware_fillet_and_chamfer() {
        let fillet =
            lower_to_freecad(r#"(model (part body (fillet 1.5 :edges top (box 20 20 10))))"#)
                .expect("fillet lower");
        assert!(
            fillet.contains("_ecky_fillet(_f0, 1.5, {'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'z', 'bound': 'max'}]}, \"body\")"),
            "fillet selector call: {}",
            fillet
        );
        assert!(
            fillet.contains("def _ecky_select_edges"),
            "selector helper missing: {}",
            fillet
        );
        assert!(
            fillet.contains(
                "direct = shape.makeFillet if op_name == 'fillet' else shape.makeChamfer"
            ),
            "fillet helper should dispatch through FreeCAD edge op helper: {}",
            fillet
        );
        assert!(
            !fillet.contains("def _ecky_fillet(shape, radius):\n    try:\n        return shape.makeFillet(abs(float(radius)), list(getattr(shape, 'Edges', [])))\n    except Exception:\n        return shape.copy()"),
            "legacy silent noop helper leaked: {}",
            fillet
        );

        let chamfer =
            lower_to_freecad(r#"(model (part body (chamfer 2 :edges "bottom" (box 20 20 10))))"#)
                .expect("chamfer lower");
        assert!(
            chamfer.contains("_ecky_chamfer(_f0, 2.0, {'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'z', 'bound': 'min'}]}, \"body\")"),
            "chamfer selector call: {}",
            chamfer
        );
        assert!(
            chamfer.contains(
                "direct = shape.makeFillet if op_name == 'fillet' else shape.makeChamfer"
            ),
            "chamfer helper should dispatch through FreeCAD edge op helper: {}",
            chamfer
        );

        let compound = lower_to_freecad(
            r#"(model (part body (fillet 1 :edges "left+vertical" (box 20 20 10))))"#,
        )
        .expect("compound selector lower");
        assert!(
            compound.contains("_ecky_fillet(_f0, 1.0, {'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'x', 'bound': 'min'}, {'kind': 'axis', 'axis': 'z'}]}, \"body\")"),
            "compound selector call: {}",
            compound
        );

        let exact = lower_to_freecad(
            r#"(model (part body (fillet 1 :edges "target-id:body:edge:0:0-0-0_10-0-0" (box 20 20 10))))"#,
        )
        .expect("exact selector lower");
        assert!(
            exact.contains("_ecky_fillet(_f0, 1.0, {'kind': 'targetIds', 'targetIds': [\"body:edge:0:0-0-0_10-0-0\"]}, \"body\")"),
            "exact selector call: {}",
            exact
        );
        assert!(
            exact.contains("def _ecky_edge_target_id(edge, edge_index, object_name):"),
            "exact selector helper missing: {}",
            exact
        );

        let exact_shell = lower_to_freecad(
            r#"(model (part body (shell 1.5 :faces "target-id:body:face:0:0-0-10:400" (box 20 20 10))))"#,
        )
        .expect("exact shell selector lower");
        assert!(
            exact_shell.contains("_ecky_shell(_f0, 1.5, {'kind': 'targetIds', 'targetIds': [\"body:face:0:0-0-10:400\"]}, \"body\")"),
            "exact shell selector call: {}",
            exact_shell
        );
        assert!(
            exact_shell.contains("def _ecky_face_target_id(face, face_index, object_name):"),
            "exact shell selector helper missing face target id helper: {}",
            exact_shell
        );
        assert!(
            exact_shell
                .contains("def _ecky_select_shell_faces(shape, selector, object_name=None):"),
            "exact shell selector helper missing shell face selector helper: {}",
            exact_shell
        );

        let coarse_shell =
            lower_to_freecad(r#"(model (part body (shell 1.5 :faces "top" (box 20 20 10))))"#)
                .expect("coarse shell selector lower");
        assert!(
            coarse_shell.contains(
                "_ecky_shell(_f0, 1.5, {'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'z', 'bound': 'max'}]}, \"body\")"
            ),
            "coarse shell selector call: {}",
            coarse_shell
        );

        let richer_shell = lower_to_freecad(
            r#"(model (part body (shell 1.5 :faces "planar+normal-z+area-max" (box 20 20 10))))"#,
        )
        .expect("richer shell selector lower");
        assert!(
            richer_shell.contains(
                "_ecky_shell(_f0, 1.5, {'kind': 'clauses', 'clauses': [{'kind': 'planar'}, {'kind': 'normal', 'axis': 'z'}, {'kind': 'area', 'rank': 'max'}]}, \"body\")"
            ),
            "richer shell selector call: {}",
            richer_shell
        );
    }

    #[test]
    fn lower_core_program_to_freecad_supports_typed_selector_nodes() {
        let program = crate::ecky_scheme::compile_to_core_program(
            r#"(model (part body (fillet 1 :edges "target-id:body:edge:0:0-0-0_10-0-0" (box 20 20 10))))"#,
        )
        .expect("program");
        let code = lower_core_program_to_freecad(&program).expect("lower");
        assert!(
            code.contains("_ecky_fillet(_f0, 1.0, {'kind': 'targetIds', 'targetIds': [\"body:edge:0:0-0-0_10-0-0\"]}, \"body\")"),
            "typed selector core lower: {}",
            code
        );
    }

    #[test]
    fn lower_core_program_to_freecad_supports_coarse_selector_payload_when_value_is_bad() {
        let mut program = crate::ecky_scheme::compile_to_core_program(
            r#"(model (part body (fillet 1 :edges "left+vertical" (box 20 20 10))))"#,
        )
        .expect("program");
        let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } =
            &mut program.parts[0].root.kind
        else {
            panic!("expected call");
        };
        *keywords[0].source_node_mut() = crate::ecky_core_ir::CoreNode::new(
            crate::ecky_core_ir::NodeId::new(99_002),
            crate::ecky_core_ir::CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(
                7.0,
            )),
            crate::ecky_core_ir::CoreValueKind::Number,
        );
        let code = lower_core_program_to_freecad(&program).expect("lower");
        assert!(
            code.contains("_ecky_fillet(_f0, 1.0, {'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'x', 'bound': 'min'}, {'kind': 'axis', 'axis': 'z'}]}, \"body\")"),
            "typed coarse selector core lower: {}",
            code
        );
    }

    #[test]
    fn freecad_lowering_emits_dict_only_selector_helpers() {
        let code = lower_to_freecad(
            r#"(model (part body (union
                (fillet 1 :edges "left+vertical" (box 20 20 10))
                (translate 40 0 0 (shell 1.5 :faces "target-id:body:face:0:0-0-10:400" (box 20 20 10))))))"#,
        )
        .expect("lower");
        assert!(
            !code.contains("def _ecky_parse_target_ids("),
            "legacy target-id string parser leaked: {}",
            code
        );
        assert!(
            !code.contains("def _ecky_edge_selector_clauses("),
            "legacy coarse string parser leaked: {}",
            code
        );
        assert!(
            code.contains("requires typed selector payload"),
            "typed selector helper guard missing: {}",
            code
        );
        assert!(
            code.contains("def _ecky_fillet(shape, radius, selector=None, object_name=None):")
                && code.contains(
                    "def _ecky_chamfer(shape, distance, selector=None, object_name=None):"
                ),
            "selector defaults not normalized: {}",
            code
        );
    }

    #[test]
    fn lower_core_program_to_freecad_rejects_missing_selector_payload_on_edges_keyword() {
        let mut program = crate::ecky_scheme::compile_to_core_program(
            r#"(model (part body (fillet 1 :edges "left+vertical" (box 20 20 10))))"#,
        )
        .expect("program");
        let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } =
            &mut program.parts[0].root.kind
        else {
            panic!("expected call");
        };
        keywords[0].set_selector_payload(None);

        let err = lower_core_program_to_freecad(&program)
            .expect_err("missing selector payload should fail");
        assert!(
            err.to_string()
                .contains("CoreProgram `:edges` keyword requires selector payload"),
            "{err}"
        );
    }

    #[test]
    fn lower_core_program_to_freecad_rejects_wrong_kind_selector_payload_on_edges_keyword() {
        let mut program = crate::ecky_scheme::compile_to_core_program(
            r#"(model (part body (fillet 1 :edges "left+vertical" (box 20 20 10))))"#,
        )
        .expect("program");
        let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } =
            &mut program.parts[0].root.kind
        else {
            panic!("expected call");
        };
        keywords[0].set_selector_payload(Some(
            crate::ecky_core_ir::CoreSelectorPayload::FaceTargetIds(vec![
                "body:face:0:0-0-1:1".into()
            ]),
        ));

        let err = lower_core_program_to_freecad(&program)
            .expect_err("wrong-kind selector payload should fail");
        assert!(
            err.to_string()
                .contains("CoreProgram `:edges` keyword requires edge selector payload"),
            "{err}"
        );
    }

    #[test]
    fn lower_core_program_to_freecad_rejects_created_by_selector_option() {
        let program = crate::ecky_scheme::compile_to_core_program(
            r#"
            (model
              (part body
                (build
                  (shape blank (box 10 10 10))
                  (shape pocket (box 4 4 4))
                  (shape solid (difference blank pocket))
                  (result
                    (fillet 1
                      :edges "left+vertical"
                      :created-by pocket
                      solid)))))
            "#,
        )
        .expect("program");

        let err = lower_core_program_to_freecad(&program)
            .expect_err("created-by should fail in freecad lowering");
        assert!(
            err.to_string()
                .contains("`fillet` does not recognize option `:created-by`"),
            "{err}"
        );
    }

    #[test]
    fn freecad_lowering_rejects_unknown_edge_selector() {
        let err = lower_to_freecad(r#"(model (part body (fillet 1 :edges side (box 10 10 10))))"#)
            .expect_err("unknown selector should fail");
        assert!(
            err.message
                .contains("Unknown edge selector `side`. Use `all`, `top`, `bottom`"),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn freecad_lowering_rejects_wrong_kind_exact_selectors() {
        let edge_err = lower_to_freecad(
            r#"(model (part body (fillet 1 :edges "target-id:body:face:0:0-0-10:400" (box 20 20 10))))"#,
        )
        .expect_err("face target id should fail edge selector");
        assert!(
            edge_err
                .message
                .contains("included non-edge target id `body:face:0:0-0-10:400`"),
            "{edge_err:?}"
        );

        let face_err = lower_to_freecad(
            r#"(model (part body (shell 1.5 :faces "target-id:body:edge:0:0-0-0_10-0-0" (box 20 20 10))))"#,
        )
        .expect_err("edge target id should fail face selector");
        assert!(
            face_err
                .message
                .contains("included non-face target id `body:edge:0:0-0-0_10-0-0`"),
            "{face_err:?}"
        );
    }
}
