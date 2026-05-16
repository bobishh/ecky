use std::collections::BTreeMap;

use crate::ecky_core_ir::{
    CoreArrayOp, CoreBinding, CoreBooleanOp, CoreFrameOp, CoreKeywordArg, CoreLiteral, CoreMetaOp,
    CoreNode, CoreNodeKind, CoreOperation, CoreParameterKind, CorePart, CorePathOp, CorePrimitive,
    CoreProgram, CoreReference, CoreSelectorPayload, CoreShapeBinding, CoreSurfaceOp, CoreSymbol,
    CoreTransformOp, CoreValueKind, NodeId,
};
use crate::models::{AppError, AppResult, DesignParams, ParamValue};

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
    Cone,
    Circle,
    Rectangle,
    RoundedRectangle,
    RoundedPolygon,
    Polygon,
    Profile,
    MakeFace,
    Extrude,
    Revolve,
    Loft,
    Sweep,
    Twist,
    Taper,
    Offset,
    Path,
    BezierPath,
    Bspline,
    Plane,
    Location,
    PathFrame,
    Place,
    ClipBox,
    LinearArray,
    RadialArray,
    GridArray,
    ArcArray,
    Union,
    Difference,
    Intersection,
    Fillet,
    Chamfer,
    Shell,
    Translate,
    Rotate,
    Scale,
    Mirror,
    Compound,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OcctKeywordValue {
    Arg(OcctArg),
    Selector {
        source: OcctArg,
        payload: CoreSelectorPayload,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcctKeyword {
    pub name: String,
    pub value: OcctKeywordValue,
}

impl OcctKeyword {
    pub fn arg(name: String, value: OcctArg) -> Self {
        Self {
            name,
            value: OcctKeywordValue::Arg(value),
        }
    }

    pub fn selector(name: String, source: OcctArg, payload: CoreSelectorPayload) -> Self {
        Self {
            name,
            value: OcctKeywordValue::Selector { source, payload },
        }
    }

    pub fn source_arg(&self) -> &OcctArg {
        match &self.value {
            OcctKeywordValue::Arg(value) => value,
            OcctKeywordValue::Selector { source, .. } => source,
        }
    }

    pub fn source_arg_mut(&mut self) -> &mut OcctArg {
        match &mut self.value {
            OcctKeywordValue::Arg(value) => value,
            OcctKeywordValue::Selector { source, .. } => source,
        }
    }

    pub fn selector_payload(&self) -> Option<&CoreSelectorPayload> {
        match &self.value {
            OcctKeywordValue::Arg(_) => None,
            OcctKeywordValue::Selector { payload, .. } => Some(payload),
        }
    }

    pub fn set_selector_payload(&mut self, selector: Option<CoreSelectorPayload>) {
        let source = self.source_arg().clone();
        self.value = match selector {
            Some(payload) => OcctKeywordValue::Selector { source, payload },
            None => OcctKeywordValue::Arg(source),
        };
    }
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
    plan_core_program_with_params(program, &DesignParams::new())
}

pub fn plan_core_program_with_params(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AppResult<OcctPlan> {
    let expanded = expand_core_program_for_direct_occt(program, parameters)?;
    plan_expanded_core_program(&expanded)
}

fn plan_expanded_core_program(program: &CoreProgram) -> AppResult<OcctPlan> {
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

fn expand_core_program_for_direct_occt(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AppResult<CoreProgram> {
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let env = crate::ecky_ir::build_core_program_param_env_for_eval(program, parameters)?;
    let mut next_node_id = next_program_node_id(program);
    let parts = program
        .parts
        .iter()
        .map(|part| {
            Ok(CorePart {
                id: part.id,
                key: part.key.clone(),
                label: part.label.clone(),
                root: expand_node_for_direct_occt(
                    &part.root,
                    &param_names,
                    &env,
                    &mut next_node_id,
                )?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(CoreProgram::new(
        program.id,
        program.parameters.clone(),
        parts,
    ))
}

fn expand_node_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => Ok(node.clone()),
        CoreNodeKind::Build { bindings, result } => {
            let bindings = bindings
                .iter()
                .map(|binding| {
                    Ok(CoreShapeBinding {
                        name: binding.name.clone(),
                        value: expand_node_for_direct_occt(
                            &binding.value,
                            param_names,
                            env,
                            next_node_id,
                        )?,
                    })
                })
                .collect::<AppResult<Vec<_>>>()?;
            Ok(rebuild_node(
                node,
                CoreNodeKind::Build {
                    bindings,
                    result: Box::new(expand_node_for_direct_occt(
                        result,
                        param_names,
                        env,
                        next_node_id,
                    )?),
                },
            ))
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested_env = env.clone();
            let mut expanded_bindings = Vec::with_capacity(bindings.len());
            for binding in bindings {
                let value = expand_node_for_direct_occt(
                    &binding.value,
                    param_names,
                    &nested_env,
                    next_node_id,
                )?;
                if let Some(param_value) =
                    eval_scalar_binding_for_direct_occt(&value, param_names, &nested_env)?
                {
                    nested_env.insert(binding.name.clone(), param_value);
                }
                expanded_bindings.push(CoreBinding {
                    name: binding.name.clone(),
                    value,
                });
            }
            Ok(rebuild_node(
                node,
                CoreNodeKind::Let {
                    bindings: expanded_bindings,
                    body: Box::new(expand_node_for_direct_occt(
                        body,
                        param_names,
                        &nested_env,
                        next_node_id,
                    )?),
                },
            ))
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => Ok(rebuild_node(
            node,
            CoreNodeKind::If {
                condition: Box::new(expand_node_for_direct_occt(
                    condition,
                    param_names,
                    env,
                    next_node_id,
                )?),
                then_branch: Box::new(expand_node_for_direct_occt(
                    then_branch,
                    param_names,
                    env,
                    next_node_id,
                )?),
                else_branch: Box::new(expand_node_for_direct_occt(
                    else_branch,
                    param_names,
                    env,
                    next_node_id,
                )?),
            },
        )),
        CoreNodeKind::Call { op, args, keywords }
            if matches!(op, CoreOperation::Surface(CoreSurfaceOp::Shell))
                && sampled_radial_loft_target(args).is_some() =>
        {
            expand_shell_sampled_radial_loft_node(
                node,
                args,
                keywords,
                param_names,
                env,
                next_node_id,
            )
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "sampled-radial-loft") => {
            expand_sampled_radial_loft_node(node, args, keywords, param_names, env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } => Ok(rebuild_node(
            node,
            CoreNodeKind::Call {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| expand_node_for_direct_occt(arg, param_names, env, next_node_id))
                    .collect::<AppResult<Vec<_>>>()?,
                keywords: keywords
                    .iter()
                    .map(|keyword| {
                        let value = expand_node_for_direct_occt(
                            keyword.source_node(),
                            param_names,
                            env,
                            next_node_id,
                        )?;
                        Ok(match keyword.selector_payload() {
                            Some(selector) => CoreKeywordArg::selector(
                                keyword.name.clone(),
                                value,
                                selector.clone(),
                            ),
                            None => CoreKeywordArg::expr(keyword.name.clone(), value),
                        })
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            },
        )),
        CoreNodeKind::Range { start, end } => Ok(rebuild_node(
            node,
            CoreNodeKind::Range {
                start: Box::new(expand_node_for_direct_occt(
                    start,
                    param_names,
                    env,
                    next_node_id,
                )?),
                end: Box::new(expand_node_for_direct_occt(
                    end,
                    param_names,
                    env,
                    next_node_id,
                )?),
            },
        )),
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => Ok(rebuild_node(
            node,
            CoreNodeKind::Map {
                params: params.clone(),
                sources: sources
                    .iter()
                    .map(|source| {
                        expand_node_for_direct_occt(source, param_names, env, next_node_id)
                    })
                    .collect::<AppResult<Vec<_>>>()?,
                body: Box::new(expand_node_for_direct_occt(
                    body,
                    param_names,
                    env,
                    next_node_id,
                )?),
            },
        )),
        CoreNodeKind::Apply { op, args, list } => Ok(rebuild_node(
            node,
            CoreNodeKind::Apply {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| expand_node_for_direct_occt(arg, param_names, env, next_node_id))
                    .collect::<AppResult<Vec<_>>>()?,
                list: Box::new(expand_node_for_direct_occt(
                    list,
                    param_names,
                    env,
                    next_node_id,
                )?),
            },
        )),
        CoreNodeKind::List(items) => Ok(rebuild_node(
            node,
            CoreNodeKind::List(
                items
                    .iter()
                    .map(|item| expand_node_for_direct_occt(item, param_names, env, next_node_id))
                    .collect::<AppResult<Vec<_>>>()?,
            ),
        )),
        CoreNodeKind::Group(items) => Ok(rebuild_node(
            node,
            CoreNodeKind::Group(
                items
                    .iter()
                    .map(|item| expand_node_for_direct_occt(item, param_names, env, next_node_id))
                    .collect::<AppResult<Vec<_>>>()?,
            ),
        )),
    }
}

fn expand_sampled_radial_loft_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if args.len() != 1 {
        return Err(AppError::validation(
            "`sampled-radial-loft` expects binder names plus keyword/value options.",
        ));
    }
    let binders = sampled_radial_loft_binders(&args[0])?;
    let height_node = sampled_keyword_node(keywords, "height")?;
    let z_steps_node = sampled_keyword_node(keywords, "z-steps")?;
    let theta_steps_node = sampled_keyword_node(keywords, "theta-steps")?;
    let radius_node = sampled_keyword_node(keywords, "radius")?;
    let z_map_node = sampled_optional_keyword_node(keywords, "z-map");

    let height = crate::ecky_ir::eval_core_number_with_locals(height_node, param_names, env)?;
    let z_steps = sampled_count(
        crate::ecky_ir::eval_core_number_with_locals(z_steps_node, param_names, env)?,
        1,
        "z-steps",
    )?;
    let theta_steps = sampled_count(
        crate::ecky_ir::eval_core_number_with_locals(theta_steps_node, param_names, env)?,
        3,
        "theta-steps",
    )?;

    let mut loft_args = Vec::with_capacity(z_steps + 3);
    loft_args.push(number_node(next_node_id, 0.0));

    for zi in 0..=z_steps {
        let fz = zi as f64 / z_steps as f64;
        let z = height * fz;
        let mut section_env = env.clone();
        section_env.insert(binders[1].clone(), ParamValue::Number(z));
        section_env.insert(binders[2].clone(), ParamValue::Number(fz));

        let mut points = Vec::with_capacity(theta_steps);
        for ti in 0..theta_steps {
            let theta = 2.0 * std::f64::consts::PI * ti as f64 / theta_steps as f64;
            section_env.insert(binders[0].clone(), ParamValue::Number(theta));
            let radius = crate::ecky_ir::eval_core_number_with_locals(
                radius_node,
                param_names,
                &section_env,
            )?;
            if !radius.is_finite() || radius <= 0.0 {
                return Err(AppError::validation(
                    "sampled-radial-loft radius must stay positive",
                ));
            }
            points.push(CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::Literal(CoreLiteral::Point2([
                    radius * theta.cos(),
                    radius * theta.sin(),
                ])),
                CoreValueKind::Point2,
            ));
        }

        let section_z = z_map_node
            .map(|z_map| {
                crate::ecky_ir::eval_core_number_with_locals(z_map, param_names, &section_env)
            })
            .transpose()?
            .unwrap_or(z);
        let polygon = CoreNode::new(
            next_id(next_node_id),
            CoreNodeKind::Call {
                op: CoreOperation::Primitive(CorePrimitive::Polygon),
                args: vec![CoreNode::new(
                    next_id(next_node_id),
                    CoreNodeKind::List(points),
                    CoreValueKind::List,
                )],
                keywords: Vec::new(),
            },
            CoreValueKind::Sketch,
        );
        let translated = CoreNode::new(
            next_id(next_node_id),
            CoreNodeKind::Call {
                op: CoreOperation::Transform(CoreTransformOp::Translate),
                args: vec![
                    number_node(next_node_id, 0.0),
                    number_node(next_node_id, 0.0),
                    number_node(next_node_id, section_z),
                    polygon,
                ],
                keywords: Vec::new(),
            },
            CoreValueKind::Sketch,
        );
        loft_args.push(translated);
    }

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Surface(CoreSurfaceOp::Loft),
            args: loft_args,
            keywords: Vec::new(),
        },
    ))
}

fn expand_shell_sampled_radial_loft_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if !keywords.is_empty() || args.len() != 2 {
        return Err(AppError::validation(
            "`shell` sampled-radial-loft expects thickness and shape only.",
        ));
    }
    let target = sampled_radial_loft_target(args).ok_or_else(|| {
        AppError::validation("`shell` sampled-radial-loft requires a sampled-radial-loft target.")
    })?;
    let target_args = match &target.kind {
        CoreNodeKind::Call { args, .. } => args,
        _ => unreachable!(),
    };
    let target_keywords = match &target.kind {
        CoreNodeKind::Call { keywords, .. } => keywords,
        _ => unreachable!(),
    };

    let outer = expand_sampled_radial_loft_node(
        target,
        target_args,
        target_keywords,
        param_names,
        env,
        next_node_id,
    )?;
    let inner_radius = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Custom("-".to_string()),
            args: vec![
                sampled_keyword_node(target_keywords, "radius")?.clone(),
                args[0].clone(),
            ],
            keywords: Vec::new(),
        },
        CoreValueKind::Number,
    );
    let inner_sampled = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Custom("sampled-radial-loft".to_string()),
            args: target_args.to_vec(),
            keywords: sampled_replaced_keywords(target_keywords, "radius", inner_radius),
        },
        CoreValueKind::Solid,
    );
    let inner = match &inner_sampled.kind {
        CoreNodeKind::Call { args, keywords, .. } => expand_sampled_radial_loft_node(
            &inner_sampled,
            args,
            keywords,
            param_names,
            env,
            next_node_id,
        )?,
        _ => unreachable!(),
    };

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(CoreBooleanOp::Difference),
            args: vec![outer, inner],
            keywords: Vec::new(),
        },
    ))
}

fn sampled_radial_loft_target(args: &[CoreNode]) -> Option<&CoreNode> {
    match args {
        [_, target]
            if matches!(
                target.kind,
                CoreNodeKind::Call {
                    op: CoreOperation::Custom(ref name),
                    ..
                } if name == "sampled-radial-loft"
            ) =>
        {
            Some(target)
        }
        _ => None,
    }
}

fn sampled_replaced_keywords(
    keywords: &[CoreKeywordArg],
    name: &str,
    value: CoreNode,
) -> Vec<CoreKeywordArg> {
    keywords
        .iter()
        .map(|keyword| {
            if keyword.name == name {
                match keyword.selector_payload() {
                    Some(selector) => CoreKeywordArg::selector(
                        keyword.name.clone(),
                        value.clone(),
                        selector.clone(),
                    ),
                    None => CoreKeywordArg::expr(keyword.name.clone(), value.clone()),
                }
            } else {
                keyword.clone()
            }
        })
        .collect()
}

fn sampled_radial_loft_binders(arg: &CoreNode) -> AppResult<[String; 3]> {
    match &arg.kind {
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            if items.len() != 3 {
                return Err(AppError::validation(
                    "`sampled-radial-loft` binders must be `(theta z fz)`.",
                ));
            }
            Ok([
                sampled_binder_name(&items[0])?,
                sampled_binder_name(&items[1])?,
                sampled_binder_name(&items[2])?,
            ])
        }
        CoreNodeKind::Call {
            op: CoreOperation::Custom(head),
            args,
            keywords,
        } if keywords.is_empty() && args.len() == 2 => Ok([
            head.clone(),
            sampled_binder_name(&args[0])?,
            sampled_binder_name(&args[1])?,
        ]),
        _ => Err(AppError::validation(
            "`sampled-radial-loft` binders must be `(theta z fz)`.",
        )),
    }
}

fn sampled_binder_name(node: &CoreNode) -> AppResult<String> {
    match &node.kind {
        CoreNodeKind::Reference(CoreReference::Local(name)) => Ok(name.clone()),
        CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(text.clone()),
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => Ok(symbol_name(symbol).to_string()),
        _ => Err(AppError::validation(
            "`sampled-radial-loft` binders must be symbols.",
        )),
    }
}

fn sampled_keyword_node<'a>(keywords: &'a [CoreKeywordArg], name: &str) -> AppResult<&'a CoreNode> {
    sampled_optional_keyword_node(keywords, name)
        .ok_or_else(|| AppError::validation(format!("`sampled-radial-loft` requires `:{}`.", name)))
}

fn sampled_optional_keyword_node<'a>(
    keywords: &'a [CoreKeywordArg],
    name: &str,
) -> Option<&'a CoreNode> {
    keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .map(|keyword| keyword.source_node())
}

fn sampled_count(value: f64, minimum: usize, label: &str) -> AppResult<usize> {
    if !value.is_finite() {
        return Err(AppError::validation(format!(
            "`sampled-radial-loft` {label} must be finite."
        )));
    }
    Ok((value.round() as isize).max(minimum as isize) as usize)
}

fn eval_scalar_binding_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<Option<ParamValue>> {
    match node.value_kind {
        CoreValueKind::Number => Ok(Some(ParamValue::Number(
            crate::ecky_ir::eval_core_number_with_locals(node, param_names, env)?,
        ))),
        CoreValueKind::Boolean => Ok(Some(ParamValue::Boolean(
            crate::ecky_ir::eval_core_bool_with_locals(node, param_names, env)?,
        ))),
        CoreValueKind::Text => Ok(Some(ParamValue::String(
            crate::ecky_ir::eval_core_stringish_with_locals(node, param_names, env)?,
        ))),
        _ => Ok(None),
    }
}

fn rebuild_node(node: &CoreNode, kind: CoreNodeKind) -> CoreNode {
    let mut rebuilt = CoreNode::new(node.id, kind, node.value_kind);
    rebuilt.span = node.span;
    rebuilt
}

fn number_node(next_node_id: &mut u64, value: f64) -> CoreNode {
    CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Literal(CoreLiteral::Number(value)),
        CoreValueKind::Number,
    )
}

fn next_program_node_id(program: &CoreProgram) -> u64 {
    program
        .parts
        .iter()
        .map(|part| max_node_id(&part.root))
        .max()
        .unwrap_or(0)
        + 1
}

fn max_node_id(node: &CoreNode) -> u64 {
    let child_max = match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => 0,
        CoreNodeKind::Build { bindings, result } => bindings
            .iter()
            .map(|binding| max_node_id(&binding.value))
            .chain(std::iter::once(max_node_id(result)))
            .max()
            .unwrap_or(0),
        CoreNodeKind::Let { bindings, body } => bindings
            .iter()
            .map(|binding| max_node_id(&binding.value))
            .chain(std::iter::once(max_node_id(body)))
            .max()
            .unwrap_or(0),
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => [
            max_node_id(condition),
            max_node_id(then_branch),
            max_node_id(else_branch),
        ]
        .into_iter()
        .max()
        .unwrap_or(0),
        CoreNodeKind::Call { args, keywords, .. } => args
            .iter()
            .map(max_node_id)
            .chain(
                keywords
                    .iter()
                    .map(|keyword| max_node_id(keyword.source_node())),
            )
            .max()
            .unwrap_or(0),
        CoreNodeKind::Range { start, end } => [max_node_id(start), max_node_id(end)]
            .into_iter()
            .max()
            .unwrap_or(0),
        CoreNodeKind::Map { sources, body, .. } => sources
            .iter()
            .map(max_node_id)
            .chain(std::iter::once(max_node_id(body)))
            .max()
            .unwrap_or(0),
        CoreNodeKind::Apply { args, list, .. } => args
            .iter()
            .map(max_node_id)
            .chain(std::iter::once(max_node_id(list)))
            .max()
            .unwrap_or(0),
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().map(max_node_id).max().unwrap_or(0)
        }
    };
    node.id.raw().max(child_max)
}

fn next_id(next_node_id: &mut u64) -> NodeId {
    let id = *next_node_id;
    *next_node_id += 1;
    NodeId::new(id)
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
                        let value = self.plan_arg(keyword.source_node())?;
                        Ok(match keyword.selector_payload() {
                            Some(selector) => {
                                OcctKeyword::selector(keyword.name.clone(), value, selector.clone())
                            }
                            None => OcctKeyword::arg(keyword.name.clone(), value),
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
        CoreOperation::Primitive(CorePrimitive::Cone) => Ok(OcctOp::Cone),
        CoreOperation::Primitive(CorePrimitive::Circle) => Ok(OcctOp::Circle),
        CoreOperation::Primitive(CorePrimitive::Rectangle) => Ok(OcctOp::Rectangle),
        CoreOperation::Primitive(CorePrimitive::RoundedRectangle) => Ok(OcctOp::RoundedRectangle),
        CoreOperation::Primitive(CorePrimitive::RoundedPolygon) => Ok(OcctOp::RoundedPolygon),
        CoreOperation::Primitive(CorePrimitive::Polygon) => Ok(OcctOp::Polygon),
        CoreOperation::Primitive(CorePrimitive::Profile) => Ok(OcctOp::Profile),
        CoreOperation::Primitive(CorePrimitive::MakeFace) => Ok(OcctOp::MakeFace),
        CoreOperation::Surface(CoreSurfaceOp::Extrude) => Ok(OcctOp::Extrude),
        CoreOperation::Surface(CoreSurfaceOp::Revolve) => Ok(OcctOp::Revolve),
        CoreOperation::Surface(CoreSurfaceOp::Loft) => Ok(OcctOp::Loft),
        CoreOperation::Surface(CoreSurfaceOp::Sweep) => Ok(OcctOp::Sweep),
        CoreOperation::Surface(CoreSurfaceOp::Twist) => Ok(OcctOp::Twist),
        CoreOperation::Surface(CoreSurfaceOp::Taper) => Ok(OcctOp::Taper),
        CoreOperation::Surface(CoreSurfaceOp::Offset) => Ok(OcctOp::Offset),
        CoreOperation::Surface(CoreSurfaceOp::OffsetRounded) => Ok(OcctOp::Offset),
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => Ok(OcctOp::Fillet),
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => Ok(OcctOp::Chamfer),
        CoreOperation::Surface(CoreSurfaceOp::Shell) => Ok(OcctOp::Shell),
        CoreOperation::Path(CorePathOp::Polyline) => Ok(OcctOp::Path),
        CoreOperation::Path(CorePathOp::BezierPath) => Ok(OcctOp::BezierPath),
        CoreOperation::Path(CorePathOp::Bspline) => Ok(OcctOp::Bspline),
        CoreOperation::Frame(CoreFrameOp::Plane) => Ok(OcctOp::Plane),
        CoreOperation::Frame(CoreFrameOp::Location) => Ok(OcctOp::Location),
        CoreOperation::Frame(CoreFrameOp::PathFrame) => Ok(OcctOp::PathFrame),
        CoreOperation::Frame(CoreFrameOp::Place) => Ok(OcctOp::Place),
        CoreOperation::Frame(CoreFrameOp::ClipBox) => Ok(OcctOp::ClipBox),
        CoreOperation::Array(CoreArrayOp::LinearArray) => Ok(OcctOp::LinearArray),
        CoreOperation::Array(CoreArrayOp::RadialArray) => Ok(OcctOp::RadialArray),
        CoreOperation::Array(CoreArrayOp::GridArray) => Ok(OcctOp::GridArray),
        CoreOperation::Array(CoreArrayOp::ArcArray) => Ok(OcctOp::ArcArray),
        CoreOperation::Boolean(CoreBooleanOp::Union) => Ok(OcctOp::Union),
        CoreOperation::Boolean(CoreBooleanOp::Difference) => Ok(OcctOp::Difference),
        CoreOperation::Boolean(CoreBooleanOp::Intersection) => Ok(OcctOp::Intersection),
        CoreOperation::Transform(CoreTransformOp::Translate) => Ok(OcctOp::Translate),
        CoreOperation::Transform(CoreTransformOp::Rotate) => Ok(OcctOp::Rotate),
        CoreOperation::Transform(CoreTransformOp::Scale) => Ok(OcctOp::Scale),
        CoreOperation::Transform(CoreTransformOp::Mirror) => Ok(OcctOp::Mirror),
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
        .and_then(|keyword| match &keyword.source_node().kind {
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
        CoreOperation::Surface(CoreSurfaceOp::OffsetRounded) => "offset-rounded",
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => "fillet",
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => "chamfer",
        CoreOperation::Surface(CoreSurfaceOp::Taper) => "taper",
        CoreOperation::Surface(CoreSurfaceOp::Twist) => "twist",
        CoreOperation::Path(CorePathOp::Polyline) => "path",
        CoreOperation::Path(CorePathOp::BezierPath) => "bezier-path",
        CoreOperation::Path(CorePathOp::Bspline) => "bspline",
        CoreOperation::Array(CoreArrayOp::LinearArray) => "linear-array",
        CoreOperation::Array(CoreArrayOp::RadialArray) => "radial-array",
        CoreOperation::Array(CoreArrayOp::GridArray) => "grid-array",
        CoreOperation::Array(CoreArrayOp::ArcArray) => "arc-array",
        CoreOperation::Array(CoreArrayOp::Repeat) => "repeat",
        CoreOperation::Array(CoreArrayOp::RepeatUnion) => "repeat-union",
        CoreOperation::Array(CoreArrayOp::RepeatCompound) => "repeat-compound",
        CoreOperation::Array(CoreArrayOp::RepeatPick) => "repeat-pick",
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
        CoreSelectorPayload, CoreSurfaceOp, CoreValueKind, NodeId, PartId, ProgramId,
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
    fn plans_exact_edge_selector_payload_into_direct_occt_keywords() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "target-id:body:edge:0:0-0-0_0-0-10"
                  (box 10 10 10))))
            "#,
        );
        let plan = plan_core_program(&program).expect("plan");
        let fillet = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Fillet)
            .expect("fillet");
        assert_eq!(
            fillet.keywords[0].selector_payload(),
            Some(CoreSelectorPayload::EdgeTargetIds(vec![
                "body:edge:0:0-0-0_0-0-10".into()
            ]))
            .as_ref()
        );
    }

    #[test]
    fn plans_coarse_edge_selector_payload_into_direct_occt_keywords() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "left+vertical"
                  (box 10 10 10))))
            "#,
        );
        let plan = plan_core_program(&program).expect("plan");
        let fillet = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Fillet)
            .expect("fillet");
        assert_eq!(
            fillet.keywords[0].selector_payload(),
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
    fn plans_exact_face_selector_payload_into_direct_occt_keywords() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "target-id:body:face:0:0-0-10:400"
                  (box 10 10 10))))
            "#,
        );
        let plan = plan_core_program(&program).expect("plan");
        let shell = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Shell)
            .expect("shell");
        assert_eq!(
            shell.keywords[0].selector_payload(),
            Some(CoreSelectorPayload::FaceTargetIds(vec![
                "body:face:0:0-0-10:400".into()
            ]))
            .as_ref()
        );
    }

    #[test]
    fn plans_richer_face_selector_payload_into_direct_occt_keywords() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "planar+normal-z+area-max"
                  (box 10 10 10))))
            "#,
        );
        let plan = plan_core_program(&program).expect("plan");
        let shell = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Shell)
            .expect("shell");
        assert_eq!(
            shell.keywords[0].selector_payload(),
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
    fn plans_cone_primitive_for_direct_occt() {
        let program = compile("(model (part body (cone 10 4 30 32)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(plan.parts[0].commands.len(), 1);
        assert_eq!(plan.parts[0].commands[0].op, OcctOp::Cone);
        assert_eq!(
            plan.parts[0].commands[0].args[..3],
            [
                OcctArg::Number(10.0),
                OcctArg::Number(4.0),
                OcctArg::Number(30.0)
            ]
        );
    }

    #[test]
    fn plans_rounded_rectangle_profile_for_direct_occt() {
        let program = compile("(model (part body (extrude (rounded_rect 20 10 2) 5)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::RoundedRectangle, OcctOp::Extrude]
        );
    }

    #[test]
    fn plans_rounded_polygon_profile_for_direct_occt() {
        let program = compile(
            "(model (part body (extrude (rounded-polygon ((0 0) (20 0) (20 10) (0 10)) 2) 5)))",
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::RoundedPolygon, OcctOp::Extrude]
        );
    }

    #[test]
    fn plans_loft_for_direct_occt() {
        let program = compile("(model (part body (loft 30 (circle 10) (rounded-rect 12 8 2))))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Circle, OcctOp::RoundedRectangle, OcctOp::Loft]
        );
    }

    #[test]
    fn plans_sweep_path_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (sweep
                  (circle 5)
                  (path ((0 0 0) (0 0 24))))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(plan.parts[0].commands.len(), 3);
        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Circle, OcctOp::Path, OcctOp::Sweep]
        );
    }

    #[test]
    fn plans_bezier_path_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (sweep
                  (circle 2)
                  (bezier-path ((0 0 0) (8 0 0) (8 8 12) (16 8 12))))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Circle, OcctOp::BezierPath, OcctOp::Sweep]
        );
    }

    #[test]
    fn plans_bspline_profile_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (bspline ((0 6) (5 2) (6 -4) (0 -6) (-6 -4) (-5 2)) #t)
                  4)))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Bspline, OcctOp::Extrude]
        );
    }

    #[test]
    fn plans_twist_for_direct_occt() {
        let program = compile("(model (part body (twist 24 90 (circle 5))))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Circle, OcctOp::Twist]
        );
    }

    #[test]
    fn plans_sampled_radial_loft_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (sampled-radial-loft
                  (theta z fz)
                  :height 40
                  :z-steps 2
                  :theta-steps 4
                  :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                  :z-map (+ z (* fz 2)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();
        assert_eq!(
            ops,
            vec![
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Loft,
            ]
        );
        let loft = plan.parts[0].commands.last().expect("loft");
        assert_eq!(loft.op, OcctOp::Loft);
        assert_eq!(loft.args[0], OcctArg::Number(0.0));
    }

    #[test]
    fn plans_shell_sampled_radial_loft_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 2
                  (sampled-radial-loft
                    (theta z fz)
                    :height 40
                    :z-steps 2
                    :theta-steps 4
                    :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();
        assert_eq!(
            ops,
            vec![
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Loft,
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Polygon,
                OcctOp::Translate,
                OcctOp::Loft,
                OcctOp::Difference,
            ]
        );
        let difference = plan.parts[0].commands.last().expect("difference");
        assert_eq!(difference.op, OcctOp::Difference);
        assert_eq!(plan.parts[0].root, difference.output);
    }

    #[test]
    fn plans_profile_with_holes_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (profile :outer (circle 10) :holes (circle 3))
                  4)))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![
                OcctOp::Circle,
                OcctOp::Circle,
                OcctOp::Profile,
                OcctOp::Extrude
            ]
        );
    }

    #[test]
    fn plans_make_face_for_direct_occt() {
        let program = compile(
            "(model (part body (extrude (make-face (polygon ((0 0) (8 0) (8 6) (0 6)))) 4)))",
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Polygon, OcctOp::MakeFace, OcctOp::Extrude]
        );
    }

    #[test]
    fn plans_offset_for_direct_occt() {
        let program = compile("(model (part body (extrude (offset 2 (circle 10)) 4)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Circle, OcctOp::Offset, OcctOp::Extrude]
        );
    }

    #[test]
    fn plans_mirror_taper_and_offset_rounded_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (compound
                  (mirror "x" 0 (box 4 5 6))
                  (translate 14 0 0
                    (taper 12 0.55 0.8 (rounded-rect 8 6 1)))
                  (translate 28 0 0
                    (extrude (offset-rounded 1.5 (circle 5)) 4)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![
                OcctOp::Box,
                OcctOp::Mirror,
                OcctOp::RoundedRectangle,
                OcctOp::Taper,
                OcctOp::Translate,
                OcctOp::Circle,
                OcctOp::Offset,
                OcctOp::Extrude,
                OcctOp::Translate,
                OcctOp::Compound,
            ]
        );
    }

    #[test]
    fn plans_path_frame_place_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape rail (path ((0 0 0) (0 0 20))))
                  (shape peg (cylinder 2 6))
                  (shape end-frame (path-frame rail :at end))
                  (result (place end-frame peg :offset (0 0 -3))))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![
                OcctOp::Path,
                OcctOp::Cylinder,
                OcctOp::PathFrame,
                OcctOp::Place
            ]
        );
        let frame = &plan.parts[0].commands[2];
        assert_eq!(frame.keywords[0].name, "at");
        assert_eq!(
            frame.keywords[0].source_arg(),
            &OcctArg::Symbol("end".into())
        );
        let place = &plan.parts[0].commands[3];
        assert_eq!(place.keywords[0].name, "offset");
        assert_eq!(
            place.keywords[0].source_arg(),
            &OcctArg::Point3([0.0, 0.0, -3.0])
        );
    }

    #[test]
    fn plans_plane_location_place_clip_box_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape base (plane :origin (0 0 4) :normal (0 0 1)))
                  (shape loc (location base :offset (5 0 0) :rotate (0 0 90)))
                  (shape peg (box 2 4 6))
                  (shape placed (place loc peg))
                  (result
                    (clip-box placed :x (0 10) :y (-5 5) :z (0 12))))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![
                OcctOp::Plane,
                OcctOp::Location,
                OcctOp::Box,
                OcctOp::Place,
                OcctOp::ClipBox
            ]
        );
        assert_eq!(plan.parts[0].commands[0].keywords[0].name, "origin");
        assert_eq!(plan.parts[0].commands[1].keywords[0].name, "offset");
        assert_eq!(plan.parts[0].commands[4].keywords[0].name, "x");
    }

    #[test]
    fn plans_array_ops_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (compound
                  (linear-array 3 10 0 0 (box 2 2 2))
                  (radial-array 4 90 20 (cylinder 2 5))
                  (grid-array 2 3 8 9 (sphere 2))
                  (arc-array 5 30 0 180 (cone 2 1 4)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![
                OcctOp::Box,
                OcctOp::LinearArray,
                OcctOp::Cylinder,
                OcctOp::RadialArray,
                OcctOp::Sphere,
                OcctOp::GridArray,
                OcctOp::Cone,
                OcctOp::ArcArray,
                OcctOp::Compound,
            ]
        );
    }

    #[test]
    fn rejects_unsupported_first_surface_ops_by_name() {
        let program = compile(
            r#"
            (model
              (part body
                (xor (box 2 2 2) (sphere 1))))
            "#,
        );

        let err = plan_core_program(&program).expect_err("xor unsupported");
        let message = err.to_string();

        assert!(message.contains("Direct OCCT adapter"), "{message}");
        assert!(message.contains("xor"), "{message}");
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
