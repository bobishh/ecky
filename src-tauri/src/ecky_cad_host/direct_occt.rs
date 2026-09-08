use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use crate::contracts::{AppError, AuthoringResult, DesignParams, ParamValue};
use crate::contracts::{AuthoringError, AuthoringReason, ErrorFix, ErrorLayer};
use crate::ecky_cad_host::svg_profile::{
    extract_svg_wire_soup_profile, parse_svg_profile, SvgFillRule, SvgFitMode,
};
use crate::ecky_cad_host::text_profile::parse_text_profile;
use crate::ecky_core_ir::{
    CoreArrayOp, CoreBinding, CoreBooleanOp, CoreFrameOp, CoreKeywordArg, CoreLiteral, CoreMetaOp,
    CoreNode, CoreNodeKind, CoreOperation, CoreParameterKind, CorePart, CorePathOp, CorePrimitive,
    CoreProgram, CoreReference, CoreSelectorPayload, CoreSelectorTagKind, CoreShapeBinding,
    CoreSurfaceOp, CoreSymbol, CoreTransformOp, CoreValueKind, NodeId,
};

// --- Authoring-error constructors (backend layer) -------------------------
// The direct OCCT planner is the backend wall: every failure here means an op
// the surface authored cannot be executed by this backend. These helpers keep
// call sites one line and guarantee a backend-layered error.

fn backend_error(reason: AuthoringReason, msg: impl Into<String>) -> AuthoringError {
    AuthoringError::backend(reason, msg)
}

fn backend_validation(msg: impl Into<String>) -> AuthoringError {
    backend_error(AuthoringReason::Type, msg)
}

fn backend_op_error(reason: AuthoringReason, op: &str, msg: impl Into<String>) -> AuthoringError {
    AuthoringError::for_op(ErrorLayer::Backend, reason, op, msg)
}

fn planner_dependency_error(context: &str, err: AppError) -> AuthoringError {
    backend_error(
        AuthoringReason::Type,
        format!("Direct OCCT planner {context}: {err}"),
    )
}

fn core_param_env(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AuthoringResult<BTreeMap<String, ParamValue>> {
    crate::ecky_ir::build_core_program_param_env_for_eval(program, parameters)
        .map_err(|err| planner_dependency_error("could not resolve parameters", err))
}

fn eval_core_number(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AuthoringResult<f64> {
    crate::ecky_ir::eval_core_number_with_locals(node, param_names, env)
        .map_err(|err| planner_dependency_error("could not evaluate number", err))
}

fn eval_core_bool(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AuthoringResult<bool> {
    crate::ecky_ir::eval_core_bool_with_locals(node, param_names, env)
        .map_err(|err| planner_dependency_error("could not evaluate boolean", err))
}

fn eval_core_stringish(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AuthoringResult<String> {
    crate::ecky_ir::eval_core_stringish_with_locals(node, param_names, env)
        .map_err(|err| planner_dependency_error("could not evaluate text", err))
}

fn bk(reason: AuthoringReason, msg: impl Into<String>) -> AuthoringError {
    backend_error(reason, msg)
}

fn bk_op(reason: AuthoringReason, op: &str, msg: impl Into<String>) -> AuthoringError {
    backend_op_error(reason, op, msg)
}

fn bk_arity(op: &str, expected: &str) -> AuthoringError {
    backend_op_error(
        AuthoringReason::Arity,
        op,
        format!("`{op}` expects {expected}."),
    )
}

fn bk_constrained(op: &str, msg: impl Into<String>, valid: &[&str]) -> AuthoringError {
    constrained_backend_error(op, msg, valid)
}

fn planner_error(reason: AuthoringReason, msg: impl Into<String>) -> AuthoringError {
    backend_error(reason, msg)
}

fn planner_op_error(reason: AuthoringReason, op: &str, msg: impl Into<String>) -> AuthoringError {
    backend_op_error(reason, op, msg)
}

fn planner_arity_error(op: &str, expected: &str) -> AuthoringError {
    planner_op_error(
        AuthoringReason::Arity,
        op,
        format!("`{op}` expects {expected}."),
    )
}

fn constrained_backend_error(op: &str, msg: impl Into<String>, valid: &[&str]) -> AuthoringError {
    AuthoringError::constrained(ErrorLayer::Backend, op, msg, valid)
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
    ImportStep,
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
    ClipThreadEnvelope,
    ClipPlane,
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
    Solidify,
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcctAuthoredShapeBinding {
    pub part_key: String,
    pub name: String,
    pub slot: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcctPlannedProgram {
    pub plan: OcctPlan,
    pub authored_shape_bindings: Vec<OcctAuthoredShapeBinding>,
}

pub fn plan_core_program(program: &CoreProgram) -> AuthoringResult<OcctPlan> {
    plan_core_program_with_params(program, &DesignParams::new())
}

pub fn plan_core_program_with_params(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AuthoringResult<OcctPlan> {
    Ok(plan_core_program_with_params_and_bindings(program, parameters)?.plan)
}

pub fn plan_core_program_with_params_and_bindings(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AuthoringResult<OcctPlannedProgram> {
    let normalized =
        super::direct_occt_normalize::normalize_core_program_for_direct_occt(program, parameters)
            .map_err(|err| planner_dependency_error("normalization failed", err))?;
    let expanded = expand_core_program_for_direct_occt(&normalized, parameters)?;
    plan_expanded_core_program_with_bindings(&expanded, parameters, true)
}

#[cfg(test)]
fn plan_expanded_core_program(
    program: &CoreProgram,
    parameters: &DesignParams,
    optimize_graph: bool,
) -> AuthoringResult<OcctPlan> {
    Ok(plan_expanded_core_program_with_bindings(program, parameters, optimize_graph)?.plan)
}

fn plan_expanded_core_program_with_bindings(
    program: &CoreProgram,
    parameters: &DesignParams,
    optimize_graph: bool,
) -> AuthoringResult<OcctPlannedProgram> {
    crate::ecky_core_ir::verify_core_program(program).map_err(|err| {
        backend_error(
            AuthoringReason::Type,
            format!(
                "Direct OCCT adapter rejected invalid Core IR before planning: {}",
                err
            ),
        )
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
    let scalar_env = core_param_env(program, parameters).map_err(|err| {
        backend_error(
            AuthoringReason::Type,
            format!("Direct OCCT adapter could not resolve parameters: {err}"),
        )
    })?;

    let planned_parts = program
        .parts
        .iter()
        .map(|part| {
            let requested_let_bindings = program
                .selector_tags
                .iter()
                .filter(|tag| tag.kind == CoreSelectorTagKind::Face && tag.target == part.key)
                .filter_map(|tag| {
                    let raw = tag.authored_selector.trim();
                    raw.to_ascii_lowercase()
                        .starts_with("created-by:")
                        .then(|| &raw["created-by:".len()..])
                })
                .flat_map(|bindings| bindings.split('|'))
                .map(str::trim)
                .filter(|binding| !binding.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>();
            let mut planner = PartPlanner::new(
                &param_names,
                &scalar_env,
                max_node_id(&part.root) + 1,
                requested_let_bindings.clone(),
            );
            let root = planner.plan_node(&part.root)?;
            let authored_shape_bindings = planner.authored_shape_bindings.clone();
            let protected_slots = authored_shape_bindings
                .iter()
                .filter(|(name, _)| requested_let_bindings.contains(name))
                .map(|(_, slot)| *slot)
                .collect::<BTreeSet<_>>();
            let commands = if optimize_graph {
                optimize_part_commands(root, planner.commands, &protected_slots).map_err(|err| {
                    backend_error(
                        AuthoringReason::Type,
                        format!("Direct OCCT adapter produced an invalid command graph: {err}"),
                    )
                })?
            } else {
                planner.commands
            };
            let retained_slots = commands
                .iter()
                .map(|command| command.output)
                .chain(std::iter::once(root))
                .collect::<std::collections::HashSet<_>>();
            let bindings = authored_shape_bindings
                .into_iter()
                .filter(|(_, slot)| retained_slots.contains(slot))
                .map(|(name, slot)| OcctAuthoredShapeBinding {
                    part_key: part.key.clone(),
                    name,
                    slot: slot.0,
                })
                .collect::<Vec<_>>();
            Ok((
                OcctPartPlan {
                    key: part.key.clone(),
                    label: part.label.clone(),
                    root,
                    commands,
                },
                bindings,
            ))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;

    let mut parts = Vec::with_capacity(planned_parts.len());
    let mut authored_shape_bindings = Vec::new();
    for (part, bindings) in planned_parts {
        parts.push(part);
        authored_shape_bindings.extend(bindings);
    }

    let mut binding_counts = BTreeMap::<(String, String), usize>::new();
    for binding in &authored_shape_bindings {
        *binding_counts
            .entry((binding.part_key.clone(), binding.name.clone()))
            .or_default() += 1;
    }
    authored_shape_bindings.retain(|binding| {
        binding_counts.get(&(binding.part_key.clone(), binding.name.clone())) == Some(&1)
    });

    Ok(OcctPlannedProgram {
        plan: OcctPlan {
            parameters: occt_parameters,
            parts,
        },
        authored_shape_bindings,
    })
}

#[cfg(test)]
pub(crate) fn plan_core_program_unoptimized(program: &CoreProgram) -> AuthoringResult<OcctPlan> {
    let parameters = DesignParams::new();
    let normalized =
        super::direct_occt_normalize::normalize_core_program_for_direct_occt(program, &parameters)
            .map_err(|err| planner_dependency_error("normalization failed", err))?;
    let expanded = expand_core_program_for_direct_occt(&normalized, &parameters)?;
    plan_expanded_core_program(&expanded, &parameters, false)
}

fn optimize_part_commands(
    root: OcctSlot,
    mut commands: Vec<OcctCommand>,
    protected_slots: &BTreeSet<OcctSlot>,
) -> AuthoringResult<Vec<OcctCommand>> {
    let producers = command_producers(&commands)?;
    validate_command_graph(root, &commands, &producers)?;

    let original = commands.clone();
    // Fresh output slots for transforms synthesized while distributing a
    // `Difference(base, affineTransform(Union(children)))` rewrite. Slot ids are
    // node ids, so anything past the planned max is free.
    let mut next_slot = commands
        .iter()
        .map(|command| command.output.0)
        .max()
        .unwrap_or(0)
        + 1;
    let mut bypassed_protected_slots = BTreeSet::new();
    let mut result: Vec<OcctCommand> = Vec::with_capacity(commands.len());
    for mut command in commands.into_iter() {
        if command.op == OcctOp::Difference && command.args.len() >= 2 {
            let mut optimized = Vec::with_capacity(command.args.len());
            optimized.push(command.args[0].clone());
            // Per-child transforms synthesized while flattening this difference's
            // tools are spliced in BEFORE the difference itself, because the
            // runner resolves slots in command order and the boolean consumes
            // them. Each synthesized transform depends only on union children,
            // which already precede the difference, so topological order holds.
            let mut synthesized: Vec<OcctCommand> = Vec::new();
            for tool in &command.args[1..] {
                if let OcctArg::Ref(slot) = tool {
                    if protected_slots.contains(slot) {
                        bypassed_protected_slots.insert(*slot);
                    }
                }
                flatten_difference_tool(
                    tool,
                    &original,
                    &producers,
                    &mut optimized,
                    &mut synthesized,
                    &mut next_slot,
                );
            }
            result.extend(synthesized);
            command.args = optimized;

            // parametric-thread-feature 3.1 binary-cut optimizer slice: a
            // Difference with two or more tools rewrites to a stable-order
            // chain of binary Difference commands so each boolean cut is
            // binary. Intermediates get fresh output slots and carry no
            // keywords; the FINAL link keeps the original command.output and
            // original keywords, so part.root and any downstream reference
            // (fillet/chamfer/topology keyword) is unchanged. A single-tool
            // Difference (one tool after flattening) is left binary as-is.
            if command.args.len() >= 3 {
                let chain = chain_binary_differences(command, &mut next_slot);
                result.extend(chain);
                continue;
            }
        }
        result.push(command);
    }
    commands = result;

    let optimized_producers = command_producers(&commands)?;
    let mut reachable = BTreeSet::new();
    collect_reachable_slots(root, &commands, &optimized_producers, &mut reachable)?;
    // Keep authored topology bindings even when the boolean consumes their
    // flattened children. The bound union remains inspectable, while the final
    // geometry avoids using its potentially disconnected fused result.
    for slot in bypassed_protected_slots {
        collect_reachable_slots(slot, &commands, &optimized_producers, &mut reachable)?;
    }
    commands.retain(|command| reachable.contains(&command.output));
    Ok(commands)
}

/// Rewrite a flattened `Difference(base, tool1, tool2, ..., toolN)` with `N >= 2`
/// tools into a stable-order chain of binary Difference commands:
///   Difference(base, tool1)         -> fresh intermediate
///   Difference(intermediate, tool2) -> fresh intermediate
///   ...
///   Difference(intermediate, toolN) -> ORIGINAL output (original keywords)
/// so each boolean cut is binary. Intermediates take fresh output slots drawn
/// from `next_slot` (advanced for each) and carry no keywords; the final link
/// reuses the original command's output slot and keywords, so `part.root` and
/// any downstream reference (fillet/chamfer/topology keyword) is unchanged.
///
/// The chain is emitted in order with intermediates before the final link, so
/// the runner/executor resolves slots in command order. Each link depends only
/// on the previous link's fresh output (which precedes it) plus an original
/// tool slot (which preceded the original difference), so topological order
/// holds. This mirrors the existing flatten pass's invariants: it never folds
/// keywords onto an intermediate, and it never touches the base.
fn chain_binary_differences(mut command: OcctCommand, next_slot: &mut u64) -> Vec<OcctCommand> {
    debug_assert_eq!(command.op, OcctOp::Difference);
    debug_assert!(command.args.len() >= 3);
    let original_output = command.output;
    let original_keywords = std::mem::take(&mut command.keywords);
    let tool_count = command.args.len() - 1;
    let mut chain: Vec<OcctCommand> = Vec::with_capacity(tool_count);
    for link_index in 0..tool_count {
        let is_final = link_index == tool_count - 1;
        let base_arg = if link_index == 0 {
            command.args[0].clone()
        } else {
            OcctArg::Ref(chain[link_index - 1].output)
        };
        let tool_arg = command.args[link_index + 1].clone();
        let (output, keywords) = if is_final {
            (original_output, original_keywords.clone())
        } else {
            let fresh = OcctSlot(*next_slot);
            *next_slot += 1;
            (fresh, Vec::new())
        };
        chain.push(OcctCommand {
            output,
            op: OcctOp::Difference,
            args: vec![base_arg, tool_arg],
            keywords,
        });
    }
    chain
}

fn command_producers(commands: &[OcctCommand]) -> AuthoringResult<BTreeMap<OcctSlot, usize>> {
    let mut producers = BTreeMap::new();
    for (index, command) in commands.iter().enumerate() {
        if producers.insert(command.output, index).is_some() {
            return Err(backend_validation(format!(
                "Direct OCCT plan has duplicate producer for slot {}.",
                command.output.0
            )));
        }
    }
    Ok(producers)
}

fn validate_command_graph(
    root: OcctSlot,
    commands: &[OcctCommand],
    producers: &BTreeMap<OcctSlot, usize>,
) -> AuthoringResult<()> {
    if !producers.contains_key(&root) {
        return Err(backend_validation(format!(
            "Direct OCCT plan root references missing slot {}.",
            root.0
        )));
    }

    for (consumer_index, command) in commands.iter().enumerate() {
        let mut dependencies = Vec::new();
        collect_command_refs(command, &mut dependencies);
        for dependency in dependencies {
            let Some(producer_index) = producers.get(&dependency).copied() else {
                return Err(backend_validation(format!(
                    "Direct OCCT command slot {} references missing slot {}.",
                    command.output.0, dependency.0
                )));
            };
            if producer_index >= consumer_index {
                return Err(backend_validation(format!(
                    "Direct OCCT command slot {} has cyclic or forward dependency on slot {}.",
                    command.output.0, dependency.0
                )));
            }
        }
    }
    Ok(())
}

/// Returns true for the affine transform ops this optimizer distributes
/// across. Every op already present under `CoreTransformOp` —
/// `Translate`/`Rotate`/`Scale`/`Mirror` — is affine, and an affine transform
/// distributes over a boolean union: `T(A ∪ B) == T(A) ∪ T(B)`. Non-affine ops
/// are never generalized here.
fn is_affine_transform_op(op: OcctOp) -> bool {
    matches!(
        op,
        OcctOp::Translate | OcctOp::Rotate | OcctOp::Scale | OcctOp::Mirror
    )
}

/// For an affine transform command, returns the index of its single geometry
/// subject (the shape being transformed) iff the command has exactly one `Ref`
/// arg and that arg is trailing. Translate/Rotate/Scale/Mirror all lay out as
/// `[scalar params..., shape-ref]`, so the subject is the trailing `Ref`. A
/// second `Ref` (ambiguous subject) or a non-trailing `Ref` bails out, so the
/// rewrite never mis-attributes a param ref as geometry.
fn affine_transform_subject_index(command: &OcctCommand) -> Option<usize> {
    if !is_affine_transform_op(command.op) || command.args.is_empty() {
        return None;
    }
    let last = command.args.len() - 1;
    if !matches!(command.args[last], OcctArg::Ref(_)) {
        return None;
    }
    let extra_refs = command.args[..last]
        .iter()
        .filter(|arg| matches!(arg, OcctArg::Ref(_)))
        .count();
    if extra_refs == 0 {
        Some(last)
    } else {
        None
    }
}

fn is_keyword_free_all_ref_union(command: &OcctCommand) -> bool {
    command.op == OcctOp::Union
        && command.keywords.is_empty()
        && command
            .args
            .iter()
            .all(|arg| matches!(arg, OcctArg::Ref(_)))
}

/// True iff `command` is a keyword-free affine transform whose subject arg is a
/// `Ref` to a keyword-free all-`Ref` union. This is the distributable shape:
/// `Difference(base, T(Union(children)))` rewrites to
/// `Difference(base, T(child1), T(child2), ...)`.
fn is_transform_of_distributable_union(
    command: &OcctCommand,
    commands: &[OcctCommand],
    producers: &BTreeMap<OcctSlot, usize>,
) -> bool {
    let Some(subject_index) = affine_transform_subject_index(command) else {
        return false;
    };
    if !command.keywords.is_empty() {
        return false;
    }
    let OcctArg::Ref(union_slot) = &command.args[subject_index] else {
        return false;
    };
    let Some(union_command) = producers
        .get(union_slot)
        .and_then(|index| commands.get(*index))
    else {
        return false;
    };
    is_keyword_free_all_ref_union(union_command)
}

fn flatten_difference_tool(
    tool: &OcctArg,
    commands: &[OcctCommand],
    producers: &BTreeMap<OcctSlot, usize>,
    output: &mut Vec<OcctArg>,
    synthesized: &mut Vec<OcctCommand>,
    next_slot: &mut u64,
) {
    let OcctArg::Ref(slot) = tool else {
        output.push(tool.clone());
        return;
    };
    let Some(producer) = producers.get(slot).and_then(|index| commands.get(*index)) else {
        output.push(tool.clone());
        return;
    };
    flatten_command_as_tool(
        producer,
        commands,
        producers,
        output,
        synthesized,
        next_slot,
    );
}

/// Flatten the command that produces a difference tool into one or more tool
/// refs. Three cases:
/// - a keyword-free all-`Ref` union: recurse over each child (existing
///   behavior, so nested unions still collapse);
/// - a keyword-free affine transform of such a union: distribute the transform
///   over each union child (affine transforms distribute over union), emitting
///   one identically-transformed tool per child so the fused union never reaches
///   the boolean, then recurse in case a child is itself a distributable union;
/// - anything else: keep the tool verbatim.
fn flatten_command_as_tool(
    producer: &OcctCommand,
    commands: &[OcctCommand],
    producers: &BTreeMap<OcctSlot, usize>,
    output: &mut Vec<OcctArg>,
    synthesized: &mut Vec<OcctCommand>,
    next_slot: &mut u64,
) {
    if is_keyword_free_all_ref_union(producer) {
        for child in &producer.args {
            flatten_difference_tool(child, commands, producers, output, synthesized, next_slot);
        }
        return;
    }
    if is_transform_of_distributable_union(producer, commands, producers) {
        let subject_index = affine_transform_subject_index(producer).expect("affine subject");
        let OcctArg::Ref(union_slot) = &producer.args[subject_index] else {
            unreachable!("affine transform subject is a Ref");
        };
        let union_command = producers
            .get(union_slot)
            .and_then(|index| commands.get(*index))
            .expect("distributable union");
        for child in &union_command.args {
            let mut transformed = producer.clone();
            transformed.output = OcctSlot(*next_slot);
            *next_slot += 1;
            transformed.args[subject_index] = child.clone();
            if is_transform_of_distributable_union(&transformed, commands, producers) {
                // The synthesized transform still wraps a union — keep
                // distributing instead of emitting an intermediate tool.
                flatten_command_as_tool(
                    &transformed,
                    commands,
                    producers,
                    output,
                    synthesized,
                    next_slot,
                );
            } else {
                let out = transformed.output;
                synthesized.push(transformed);
                output.push(OcctArg::Ref(out));
            }
        }
        return;
    }
    output.push(OcctArg::Ref(producer.output));
}

fn collect_reachable_slots(
    slot: OcctSlot,
    commands: &[OcctCommand],
    producers: &BTreeMap<OcctSlot, usize>,
    reachable: &mut BTreeSet<OcctSlot>,
) -> AuthoringResult<()> {
    if !reachable.insert(slot) {
        return Ok(());
    }
    let command = producers
        .get(&slot)
        .and_then(|index| commands.get(*index))
        .ok_or_else(|| {
            backend_validation(format!(
                "Direct OCCT reachability references missing slot {}.",
                slot.0
            ))
        })?;
    let mut dependencies = Vec::new();
    collect_command_refs(command, &mut dependencies);
    for dependency in dependencies {
        collect_reachable_slots(dependency, commands, producers, reachable)?;
    }
    Ok(())
}

fn collect_command_refs(command: &OcctCommand, output: &mut Vec<OcctSlot>) {
    for arg in &command.args {
        collect_arg_refs(arg, output);
    }
    for keyword in &command.keywords {
        collect_arg_refs(keyword.source_arg(), output);
    }
}

fn collect_arg_refs(arg: &OcctArg, output: &mut Vec<OcctSlot>) {
    match arg {
        OcctArg::Ref(slot) => output.push(*slot),
        OcctArg::List(items) => {
            for item in items {
                collect_arg_refs(item, output);
            }
        }
        OcctArg::Number(_)
        | OcctArg::Boolean(_)
        | OcctArg::Text(_)
        | OcctArg::Symbol(_)
        | OcctArg::Point2(_)
        | OcctArg::Point3(_)
        | OcctArg::Param(_) => {}
    }
}

fn expand_core_program_for_direct_occt(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AuthoringResult<CoreProgram> {
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let env = core_param_env(program, parameters)?;
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
        .collect::<AuthoringResult<Vec<_>>>()?;
    Ok(
        CoreProgram::new(program.id, program.parameters.clone(), parts)
            .with_selector_tags(program.selector_tags.clone()),
    )
}

fn expand_node_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AuthoringResult<CoreNode> {
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
                    bk(
                        AuthoringReason::Type,
                        format!(
                            "Direct OCCT expander could not evaluate build binding `{}`: {err}",
                            binding.name
                        ),
                    )
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
                    bk(
                        AuthoringReason::Type,
                        format!(
                            "Direct OCCT expander could not evaluate let binding `{}`: {err}",
                            binding.name
                        ),
                    )
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
        } if !keywords.is_empty() => Err(bk_op(
            AuthoringReason::Unsupported,
            "svg",
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
            expand_thread_node(
                node,
                args,
                keywords,
                param_names,
                env,
                node_env,
                next_node_id,
            )
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "tapped-hole") => {
            expand_tapped_hole_node(
                node,
                args,
                keywords,
                param_names,
                env,
                node_env,
                next_node_id,
            )
        }
        CoreNodeKind::Call { op, args, .. } if matches!(op, CoreOperation::Custom(name) if name == "rib" || name == "groove") =>
        {
            let is_rib = matches!(op, CoreOperation::Custom(name) if name == "rib");
            expand_rib_groove_node(node, is_rib, args, param_names, env, node_env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "sampled-radial-loft") => {
            expand_sampled_radial_loft_node(node, args, keywords, param_names, env, next_node_id)
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "regular-polygon") => {
            expand_regular_polygon_node(
                node,
                args,
                keywords,
                param_names,
                env,
                node_env,
                next_node_id,
            )
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "voronoi-cell") => {
            expand_voronoi_cell_node(
                node,
                args,
                keywords,
                param_names,
                env,
                node_env,
                next_node_id,
            )
        }
        CoreNodeKind::Call { op, args, keywords } if matches!(op, CoreOperation::Custom(name) if name == "trapezoid") => {
            expand_trapezoid_node(
                node,
                args,
                keywords,
                param_names,
                env,
                node_env,
                next_node_id,
            )
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
                    .collect::<AuthoringResult<Vec<_>>>()?,
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
                    .collect::<AuthoringResult<Vec<_>>>()?,
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
                    .collect::<AuthoringResult<Vec<_>>>()?,
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
                    .collect::<AuthoringResult<Vec<_>>>()?,
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
                    .collect::<AuthoringResult<Vec<_>>>()?,
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
                    .collect::<AuthoringResult<Vec<_>>>()?,
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
) -> AuthoringResult<CoreNode> {
    if args.len() < 2 {
        return Err(bk_arity("xor", "at least two operands"));
    }

    let normalized_args = args
        .iter()
        .map(|arg| expand_node_for_direct_occt(arg, param_names, env, node_env, next_node_id))
        .collect::<AuthoringResult<Vec<_>>>()?;

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
) -> AuthoringResult<CoreNode> {
    if args.is_empty() || args.len() > 4 {
        return Err(bk_arity(
            "svg",
            "a file path, optional target width/height, and optional fit mode",
        ));
    }

    let source = eval_core_stringish(&args[0], param_names, env)?;
    let svg_text = if fs::metadata(&source).is_ok() {
        fs::read_to_string(&source).map_err(|err| {
            AuthoringError::surface(
                AuthoringReason::Type,
                format!("Direct OCCT adapter could not read SVG file `{source}`: {err}"),
            )
        })?
    } else if source.trim_start().starts_with('<') {
        source
    } else {
        return Err(AuthoringError::surface(
            AuthoringReason::Type,
            format!("Direct OCCT adapter could not read SVG source at `{source}`."),
        ));
    };

    let target_width = args
        .get(1)
        .map(|arg| {
            eval_core_number(arg, param_names, env).map_err(|err| {
                bk_op(
                    AuthoringReason::Type,
                    "svg",
                    format!("Direct OCCT adapter could not evaluate `svg` width: {err}",),
                )
            })
        })
        .transpose()?;

    let target_height = args
        .get(2)
        .map(|arg| {
            eval_core_number(arg, param_names, env).map_err(|err| {
                bk_op(
                    AuthoringReason::Type,
                    "svg",
                    format!("Direct OCCT adapter could not evaluate `svg` height: {err}",),
                )
            })
        })
        .transpose()?;

    let fit_mode = args
        .get(3)
        .map(|arg| {
            let value = eval_core_stringish(arg, param_names, env)?;
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
            let soup = extract_svg_wire_soup_profile(&svg_text, target_width, target_height, fit)
                .map_err(|err| {
                AuthoringError::surface(
                    AuthoringReason::Type,
                    format!("Direct OCCT adapter could not parse SVG source: {err}"),
                )
                .with_op("svg")
            })?;
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
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    next_node_id: &mut u64,
) -> AuthoringResult<CoreNode> {
    if args.len() != 2 {
        return Err(bk_arity("text", "text value and size"));
    }

    let value = eval_core_stringish(&args[0], param_names, env)?;
    let size = eval_core_number(&args[1], param_names, env)?;
    let font = keywords
        .iter()
        .find(|keyword| keyword.name == "font")
        .map(|keyword| eval_core_stringish(keyword.source_node(), param_names, env))
        .transpose()?;
    let components = parse_text_profile(&value, size, font.as_deref()).map_err(|err| {
        backend_op_error(
            AuthoringReason::Type,
            "text",
            format!("Direct OCCT adapter could not lower text profile: {err}"),
        )
    })?;
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
) -> AuthoringResult<CoreNode> {
    if !args.is_empty() {
        return Err(bk_op(
            AuthoringReason::Unsupported,
            "helical-ridge",
            "`helical-ridge` expects keyword options only.",
        ));
    }
    reject_unknown_keywords(
        keywords,
        &[
            "radius",
            "top-radius",
            "pitch",
            "height",
            "base-width",
            "crest-width",
            "depth",
            "lower-flank",
            "upper-flank",
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
    let has_top_radius = keywords.iter().any(|keyword| keyword.name == "top-radius");
    let top_radius = optional_keyword_number(
        keywords,
        "top-radius",
        radius,
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?;
    if !top_radius.is_finite() || top_radius <= 0.0 {
        return Err(bk_op(
            AuthoringReason::Type,
            "helical-ridge",
            "`helical-ridge` top-radius must be positive and finite.",
        ));
    }
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
    let lower_flank = optional_keyword_number(
        keywords,
        "lower-flank",
        0.0,
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?
    .to_radians();
    let upper_flank = optional_keyword_number(
        keywords,
        "upper-flank",
        0.0,
        "helical-ridge",
        param_names,
        env,
        node_env,
    )?
    .to_radians();
    let has_lower_flank = keywords.iter().any(|keyword| keyword.name == "lower-flank");
    let has_upper_flank = keywords.iter().any(|keyword| keyword.name == "upper-flank");
    if has_lower_flank != has_upper_flank {
        return Err(bk_constrained(
            "helical-ridge",
            "`helical-ridge` needs both `:lower-flank` and `:upper-flank` for an asymmetric profile.",
            &[":lower-flank", ":upper-flank"],
        ));
    }
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
    // Local section. The Direct OCCT helical loft places each sampled section
    // on the spine; keeping the absolute radius in both the section and helix
    // path displaces the ridge away from the thread core.
    let (base_lower, base_upper) = if has_lower_flank {
        (
            -crest_half - ridge_depth * lower_flank.tan(),
            crest_half + ridge_depth * upper_flank.tan(),
        )
    } else {
        (-base_half, base_half)
    };
    let profile_wire = path3_node(
        &[
            [0.0, 0.0, 0.0],
            [0.0, 0.0, base_lower],
            [ridge_depth, 0.0, -crest_half],
            [ridge_depth, 0.0, crest_half],
            [0.0, 0.0, base_upper],
            [0.0, 0.0, 0.0],
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
    let mut path_args = vec![radius_node, pitch_node, height_node, lefthand_node];
    if has_top_radius {
        path_args.push(number_node(next_node_id, top_radius));
    }
    let path = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Custom("helix-path".to_string()),
            args: path_args,
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct ThreadProfile {
    base_width: f64,
    crest_width: f64,
    lower_flank: f64,
    upper_flank: f64,
}

/// Closed-form symmetric thread profile. Backend geometry math uses radians;
/// Core IR angle literals and angle parameters are canonical degrees.
fn derive_thread_profile(
    pitch: f64,
    depth: f64,
    flank: Option<f64>,
    crest: Option<f64>,
    base_width: Option<f64>,
    crest_width: Option<f64>,
) -> ThreadProfile {
    let crest_width = crest_width.or(crest).unwrap_or(pitch * 0.25);
    let base_width = base_width.unwrap_or_else(|| {
        flank
            .map(|angle| crest_width + 2.0 * depth * angle.tan())
            .unwrap_or(pitch * 0.75)
    });
    let symmetric_flank = ((base_width - crest_width).max(0.0) / (2.0 * depth)).atan();
    ThreadProfile {
        base_width,
        crest_width,
        lower_flank: symmetric_flank,
        upper_flank: symmetric_flank,
    }
}

fn derive_buttress_thread_profile(
    _pitch: f64,
    depth: f64,
    load_flank: f64,
    return_flank: f64,
    crest: Option<f64>,
    crest_width: Option<f64>,
) -> ThreadProfile {
    let crest_width = crest_width.or(crest).unwrap_or(_pitch * 0.25);
    let base_width = crest_width + depth * (load_flank.tan() + return_flank.tan());
    ThreadProfile {
        base_width,
        crest_width,
        lower_flank: load_flank,
        upper_flank: return_flank,
    }
}

fn thread_profile_printability_diagnostic(
    pitch: f64,
    base_width: f64,
    clearance: f64,
) -> Option<String> {
    (pitch <= base_width + clearance.max(0.0)).then(|| {
        "Printability diagnostic: thread turns merge (pitch <= base + clearance).".to_string()
    })
}

/// Surface printability advisories without rejecting a renderable thread.
/// Kept beside native expansion so native and build123d share the same guard.
pub(crate) fn thread_printability_warnings(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AuthoringResult<Vec<String>> {
    // Component expansion introduces hygienic local aliases (`##name2`) whose
    // values are resolved by the Direct OCCT normalization pass. Inspect the
    // same normalized Core consumed by the planner; walking authored Core here
    // makes a successfully rendered component fail later while building its
    // advisory manifest.
    let normalized =
        super::direct_occt_normalize::normalize_core_program_for_direct_occt(program, parameters)
            .map_err(|err| planner_dependency_error("printability normalization failed", err))?;
    let program = &normalized;
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let env = core_param_env(program, parameters)?;
    let node_env = BTreeMap::new();
    let mut warnings = Vec::new();
    for part in &program.parts {
        collect_thread_printability_warnings(
            &part.root,
            &param_names,
            &env,
            &node_env,
            &mut warnings,
        )?;
    }
    warnings.sort();
    warnings.dedup();
    Ok(warnings)
}

fn contains_thread_geometry(node: &CoreNode) -> bool {
    match &node.kind {
        CoreNodeKind::Call { op, args, keywords } => {
            matches!(op, CoreOperation::Custom(name) if name == "thread")
                || args.iter().any(contains_thread_geometry)
                || keywords
                    .iter()
                    .any(|arg| contains_thread_geometry(arg.source_node()))
        }
        CoreNodeKind::Build { bindings, result } => {
            bindings
                .iter()
                .any(|binding| contains_thread_geometry(&binding.value))
                || contains_thread_geometry(result)
        }
        CoreNodeKind::Let { bindings, body } => {
            bindings
                .iter()
                .any(|binding| contains_thread_geometry(&binding.value))
                || contains_thread_geometry(body)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            contains_thread_geometry(condition)
                || contains_thread_geometry(then_branch)
                || contains_thread_geometry(else_branch)
        }
        CoreNodeKind::Map { sources, body, .. } => {
            sources.iter().any(contains_thread_geometry) || contains_thread_geometry(body)
        }
        CoreNodeKind::Apply { args, list, .. } => {
            args.iter().any(contains_thread_geometry) || contains_thread_geometry(list)
        }
        CoreNodeKind::Range { start, end } => {
            contains_thread_geometry(start) || contains_thread_geometry(end)
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().any(contains_thread_geometry)
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => false,
    }
}

fn collect_thread_printability_warnings(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    warnings: &mut Vec<String>,
) -> AuthoringResult<()> {
    // A thread-specific advisory must not execute unrelated deferred helpers.
    // Map callback locals exist only during geometry planning, not this walk.
    if !contains_thread_geometry(node) {
        return Ok(());
    }
    match &node.kind {
        CoreNodeKind::Call {
            op: CoreOperation::Custom(name),
            keywords,
            ..
        } if name == "thread" => {
            let (pitch, depth) = if let Some(designation) = keyword_text(keywords, "iso") {
                let (_, pitch, depth) = crate::ecky_core_ir::iso_metric_thread_core(&designation)
                    .ok_or_else(|| {
                    bk_constrained(
                        "thread",
                        format!("`thread` unknown ISO designation `{designation}`"),
                        &[],
                    )
                })?;
                (pitch, depth)
            } else {
                (
                    positive_keyword_number(
                        keywords,
                        "pitch",
                        "thread",
                        param_names,
                        env,
                        node_env,
                    )?,
                    positive_keyword_number(
                        keywords,
                        "depth",
                        "thread",
                        param_names,
                        env,
                        node_env,
                    )?,
                )
            };
            let optional = |name| -> AuthoringResult<Option<f64>> {
                keywords
                    .iter()
                    .any(|keyword| keyword.name == name)
                    .then(|| {
                        optional_keyword_number(
                            keywords,
                            name,
                            0.0,
                            "thread",
                            param_names,
                            env,
                            node_env,
                        )
                    })
                    .transpose()
            };
            let profile = derive_thread_profile(
                pitch,
                depth,
                optional("flank")?.map(f64::to_radians),
                optional("crest")?,
                optional("base-width")?,
                optional("crest-width")?,
            );
            let clearance = optional("clearance")?.unwrap_or(0.0);
            if let Some(warning) =
                thread_profile_printability_diagnostic(pitch, profile.base_width, clearance)
            {
                warnings.push(warning);
            }
        }
        CoreNodeKind::Build { bindings, result } => {
            let mut nested_env = env.clone();
            let mut nested_node_env = node_env.clone();
            for binding in bindings {
                collect_thread_printability_warnings(
                    &binding.value,
                    param_names,
                    &nested_env,
                    &nested_node_env,
                    warnings,
                )?;
                if let Some(param_value) = eval_scalar_binding_for_direct_occt(
                    &binding.value,
                    param_names,
                    &nested_env,
                    &nested_node_env,
                )? {
                    nested_env.insert(binding.name.clone(), param_value.clone());
                    nested_node_env.insert(binding.value.id.raw(), param_value);
                    record_scalar_node_values_for_direct_occt(
                        &binding.value,
                        param_names,
                        &nested_env,
                        &mut nested_node_env,
                    );
                }
            }
            collect_thread_printability_warnings(
                result,
                param_names,
                &nested_env,
                &nested_node_env,
                warnings,
            )?;
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested_env = env.clone();
            let mut nested_node_env = node_env.clone();
            for binding in bindings {
                collect_thread_printability_warnings(
                    &binding.value,
                    param_names,
                    &nested_env,
                    &nested_node_env,
                    warnings,
                )?;
                if let Some(param_value) = eval_scalar_binding_for_direct_occt(
                    &binding.value,
                    param_names,
                    &nested_env,
                    &nested_node_env,
                )? {
                    nested_env.insert(binding.name.clone(), param_value.clone());
                    nested_node_env.insert(binding.value.id.raw(), param_value);
                    record_scalar_node_values_for_direct_occt(
                        &binding.value,
                        param_names,
                        &nested_env,
                        &mut nested_node_env,
                    );
                }
            }
            collect_thread_printability_warnings(
                body,
                param_names,
                &nested_env,
                &nested_node_env,
                warnings,
            )?;
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_thread_printability_warnings(condition, param_names, env, node_env, warnings)?;
            collect_thread_printability_warnings(
                then_branch,
                param_names,
                env,
                node_env,
                warnings,
            )?;
            collect_thread_printability_warnings(
                else_branch,
                param_names,
                env,
                node_env,
                warnings,
            )?;
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                collect_thread_printability_warnings(arg, param_names, env, node_env, warnings)?;
            }
            for keyword in keywords {
                collect_thread_printability_warnings(
                    keyword.source_node(),
                    param_names,
                    env,
                    node_env,
                    warnings,
                )?;
            }
        }
        CoreNodeKind::Range { start, end } => {
            collect_thread_printability_warnings(start, param_names, env, node_env, warnings)?;
            collect_thread_printability_warnings(end, param_names, env, node_env, warnings)?;
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                collect_thread_printability_warnings(source, param_names, env, node_env, warnings)?;
            }
            collect_thread_printability_warnings(body, param_names, env, node_env, warnings)?;
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                collect_thread_printability_warnings(arg, param_names, env, node_env, warnings)?;
            }
            collect_thread_printability_warnings(list, param_names, env, node_env, warnings)?;
        }
        CoreNodeKind::List(nodes) | CoreNodeKind::Group(nodes) => {
            for child in nodes {
                collect_thread_printability_warnings(child, param_names, env, node_env, warnings)?;
            }
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => {}
    }
    Ok(())
}

fn expand_thread_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AuthoringResult<CoreNode> {
    if !args.is_empty() {
        return Err(bk_op(
            AuthoringReason::Unsupported,
            "thread",
            "`thread` expects keyword options only.",
        ));
    }
    reject_unknown_keywords(
        keywords,
        &[
            "iso",
            "radius",
            "top-radius",
            "pitch",
            "length",
            "depth",
            "flank",
            "profile",
            "load-flank",
            "return-flank",
            "crest",
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
    let optional_thread_keyword = |name| -> AuthoringResult<Option<f64>> {
        if keywords.iter().any(|keyword| keyword.name == name) {
            Ok(Some(optional_keyword_number(
                keywords,
                name,
                0.0,
                "thread",
                param_names,
                env,
                node_env,
            )?))
        } else {
            Ok(None)
        }
    };
    let top_radius = optional_thread_keyword("top-radius")?.unwrap_or(radius);
    if !top_radius.is_finite() || top_radius <= 0.0 {
        return Err(bk_op(
            AuthoringReason::Type,
            "thread",
            "`thread` top-radius must be positive and finite.",
        ));
    }
    let profile_mode = keyword_symbol_or_text(keywords, "profile");
    let asymmetric_profile = matches!(profile_mode.as_deref(), Some("buttress"));
    let profile = match profile_mode.as_deref() {
        None | Some("sym") | Some("symmetric") => derive_thread_profile(
            pitch,
            depth,
            optional_thread_keyword("flank")?.map(f64::to_radians),
            optional_thread_keyword("crest")?,
            optional_thread_keyword("base-width")?,
            optional_thread_keyword("crest-width")?,
        ),
        Some("buttress") => derive_buttress_thread_profile(
            pitch,
            depth,
            positive_keyword_number(keywords, "load-flank", "thread", param_names, env, node_env)?
                .to_radians(),
            positive_keyword_number(
                keywords,
                "return-flank",
                "thread",
                param_names,
                env,
                node_env,
            )?
            .to_radians(),
            optional_thread_keyword("crest")?,
            optional_thread_keyword("crest-width")?,
        ),
        Some(other) => {
            return Err(bk_constrained(
                "thread",
                format!("`thread :profile` does not recognize `{other}`."),
                &["sym", "buttress"],
            ))
        }
    };
    let base_width = profile.base_width;
    let crest_width = profile.crest_width;
    let female = optional_keyword_bool(
        keywords,
        "female",
        false,
        "thread",
        param_names,
        env,
        node_env,
    )?;
    let lefthand = optional_keyword_bool(
        keywords,
        "lefthand",
        false,
        "thread",
        param_names,
        env,
        node_env,
    )?;
    let clearance = optional_keyword_number(
        keywords,
        "clearance",
        0.0,
        "thread",
        param_names,
        env,
        node_env,
    )?
    .max(0.0);

    // Bury the ridge root inside the core by `overlap` so the `union(core, ridge)`
    // below never shares a coincident cylinder face. Without this the boolean
    // drops the core on coarse/deep threads, leaving a hollow spiral. The ridge
    // root moves in by `overlap` and its depth grows by `overlap`, so the crest
    // (major = radius + depth) and the core surface (minor = radius) are
    // unchanged — only the buried part of the ridge differs.
    let overlap = 0.3_f64.min(radius.min(top_radius) * 0.5).min(depth);
    let ridge_radius = radius - overlap;
    let ridge_top_radius = top_radius - overlap;
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
        CoreKeywordArg::expr(
            "radius".to_string(),
            number_node(next_node_id, ridge_radius),
        ),
        CoreKeywordArg::expr("pitch".to_string(), number_node(next_node_id, pitch)),
        CoreKeywordArg::expr("height".to_string(), number_node(next_node_id, length)),
        CoreKeywordArg::expr(
            "base-width".to_string(),
            number_node(next_node_id, base_width),
        ),
        CoreKeywordArg::expr(
            "crest-width".to_string(),
            number_node(next_node_id, crest_width),
        ),
        CoreKeywordArg::expr("depth".to_string(), number_node(next_node_id, ridge_depth)),
        CoreKeywordArg::expr("lefthand".to_string(), bool_node(next_node_id, lefthand)),
    ];
    if top_radius != radius {
        ridge_keywords.push(CoreKeywordArg::expr(
            "top-radius".to_string(),
            number_node(next_node_id, ridge_top_radius),
        ));
    }
    if asymmetric_profile {
        ridge_keywords.push(CoreKeywordArg::expr(
            "lower-flank".to_string(),
            number_node(next_node_id, profile.lower_flank.to_degrees()),
        ));
        ridge_keywords.push(CoreKeywordArg::expr(
            "upper-flank".to_string(),
            number_node(next_node_id, profile.upper_flank.to_degrees()),
        ));
    }
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

    // The swept profile is centered on both helix endpoints, so its raw solid
    // extends below z=0 and above `length`. Intersect the fused male solid with
    // the named major envelope. Clipping after fusion keeps the buried root
    // overlap intact during union, then trims both end spill and any sweep
    // tolerance outside the intended radial boundary. Female threads return
    // above as subtraction cutters and remain naturally bounded by their host.
    let major_radius = radius + depth;
    let major_top_radius = top_radius + depth;
    let core = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: if top_radius == radius {
                CoreOperation::Primitive(CorePrimitive::Cylinder)
            } else {
                CoreOperation::Primitive(CorePrimitive::Cone)
            },
            args: if top_radius == radius {
                vec![
                    number_node(next_node_id, radius),
                    number_node(next_node_id, length),
                ]
            } else {
                vec![
                    number_node(next_node_id, radius),
                    number_node(next_node_id, top_radius),
                    number_node(next_node_id, length),
                ]
            },
            keywords: Vec::new(),
        },
        CoreValueKind::Solid,
    );

    let unbounded_thread = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Boolean(CoreBooleanOp::Union),
            args: vec![core, ridge],
            keywords: Vec::new(),
        },
        CoreValueKind::Solid,
    );

    Ok(rebuild_node(
        node,
        CoreNodeKind::Call {
            op: CoreOperation::Custom("clip-thread-envelope".to_string()),
            args: vec![
                number_node(next_node_id, major_radius),
                number_node(next_node_id, major_top_radius),
                number_node(next_node_id, length),
                unbounded_thread,
            ],
            keywords: Vec::new(),
        },
    ))
}

/// Expand a `tapped-hole` into a positive tapped-thread cavity: a named-radius
/// bore cylinder at the ISO minor diameter, unioned with a helical relief ridge
/// whose crest reaches the major diameter. This mirrors `expand_thread_node`'s
/// male path with `radius := minor`: the bore plays the role of the core and the
/// relief ridge is the same helical-ridge tooth (pointing outward into the
/// material). An equal-nominal external `thread` therefore mates with it.
///
/// `iso_metric_thread_core` returns `(minor, pitch, depth)` where `minor` is
/// `major/2 - depth`; the bore sits at `minor` and the ridge crest lands at
/// `minor + depth = major`. The `overlap` is a NAMED, bounded value (never an
/// anonymous fit offset) so `union(bore, ridge)` never shares a coincident
/// cylinder face, identical to the male-thread rule.
fn expand_tapped_hole_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AuthoringResult<CoreNode> {
    if !args.is_empty() {
        return Err(bk_op(
            AuthoringReason::Unsupported,
            "tapped-hole",
            "`tapped-hole` expects keyword options only.",
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
            "lefthand",
        ],
        "tapped-hole",
    )?;

    let length = positive_keyword_number(
        keywords,
        "length",
        "tapped-hole",
        param_names,
        env,
        node_env,
    )?;
    // `iso_metric_thread_core` returns (minor radius, pitch, depth-to-major).
    // `minor` is a named value: the bore radius and the ridge root reference.
    let (minor, pitch, depth) = if let Some(designation) = keyword_text(keywords, "iso") {
        crate::ecky_core_ir::iso_metric_thread_core(&designation).ok_or_else(|| {
            bk_constrained(
                "tapped-hole",
                format!("`tapped-hole` unknown ISO designation `{designation}`"),
                &["M3", "M4", "M5", "M6", "M8", "M10", "M12", "M16", "M20"],
            )
        })?
    } else {
        (
            positive_keyword_number(
                keywords,
                "radius",
                "tapped-hole",
                param_names,
                env,
                node_env,
            )?,
            positive_keyword_number(keywords, "pitch", "tapped-hole", param_names, env, node_env)?,
            positive_keyword_number(keywords, "depth", "tapped-hole", param_names, env, node_env)?,
        )
    };
    let base_width = optional_keyword_number(
        keywords,
        "base-width",
        pitch * 0.75,
        "tapped-hole",
        param_names,
        env,
        node_env,
    )?;
    let crest_width = optional_keyword_number(
        keywords,
        "crest-width",
        pitch * 0.25,
        "tapped-hole",
        param_names,
        env,
        node_env,
    )?;
    let lefthand = optional_keyword_bool(
        keywords,
        "lefthand",
        false,
        "tapped-hole",
        param_names,
        env,
        node_env,
    )?;

    // Named, bounded overlap (same rule as the external `thread`): bury the
    // ridge root inside the bore by `overlap` so the union never shares a
    // coincident cylinder face. The ridge root moves in by `overlap` and its
    // depth grows by `overlap`, so the crest (major = minor + depth) and the
    // bore surface (minor) are unchanged — only the buried part differs.
    // The sampled helical loft needs substantial radial burial before OCCT's
    // fuse recognizes a shared volume. Crest radius stays unchanged because
    // ridge depth grows by the same named overlap.
    let overlap = 0.8_f64.min(minor * 0.5).min(depth);
    let ridge_radius = minor - overlap;
    let ridge_depth = depth + overlap;

    let bool_node = |next: &mut u64, value: bool| {
        CoreNode::new(
            next_id(next),
            CoreNodeKind::Literal(CoreLiteral::Boolean(value)),
            CoreValueKind::Boolean,
        )
    };
    let ridge_keywords = vec![
        CoreKeywordArg::expr(
            "radius".to_string(),
            number_node(next_node_id, ridge_radius),
        ),
        CoreKeywordArg::expr("pitch".to_string(), number_node(next_node_id, pitch)),
        CoreKeywordArg::expr("height".to_string(), number_node(next_node_id, length)),
        CoreKeywordArg::expr(
            "base-width".to_string(),
            number_node(next_node_id, base_width),
        ),
        CoreKeywordArg::expr(
            "crest-width".to_string(),
            number_node(next_node_id, crest_width),
        ),
        CoreKeywordArg::expr("depth".to_string(), number_node(next_node_id, ridge_depth)),
        CoreKeywordArg::expr("lefthand".to_string(), bool_node(next_node_id, lefthand)),
    ];
    let mut ridge = expand_helical_ridge_node(
        node,
        &[],
        &ridge_keywords,
        param_names,
        env,
        node_env,
        next_node_id,
    )?;

    // `expand_helical_ridge_node` rebuilds onto `node`'s id; the bore `union`
    // below also rebuilds onto `node`'s id. Re-id the ridge so the two get
    // distinct slots (mirrors the male-thread expansion).
    ridge.id = next_id(next_node_id);

    let bore = CoreNode::new(
        next_id(next_node_id),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Cylinder),
            args: vec![
                number_node(next_node_id, minor),
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
            args: vec![bore, ridge],
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
) -> AuthoringResult<CoreNode> {
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
) -> AuthoringResult<CoreNode> {
    if args.len() != 2 {
        return Err(bk_arity(
            "regular-polygon",
            "sides and radius (plus optional `:rotation`)",
        ));
    }
    reject_unknown_keywords(keywords, &["rotation"], "regular-polygon")?;

    let sides = eval_core_number(&args[0], param_names, env)?;
    let radius = eval_core_number(&args[1], param_names, env)?;
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
        return Err(bk_constrained(
            "regular-polygon",
            "`regular-polygon` needs at least 3 sides.",
            &["3", "4", "5", "6", "8"],
        ));
    }
    if radius.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return Err(bk_op(
            AuthoringReason::Type,
            "regular-polygon",
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

fn expand_voronoi_cell_node(
    node: &CoreNode,
    args: &[CoreNode],
    keywords: &[CoreKeywordArg],
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
    next_node_id: &mut u64,
) -> AuthoringResult<CoreNode> {
    if args.len() != 5 {
        return Err(bk_arity(
            "voronoi-cell",
            "sites, index, width, height, and inset",
        ));
    }
    reject_unknown_keywords(keywords, &[], "voronoi-cell")?;

    let sites_node =
        expand_node_for_direct_occt(&args[0], param_names, env, node_env, next_node_id)?;
    let CoreNodeKind::List(site_nodes) = &sites_node.kind else {
        return Err(bk_op(
            AuthoringReason::Type,
            "voronoi-cell",
            "`voronoi-cell` sites must evaluate to a point2 list.",
        ));
    };
    let sites = site_nodes
        .iter()
        .enumerate()
        .map(|(site_index, site)| {
            let CoreNodeKind::List(coordinates) = &site.kind else {
                return Err(bk_op(
                    AuthoringReason::Type,
                    "voronoi-cell",
                    format!("`voronoi-cell` site {site_index} must be point2."),
                ));
            };
            if coordinates.len() != 2 {
                return Err(bk_op(
                    AuthoringReason::Type,
                    "voronoi-cell",
                    format!("`voronoi-cell` site {site_index} must have two coordinates."),
                ));
            }
            Ok([
                eval_number_for_direct_occt(&coordinates[0], param_names, env, node_env)?,
                eval_number_for_direct_occt(&coordinates[1], param_names, env, node_env)?,
            ])
        })
        .collect::<AuthoringResult<Vec<_>>>()?;

    let index = eval_number_for_direct_occt(&args[1], param_names, env, node_env)?;
    if !index.is_finite() || index < 0.0 || index.fract().abs() > 1.0e-9 {
        return Err(bk_op(
            AuthoringReason::Type,
            "voronoi-cell",
            "`voronoi-cell` index must be a non-negative integer.",
        ));
    }
    let width = eval_number_for_direct_occt(&args[2], param_names, env, node_env)?;
    let height = eval_number_for_direct_occt(&args[3], param_names, env, node_env)?;
    let inset = eval_number_for_direct_occt(&args[4], param_names, env, node_env)?;
    let points = crate::ecky_deterministic::voronoi_cell_polygon(
        &sites,
        index as usize,
        width,
        height,
        inset,
    )
    .map_err(|message| bk_op(AuthoringReason::Type, "voronoi-cell", message))?;
    let point_nodes = points
        .into_iter()
        .map(|point| {
            CoreNode::new(
                next_id(next_node_id),
                CoreNodeKind::Literal(CoreLiteral::Point2(point)),
                CoreValueKind::Point2,
            )
        })
        .collect();
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
) -> AuthoringResult<CoreNode> {
    if args.len() != 3 {
        return Err(bk_arity(
            "trapezoid",
            "bottom, top, and height (plus optional `:skew`)",
        ));
    }
    reject_unknown_keywords(keywords, &["skew"], "trapezoid")?;

    let bottom = eval_core_number(&args[0], param_names, env)?;
    let top = eval_core_number(&args[1], param_names, env)?;
    let height = eval_core_number(&args[2], param_names, env)?;
    let skew = optional_keyword_number(
        keywords,
        "skew",
        0.0,
        "trapezoid",
        param_names,
        env,
        node_env,
    )?;

    if bottom.partial_cmp(&0.0) != Some(Ordering::Greater)
        || top.partial_cmp(&0.0) != Some(Ordering::Greater)
    {
        return Err(bk_op(
            AuthoringReason::Type,
            "trapezoid",
            "`trapezoid` bottom and top must be positive.",
        ));
    }
    if height.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return Err(bk_op(
            AuthoringReason::Type,
            "trapezoid",
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
) -> AuthoringResult<CoreNode> {
    if args.len() != 2 {
        return Err(bk_arity(
            "slot-center-to-center",
            "center separation and width",
        ));
    }
    reject_unknown_keywords(keywords, &[], "slot-center-to-center")?;

    let separation = eval_core_number(&args[0], param_names, env)?;
    let width = eval_core_number(&args[1], param_names, env)?;
    if width.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return Err(bk_op(
            AuthoringReason::Type,
            "slot-center-to-center",
            "`slot-center-to-center` width must be positive.",
        ));
    }
    if matches!(separation.partial_cmp(&0.0), None | Some(Ordering::Less)) {
        return Err(bk_op(
            AuthoringReason::Type,
            "slot-center-to-center",
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
) -> AuthoringResult<CoreNode> {
    if args.len() != 5 {
        return Err(bk_arity("slot-center-point", "cx, cy, px, py, width"));
    }
    reject_unknown_keywords(keywords, &[], "slot-center-point")?;

    let cx = eval_core_number(&args[0], param_names, env)?;
    let cy = eval_core_number(&args[1], param_names, env)?;
    let px = eval_core_number(&args[2], param_names, env)?;
    let py = eval_core_number(&args[3], param_names, env)?;
    let width = eval_core_number(&args[4], param_names, env)?;
    if width.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return Err(bk_op(
            AuthoringReason::Type,
            "slot-center-point",
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
    let gap =
        ((cursor[0] - geometry.start[0]).powi(2) + (cursor[1] - geometry.start[1]).powi(2)).sqrt();
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
) -> AuthoringResult<CoreNode> {
    if args.len() != 1 {
        return Err(bk_arity(
            "sampled-radial-loft",
            "binder names plus keyword/value options",
        ));
    }
    let binders = sampled_radial_loft_binders(&args[0])?;
    let height_node = sampled_keyword_node(keywords, "height")?;
    let z_steps_node = sampled_keyword_node(keywords, "z-steps")?;
    let theta_steps_node = sampled_keyword_node(keywords, "theta-steps")?;
    let radius_node = sampled_keyword_node(keywords, "radius")?;
    let z_map_node = sampled_optional_keyword_node(keywords, "z-map");

    let height = eval_core_number(height_node, param_names, env)?;
    let z_steps = sampled_count(
        eval_core_number(z_steps_node, param_names, env)?,
        1,
        "z-steps",
    )?;
    let theta_steps = sampled_count(
        eval_core_number(theta_steps_node, param_names, env)?,
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
            let radius = eval_core_number(radius_node, param_names, &section_env)?;
            if !radius.is_finite() || radius <= 0.0 {
                return Err(bk_op(
                    AuthoringReason::Type,
                    "sampled-radial-loft",
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
            .map(|z_map| eval_core_number(z_map, param_names, &section_env))
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
) -> AuthoringResult<CoreNode> {
    if !keywords.is_empty() || args.len() != 2 {
        return Err(bk_arity(
            "shell",
            "thickness and shape only (sampled-radial-loft)",
        ));
    }
    let target = sampled_radial_loft_target(args).ok_or_else(|| {
        bk_op(
            AuthoringReason::Type,
            "shell",
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

fn sampled_radial_loft_binders(arg: &CoreNode) -> AuthoringResult<[String; 3]> {
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

fn sampled_binder_name(node: &CoreNode) -> AuthoringResult<String> {
    match &node.kind {
        CoreNodeKind::Reference(CoreReference::Local(name)) => Ok(name.clone()),
        CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(text.clone()),
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => Ok(symbol_name(symbol).to_string()),
        _ => Err(bk_op(
            AuthoringReason::Type,
            "sampled-radial-loft",
            "`sampled-radial-loft` binders must be symbols.",
        )),
    }
}

fn sampled_keyword_node<'a>(
    keywords: &'a [CoreKeywordArg],
    name: &str,
) -> AuthoringResult<&'a CoreNode> {
    sampled_optional_keyword_node(keywords, name).ok_or_else(|| {
        bk_op(
            AuthoringReason::Arity,
            "sampled-radial-loft",
            format!("`sampled-radial-loft` requires `:{name}`."),
        )
    })
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

fn sampled_count(value: f64, minimum: usize, label: &str) -> AuthoringResult<usize> {
    if !value.is_finite() {
        return Err(bk_op(
            AuthoringReason::Type,
            "sampled-radial-loft",
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
) -> AuthoringResult<Option<ParamValue>> {
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
) -> AuthoringResult<f64> {
    let node = rewrite_eval_node_for_direct_occt(node, env, node_env);
    eval_core_number(&node, param_names, env).map_err(|err| {
        bk(
            AuthoringReason::Type,
            format!("could not evaluate numeric Core node {:?}: {err}", node.id),
        )
    })
}

fn eval_bool_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AuthoringResult<bool> {
    let node = rewrite_eval_node_for_direct_occt(node, env, node_env);
    eval_core_bool(&node, param_names, env)
}

fn eval_stringish_for_direct_occt(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AuthoringResult<String> {
    let node = rewrite_eval_node_for_direct_occt(node, env, node_env);
    eval_core_stringish(&node, param_names, env)
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
) -> AuthoringResult<&'a CoreNode> {
    keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .map(|keyword| keyword.source_node())
        .ok_or_else(|| {
            bk_op(
                AuthoringReason::Arity,
                op,
                format!("`{op}` requires `:{name}`."),
            )
        })
}

fn positive_keyword_number(
    keywords: &[CoreKeywordArg],
    name: &str,
    op: &str,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AuthoringResult<f64> {
    let value = eval_number_for_direct_occt(
        required_keyword_node(keywords, name, op)?,
        param_names,
        env,
        node_env,
    )?;
    if !value.is_finite() || value <= 0.0 {
        return Err(bk_op(
            AuthoringReason::Type,
            op,
            format!("`{op}` {name} must be positive and finite."),
        ));
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
) -> AuthoringResult<f64> {
    let Some(node) = keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .map(|keyword| keyword.source_node())
    else {
        return Ok(default);
    };
    eval_number_for_direct_occt(node, param_names, env, node_env).map_err(|err| {
        bk_op(
            AuthoringReason::Type,
            op,
            format!("`{op}` could not evaluate `:{name}`: {err}"),
        )
    })
}

fn optional_keyword_bool(
    keywords: &[CoreKeywordArg],
    name: &str,
    default: bool,
    op: &str,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
    node_env: &BTreeMap<u64, ParamValue>,
) -> AuthoringResult<bool> {
    let Some(node) = keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .map(|keyword| keyword.source_node())
    else {
        return Ok(default);
    };
    eval_bool_for_direct_occt(node, param_names, env, node_env).map_err(|err| {
        bk_op(
            AuthoringReason::Type,
            op,
            format!("`{op}` could not evaluate `:{name}`: {err}"),
        )
    })
}

fn reject_unknown_keywords(
    keywords: &[CoreKeywordArg],
    allowed: &[&str],
    op: &str,
) -> AuthoringResult<()> {
    for keyword in keywords {
        if allowed
            .iter()
            .any(|allowed_name| *allowed_name == keyword.name)
        {
            continue;
        }
        return Err(bk_op(
            AuthoringReason::Arity,
            op,
            format!("`{op}` does not recognize `:{}`.", keyword.name),
        ));
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
    authored_shape_bindings: Vec<(String, OcctSlot)>,
    requested_let_bindings: BTreeSet<String>,
}

impl<'a> PartPlanner<'a> {
    fn new(
        param_names: &'a BTreeMap<u64, String>,
        scalar_env: &'a BTreeMap<String, ParamValue>,
        next_node_id: u64,
        requested_let_bindings: BTreeSet<String>,
    ) -> Self {
        Self {
            param_names,
            scalar_env: scalar_env.clone(),
            scalar_node_values: BTreeMap::new(),
            node_refs: BTreeMap::new(),
            locals: BTreeMap::new(),
            next_node_id,
            commands: Vec::new(),
            authored_shape_bindings: Vec::new(),
            requested_let_bindings,
        }
    }

    fn scalar_env_snapshot(&self) -> BTreeMap<String, ParamValue> {
        let mut env = self.scalar_env.clone();
        for (name, arg) in &self.locals {
            let scalar = match arg {
                OcctArg::Param(key) => self.scalar_env.get(key).cloned(),
                _ => occt_arg_to_scalar(arg),
            };
            if let Some(value) = scalar {
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

    fn plan_node(&mut self, node: &CoreNode) -> AuthoringResult<OcctSlot> {
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
                    .collect::<AuthoringResult<Vec<_>>>()?;
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
                    .collect::<AuthoringResult<Vec<_>>>()?;
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
                return Err(unsupported_authoring(
                    "if",
                    "branching Core IR needs runtime selection before direct OCCT planning",
                ));
            }
            CoreNodeKind::Reference(_) => match self.plan_arg(node)? {
                OcctArg::Ref(slot) => slot,
                other => {
                    return Err(planner_error(
                        AuthoringReason::Type,
                        format!(
                            "Direct OCCT adapter expected geometry reference, got {:?}.",
                            other
                        ),
                    ));
                }
            },
            _ => {
                return Err(planner_error(
                    AuthoringReason::Type,
                    format!(
                        "Direct OCCT adapter expected geometry node, got {:?}.",
                        node.kind
                    ),
                ));
            }
        };

        self.node_refs.insert(node.id.raw(), slot);
        Ok(slot)
    }

    fn plan_build(
        &mut self,
        bindings: &[CoreShapeBinding],
        result: &CoreNode,
    ) -> AuthoringResult<OcctSlot> {
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
                self.authored_shape_bindings
                    .push((binding.name.clone(), slot));
            }
        }
        let root = self.plan_node(result);
        self.locals = saved_locals;
        root
    }

    fn plan_let(&mut self, bindings: &[CoreBinding], body: &CoreNode) -> AuthoringResult<OcctSlot> {
        let saved_locals = self.locals.clone();
        for binding in bindings {
            let value = self.plan_arg(&binding.value)?;
            if let Some(scalar) = occt_arg_to_scalar(&value) {
                self.scalar_env.insert(binding.name.clone(), scalar);
                self.scalar_node_values
                    .insert(binding.value.id.raw(), value.clone());
            }
            if let OcctArg::Ref(slot) = &value {
                if let Some(authored_name) = self.requested_let_bindings.iter().find(|requested| {
                    binding.name == requested.as_str()
                        || binding
                            .name
                            .strip_prefix(&format!("##{requested}"))
                            .is_some_and(|suffix| {
                                !suffix.is_empty()
                                    && suffix.chars().all(|character| character.is_ascii_digit())
                            })
                }) {
                    self.authored_shape_bindings
                        .push((authored_name.clone(), *slot));
                }
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
    ) -> AuthoringResult<OcctSlot> {
        let output = OcctSlot(node.id.raw());
        let mut planned_args = args
            .iter()
            .map(|arg| self.plan_arg(arg))
            .collect::<AuthoringResult<Vec<_>>>()?;
        let list_arg = self.plan_arg(list)?;
        let OcctArg::List(items) = list_arg else {
            return Err(planner_error(
                AuthoringReason::Type,
                format!(
                    "Direct OCCT adapter `apply` expected list argument, got {:?}.",
                    list_arg
                ),
            ));
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

    fn plan_arg(&mut self, node: &CoreNode) -> AuthoringResult<OcctArg> {
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
                    planner_error(
                        AuthoringReason::Type,
                        format!("Direct OCCT adapter could not resolve parameter {:?}.", id),
                    )
                })?;
                Ok(OcctArg::Param(name))
            }
            CoreNodeKind::Reference(CoreReference::Node(id)) => {
                if let Some(value) = self.scalar_node_values.get(&id.raw()).cloned() {
                    return Ok(value);
                }
                let slot = self.node_refs.get(&id.raw()).copied().ok_or_else(|| {
                    planner_error(
                        AuthoringReason::Type,
                        format!(
                            "Direct OCCT adapter could not resolve Core node reference {:?}.",
                            id
                        ),
                    )
                })?;
                Ok(OcctArg::Ref(slot))
            }
            CoreNodeKind::Reference(CoreReference::Local(name)) => self
                .locals
                .get(name)
                .cloned()
                .or_else(|| match name.as_str() {
                    "pi" => Some(OcctArg::Number(std::f64::consts::PI)),
                    "tau" => Some(OcctArg::Number(std::f64::consts::TAU)),
                    _ => None,
                })
                .ok_or_else(|| {
                    planner_error(
                        AuthoringReason::Type,
                        format!("Direct OCCT adapter could not resolve local `{}`.", name),
                    )
                }),
            CoreNodeKind::List(items) | CoreNodeKind::Group(items) => Ok(OcctArg::List(
                items
                    .iter()
                    .map(|item| self.plan_arg(item))
                    .collect::<AuthoringResult<Vec<_>>>()?,
            )),
            CoreNodeKind::Range { start, end } => self.plan_range_arg(start, end),
            CoreNodeKind::Map {
                params,
                sources,
                body,
            } => self.plan_map_arg(params, sources, body),
            CoreNodeKind::Let { bindings, body } => self.plan_let_arg(bindings, body),
            CoreNodeKind::Build { bindings, result } => self.plan_build_arg(bindings, result),
            CoreNodeKind::Apply {
                op: CoreOperation::Custom(name),
                args,
                list,
            } if name == "append" => {
                let mut lists = args
                    .iter()
                    .map(|arg| self.plan_arg(arg))
                    .collect::<AuthoringResult<Vec<_>>>()?;
                match self.plan_arg(list)? {
                    OcctArg::List(items) => lists.extend(items),
                    other => {
                        return Err(planner_error(
                            AuthoringReason::Type,
                            format!(
                                "Direct OCCT adapter `apply append` expected list, got {other:?}."
                            ),
                        ))
                    }
                }
                let mut combined = Vec::new();
                for value in lists {
                    match value {
                        OcctArg::List(items) => combined.extend(items),
                        other => return Err(planner_error(AuthoringReason::Type,
                            format!("Direct OCCT adapter `apply append` expected list item, got {other:?}."))),
                    }
                }
                Ok(OcctArg::List(combined))
            }
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
                            return Err(planner_error(
                                AuthoringReason::Type,
                                format!(
                                "Direct OCCT adapter `append` expected list argument, got {:?}.",
                                other
                            ),
                            ))
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
                    return Err(planner_error(
                        AuthoringReason::Arity,
                        format!(
                            "Direct OCCT adapter `reverse` expected one list, got {} arguments.",
                            args.len()
                        ),
                    ));
                };
                match self.plan_arg(arg)? {
                    OcctArg::List(mut items) => {
                        items.reverse();
                        Ok(OcctArg::List(items))
                    }
                    other => Err(planner_error(
                        AuthoringReason::Type,
                        format!(
                            "Direct OCCT adapter `reverse` expected list argument, got {:?}.",
                            other
                        ),
                    )),
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
                    return Err(planner_error(
                        AuthoringReason::Arity,
                        format!(
                            "Direct OCCT adapter `{name}` expected one list, got {} arguments.",
                            args.len()
                        ),
                    ));
                };
                let items = match self.plan_arg(arg)? {
                    OcctArg::List(items) => items,
                    OcctArg::Point2(point) => point.iter().copied().map(OcctArg::Number).collect(),
                    OcctArg::Point3(point) => point.iter().copied().map(OcctArg::Number).collect(),
                    other => {
                        return Err(planner_error(
                            AuthoringReason::Type,
                            format!(
                                "Direct OCCT adapter `{name}` expected list argument, got {:?}.",
                                other
                            ),
                        ))
                    }
                };
                items.get(index).cloned().ok_or_else(|| {
                    planner_error(
                        AuthoringReason::Arity,
                        format!(
                            "Direct OCCT adapter `{name}` expected at least {} item(s), got {}.",
                            index + 1,
                            items.len()
                        ),
                    )
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
            CoreNodeKind::If { .. } => Err(planner_error(
                AuthoringReason::Unsupported,
                format!(
                "Direct OCCT adapter cannot plan dynamic expression node {:?} before evaluation.",
                node.kind
            ),
            )),
            CoreNodeKind::Reference(CoreReference::Part(id)) => Err(planner_error(
                AuthoringReason::Unsupported,
                format!(
                    "Direct OCCT adapter cannot plan part reference {:?} in first surface.",
                    id
                ),
            )),
        }
    }

    /// Clone `node` with list-accessor calls (`car`, `cadr`, ...) replaced by
    /// literal scalars resolved against planned locals, so the shared scalar
    /// evaluator can fold the surrounding arithmetic.
    fn substitute_list_accessors(&mut self, node: &CoreNode) -> AuthoringResult<CoreNode> {
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

    fn plan_align_arg(&mut self, node: &CoreNode) -> AuthoringResult<OcctArg> {
        let symbols = match &node.kind {
            CoreNodeKind::List(items) | CoreNodeKind::Group(items) => items
                .iter()
                .map(align_axis_arg)
                .collect::<AuthoringResult<Vec<_>>>()?,
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
                return Err(constrained_backend_error(
                    "align",
                    "Direct OCCT adapter `:align` expects `(min|center|max)^3`.",
                    &["min", "center", "max"],
                ));
            }
        };
        if symbols.len() != 3 {
            return Err(planner_arity_error("align", "exactly 3 axes"));
        }
        Ok(OcctArg::List(
            symbols
                .into_iter()
                .map(|symbol| OcctArg::Symbol(symbol.to_string()))
                .collect(),
        ))
    }

    fn plan_scalar_arg(&mut self, node: &CoreNode) -> AuthoringResult<Option<OcctArg>> {
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

    fn plan_range_arg(&mut self, start: &CoreNode, end: &CoreNode) -> AuthoringResult<OcctArg> {
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
    ) -> AuthoringResult<OcctArg> {
        if params.len() != sources.len() {
            return Err(planner_error(
                AuthoringReason::Arity,
                format!(
                    "Direct OCCT adapter map expected {} source list(s), got {}.",
                    params.len(),
                    sources.len()
                ),
            ));
        }
        let source_values = sources
            .iter()
            .map(|source| match self.plan_arg(source)? {
                OcctArg::List(items) => Ok(items),
                other => Err(planner_error(
                    AuthoringReason::Type,
                    format!(
                        "Direct OCCT adapter map expected list source, got {:?}.",
                        other
                    ),
                )),
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        let Some(first_source) = source_values.first() else {
            return Ok(OcctArg::List(Vec::new()));
        };
        let count = first_source.len();
        if source_values.iter().any(|source| source.len() != count) {
            return Err(planner_error(
                AuthoringReason::Type,
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
                let scalar_env = self.scalar_env_snapshot();
                let scalar_node_values = self.scalar_param_node_values();
                let expanded_body = expand_node_for_direct_occt(
                    &iteration_body,
                    self.param_names,
                    &scalar_env,
                    &scalar_node_values,
                    &mut self.next_node_id,
                )?;
                items.push(self.plan_arg(&expanded_body)?);
            }
            Ok(OcctArg::List(items))
        })();
        self.locals = saved_locals;
        result
    }

    fn plan_let_arg(
        &mut self,
        bindings: &[CoreBinding],
        body: &CoreNode,
    ) -> AuthoringResult<OcctArg> {
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
    ) -> AuthoringResult<OcctArg> {
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

fn align_axis_arg(node: &CoreNode) -> AuthoringResult<&'static str> {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => Ok(symbol_name(symbol)),
        CoreNodeKind::Call {
            op: CoreOperation::Custom(name),
            args,
            keywords,
        } if args.is_empty() && keywords.is_empty() => align_axis_name(name),
        _ => Err(constrained_backend_error(
            "align",
            "Direct OCCT adapter `:align` axes must be `min`, `center`, or `max`.",
            &["min", "center", "max"],
        )),
    }
}

fn align_axis_name(name: &str) -> AuthoringResult<&'static str> {
    match name {
        "min" => Ok("min"),
        "center" => Ok("center"),
        "max" => Ok("max"),
        _ => Err(constrained_backend_error(
            "align",
            format!("Direct OCCT adapter `:align` axis `{name}` is not supported."),
            &["min", "center", "max"],
        )),
    }
}

fn occt_op(op: &CoreOperation) -> AuthoringResult<OcctOp> {
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
        CoreOperation::Custom(name) if name == "import-step" => Ok(OcctOp::ImportStep),
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
        CoreOperation::Custom(name) if name == "clip-thread-envelope" => {
            Ok(OcctOp::ClipThreadEnvelope)
        }
        CoreOperation::Frame(CoreFrameOp::ClipPlane) => Ok(OcctOp::ClipPlane),
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
        CoreOperation::Custom(name) if name == "solidify" => Ok(OcctOp::Solidify),
        CoreOperation::Custom(name) if name == "hole" => Err(planner_op_error(
            AuthoringReason::Unsupported,
            "hole",
            "Typed hole must be filled before direct OCCT planning.",
        )),
        CoreOperation::Custom(name) => Err(unknown_core_ir_authoring(name)),
        _ => Err(unsupported_authoring(
            &operation_name(op),
            "not in first surface",
        )),
    }
}

fn typed_hole_error(keywords: &[CoreKeywordArg]) -> AuthoringError {
    let requested_type = keyword_text(keywords, "type").unwrap_or_else(|| "unknown".to_string());
    let goal = keyword_text(keywords, "goal").unwrap_or_else(|| "unspecified".to_string());
    planner_op_error(
        AuthoringReason::Unsupported,
        "hole",
        format!(
        "Typed hole requested type `{}` with goal `{}` must be filled before direct OCCT planning.",
        requested_type, goal
    ),
    )
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

fn keyword_symbol_or_text(keywords: &[CoreKeywordArg], name: &str) -> Option<String> {
    keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .and_then(|keyword| match &keyword.source_node().kind {
            CoreNodeKind::Literal(CoreLiteral::Text(text)) => Some(text.clone()),
            CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => {
                Some(symbol_name(symbol).to_string())
            }
            _ => None,
        })
}

fn unsupported_authoring(op: &str, reason: &str) -> AuthoringError {
    backend_op_error(
        AuthoringReason::Unsupported,
        op,
        format!("The active backend (direct OCCT) cannot execute `{op}`: {reason}."),
    )
    .with_fix(crate::contracts::ErrorFix {
        hint: Some(
            "switch to a backend that supports this operation, or replace it with an \
             equivalent the current backend can render."
                .into(),
        ),
        suggestions: Vec::new(),
    })
}

fn unknown_core_ir_authoring(op: &str) -> AuthoringError {
    let suggestions = crate::ecky_ir::op_suggest::suggest_ops(op);
    AuthoringError::core_ir(
        AuthoringReason::UnknownOp,
        format!("Unknown Core IR operation `{op}`."),
    )
    .with_op(op)
    .with_fix(ErrorFix {
        hint: Some(format!("replace `{op}` with a known Core IR operation.")),
        suggestions,
    })
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
        CoreOperation::Frame(CoreFrameOp::ClipPlane) => "clip-plane",
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
        CoreLiteral, CoreMetaOp, CoreNode, CoreNodeKind, CoreOperation, CorePart, CorePrimitive,
        CoreProgram, CoreSelectorPayload, CoreSurfaceOp, CoreValueKind, NodeId, PartId, ProgramId,
    };
    use sha2::{Digest, Sha256};
    use std::io::Write;

    fn compile(source: &str) -> CoreProgram {
        crate::ecky_scheme::compile_to_core_program(source).expect("compile")
    }

    #[test]
    fn flattens_nested_union_tools_into_one_difference() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape base (box 40 30 20))
                  (shape cutter-a (translate -8 0 0 (cylinder 2 24)))
                  (shape cutter-b (translate 0 0 0 (cylinder 2 24)))
                  (shape cutter-c (translate 8 0 0 (cylinder 2 24)))
                  (shape inner-cutters (union cutter-a cutter-b))
                  (shape cutters (union inner-cutters cutter-c))
                  (result (difference base cutters)))))
            "#,
        );

        let params = DesignParams::from([("length".to_string(), ParamValue::Number(24.0))]);
        let plan = plan_core_program_with_params(&program, &params).expect("plan");
        let part = &plan.parts[0];
        // parametric-thread-feature 3.1 binary-cut: base plus three flattened
        // cutter tools becomes a stable-order chain of exactly three binary
        // Difference commands (each base plus one tool), not one n-ary diff.
        let differences: Vec<&OcctCommand> = part
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Difference)
            .collect();
        assert_eq!(
            differences.len(),
            3,
            "base plus three cutter tools -> three binary cuts",
        );
        for command in &differences {
            assert_eq!(
                command.args.len(),
                2,
                "each cut must be binary (base plus one tool), got {:?}",
                command.args,
            );
        }
        // The chain threads base -> cut1 -> cut2 -> cut3 in source order.
        assert_eq!(
            differences[1].args[0],
            OcctArg::Ref(differences[0].output),
            "second binary cut must consume the first cut's result",
        );
        assert_eq!(
            differences[2].args[0],
            OcctArg::Ref(differences[1].output),
            "third binary cut must consume the second cut's result",
        );
        assert!(
            part.commands
                .iter()
                .all(|command| command.op != OcctOp::Union),
            "bypassed cutter unions must be dead"
        );
    }

    #[test]
    fn retains_flattened_union_when_topology_keyword_references_it() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape base (box 40 30 20))
                  (shape cutter-a (translate -6 0 0 (cylinder 2 24)))
                  (shape cutter-b (translate 6 0 0 (cylinder 2 24)))
                  (shape cutters (union cutter-a cutter-b))
                  (shape cut-body (difference base cutters))
                  (result
                    (fillet 0.5
                      :edges "left+vertical"
                      :created-by cutters
                      cut-body)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");
        let part = &plan.parts[0];
        let cutters = part
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Union)
            .expect("topology-referenced union remains");
        // The topology-referenced union survives, but the difference still
        // flattens it into a binary-cut chain (parametric-thread-feature 3.1):
        // base plus two cutter tools -> two binary Differences.
        let differences: Vec<&OcctCommand> = part
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Difference)
            .collect();
        assert_eq!(
            differences.len(),
            2,
            "base plus two cutter tools -> two binary cuts",
        );
        for command in &differences {
            assert_eq!(
                command.args.len(),
                2,
                "each cut must be binary (base plus one tool)",
            );
        }
        let fillet = part
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Fillet)
            .expect("fillet");
        assert_eq!(
            fillet
                .keywords
                .iter()
                .find(|keyword| keyword.name == "created-by")
                .expect("created-by")
                .source_arg(),
            &OcctArg::Ref(cutters.output)
        );
    }

    #[test]
    fn flattens_union_across_affine_transform_into_separate_transforms() {
        // parametric-thread-feature 3.1 optimizer slice: a difference whose tool
        // is `affineTransform(Union(children))` must rewrite to
        // `Difference(base, sameTransform(child1), sameTransform(child2), ...)`
        // so the fused union never reaches the boolean. Affine transforms
        // distribute over union: `T(A ∪ B) == T(A) ∪ T(B)`, so each child is cut
        // by its own identically-transformed tool with no fused intermediate.
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape base (box 40 30 20))
                  (shape cutter-a (cylinder 2 24))
                  (shape cutter-b (translate 8 0 0 cutter-a))
                  (shape cutters (union cutter-a cutter-b))
                  (shape moved-cutters (translate -4 0 0 cutters))
                  (result (difference base moved-cutters)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("plan");
        let part = &plan.parts[0];
        let producers = command_producers(&part.commands).expect("producers");
        // parametric-thread-feature 3.1 binary-cut: the distributed tools now
        // reach the boolean as a chain of binary Differences, one per union
        // child. Each binary cut's tool is a separate Translate(child).
        let differences: Vec<&OcctCommand> = part
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Difference)
            .collect();
        assert_eq!(
            differences.len(),
            2,
            "expected two binary cuts (one transformed tool per union child)",
        );
        for command in &differences {
            assert_eq!(command.args.len(), 2, "each cut must be binary");
        }

        // Each tool is a Translate that preserves the original affine params
        // (-4, 0, 0) and targets exactly one of the union's children.
        let mut subject_ops: Vec<OcctOp> = Vec::new();
        for command in &differences {
            let tool = &command.args[1];
            let OcctArg::Ref(slot) = tool else {
                panic!("expected tool ref, got {tool:?}");
            };
            let transform = producers
                .get(slot)
                .and_then(|index| part.commands.get(*index))
                .expect("tool producer");
            assert_eq!(
                transform.op,
                OcctOp::Translate,
                "tool {tool:?} should be produced by a Translate",
            );
            let params: Vec<f64> = transform
                .args
                .iter()
                .take(3)
                .map(|arg| match arg {
                    OcctArg::Number(value) => *value,
                    other => panic!("expected number param, got {other:?}"),
                })
                .collect();
            assert_eq!(
                params,
                vec![-4.0, 0.0, 0.0],
                "affine translate params must be preserved per child",
            );
            let OcctArg::Ref(subject_slot) = &transform.args[3] else {
                panic!("expected subject ref, got {:?}", transform.args[3]);
            };
            let subject = producers
                .get(subject_slot)
                .and_then(|index| part.commands.get(*index))
                .expect("subject producer");
            subject_ops.push(subject.op);
        }
        // The two tools target the two distinct union children: a bare cylinder
        // (cutter-a) and an already-translated cylinder (cutter-b = translate 8 0 0).
        assert_eq!(subject_ops.len(), 2, "one tool per union child");
        assert!(
            subject_ops.contains(&OcctOp::Cylinder),
            "a tool must target the bare cylinder child (cutter-a)",
        );
        assert!(
            subject_ops.contains(&OcctOp::Translate),
            "a tool must target the translated cylinder child (cutter-b)",
        );

        // The fused union is dead — it never reaches the Difference.
        assert!(
            !part
                .commands
                .iter()
                .any(|command| command.op == OcctOp::Union),
            "no fused union should remain in the optimized part"
        );
    }

    #[test]
    fn difference_with_two_tools_becomes_binary_cut_chain_with_final_original_slot() {
        // parametric-thread-feature 3.1 binary-cut optimizer slice (RED): every
        // Difference with >=2 tools rewrites to a stable-order chain of binary
        // Difference commands — Difference(base, tool1) -> fresh intermediate,
        // Difference(intermediate, tool2) -> ... -> Difference(intermediate,
        // toolN) -> ORIGINAL output — so each boolean cut is binary. Fresh
        // intermediates carry no keywords; the FINAL link MUST keep the original
        // command.output (and its keywords) so part.root and downstream refs
        // (fillet/chamfer/topology keyword) are unchanged. A single-tool
        // Difference stays binary as-is (guarded in the optimizer).
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape base (box 40 30 20))
                  (shape cutter-a (translate -8 0 0 (cylinder 2 24)))
                  (shape cutter-b (translate 8 0 0 (cylinder 2 24)))
                  (shape cutters (union cutter-a cutter-b))
                  (result (difference base cutters)))))
            "#,
        );

        // The unoptimized plan keeps the union as a single tool, so its lone
        // difference's output is the slot the optimized chain's final link MUST
        // preserve verbatim.
        let unoptimized = plan_core_program_unoptimized(&program).expect("unoptimized plan");
        let original_difference = unoptimized.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Difference)
            .expect("original difference");
        let original_output = original_difference.output;

        let plan = plan_core_program(&program).expect("plan");
        let part = &plan.parts[0];
        let differences: Vec<&OcctCommand> = part
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Difference)
            .collect();

        // base + two flattened cutter tools -> a chain of exactly two binary cuts.
        assert_eq!(
            differences.len(),
            2,
            "expected two binary difference commands, got {}: {:?}",
            differences.len(),
            differences,
        );

        // Every link is binary (base plus exactly one tool) and keyword-free in
        // this keyword-less source; only the final link may carry keywords.
        for command in &differences {
            assert_eq!(
                command.args.len(),
                2,
                "binary difference must be base plus one tool, got {:?}",
                command.args,
            );
            assert!(
                command.keywords.is_empty(),
                "binary links must be keyword-free in this keyword-less source",
            );
        }

        let first = differences[0];
        let final_link = differences[1];

        // The intermediate takes a fresh slot strictly above the planned max;
        // the final link keeps the original difference's output slot verbatim.
        assert!(
            first.output.0 > original_output.0,
            "intermediate must take a fresh slot above the original; got first={} original={}",
            first.output.0,
            original_output.0,
        );
        assert_eq!(
            final_link.output, original_output,
            "final binary link MUST keep the original difference output so downstream refs hold",
        );

        // Stable order: the final link consumes the intermediate's result as its
        // base, so the chain threads base -> cut1 -> cut2 in source order.
        assert_eq!(
            final_link.args[0],
            OcctArg::Ref(first.output),
            "final link must cut from the intermediate's result",
        );
        assert_eq!(
            part.root, final_link.output,
            "part root must resolve to the final binary link's output",
        );

        // The fused union never reaches the boolean.
        assert!(
            !part
                .commands
                .iter()
                .any(|command| command.op == OcctOp::Union),
            "bypassed cutter union must be dead"
        );
    }

    #[test]
    fn rejects_missing_command_dependency_before_runner_serialization() {
        let error = optimize_part_commands(
            OcctSlot(2),
            vec![OcctCommand {
                output: OcctSlot(2),
                op: OcctOp::Translate,
                args: vec![
                    OcctArg::Number(1.0),
                    OcctArg::Number(0.0),
                    OcctArg::Number(0.0),
                    OcctArg::Ref(OcctSlot(99)),
                ],
                keywords: Vec::new(),
            }],
            &BTreeSet::new(),
        )
        .expect_err("missing dependency");

        assert!(error.to_string().contains("missing slot 99"), "{error}");
    }

    #[test]
    fn rejects_cyclic_command_dependency_before_runner_serialization() {
        let error = optimize_part_commands(
            OcctSlot(1),
            vec![
                OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Translate,
                    args: vec![
                        OcctArg::Number(1.0),
                        OcctArg::Number(0.0),
                        OcctArg::Number(0.0),
                        OcctArg::Ref(OcctSlot(2)),
                    ],
                    keywords: Vec::new(),
                },
                OcctCommand {
                    output: OcctSlot(2),
                    op: OcctOp::Translate,
                    args: vec![
                        OcctArg::Number(-1.0),
                        OcctArg::Number(0.0),
                        OcctArg::Number(0.0),
                        OcctArg::Ref(OcctSlot(1)),
                    ],
                    keywords: Vec::new(),
                },
            ],
            &BTreeSet::new(),
        )
        .expect_err("cycle");

        assert!(
            error.to_string().contains("cyclic or forward dependency"),
            "{error}"
        );
    }

    #[test]
    fn optimizes_real_toothbrush_holder_repeated_cut_graph() {
        let source = include_str!("../../tests/fixtures/cad/perf/toothbrush_holder_versions.ecky");
        let model_source = source
            .split_once("(model")
            .map(|(_, body)| format!("(model{}", body.trim_end()))
            .expect("model source");
        assert_eq!(
            format!("{:x}", Sha256::digest(model_source.as_bytes())),
            "81f7ded44df1dbd1d38588fe2db876e721130889acbc235e4faa5c3b3c7e033f"
        );

        let program = compile(source);
        let baseline = plan_core_program_unoptimized(&program).expect("baseline plan");
        let optimized = plan_core_program(&program).expect("optimized plan");
        let baseline_part = &baseline.parts[0];
        let optimized_part = &optimized.parts[0];
        let baseline_union_count = baseline_part
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Union)
            .count();
        let optimized_union_count = optimized_part
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Union)
            .count();
        // parametric-thread-feature 3.1 binary-cut: the repeated cutter graph
        // now lowers to a chain of binary Differences (one tool each), so the
        // relevant signals are the binary cut count and that NO difference
        // carries more than one tool. Command count is NOT expected to drop —
        // the binary-cut contract intentionally trades one n-ary difference for
        // many binary ones — so union reduction (the real flatten effect) is the
        // command-graph signal instead.
        let optimized_difference_count = optimized_part
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Difference)
            .count();
        let optimized_max_tools = optimized_part
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Difference)
            .map(|command| command.args.len().saturating_sub(1))
            .max()
            .unwrap_or_default();

        assert_eq!(baseline_part.root, optimized_part.root);
        assert!(
            optimized_union_count < baseline_union_count,
            "optimized unions={optimized_union_count} baseline unions={baseline_union_count}"
        );
        assert_eq!(
            optimized_max_tools, 1,
            "every optimized difference must be binary under the binary-cut contract"
        );
        assert!(
            optimized_difference_count >= 40,
            "expected the repeated cutter graph to become >=40 binary cuts, got {optimized_difference_count}"
        );
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
    fn plans_male_thread_with_named_radial_axial_envelope_clip() {
        let plan = expand_to_plan(
            "(model (part papa (thread :radius 8 :pitch 4 :length 12 :depth 1 :base-width 2 :crest-width 1)))",
        );
        let commands = &plan.parts[0].commands;
        let envelope_clip = commands
            .iter()
            .find(|command| command.op == OcctOp::ClipThreadEnvelope)
            .expect("male thread must be clipped to its radial/axial envelope");

        assert_eq!(
            envelope_clip.args[..3],
            [
                OcctArg::Number(9.0),
                OcctArg::Number(9.0),
                OcctArg::Number(12.0),
            ],
            "named envelope must use bottom/top major radii and authored length"
        );
        assert!(matches!(envelope_clip.args[3], OcctArg::Ref(_)));
    }

    #[test]
    fn plans_conical_thread_as_cone_and_conical_helix_for_direct_occt() {
        let plan = expand_to_plan(
            "(model (part pipe (thread :radius 6 :top-radius 5 :pitch 2 :length 20 :depth 0.8)))",
        );
        let commands = &plan.parts[0].commands;
        assert!(commands.iter().any(|command| command.op == OcctOp::Cone));
        let helix = commands
            .iter()
            .find(|command| command.op == OcctOp::HelixPath)
            .expect("conical helix path");
        assert_eq!(
            helix.args.len(),
            5,
            "top radius travels with helix: {helix:?}"
        );
        assert_eq!(helix.args[4], OcctArg::Number(4.7));
        let envelope_clip = commands
            .iter()
            .find(|command| command.op == OcctOp::ClipThreadEnvelope)
            .expect("conical thread envelope clip");
        assert_eq!(
            envelope_clip.args[..3],
            [
                OcctArg::Number(6.8),
                OcctArg::Number(5.8),
                OcctArg::Number(20.0),
            ]
        );
    }

    #[test]
    fn thread_profile_derives_base_from_flank_and_preserves_explicit_width_overrides() {
        let derived = derive_thread_profile(
            4.0,
            1.0,
            Some(std::f64::consts::FRAC_PI_4),
            Some(0.4),
            None,
            None,
        );
        assert!((derived.base_width - 2.4).abs() < 1.0e-9, "{derived:?}");
        assert!((derived.crest_width - 0.4).abs() < 1.0e-9, "{derived:?}");

        let overridden = derive_thread_profile(
            4.0,
            1.0,
            Some(std::f64::consts::FRAC_PI_4),
            Some(0.4),
            Some(1.7),
            Some(0.2),
        );
        assert_eq!(overridden.base_width, 1.7, "{overridden:?}");
        assert_eq!(overridden.crest_width, 0.2, "{overridden:?}");
    }

    #[test]
    fn plans_thread_angle_param_like_literal_degrees() {
        let literal = expand_to_plan(
            "(model (part screw (thread :radius 8 :pitch 4 :length 16 :depth 1 :crest 0.4 :flank 10deg)))",
        );
        let parameter = expand_to_plan(
            "(model (params (number thread_flank 10deg)) (part screw (thread :radius 8 :pitch 4 :length 16 :depth 1 :crest 0.4 :flank thread_flank)))",
        );
        let profile_args = |plan: &OcctPlan| {
            plan.parts[0]
                .commands
                .iter()
                .find(|command| command.op == OcctOp::Path)
                .expect("thread profile path")
                .args
                .clone()
        };

        let literal_profile = profile_args(&literal);
        let parameter_profile = profile_args(&parameter);
        assert_eq!(parameter_profile, literal_profile);

        let expected_base = 0.4 + 2.0 * 1.0 * 10.0_f64.to_radians().tan();
        let OcctArg::List(points) = &literal_profile[0] else {
            panic!("thread profile must lower as a point list: {literal_profile:?}");
        };
        assert_eq!(
            points[1],
            OcctArg::Point3([0.0, 0.0, -expected_base * 0.5]),
            "native thread must consume canonical IR degrees exactly once"
        );
        assert_eq!(points[4], OcctArg::Point3([0.0, 0.0, expected_base * 0.5]));
    }

    #[test]
    fn buttress_profile_keeps_return_flank_printable_and_load_flank_steep() {
        let profile = derive_buttress_thread_profile(
            4.0,
            1.0,
            0.174_532_925_199_432_95,
            std::f64::consts::FRAC_PI_4,
            None,
            None,
        );

        assert!(
            (profile.lower_flank - 0.174_532_925_199_432_95).abs() < 1.0e-9,
            "{profile:?}"
        );
        assert!(
            (profile.upper_flank - std::f64::consts::FRAC_PI_4).abs() < 1.0e-9,
            "{profile:?}"
        );
        assert!(
            profile.upper_flank <= std::f64::consts::FRAC_PI_4,
            "return/overhang flank: {profile:?}"
        );
        assert!(
            profile.lower_flank < profile.upper_flank,
            "load flank must stay steep: {profile:?}"
        );
        assert!(
            (profile.base_width - (1.0 + 1.0 * (0.174_532_925_199_432_95_f64.tan() + 1.0))).abs()
                < 1.0e-9,
            "{profile:?}"
        );
    }

    #[test]
    fn plans_buttress_thread_as_asymmetric_native_ridge() {
        let plan = expand_to_plan(
            "(model (part screw (thread :radius 8 :pitch 4 :length 16 :depth 1 :profile 'buttress :load-flank 10deg :return-flank 45deg)))",
        );

        assert!(
            plan.parts[0]
                .commands
                .iter()
                .any(|command| command.op == OcctOp::Sweep),
            "buttress thread must lower to a native sweep"
        );
    }

    #[test]
    fn thread_profile_warns_when_derived_turns_merge_without_rejecting_plan() {
        let diagnostic =
            thread_profile_printability_diagnostic(2.0, 2.4, 0.1).expect("merged turns diagnostic");
        assert!(diagnostic.contains("turns merge"), "{diagnostic}");
        assert!(
            diagnostic.contains("pitch <= base + clearance"),
            "{diagnostic}"
        );

        let plan = expand_to_plan(
            "(model (part screw (thread :radius 8 :pitch 2 :length 16 :depth 1 :crest 0.4 :flank 45deg :clearance 0.1)))",
        );
        assert!(
            plan.parts[0]
                .commands
                .iter()
                .any(|command| command.op == OcctOp::Sweep),
            "diagnostic must not hard-fail a native thread plan"
        );
        let program = compile(
            "(model (part screw (thread :radius 8 :pitch 2 :length 16 :depth 1 :crest 0.4 :flank 45deg :clearance 0.1)))",
        );
        let warnings = thread_printability_warnings(&program, &DesignParams::new())
            .expect("native warning collection");
        assert_eq!(
            warnings,
            vec![diagnostic],
            "native manifest warning payload"
        );
    }

    #[test]
    fn thread_advisories_ignore_unrelated_deferred_point_helpers() {
        let program = compile(
            r#"(model (params (number count 3))
          (part wire (path (map (lambda (i) (let* ((x (* i 2))) (list x 0 0))) (range count))))
          (part screw (thread :radius 8 :pitch 2 :length 16 :depth 1 :crest 0.4 :flank 45deg :clearance 0.1)))"#,
        );
        let warnings = thread_printability_warnings(&program, &DesignParams::new())
            .expect("thread advisories must not evaluate unrelated map callbacks");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("turns merge"));
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
            !ops.contains(&OcctOp::Union)
                && !ops.contains(&OcctOp::Cylinder)
                && !ops.contains(&OcctOp::ClipThreadEnvelope),
            "female thread should be a bare ridge cutter (no core/envelope clip), got {ops:?}"
        );
    }

    #[test]
    fn plans_tapped_hole_as_union_of_bore_and_relief_for_direct_occt() {
        let program = compile("(model (part wall (tapped-hole :iso \"M8\" :length 14)))");

        let plan = plan_core_program(&program).expect("plan");

        let ops: Vec<_> = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect();
        assert!(
            ops.contains(&OcctOp::Union) && ops.contains(&OcctOp::Cylinder),
            "expected tapped-hole to expand into union(cylinder bore, relief ridge), got {ops:?}"
        );
    }

    // The end-to-end `plan_core_program` path runs the Direct OCCT normalizer
    // first; until `tapped-hole` is admitted there these expand-path helpers
    // exercise the custom-op expansion directly (compile -> expand -> plan),
    // giving reliable RED/GREEN for the expansion logic in this file without a
    // bundled OCCT runtime.
    fn expand_to_plan(source: &str) -> OcctPlan {
        let program = compile(source);
        let parameters = DesignParams::new();
        let expanded = expand_core_program_for_direct_occt(&program, &parameters).expect("expand");
        plan_expanded_core_program(&expanded, &parameters, true).expect("plan")
    }

    #[test]
    fn expands_tapped_hole_into_bore_and_relief_ridge_union_for_direct_occt() {
        // Focused difference regression: a tapped-hole expands to a named-radius
        // bore cylinder UNION a helical relief ridge (sweep along a helix path),
        // mirroring the male thread's union(core, ridge) with radius := minor.
        let plan = expand_to_plan("(model (part wall (tapped-hole :iso \"M8\" :length 14)))");

        let ops: Vec<_> = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect();
        assert!(
            ops.contains(&OcctOp::Union) && ops.contains(&OcctOp::Cylinder),
            "expected tapped-hole to expand into union(cylinder bore, relief ridge), got {ops:?}"
        );
        assert!(
            ops.contains(&OcctOp::HelixPath) && ops.contains(&OcctOp::Sweep),
            "expected a helical relief ridge (helix-path + sweep), got {ops:?}"
        );
        let union = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.op == OcctOp::Union)
            .expect("tapped-hole union");
        let OcctArg::Ref(first_tool) = union.args[0] else {
            panic!("tapped-hole union first argument must be a shape ref");
        };
        let first_producer = plan.parts[0]
            .commands
            .iter()
            .find(|command| command.output == first_tool)
            .expect("tapped-hole union first producer");
        assert_eq!(
            first_producer.op,
            OcctOp::Cylinder,
            "tapped-hole must cut the bore before the relief to avoid coincident internal seams",
        );

        assert!(plan.parts[0]
            .commands
            .iter()
            .any(|command| command.op == OcctOp::Cylinder));
        assert!(plan.parts[0]
            .commands
            .iter()
            .any(|command| command.op == OcctOp::Union));
    }

    #[test]
    fn thread_and_tapped_hole_of_equal_nominal_share_iso_minor_for_direct_occt() {
        // Parity: an external `thread` and a `tapped-hole` of equal nominal size
        // decode the same ISO core. The male core cylinder and the female bore
        // cylinder therefore share one named minor radius, so the bolt core
        // seats in the tapped bore and both ridge crests reach the same major.
        let male_plan = expand_to_plan("(model (part bolt (thread :iso \"M8\" :length 14)))");
        let female_plan = expand_to_plan("(model (part nut (tapped-hole :iso \"M8\" :length 14)))");

        let cylinder_minor = |plan: &OcctPlan| {
            plan.parts[0]
                .commands
                .iter()
                .find(|command| command.op == OcctOp::Cylinder)
                .and_then(|command| command.args.first())
                .and_then(|arg| match arg {
                    OcctArg::Number(value) => Some(*value),
                    _ => None,
                })
                .expect("cylinder minor radius")
        };

        let male_minor = cylinder_minor(&male_plan);
        let female_minor = cylinder_minor(&female_plan);

        // M8 -> minor radius 3.23325 (major 8 / 2 - depth 0.76675).
        assert!(
            (male_minor - 3.23325).abs() < 1e-6,
            "male thread core cylinder must sit at M8 minor radius, got {male_minor}"
        );
        assert!(
            (female_minor - 3.23325).abs() < 1e-6,
            "tapped-hole bore cylinder must sit at M8 minor radius, got {female_minor}"
        );
        assert!(
            (male_minor - female_minor).abs() < 1e-9,
            "equal-nominal thread and tapped-hole must share one ISO minor radius to mate"
        );
    }

    #[test]
    fn plans_slot_arc_primitive_for_direct_occt() {
        let program = compile("(model (part body (extrude (slot-arc 20 0 90 10) 5)))");

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
        let program = compile("(model (part body (extrude (slot-center-point 0 0 15 0 10) 5)))");

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
    fn plans_parametric_slot_center_point_inside_map_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (params
                (number rows 3 :min 2 :max 6 :step 1)
                (number seed 0.18 :min 0 :max 1 :step 0.01))
              (part lattice
                (apply union
                  (map
                    (lambda (row)
                      (extrude
                        (slot-center-point
                          0
                          (+ row (hash-signed row 0 seed))
                          15
                          (+ row (hash-signed row 1 seed))
                          2)
                        1.5))
                    (range 0 rows)))))
            "#,
        );

        let plan = plan_core_program(&program).expect("parametric mapped slots planned");
        let slot_count = plan.parts[0]
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Slot)
            .count();

        assert_eq!(slot_count, 3);
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
    fn plans_local_component_ports_on_front_and_side_without_euler_rotations() {
        let program = compile(include_str!(
            "../../tests/fixtures/component-placement/dryer-latch-front-side.ecky"
        ));
        let plan = plan_core_program(&program).expect("native placement plan");
        let portable = {
            use ecky_render::KernelPlanner;
            ecky_render::PortableKernelPlanner
                .plan(&program, &BTreeMap::new())
                .expect("portable placement plan")
        };
        assert_eq!(
            plan.parts
                .iter()
                .map(|part| part.key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "enclosure",
                "front-latch",
                "side-latch",
                "mirrored-side-latch"
            ]
        );

        let plane_origin = |part_index: usize| -> [f64; 3] {
            let plane = plan.parts[part_index]
                .commands
                .iter()
                .find(|command| command.op == OcctOp::Plane)
                .expect("placed part plane");
            let origin = plane
                .keywords
                .iter()
                .find(|keyword| keyword.name == "origin")
                .expect("plane origin")
                .source_arg();
            match origin {
                OcctArg::Point3(point) => *point,
                OcctArg::List(values) if values.len() == 3 => {
                    let mut point = [0.0; 3];
                    for (index, value) in values.iter().enumerate() {
                        let OcctArg::Number(value) = value else {
                            panic!("plane origin coordinate must be numeric");
                        };
                        point[index] = *value;
                    }
                    point
                }
                other => panic!("plane origin must be a point, got {other:?}"),
            }
        };
        assert_eq!(plane_origin(1), [0.0, -25.0, 15.0]);
        assert_eq!(plane_origin(2), [50.0, 0.0, 15.0]);
        assert_eq!(plane_origin(3), [-50.0, 0.0, 15.0]);
        fn portable_scalar(part: &ecky_render::KernelPart, arg: &ecky_render::KernelArg) -> f64 {
            if arg.kind == "number" {
                return arg.value.as_f64().expect("portable number");
            }
            assert_eq!(arg.kind, "ref", "unexpected portable scalar: {arg:?}");
            let output = arg.value.as_u64().expect("portable ref output");
            let command = part
                .commands
                .iter()
                .find(|command| command.output == output)
                .expect("portable scalar command");
            let values = command
                .args
                .iter()
                .map(|value| portable_scalar(part, value))
                .collect::<Vec<_>>();
            match command.op.as_str() {
                "+" => values.iter().sum(),
                "-" if values.len() == 1 => -values[0],
                "-" => values[1..].iter().fold(values[0], |acc, value| acc - value),
                "*" => values.iter().product(),
                "/" => values[1..].iter().fold(values[0], |acc, value| acc / value),
                other => panic!("unsupported portable scalar op `{other}`"),
            }
        }
        let portable_plane_axis = |part_index: usize, name: &str| -> [f64; 3] {
            let part = &portable.parts[part_index];
            let plane = part
                .commands
                .iter()
                .find(|command| command.op == "plane")
                .expect("portable placed part plane");
            let value = plane
                .keywords
                .iter()
                .find(|keyword| keyword.name == name)
                .and_then(|keyword| keyword.value.as_ref())
                .expect("portable plane axis")
                .value
                .as_array()
                .expect("portable plane axis array");
            std::array::from_fn(|index| {
                value[index]
                    .as_f64()
                    .or_else(|| {
                        serde_json::from_value::<ecky_render::KernelArg>(value[index].clone())
                            .ok()
                            .map(|arg| portable_scalar(part, &arg))
                    })
                    .expect("numeric axis")
            })
        };
        for part_index in 1..=3 {
            assert_eq!(
                portable_plane_axis(part_index, "origin"),
                plane_origin(part_index)
            );
        }
        let local_box = |part_index: usize| {
            plan.parts[part_index]
                .commands
                .iter()
                .find(|command| command.op == OcctOp::Box)
                .expect("local latch box")
        };
        let local_body_digest = |part_index: usize| {
            let box_command = local_box(part_index);
            format!(
                "sha256:{:x}",
                Sha256::digest(
                    format!(
                        "{:?}\0{:?}\0{:?}",
                        box_command.op, box_command.args, box_command.keywords
                    )
                    .as_bytes()
                )
            )
        };
        assert_eq!(local_box(1).args, local_box(2).args);
        assert_eq!(local_box(1).keywords, local_box(2).keywords);
        assert_eq!(local_box(1).args, local_box(3).args);
        assert_eq!(local_box(1).keywords, local_box(3).keywords);
        assert_eq!(local_body_digest(1), local_body_digest(2));
        assert_eq!(local_body_digest(1), local_body_digest(3));
        let plane_axis = |part_index: usize, name: &str| -> [f64; 3] {
            let plane = plan.parts[part_index]
                .commands
                .iter()
                .find(|command| command.op == OcctOp::Plane)
                .expect("placed part plane");
            let axis = plane
                .keywords
                .iter()
                .find(|keyword| keyword.name == name)
                .expect("plane axis")
                .source_arg();
            let OcctArg::Point3(axis) = axis else {
                panic!("plane {name} must be a point, got {axis:?}");
            };
            *axis
        };
        let front_normal = plane_axis(1, "normal");
        let side_normal = plane_axis(2, "normal");
        assert_eq!(portable_plane_axis(1, "normal"), front_normal);
        assert_eq!(portable_plane_axis(2, "normal"), side_normal);
        assert_eq!(portable_plane_axis(3, "normal"), plane_axis(3, "normal"));
        assert_eq!(front_normal, [0.0, 1.0, 0.0]);
        assert_eq!(side_normal, [-1.0, 0.0, 0.0]);
        assert_eq!(
            front_normal
                .iter()
                .zip(side_normal)
                .map(|(left, right)| left * right)
                .sum::<f64>(),
            0.0
        );
        for part in &plan.parts[1..] {
            let ops = part
                .commands
                .iter()
                .map(|command| command.op)
                .collect::<Vec<_>>();
            assert!(ops.contains(&OcctOp::Plane), "{ops:?}");
            assert!(ops.contains(&OcctOp::Box), "{ops:?}");
            assert!(ops.contains(&OcctOp::Place), "{ops:?}");
            assert!(
                !ops.contains(&OcctOp::Rotate),
                "manual Euler leaked: {ops:?}"
            );
        }
    }

    #[test]
    fn plans_parameterized_thread_inside_component_for_direct_occt() {
        let program = compile(
            r#"
            (define-component threaded-ring
              ((number radius 18)
               (number pitch 4)
               (number length 12)
               (number depth 1.4))
              (build
                (shape positive
                  (thread
                    :radius radius
                    :pitch pitch
                    :length length
                    :depth depth
                    :base-width 2.75
                    :crest-width 1))
                (result positive)))
            (model
              (params
                (number ring-radius 20)
                (number ring-pitch 4)
                (number ring-length 12)
                (number ring-depth 1.4))
              (part ring
                (threaded-ring
                  :radius ring-radius
                  :pitch ring-pitch
                  :length ring-length
                  :depth ring-depth)))
            "#,
        );

        let parameters = DesignParams::from([
            ("ring-radius".to_string(), ParamValue::Number(20.0)),
            ("ring-pitch".to_string(), ParamValue::Number(4.0)),
            ("ring-length".to_string(), ParamValue::Number(12.0)),
            ("ring-depth".to_string(), ParamValue::Number(1.4)),
        ]);
        let plan =
            plan_core_program_with_params(&program, &parameters).expect("component thread planned");
        let warnings = thread_printability_warnings(&program, &parameters)
            .expect("component thread printability inspected");

        assert!(!plan.parts.is_empty());
        assert!(plan.parts.iter().any(|part| part
            .commands
            .iter()
            .any(|command| command.op == OcctOp::Sweep)));
        assert!(warnings.iter().all(|warning| !warning.is_empty()));
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
        assert!(hull.args.iter().all(|arg| matches!(arg, OcctArg::Ref(_))));
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
            plan.parts[0]
                .commands
                .iter()
                .any(|c| c.op == OcctOp::Translate),
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
    fn preserves_authored_shape_binding_slots_for_topology_provenance() {
        let program = compile(
            r#"(model
              (part body (build
                (shape base (box 20 10 8))
                (shape bore (translate 10 5 0 (cylinder 2 8)))
                (result (difference base bore)))))"#,
        );

        let planned = plan_core_program_with_params_and_bindings(&program, &DesignParams::new())
            .expect("plan with authored shape bindings");

        assert_eq!(planned.authored_shape_bindings.len(), 2);
        assert_eq!(planned.authored_shape_bindings[0].part_key, "body");
        assert_eq!(planned.authored_shape_bindings[0].name, "base");
        assert_eq!(planned.authored_shape_bindings[1].name, "bore");
        for binding in &planned.authored_shape_bindings {
            assert!(planned.plan.parts[0]
                .commands
                .iter()
                .any(|command| command.output.0 == binding.slot));
        }
    }

    #[test]
    fn preserves_only_created_by_let_bindings_for_topology_provenance() {
        let program = compile(
            r#"(model
              (tag-face mounting :faces "created-by:lower_slot|upper_slot" body)
              (part body
                (let* ((base (box 20 10 72))
                       (lower_slot (translate 10 5 16 (cylinder 2 20)))
                       (upper_slot (translate 10 5 56 (cylinder 2 20)))
                       (unrelated (box 1 1 1))
                       (slotted (difference base lower_slot upper_slot)))
                  slotted)))"#,
        );

        assert_eq!(program.selector_tags[0].target, "body");
        assert_eq!(
            program.selector_tags[0].authored_selector,
            "created-by:lower_slot|upper_slot"
        );

        let normalized =
            crate::ecky_cad_host::direct_occt_normalize::normalize_core_program_for_direct_occt(
                &program,
                &DesignParams::new(),
            )
            .expect("normalize");
        let expanded =
            expand_core_program_for_direct_occt(&normalized, &DesignParams::new()).expect("expand");
        assert_eq!(expanded.selector_tags.len(), 1);
        let unoptimized =
            plan_expanded_core_program_with_bindings(&expanded, &DesignParams::new(), false)
                .expect("unoptimized plan");
        assert_eq!(
            unoptimized
                .authored_shape_bindings
                .iter()
                .map(|binding| binding.name.as_str())
                .collect::<Vec<_>>(),
            ["lower_slot", "upper_slot"]
        );

        let planned = plan_core_program_with_params_and_bindings(&program, &DesignParams::new())
            .expect("plan with requested let bindings");
        let names = planned
            .authored_shape_bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, ["lower_slot", "upper_slot"]);
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
        let ops = commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();

        assert_eq!(
            ops,
            vec![
                OcctOp::Polygon,
                OcctOp::Polygon,
                OcctOp::Profile,
                OcctOp::Extrude
            ],
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
            profile
                .keywords
                .iter()
                .any(|keyword| keyword.name == "outer"),
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
    fn text_font_keyword_overrides_global_font_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (params
                (select font "ecky-missing-font"
                  :label "Font"
                  :options (("Missing" "ecky-missing-font") ("Arial" "Arial"))))
              (part body (extrude (text "II" 12 :font font) 4)))
        "#,
        );
        let controls = crate::ecky_ir::derive_controls_from_core_program(&program)
            .expect("derive font control");

        assert!(matches!(
            controls.fields.as_slice(),
            [crate::contracts::UiField::Select { key, label, options, .. }]
                if key == "font" && label == "Font" && options.len() == 2
        ));

        let err = plan_core_program(&program).expect_err("local missing font must fail");

        assert!(err.to_string().contains("ecky-missing-font"), "{err}");
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
    fn plans_thread_and_tapped_hole_placement_on_arbitrary_axis_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part assembly
                (build
                  (shape wall (box 40 12 30))
                  (shape axis-plane (plane :origin (0 6 15) :x (0 0 1) :normal (0 1 0)))
                  (shape axis-location (location axis-plane :offset (0 0 -2) :rotate (90 0 0)))
                  (shape male (place axis-location (thread :iso "M8" :length 14)))
                  (shape cutter (place axis-location (tapped-hole :iso "M8" :length 16)))
                  (result (compound male (difference wall cutter))))))
            "#,
        );
        let plan = plan_core_program(&program).expect("native arbitrary-axis thread plan");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();
        assert!(ops.contains(&OcctOp::Plane), "{ops:?}");
        assert!(ops.contains(&OcctOp::Location), "{ops:?}");
        assert!(
            ops.iter().filter(|op| **op == OcctOp::Place).count() >= 2,
            "{ops:?}"
        );
        assert!(ops.contains(&OcctOp::Sweep), "thread ridges: {ops:?}");
        assert!(ops.contains(&OcctOp::Difference), "wall cut: {ops:?}");
    }

    #[test]
    fn plans_two_composable_clip_planes_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part head
                (clip-plane
                  (clip-plane
                    (box 20 20 20)
                    :origin (0 0 8)
                    :normal (0 0 1)
                    :keep "positive")
                  :origin (14 0 0)
                  :normal (-1 0 0)
                  :keep "positive")))
            "#,
        );
        let plan = plan_core_program(&program).expect("native two-plane crop plan");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();
        assert_eq!(ops, vec![OcctOp::Box, OcctOp::ClipPlane, OcctOp::ClipPlane]);
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
        assert!(
            !ops.contains(&OcctOp::Union),
            "cutter-only union should flatten into the difference"
        );
        // parametric-thread-feature 3.1 binary-cut: base plus the mapped
        // cutters now lower to a chain of binary Differences (one tool each),
        // so each cut is binary; the cutter count surfaces as the binary-cut
        // count instead of one n-ary difference's tool arity.
        let binary_difference_count = plan.parts[0]
            .commands
            .iter()
            .filter(|command| command.op == OcctOp::Difference)
            .count();
        assert!(
            binary_difference_count >= 12,
            "base plus mapped cutters should reach a binary-cut chain of >=12 cuts, got {binary_difference_count}"
        );
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
            vec![
                OcctOp::Path,
                OcctOp::MakeFace,
                OcctOp::HelixPath,
                OcctOp::Sweep
            ]
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
    fn plans_voronoi_cell_as_polygon_for_direct_occt() {
        let program = compile(
            r#"
            (model
              (part cell
                (extrude
                  (voronoi-cell
                    (voronoi-cells 3 3 12 12 1.5 23)
                    4 40 40 1.2)
                  5)))
            "#,
        );
        let plan = plan_core_program(&program).expect("voronoi-cell planned");
        let ops = plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op)
            .collect::<Vec<_>>();
        assert!(
            ops.contains(&OcctOp::Polygon) && ops.contains(&OcctOp::Extrude),
            "expected voronoi-cell to expand into a polygon + extrude plan, got {ops:?}"
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
    fn public_plan_reports_unsupported_backend_op_with_fix() {
        use crate::contracts::{AppError, AppResult, AuthoringError, ErrorLayer};

        let root = CoreNode::new(
            NodeId::new(1),
            CoreNodeKind::Call {
                op: CoreOperation::Meta(CoreMetaOp::Annotate),
                args: Vec::new(),
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
                root,
            }],
        );

        fn assert_authoring<T>(_: Result<T, AuthoringError>) {}
        assert_authoring(plan_core_program(&program));
        let boundary: AppResult<_> = plan_core_program(&program).map_err(AppError::from);
        let err = boundary.expect_err("backend must reject annotate");

        assert_eq!(err.operation.as_deref(), Some("annotate"));
        assert_eq!(err.code, crate::contracts::AppErrorCode::Render);
        assert!(
            err.to_string().contains("direct OCCT"),
            "backend named: {}",
            err
        );
        assert_eq!(err.layer, Some(ErrorLayer::Backend));
        assert!(err.fix.expect("fix present").hint.is_some());
    }

    #[test]
    fn plan_reports_authoring_failure_naming_op() {
        use crate::contracts::{AppError, AppResult, ErrorLayer};

        let program = compile(
            r#"
            (model
              (part body (bx 1 1 1))
              (part handle (sphre 2)))
            "#,
        );
        let boundary: AppResult<_> = plan_core_program(&program).map_err(AppError::from);
        let err = boundary.expect_err("authoring failure");

        assert_eq!(err.layer, Some(ErrorLayer::CoreIr));
        assert_eq!(err.code, crate::contracts::AppErrorCode::Validation);
        assert_eq!(err.operation.as_deref(), Some("bx"));
        assert!(
            err.message.contains("bx"),
            "summary names op: {}",
            err.message
        );
        assert!(
            err.fix
                .expect("nearest-op fix")
                .suggestions
                .contains(&"box".to_string()),
            "nearest op suggestion must cross public boundary"
        );
    }

    #[test]
    fn unknown_core_ir_op_reports_suggestion() {
        use crate::contracts::{ErrorFix, ErrorLayer};

        let err = unknown_core_ir_authoring("bx");

        assert_eq!(err.layer, ErrorLayer::CoreIr);
        assert_eq!(err.op.as_deref(), Some("bx"));
        assert_eq!(
            err.fix,
            Some(ErrorFix {
                hint: Some("replace `bx` with a known Core IR operation.".into()),
                suggestions: vec!["box".into()],
            })
        );
    }

    #[test]
    fn constrained_align_axis_lists_valid_values() {
        use crate::contracts::ErrorLayer;

        let err = align_axis_name("sideways").expect_err("invalid constrained value");

        assert_eq!(err.layer, ErrorLayer::Backend);
        assert_eq!(err.reason, AuthoringReason::ConstrainedValue);
        let fix = err.fix.expect("valid axis values");
        assert_eq!(fix.suggestions, vec!["min", "center", "max"]);
    }
}
