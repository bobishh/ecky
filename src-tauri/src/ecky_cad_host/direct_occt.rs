use std::collections::BTreeMap;

use crate::ecky_core_ir::{
    CoreBinding, CoreBooleanOp, CoreFrameOp, CoreKeywordArg, CoreLiteral, CoreMetaOp, CoreNode,
    CoreNodeKind, CoreOperation, CoreParameterKind, CorePathOp, CorePrimitive, CoreProgram,
    CoreReference, CoreShapeBinding, CoreSurfaceOp, CoreSymbol, CoreTransformOp,
};
use crate::models::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcctParameterKind {
    Number,
    Boolean,
    Text,
    Choice,
    Image,
}

impl From<CoreParameterKind> for OcctParameterKind {
    fn from(kind: CoreParameterKind) -> Self {
        match kind {
            CoreParameterKind::Number => Self::Number,
            CoreParameterKind::Boolean => Self::Boolean,
            CoreParameterKind::Text => Self::Text,
            CoreParameterKind::Choice => Self::Choice,
            CoreParameterKind::Image => Self::Image,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcctParameter {
    pub key: String,
    pub kind: OcctParameterKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OcctSlot(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub enum OcctArg {
    Number(f64),
    Boolean(bool),
    Text(String),
    Symbol(String),
    Point2([f64; 2]),
    Point3([f64; 3]),
    List(Vec<OcctArg>),
    Param(String),
    Ref(OcctSlot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcctOp {
    Box,
    Sphere,
    Cylinder,
    Circle,
    Rectangle,
    Polygon,
    Extrude,
    Revolve,
    Union,
    Difference,
    Intersection,
    Fillet,
    Chamfer,
    Shell,
    Translate,
    Rotate,
    Scale,
    Compound,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcctKeyword {
    pub name: String,
    pub value: OcctArg,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcctCommand {
    pub output: OcctSlot,
    pub op: OcctOp,
    pub args: Vec<OcctArg>,
    pub keywords: Vec<OcctKeyword>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcctPartPlan {
    pub key: String,
    pub label: String,
    pub root: OcctSlot,
    pub commands: Vec<OcctCommand>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcctPlan {
    pub parameters: Vec<OcctParameter>,
    pub parts: Vec<OcctPartPlan>,
}

pub fn plan_core_program(program: &CoreProgram) -> AppResult<OcctPlan> {
    crate::ecky_core_ir::verify_core_program(program).map_err(|err| {
        AppError::validation(format!(
            "Direct OCCT adapter rejected invalid Core IR before planning: {}",
            err
        ))
    })?;

    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let parameters = program
        .parameters
        .iter()
        .map(|param| OcctParameter {
            key: param.key.clone(),
            kind: param.kind.into(),
        })
        .collect::<Vec<_>>();

    let parts = program
        .parts
        .iter()
        .map(|part| {
            let mut planner = PartPlanner::new(&param_names);
            let root = planner.plan_node(&part.root)?;
            Ok(OcctPartPlan {
                key: part.key.clone(),
                label: part.label.clone(),
                root,
                commands: planner.commands,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(OcctPlan { parameters, parts })
}

struct PartPlanner<'a> {
    param_names: &'a BTreeMap<u64, String>,
    node_refs: BTreeMap<u64, OcctSlot>,
    locals: BTreeMap<String, OcctArg>,
    commands: Vec<OcctCommand>,
}

impl<'a> PartPlanner<'a> {
    fn new(param_names: &'a BTreeMap<u64, String>) -> Self {
        Self {
            param_names,
            node_refs: BTreeMap::new(),
            locals: BTreeMap::new(),
            commands: Vec::new(),
        }
    }

    fn plan_node(&mut self, node: &CoreNode) -> AppResult<OcctSlot> {
        if let Some(slot) = self.node_refs.get(&node.id.raw()).copied() {
            return Ok(slot);
        }

        let slot = match &node.kind {
            CoreNodeKind::Call { op, args, keywords } => {
                if matches!(op, CoreOperation::Custom(name) if name == "hole") {
                    return Err(typed_hole_error(keywords));
                }
                let op = occt_op(op)?;
                let output = OcctSlot(node.id.raw());
                let args = args
                    .iter()
                    .map(|arg| self.plan_arg(arg))
                    .collect::<AppResult<Vec<_>>>()?;
                let keywords = keywords
                    .iter()
                    .map(|keyword| {
                        Ok(OcctKeyword {
                            name: keyword.name.clone(),
                            value: self.plan_arg(&keyword.value)?,
                        })
                    })
                    .collect::<AppResult<Vec<_>>>()?;
                self.commands.push(OcctCommand {
                    output,
                    op,
                    args,
                    keywords,
                });
                output
            }
            CoreNodeKind::Build { bindings, result } => self.plan_build(bindings, result)?,
            CoreNodeKind::Let { bindings, body } => self.plan_let(bindings, body)?,
            CoreNodeKind::If { .. } => {
                return Err(unsupported(
                    "if",
                    "branching Core IR needs runtime selection before direct OCCT planning",
                ));
            }
            CoreNodeKind::Reference(_) => match self.plan_arg(node)? {
                OcctArg::Ref(slot) => slot,
                other => {
                    return Err(AppError::validation(format!(
                        "Direct OCCT adapter expected geometry reference, got {:?}.",
                        other
                    )));
                }
            },
            _ => {
                return Err(AppError::validation(format!(
                    "Direct OCCT adapter expected geometry node, got {:?}.",
                    node.kind
                )));
            }
        };

        self.node_refs.insert(node.id.raw(), slot);
        Ok(slot)
    }

    fn plan_build(
        &mut self,
        bindings: &[CoreShapeBinding],
        result: &CoreNode,
    ) -> AppResult<OcctSlot> {
        let saved_locals = self.locals.clone();
        for binding in bindings {
            let slot = self.plan_node(&binding.value)?;
            self.node_refs.insert(binding.value.id.raw(), slot);
            self.locals.insert(binding.name.clone(), OcctArg::Ref(slot));
        }
        let root = self.plan_node(result);
        self.locals = saved_locals;
        root
    }

    fn plan_let(&mut self, bindings: &[CoreBinding], body: &CoreNode) -> AppResult<OcctSlot> {
        let saved_locals = self.locals.clone();
        for binding in bindings {
            let value = self.plan_arg(&binding.value)?;
            self.locals.insert(binding.name.clone(), value);
        }
        let root = self.plan_node(body);
        self.locals = saved_locals;
        root
    }

    fn plan_arg(&mut self, node: &CoreNode) -> AppResult<OcctArg> {
        match &node.kind {
            CoreNodeKind::Literal(CoreLiteral::Number(number)) => Ok(OcctArg::Number(*number)),
            CoreNodeKind::Literal(CoreLiteral::Boolean(flag)) => Ok(OcctArg::Boolean(*flag)),
            CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(OcctArg::Text(text.clone())),
            CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => {
                Ok(OcctArg::Symbol(symbol_name(symbol).to_string()))
            }
            CoreNodeKind::Literal(CoreLiteral::Point2(point)) => Ok(OcctArg::Point2(*point)),
            CoreNodeKind::Literal(CoreLiteral::Point3(point)) => Ok(OcctArg::Point3(*point)),
            CoreNodeKind::Reference(CoreReference::Parameter(id)) => {
                let name = self.param_names.get(&id.raw()).cloned().ok_or_else(|| {
                    AppError::validation(format!(
                        "Direct OCCT adapter could not resolve parameter {:?}.",
                        id
                    ))
                })?;
                Ok(OcctArg::Param(name))
            }
            CoreNodeKind::Reference(CoreReference::Node(id)) => {
                let slot = self.node_refs.get(&id.raw()).copied().ok_or_else(|| {
                    AppError::validation(format!(
                        "Direct OCCT adapter could not resolve Core node reference {:?}.",
                        id
                    ))
                })?;
                Ok(OcctArg::Ref(slot))
            }
            CoreNodeKind::Reference(CoreReference::Local(name)) => {
                self.locals.get(name).cloned().ok_or_else(|| {
                    AppError::validation(format!(
                        "Direct OCCT adapter could not resolve local `{}`.",
                        name
                    ))
                })
            }
            CoreNodeKind::List(items) | CoreNodeKind::Group(items) => Ok(OcctArg::List(
                items
                    .iter()
                    .map(|item| self.plan_arg(item))
                    .collect::<AppResult<Vec<_>>>()?,
            )),
            CoreNodeKind::Call { .. } | CoreNodeKind::Build { .. } | CoreNodeKind::Let { .. } => {
                let slot = self.plan_node(node)?;
                Ok(OcctArg::Ref(slot))
            }
            CoreNodeKind::If { .. }
            | CoreNodeKind::Range { .. }
            | CoreNodeKind::Map { .. }
            | CoreNodeKind::Apply { .. } => Err(AppError::validation(format!(
                "Direct OCCT adapter cannot plan dynamic expression node {:?} before evaluation.",
                node.kind
            ))),
            CoreNodeKind::Reference(CoreReference::Part(id)) => Err(AppError::validation(format!(
                "Direct OCCT adapter cannot plan part reference {:?} in first surface.",
                id
            ))),
        }
    }
}

fn occt_op(op: &CoreOperation) -> AppResult<OcctOp> {
    match op {
        CoreOperation::Primitive(CorePrimitive::Box) => Ok(OcctOp::Box),
        CoreOperation::Primitive(CorePrimitive::Sphere) => Ok(OcctOp::Sphere),
        CoreOperation::Primitive(CorePrimitive::Cylinder) => Ok(OcctOp::Cylinder),
        CoreOperation::Primitive(CorePrimitive::Circle) => Ok(OcctOp::Circle),
        CoreOperation::Primitive(CorePrimitive::Rectangle) => Ok(OcctOp::Rectangle),
        CoreOperation::Primitive(CorePrimitive::Polygon) => Ok(OcctOp::Polygon),
        CoreOperation::Surface(CoreSurfaceOp::Extrude) => Ok(OcctOp::Extrude),
        CoreOperation::Surface(CoreSurfaceOp::Revolve) => Ok(OcctOp::Revolve),
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => Ok(OcctOp::Fillet),
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => Ok(OcctOp::Chamfer),
        CoreOperation::Surface(CoreSurfaceOp::Shell) => Ok(OcctOp::Shell),
        CoreOperation::Boolean(CoreBooleanOp::Union) => Ok(OcctOp::Union),
        CoreOperation::Boolean(CoreBooleanOp::Difference) => Ok(OcctOp::Difference),
        CoreOperation::Boolean(CoreBooleanOp::Intersection) => Ok(OcctOp::Intersection),
        CoreOperation::Transform(CoreTransformOp::Translate) => Ok(OcctOp::Translate),
        CoreOperation::Transform(CoreTransformOp::Rotate) => Ok(OcctOp::Rotate),
        CoreOperation::Transform(CoreTransformOp::Scale) => Ok(OcctOp::Scale),
        CoreOperation::Meta(CoreMetaOp::Group) => Ok(OcctOp::Compound),
        CoreOperation::Custom(name) if name == "hole" => Err(AppError::validation(
            "Typed hole must be filled before direct OCCT planning.",
        )),
        _ => Err(unsupported(&operation_name(op), "not in first surface")),
    }
}

fn typed_hole_error(keywords: &[CoreKeywordArg]) -> AppError {
    let requested_type = keyword_text(keywords, "type").unwrap_or_else(|| "unknown".to_string());
    let goal = keyword_text(keywords, "goal").unwrap_or_else(|| "unspecified".to_string());
    AppError::validation(format!(
        "Typed hole requested type `{}` with goal `{}` must be filled before direct OCCT planning.",
        requested_type, goal
    ))
}

fn keyword_text(keywords: &[CoreKeywordArg], name: &str) -> Option<String> {
    keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .and_then(|keyword| match &keyword.value.kind {
            CoreNodeKind::Literal(CoreLiteral::Text(text)) => Some(text.clone()),
            _ => None,
        })
}

fn unsupported(op: &str, reason: &str) -> AppError {
    AppError::validation(format!(
        "Direct OCCT adapter first surface does not support `{}`: {}.",
        op, reason
    ))
}

fn symbol_name(symbol: &CoreSymbol) -> &'static str {
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

fn operation_name(op: &CoreOperation) -> String {
    match op {
        CoreOperation::Primitive(CorePrimitive::Box) => "box",
        CoreOperation::Primitive(CorePrimitive::Sphere) => "sphere",
        CoreOperation::Primitive(CorePrimitive::Cylinder) => "cylinder",
        CoreOperation::Primitive(CorePrimitive::Cone) => "cone",
        CoreOperation::Primitive(CorePrimitive::Circle) => "circle",
        CoreOperation::Primitive(CorePrimitive::Rectangle) => "rectangle",
        CoreOperation::Primitive(CorePrimitive::RoundedRectangle) => "rounded-rect",
        CoreOperation::Primitive(CorePrimitive::RoundedPolygon) => "rounded-polygon",
        CoreOperation::Primitive(CorePrimitive::Polygon) => "polygon",
        CoreOperation::Primitive(CorePrimitive::Profile) => "profile",
        CoreOperation::Primitive(CorePrimitive::MakeFace) => "make-face",
        CoreOperation::Primitive(CorePrimitive::Text) => "text",
        CoreOperation::Primitive(CorePrimitive::Svg) => "svg",
        CoreOperation::Primitive(CorePrimitive::Stl) => "import-stl",
        CoreOperation::Boolean(CoreBooleanOp::Union) => "union",
        CoreOperation::Boolean(CoreBooleanOp::Difference) => "difference",
        CoreOperation::Boolean(CoreBooleanOp::Intersection) => "intersection",
        CoreOperation::Boolean(CoreBooleanOp::Xor) => "xor",
        CoreOperation::Transform(CoreTransformOp::Translate) => "translate",
        CoreOperation::Transform(CoreTransformOp::Rotate) => "rotate",
        CoreOperation::Transform(CoreTransformOp::Scale) => "scale",
        CoreOperation::Transform(CoreTransformOp::Mirror) => "mirror",
        CoreOperation::Surface(CoreSurfaceOp::Extrude) => "extrude",
        CoreOperation::Surface(CoreSurfaceOp::Revolve) => "revolve",
        CoreOperation::Surface(CoreSurfaceOp::Loft) => "loft",
        CoreOperation::Surface(CoreSurfaceOp::Sweep) => "sweep",
        CoreOperation::Surface(CoreSurfaceOp::Shell) => "shell",
        CoreOperation::Surface(CoreSurfaceOp::Offset) => "offset",
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => "fillet",
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => "chamfer",
        CoreOperation::Surface(CoreSurfaceOp::Twist) => "twist",
        CoreOperation::Path(CorePathOp::Polyline) => "path",
        CoreOperation::Path(CorePathOp::BezierPath) => "bezier-path",
        CoreOperation::Path(CorePathOp::Bspline) => "bspline",
        CoreOperation::Array(_) => "array",
        CoreOperation::Frame(CoreFrameOp::Plane) => "plane",
        CoreOperation::Frame(CoreFrameOp::Location) => "location",
        CoreOperation::Frame(CoreFrameOp::PathFrame) => "path-frame",
        CoreOperation::Frame(CoreFrameOp::Place) => "place",
        CoreOperation::Frame(CoreFrameOp::ClipBox) => "clip-box",
        CoreOperation::Meta(CoreMetaOp::Group) => "compound",
        CoreOperation::Meta(CoreMetaOp::Comment) => "comment",
        CoreOperation::Meta(CoreMetaOp::Annotate) => "annotate",
        CoreOperation::Custom(name) => return name.clone(),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_core_ir::{
        CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CorePart, CorePrimitive, CoreProgram,
        CoreSurfaceOp, CoreValueKind, NodeId, PartId, ProgramId,
    };

    fn compile(source: &str) -> CoreProgram {
        crate::ecky_scheme::compile_to_core_program(source).expect("compile")
    }

    #[test]
    fn plans_typed_core_program_into_direct_occt_commands() {
        let program = compile(
            r#"
            (model
              (params
                (number radius 12)
                (number height 30))
              (part body
                (fillet 1
                  (difference
                    (extrude (circle radius) height)
                    (box 5 5 10)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parameters,
            vec![
                OcctParameter {
                    key: "radius".into(),
                    kind: OcctParameterKind::Number,
                },
                OcctParameter {
                    key: "height".into(),
                    kind: OcctParameterKind::Number,
                },
            ]
        );
        assert_eq!(plan.parts.len(), 1);
        assert_eq!(plan.parts[0].key, "body");
        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![
                OcctOp::Circle,
                OcctOp::Extrude,
                OcctOp::Box,
                OcctOp::Difference,
                OcctOp::Fillet,
            ]
        );
        assert!(plan.parts[0]
            .commands
            .iter()
            .any(|command| command.args.contains(&OcctArg::Param("radius".into()))));
    }

    #[test]
    fn plans_build_shape_references_without_raw_source() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape profile (circle 5))
                  (shape solid (extrude profile 10))
                  (result (shell 1 solid)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Circle, OcctOp::Extrude, OcctOp::Shell]
        );
        assert_eq!(plan.parts[0].root, plan.parts[0].commands[2].output);
    }

    #[test]
    fn rejects_unsupported_first_surface_ops_by_name() {
        let program = compile(
            r#"
            (model
              (part body
                (loft 10 (circle 1) (circle 2))))
            "#,
        );

        let err = plan_core_program(&program).expect_err("loft unsupported");
        let message = err.to_string();

        assert!(message.contains("Direct OCCT adapter"), "{message}");
        assert!(message.contains("loft"), "{message}");
        assert!(message.contains("first surface"), "{message}");
    }

    #[test]
    fn rejects_typed_holes_before_runtime_adapter() {
        let program = compile(
            r#"
            (model
              (part body
                (difference
                  (box 1 1 1)
                  (hole :type solid :goal "threaded insert cavity"))))
            "#,
        );

        let err = plan_core_program(&program).expect_err("hole unsupported");
        let message = err.to_string();

        assert!(message.contains("Typed hole"), "{message}");
        assert!(message.contains("threaded insert cavity"), "{message}");
        assert!(message.contains("before direct OCCT planning"), "{message}");
    }

    #[test]
    fn verifies_core_program_before_planning() {
        let box_node = CoreNode::new(
            NodeId::new(2),
            CoreNodeKind::Call {
                op: CoreOperation::Primitive(CorePrimitive::Box),
                args: vec![
                    CoreNode::new(
                        NodeId::new(3),
                        CoreNodeKind::Literal(CoreLiteral::Number(1.0)),
                        CoreValueKind::Number,
                    ),
                    CoreNode::new(
                        NodeId::new(4),
                        CoreNodeKind::Literal(CoreLiteral::Number(1.0)),
                        CoreValueKind::Number,
                    ),
                    CoreNode::new(
                        NodeId::new(5),
                        CoreNodeKind::Literal(CoreLiteral::Number(1.0)),
                        CoreValueKind::Number,
                    ),
                ],
                keywords: Vec::new(),
            },
            CoreValueKind::Solid,
        );
        let bad_extrude = CoreNode::new(
            NodeId::new(1),
            CoreNodeKind::Call {
                op: CoreOperation::Surface(CoreSurfaceOp::Extrude),
                args: vec![
                    box_node,
                    CoreNode::new(
                        NodeId::new(6),
                        CoreNodeKind::Literal(CoreLiteral::Number(10.0)),
                        CoreValueKind::Number,
                    ),
                ],
                keywords: Vec::new(),
            },
            CoreValueKind::Solid,
        );
        let program = CoreProgram::new(
            ProgramId::new(1),
            Vec::new(),
            vec![CorePart {
                id: PartId::new(1),
                key: "body".into(),
                label: "Body".into(),
                root: bad_extrude,
            }],
        );

        let err = plan_core_program(&program).expect_err("invalid type");
        let message = err.to_string();

        assert!(message.contains("extrude"), "{message}");
        assert!(message.contains("sketch"), "{message}");
        assert!(message.contains("solid"), "{message}");
    }
}
