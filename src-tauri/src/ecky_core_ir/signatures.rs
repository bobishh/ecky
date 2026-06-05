use std::collections::HashMap;

use super::{
    CompilerError, CompilerErrorKind, CoreArrayOp, CoreBinding, CoreBooleanOp, CoreFrameOp,
    CoreKeywordArg, CoreLiteral, CoreMetaOp, CoreNode, CoreNodeKind, CoreOperation, CorePathOp,
    CorePrimitive, CoreProgram, CoreReference, CoreResult, CoreSelectorPayload, CoreSurfaceOp,
    CoreTransformOp, CoreValueKind, NodeId, ParamId, SourceSpan,
};

#[derive(Debug, Clone, Default)]
struct KindEnv {
    locals: HashMap<String, CoreValueKind>,
    local_list_items: HashMap<String, CoreValueKind>,
    local_dimensions: HashMap<String, UnitDimension>,
    nodes: HashMap<NodeId, CoreValueKind>,
    node_list_items: HashMap<NodeId, CoreValueKind>,
    node_dimensions: HashMap<NodeId, UnitDimension>,
    param_dimensions: HashMap<ParamId, UnitDimension>,
    literal_dimensions: HashMap<SourceSpan, UnitDimension>,
    unit_mode: UnitCheckMode,
}

impl KindEnv {
    fn for_program(
        program: &CoreProgram,
        unit_mode: UnitCheckMode,
        literal_dimensions: &HashMap<SourceSpan, UnitDimension>,
    ) -> Self {
        let param_dimensions = program
            .parameters
            .iter()
            .filter_map(|param| {
                unit_dimension_from_unit_name(param.constraints.unit.as_deref())
                    .map(|dimension| (param.id, dimension))
            })
            .collect();
        Self {
            param_dimensions,
            literal_dimensions: literal_dimensions.clone(),
            unit_mode,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum UnitCheckMode {
    #[default]
    Permissive,
    Strict,
}

#[derive(Debug, Clone, Copy)]
enum ExpectedKind {
    Any,
    Boolean,
    Number,
    List,
    Point2List,
    Point3List,
    Point3,
    Sketch,
    Path,
    Frame,
    Shape,
    Solid,
}

#[derive(Debug, Clone, Copy)]
struct ArgSpec {
    name: &'static str,
    expected: ExpectedKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitDimension {
    Length,
    Angle,
    Scalar,
}

#[derive(Debug, Clone, Copy)]
enum DimensionSlots {
    Fixed(&'static [Option<UnitDimension>]),
    NumericPrefixExceptTrailing {
        dimension: UnitDimension,
        trailing_args: usize,
    },
}

#[derive(Debug, Clone, Copy)]
struct OpDimensionSignature {
    slots: DimensionSlots,
}

const NO_DIMS: &[Option<UnitDimension>] = &[];
const BOX_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
];
const ONE_LENGTH_DIM: &[Option<UnitDimension>] = &[Some(UnitDimension::Length)];
const TWO_LENGTH_DIMS: &[Option<UnitDimension>] =
    &[Some(UnitDimension::Length), Some(UnitDimension::Length)];
const CYLINDER_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Scalar),
];
const CIRCLE_DIMS: &[Option<UnitDimension>] =
    &[Some(UnitDimension::Length), Some(UnitDimension::Scalar)];
const CONE_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Scalar),
];
const TORUS_DIMS: &[Option<UnitDimension>] =
    &[Some(UnitDimension::Length), Some(UnitDimension::Length)];
const ELLIPSE_DIMS: &[Option<UnitDimension>] =
    &[Some(UnitDimension::Length), Some(UnitDimension::Length)];
const SLOT_DIMS: &[Option<UnitDimension>] =
    &[Some(UnitDimension::Length), Some(UnitDimension::Length)];
const SLOT_ARC_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Angle),
    Some(UnitDimension::Angle),
    Some(UnitDimension::Length),
];
const WEDGE_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
];
const ROUNDED_RECTANGLE_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
];
const ROUNDED_POLYGON_DIMS: &[Option<UnitDimension>] = &[
    None,
    Some(UnitDimension::Length),
    Some(UnitDimension::Scalar),
];
const TRANSLATE_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    None,
];
const ROTATE_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Angle),
    Some(UnitDimension::Angle),
    Some(UnitDimension::Angle),
    None,
];
const MIRROR_DIMS: &[Option<UnitDimension>] = &[None, Some(UnitDimension::Length), None];
const EXTRUDE_DIMS: &[Option<UnitDimension>] = &[None, Some(UnitDimension::Length)];
const REVOLVE_DIMS: &[Option<UnitDimension>] = &[
    None,
    Some(UnitDimension::Angle),
    Some(UnitDimension::Scalar),
];
const LOFT_DIMS: &[Option<UnitDimension>] = &[Some(UnitDimension::Length)];
const SHELL_DIMS: &[Option<UnitDimension>] = &[Some(UnitDimension::Length), None];
const OFFSET_DIMS: &[Option<UnitDimension>] = &[Some(UnitDimension::Length), None];
const TAPER_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Scalar),
    Some(UnitDimension::Scalar),
    None,
];
const TWIST_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Length),
    Some(UnitDimension::Angle),
    None,
];
const DRAFT_DIMS: &[Option<UnitDimension>] = &[Some(UnitDimension::Angle), None];
const LINEAR_ARRAY_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Scalar),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    None,
];
const RADIAL_ARRAY_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Scalar),
    Some(UnitDimension::Angle),
    Some(UnitDimension::Length),
    None,
];
const GRID_ARRAY_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Scalar),
    Some(UnitDimension::Scalar),
    Some(UnitDimension::Length),
    Some(UnitDimension::Length),
    None,
];
const ARC_ARRAY_DIMS: &[Option<UnitDimension>] = &[
    Some(UnitDimension::Scalar),
    Some(UnitDimension::Length),
    Some(UnitDimension::Angle),
    Some(UnitDimension::Angle),
    None,
];

pub fn verify_core_program(program: &CoreProgram) -> CoreResult<()> {
    verify_core_program_with_unit_mode(program, UnitCheckMode::Permissive)
}

pub fn verify_core_program_strict_units(program: &CoreProgram) -> CoreResult<()> {
    verify_core_program_with_unit_mode(program, UnitCheckMode::Strict)
}

pub(crate) fn verify_core_program_with_literal_dimensions(
    program: &CoreProgram,
    literal_dimensions: &HashMap<SourceSpan, String>,
    strict_units: bool,
) -> CoreResult<Vec<CompilerError>> {
    let literal_dimensions = literal_dimensions
        .iter()
        .filter_map(|(span, dimension)| {
            unit_dimension_from_unit_name(Some(dimension.as_str())).map(|kind| (*span, kind))
        })
        .collect::<HashMap<_, _>>();
    verify_core_program_with_unit_mode_and_literal_dimensions(
        program,
        if strict_units {
            UnitCheckMode::Strict
        } else {
            UnitCheckMode::Permissive
        },
        &literal_dimensions,
    )
}

fn verify_core_program_with_unit_mode(
    program: &CoreProgram,
    unit_mode: UnitCheckMode,
) -> CoreResult<()> {
    verify_core_program_with_unit_mode_and_literal_dimensions(program, unit_mode, &HashMap::new())
        .map(|_| ())
}

fn verify_core_program_with_unit_mode_and_literal_dimensions(
    program: &CoreProgram,
    unit_mode: UnitCheckMode,
    literal_dimensions: &HashMap<SourceSpan, UnitDimension>,
) -> CoreResult<Vec<CompilerError>> {
    let env = KindEnv::for_program(program, unit_mode, literal_dimensions);
    let mut warnings = Vec::new();
    for part in &program.parts {
        verify_node(&part.root, &env, &mut warnings)?;
    }
    Ok(warnings)
}

fn verify_node(
    node: &CoreNode,
    env: &KindEnv,
    warnings: &mut Vec<CompilerError>,
) -> CoreResult<()> {
    match &node.kind {
        CoreNodeKind::Literal(literal) => verify_literal_node(node, literal),
        CoreNodeKind::Reference(_) => Ok(()),
        CoreNodeKind::Build { bindings, result } => {
            let mut nested = env.clone();
            for binding in bindings {
                verify_node(&binding.value, &nested, warnings)?;
                nested
                    .nodes
                    .insert(binding.value.id, effective_kind(&binding.value, &nested));
                if let Some(dimension) = effective_dimension(&binding.value, &nested) {
                    nested.node_dimensions.insert(binding.value.id, dimension);
                } else {
                    nested.node_dimensions.remove(&binding.value.id);
                }
                if let Some(item_kind) = list_item_kind(&binding.value, &nested) {
                    nested.node_list_items.insert(binding.value.id, item_kind);
                }
            }
            verify_node(result, &nested, warnings)
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested = env.clone();
            for binding in bindings {
                verify_node(&binding.value, &nested, warnings)?;
                let kind = effective_kind(&binding.value, &nested);
                nested.locals.insert(binding.name.clone(), kind);
                nested.nodes.insert(binding.value.id, kind);
                if let Some(dimension) = effective_dimension(&binding.value, &nested) {
                    nested
                        .local_dimensions
                        .insert(binding.name.clone(), dimension);
                    nested.node_dimensions.insert(binding.value.id, dimension);
                } else {
                    nested.local_dimensions.remove(&binding.name);
                    nested.node_dimensions.remove(&binding.value.id);
                }
                if let Some(item_kind) = list_item_kind(&binding.value, &nested) {
                    nested
                        .local_list_items
                        .insert(binding.name.clone(), item_kind);
                    nested.node_list_items.insert(binding.value.id, item_kind);
                } else {
                    nested.local_list_items.remove(&binding.name);
                }
            }
            verify_node(body, &nested, warnings)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            verify_node(condition, env, warnings)?;
            verify_expected_node("if", 0, "condition", ExpectedKind::Boolean, condition, env)?;
            verify_node(then_branch, env, warnings)?;
            verify_node(else_branch, env, warnings)?;
            let then_kind = effective_kind(then_branch, env);
            let else_kind = effective_kind(else_branch, env);
            if kinds_are_known_and_distinct(then_kind, else_kind) {
                return Err(type_error(
                    "if",
                    "branches expected matching branch kinds",
                    &format!(
                        "then branch got {}, else branch got {}",
                        kind_label(then_kind),
                        kind_label(else_kind)
                    ),
                    node.span,
                ));
            }
            Ok(())
        }
        CoreNodeKind::Call { op, args, keywords } => {
            for arg in args {
                verify_node(arg, env, warnings)?;
            }
            for keyword in keywords {
                verify_node(keyword.source_node(), env, warnings)?;
            }
            verify_call(op, args, keywords, node, env, warnings)
        }
        CoreNodeKind::Range { start, end } => {
            verify_node(start, env, warnings)?;
            verify_node(end, env, warnings)?;
            verify_expected_node("range", 0, "start", ExpectedKind::Number, start, env)?;
            verify_expected_node("range", 1, "end", ExpectedKind::Number, end, env)
        }
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => {
            if params.len() != sources.len() {
                return Err(type_error(
                    "map",
                    "expected one parameter per source",
                    &format!(
                        "got {} parameter(s) and {} source(s)",
                        params.len(),
                        sources.len()
                    ),
                    node.span,
                ));
            }
            let mut nested = env.clone();
            for (index, source) in sources.iter().enumerate() {
                verify_node(source, env, warnings)?;
                verify_expected_node("map", index, "source", ExpectedKind::List, source, env)?;
                if let Some(item_kind) = list_item_kind(source, env) {
                    nested.locals.insert(params[index].clone(), item_kind);
                } else {
                    nested.locals.remove(&params[index]);
                    nested.local_list_items.remove(&params[index]);
                }
            }
            verify_node(body, &nested, warnings)
        }
        CoreNodeKind::Apply { op, args, list } => {
            for arg in args {
                verify_node(arg, env, warnings)?;
            }
            verify_node(list, env, warnings)?;
            verify_apply(op, args, list, node, env)
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                verify_node(item, env, warnings)?;
            }
            if matches!(
                node.value_kind,
                CoreValueKind::Point2 | CoreValueKind::Point3
            ) {
                verify_point_list(node, items, env)?;
            }
            Ok(())
        }
    }
}

fn verify_literal_node(node: &CoreNode, literal: &CoreLiteral) -> CoreResult<()> {
    if node.value_kind == CoreValueKind::Any || node.value_kind == literal.kind() {
        return Ok(());
    }
    Err(type_error(
        "literal",
        "result expected literal kind",
        &format!(
            "expected {}, got {}",
            kind_label(literal.kind()),
            kind_label(node.value_kind)
        ),
        node.span,
    ))
}

fn verify_point_list(node: &CoreNode, items: &[CoreNode], env: &KindEnv) -> CoreResult<()> {
    let expected_len = match node.value_kind {
        CoreValueKind::Point2 => 2,
        CoreValueKind::Point3 => 3,
        _ => return Ok(()),
    };
    if items.len() != expected_len {
        return Err(type_error(
            "point",
            "result expected point component count",
            &format!("expected {expected_len}, got {}", items.len()),
            node.span,
        ));
    }
    for (index, item) in items.iter().enumerate() {
        verify_expected_node("point", index, "component", ExpectedKind::Number, item, env)?;
    }
    Ok(())
}

fn verify_call(
    op: &CoreOperation,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    node: &CoreNode,
    env: &KindEnv,
    warnings: &mut Vec<CompilerError>,
) -> CoreResult<()> {
    let name = operation_name(op);
    match op {
        CoreOperation::Primitive(primitive) => {
            verify_primitive(primitive.clone(), &name, args, node, env)
        }
        CoreOperation::Boolean(boolean) => verify_boolean(boolean.clone(), &name, args, node, env),
        CoreOperation::Transform(transform) => {
            verify_transform(transform.clone(), &name, args, node, env)
        }
        CoreOperation::Surface(surface) => verify_surface(surface.clone(), &name, args, node, env),
        CoreOperation::Path(path) => verify_path(path.clone(), &name, args, node, env),
        CoreOperation::Array(array) => verify_array(array.clone(), &name, args, node, env),
        CoreOperation::Frame(frame) => verify_frame(frame.clone(), &name, args, node, env),
        CoreOperation::Meta(meta) => verify_meta(meta.clone(), &name, args, node, env),
        CoreOperation::Custom(custom) if custom == "hole" => {
            verify_typed_hole(&name, args, keywords, node)
        }
        CoreOperation::Custom(_) => Ok(()),
    }?;
    verify_keywords(&name, keywords, env)?;
    verify_call_dimensions(op, &name, args, env, warnings)
}

fn verify_typed_hole(
    name: &str,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    node: &CoreNode,
) -> CoreResult<()> {
    verify_exact(name, args, &[], &KindEnv::default())?;
    let type_keyword = keywords
        .iter()
        .find(|keyword| keyword.name == "type")
        .ok_or_else(|| type_error(name, "expected `:type`", "got no type", node.span))?;
    let CoreNodeKind::Literal(CoreLiteral::Text(type_name)) = &type_keyword.source_node().kind
    else {
        return Err(type_error(
            name,
            "`:type` expected text",
            &format!("got {}", kind_label(type_keyword.source_node().value_kind)),
            type_keyword.source_node().span,
        ));
    };
    let expected = typed_hole_kind(type_name).ok_or_else(|| {
        type_error(
            name,
            "`:type` expected solid, sketch, path, or shape",
            &format!("got `{}`", type_name),
            type_keyword.source_node().span,
        )
    })?;
    verify_result(name, expected, node, &KindEnv::default())?;

    if let Some(goal) = keywords.iter().find(|keyword| keyword.name == "goal") {
        if !matches!(
            goal.source_node().kind,
            CoreNodeKind::Literal(CoreLiteral::Text(_))
        ) {
            return Err(type_error(
                name,
                "`:goal` expected text",
                &format!("got {}", kind_label(goal.source_node().value_kind)),
                goal.source_node().span,
            ));
        }
    }

    Ok(())
}

fn verify_primitive(
    primitive: CorePrimitive,
    name: &str,
    args: &[CoreNode],
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    match primitive {
        CorePrimitive::Box => {
            verify_exact(
                name,
                args,
                &[num("width"), num("depth"), num("height")],
                env,
            )?;
            verify_result(name, ExpectedKind::Solid, node, env)
        }
        CorePrimitive::Sphere => {
            verify_exact(name, args, &[num("radius")], env)?;
            verify_result(name, ExpectedKind::Solid, node, env)
        }
        CorePrimitive::Cylinder => {
            verify_between(name, args, 2, 3, "radius, height, and optional segments")?;
            verify_prefix(name, args, &[num("radius"), num("height")], env)?;
            verify_optional(name, args, 2, num("segments"), env)?;
            verify_result(name, ExpectedKind::Solid, node, env)
        }
        CorePrimitive::Cone => {
            verify_between(
                name,
                args,
                3,
                4,
                "radius1, radius2, height, and optional segments",
            )?;
            verify_prefix(
                name,
                args,
                &[num("radius1"), num("radius2"), num("height")],
                env,
            )?;
            verify_optional(name, args, 3, num("segments"), env)?;
            verify_result(name, ExpectedKind::Solid, node, env)
        }
        CorePrimitive::Torus => {
            verify_exact(name, args, &[num("major_radius"), num("minor_radius")], env)?;
            verify_result(name, ExpectedKind::Solid, node, env)
        }
        CorePrimitive::Ellipse => {
            verify_exact(name, args, &[num("x_radius"), num("y_radius")], env)?;
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::Slot => {
            verify_exact(name, args, &[num("length"), num("width")], env)?;
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::SlotArc => {
            verify_exact(
                name,
                args,
                &[
                    num("radius"),
                    num("start_angle"),
                    num("end_angle"),
                    num("width"),
                ],
                env,
            )?;
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::Wedge => {
            verify_exact(
                name,
                args,
                &[
                    num("dx"),
                    num("dy"),
                    num("dz"),
                    num("xmin"),
                    num("zmin"),
                    num("xmax"),
                    num("zmax"),
                ],
                env,
            )?;
            verify_result(name, ExpectedKind::Solid, node, env)
        }
        CorePrimitive::Circle => {
            verify_between(name, args, 1, 2, "radius and optional segments")?;
            verify_prefix(name, args, &[num("radius")], env)?;
            verify_optional(name, args, 1, num("segments"), env)?;
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::Rectangle => {
            verify_exact(name, args, &[num("width"), num("height")], env)?;
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::RoundedRectangle => {
            verify_exact(
                name,
                args,
                &[num("width"), num("height"), num("radius")],
                env,
            )?;
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::RoundedPolygon => {
            verify_between(name, args, 2, 3, "points, radius, and optional segments")?;
            verify_prefix(name, args, &[point2_list("points"), num("radius")], env)?;
            verify_optional(name, args, 2, num("segments"), env)?;
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::Polygon => {
            verify_exact(name, args, &[point2_list("points")], env)?;
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::Profile => {
            for (index, arg) in args.iter().enumerate() {
                verify_expected_node(name, index, "loop", ExpectedKind::Sketch, arg, env)?;
            }
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::MakeFace => {
            for (index, arg) in args.iter().enumerate() {
                verify_expected_node(name, index, "wire", ExpectedKind::Shape, arg, env)?;
            }
            verify_result(name, ExpectedKind::Sketch, node, env)
        }
        CorePrimitive::Text | CorePrimitive::Svg | CorePrimitive::Stl => Ok(()),
    }
}

fn verify_boolean(
    _boolean: CoreBooleanOp,
    name: &str,
    args: &[CoreNode],
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    verify_min_arity(name, args, 1, "at least one shape")?;
    for (index, arg) in args.iter().enumerate() {
        verify_expected_node(name, index, "shape", ExpectedKind::Shape, arg, env)?;
    }
    verify_result(name, ExpectedKind::Shape, node, env)
}

fn verify_transform(
    transform: CoreTransformOp,
    name: &str,
    args: &[CoreNode],
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    match transform {
        CoreTransformOp::Translate | CoreTransformOp::Rotate => {
            verify_exact(
                name,
                args,
                &[num("x"), num("y"), num("z"), shape("shape")],
                env,
            )?;
        }
        CoreTransformOp::Scale => {
            verify_between(name, args, 2, 4, "scale factor(s) and shape")?;
            for (index, arg) in args[..args.len() - 1].iter().enumerate() {
                verify_expected_node(name, index, "factor", ExpectedKind::Number, arg, env)?;
            }
            verify_expected_node(
                name,
                args.len() - 1,
                "shape",
                ExpectedKind::Shape,
                args.last().expect("scale shape"),
                env,
            )?;
        }
        CoreTransformOp::Mirror => {
            verify_exact(
                name,
                args,
                &[any("axis"), num("offset"), shape("shape")],
                env,
            )?;
        }
    }
    verify_result(name, ExpectedKind::Shape, node, env)
}

fn verify_surface(
    surface: CoreSurfaceOp,
    name: &str,
    args: &[CoreNode],
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    match surface {
        CoreSurfaceOp::Extrude | CoreSurfaceOp::Revolve => {
            verify_exact(name, args, &[sketch("profile"), num("distance")], env)?;
        }
        CoreSurfaceOp::Loft => {
            verify_min_arity(name, args, 3, "distance and at least two profiles")?;
            verify_expected_node(name, 0, "distance", ExpectedKind::Number, &args[0], env)?;
            for (index, arg) in args.iter().enumerate().skip(1) {
                verify_expected_node(name, index, "profile", ExpectedKind::Sketch, arg, env)?;
            }
        }
        CoreSurfaceOp::Sweep => {
            verify_exact(name, args, &[sketch("profile"), path("path")], env)?;
        }
        CoreSurfaceOp::Shell => {
            verify_exact(name, args, &[num("thickness"), solid("solid")], env)?;
        }
        CoreSurfaceOp::Offset | CoreSurfaceOp::OffsetRounded => {
            verify_min_arity(name, args, 2, "amount and profile")?;
            verify_expected_node(name, 0, "amount", ExpectedKind::Number, &args[0], env)?;
            verify_expected_node(name, 1, "profile", ExpectedKind::Sketch, &args[1], env)?;
            verify_result(name, ExpectedKind::Sketch, node, env)?;
            return Ok(());
        }
        CoreSurfaceOp::Fillet | CoreSurfaceOp::Chamfer => {
            verify_exact(name, args, &[num("radius"), solid("solid")], env)?;
        }
        CoreSurfaceOp::Taper => {
            verify_between(
                name,
                args,
                3,
                4,
                "height, scale, profile or height, scale-x, scale-y, profile",
            )?;
            verify_expected_node(name, 0, "height", ExpectedKind::Number, &args[0], env)?;
            for index in 1..args.len() - 1 {
                verify_expected_node(
                    name,
                    index,
                    "scale",
                    ExpectedKind::Number,
                    &args[index],
                    env,
                )?;
            }
            verify_expected_node(
                name,
                args.len() - 1,
                "profile",
                ExpectedKind::Sketch,
                args.last().expect("taper profile"),
                env,
            )?;
        }
        CoreSurfaceOp::Twist => {
            verify_exact(
                name,
                args,
                &[num("height"), num("angle"), sketch("profile")],
                env,
            )?;
        }
        CoreSurfaceOp::Draft => {
            verify_exact(name, args, &[num("angle"), solid("solid")], env)?;
        }
    }
    verify_result(name, ExpectedKind::Solid, node, env)
}

fn verify_path(
    path: CorePathOp,
    name: &str,
    args: &[CoreNode],
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    match path {
        CorePathOp::Polyline => {
            if args.len() == 1 {
                verify_expected_node(name, 0, "points", ExpectedKind::Point3List, &args[0], env)?;
            } else {
                verify_min_arity(name, args, 2, "point list or at least two 3D points")?;
                for (index, arg) in args.iter().enumerate() {
                    verify_expected_node(name, index, "point", ExpectedKind::Point3, arg, env)?;
                }
            }
        }
        CorePathOp::BezierPath | CorePathOp::Bspline => {
            verify_min_arity(name, args, 1, "point list")?;
            let expected = match path {
                CorePathOp::Bspline => ExpectedKind::Point2List,
                CorePathOp::BezierPath => ExpectedKind::Point3List,
                CorePathOp::Polyline => unreachable!(),
            };
            verify_expected_node(name, 0, "points", expected, &args[0], env)?;
        }
    }
    let expected_result = match path {
        CorePathOp::Bspline => ExpectedKind::Sketch,
        CorePathOp::Polyline | CorePathOp::BezierPath => ExpectedKind::Path,
    };
    verify_result(name, expected_result, node, env)
}

fn verify_array(
    array: CoreArrayOp,
    name: &str,
    args: &[CoreNode],
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    match array {
        CoreArrayOp::LinearArray => {
            verify_exact(
                name,
                args,
                &[num("count"), num("x"), num("y"), num("z"), shape("shape")],
                env,
            )?;
        }
        CoreArrayOp::RadialArray => {
            verify_exact(
                name,
                args,
                &[num("count"), num("angle"), num("radius"), shape("shape")],
                env,
            )?;
        }
        CoreArrayOp::GridArray => {
            verify_exact(
                name,
                args,
                &[num("rows"), num("cols"), num("x"), num("y"), shape("shape")],
                env,
            )?;
        }
        CoreArrayOp::ArcArray => {
            verify_exact(
                name,
                args,
                &[
                    num("count"),
                    num("radius"),
                    num("start-angle"),
                    num("end-angle"),
                    shape("shape"),
                ],
                env,
            )?;
        }
        CoreArrayOp::Repeat
        | CoreArrayOp::RepeatUnion
        | CoreArrayOp::RepeatCompound
        | CoreArrayOp::RepeatPick => {
            verify_min_arity(name, args, 2, "repeat bindings and body")?;
        }
    }
    verify_result(name, ExpectedKind::Shape, node, env)
}

fn verify_frame(
    frame: CoreFrameOp,
    name: &str,
    args: &[CoreNode],
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    match frame {
        CoreFrameOp::Plane => verify_max_arity(name, args, 0, "keywords only")?,
        CoreFrameOp::Location => {
            verify_between(name, args, 0, 1, "optional frame")?;
            verify_optional(name, args, 0, frame_arg("frame"), env)?;
        }
        CoreFrameOp::PathFrame => verify_exact(name, args, &[path("path")], env)?,
        CoreFrameOp::Place => verify_exact(name, args, &[frame_arg("frame"), shape("shape")], env)?,
        CoreFrameOp::ClipBox => verify_exact(name, args, &[shape("shape")], env)?,
    }
    match frame {
        CoreFrameOp::Plane | CoreFrameOp::Location | CoreFrameOp::PathFrame => {
            verify_result(name, ExpectedKind::Frame, node, env)
        }
        CoreFrameOp::Place | CoreFrameOp::ClipBox => {
            verify_result(name, ExpectedKind::Shape, node, env)
        }
    }
}

fn verify_meta(
    meta: CoreMetaOp,
    name: &str,
    args: &[CoreNode],
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    match meta {
        CoreMetaOp::Group => {
            for (index, arg) in args.iter().enumerate() {
                verify_expected_node(name, index, "shape", ExpectedKind::Shape, arg, env)?;
            }
            verify_result(name, ExpectedKind::Shape, node, env)
        }
        CoreMetaOp::Comment | CoreMetaOp::Annotate => Ok(()),
    }
}

fn verify_apply(
    op: &CoreOperation,
    args: &[CoreNode],
    list: &CoreNode,
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    let name = operation_name(op);
    verify_expected_node(&name, args.len(), "list", ExpectedKind::List, list, env)?;
    match op {
        CoreOperation::Boolean(_) | CoreOperation::Meta(CoreMetaOp::Group) => {
            for (index, arg) in args.iter().enumerate() {
                verify_expected_node(&name, index, "shape", ExpectedKind::Shape, arg, env)?;
            }
            verify_expected_list_items(
                &name,
                args.len(),
                "list item",
                ExpectedKind::Shape,
                list,
                env,
            )?;
            verify_result(&name, ExpectedKind::Shape, node, env)
        }
        _ => Ok(()),
    }
}

fn verify_keywords(name: &str, keywords: &[CoreKeywordArg], env: &KindEnv) -> CoreResult<()> {
    for keyword in keywords {
        match (keyword.name.as_str(), keyword.selector_payload()) {
            (
                "edges",
                Some(CoreSelectorPayload::FaceClauses(_) | CoreSelectorPayload::FaceTargetIds(_)),
            ) => {
                return Err(type_error(
                    name,
                    "`:edges` expected edge selector payload",
                    "got face selector payload",
                    keyword.source_node().span,
                ))
            }
            ("edges", None) => {
                return Err(type_error(
                    name,
                    "`:edges` expected selector payload",
                    "got no selector payload",
                    keyword.source_node().span,
                ))
            }
            ("faces", Some(CoreSelectorPayload::EdgeAll))
            | ("faces", Some(CoreSelectorPayload::EdgeClauses(_)))
            | ("faces", Some(CoreSelectorPayload::EdgeTargetIds(_))) => {
                return Err(type_error(
                    name,
                    "`:faces` expected face selector payload",
                    "got edge selector payload",
                    keyword.source_node().span,
                ))
            }
            ("faces", None) => {
                return Err(type_error(
                    name,
                    "`:faces` expected selector payload",
                    "got no selector payload",
                    keyword.source_node().span,
                ))
            }
            _ => {}
        }
        if keyword.name == "openings" {
            verify_openings_keyword(name, keyword, env)?;
            continue;
        }
        let expected = match (name, keyword.name.as_str()) {
            ("clip-box", "x" | "y" | "z") => Some(ExpectedKind::List),
            (_, "offset" | "rotate" | "origin" | "x" | "normal") => Some(ExpectedKind::Point3),
            (_, _) => None,
        };
        if let Some(expected) = expected {
            verify_expected_node(name, 0, &keyword.name, expected, keyword.source_node(), env)?;
        }
    }
    Ok(())
}

fn verify_openings_keyword(name: &str, keyword: &CoreKeywordArg, env: &KindEnv) -> CoreResult<()> {
    let node = keyword.source_node();
    let actual = effective_kind(node, env);
    if kind_matches(ExpectedKind::Sketch, actual) {
        return Ok(());
    }
    if kind_matches(ExpectedKind::List, actual)
        && matches!(
            list_item_kind(node, env),
            Some(CoreValueKind::Sketch | CoreValueKind::Any)
        )
    {
        return Ok(());
    }
    Err(CompilerError::new(
        CompilerErrorKind::TypeMismatch,
        format!(
            "op `{}` arg 0 `openings` expected sketch or sketch list, got {}.",
            name,
            kind_label(actual)
        ),
    )
    .with_span(node.span.unwrap_or(SourceSpan::new(None, 0, 0))))
}

fn verify_exact(
    op_name: &str,
    args: &[CoreNode],
    specs: &[ArgSpec],
    env: &KindEnv,
) -> CoreResult<()> {
    if args.len() != specs.len() {
        return Err(arity_error(
            op_name,
            &format!("{} argument(s)", specs.len()),
            args.len(),
            args.first().and_then(|arg| arg.span),
        ));
    }
    verify_prefix(op_name, args, specs, env)
}

fn verify_prefix(
    op_name: &str,
    args: &[CoreNode],
    specs: &[ArgSpec],
    env: &KindEnv,
) -> CoreResult<()> {
    for (index, spec) in specs.iter().enumerate() {
        verify_expected_node(op_name, index, spec.name, spec.expected, &args[index], env)?;
    }
    Ok(())
}

fn verify_optional(
    op_name: &str,
    args: &[CoreNode],
    index: usize,
    spec: ArgSpec,
    env: &KindEnv,
) -> CoreResult<()> {
    if let Some(arg) = args.get(index) {
        verify_expected_node(op_name, index, spec.name, spec.expected, arg, env)?;
    }
    Ok(())
}

fn verify_expected_node(
    op_name: &str,
    index: usize,
    arg_name: &str,
    expected: ExpectedKind,
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    if matches!(
        expected,
        ExpectedKind::Point2List | ExpectedKind::Point3List
    ) {
        return verify_expected_point_list(op_name, index, arg_name, expected, node, env);
    }
    if matches!(expected, ExpectedKind::Point3) {
        return verify_expected_point3(op_name, index, arg_name, node, env);
    }
    let actual = effective_kind(node, env);
    if kind_matches(expected, actual) {
        return Ok(());
    }
    Err(CompilerError::new(
        CompilerErrorKind::TypeMismatch,
        format!(
            "op `{}` arg {} `{}` expected {}, got {}.",
            op_name,
            index,
            arg_name,
            expected_label(expected),
            kind_label(actual)
        ),
    )
    .with_span(node.span.unwrap_or(SourceSpan::new(None, 0, 0))))
}

fn verify_expected_point3(
    op_name: &str,
    index: usize,
    arg_name: &str,
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    let actual = effective_kind(node, env);
    if !matches!(
        actual,
        CoreValueKind::Any | CoreValueKind::List | CoreValueKind::Point3
    ) {
        return Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!(
                "op `{}` arg {} `{}` expected {}, got {}.",
                op_name,
                index,
                arg_name,
                expected_label(ExpectedKind::Point3),
                kind_label(actual)
            ),
        )
        .with_span(node.span.unwrap_or(SourceSpan::new(None, 0, 0))));
    }

    let (CoreNodeKind::List(items) | CoreNodeKind::Group(items)) = &node.kind else {
        return Ok(());
    };
    if items.len() != 3 {
        return Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!(
                "op `{}` arg {} `{}` expected {}, got {} component(s).",
                op_name,
                index,
                arg_name,
                expected_label(ExpectedKind::Point3),
                items.len()
            ),
        )
        .with_span(node.span.unwrap_or(SourceSpan::new(None, 0, 0))));
    }
    for (component_index, item) in items.iter().enumerate() {
        verify_expected_node(
            op_name,
            component_index,
            "point component",
            ExpectedKind::Number,
            item,
            env,
        )?;
    }
    Ok(())
}

fn verify_expected_point_list(
    op_name: &str,
    index: usize,
    arg_name: &str,
    expected: ExpectedKind,
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    let actual = effective_kind(node, env);
    if !matches!(
        actual,
        CoreValueKind::Any | CoreValueKind::List | CoreValueKind::Point2 | CoreValueKind::Point3
    ) {
        return Err(CompilerError::new(
            CompilerErrorKind::TypeMismatch,
            format!(
                "op `{}` arg {} `{}` expected {}, got {}.",
                op_name,
                index,
                arg_name,
                expected_label(expected),
                kind_label(actual)
            ),
        )
        .with_span(node.span.unwrap_or(SourceSpan::new(None, 0, 0))));
    }

    let Some(list_kind) = list_item_kind(node, env) else {
        return Ok(());
    };
    let expected_item = match expected {
        ExpectedKind::Point2List => CoreValueKind::Point2,
        ExpectedKind::Point3List => CoreValueKind::Point3,
        _ => return Ok(()),
    };
    if list_kind == expected_item {
        return Ok(());
    }
    Err(CompilerError::new(
        CompilerErrorKind::TypeMismatch,
        format!(
            "op `{}` arg {} `{}` expected {}, got {} list.",
            op_name,
            index,
            arg_name,
            expected_label(expected),
            kind_label(list_kind)
        ),
    )
    .with_span(node.span.unwrap_or(SourceSpan::new(None, 0, 0))))
}

fn verify_expected_list_items(
    op_name: &str,
    index: usize,
    arg_name: &str,
    expected: ExpectedKind,
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    let Some(item_kind) = list_item_kind(node, env) else {
        return Ok(());
    };
    if kind_matches(expected, item_kind) {
        return Ok(());
    }
    Err(CompilerError::new(
        CompilerErrorKind::TypeMismatch,
        format!(
            "op `{}` arg {} `{}` expected {} items, got {} list.",
            op_name,
            index,
            arg_name,
            expected_label(expected),
            kind_label(item_kind)
        ),
    )
    .with_span(node.span.unwrap_or(SourceSpan::new(None, 0, 0))))
}

fn verify_result(
    op_name: &str,
    expected: ExpectedKind,
    node: &CoreNode,
    env: &KindEnv,
) -> CoreResult<()> {
    let actual = effective_kind(node, env);
    if kind_matches(expected, actual) {
        return Ok(());
    }
    Err(type_error(
        op_name,
        &format!("result expected {}", expected_label(expected)),
        &format!("got {}", kind_label(actual)),
        node.span,
    ))
}

fn kind_matches(expected: ExpectedKind, actual: CoreValueKind) -> bool {
    if matches!(expected, ExpectedKind::Any) || actual == CoreValueKind::Any {
        return true;
    }
    match expected {
        ExpectedKind::Any => true,
        ExpectedKind::Boolean => actual == CoreValueKind::Boolean,
        ExpectedKind::Number => actual == CoreValueKind::Number,
        ExpectedKind::Point2List | ExpectedKind::Point3List => matches!(
            actual,
            CoreValueKind::Any
                | CoreValueKind::List
                | CoreValueKind::Point2
                | CoreValueKind::Point3
        ),
        ExpectedKind::List => matches!(
            actual,
            CoreValueKind::List | CoreValueKind::Point2 | CoreValueKind::Point3
        ),
        ExpectedKind::Point3 => matches!(actual, CoreValueKind::Point3 | CoreValueKind::List),
        ExpectedKind::Sketch => actual == CoreValueKind::Sketch,
        ExpectedKind::Path => actual == CoreValueKind::Path,
        ExpectedKind::Frame => actual == CoreValueKind::Frame,
        ExpectedKind::Shape => matches!(
            actual,
            CoreValueKind::Sketch
                | CoreValueKind::Path
                | CoreValueKind::Compound
                | CoreValueKind::Solid
        ),
        ExpectedKind::Solid => matches!(actual, CoreValueKind::Solid | CoreValueKind::Compound),
    }
}

fn list_item_kind(node: &CoreNode, env: &KindEnv) -> Option<CoreValueKind> {
    match &node.kind {
        CoreNodeKind::Range { .. } => Some(CoreValueKind::Number),
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => homogeneous_item_kind(items, env),
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => {
            let nested = env_with_sequence_item_bindings(params, sources, env)?;
            let item_kind =
                point_tuple_kind(body, &nested).unwrap_or_else(|| effective_kind(body, &nested));
            (item_kind != CoreValueKind::Any).then_some(item_kind)
        }
        CoreNodeKind::Let { bindings, body } => {
            let nested = env_with_let_bindings(bindings, env);
            list_item_kind(body, &nested)
        }
        CoreNodeKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            let then_kind = list_item_kind(then_branch, env)?;
            let else_kind = list_item_kind(else_branch, env)?;
            (then_kind == else_kind).then_some(then_kind)
        }
        CoreNodeKind::Reference(CoreReference::Local(name)) => {
            env.local_list_items.get(name).copied()
        }
        CoreNodeKind::Reference(CoreReference::Node(id)) => env.node_list_items.get(id).copied(),
        _ => None,
    }
}

fn env_with_sequence_item_bindings(
    params: &[String],
    sources: &[CoreNode],
    env: &KindEnv,
) -> Option<KindEnv> {
    if params.len() != sources.len() {
        return None;
    }
    let mut nested = env.clone();
    for (index, source) in sources.iter().enumerate() {
        if let Some(item_kind) = list_item_kind(source, env) {
            nested.locals.insert(params[index].clone(), item_kind);
        } else {
            nested.locals.remove(&params[index]);
            nested.local_list_items.remove(&params[index]);
        }
    }
    Some(nested)
}

fn env_with_let_bindings(bindings: &[CoreBinding], env: &KindEnv) -> KindEnv {
    let mut nested = env.clone();
    for binding in bindings {
        let kind = effective_kind(&binding.value, &nested);
        nested.locals.insert(binding.name.clone(), kind);
        nested.nodes.insert(binding.value.id, kind);
        if let Some(item_kind) = list_item_kind(&binding.value, &nested) {
            nested
                .local_list_items
                .insert(binding.name.clone(), item_kind);
            nested.node_list_items.insert(binding.value.id, item_kind);
        } else {
            nested.local_list_items.remove(&binding.name);
        }
    }
    nested
}

fn homogeneous_item_kind(items: &[CoreNode], env: &KindEnv) -> Option<CoreValueKind> {
    let mut known = None;
    for item in items {
        let item_kind = point_tuple_kind(item, env).unwrap_or_else(|| effective_kind(item, env));
        if matches!(item_kind, CoreValueKind::Any) {
            return None;
        }
        match known {
            None => known = Some(item_kind),
            Some(existing) if existing == item_kind => {}
            Some(_) => return None,
        }
    }
    known
}

fn point_tuple_kind(node: &CoreNode, env: &KindEnv) -> Option<CoreValueKind> {
    let actual = effective_kind(node, env);
    if matches!(actual, CoreValueKind::Point2 | CoreValueKind::Point3) {
        return Some(actual);
    }
    match &node.kind {
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            let tuple_kind = match items.len() {
                2 => CoreValueKind::Point2,
                3 => CoreValueKind::Point3,
                _ => return None,
            };
            items
                .iter()
                .all(|item| effective_kind(item, env) == CoreValueKind::Number)
                .then_some(tuple_kind)
        }
        CoreNodeKind::Let { bindings, body } => {
            let nested = env_with_let_bindings(bindings, env);
            point_tuple_kind(body, &nested)
        }
        CoreNodeKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            let then_kind = point_tuple_kind(then_branch, env)?;
            let else_kind = point_tuple_kind(else_branch, env)?;
            (then_kind == else_kind).then_some(then_kind)
        }
        _ => None,
    }
}

fn typed_hole_kind(type_name: &str) -> Option<ExpectedKind> {
    match type_name.to_ascii_lowercase().as_str() {
        "solid" => Some(ExpectedKind::Solid),
        "sketch" => Some(ExpectedKind::Sketch),
        "path" => Some(ExpectedKind::Path),
        "shape" => Some(ExpectedKind::Shape),
        _ => None,
    }
}

fn effective_kind(node: &CoreNode, env: &KindEnv) -> CoreValueKind {
    match &node.kind {
        CoreNodeKind::Reference(CoreReference::Local(name)) => {
            if node.value_kind != CoreValueKind::Any {
                node.value_kind
            } else {
                env.locals.get(name).copied().unwrap_or(CoreValueKind::Any)
            }
        }
        CoreNodeKind::Reference(CoreReference::Node(id)) => {
            if node.value_kind != CoreValueKind::Any {
                node.value_kind
            } else {
                env.nodes.get(id).copied().unwrap_or(CoreValueKind::Any)
            }
        }
        CoreNodeKind::Literal(literal) if node.value_kind == CoreValueKind::Any => literal.kind(),
        CoreNodeKind::Let { body, .. } if node.value_kind == CoreValueKind::Any => {
            effective_kind(body, env)
        }
        CoreNodeKind::If {
            then_branch,
            else_branch,
            ..
        } if node.value_kind == CoreValueKind::Any => {
            let then_kind = effective_kind(then_branch, env);
            let else_kind = effective_kind(else_branch, env);
            if then_kind == else_kind {
                then_kind
            } else {
                CoreValueKind::Any
            }
        }
        CoreNodeKind::Call {
            op: CoreOperation::Transform(_),
            args,
            ..
        } => args
            .last()
            .map(|arg| effective_kind(arg, env))
            .filter(|kind| *kind != CoreValueKind::Any)
            .unwrap_or(node.value_kind),
        _ => node.value_kind,
    }
}

fn effective_dimension(node: &CoreNode, env: &KindEnv) -> Option<UnitDimension> {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Number(_)) => node.span.and_then(|span| {
            env.literal_dimensions.get(&span).copied().or_else(|| {
                env.literal_dimensions
                    .get(&SourceSpan::new(None, span.start, span.end))
                    .copied()
            })
        }),
        CoreNodeKind::Reference(CoreReference::Parameter(id)) => {
            env.param_dimensions.get(id).copied()
        }
        CoreNodeKind::Reference(CoreReference::Local(name)) => {
            env.local_dimensions.get(name).copied()
        }
        CoreNodeKind::Reference(CoreReference::Node(id)) => env.node_dimensions.get(id).copied(),
        CoreNodeKind::Let { bindings, body } => {
            let nested = env_with_let_bindings(bindings, env);
            effective_dimension(body, &nested)
        }
        CoreNodeKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            let then_dimension = effective_dimension(then_branch, env)?;
            let else_dimension = effective_dimension(else_branch, env)?;
            (then_dimension == else_dimension).then_some(then_dimension)
        }
        _ => None,
    }
}

fn unit_dimension_from_unit_name(unit: Option<&str>) -> Option<UnitDimension> {
    let unit = unit?.to_ascii_lowercase();
    match unit.as_str() {
        "length" | "mm" | "millimeter" | "millimeters" | "cm" | "centimeter" | "centimeters"
        | "m" | "meter" | "meters" | "in" | "inch" | "inches" => Some(UnitDimension::Length),
        "angle" | "deg" | "degree" | "degrees" | "rad" | "radian" | "radians" => {
            Some(UnitDimension::Angle)
        }
        "scalar" | "ratio" | "count" | "segments" => Some(UnitDimension::Scalar),
        _ => None,
    }
}

fn verify_call_dimensions(
    op: &CoreOperation,
    op_name: &str,
    args: &[CoreNode],
    env: &KindEnv,
    warnings: &mut Vec<CompilerError>,
) -> CoreResult<()> {
    let Some(signature) = op_dimension_signature(op) else {
        return Ok(());
    };
    for (index, arg) in args.iter().enumerate() {
        let Some(expected) = expected_dimension_for_arg(signature, index, args.len()) else {
            continue;
        };
        let Some(actual) = effective_dimension(arg, env) else {
            continue;
        };
        if actual != expected {
            let error = CompilerError::new(
                CompilerErrorKind::TypeMismatch,
                format!(
                    "op `{}` arg {} expected {} dimension, got {} dimension.",
                    op_name,
                    index,
                    unit_dimension_label(expected),
                    unit_dimension_label(actual)
                ),
            )
            .with_span(arg.span.unwrap_or(SourceSpan::new(None, 0, 0)));
            if env.unit_mode == UnitCheckMode::Strict {
                return Err(error);
            }
            warnings.push(error);
        }
    }
    Ok(())
}

fn expected_dimension_for_arg(
    signature: OpDimensionSignature,
    index: usize,
    arity: usize,
) -> Option<UnitDimension> {
    match signature.slots {
        DimensionSlots::Fixed(slots) => slots.get(index).copied().flatten(),
        DimensionSlots::NumericPrefixExceptTrailing {
            dimension,
            trailing_args,
        } => (index + trailing_args < arity).then_some(dimension),
    }
}

fn op_dimension_signature(op: &CoreOperation) -> Option<OpDimensionSignature> {
    let slots = match op {
        CoreOperation::Primitive(CorePrimitive::Box) => DimensionSlots::Fixed(BOX_DIMS),
        CoreOperation::Primitive(CorePrimitive::Sphere) => DimensionSlots::Fixed(ONE_LENGTH_DIM),
        CoreOperation::Primitive(CorePrimitive::Cylinder) => DimensionSlots::Fixed(CYLINDER_DIMS),
        CoreOperation::Primitive(CorePrimitive::Cone) => DimensionSlots::Fixed(CONE_DIMS),
        CoreOperation::Primitive(CorePrimitive::Torus) => DimensionSlots::Fixed(TORUS_DIMS),
        CoreOperation::Primitive(CorePrimitive::Wedge) => DimensionSlots::Fixed(WEDGE_DIMS),
        CoreOperation::Primitive(CorePrimitive::Ellipse) => DimensionSlots::Fixed(ELLIPSE_DIMS),
        CoreOperation::Primitive(CorePrimitive::Slot) => DimensionSlots::Fixed(SLOT_DIMS),
        CoreOperation::Primitive(CorePrimitive::SlotArc) => DimensionSlots::Fixed(SLOT_ARC_DIMS),
        CoreOperation::Primitive(CorePrimitive::Circle) => DimensionSlots::Fixed(CIRCLE_DIMS),
        CoreOperation::Primitive(CorePrimitive::Rectangle) => {
            DimensionSlots::Fixed(TWO_LENGTH_DIMS)
        }
        CoreOperation::Primitive(CorePrimitive::RoundedRectangle) => {
            DimensionSlots::Fixed(ROUNDED_RECTANGLE_DIMS)
        }
        CoreOperation::Primitive(CorePrimitive::RoundedPolygon) => {
            DimensionSlots::Fixed(ROUNDED_POLYGON_DIMS)
        }
        CoreOperation::Primitive(CorePrimitive::Polygon)
        | CoreOperation::Primitive(CorePrimitive::Profile)
        | CoreOperation::Primitive(CorePrimitive::MakeFace)
        | CoreOperation::Primitive(CorePrimitive::Text)
        | CoreOperation::Primitive(CorePrimitive::Svg)
        | CoreOperation::Primitive(CorePrimitive::Stl)
        | CoreOperation::Boolean(_)
        | CoreOperation::Frame(_)
        | CoreOperation::Meta(_) => DimensionSlots::Fixed(NO_DIMS),
        CoreOperation::Transform(CoreTransformOp::Translate) => {
            DimensionSlots::Fixed(TRANSLATE_DIMS)
        }
        CoreOperation::Transform(CoreTransformOp::Rotate) => DimensionSlots::Fixed(ROTATE_DIMS),
        CoreOperation::Transform(CoreTransformOp::Scale) => {
            DimensionSlots::NumericPrefixExceptTrailing {
                dimension: UnitDimension::Scalar,
                trailing_args: 1,
            }
        }
        CoreOperation::Transform(CoreTransformOp::Mirror) => DimensionSlots::Fixed(MIRROR_DIMS),
        CoreOperation::Surface(CoreSurfaceOp::Extrude) => DimensionSlots::Fixed(EXTRUDE_DIMS),
        CoreOperation::Surface(CoreSurfaceOp::Revolve) => DimensionSlots::Fixed(REVOLVE_DIMS),
        CoreOperation::Surface(CoreSurfaceOp::Loft) => DimensionSlots::Fixed(LOFT_DIMS),
        CoreOperation::Surface(CoreSurfaceOp::Sweep) => DimensionSlots::Fixed(NO_DIMS),
        CoreOperation::Surface(CoreSurfaceOp::Shell)
        | CoreOperation::Surface(CoreSurfaceOp::Fillet)
        | CoreOperation::Surface(CoreSurfaceOp::Chamfer) => DimensionSlots::Fixed(SHELL_DIMS),
        CoreOperation::Surface(CoreSurfaceOp::Offset)
        | CoreOperation::Surface(CoreSurfaceOp::OffsetRounded) => {
            DimensionSlots::Fixed(OFFSET_DIMS)
        }
        CoreOperation::Surface(CoreSurfaceOp::Taper) => DimensionSlots::Fixed(TAPER_DIMS),
        CoreOperation::Surface(CoreSurfaceOp::Twist) => DimensionSlots::Fixed(TWIST_DIMS),
        CoreOperation::Surface(CoreSurfaceOp::Draft) => DimensionSlots::Fixed(DRAFT_DIMS),
        CoreOperation::Path(_) => DimensionSlots::Fixed(NO_DIMS),
        CoreOperation::Array(CoreArrayOp::LinearArray) => DimensionSlots::Fixed(LINEAR_ARRAY_DIMS),
        CoreOperation::Array(CoreArrayOp::RadialArray) => DimensionSlots::Fixed(RADIAL_ARRAY_DIMS),
        CoreOperation::Array(CoreArrayOp::GridArray) => DimensionSlots::Fixed(GRID_ARRAY_DIMS),
        CoreOperation::Array(CoreArrayOp::ArcArray) => DimensionSlots::Fixed(ARC_ARRAY_DIMS),
        CoreOperation::Array(CoreArrayOp::Repeat)
        | CoreOperation::Array(CoreArrayOp::RepeatUnion)
        | CoreOperation::Array(CoreArrayOp::RepeatCompound)
        | CoreOperation::Array(CoreArrayOp::RepeatPick) => DimensionSlots::Fixed(NO_DIMS),
        CoreOperation::Custom(_) => return None,
    };
    Some(OpDimensionSignature { slots })
}

fn unit_dimension_label(dimension: UnitDimension) -> &'static str {
    match dimension {
        UnitDimension::Length => "length",
        UnitDimension::Angle => "angle",
        UnitDimension::Scalar => "scalar",
    }
}

fn kinds_are_known_and_distinct(left: CoreValueKind, right: CoreValueKind) -> bool {
    if matches!(
        (left, right),
        (CoreValueKind::Compound, CoreValueKind::Solid)
            | (CoreValueKind::Solid, CoreValueKind::Compound)
    ) {
        return false;
    }
    !matches!(left, CoreValueKind::Any | CoreValueKind::List)
        && !matches!(right, CoreValueKind::Any | CoreValueKind::List)
        && left != right
}

fn verify_min_arity(
    op_name: &str,
    args: &[CoreNode],
    min: usize,
    expected: &str,
) -> CoreResult<()> {
    if args.len() < min {
        return Err(arity_error(
            op_name,
            expected,
            args.len(),
            args.first().and_then(|arg| arg.span),
        ));
    }
    Ok(())
}

fn verify_max_arity(
    op_name: &str,
    args: &[CoreNode],
    max: usize,
    expected: &str,
) -> CoreResult<()> {
    if args.len() > max {
        return Err(arity_error(
            op_name,
            expected,
            args.len(),
            args.first().and_then(|arg| arg.span),
        ));
    }
    Ok(())
}

fn verify_between(
    op_name: &str,
    args: &[CoreNode],
    min: usize,
    max: usize,
    expected: &str,
) -> CoreResult<()> {
    if args.len() < min || args.len() > max {
        return Err(arity_error(
            op_name,
            expected,
            args.len(),
            args.first().and_then(|arg| arg.span),
        ));
    }
    Ok(())
}

fn arity_error(
    op_name: &str,
    expected: &str,
    actual: usize,
    span: Option<SourceSpan>,
) -> CompilerError {
    type_error(
        op_name,
        &format!("expected {expected}"),
        &format!("got {actual} argument(s)"),
        span,
    )
}

fn type_error(
    op_name: &str,
    expected: &str,
    actual: &str,
    span: Option<SourceSpan>,
) -> CompilerError {
    let err = CompilerError::new(
        CompilerErrorKind::TypeMismatch,
        format!("op `{op_name}` {expected}, {actual}."),
    );
    if let Some(span) = span {
        err.with_span(span)
    } else {
        err
    }
}

fn any(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        expected: ExpectedKind::Any,
    }
}

fn num(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        expected: ExpectedKind::Number,
    }
}

fn point2_list(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        expected: ExpectedKind::Point2List,
    }
}

fn sketch(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        expected: ExpectedKind::Sketch,
    }
}

fn path(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        expected: ExpectedKind::Path,
    }
}

fn frame_arg(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        expected: ExpectedKind::Frame,
    }
}

fn shape(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        expected: ExpectedKind::Shape,
    }
}

fn solid(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        expected: ExpectedKind::Solid,
    }
}

fn expected_label(expected: ExpectedKind) -> &'static str {
    match expected {
        ExpectedKind::Any => "value",
        ExpectedKind::Boolean => "boolean",
        ExpectedKind::Number => "number",
        ExpectedKind::List => "list",
        ExpectedKind::Point2List => "2D point list",
        ExpectedKind::Point3List => "3D point list",
        ExpectedKind::Point3 => "3D point",
        ExpectedKind::Sketch => "2D sketch (sketch)",
        ExpectedKind::Path => "3D path (path)",
        ExpectedKind::Frame => "frame",
        ExpectedKind::Shape => "shape (sketch, path, compound, or solid)",
        ExpectedKind::Solid => "solid",
    }
}

fn kind_label(kind: CoreValueKind) -> &'static str {
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

fn operation_name(op: &CoreOperation) -> String {
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
        CoreOperation::Meta(CoreMetaOp::Comment) => "comment".to_string(),
        CoreOperation::Meta(CoreMetaOp::Annotate) => "annotate".to_string(),
        CoreOperation::Custom(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_core_ir::{
        CompilerErrorKind, CoreBinding, CoreLiteral, CoreNode, CoreNodeKind, CoreOperation,
        CoreParameter, CoreParameterConstraints, CoreParameterKind, CoreParameterValue, CorePart,
        CorePathOp, CorePrimitive, CoreSurfaceOp, CoreTransformOp, CoreValueKind, NodeId, ParamId,
        PartId, ProgramId,
    };

    fn num(id: u64, value: f64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Literal(CoreLiteral::Number(value)),
            CoreValueKind::Number,
        )
    }

    fn bool_lit(id: u64, value: bool) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Literal(CoreLiteral::Boolean(value)),
            CoreValueKind::Boolean,
        )
    }

    fn local_ref(id: u64, name: &str) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Local(name.into())),
            CoreValueKind::Any,
        )
    }

    fn param_ref(id: u64, param_id: u64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Parameter(ParamId::new(param_id))),
            CoreValueKind::Any,
        )
    }

    fn point2(id: u64, x: f64, y: f64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Group(vec![num(id + 1, x), num(id + 2, y)]),
            CoreValueKind::Point2,
        )
    }

    fn point3(id: u64, x: f64, y: f64, z: f64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Group(vec![num(id + 1, x), num(id + 2, y), num(id + 3, z)]),
            CoreValueKind::Point3,
        )
    }

    fn list_node(id: u64, items: Vec<CoreNode>) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::List(items),
            CoreValueKind::List,
        )
    }

    fn range_node(id: u64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Range {
                start: Box::new(num(id + 1, 0.0)),
                end: Box::new(num(id + 2, 3.0)),
            },
            CoreValueKind::List,
        )
    }

    fn call(id: u64, op: CoreOperation, args: Vec<CoreNode>, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Call {
                op,
                args,
                keywords: vec![],
            },
            kind,
        )
    }

    fn part(root: CoreNode) -> CoreProgram {
        CoreProgram::new(
            ProgramId::new(1),
            vec![],
            vec![CorePart {
                id: PartId::new(1),
                key: "body".into(),
                label: "Body".into(),
                root,
            }],
        )
    }

    fn part_with_params(root: CoreNode, parameters: Vec<CoreParameter>) -> CoreProgram {
        CoreProgram::new(
            ProgramId::new(1),
            parameters,
            vec![CorePart {
                id: PartId::new(1),
                key: "body".into(),
                label: "Body".into(),
                root,
            }],
        )
    }

    fn number_param(id: u64, key: &str, unit: &str) -> CoreParameter {
        CoreParameter {
            id: ParamId::new(id),
            key: key.into(),
            label: key.into(),
            kind: CoreParameterKind::Number,
            default_value: CoreParameterValue::Number(1.0),
            frozen: false,
            constraints: CoreParameterConstraints {
                unit: Some(unit.into()),
                ..CoreParameterConstraints::default()
            },
        }
    }

    fn box_node(id: u64) -> CoreNode {
        call(
            id,
            CoreOperation::Primitive(CorePrimitive::Box),
            vec![num(id + 1, 1.0), num(id + 2, 2.0), num(id + 3, 3.0)],
            CoreValueKind::Solid,
        )
    }

    fn circle_node(id: u64) -> CoreNode {
        call(
            id,
            CoreOperation::Primitive(CorePrimitive::Circle),
            vec![num(id + 1, 3.0)],
            CoreValueKind::Sketch,
        )
    }

    fn polygon_node(id: u64, points: CoreNode) -> CoreNode {
        call(
            id,
            CoreOperation::Primitive(CorePrimitive::Polygon),
            vec![points],
            CoreValueKind::Sketch,
        )
    }

    fn path_node(id: u64, points: Vec<CoreNode>) -> CoreNode {
        call(
            id,
            CoreOperation::Path(CorePathOp::Polyline),
            points,
            CoreValueKind::Path,
        )
    }

    fn dynamic_map_node(id: u64, param: &str, source: CoreNode, body: CoreNode) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Map {
                params: vec![param.into()],
                sources: vec![source],
                body: Box::new(body),
            },
            CoreValueKind::List,
        )
    }

    fn apply_node(id: u64, op: CoreOperation, args: Vec<CoreNode>, list: CoreNode) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Apply {
                op,
                args,
                list: Box::new(list),
            },
            CoreValueKind::Solid,
        )
    }

    fn typed_hole_node(id: u64, type_name: &str, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Call {
                op: CoreOperation::Custom("hole".into()),
                args: vec![],
                keywords: vec![
                    CoreKeywordArg::expr(
                        "type".into(),
                        CoreNode::new(
                            NodeId::new(id + 1),
                            CoreNodeKind::Literal(CoreLiteral::Text(type_name.into())),
                            CoreValueKind::Text,
                        ),
                    ),
                    CoreKeywordArg::expr(
                        "goal".into(),
                        CoreNode::new(
                            NodeId::new(id + 2),
                            CoreNodeKind::Literal(CoreLiteral::Text("snap clip".into())),
                            CoreValueKind::Text,
                        ),
                    ),
                ],
            },
            kind,
        )
    }

    fn selector_keyword_program(
        keyword_name: &str,
        selector: Option<CoreSelectorPayload>,
    ) -> CoreProgram {
        part(CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Call {
                op: CoreOperation::Primitive(CorePrimitive::Box),
                args: vec![num(20, 1.0), num(21, 1.0), num(22, 1.0)],
                keywords: vec![match selector {
                    Some(payload) => CoreKeywordArg::selector(
                        keyword_name.into(),
                        CoreNode::new(
                            NodeId::new(23),
                            CoreNodeKind::Literal(CoreLiteral::Text("left+vertical".into())),
                            CoreValueKind::Text,
                        ),
                        payload,
                    ),
                    None => CoreKeywordArg::expr(
                        keyword_name.into(),
                        CoreNode::new(
                            NodeId::new(23),
                            CoreNodeKind::Literal(CoreLiteral::Text("left+vertical".into())),
                            CoreValueKind::Text,
                        ),
                    ),
                }],
            },
            CoreValueKind::Solid,
        ))
    }

    fn verify_err(program: CoreProgram) -> String {
        let err = verify_core_program(&program).expect_err("expected verifier failure");
        assert_eq!(err.kind, CompilerErrorKind::TypeMismatch);
        err.message
    }

    fn strict_units_err(program: CoreProgram) -> String {
        let err = verify_core_program_with_unit_mode(&program, UnitCheckMode::Strict)
            .expect_err("expected verifier failure");
        assert_eq!(err.kind, CompilerErrorKind::TypeMismatch);
        err.message
    }

    fn permissive_unit_warnings(
        program: &CoreProgram,
        literal_dimensions: &std::collections::HashMap<SourceSpan, String>,
    ) -> Vec<CompilerError> {
        verify_core_program_with_literal_dimensions(program, literal_dimensions, false)
            .expect("permissive verifier should not fail")
    }

    fn all_core_operations() -> Vec<CoreOperation> {
        vec![
            CoreOperation::Primitive(CorePrimitive::Box),
            CoreOperation::Primitive(CorePrimitive::Sphere),
            CoreOperation::Primitive(CorePrimitive::Cylinder),
            CoreOperation::Primitive(CorePrimitive::Cone),
            CoreOperation::Primitive(CorePrimitive::Torus),
            CoreOperation::Primitive(CorePrimitive::Wedge),
            CoreOperation::Primitive(CorePrimitive::Ellipse),
            CoreOperation::Primitive(CorePrimitive::Slot),
            CoreOperation::Primitive(CorePrimitive::SlotArc),
            CoreOperation::Primitive(CorePrimitive::Circle),
            CoreOperation::Primitive(CorePrimitive::Rectangle),
            CoreOperation::Primitive(CorePrimitive::RoundedRectangle),
            CoreOperation::Primitive(CorePrimitive::RoundedPolygon),
            CoreOperation::Primitive(CorePrimitive::Polygon),
            CoreOperation::Primitive(CorePrimitive::Profile),
            CoreOperation::Primitive(CorePrimitive::MakeFace),
            CoreOperation::Primitive(CorePrimitive::Text),
            CoreOperation::Primitive(CorePrimitive::Svg),
            CoreOperation::Primitive(CorePrimitive::Stl),
            CoreOperation::Boolean(CoreBooleanOp::Union),
            CoreOperation::Boolean(CoreBooleanOp::Difference),
            CoreOperation::Boolean(CoreBooleanOp::Intersection),
            CoreOperation::Boolean(CoreBooleanOp::Xor),
            CoreOperation::Transform(CoreTransformOp::Translate),
            CoreOperation::Transform(CoreTransformOp::Rotate),
            CoreOperation::Transform(CoreTransformOp::Scale),
            CoreOperation::Transform(CoreTransformOp::Mirror),
            CoreOperation::Surface(CoreSurfaceOp::Extrude),
            CoreOperation::Surface(CoreSurfaceOp::Revolve),
            CoreOperation::Surface(CoreSurfaceOp::Loft),
            CoreOperation::Surface(CoreSurfaceOp::Sweep),
            CoreOperation::Surface(CoreSurfaceOp::Shell),
            CoreOperation::Surface(CoreSurfaceOp::Offset),
            CoreOperation::Surface(CoreSurfaceOp::OffsetRounded),
            CoreOperation::Surface(CoreSurfaceOp::Fillet),
            CoreOperation::Surface(CoreSurfaceOp::Chamfer),
            CoreOperation::Surface(CoreSurfaceOp::Taper),
            CoreOperation::Surface(CoreSurfaceOp::Twist),
            CoreOperation::Surface(CoreSurfaceOp::Draft),
            CoreOperation::Path(CorePathOp::Polyline),
            CoreOperation::Path(CorePathOp::BezierPath),
            CoreOperation::Path(CorePathOp::Bspline),
            CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::LinearArray),
            CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::RadialArray),
            CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::GridArray),
            CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::ArcArray),
            CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::Repeat),
            CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::RepeatUnion),
            CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::RepeatCompound),
            CoreOperation::Array(crate::ecky_core_ir::CoreArrayOp::RepeatPick),
            CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::Plane),
            CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::Location),
            CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::PathFrame),
            CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::Place),
            CoreOperation::Frame(crate::ecky_core_ir::CoreFrameOp::ClipBox),
            CoreOperation::Meta(crate::ecky_core_ir::CoreMetaOp::Group),
            CoreOperation::Meta(crate::ecky_core_ir::CoreMetaOp::Comment),
            CoreOperation::Meta(crate::ecky_core_ir::CoreMetaOp::Annotate),
        ]
    }

    #[test]
    fn valid_box_extrude_passes() {
        let program = part(call(
            10,
            CoreOperation::Surface(CoreSurfaceOp::Extrude),
            vec![circle_node(20), num(30, 5.0)],
            CoreValueKind::Solid,
        ));

        verify_core_program(&program).expect("valid program");
    }

    #[test]
    fn extrude_rejects_solid_profile() {
        let message = verify_err(part(call(
            10,
            CoreOperation::Surface(CoreSurfaceOp::Extrude),
            vec![box_node(20), num(30, 5.0)],
            CoreValueKind::Solid,
        )));

        assert!(message.contains("extrude"), "{message}");
        assert!(message.contains("arg 0"), "{message}");
        assert!(message.contains("sketch"), "{message}");
        assert!(message.contains("solid"), "{message}");
    }

    #[test]
    fn offset_accepts_openings_sketch_list() {
        let program = part(CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Call {
                op: CoreOperation::Surface(CoreSurfaceOp::Offset),
                args: vec![num(20, 2.0), circle_node(30)],
                keywords: vec![CoreKeywordArg::expr(
                    "openings".into(),
                    list_node(40, vec![circle_node(50)]),
                )],
            },
            CoreValueKind::Sketch,
        ));

        verify_core_program(&program).expect("offset openings should accept sketch lists");
    }

    #[test]
    fn difference_accepts_typed_solid_hole() {
        let program = part(call(
            10,
            CoreOperation::Boolean(CoreBooleanOp::Difference),
            vec![
                typed_hole_node(20, "solid", CoreValueKind::Solid),
                box_node(30),
            ],
            CoreValueKind::Solid,
        ));

        verify_core_program(&program).expect("solid hole should typecheck as shape");
    }

    #[test]
    fn verify_core_program_rejects_missing_edge_selector_payload() {
        let message = verify_err(selector_keyword_program("edges", None));
        assert!(
            message.contains("`:edges` expected selector payload"),
            "{message}"
        );
    }

    #[test]
    fn verify_core_program_rejects_missing_face_selector_payload() {
        let message = verify_err(selector_keyword_program("faces", None));
        assert!(
            message.contains("`:faces` expected selector payload"),
            "{message}"
        );
    }

    #[test]
    fn verify_core_program_rejects_wrong_kind_selector_payload() {
        let message = verify_err(selector_keyword_program(
            "edges",
            Some(CoreSelectorPayload::FaceTargetIds(vec![
                "body:face:0:0-0-1:1".into(),
            ])),
        ));
        assert!(
            message.contains("`:edges` expected edge selector payload"),
            "{message}"
        );
    }

    #[test]
    fn extrude_rejects_typed_solid_hole_profile() {
        let message = verify_err(part(call(
            10,
            CoreOperation::Surface(CoreSurfaceOp::Extrude),
            vec![
                typed_hole_node(20, "solid", CoreValueKind::Solid),
                num(30, 5.0),
            ],
            CoreValueKind::Solid,
        )));

        assert!(message.contains("extrude"), "{message}");
        assert!(message.contains("sketch"), "{message}");
        assert!(message.contains("solid"), "{message}");
    }

    #[test]
    fn translate_rejects_number_shape() {
        let message = verify_err(part(call(
            10,
            CoreOperation::Transform(CoreTransformOp::Translate),
            vec![num(11, 1.0), num(12, 0.0), num(13, 0.0), num(14, 7.0)],
            CoreValueKind::Solid,
        )));

        assert!(message.contains("translate"), "{message}");
        assert!(message.contains("arg 3"), "{message}");
        assert!(message.contains("shape"), "{message}");
        assert!(message.contains("number"), "{message}");
    }

    #[test]
    fn map_binds_range_items_as_numbers_inside_body() {
        let program = part(dynamic_map_node(
            10,
            "r",
            range_node(20),
            call(
                30,
                CoreOperation::Primitive(CorePrimitive::Circle),
                vec![local_ref(31, "r")],
                CoreValueKind::Sketch,
            ),
        ));

        verify_core_program(&program).expect("range map item should typecheck as number");
    }

    #[test]
    fn polygon_accepts_dynamic_map_of_numeric_tuples() {
        let points = dynamic_map_node(
            10,
            "i",
            range_node(20),
            CoreNode::new(
                NodeId::new(30),
                CoreNodeKind::List(vec![local_ref(31, "i"), local_ref(32, "i")]),
                CoreValueKind::List,
            ),
        );
        let program = part(polygon_node(40, points));

        verify_core_program(&program).expect("dynamic map numeric tuples should typecheck");
    }

    #[test]
    fn polygon_rejects_numeric_list() {
        let message = verify_err(part(polygon_node(
            10,
            list_node(20, vec![num(21, 1.0), num(22, 2.0)]),
        )));

        assert!(message.contains("polygon"), "{message}");
        assert!(message.contains("2D point list"), "{message}");
        assert!(message.contains("number list"), "{message}");
    }

    #[test]
    fn polygon_rejects_dynamic_map_of_solids() {
        let points = dynamic_map_node(10, "i", range_node(20), box_node(30));
        let message = verify_err(part(polygon_node(40, points)));

        assert!(message.contains("polygon"), "{message}");
        assert!(message.contains("2D point list"), "{message}");
        assert!(message.contains("solid list"), "{message}");
    }

    #[test]
    fn map_rejects_point_item_used_as_number() {
        let point_source = list_node(20, vec![point2(21, 0.0, 0.0), point2(24, 2.0, 0.0)]);
        let message = verify_err(part(CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Map {
                params: vec!["p".into()],
                sources: vec![point_source],
                body: Box::new(call(
                    30,
                    CoreOperation::Primitive(CorePrimitive::Circle),
                    vec![local_ref(31, "p")],
                    CoreValueKind::Sketch,
                )),
            },
            CoreValueKind::List,
        )));

        assert!(message.contains("circle"), "{message}");
        assert!(message.contains("radius"), "{message}");
        assert!(message.contains("point2"), "{message}");
    }

    #[test]
    fn map_binds_point3_items_for_path_body() {
        let point_source = list_node(
            20,
            vec![point3(21, 0.0, 0.0, 0.0), point3(25, 2.0, 0.0, 0.0)],
        );
        let program = part(CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Map {
                params: vec!["p".into()],
                sources: vec![point_source],
                body: Box::new(path_node(
                    30,
                    vec![local_ref(31, "p"), point3(40, 1.0, 0.0, 0.0)],
                )),
            },
            CoreValueKind::List,
        ));

        verify_core_program(&program).expect("map point3 item should typecheck as path point");
    }

    #[test]
    fn apply_union_rejects_numeric_list_items() {
        let message = verify_err(part(apply_node(
            10,
            CoreOperation::Boolean(CoreBooleanOp::Union),
            vec![],
            range_node(20),
        )));

        assert!(message.contains("union"), "{message}");
        assert!(message.contains("shape"), "{message}");
        assert!(message.contains("number list"), "{message}");
    }

    #[test]
    fn apply_union_accepts_dynamic_shape_map_items() {
        let solids = dynamic_map_node(10, "i", range_node(20), box_node(30));
        let program = part(apply_node(
            40,
            CoreOperation::Boolean(CoreBooleanOp::Union),
            vec![box_node(50)],
            solids,
        ));

        verify_core_program(&program).expect("apply union should accept solid list items");
    }

    #[test]
    fn path_rejects_raw_two_component_list_as_3d_point() {
        let message = verify_err(part(path_node(
            10,
            vec![
                list_node(20, vec![num(21, 0.0), num(22, 0.0)]),
                point3(30, 1.0, 0.0, 0.0),
            ],
        )));

        assert!(message.contains("path"), "{message}");
        assert!(message.contains("3D point"), "{message}");
        assert!(message.contains("2 component"), "{message}");
    }

    #[test]
    fn map_rejects_parameter_source_count_mismatch() {
        let message = verify_err(part(CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Map {
                params: vec!["x".into(), "y".into()],
                sources: vec![range_node(20)],
                body: Box::new(circle_node(30)),
            },
            CoreValueKind::List,
        )));

        assert!(message.contains("map"), "{message}");
        assert!(message.contains("one parameter per source"), "{message}");
    }

    #[test]
    fn let_bound_point_list_preserves_item_kind_for_polygon() {
        let points = list_node(
            20,
            vec![point3(21, 0.0, 0.0, 0.0), point3(25, 2.0, 0.0, 0.0)],
        );
        let message = verify_err(part(CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Let {
                bindings: vec![CoreBinding {
                    name: "points".into(),
                    value: points,
                }],
                body: Box::new(polygon_node(30, local_ref(31, "points"))),
            },
            CoreValueKind::Sketch,
        )));

        assert!(message.contains("polygon"), "{message}");
        assert!(message.contains("2D point list"), "{message}");
        assert!(message.contains("point3"), "{message}");
    }

    #[test]
    fn if_branch_mismatch_fails_clearly() {
        let program = part(CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::If {
                condition: Box::new(bool_lit(11, true)),
                then_branch: Box::new(circle_node(20)),
                else_branch: Box::new(box_node(30)),
            },
            CoreValueKind::Any,
        ));

        let message = verify_err(program);
        assert!(message.contains("if"), "{message}");
        assert!(message.contains("matching branch kinds"), "{message}");
        assert!(message.contains("sketch"), "{message}");
        assert!(message.contains("solid"), "{message}");
    }

    #[test]
    fn if_allows_solid_and_compound_branch_shapes() {
        let program = part(CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::If {
                condition: Box::new(bool_lit(11, true)),
                then_branch: Box::new(call(
                    20,
                    CoreOperation::Meta(crate::ecky_core_ir::CoreMetaOp::Group),
                    vec![box_node(30)],
                    CoreValueKind::Compound,
                )),
                else_branch: Box::new(box_node(40)),
            },
            CoreValueKind::Any,
        ));

        verify_core_program(&program).expect("compound/solid branches are compatible shapes");
    }

    #[test]
    fn op_dimension_table_covers_every_builtin_operation() {
        for op in all_core_operations() {
            assert!(
                op_dimension_signature(&op).is_some(),
                "missing dimension signature for {}",
                operation_name(&op)
            );
        }
    }

    #[test]
    fn strict_units_reject_length_parameter_where_angle_expected() {
        let width = number_param(1, "width", "length");
        let root = call(
            10,
            CoreOperation::Transform(CoreTransformOp::Rotate),
            vec![num(11, 0.0), num(12, 0.0), param_ref(13, 1), box_node(20)],
            CoreValueKind::Solid,
        );
        let message = strict_units_err(part_with_params(root, vec![width]));

        assert!(message.contains("rotate"), "{message}");
        assert!(message.contains("arg 2"), "{message}");
        assert!(message.contains("angle"), "{message}");
        assert!(message.contains("length"), "{message}");
    }

    #[test]
    fn permissive_units_allow_parameter_dimension_mismatch() {
        let width = number_param(1, "width", "length");
        let root = call(
            10,
            CoreOperation::Transform(CoreTransformOp::Rotate),
            vec![num(11, 0.0), num(12, 0.0), param_ref(13, 1), box_node(20)],
            CoreValueKind::Solid,
        );
        let program = part_with_params(root, vec![width]);

        verify_core_program(&program).expect("default unit mode remains permissive");
    }

    #[test]
    fn permissive_units_collect_warning_for_literal_dimension_mismatch() {
        let span = SourceSpan::new(None, 12, 16);
        let root = CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Call {
                op: CoreOperation::Transform(CoreTransformOp::Rotate),
                args: vec![
                    num(11, 0.0),
                    num(12, 0.0),
                    CoreNode::new(
                        NodeId::new(13),
                        CoreNodeKind::Literal(CoreLiteral::Number(12.0)),
                        CoreValueKind::Number,
                    )
                    .with_span(span),
                    box_node(20),
                ],
                keywords: vec![],
            },
            CoreValueKind::Solid,
        );
        let warnings = permissive_unit_warnings(
            &part(root),
            &std::collections::HashMap::from([(span, "length".to_string())]),
        );

        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("rotate"),
            "{}",
            warnings[0].message
        );
        assert_eq!(warnings[0].primary_span, Some(span));
    }

    #[test]
    fn strict_units_reject_literal_dimension_mismatch_from_span_table() {
        let span = SourceSpan::new(None, 12, 16);
        let root = CoreNode::new(
            NodeId::new(10),
            CoreNodeKind::Call {
                op: CoreOperation::Transform(CoreTransformOp::Rotate),
                args: vec![
                    num(11, 0.0),
                    num(12, 0.0),
                    CoreNode::new(
                        NodeId::new(13),
                        CoreNodeKind::Literal(CoreLiteral::Number(12.0)),
                        CoreValueKind::Number,
                    )
                    .with_span(SourceSpan::new(
                        Some(crate::ecky_core_ir::SourceFileId::new(1)),
                        12,
                        16,
                    )),
                    box_node(20),
                ],
                keywords: vec![],
            },
            CoreValueKind::Solid,
        );
        let err = verify_core_program_with_literal_dimensions(
            &part(root),
            &std::collections::HashMap::from([(span, "length".to_string())]),
            true,
        )
        .expect_err("strict literal dimension mismatch should fail");

        assert_eq!(err.kind, CompilerErrorKind::TypeMismatch);
        assert!(err.message.contains("rotate"), "{}", err.message);
        assert_eq!(
            err.primary_span,
            Some(SourceSpan::new(
                Some(crate::ecky_core_ir::SourceFileId::new(1)),
                12,
                16,
            ))
        );
    }
}
