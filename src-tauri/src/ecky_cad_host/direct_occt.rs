use std::collections::BTreeMap;
use std::fs;

use crate::ecky_cad_host::svg_profile::{
    extract_svg_wire_soup_profile, parse_svg_profile, SvgFillRule, SvgFitMode,
};
use crate::ecky_cad_host::text_profile::parse_text_profile;
use crate::ecky_core_ir::{
    CoreArrayOp, CoreBinding, CoreBooleanOp, CoreFrameOp, CoreKeywordArg, CoreLiteral, CoreMetaOp,
    CoreNode, CoreNodeKind, CoreOperation, CoreParameterKind, CorePart, CorePathOp, CorePrimitive,
    CoreProgram, CoreReference, CoreSelectorPayload, CoreShapeBinding, CoreSurfaceOp, CoreSymbol,
    CoreTransformOp, CoreValueKind, NodeId,
};
use crate::contracts::{AuthoringError, AuthoringReason, ErrorFix};
use crate::models::{AppError, AppResult, DesignParams, ParamValue};

// --- Authoring-error constructors (backend layer) -------------------------
// The direct OCCT planner is the backend wall: every failure here means an op
// the surface authored cannot be executed by this backend. These helpers keep
// call sites one line and guarantee a backend-layered error.

fn bk(reason: AuthoringReason, msg: impl Into<String>) -> AppError {
    AuthoringError::backend(reason, msg).into()
}

fn bk_op(reason: AuthoringReason, op: &str, msg: impl Into<String>) -> AppError {
    AuthoringError::backend(reason, msg).with_op(op).into()
}

fn bk_arity(op: &str, expected: &str) -> AppError {
    AuthoringError::backend(AuthoringReason::Arity, format!("`{op}` expects {expected}."))
        .with_op(op)
        .into()
}

fn bk_constrained(op: &str, msg: impl Into<String>, valid: &[&str]) -> AppError {
    AuthoringError::backend(AuthoringReason::ConstrainedValue, msg)
        .with_op(op)
        .with_fix(ErrorFix {
            hint: Some(format!("valid values: {}", valid.join(", "))),
            suggestions: valid.iter().map(|s| (*s).to_string()).collect(),
        })
        .into()
}

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
    Torus,
    Wedge,
    Circle,
    Ellipse,
    Slot,
    SlotArc,
    Rectangle,
    RoundedRectangle,
    RoundedPolygon,
    Polygon,
    Profile,
    MakeFace,
    ImportStl,
    Extrude,
    Revolve,
    Loft,
    Sweep,
    Twist,
    Taper,
    Draft,
    Offset,
    Path,
    HelixPath,
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
    Hull,
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
    let normalized =
        super::direct_occt_normalize::normalize_core_program_for_direct_occt(program, parameters)?;
    let expanded = expand_core_program_for_direct_occt(&normalized, parameters)?;
    plan_expanded_core_program(&expanded, parameters)
}

fn plan_expanded_core_program(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AppResult<OcctPlan> {
    crate::ecky_core_ir::verify_core_program(program).map_err(|err| {
        bk(AuthoringReason::Type, format!(
            "Direct OCCT adapter rejected invalid Core IR before planning: {}",
            err
        ))
    })?;

    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let occt_parameters = program
        .parameters
        .iter()
        .map(|param| OcctParameter {
            key: param.key.clone(),
            kind: param.kind.into(),
        })
        .collect::<Vec<_>>();
    let scalar_env = crate::ecky_ir::build_core_program_param_env_for_eval(program, parameters)?;

    let parts = program
        .parts
        .iter()
        .map(|part| {
            let mut planner =
                PartPlanner::new(&param_names, &scalar_env, max_node_id(&part.root) + 1);
            let root = planner.plan_node(&part.root)?;
            Ok(OcctPartPlan {
                key: part.key.clone(),
                label: part.label.clone(),
                root,
                commands: planner.commands,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(OcctPlan {
        parameters: occt_parameters,
        parts,
    })
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
    let node_env = BTreeMap::new();
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
                    &node_env,
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
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => Ok(node.clone()),
        CoreNodeKind::Build { bindings, result } => {
            let mut nested_env = env.clone();
            let mut nested_node_env = node_env.clone();
            let mut expanded_bindings = Vec::with_capacity(bindings.len());
            for binding in bindings {
                let value = expand_node_for_direct_occt(
                    &binding.value,
                    param_names,
                    &nested_env,
                    &nested_node_env,
                    next_node_id,
                )?;
                if let Some(param_value) = eval_scalar_binding_for_direct_occt(
                    &value,
                    param_names,
                    &nested_env,
                    &nested_node_env,
                )
                .map_err(|err| {
                    bk(AuthoringReason::Type, format!(
                        "Direct OCCT expander could not evaluate build binding `{}`: {err}",
                        binding.name
                    ))
                })? {
                    nested_env.insert(binding.name.clone(), param_value.clone());
                    nested_node_env.insert(binding.value.id.raw(), param_value.clone());
                    nested_node_env.insert(value.id.raw(), param_value);
                    record_scalar_node_values_for_direct_occt(
                        &value,
                        param_names,
                        &nested_env,
                        &mut nested_node_env,
                    );
                }
                expanded_bindings.push(CoreShapeBinding {
                    name: binding.name.clone(),
                    value,
                });
            }
            Ok(rebuild_node(
                node,
                CoreNodeKind::Build {
                    bindings: expanded_bindings,
                    result: Box::new(expand_node_for_direct_occt(
                        result,
                        param_names,
                        &nested_env,
                        &nested_node_env,
                        next_node_id,
                    )?),
                },
            ))
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested_env = env.clone();
            let mut nested_node_env = node_env.clone();
            let mut expanded_bindings = Vec::with_capacity(bindings.len());
            for binding in bindings {
                let value = expand_node_for_direct_occt(
                    &binding.value,
                    param_names,
                    &nested_env,
                    &nested_node_env,
                    next_node_id,
                )?;
                if let Some(param_value) = eval_scalar_binding_for_direct_occt(
                    &value,
                    param_names,
                    &nested_env,
                    &nested_node_env,
                )
                .map_err(|err| {
                    bk(AuthoringReason::Type, format!(
                        "Direct OCCT expander could not evaluate let binding `{}`: {err}",
                        binding.name
                    ))
                })? {
                    nested_env.insert(binding.name.clone(), param_value.clone());
                    nested_node_env.insert(binding.value.id.raw(), param_value.clone());
                    nested_node_env.insert(value.id.raw(), param_value);
                    record_scalar_node_values_for_direct_occt(
                        &value,
                        param_names,
                        &nested_env,
                        &mut nested_node_env,
                    );
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
                        &nested_node_env,
                        next_node_id,
                    )?),
                },
            ))
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let expanded_condition =
                expand_node_for_direct_occt(condition, param_names, env, node_env, next_node_id)?;
            match eval_bool_for_direct_occt(&expanded_condition, param_names, env, node_env) {
                Ok(true) => expand_node_for_direct_occt(
                    then_branch,
                    param_names,
                    env,
                    node_env,
                    next_node_id,
                ),
                Ok(false) => expand_node_for_direct_occt(
                    else_branch,
                    param_names,
                    env,
                    node_env,
                    next_node_id,
                ),
                Err(_) => Ok(rebuild_node(
                    node,
                    CoreNodeKind::If {
                        condition: Box::new(expanded_condition),
                        then_branch: Box::new(expand_node_for_direct_occt(
                            then_branch,
                            param_names,
                            env,
                            node_env,
                            next_node_id,
                        )?),
                        else_branch: Box::new(expand_node_for_direct_occt(
                            else_branch,
                            param_names,
                            env,
                            node_env,
                            next_node_id,
                        )?),
                    },
                )),
            }
        }
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
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(CoreBooleanOp::Xor),
            args,
            keywords,
        } if keywords.is_empty() => {
            expand_xor_node(node, args, param_names, env, node_env, next_node_id)
        }
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Text),
            args,
            keywords,
        } => expand_text_node(node, args, keywords, param_names, env, next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Svg),
            args,
            keywords,
        } if !keywords.is_empty() => Err(bk_op(AuthoringReason::Unsupported, "svg",
            "`svg` does not support keyword arguments yet in Direct OCCT adapter.",
        )),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Svg),
            args,
            ..
        } => expand_svg_node(node, args, param_names, env, next_node_id),
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "helical-ridge") => {
            expand_helical_ridge_node(
                node,
                args,
                keywords,
                param_names,
                env,
                node_env,
                next_node_id,
            )
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "thread") => {
            expand_thread_node(node, args, keywords, param_names, env, node_env, next_node_id)
        }
        CoreNodeKind::Call { op, args, .. } if matches!(op, CoreOperation::Custom(name) if name == "rib" || name == "groove") => {
            let is_rib = matches!(op, CoreOperation::Custom(name) if name == "rib");
            expand_rib_groove_node(node, is_rib, args, param_names, env, node_env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "sampled-radial-loft") => {
            expand_sampled_radial_loft_node(node, args, keywords, param_names, env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "regular-polygon") => {
            expand_regular_polygon_node(node, args, keywords, param_names, env, node_env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "trapezoid") => {
            expand_trapezoid_node(node, args, keywords, param_names, env, node_env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "slot-center-to-center" || name == "slot_center_to_center") => {
            expand_slot_center_to_center_node(node, args, keywords, param_names, env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "slot-center-point" || name == "slot_center_point") => {
            expand_slot_center_point_node(node, args, keywords, param_names, env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } => Ok(rebuild_node(
            node,
            CoreNodeKind::Call {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| {
                        expand_node_for_direct_occt(arg, param_names, env, node_env, next_node_id)
                    })
                    .collect::<AppResult<Vec<_>>>()?,
                keywords: keywords
                    .iter()
                    .map(|keyword| {
                        let value = expand_node_for_direct_occt(
                            keyword.source_node(),
                            param_names,
                            env,
                            node_env,
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
                    node_env,
                    next_node_id,
                )?),
                end: Box::new(expand_node_for_direct_occt(
                    end,
                    param_names,
                    env,
                    node_env,
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
                        expand_node_for_direct_occt(
                            source,
                            param_names,
                            env,
                            node_env,
                            next_node_id,
                        )
                    })
                    .collect::<AppResult<Vec<_>>>()?,
                body: Box::new(clone_node_with_fresh_ids(body, next_node_id)),
            },
        )),
        CoreNodeKind::Apply { op, args, list } => Ok(rebuild_node(
            node,
            CoreNodeKind::Apply {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| {
                        expand_node_for_direct_occt(arg, param_names, env, node_env, next_node_id)
                    })
                    .collect::<AppResult<Vec<_>>>()?,
                list: Box::new(expand_node_for_direct_occt(
                    list,
                    param_names,
                    env,
                    node_env,
                    next_node_id,
                )?),
            },
        )),
        CoreNodeKind::List(items) => Ok(rebuild_node(
            node,
            CoreNodeKind::List(
                items
                    .iter()
                    .map(|item| {
                        expand_node_for_direct_occt(item, param_names, env, node_env, next_node_id)
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            ),
        )),
        CoreNodeKind::Group(items) => Ok(rebuild_node(
            node,
            CoreNodeKind::Group(
                items
                    .iter()
                    .map(|item| {
                        expand_node_for_direct_occt(item, param_names, env, node_env, next_node_id)
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            ),
        )),
    }
}

fn expand_xor_node(
    node: &CoreNode,
    args: &[CoreNode],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if args.len() < 2 {
        return Err(bk_arity("xor", "at least two operands"));
    }

    let normalized_args = args
        .iter()
        .map(|arg| expand_node_for_direct_occt(arg, param_names, env, node_env, next_node_id))
        .collect::<AppResult<Vec<_>>>()?;

    let union_node = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(CoreBooleanOp::Union),
            args: normalized_args.clone(),
            keywords: Vec::new(),
        },
        node.value_kind,
    );
    let intersection_node = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(CoreBooleanOp::Intersection),
            args: normalized_args,
            keywords: Vec::new(),
        },
        node.value_kind,
    );

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(CoreBooleanOp::Difference),
            args: vec![union_node, intersection_node],
            keywords: Vec::new(),
        },
    ))
}

fn expand_svg_node(
    node: &CoreNode,
    args: &[CoreNode],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if args.is_empty() || args.len() > 4 {
        return Err(bk_arity("svg", "a file path, optional target width/height, and optional fit mode"));
    }

    let source = crate::ecky_ir::eval_core_stringish_with_locals(&args[0], param_names, env)?;
    let svg_text = if fs::metadata(&source).is_ok() {
        fs::read_to_string(&source).map_err(|err| {
            AppError::from(AuthoringError::surface(
                AuthoringReason::Type,
                format!("Direct OCCT adapter could not read SVG file `{source}`: {err}"),
            ))
        })?
    } else if source.trim_start().starts_with('<') {
        source
    } else {
        return Err(AuthoringError::surface(
            AuthoringReason::Type,
            format!("Direct OCCT adapter could not read SVG source at `{source}`."),
        ).into());
    };

    let target_width = args
        .get(1)
        .map(|arg| {
            crate::ecky_ir::eval_core_number_with_locals(arg, param_names, env).map_err(|err| {
                bk_op(AuthoringReason::Type, "svg", format!(
                    "Direct OCCT adapter could not evaluate `svg` width: {err}",
                ))
            })
        })
        .transpose()?;

    let target_height = args
        .get(2)
        .map(|arg| {
            crate::ecky_ir::eval_core_number_with_locals(arg, param_names, env).map_err(|err| {
                bk_op(AuthoringReason::Type, "svg", format!(
                    "Direct OCCT adapter could not evaluate `svg` height: {err}",
                ))
            })
        })
        .transpose()?;

    let fit_mode = args
        .get(3)
        .map(|arg| {
            let value = crate::ecky_ir::eval_core_stringish_with_locals(arg, param_names, env)?;
            value.parse::<SvgFitMode>().map_err(|()| {
                bk_constrained(
                    "svg",
                    format!("`svg` fit mode must be `contain`, `cover`, or `stretch`, got {value}"),
                    &["contain", "cover", "stretch"],
                )
            })
        })
        .transpose()?;

    let fit = fit_mode.unwrap_or(SvgFitMode::Contain);

    // Clean fast path: a single-outer, non-self-intersecting profile keeps its
    // exact current geometry. Artwork that the clean path rejects (self-
    // intersecting, multi-outer, even-odd) falls back to the tolerant wire soup
    // and lets OCCT resolve regions, mirroring build123d/ocpsvg.
    match parse_svg_profile(&svg_text, target_width, target_height, fit, true) {
        Ok(profile) => {
            let outer = vec![profile_contour_node(
                &profile.outer_loop,
                &profile.outer_geometry,
                next_node_id,
            )];
            let holes = profile
                .hole_loops
                .iter()
                .zip(profile.hole_geometries.iter())
                .map(|(points, geometry)| profile_contour_node(points, geometry, next_node_id))
                .collect::<Vec<_>>();
            // Same positional-vs-keyword split as text glyphs: executors reject
            // a positional outer mixed with a `:holes` keyword.
            let (args, keywords) = profile_components(outer, holes, next_node_id);

            Ok(rebuild_node(
                node,
                CoreNodeKind::Call {
                    op: CoreOperation::Primitive(CorePrimitive::Profile),
                    args,
                    keywords,
                },
            ))
        }
        Err(_) => {
            let soup =
                extract_svg_wire_soup_profile(&svg_text, target_width, target_height, fit)?;
            let wire_nodes = soup
                .wires
                .iter()
                .zip(soup.wire_geometries.iter())
                .map(|(points, geometry)| profile_contour_node(points, geometry, next_node_id))
                .collect::<Vec<_>>();
            let fill_rule = match soup.fill_rule {
                SvgFillRule::NonZero => "nonzero",
                SvgFillRule::EvenOdd => "evenodd",
            };
            let keywords = vec![
                CoreKeywordArg::expr(
                    "outer".to_string(),
                    CoreNode::new(
                        next_id(next_node_id),
                        CoreNodeKind::List(wire_nodes),
                        CoreValueKind::List,
                    ),
                ),
                CoreKeywordArg::expr(
                    "fill-rule".to_string(),
                    CoreNode::new(
                        next_id(next_node_id),
                        CoreNodeKind::Literal(CoreLiteral::Text(fill_rule.to_string())),
                        CoreValueKind::Text,
                    ),
                ),
            ];

            Ok(rebuild_node(
                node,
                CoreNodeKind::Call {
                    op: CoreOperation::Primitive(CorePrimitive::Profile),
                    args: Vec::new(),
                    keywords,
                },
            ))
        }
    }
}

fn expand_text_node(
    node: &CoreNode,
    args: &[CoreNode],
    _keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if args.len() < 2 {
        return Err(bk_arity("text", "text value and size"));
    }

    let value = crate::ecky_ir::eval_core_stringish_with_locals(&args[0], param_names, env)?;
    let size = crate::ecky_ir::eval_core_number_with_locals(&args[1], param_names, env)?;
    let components = parse_text_profile(&value, size, None)?;
    let outer_nodes = components
        .iter()
        .map(|component| {
            profile_contour_node(
                &component.outer_loop,
                &component.outer_geometry,
                next_node_id,
            )
        })
        .collect::<Vec<_>>();
    let hole_nodes = components
        .iter()
        .flat_map(|component| {
            component
                .hole_loops
                .iter()
                .zip(component.hole_geometries.iter())
        })
        .map(|(points, geometry)| profile_contour_node(points, geometry, next_node_id))
        .collect::<Vec<_>>();
    let (profile_args, profile_keywords) =
        profile_components(outer_nodes, hole_nodes, next_node_id);

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Profile),
            args: profile_args,
            keywords: profile_keywords,
        },
    ))
}

fn expand_helical_ridge_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if !args.is_empty() {
        return Err(bk_op(AuthoringReason::Unsupported, "helical-ridge",
            "`helical-ridge` expects keyword options only.",
        ));
    }
    reject_unknown_keywords(
        keywords,
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
        "helical-ridge",
    )?;

    let radius = positive_keyword_number(
        keywords,
        "radius",
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    let pitch = positive_keyword_number(
        keywords,
        "pitch",
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    let height = positive_keyword_number(
        keywords,
        "height",
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    let base_width = positive_keyword_number(
        keywords,
        "base-width",
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    let crest_width = positive_keyword_number(
        keywords,
        "crest-width",
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    let depth = positive_keyword_number(
        keywords,
        "depth",
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    let female = optional_keyword_bool(
        keywords,
        "female",
        false,
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    let lefthand = optional_keyword_bool(
        keywords,
        "lefthand",
        false,
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    let clearance = optional_keyword_number(
        keywords,
        "clearance",
        0.0,
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?
    .max(0.0);

    let envelope_clearance = if female { clearance } else { 0.0 };
    let base_half = (base_width + 2.0 * envelope_clearance) * 0.5;
    let crest_half = (crest_width + 2.0 * envelope_clearance) * 0.5;
    let ridge_depth = depth + envelope_clearance;
    // Profile trapezoid: wide base (`base_width`) at `radius`, narrow crest
    // (`crest_width`) at `radius + ridge_depth`. Must match the build123d
    // lowering profile exactly for backend parity (note the final point uses
    // `base_half`, not `crest_half`).
    let profile_wire = path3_node(
        &[
            [radius, 0.0, -base_half],
            [radius + ridge_depth, 0.0, -crest_half],
            [radius + ridge_depth, 0.0, crest_half],
            [radius, 0.0, base_half],
            [radius, 0.0, -base_half],
        ],
        next_node_id,
    );
    let profile = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::MakeFace),
            args: vec![profile_wire],
            keywords: Vec::new(),
        },
        CoreValueKind::Sketch,
    );
    // Sweep along a true helix (`helix-path` -> Geom_CylindricalSurface helix),
    // matching build123d's `Edge.make_helix`. A sampled polyline spine here
    // produced a faceted, gapped thread with the wrong apparent pitch.
    let lefthand_node = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Literal(CoreLiteral::Boolean(lefthand)),
        CoreValueKind::Boolean,
    );
    let radius_node = number_node(next_node_id, radius);
    let pitch_node = number_node(next_node_id, pitch);
    let height_node = number_node(next_node_id, height);
    let path = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Custom("helix-path".to_string()),
            args: vec![radius_node, pitch_node, height_node, lefthand_node],
            keywords: Vec::new(),
        },
        CoreValueKind::Path,
    );

    // A helical spine needs the Frenet trihedron (centripetal normal points at the
    // axis), keeping the trapezoid section radial. The generic-sweep default
    // (corrected-Frenet) banks the section, pulling the base off `radius` (the
    // observed thread defect). Declare the trihedron mode explicitly via `:frenet`.
    let frenet_kw = CoreKeywordArg::expr(
        "frenet".to_string(),
        CoreNode::new(
            next_id(next_node_id),
            CoreNodeKind::Literal(CoreLiteral::Boolean(true)),
            CoreValueKind::Boolean,
        ),
    );
    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Surface(CoreSurfaceOp::Sweep),
            args: vec![profile, path],
            keywords: vec![frenet_kw],
        },
    ))
}

fn expand_thread_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if !args.is_empty() {
        return Err(bk_op(AuthoringReason::Unsupported, "thread",
            "`thread` expects keyword options only.",
        ));
    }
    reject_unknown_keywords(
        keywords,
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
        "thread",
    )?;

    let length = positive_keyword_number(keywords, "length", "thread", param_names, env, node_env)?;
    let (radius, pitch, depth) = if let Some(designation) = keyword_text(keywords, "iso") {
        crate::ecky_core_ir::iso_metric_thread_core(&designation).ok_or_else(|| {
            bk_constrained(
                "thread",
                format!("`thread` unknown ISO designation `{designation}`"),
                &["M3", "M4", "M5", "M6", "M8", "M10", "M12", "M16", "M20"],
            )
        })?
    } else {
        (
            positive_keyword_number(keywords, "radius", "thread", param_names, env, node_env)?,
            positive_keyword_number(keywords, "pitch", "thread", param_names, env, node_env)?,
            positive_keyword_number(keywords, "depth", "thread", param_names, env, node_env)?,
        )
    };
    let base_width =
        optional_keyword_number(keywords, "base-width", pitch * 0.75, "thread", param_names, env, node_env)?;
    let crest_width =
        optional_keyword_number(keywords, "crest-width", pitch * 0.25, "thread", param_names, env, node_env)?;
    let female =
        optional_keyword_bool(keywords, "female", false, "thread", param_names, env, node_env)?;
    let lefthand =
        optional_keyword_bool(keywords, "lefthand", false, "thread", param_names, env, node_env)?;
    let clearance =
        optional_keyword_number(keywords, "clearance", 0.0, "thread", param_names, env, node_env)?.max(0.0);

    // Bury the ridge root inside the core by `overlap` so the `union(core, ridge)`
    // below never shares a coincident cylinder face. Without this the boolean
    // drops the core on coarse/deep threads, leaving a hollow spiral. The ridge
    // root moves in by `overlap` and its depth grows by `overlap`, so the crest
    // (major = radius + depth) and the core surface (minor = radius) are
    // unchanged — only the buried part of the ridge differs.
    let overlap = 0.3_f64.min(radius * 0.5).min(depth);
    let ridge_radius = radius - overlap;
    let ridge_depth = depth + overlap;

    // Compose: the canonical thread is the union of a core cylinder with a helical
    // ridge (or just the ridge cutter when female). Building from the existing
    // helical-ridge + cylinder ops keeps native and build123d identical by
    // construction (both already parity-matched).
    let bool_node = |next: &mut u64, value: bool| {
        CoreNode::new(
            next_id(next),
            CoreNodeKind::Literal(CoreLiteral::Boolean(value)),
            CoreValueKind::Boolean,
        )
    };
    let mut ridge_keywords = vec![
        CoreKeywordArg::expr("radius".to_string(), number_node(next_node_id, ridge_radius)),
        CoreKeywordArg::expr("pitch".to_string(), number_node(next_node_id, pitch)),
        CoreKeywordArg::expr("height".to_string(), number_node(next_node_id, length)),
        CoreKeywordArg::expr("base-width".to_string(), number_node(next_node_id, base_width)),
        CoreKeywordArg::expr("crest-width".to_string(), number_node(next_node_id, crest_width)),
        CoreKeywordArg::expr("depth".to_string(), number_node(next_node_id, ridge_depth)),
        CoreKeywordArg::expr("lefthand".to_string(), bool_node(next_node_id, lefthand)),
    ];
    if female {
        ridge_keywords.push(CoreKeywordArg::expr(
            "female".to_string(),
            bool_node(next_node_id, true),
        ));
        ridge_keywords.push(CoreKeywordArg::expr(
            "clearance".to_string(),
            number_node(next_node_id, clearance),
        ));
    }
    let mut ridge = expand_helical_ridge_node(
        node,
        &[],
        &ridge_keywords,
        param_names,
        env,
        node_env,
        next_node_id,
    )?;

    if female {
        return Ok(ridge);
    }

    // `expand_helical_ridge_node` rebuilds onto `node`'s id; the male thread's
    // `union` below also rebuilds onto `node`'s id. Re-id the ridge so the two
    // get distinct slots — otherwise the executor emits `fuse(ridge, ridge)`
    // against a redefined variable (the runner path never exercised this).
    ridge.id = next_id(next_node_id);

    let core = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Cylinder),
            args: vec![
                number_node(next_node_id, radius),
                number_node(next_node_id, length),
            ],
            keywords: Vec::new(),
        },
        CoreValueKind::Solid,
    );

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(CoreBooleanOp::Union),
            args: vec![core, ridge],
            keywords: Vec::new(),
        },
    ))
}

fn expand_rib_groove_node(
    node: &CoreNode,
    is_rib: bool,
    args: &[CoreNode],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    let op_name = if is_rib { "rib" } else { "groove" };
    if args.len() != 3 {
        return Err(bk_arity(op_name, "a solid, a profile, and a path"));
    }
    let solid = expand_node_for_direct_occt(&args[0], param_names, env, node_env, next_node_id)?;
    let profile = expand_node_for_direct_occt(&args[1], param_names, env, node_env, next_node_id)?;
    let path = expand_node_for_direct_occt(&args[2], param_names, env, node_env, next_node_id)?;
    // A rib/groove is a profile swept along a path, then fused (rib) or cut (groove).
    let swept = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Surface(CoreSurfaceOp::Sweep),
            args: vec![profile, path],
            keywords: Vec::new(),
        },
        CoreValueKind::Solid,
    );
    let bool_op = if is_rib {
        CoreBooleanOp::Union
    } else {
        CoreBooleanOp::Difference
    };
    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(bool_op),
            args: vec![solid, swept],
            keywords: Vec::new(),
        },
    ))
}

fn expand_regular_polygon_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if args.len() != 2 {
        return Err(bk_arity("regular-polygon", "sides and radius (plus optional `:rotation`)"));
    }
    reject_unknown_keywords(keywords, &["rotation"], "regular-polygon")?;

    let sides = crate::ecky_ir::eval_core_number_with_locals(&args[0], param_names, env)?;
    let radius = crate::ecky_ir::eval_core_number_with_locals(&args[1], param_names, env)?;
    let rotation = optional_keyword_number(
        keywords,
        "rotation",
        0.0,
        "regular-polygon",
        param_names,
        env,
        node_env,
    )?;

    let sides = sides.round();
    if sides < 3.0 {
        return Err(bk_constrained("regular-polygon",
            "`regular-polygon` needs at least 3 sides.",
            &["3", "4", "5", "6", "8"],
        ));
    }
    if !(radius > 0.0) {
        return Err(bk_op(AuthoringReason::Type, "regular-polygon",
            "`regular-polygon` radius must be positive.",
        ));
    }

    let points = crate::ecky_core_ir::regular_polygon_vertices(sides as u32, radius, rotation);
    let point_nodes = points
        .iter()
        .map(|point| {
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::Literal(CoreLiteral::Point2(*point)),
                CoreValueKind::Point2,
            )
        })
        .collect::<Vec<_>>();
    let list = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::List(point_nodes),
        CoreValueKind::List,
    );

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Polygon),
            args: vec![list],
            keywords: Vec::new(),
        },
    ))
}

fn expand_trapezoid_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if args.len() != 3 {
        return Err(bk_arity("trapezoid", "bottom, top, and height (plus optional `:skew`)"));
    }
    reject_unknown_keywords(keywords, &["skew"], "trapezoid")?;

    let bottom = crate::ecky_ir::eval_core_number_with_locals(&args[0], param_names, env)?;
    let top = crate::ecky_ir::eval_core_number_with_locals(&args[1], param_names, env)?;
    let height = crate::ecky_ir::eval_core_number_with_locals(&args[2], param_names, env)?;
    let skew = optional_keyword_number(
        keywords,
        "skew",
        0.0,
        "trapezoid",
        param_names,
        env,
        node_env,
    )?;

    if !(bottom > 0.0) || !(top > 0.0) {
        return Err(bk_op(AuthoringReason::Type, "trapezoid",
            "`trapezoid` bottom and top must be positive.",
        ));
    }
    if !(height > 0.0) {
        return Err(bk_op(AuthoringReason::Type, "trapezoid",
            "`trapezoid` height must be positive.",
        ));
    }

    let points = crate::ecky_core_ir::trapezoid_vertices(bottom, top, height, skew);
    let point_nodes = points
        .iter()
        .map(|point| {
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::Literal(CoreLiteral::Point2(*point)),
                CoreValueKind::Point2,
            )
        })
        .collect::<Vec<_>>();
    let list = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::List(point_nodes),
        CoreValueKind::List,
    );

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Polygon),
            args: vec![list],
            keywords: Vec::new(),
        },
    ))
}

fn expand_slot_center_to_center_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if args.len() != 2 {
        return Err(bk_arity("slot-center-to-center", "center separation and width"));
    }
    reject_unknown_keywords(keywords, &[], "slot-center-to-center")?;

    let separation = crate::ecky_ir::eval_core_number_with_locals(&args[0], param_names, env)?;
    let width = crate::ecky_ir::eval_core_number_with_locals(&args[1], param_names, env)?;
    if !(width > 0.0) {
        return Err(bk_op(AuthoringReason::Type, "slot-center-to-center",
            "`slot-center-to-center` width must be positive.",
        ));
    }
    if !(separation >= 0.0) {
        return Err(bk_op(AuthoringReason::Type, "slot-center-to-center",
            "`slot-center-to-center` separation must be non-negative.",
        ));
    }

    let length = separation + width;
    let length_node = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Literal(CoreLiteral::Number(length)),
        CoreValueKind::Number,
    );
    let width_node = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Literal(CoreLiteral::Number(width)),
        CoreValueKind::Number,
    );

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Slot),
            args: vec![length_node, width_node],
            keywords: Vec::new(),
        },
    ))
}

fn expand_slot_center_point_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AppResult<CoreNode> {
    if args.len() != 5 {
        return Err(bk_arity("slot-center-point", "cx, cy, px, py, width"));
    }
    reject_unknown_keywords(keywords, &[], "slot-center-point")?;

    let cx = crate::ecky_ir::eval_core_number_with_locals(&args[0], param_names, env)?;
    let cy = crate::ecky_ir::eval_core_number_with_locals(&args[1], param_names, env)?;
    let px = crate::ecky_ir::eval_core_number_with_locals(&args[2], param_names, env)?;
    let py = crate::ecky_ir::eval_core_number_with_locals(&args[3], param_names, env)?;
    let width = crate::ecky_ir::eval_core_number_with_locals(&args[4], param_names, env)?;
    if !(width > 0.0) {
        return Err(bk_op(AuthoringReason::Type, "slot-center-point",
            "`slot-center-point` width must be positive.",
        ));
    }

    let d = (px - cx).hypot(py - cy);
    let length = 2.0 * d + width;
    let angle_deg = (py - cy).atan2(px - cx).to_degrees();

    let slot = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Slot),
            args: vec![
                number_node(next_node_id, length),
                number_node(next_node_id, width),
            ],
            keywords: Vec::new(),
        },
        CoreValueKind::Sketch,
    );
    let rotated = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Transform(CoreTransformOp::Rotate),
            args: vec![
                number_node(next_node_id, 0.0),
                number_node(next_node_id, 0.0),
                number_node(next_node_id, angle_deg),
                slot,
            ],
            keywords: Vec::new(),
        },
        CoreValueKind::Sketch,
    );

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Transform(CoreTransformOp::Translate),
            args: vec![
                number_node(next_node_id, cx),
                number_node(next_node_id, cy),
                number_node(next_node_id, 0.0),
                rotated,
            ],
            keywords: Vec::new(),
        },
    ))
}

/// Emit one profile loop as its exact geometry (ocpsvg/build123d parity):
/// contours with curves become a `bezier-path` wire of consecutive cubics
/// (lines encoded as exact degree-3 segments), pure-line contours keep the
/// flattened `polygon` plan unchanged.
pub(crate) fn profile_contour_node(
    points: &[[f64; 2]],
    geometry: &crate::ecky_cad_host::svg_profile::SvgContourGeometry,
    next_node_id: &mut u64,
) -> CoreNode {
    use crate::ecky_cad_host::svg_profile::SvgPathSegment;

    if !geometry.has_curves() || geometry.segments.is_empty() {
        return profile_polygon_node(points, next_node_id);
    }

    let cubic_third = |from: [f64; 2], to: [f64; 2]| -> ([f64; 2], [f64; 2]) {
        (
            [
                from[0] + (to[0] - from[0]) / 3.0,
                from[1] + (to[1] - from[1]) / 3.0,
            ],
            [
                from[0] + 2.0 * (to[0] - from[0]) / 3.0,
                from[1] + 2.0 * (to[1] - from[1]) / 3.0,
            ],
        )
    };

    let mut controls: Vec<[f64; 3]> = vec![[geometry.start[0], geometry.start[1], 0.0]];
    let mut cursor = geometry.start;
    let near = |a: [f64; 2], b: [f64; 2]| -> bool {
        (a[0] - b[0]).abs() <= 1.0e-9 && (a[1] - b[1]).abs() <= 1.0e-9
    };
    for segment in &geometry.segments {
        let (c1, c2, to) = match segment {
            SvgPathSegment::Line { to } => {
                let (c1, c2) = cubic_third(cursor, *to);
                (c1, c2, *to)
            }
            SvgPathSegment::Cubic { c1, c2, to } => (*c1, *c2, *to),
        };
        // Degenerate (zero-extent) segments produce degenerate OCCT edges;
        // drop them the way the flattened path's point dedupe used to.
        if near(to, cursor) && near(c1, cursor) && near(c2, cursor) {
            continue;
        }
        controls.push([c1[0], c1[1], 0.0]);
        controls.push([c2[0], c2[1], 0.0]);
        controls.push([to[0], to[1], 0.0]);
        cursor = to;
    }
    if controls.len() < 4 {
        return profile_polygon_node(points, next_node_id);
    }
    // Profile loops must be closed wires. A near-coincident endpoint (float
    // noise from the SVG/font parser) must SNAP onto the start — emitting a
    // micro closing segment instead creates a degenerate edge that corrupts
    // meshing and booleans (non-manifold shells, swallowed fuses). Only a
    // genuinely open contour gets a real closing line.
    let gap = ((cursor[0] - geometry.start[0]).powi(2) + (cursor[1] - geometry.start[1]).powi(2))
        .sqrt();
    if gap <= 1.0e-6 {
        let last = controls.last_mut().expect("closing endpoint");
        *last = [geometry.start[0], geometry.start[1], 0.0];
    } else {
        let (c1, c2) = cubic_third(cursor, geometry.start);
        controls.push([c1[0], c1[1], 0.0]);
        controls.push([c2[0], c2[1], 0.0]);
        controls.push([geometry.start[0], geometry.start[1], 0.0]);
    }

    let point_nodes = controls
        .iter()
        .map(|point| {
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::Literal(CoreLiteral::Point3(*point)),
                CoreValueKind::Point3,
            )
        })
        .collect::<Vec<_>>();
    let list = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::List(point_nodes),
        CoreValueKind::List,
    );
    CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Path(CorePathOp::BezierPath),
            args: vec![list],
            keywords: Vec::new(),
        },
        CoreValueKind::Path,
    )
}

fn profile_polygon_node(points: &[[f64; 2]], next_node_id: &mut u64) -> CoreNode {
    let point_nodes = points
        .iter()
        .map(|point| {
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::Literal(CoreLiteral::Point2(*point)),
                CoreValueKind::Point2,
            )
        })
        .collect::<Vec<_>>();

    let list = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::List(point_nodes),
        CoreValueKind::List,
    );

    CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Polygon),
            args: vec![list],
            keywords: Vec::new(),
        },
        CoreValueKind::Sketch,
    )
}

pub(crate) fn profile_components(
    outer_nodes: Vec<CoreNode>,
    hole_nodes: Vec<CoreNode>,
    next_node_id: &mut u64,
) -> (Vec<CoreNode>, Vec<CoreKeywordArg>) {
    if hole_nodes.is_empty() && outer_nodes.len() == 1 {
        return (outer_nodes, Vec::new());
    }

    let mut keywords = vec![CoreKeywordArg::expr(
        "outer".to_string(),
        CoreNode::new(
            next_id(next_node_id),
            CoreNodeKind::List(outer_nodes),
            CoreValueKind::List,
        ),
    )];
    if !hole_nodes.is_empty() {
        keywords.push(CoreKeywordArg::expr(
            "holes".to_string(),
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::List(hole_nodes),
                CoreValueKind::List,
            ),
        ));
    }
    (Vec::new(), keywords)
}

fn path3_node(points: &[[f64; 3]], next_node_id: &mut u64) -> CoreNode {
    let point_nodes = points
        .iter()
        .map(|point| {
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::Literal(CoreLiteral::Point3(*point)),
                CoreValueKind::Point3,
            )
        })
        .collect::<Vec<_>>();

    let list = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::List(point_nodes),
        CoreValueKind::List,
    );

    CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Path(CorePathOp::Polyline),
            args: vec![list],
            keywords: Vec::new(),
        },
        CoreValueKind::Path,
    )
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
        return Err(bk_arity("sampled-radial-loft", "binder names plus keyword/value options"));
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
                return Err(bk_op(AuthoringReason::Type, "sampled-radial-loft",
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
        return Err(bk_arity("shell", "thickness and shape only (sampled-radial-loft)"));
    }
    let target = sampled_radial_loft_target(args).ok_or_else(|| {
        bk_op(AuthoringReason::Type, "shell",
            "`shell` sampled-radial-loft requires a sampled-radial-loft target.",
        )
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
                return Err(bk_arity("sampled-radial-loft", "binders `(theta z fz)`"));
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
        _ => Err(bk_arity("sampled-radial-loft", "binders `(theta z fz)`")),
    }
}

fn sampled_binder_name(node: &CoreNode) -> AppResult<String> {
    match &node.kind {
        CoreNodeKind::Reference(CoreReference::Local(name)) => Ok(name.clone()),
        CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(text.clone()),
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => Ok(symbol_name(symbol).to_string()),
        _ => Err(bk_op(AuthoringReason::Type, "sampled-radial-loft",
            "`sampled-radial-loft` binders must be symbols.",
        )),
    }
}

fn sampled_keyword_node<'a>(keywords: &'a [CoreKeywordArg], name: &str) -> AppResult<&'a CoreNode> {
    sampled_optional_keyword_node(keywords, name)
        .ok_or_else(|| bk_op(AuthoringReason::Arity, "sampled-radial-loft",
            format!("`sampled-radial-loft` requires `:{name}`."),
        ))
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
        return Err(bk_op(AuthoringReason::Type, "sampled-radial-loft",
            format!("`sampled-radial-loft` {label} must be finite."),
        ));
    }
    Ok((value.round() as isize).max(minimum as isize) as usize)
}

fn eval_scalar_binding_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AppResult<Option<ParamValue>> {
    match node.value_kind {
        CoreValueKind::Number => Ok(Some(ParamValue::Number(eval_number_for_direct_occt(
            node,
            param_names,
            env,
            node_env,
        )?))),
        CoreValueKind::Boolean => Ok(Some(ParamValue::Boolean(eval_bool_for_direct_occt(
            node,
            param_names,
            env,
            node_env,
        )?))),
        CoreValueKind::Text => Ok(Some(ParamValue::String(eval_stringish_for_direct_occt(
            node,
            param_names,
            env,
            node_env,
        )?))),
        CoreValueKind::Any => {
            if let Ok(number) = eval_number_for_direct_occt(node, param_names, env, node_env) {
                Ok(Some(ParamValue::Number(number)))
            } else if let Ok(flag) = eval_bool_for_direct_occt(node, param_names, env, node_env) {
                Ok(Some(ParamValue::Boolean(flag)))
            } else if let Ok(text) =
                eval_stringish_for_direct_occt(node, param_names, env, node_env)
            {
                Ok(Some(ParamValue::String(text)))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

fn record_scalar_node_values_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &mut BTreeMap<u64, ParamValue>,
) {
    let snapshot = node_env.clone();
    if let Ok(Some(value)) = eval_scalar_binding_for_direct_occt(node, param_names, env, &snapshot)
    {
        node_env.insert(node.id.raw(), value);
    }

    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => {}
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings {
                record_scalar_node_values_for_direct_occt(
                    &binding.value,
                    param_names,
                    env,
                    node_env,
                );
            }
            record_scalar_node_values_for_direct_occt(result, param_names, env, node_env);
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                record_scalar_node_values_for_direct_occt(
                    &binding.value,
                    param_names,
                    env,
                    node_env,
                );
            }
            record_scalar_node_values_for_direct_occt(body, param_names, env, node_env);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            record_scalar_node_values_for_direct_occt(condition, param_names, env, node_env);
            record_scalar_node_values_for_direct_occt(then_branch, param_names, env, node_env);
            record_scalar_node_values_for_direct_occt(else_branch, param_names, env, node_env);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                record_scalar_node_values_for_direct_occt(arg, param_names, env, node_env);
            }
            for keyword in keywords {
                record_scalar_node_values_for_direct_occt(
                    keyword.source_node(),
                    param_names,
                    env,
                    node_env,
                );
            }
        }
        CoreNodeKind::Range { start, end } => {
            record_scalar_node_values_for_direct_occt(start, param_names, env, node_env);
            record_scalar_node_values_for_direct_occt(end, param_names, env, node_env);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                record_scalar_node_values_for_direct_occt(source, param_names, env, node_env);
            }
            record_scalar_node_values_for_direct_occt(body, param_names, env, node_env);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                record_scalar_node_values_for_direct_occt(arg, param_names, env, node_env);
            }
            record_scalar_node_values_for_direct_occt(list, param_names, env, node_env);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                record_scalar_node_values_for_direct_occt(item, param_names, env, node_env);
            }
        }
    }
}

fn eval_number_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AppResult<f64> {
    let node = rewrite_eval_node_for_direct_occt(node, env, node_env);
    crate::ecky_ir::eval_core_number_with_locals(&node, param_names, env).map_err(|err| {
        bk(AuthoringReason::Type, format!(
            "could not evaluate numeric Core node {:?}: {err}",
            node.id
        ))
    })
}

fn eval_bool_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AppResult<bool> {
    let node = rewrite_eval_node_for_direct_occt(node, env, node_env);
    crate::ecky_ir::eval_core_bool_with_locals(&node, param_names, env)
}

fn eval_stringish_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AppResult<String> {
    let node = rewrite_eval_node_for_direct_occt(node, env, node_env);
    crate::ecky_ir::eval_core_stringish_with_locals(&node, param_names, env)
}

fn rewrite_eval_node_for_direct_occt(
    node: &CoreNode,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> CoreNode {
    let node = super::direct_occt_normalize::rewrite_local_aliases_for_eval(node, env);
    rewrite_scalar_node_refs_for_eval(&node, node_env)
}

fn rewrite_scalar_node_refs_for_eval(
    node: &CoreNode,
    node_env: &BTreeMap<u64, ParamValue>,
) -> CoreNode {
    match &node.kind {
        CoreNodeKind::Reference(crate::ecky_core_ir::CoreReference::Node(id)) => {
            if let Some(value) = node_env.get(&id.raw()) {
                return param_value_node_with_id(node.id, value, node.span);
            }
            node.clone()
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => node.clone(),
        CoreNodeKind::Build { bindings, result } => rebuild_node(
            node,
            CoreNodeKind::Build {
                bindings: bindings
                    .iter()
                    .map(|binding| CoreShapeBinding {
                        name: binding.name.clone(),
                        value: rewrite_scalar_node_refs_for_eval(&binding.value, node_env),
                    })
                    .collect(),
                result: Box::new(rewrite_scalar_node_refs_for_eval(result, node_env)),
            },
        ),
        CoreNodeKind::Let { bindings, body } => rebuild_node(
            node,
            CoreNodeKind::Let {
                bindings: bindings
                    .iter()
                    .map(|binding| CoreBinding {
                        name: binding.name.clone(),
                        value: rewrite_scalar_node_refs_for_eval(&binding.value, node_env),
                    })
                    .collect(),
                body: Box::new(rewrite_scalar_node_refs_for_eval(body, node_env)),
            },
        ),
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => rebuild_node(
            node,
            CoreNodeKind::If {
                condition: Box::new(rewrite_scalar_node_refs_for_eval(condition, node_env)),
                then_branch: Box::new(rewrite_scalar_node_refs_for_eval(then_branch, node_env)),
                else_branch: Box::new(rewrite_scalar_node_refs_for_eval(else_branch, node_env)),
            },
        ),
        CoreNodeKind::Call { op, args, keywords } => rebuild_node(
            node,
            CoreNodeKind::Call {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| rewrite_scalar_node_refs_for_eval(arg, node_env))
                    .collect(),
                keywords: keywords
                    .iter()
                    .map(|keyword| match keyword.selector_payload() {
                        Some(selector) => CoreKeywordArg::selector(
                            keyword.name.clone(),
                            rewrite_scalar_node_refs_for_eval(keyword.source_node(), node_env),
                            selector.clone(),
                        ),
                        None => CoreKeywordArg::expr(
                            keyword.name.clone(),
                            rewrite_scalar_node_refs_for_eval(keyword.source_node(), node_env),
                        ),
                    })
                    .collect(),
            },
        ),
        CoreNodeKind::Range { start, end } => rebuild_node(
            node,
            CoreNodeKind::Range {
                start: Box::new(rewrite_scalar_node_refs_for_eval(start, node_env)),
                end: Box::new(rewrite_scalar_node_refs_for_eval(end, node_env)),
            },
        ),
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => rebuild_node(
            node,
            CoreNodeKind::Map {
                params: params.clone(),
                sources: sources
                    .iter()
                    .map(|source| rewrite_scalar_node_refs_for_eval(source, node_env))
                    .collect(),
                body: Box::new(rewrite_scalar_node_refs_for_eval(body, node_env)),
            },
        ),
        CoreNodeKind::Apply { op, args, list } => rebuild_node(
            node,
            CoreNodeKind::Apply {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| rewrite_scalar_node_refs_for_eval(arg, node_env))
                    .collect(),
                list: Box::new(rewrite_scalar_node_refs_for_eval(list, node_env)),
            },
        ),
        CoreNodeKind::List(items) => rebuild_node(
            node,
            CoreNodeKind::List(
                items
                    .iter()
                    .map(|item| rewrite_scalar_node_refs_for_eval(item, node_env))
                    .collect(),
            ),
        ),
        CoreNodeKind::Group(items) => rebuild_node(
            node,
            CoreNodeKind::Group(
                items
                    .iter()
                    .map(|item| rewrite_scalar_node_refs_for_eval(item, node_env))
                    .collect(),
            ),
        ),
    }
}

fn param_value_node_with_id(
    id: crate::ecky_core_ir::NodeId,
    value: &ParamValue,
    span: Option<crate::ecky_core_ir::SourceSpan>,
) -> CoreNode {
    match value {
        ParamValue::Number(number) => CoreNode {
            id,
            kind: CoreNodeKind::Literal(CoreLiteral::Number(*number)),
            value_kind: CoreValueKind::Number,
            span,
        },
        ParamValue::Boolean(flag) => CoreNode {
            id,
            kind: CoreNodeKind::Literal(CoreLiteral::Boolean(*flag)),
            value_kind: CoreValueKind::Boolean,
            span,
        },
        ParamValue::String(text) => CoreNode {
            id,
            kind: CoreNodeKind::Literal(CoreLiteral::Text(text.clone())),
            value_kind: CoreValueKind::Text,
            span,
        },
        ParamValue::Null => CoreNode {
            id,
            kind: CoreNodeKind::Literal(CoreLiteral::Text(String::new())),
            value_kind: CoreValueKind::Text,
            span,
        },
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

fn required_keyword_node<'a>(
    keywords: &'a [CoreKeywordArg],
    name: &str,
    op: &str,
) -> AppResult<&'a CoreNode> {
    keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .map(|keyword| keyword.source_node())
        .ok_or_else(|| bk_op(AuthoringReason::Arity, op, format!("`{op}` requires `:{name}`.")))
}

fn positive_keyword_number(
    keywords: &[CoreKeywordArg],
    name: &str,
    op: &str,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AppResult<f64> {
    let value = eval_number_for_direct_occt(
        required_keyword_node(keywords, name, op)?,
        param_names,
        env,
        node_env,
    )?;
    if !value.is_finite() || value <= 0.0 {
        return Err(bk_op(AuthoringReason::Type, op, format!(
            "`{op}` {name} must be positive and finite."
        )));
    }
    Ok(value)
}

fn optional_keyword_number(
    keywords: &[CoreKeywordArg],
    name: &str,
    default: f64,
    op: &str,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AppResult<f64> {
    let Some(node) = keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .map(|keyword| keyword.source_node())
    else {
        return Ok(default);
    };
    eval_number_for_direct_occt(node, param_names, env, node_env)
        .map_err(|err| bk_op(AuthoringReason::Type, op, format!("`{op}` could not evaluate `:{name}`: {err}")))
}

fn optional_keyword_bool(
    keywords: &[CoreKeywordArg],
    name: &str,
    default: bool,
    op: &str,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AppResult<bool> {
    let Some(node) = keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .map(|keyword| keyword.source_node())
    else {
        return Ok(default);
    };
    eval_bool_for_direct_occt(node, param_names, env, node_env)
        .map_err(|err| bk_op(AuthoringReason::Type, op, format!("`{op}` could not evaluate `:{name}`: {err}")))
}

fn reject_unknown_keywords(
    keywords: &[CoreKeywordArg],
    allowed: &[&str],
    op: &str,
) -> AppResult<()> {
    for keyword in keywords {
        if allowed
            .iter()
            .any(|allowed_name| *allowed_name == keyword.name)
        {
            continue;
        }
        return Err(bk_op(AuthoringReason::Arity, op, format!(
            "`{op}` does not recognize `:{}`.",
            keyword.name
        )));
    }
    Ok(())
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

fn clone_node_with_fresh_ids(node: &CoreNode, next_node_id: &mut u64) -> CoreNode {
    CoreNode {
        id: next_id(next_node_id),
        kind: match &node.kind {
            CoreNodeKind::Literal(literal) => CoreNodeKind::Literal(literal.clone()),
            CoreNodeKind::Reference(reference) => CoreNodeKind::Reference(reference.clone()),
            CoreNodeKind::Build { bindings, result } => CoreNodeKind::Build {
                bindings: bindings
                    .iter()
                    .map(|binding| CoreShapeBinding {
                        name: binding.name.clone(),
                        value: clone_node_with_fresh_ids(&binding.value, next_node_id),
                    })
                    .collect(),
                result: Box::new(clone_node_with_fresh_ids(result, next_node_id)),
            },
            CoreNodeKind::Let { bindings, body } => CoreNodeKind::Let {
                bindings: bindings
                    .iter()
                    .map(|binding| CoreBinding {
                        name: binding.name.clone(),
                        value: clone_node_with_fresh_ids(&binding.value, next_node_id),
                    })
                    .collect(),
                body: Box::new(clone_node_with_fresh_ids(body, next_node_id)),
            },
            CoreNodeKind::If {
                condition,
                then_branch,
                else_branch,
            } => CoreNodeKind::If {
                condition: Box::new(clone_node_with_fresh_ids(condition, next_node_id)),
                then_branch: Box::new(clone_node_with_fresh_ids(then_branch, next_node_id)),
                else_branch: Box::new(clone_node_with_fresh_ids(else_branch, next_node_id)),
            },
            CoreNodeKind::Call { op, args, keywords } => CoreNodeKind::Call {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| clone_node_with_fresh_ids(arg, next_node_id))
                    .collect(),
                keywords: keywords
                    .iter()
                    .map(|keyword| match keyword.selector_payload() {
                        Some(selector) => CoreKeywordArg::selector(
                            keyword.name.clone(),
                            clone_node_with_fresh_ids(keyword.source_node(), next_node_id),
                            selector.clone(),
                        ),
                        None => CoreKeywordArg::expr(
                            keyword.name.clone(),
                            clone_node_with_fresh_ids(keyword.source_node(), next_node_id),
                        ),
                    })
                    .collect(),
            },
            CoreNodeKind::Range { start, end } => CoreNodeKind::Range {
                start: Box::new(clone_node_with_fresh_ids(start, next_node_id)),
                end: Box::new(clone_node_with_fresh_ids(end, next_node_id)),
            },
            CoreNodeKind::Map {
                params,
                sources,
                body,
            } => CoreNodeKind::Map {
                params: params.clone(),
                sources: sources
                    .iter()
                    .map(|source| clone_node_with_fresh_ids(source, next_node_id))
                    .collect(),
                body: Box::new(clone_node_with_fresh_ids(body, next_node_id)),
            },
            CoreNodeKind::Apply { op, args, list } => CoreNodeKind::Apply {
                op: op.clone(),
                args: args
                    .iter()
                    .map(|arg| clone_node_with_fresh_ids(arg, next_node_id))
                    .collect(),
                list: Box::new(clone_node_with_fresh_ids(list, next_node_id)),
            },
            CoreNodeKind::List(items) => CoreNodeKind::List(
                items
                    .iter()
                    .map(|item| clone_node_with_fresh_ids(item, next_node_id))
                    .collect(),
            ),
            CoreNodeKind::Group(items) => CoreNodeKind::Group(
                items
                    .iter()
                    .map(|item| clone_node_with_fresh_ids(item, next_node_id))
                    .collect(),
            ),
        },
        value_kind: node.value_kind,
        span: node.span,
    }
}

fn next_id(next_node_id: &mut u64) -> NodeId {
    let id = *next_node_id;
    *next_node_id += 1;
    NodeId::new(id)
}

struct PartPlanner<'a> {
    param_names: &'a BTreeMap<u64, String>,
    scalar_env: BTreeMap<String, ParamValue>,
    scalar_node_values: BTreeMap<u64, OcctArg>,
    node_refs: BTreeMap<u64, OcctSlot>,
    locals: BTreeMap<String, OcctArg>,
    next_node_id: u64,
    commands: Vec<OcctCommand>,
}

impl<'a> PartPlanner<'a> {
    fn new(
        param_names: &'a BTreeMap<u64, String>,
        scalar_env: &'a BTreeMap<String, ParamValue>,
        next_node_id: u64,
    ) -> Self {
        Self {
            param_names,
            scalar_env: scalar_env.clone(),
            scalar_node_values: BTreeMap::new(),
            node_refs: BTreeMap::new(),
            locals: BTreeMap::new(),
            next_node_id,
            commands: Vec::new(),
        }
    }

    fn scalar_env_snapshot(&self) -> BTreeMap<String, ParamValue> {
        let mut env = self.scalar_env.clone();
        for (name, arg) in &self.locals {
            if let Some(value) = occt_arg_to_scalar(arg) {
                env.insert(name.clone(), value);
            }
        }
        env
    }

    fn scalar_param_node_values(&self) -> BTreeMap<u64, ParamValue> {
        self.scalar_node_values
            .iter()
            .filter_map(|(id, arg)| occt_arg_to_scalar(arg).map(|value| (*id, value)))
            .collect()
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
                        let value = if keyword.name == "align" {
                            self.plan_align_arg(keyword.source_node())?
                        } else if let Some(selector) = keyword.selector_payload() {
                            self.plan_arg(keyword.source_node())
                                .unwrap_or_else(|_| selector_source_placeholder_arg(selector))
                        } else {
                            self.plan_arg(keyword.source_node())?
                        };
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
            CoreNodeKind::Apply { op, args, list } => self.plan_apply(op, args, list, node)?,
            CoreNodeKind::If { .. } => {
                return Err(unsupported(
                    "if",
                    "branching Core IR needs runtime selection before direct OCCT planning",
                ));
            }
            CoreNodeKind::Reference(_) => match self.plan_arg(node)? {
                OcctArg::Ref(slot) => slot,
                other => {
                    return Err(bk(AuthoringReason::Type, format!(
                        "Direct OCCT adapter expected geometry reference, got {:?}.",
                        other
                    )));
                }
            },
            _ => {
                return Err(bk(AuthoringReason::Type, format!(
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
            let value = self.plan_arg(&binding.value)?;
            if let Some(scalar) = occt_arg_to_scalar(&value) {
                self.scalar_env.insert(binding.name.clone(), scalar);
                self.scalar_node_values
                    .insert(binding.value.id.raw(), value.clone());
            }
            self.locals.insert(binding.name.clone(), value.clone());
            if let OcctArg::Ref(slot) = value {
                self.node_refs.insert(binding.value.id.raw(), slot);
            }
        }
        let root = self.plan_node(result);
        self.locals = saved_locals;
        root
    }

    fn plan_let(&mut self, bindings: &[CoreBinding], body: &CoreNode) -> AppResult<OcctSlot> {
        let saved_locals = self.locals.clone();
        for binding in bindings {
            let value = self.plan_arg(&binding.value)?;
            if let Some(scalar) = occt_arg_to_scalar(&value) {
                self.scalar_env.insert(binding.name.clone(), scalar);
                self.scalar_node_values
                    .insert(binding.value.id.raw(), value.clone());
            }
            self.locals.insert(binding.name.clone(), value);
        }
        let root = self.plan_node(body);
        self.locals = saved_locals;
        root
    }

    fn plan_apply(
        &mut self,
        op: &CoreOperation,
        args: &[CoreNode],
        list: &CoreNode,
        node: &CoreNode,
    ) -> AppResult<OcctSlot> {
        let output = OcctSlot(node.id.raw());
        let mut planned_args = args
            .iter()
            .map(|arg| self.plan_arg(arg))
            .collect::<AppResult<Vec<_>>>()?;
        let list_arg = self.plan_arg(list)?;
        let OcctArg::List(items) = list_arg else {
            return Err(bk(AuthoringReason::Type, format!(
                "Direct OCCT adapter `apply` expected list argument, got {:?}.",
                list_arg
            )));
        };
        planned_args.extend(items);
        self.commands.push(OcctCommand {
            output,
            op: occt_op(op)?,
            args: planned_args,
            keywords: Vec::new(),
        });
        Ok(output)
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
                    bk(AuthoringReason::Type, format!(
                        "Direct OCCT adapter could not resolve parameter {:?}.",
                        id
                    ))
                })?;
                Ok(OcctArg::Param(name))
            }
            CoreNodeKind::Reference(CoreReference::Node(id)) => {
                if let Some(value) = self.scalar_node_values.get(&id.raw()).cloned() {
                    return Ok(value);
                }
                let slot = self.node_refs.get(&id.raw()).copied().ok_or_else(|| {
                    bk(AuthoringReason::Type, format!(
                        "Direct OCCT adapter could not resolve Core node reference {:?}.",
                        id
                    ))
                })?;
                Ok(OcctArg::Ref(slot))
            }
            CoreNodeKind::Reference(CoreReference::Local(name)) => {
                self.locals.get(name).cloned().ok_or_else(|| {
                    bk(AuthoringReason::Type, format!(
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
            CoreNodeKind::Range { start, end } => self.plan_range_arg(start, end),
            CoreNodeKind::Map {
                params,
                sources,
                body,
            } => self.plan_map_arg(params, sources, body),
            CoreNodeKind::Let { bindings, body } => self.plan_let_arg(bindings, body),
            CoreNodeKind::Build { bindings, result } => self.plan_build_arg(bindings, result),
            CoreNodeKind::Call {
                op: CoreOperation::Custom(name),
                args,
                ..
            } if name == "append" => {
                let mut combined = Vec::new();
                for arg in args {
                    match self.plan_arg(arg)? {
                        OcctArg::List(items) => combined.extend(items),
                        other => {
                            return Err(bk(AuthoringReason::Type, format!(
                                "Direct OCCT adapter `append` expected list argument, got {:?}.",
                                other
                            )))
                        }
                    }
                }
                Ok(OcctArg::List(combined))
            }
            CoreNodeKind::Call {
                op: CoreOperation::Custom(name),
                args,
                ..
            } if name == "reverse" => {
                let [arg] = args.as_slice() else {
                    return Err(bk(AuthoringReason::Arity, format!(
                        "Direct OCCT adapter `reverse` expected one list, got {} arguments.",
                        args.len()
                    )));
                };
                match self.plan_arg(arg)? {
                    OcctArg::List(mut items) => {
                        items.reverse();
                        Ok(OcctArg::List(items))
                    }
                    other => Err(bk(AuthoringReason::Type, format!(
                        "Direct OCCT adapter `reverse` expected list argument, got {:?}.",
                        other
                    ))),
                }
            }
            CoreNodeKind::Call {
                op: CoreOperation::Custom(name),
                args,
                ..
            } if matches!(name.as_str(), "car" | "first" | "cadr" | "second" | "third") => {
                let index = match name.as_str() {
                    "car" | "first" => 0,
                    "cadr" | "second" => 1,
                    _ => 2,
                };
                let [arg] = args.as_slice() else {
                    return Err(bk(AuthoringReason::Arity, format!(
                        "Direct OCCT adapter `{name}` expected one list, got {} arguments.",
                        args.len()
                    )));
                };
                let items = match self.plan_arg(arg)? {
                    OcctArg::List(items) => items,
                    OcctArg::Point2(point) => {
                        point.iter().copied().map(OcctArg::Number).collect()
                    }
                    OcctArg::Point3(point) => {
                        point.iter().copied().map(OcctArg::Number).collect()
                    }
                    other => {
                        return Err(bk(AuthoringReason::Type, format!(
                            "Direct OCCT adapter `{name}` expected list argument, got {:?}.",
                            other
                        )))
                    }
                };
                items.get(index).cloned().ok_or_else(|| {
                    bk(AuthoringReason::Arity, format!(
                        "Direct OCCT adapter `{name}` expected at least {} item(s), got {}.",
                        index + 1,
                        items.len()
                    ))
                })
            }
            CoreNodeKind::Call { .. } | CoreNodeKind::Apply { .. } => {
                // Arithmetic over list accessors (`(- (cadr p))` in a map
                // body) cannot reach the shared scalar evaluator: resolve the
                // accessor subnodes to literals first.
                let substituted;
                let node = if node_contains_list_accessor(node) {
                    substituted = self.substitute_list_accessors(node)?;
                    &substituted
                } else {
                    node
                };
                if let Some(scalar) = self.plan_scalar_arg(node)? {
                    return Ok(scalar);
                }
                let slot = self.plan_node(node)?;
                Ok(OcctArg::Ref(slot))
            }
            CoreNodeKind::If { .. } => Err(bk(AuthoringReason::Unsupported, format!(
                "Direct OCCT adapter cannot plan dynamic expression node {:?} before evaluation.",
                node.kind
            ))),
            CoreNodeKind::Reference(CoreReference::Part(id)) => Err(bk(AuthoringReason::Unsupported, format!(
                "Direct OCCT adapter cannot plan part reference {:?} in first surface.",
                id
            ))),
        }
    }

    /// Clone `node` with list-accessor calls (`car`, `cadr`, ...) replaced by
    /// literal scalars resolved against planned locals, so the shared scalar
    /// evaluator can fold the surrounding arithmetic.
    fn substitute_list_accessors(&mut self, node: &CoreNode) -> AppResult<CoreNode> {
        if let CoreNodeKind::Call {
            op: CoreOperation::Custom(name),
            ..
        } = &node.kind
        {
            if is_list_accessor_name(name) {
                let literal = match self.plan_arg(node)? {
                    OcctArg::Number(value) => CoreLiteral::Number(value),
                    OcctArg::Boolean(flag) => CoreLiteral::Boolean(flag),
                    OcctArg::Text(text) => CoreLiteral::Text(text),
                    _ => return Ok(node.clone()),
                };
                let mut resolved = node.clone();
                resolved.kind = CoreNodeKind::Literal(literal);
                return Ok(resolved);
            }
        }
        let mut resolved = node.clone();
        if let CoreNodeKind::Call { args, keywords, .. } = &mut resolved.kind {
            for arg in args.iter_mut() {
                *arg = self.substitute_list_accessors(arg)?;
            }
            for keyword in keywords.iter_mut() {
                let value = self.substitute_list_accessors(keyword.source_node())?;
                *keyword = match keyword.selector_payload() {
                    Some(selector) => {
                        CoreKeywordArg::selector(keyword.name.clone(), value, selector.clone())
                    }
                    None => CoreKeywordArg::expr(keyword.name.clone(), value),
                };
            }
        }
        Ok(resolved)
    }

    fn plan_align_arg(&mut self, node: &CoreNode) -> AppResult<OcctArg> {
        let symbols = match &node.kind {
            CoreNodeKind::List(items) | CoreNodeKind::Group(items) => items
                .iter()
                .map(align_axis_arg)
                .collect::<AppResult<Vec<_>>>()?,
            CoreNodeKind::Call {
                op: CoreOperation::Custom(head),
                args,
                keywords,
            } if keywords.is_empty() => {
                let mut symbols = Vec::with_capacity(args.len() + 1);
                symbols.push(align_axis_name(head)?);
                for arg in args {
                    symbols.push(align_axis_arg(arg)?);
                }
                symbols
            }
            _ => {
                return Err(bk_constrained("align",
                    "Direct OCCT adapter `:align` expects `(min|center|max)^3`.",
                    &["min", "center", "max"],
                ));
            }
        };
        if symbols.len() != 3 {
            return Err(bk_arity("align", "exactly 3 axes"));
        }
        Ok(OcctArg::List(
            symbols
                .into_iter()
                .map(|symbol| OcctArg::Symbol(symbol.to_string()))
                .collect(),
        ))
    }

    fn plan_scalar_arg(&mut self, node: &CoreNode) -> AppResult<Option<OcctArg>> {
        let env = self.scalar_env_snapshot();
        let node_env = self.scalar_param_node_values();
        Ok(match node.value_kind {
            CoreValueKind::Number => Some(OcctArg::Number(eval_number_for_direct_occt(
                node,
                self.param_names,
                &env,
                &node_env,
            )?)),
            CoreValueKind::Boolean => Some(OcctArg::Boolean(eval_bool_for_direct_occt(
                node,
                self.param_names,
                &env,
                &node_env,
            )?)),
            CoreValueKind::Text => Some(OcctArg::Text(eval_stringish_for_direct_occt(
                node,
                self.param_names,
                &env,
                &node_env,
            )?)),
            CoreValueKind::Any => {
                if let Ok(number) =
                    eval_number_for_direct_occt(node, self.param_names, &env, &node_env)
                {
                    Some(OcctArg::Number(number))
                } else if let Ok(flag) =
                    eval_bool_for_direct_occt(node, self.param_names, &env, &node_env)
                {
                    Some(OcctArg::Boolean(flag))
                } else if let Ok(text) =
                    eval_stringish_for_direct_occt(node, self.param_names, &env, &node_env)
                {
                    Some(OcctArg::Text(text))
                } else {
                    None
                }
            }
            _ => None,
        })
    }

    fn plan_range_arg(&mut self, start: &CoreNode, end: &CoreNode) -> AppResult<OcctArg> {
        let env = self.scalar_env_snapshot();
        let node_env = self.scalar_param_node_values();
        let start = eval_number_for_direct_occt(start, self.param_names, &env, &node_env)?;
        let end = eval_number_for_direct_occt(end, self.param_names, &env, &node_env)?;
        let start = start.floor() as i64;
        let end = end.floor() as i64;
        let items = if start <= end {
            (start..end)
                .map(|value| OcctArg::Number(value as f64))
                .collect()
        } else {
            (end + 1..=start)
                .rev()
                .map(|value| OcctArg::Number(value as f64))
                .collect()
        };
        Ok(OcctArg::List(items))
    }

    fn plan_map_arg(
        &mut self,
        params: &[String],
        sources: &[CoreNode],
        body: &CoreNode,
    ) -> AppResult<OcctArg> {
        if params.len() != sources.len() {
            return Err(bk(AuthoringReason::Arity, format!(
                "Direct OCCT adapter map expected {} source list(s), got {}.",
                params.len(),
                sources.len()
            )));
        }
        let source_values = sources
            .iter()
            .map(|source| match self.plan_arg(source)? {
                OcctArg::List(items) => Ok(items),
                other => Err(bk(AuthoringReason::Type, format!(
                    "Direct OCCT adapter map expected list source, got {:?}.",
                    other
                ))),
            })
            .collect::<AppResult<Vec<_>>>()?;
        let Some(first_source) = source_values.first() else {
            return Ok(OcctArg::List(Vec::new()));
        };
        let count = first_source.len();
        if source_values.iter().any(|source| source.len() != count) {
            return Err(bk(AuthoringReason::Type,
                "Direct OCCT adapter map source lists must have matching lengths.",
            ));
        }

        let saved_locals = self.locals.clone();
        let mut items = Vec::with_capacity(count);
        let result = (|| {
            for index in 0..count {
                self.locals = saved_locals.clone();
                for (param, source) in params.iter().zip(source_values.iter()) {
                    self.locals.insert(param.clone(), source[index].clone());
                }
                let iteration_body = clone_node_with_fresh_ids(body, &mut self.next_node_id);
                items.push(self.plan_arg(&iteration_body)?);
            }
            Ok(OcctArg::List(items))
        })();
        self.locals = saved_locals;
        result
    }

    fn plan_let_arg(&mut self, bindings: &[CoreBinding], body: &CoreNode) -> AppResult<OcctArg> {
        let saved_locals = self.locals.clone();
        let saved_scalar_env = self.scalar_env.clone();
        let saved_scalar_node_values = self.scalar_node_values.clone();
        for binding in bindings {
            let value = self.plan_arg(&binding.value)?;
            if let Some(scalar) = occt_arg_to_scalar(&value) {
                self.scalar_env.insert(binding.name.clone(), scalar);
                self.scalar_node_values
                    .insert(binding.value.id.raw(), value.clone());
            }
            self.locals.insert(binding.name.clone(), value.clone());
            if let OcctArg::Ref(slot) = value {
                self.node_refs.insert(binding.value.id.raw(), slot);
            }
        }
        let result = self.plan_arg(body);
        self.locals = saved_locals;
        self.scalar_env = saved_scalar_env;
        self.scalar_node_values = saved_scalar_node_values;
        result
    }

    fn plan_build_arg(
        &mut self,
        bindings: &[CoreShapeBinding],
        result: &CoreNode,
    ) -> AppResult<OcctArg> {
        let saved_locals = self.locals.clone();
        let saved_scalar_env = self.scalar_env.clone();
        let saved_scalar_node_values = self.scalar_node_values.clone();
        for binding in bindings {
            let value = self.plan_arg(&binding.value)?;
            if let Some(scalar) = occt_arg_to_scalar(&value) {
                self.scalar_env.insert(binding.name.clone(), scalar);
                self.scalar_node_values
                    .insert(binding.value.id.raw(), value.clone());
            }
            self.locals.insert(binding.name.clone(), value.clone());
            if let OcctArg::Ref(slot) = value {
                self.node_refs.insert(binding.value.id.raw(), slot);
            }
        }
        let planned = self.plan_arg(result);
        self.locals = saved_locals;
        self.scalar_env = saved_scalar_env;
        self.scalar_node_values = saved_scalar_node_values;
        planned
    }
}

fn is_list_accessor_name(name: &str) -> bool {
    matches!(name, "car" | "first" | "cadr" | "second" | "third")
}

fn node_contains_list_accessor(node: &CoreNode) -> bool {
    match &node.kind {
        CoreNodeKind::Call { op, args, keywords } => {
            matches!(op, CoreOperation::Custom(name) if is_list_accessor_name(name))
                || args.iter().any(node_contains_list_accessor)
                || keywords
                    .iter()
                    .any(|keyword| node_contains_list_accessor(keyword.source_node()))
        }
        _ => false,
    }
}

fn occt_arg_to_scalar(arg: &OcctArg) -> Option<ParamValue> {
    match arg {
        OcctArg::Number(value) => Some(ParamValue::Number(*value)),
        OcctArg::Boolean(flag) => Some(ParamValue::Boolean(*flag)),
        OcctArg::Text(text) => Some(ParamValue::String(text.clone())),
        _ => None,
    }
}

fn align_axis_arg(node: &CoreNode) -> AppResult<&'static str> {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => Ok(symbol_name(symbol)),
        CoreNodeKind::Call {
            op: CoreOperation::Custom(name),
            args,
            keywords,
        } if args.is_empty() && keywords.is_empty() => align_axis_name(name),
        _ => Err(bk_constrained("align",
            "Direct OCCT adapter `:align` axes must be `min`, `center`, or `max`.",
            &["min", "center", "max"],
        )),
    }
}

fn align_axis_name(name: &str) -> AppResult<&'static str> {
    match name {
        "min" => Ok("min"),
        "center" => Ok("center"),
        "max" => Ok("max"),
        _ => Err(bk_constrained("align",
            format!("Direct OCCT adapter `:align` axis `{name}` is not supported."),
            &["min", "center", "max"],
        )),
    }
}

fn occt_op(op: &CoreOperation) -> AppResult<OcctOp> {
    match op {
        CoreOperation::Primitive(CorePrimitive::Box) => Ok(OcctOp::Box),
        CoreOperation::Primitive(CorePrimitive::Sphere) => Ok(OcctOp::Sphere),
        CoreOperation::Primitive(CorePrimitive::Cylinder) => Ok(OcctOp::Cylinder),
        CoreOperation::Primitive(CorePrimitive::Cone) => Ok(OcctOp::Cone),
        CoreOperation::Primitive(CorePrimitive::Torus) => Ok(OcctOp::Torus),
        CoreOperation::Primitive(CorePrimitive::Wedge) => Ok(OcctOp::Wedge),
        CoreOperation::Primitive(CorePrimitive::Ellipse) => Ok(OcctOp::Ellipse),
        CoreOperation::Primitive(CorePrimitive::Slot) => Ok(OcctOp::Slot),
        CoreOperation::Primitive(CorePrimitive::SlotArc) => Ok(OcctOp::SlotArc),
        CoreOperation::Primitive(CorePrimitive::Circle) => Ok(OcctOp::Circle),
        CoreOperation::Primitive(CorePrimitive::Rectangle) => Ok(OcctOp::Rectangle),
        CoreOperation::Primitive(CorePrimitive::RoundedRectangle) => Ok(OcctOp::RoundedRectangle),
        CoreOperation::Primitive(CorePrimitive::RoundedPolygon) => Ok(OcctOp::RoundedPolygon),
        CoreOperation::Primitive(CorePrimitive::Polygon) => Ok(OcctOp::Polygon),
        CoreOperation::Primitive(CorePrimitive::Profile) => Ok(OcctOp::Profile),
        CoreOperation::Primitive(CorePrimitive::MakeFace) => Ok(OcctOp::MakeFace),
        CoreOperation::Primitive(CorePrimitive::Stl) => Ok(OcctOp::ImportStl),
        CoreOperation::Surface(CoreSurfaceOp::Extrude) => Ok(OcctOp::Extrude),
        CoreOperation::Surface(CoreSurfaceOp::Revolve) => Ok(OcctOp::Revolve),
        CoreOperation::Surface(CoreSurfaceOp::Loft) => Ok(OcctOp::Loft),
        CoreOperation::Surface(CoreSurfaceOp::Sweep) => Ok(OcctOp::Sweep),
        CoreOperation::Surface(CoreSurfaceOp::Twist) => Ok(OcctOp::Twist),
        CoreOperation::Surface(CoreSurfaceOp::Draft) => Ok(OcctOp::Draft),
        CoreOperation::Surface(CoreSurfaceOp::Taper) => Ok(OcctOp::Taper),
        CoreOperation::Surface(CoreSurfaceOp::Offset) => Ok(OcctOp::Offset),
        CoreOperation::Surface(CoreSurfaceOp::OffsetRounded) => Ok(OcctOp::Offset),
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => Ok(OcctOp::Fillet),
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => Ok(OcctOp::Chamfer),
        CoreOperation::Surface(CoreSurfaceOp::Shell) => Ok(OcctOp::Shell),
        CoreOperation::Path(CorePathOp::Polyline) => Ok(OcctOp::Path),
        CoreOperation::Custom(name) if name == "helix-path" => Ok(OcctOp::HelixPath),
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
        CoreOperation::Custom(name) if name == "hull" => Ok(OcctOp::Hull),
        CoreOperation::Custom(name) if name == "hole" => Err(bk_op(AuthoringReason::Unsupported, "hole",
            "Typed hole must be filled before direct OCCT planning.",
        )),
        _ => Err(unsupported(&operation_name(op), "not in first surface")),
    }
}

fn typed_hole_error(keywords: &[CoreKeywordArg]) -> AppError {
    let requested_type = keyword_text(keywords, "type").unwrap_or_else(|| "unknown".to_string());
    let goal = keyword_text(keywords, "goal").unwrap_or_else(|| "unspecified".to_string());
    bk_op(AuthoringReason::Unsupported, "hole", format!(
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
    crate::contracts::AuthoringError::backend(
        crate::contracts::AuthoringReason::Unsupported,
        format!("The active backend (direct OCCT) cannot execute `{op}`: {reason}."),
    )
    .with_op(op)
    .with_fix(crate::contracts::ErrorFix {
        hint: Some(
            "switch to a backend that supports this operation, or replace it with an \
             equivalent the current backend can render."
                .into(),
        ),
        suggestions: Vec::new(),
    })
    .into()
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
        CoreOperation::Primitive(CorePrimitive::Torus) => "torus",
        CoreOperation::Primitive(CorePrimitive::Wedge) => "wedge",
        CoreOperation::Primitive(CorePrimitive::Ellipse) => "ellipse",
        CoreOperation::Primitive(CorePrimitive::Slot) => "slot-overall",
        CoreOperation::Primitive(CorePrimitive::SlotArc) => "slot-arc",
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
        CoreOperation::Surface(CoreSurfaceOp::Draft) => "draft",
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

fn selector_source_placeholder_arg(selector: &CoreSelectorPayload) -> OcctArg {
    match selector {
        CoreSelectorPayload::EdgeAll => OcctArg::Text("all".to_string()),
        CoreSelectorPayload::EdgeTargetIds(target_ids)
        | CoreSelectorPayload::FaceTargetIds(target_ids) => OcctArg::Text(
            target_ids
                .first()
                .cloned()
                .unwrap_or_else(|| "selector".to_string()),
        ),
        CoreSelectorPayload::EdgeTag(tag_name) | CoreSelectorPayload::FaceTag(tag_name) => {
            OcctArg::Text(format!("tag:{tag_name}"))
        }
        CoreSelectorPayload::EdgeClauses(_) | CoreSelectorPayload::FaceClauses(_) => {
            OcctArg::Text("selector".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_core_ir::{
        CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CorePart, CorePrimitive, CoreProgram,
        CoreSelectorPayload, CoreSurfaceOp, CoreValueKind, NodeId, PartId, ProgramId,
    };
    use std::io::Write;

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
    fn plans_scalar_build_bindings_with_arithmetic_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape x (/ 10 2))
                  (result (box x 2 2)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(plan.parts[0].commands.len(), 1);
        assert_eq!(plan.parts[0].commands[0].op, OcctOp::Box);
        assert_eq!(
            plan.parts[0].commands[0].args,
            vec![
                OcctArg::Number(5.0),
                OcctArg::Number(2.0),
                OcctArg::Number(2.0)
            ]
        );
    }

    #[test]
    fn plans_scalar_build_bindings_referencing_prior_shape_scalars() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape frame_w 84)
                  (shape extra 4)
                  (shape holder_w (+ frame_w extra))
                  (result (box holder_w 2 2)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(plan.parts[0].commands.len(), 1);
        assert_eq!(plan.parts[0].commands[0].op, OcctOp::Box);
        assert_eq!(
            plan.parts[0].commands[0].args,
            vec![
                OcctArg::Number(88.0),
                OcctArg::Number(2.0),
                OcctArg::Number(2.0)
            ]
        );
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
    fn plans_created_by_keyword_into_direct_occt_slot_reference() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape blank (box 10 10 10))
                  (shape pocket (box 4 4 4))
                  (shape solid (difference blank pocket))
                  (result
                    (fillet 0.5
                      :edges "left+vertical"
                      :created-by pocket
                      solid)))))
            "#,
        );
        let plan = plan_core_program(&program).expect("plan");
        let pocket_slot = plan.parts[0]
            .commands
            .iter()
            .find(|command| {
                command.op == OcctOp::Box
                    && command.args
                        == vec![
                            OcctArg::Number(4.0),
                            OcctArg::Number(4.0),
                            OcctArg::Number(4.0),
                        ]
            })
            .map(|command| command.output)
            .expect("pocket slot");
        let fillet = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Fillet)
            .expect("fillet");
        assert_eq!(fillet.keywords.len(), 2);
        assert_eq!(fillet.keywords[0].name, "edges");
        assert_eq!(fillet.keywords[1].name, "created-by");
        assert_eq!(fillet.keywords[1].source_arg(), &OcctArg::Ref(pocket_slot));
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
    fn plans_tagged_face_selector_payload_into_direct_occt_keywords() {
        let program = compile(
            r#"
            (model
              (tag-face mounting_top :faces "top" body)
              (part body
                (shell 0.8
                  :faces (tag mounting_top)
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
                "tag:mounting_top".into()
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
    fn plans_torus_primitive_for_direct_occt() {
        let program = compile("(model (part body (torus 10 3)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(plan.parts[0].commands.len(), 1);
        assert_eq!(plan.parts[0].commands[0].op, OcctOp::Torus);
        assert_eq!(
            plan.parts[0].commands[0].args[..2],
            [OcctArg::Number(10.0), OcctArg::Number(3.0)]
        );
    }

    #[test]
    fn live_draft_matches_build123d_reference() {
        // T10.9: draft was the only OcctOp with no precompiled-runner
        // dispatch at all (generated-source-only). Proves the new runner
        // `draft_shape` produces geometry matching build123d within the
        // shared differential tolerance, routed through whichever path
        // runner-first selects.
        crate::ecky_cad_host::native_parity_harness::assert_native_matches_reference(
            "(model (part body (draft 10 (box 20 20 20))))",
            &DesignParams::new(),
            "draft-op",
            crate::ecky_cad_host::native_parity_harness::ParityReference::Build123d,
        );
    }

    #[test]
    fn live_torus_matches_build123d_reference() {
        // Proves the parity harness (language-convenience-stdlib 5.1) is
        // reusable outside direct_occt_executor.rs: torus parity was
        // previously only checked by hand on a live render.
        crate::ecky_cad_host::native_parity_harness::assert_native_matches_reference(
            "(model (part body (torus 10 3)))",
            &DesignParams::new(),
            "torus-primitive",
            crate::ecky_cad_host::native_parity_harness::ParityReference::Build123d,
        );
    }

    #[test]
    fn plans_slot_overall_primitive_for_direct_occt() {
        let program = compile("(model (part body (extrude (slot-overall 40 10) 5)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Slot, OcctOp::Extrude]
        );
        assert_eq!(
            plan.parts[0].commands[0].args[..2],
            [OcctArg::Number(40.0), OcctArg::Number(10.0)]
        );
    }

    #[test]
    fn plans_slot_center_to_center_as_slot_for_direct_occt() {
        // Custom op expands to the canonical Slot primitive with length = sep + width.
        let program = compile("(model (part body (extrude (slot-center-to-center 30 10) 5)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(plan.parts[0].commands[0].op, OcctOp::Slot);
        assert_eq!(
            plan.parts[0].commands[0].args[..2],
            [OcctArg::Number(40.0), OcctArg::Number(10.0)]
        );
    }

    #[test]
    fn plans_rib_and_groove_as_sweep_booleans_for_direct_occt() {
        let rib = plan_core_program(&compile(
            "(model (part p (rib (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))))",
        ))
        .expect("rib plan");
        let rib_ops: Vec<_> = rib.parts[0].commands.iter().map(|c| c.op).collect();
        assert!(
            rib_ops.contains(&OcctOp::Union) && rib_ops.contains(&OcctOp::Sweep),
            "rib should be union(solid, sweep), got {rib_ops:?}"
        );

        let groove = plan_core_program(&compile(
            "(model (part p (groove (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))))",
        ))
        .expect("groove plan");
        let groove_ops: Vec<_> = groove.parts[0].commands.iter().map(|c| c.op).collect();
        assert!(
            groove_ops.contains(&OcctOp::Difference) && groove_ops.contains(&OcctOp::Sweep),
            "groove should be difference(solid, sweep), got {groove_ops:?}"
        );
    }

    #[test]
    fn plans_draft_as_draft_op_for_direct_occt() {
        let program = compile("(model (part p (draft 10 (box 20 20 20))))");

        let plan = plan_core_program(&program).expect("plan");

        let ops: Vec<_> = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect();
        assert!(
            ops.contains(&OcctOp::Draft) && ops.contains(&OcctOp::Box),
            "expected draft over a box, got {ops:?}"
        );
    }

    #[test]
    fn plans_thread_as_union_of_cylinder_and_ridge_for_direct_occt() {
        let program =
            compile("(model (part screw (thread :radius 8 :pitch 2 :length 16 :depth 1)))");

        let plan = plan_core_program(&program).expect("plan");

        let ops: Vec<_> = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect();
        assert!(
            ops.contains(&OcctOp::Union) && ops.contains(&OcctOp::Cylinder),
            "expected thread to expand into union(cylinder, ridge), got {ops:?}"
        );
    }

    #[test]
    fn plans_female_thread_as_ridge_cutter_for_direct_occt() {
        let program = compile(
            "(model (part cut (thread :radius 8 :pitch 2 :length 16 :depth 1 :female #t :clearance 0.2)))",
        );

        let plan = plan_core_program(&program).expect("plan");

        let ops: Vec<_> = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect();
        assert!(
            !ops.contains(&OcctOp::Union) && !ops.contains(&OcctOp::Cylinder),
            "female thread should be a bare ridge cutter (no core cylinder/union), got {ops:?}"
        );
    }

    #[test]
    fn plans_slot_arc_primitive_for_direct_occt() {
        let program =
            compile("(model (part body (extrude (slot-arc 20 0 90 10) 5)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::SlotArc, OcctOp::Extrude]
        );
        assert_eq!(
            plan.parts[0].commands[0].args[..4],
            [
                OcctArg::Number(20.0),
                OcctArg::Number(0.0),
                OcctArg::Number(90.0),
                OcctArg::Number(10.0),
            ]
        );
    }

    #[test]
    fn plans_slot_center_point_as_transformed_slot_for_direct_occt() {
        // Custom op expands to Slot wrapped in rotate + translate.
        let program =
            compile("(model (part body (extrude (slot-center-point 0 0 15 0 10) 5)))");

        let plan = plan_core_program(&program).expect("plan");

        let ops: Vec<_> = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect();
        assert!(
            ops.contains(&OcctOp::Slot)
                && ops.contains(&OcctOp::Rotate)
                && ops.contains(&OcctOp::Translate),
            "expected slot+rotate+translate, got {ops:?}"
        );
    }

    #[test]
    fn plans_wedge_primitive_for_direct_occt() {
        let program = compile("(model (part body (wedge 20 10 20 5 5 15 15)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(plan.parts[0].commands.len(), 1);
        assert_eq!(plan.parts[0].commands[0].op, OcctOp::Wedge);
        assert_eq!(
            plan.parts[0].commands[0].args[..7],
            [
                OcctArg::Number(20.0),
                OcctArg::Number(10.0),
                OcctArg::Number(20.0),
                OcctArg::Number(5.0),
                OcctArg::Number(5.0),
                OcctArg::Number(15.0),
                OcctArg::Number(15.0),
            ]
        );
    }

    #[test]
    fn plans_ellipse_profile_for_direct_occt() {
        let program = compile("(model (part body (extrude (ellipse 10 4) 5)))");

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Ellipse, OcctOp::Extrude]
        );
        assert_eq!(
            plan.parts[0].commands[0].args[..2],
            [OcctArg::Number(10.0), OcctArg::Number(4.0)]
        );
    }

    #[test]
    fn plans_trapezoid_as_polygon_for_direct_occt() {
        // `trapezoid` is a Custom op that expands to a `polygon` of four vertices
        // computed by the shared `trapezoid_vertices` builder.
        let program = compile("(model (part body (extrude (trapezoid 20 10 8) 5)))");

        let plan = plan_core_program(&program).expect("trapezoid planned");

        let ops: Vec<_> = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect();
        assert_eq!(
            ops,
            vec![OcctOp::Polygon, OcctOp::Extrude],
            "expected trapezoid to expand into a polygon + extrude plan, got {ops:?}"
        );
    }

    #[test]
    fn plans_deferred_append_reverse_polygon_points() {
        // Param-dependent `map` lists composed with `append`/`reverse` cannot
        // be flattened at compile time; the planner must evaluate the deferred
        // calls into one concrete point list (16 arc + 2 fixed + 16 mirrored).
        let program = compile(
            r#"
            (model
              (params (number tube_od 22) (number wall 2.4) (number clip_gap 2.2))
              (let* ((or (/ tube_od 2))
                     (ir (- or wall))
                     (cr (+ or wall))
                     (step-a (* 0.5 3.14159265))
                     (n-pts-a 16)
                     (arc-a (map (lambda (i)
                       (let* ((t (/ i n-pts-a))
                              (a (+ step-a (* t (- 1.5707963 step-a)))))
                         (list (* ir (cos a)) (* ir (sin a)))))
                       (range n-pts-a)))
                     (ox-end (list (* cr (cos step-a)) (* cr (sin step-a))))
                     (path (append arc-a (list ox-end (list (- clip_gap) ir))
                                   (reverse (map (lambda (p) (list (car p) (- (cadr p)))) arc-a)))))
                (part clip (extrude (polygon path) 2))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        let polygon = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Polygon)
            .expect("polygon command");
        let OcctArg::List(points) = &polygon.args[0] else {
            panic!("expected concrete point list, got {:?}", polygon.args[0]);
        };
        assert_eq!(points.len(), 34);
        // The tail comes from `reverse`: its first point must mirror (negate y)
        // the LAST arc point, not the first.
        let point_xy = |arg: &OcctArg| -> (f64, f64) {
            match arg {
                OcctArg::Point2(point) => (point[0], point[1]),
                OcctArg::List(items) => match items.as_slice() {
                    [OcctArg::Number(x), OcctArg::Number(y)] => (*x, *y),
                    other => panic!("expected 2 numbers, got {other:?}"),
                },
                other => panic!("expected point, got {other:?}"),
            }
        };
        let (first_arc_x, first_arc_y) = point_xy(&points[0]);
        let (last_arc_x, last_arc_y) = point_xy(&points[15]);
        let (first_tail_x, first_tail_y) = point_xy(&points[18]);
        let (last_tail_x, last_tail_y) = point_xy(&points[33]);
        assert!((first_tail_x - last_arc_x).abs() < 1e-9);
        assert!((first_tail_y + last_arc_y).abs() < 1e-9);
        assert!((last_tail_x - first_arc_x).abs() < 1e-9);
        assert!((last_tail_y + first_arc_y).abs() < 1e-9);
    }

    #[test]
    fn plans_nested_parameterized_component_for_direct_occt() {
        // G-COMP: a nested, parameterized component instantiated with a
        // param-driven override plans through the native Direct OCCT path.
        let program = compile(
            r#"
            (define-component rib
              ((number w 2) (number h 8))
              (box w 20 h))
            (define-component ribbed-wall
              ((number rib-h 8))
              (union
                (box 60 20 3)
                (repeat-union i 3
                  (translate (- (* i 20) 20) 0 3
                    (rib :h rib-h)))))
            (model
              (params (number rib_h 8))
              (part wall (ribbed-wall :rib-h rib_h)))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(plan.parts.len(), 1);
        assert_eq!(plan.parts[0].key, "wall");
        let boxes = plan.parts[0]
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Box)
            .count();
        assert!(
            boxes >= 4,
            "expected wall + 3 expanded rib instances, got {boxes} boxes"
        );
    }

    #[test]
    fn plans_full_tube_clip_freecad_migration_fixture() {
        // Real FreeCAD-migrated model: three param-dependent arcs composed
        // with `append`/`reverse`, unary `(/ x)` reciprocal, `car`/`cadr`
        // accessors. Must compile through the expanded path and plan natively.
        let program = compile(include_str!("../ecky_scheme/clip_full.ecky"));

        let plan = plan_core_program(&program).expect("plan");

        assert!(!plan.parts.is_empty());
        assert!(plan.parts[0]
            .commands
            .iter()
            .any(|command| command.op == OcctOp::Polygon));
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
    fn plans_mapped_bspline_points_for_direct_occt() {
        let program = compile(
            r#"
            (define control-points
              (map
                (lambda (angle)
                  (list
                    (* 26 (cos (* pi (/ angle 180.0))))
                    (* 16 (sin (* pi (/ angle 180.0))))))
                (linspace 0 315 8)))

            (model
              (part body
                (extrude (bspline control-points :closed #t) 10)))
            "#,
        );
        assert_eq!(program.parts.len(), 1, "{:?}", program.parts);
        let plan = plan_core_program(&program).expect("plan");
        let bspline = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Bspline)
            .expect("bspline command");
        assert!(matches!(bspline.args[0], OcctArg::List(_)));
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
    fn plans_hull_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (hull
                  (sphere 6)
                  (translate 30 0 0 (sphere 6)))))
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
                OcctOp::Sphere,
                OcctOp::Sphere,
                OcctOp::Translate,
                OcctOp::Hull,
            ]
        );
        let hull = plan.parts[0].commands.last().expect("hull");
        assert_eq!(hull.op, OcctOp::Hull);
        assert_eq!(hull.args.len(), 2);
        assert!(hull
            .args
            .iter()
            .all(|arg| matches!(arg, OcctArg::Ref(_))));
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
    fn plans_svg_profile_for_direct_occt_extrusion() {
        let svg_path = std::path::Path::new("/tmp/ecky-direct-occt-svg-profile.svg");
        {
            let mut file = std::fs::File::create(svg_path).expect("create svg");
            file.write_all(
                b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\">\n  <path d=\"M2 2h6v6h-6z\"/>\n</svg>\n",
            )
            .expect("write svg");
        }

        let program = compile(
            r#"(model (part body (extrude (svg "/tmp/ecky-direct-occt-svg-profile.svg" 10 10 "contain") 4)))"#,
        );

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::Polygon, OcctOp::Profile, OcctOp::Extrude]
        );

        assert!(std::fs::remove_file(svg_path).is_ok());
    }

    #[test]
    fn plans_folded_if_branch_referenced_by_build_result() {
        // Normalize folds a statically-known `if` to one branch. The branch must
        // keep the `if` node's id, or every `RefNode(if_id)` (e.g. the build
        // result referencing the `overlay` binding) dangles at plan time.
        let program = compile(
            r#"(model
              (params (number k 1))
              (part p (build
                (shape base (box 20 10 2))
                (shape empty_overlay (difference base base))
                (shape overlay (if (= k 0) empty_overlay (translate 0 0 1 (box 3 3 3))))
                (result (fuse base overlay)))))"#,
        );
        let plan = plan_core_program(&program).expect("folded-if plan");
        assert!(
            plan.parts[0].commands.iter().any(|c| c.op == OcctOp::Translate),
            "else branch survives fold"
        );

        // Then-branch case: the fold result is itself a reference to another
        // binding (`empty_overlay`); the aliased id must resolve too.
        let program = compile(
            r#"(model
              (params (number k 0))
              (part p (build
                (shape base (box 20 10 2))
                (shape empty_overlay (difference base base))
                (shape overlay (if (= k 0) empty_overlay (translate 0 0 1 (box 3 3 3))))
                (result (fuse base overlay)))))"#,
        );
        plan_core_program(&program).expect("folded-if reference-branch plan");
    }

    #[test]
    fn plans_svg_wire_soup_for_artwork_rejected_by_clean_path() {
        // Two disjoint filled squares = multiple outer loops, which the clean
        // profile path rejects. The tolerant wire-soup fallback must instead
        // hand every wire to OCCT with a fill-rule, so region resolution happens
        // in the runner (mirrors build123d/ocpsvg).
        let svg_path = std::path::Path::new("/tmp/ecky-direct-occt-svg-artwork.svg");
        {
            let mut file = std::fs::File::create(svg_path).expect("create svg");
            file.write_all(
                b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 20 10\">\n  <path fill-rule=\"evenodd\" d=\"M0 0h4v4h-4z M10 0h4v4h-4z\"/>\n</svg>\n",
            )
            .expect("write svg");
        }

        let program = compile(
            r#"(model (part body (extrude (svg "/tmp/ecky-direct-occt-svg-artwork.svg" 20 10 "contain") 4)))"#,
        );

        let plan = plan_core_program(&program).expect("plan");
        let commands = &plan.parts[0].commands;
        let ops = commands.iter().map(|command| command.op).collect::<Vec<_>>();

        assert_eq!(
            ops,
            vec![OcctOp::Polygon, OcctOp::Polygon, OcctOp::Profile, OcctOp::Extrude],
            "two wires + soup profile + extrude"
        );

        let profile = commands
            .iter()
            .find(|command| command.op == OcctOp::Profile)
            .expect("profile command");
        let fill_rule = profile
            .keywords
            .iter()
            .find(|keyword| keyword.name == "fill-rule")
            .expect("fill-rule keyword present");
        assert_eq!(
            fill_rule.source_arg(),
            &OcctArg::Text("evenodd".to_string())
        );
        assert!(
            profile.keywords.iter().any(|keyword| keyword.name == "outer"),
            "wires ride via :outer"
        );

        assert!(std::fs::remove_file(svg_path).is_ok());
    }

    #[test]
    fn plans_import_stl_for_direct_occt() {
        let program = compile(r#"(model (part body (import-stl "/tmp/sample.stl")))"#);

        let plan = plan_core_program(&program).expect("plan");

        assert_eq!(
            plan.parts[0]
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>(),
            vec![OcctOp::ImportStl]
        );
    }

    #[test]
    fn plans_text_profile_for_direct_occt_extrusion() {
        let program = compile(r#"(model (part body (extrude (text "II" 12) 4)))"#);

        let plan = plan_core_program(&program).expect("plan");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();

        assert!(ops.len() >= 4, "{ops:?}");
        assert_eq!(ops.last(), Some(&OcctOp::Extrude));
        assert_eq!(ops[ops.len() - 2], OcctOp::Profile);
        assert!(
            ops[..ops.len() - 2].iter().all(|op| *op == OcctOp::Polygon),
            "{ops:?}"
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
    fn plans_box_align_tuple_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (box 4 6 8 :align '(center center min))))
            "#,
        );

        let plan = plan_core_program(&program).expect("box align planned");
        assert_eq!(plan.parts[0].commands.len(), 1);
        assert_eq!(plan.parts[0].commands[0].op, OcctOp::Box);
        assert_eq!(plan.parts[0].commands[0].keywords.len(), 1);
        assert_eq!(plan.parts[0].commands[0].keywords[0].name, "align");
        assert_eq!(
            plan.parts[0].commands[0].keywords[0].source_arg(),
            &OcctArg::List(vec![
                OcctArg::Symbol("center".into()),
                OcctArg::Symbol("center".into()),
                OcctArg::Symbol("min".into()),
            ])
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
    fn plans_xor_by_rewriting_into_supported_boolean_ops() {
        let program = compile(
            r#"
            (model
              (part body
                (xor (box 2 2 2) (sphere 1))))
            "#,
        );

        let plan = plan_core_program(&program).expect("xor planned");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();

        assert_eq!(
            ops,
            vec![
                OcctOp::Box,
                OcctOp::Sphere,
                OcctOp::Union,
                OcctOp::Intersection,
                OcctOp::Difference,
            ]
        );
    }

    #[test]
    fn plans_finite_map_apply_range_for_direct_occt() {
        let program = compile(include_str!(
            "../../tests/fixtures/cad/surface/voronoi_perforated_panel.ecky"
        ));

        let plan = plan_core_program(&program).expect("finite map/apply/range planned");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();

        assert_eq!(ops.first(), Some(&OcctOp::Box));
        assert_eq!(ops.last(), Some(&OcctOp::Difference));
        assert!(ops.iter().filter(|op| **op == OcctOp::Cylinder).count() >= 12);
        assert!(ops.contains(&OcctOp::Union));
    }

    #[test]
    fn plans_parameterized_map_body_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (params (number cell-count 4 :min 1 :max 8 :step 1))
              (part panel
                (build
                  (shape panel (box 72 48 4 :align '(center center min)))
                  (result
                    (difference
                      panel
                      (apply union
                        (map
                          (lambda (cell)
                            (let* ((col (- cell (* 4 (floor (/ cell 4)))))
                                   (x (* (- col 1.5) 14)))
                              (translate x 0 0
                                (cylinder 2 8 24))))
                          (range 0 cell-count))))))))
            "#,
        );

        let plan = plan_core_program(&program).expect("parameterized map planned");
        let cylinder_count = plan.parts[0]
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Cylinder)
            .count();

        assert_eq!(cylinder_count, 4);
    }

    #[test]
    fn plans_map_range_count_from_build_scalar_binding_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (params (number chamber_cols 5 :min 3 :max 7 :step 1))
              (part panel
                (build
                  (shape wall 3)
                  (shape count (* chamber_cols 3))
                  (shape panel (box 72 48 4 :align '(center center min)))
                  (shape cutters
                    (apply union
                      (map
                        (lambda (cell)
                          (translate cell 0 0
                            (cylinder 2 8 24)))
                        (range 0 count))))
                  (result
                    (difference panel cutters)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("build-bound range count planned");
        let cylinder_count = plan.parts[0]
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Cylinder)
            .count();

        assert_eq!(cylinder_count, 15);
    }

    #[test]
    fn plans_map_body_box_align_tuple_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape dividers
                    (apply union
                      (map
                        (lambda (divider)
                          (translate divider 0 0
                            (box 1 2 3 :align '(center center center))))
                        (range 1 4))))
                  (result dividers))))
            "#,
        );

        let plan = plan_core_program(&program).expect("map body box align planned");
        let box_count = plan.parts[0]
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Box)
            .count();

        assert_eq!(box_count, 3);
    }

    #[test]
    fn plans_parametric_map_with_build_scalars_and_align_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (params
                (number hotel_w 74 :min 50 :max 110 :step 1)
                (number hotel_d 34 :min 24 :max 54 :step 1)
                (number hotel_h 42 :min 28 :max 70 :step 1)
                (number chamber_cols 5 :min 3 :max 7 :step 1))
              (part body
                (build
                  (shape wall 3)
                  (shape col_gap (/ (- hotel_w (* 2 wall)) chamber_cols))
                  (shape dividers
                    (apply union
                      (map
                        (lambda (divider)
                          (translate (+ (* -0.5 hotel_w) wall (* divider col_gap)) 0 (/ hotel_h 2)
                            (box 1.4 (+ hotel_d 2) (- hotel_h (* 2 wall)) :align '(center center center))))
                        (range 1 chamber_cols))))
                  (result dividers))))
            "#,
        );

        let plan = plan_core_program(&program).expect("parametric aligned dividers planned");
        let box_count = plan.parts[0]
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Box)
            .count();

        assert_eq!(box_count, 4);
    }

    #[test]
    fn plans_repeat_pick_binding_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape marker
                    (repeat-pick i 4 (= i 3)
                      (translate (+ (* i 10) 5) 0 12 (sphere 3))))
                  (result (compound marker)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("repeat-pick planned");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();

        assert_eq!(
            ops,
            vec![OcctOp::Sphere, OcctOp::Translate, OcctOp::Compound]
        );
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
    fn plans_helical_ridge_for_direct_occt_sweep() {
        let program = compile(
            r#"
            (model
              (part body
                (helical-ridge
                  :radius 20
                  :pitch 6
                  :height 30
                  :base-width 2
                  :crest-width 1
                  :depth 1.5)))
            "#,
        );

        let plan = plan_core_program(&program).expect("helical-ridge planned");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();

        // Profile is a polyline (`Path`), but the spine is a true `HelixPath`
        // (Geom helix), matching build123d's `Edge.make_helix` — not a sampled
        // polyline. A faceted polyline spine rendered the wrong pitch and gaps.
        assert_eq!(
            ops,
            vec![OcctOp::Path, OcctOp::MakeFace, OcctOp::HelixPath, OcctOp::Sweep]
        );
    }

    #[test]
    fn plans_regular_polygon_as_polygon_for_direct_occt() {
        // `regular-polygon` is a Custom op that expands to a `polygon` of the
        // shared computed vertices, so native matches build123d by construction.
        let program = compile(
            r#"
            (model
              (part hex
                (extrude (regular-polygon 6 10) 5)))
            "#,
        );
        let plan = plan_core_program(&program).expect("regular-polygon planned");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();
        assert!(
            ops.contains(&OcctOp::Polygon) && ops.contains(&OcctOp::Extrude),
            "expected regular-polygon to expand into a polygon + extrude plan, got {ops:?}"
        );
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

    #[test]
    fn unsupported_op_reports_backend_layer_with_fix() {
        use crate::contracts::ErrorLayer;
        let err = unsupported("fillet", "not in first surface");
        // `unsupported` builds an `AuthoringError` then bridges to `AppError`
        // via `From`; layer/fix now live directly on the boundary `AppError`.
        assert_eq!(err.operation.as_deref(), Some("fillet"));
        assert_eq!(err.code, crate::contracts::AppErrorCode::Render);
        assert!(err.to_string().contains("direct OCCT"), "backend named: {}", err);
        assert_eq!(err.layer, Some(ErrorLayer::Backend));
        assert!(err.fix.expect("fix present").hint.is_some());
    }

    #[test]
    fn plan_reports_authoring_failure_naming_op() {
        // A bad op fails planning; the boundary error is a single `AppError`
        // whose summary names an offending op (no diagnostics collection anymore).
        let program = compile(
            r#"
            (model
              (part body (bx 1 1 1))
              (part handle (sphre 2)))
            "#,
        );
        let err = plan_core_program(&program).expect_err("authoring failure");
        assert!(
            err.message.contains("bx") || err.message.contains("sphre"),
            "summary names an op: {}",
            err.message
        );
    }
}


