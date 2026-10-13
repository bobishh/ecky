//! Partition analysis for hybrid poly BRep rendering.
//!
//! Walks a Core IR part tree and classifies how it should be rendered:
//!
//! - [`PureOcct`][PartRenderStrategy::PureOcct]: no mesh-only ops → render
//!   entirely through OCCT exact BRep.
//! - [`PureMesh`][PartRenderStrategy::PureMesh]: mesh-only ops present but no
//!   BRep-required op consumes their displaced output → render entirely through
//!   the Rust mesh renderer.
//! - [`Hybrid`][PartRenderStrategy::Hybrid]: mesh-only ops present AND a
//!   BRep-required op (boolean, chamfer, fillet, …) consumes the displaced
//!   output → render through the hybrid poly BRep pipeline (exact BRep →
//!   tessellate → mesh displacement → poly BRep → OCCT hybrid boolean).

use crate::ecky_core_ir::{
    CoreBooleanOp, CoreFrameOp, CoreKeywordArg, CoreNode, CoreNodeKind, CoreOperation,
    CorePrimitive, CoreProgram, CoreReference, CoreSelectorPayload, CoreSurfaceOp, NodeId,
};

// ---------------------------------------------------------------------------
// Public data model
// ---------------------------------------------------------------------------

/// How a single part should be rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartRenderStrategy {
    /// No mesh-only ops in this part.
    PureOcct,
    /// Mesh-only ops exist, but every downstream consumer is mesh-safe
    /// (translate, rotate, scale, mirror, group, …).
    PureMesh,
    /// A BRep-required op consumes the output of a mesh-only op.
    Hybrid,
}

/// Result of partitioning one part's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartPartition {
    pub strategy: PartRenderStrategy,
    /// Node IDs of every mesh-only op (the boundaries).
    pub boundary_node_ids: Vec<NodeId>,
    /// Maximal mesh-evaluated nodes that are imported back into OCCT.
    ///
    /// Mesh-capable transforms and poly edge operations may extend past the
    /// raw mesh-only boundary. OCCT booleans consume these outputs.
    pub mesh_output_node_ids: Vec<NodeId>,
    /// Post-order Boolean boundaries owned by this part. Operand order is the
    /// authored AST order; mesh kernels must never merge boundaries globally.
    pub mesh_boolean_boundaries: Vec<MeshBooleanBoundary>,
    /// True when at least one BRep-required op sits above a boundary.
    pub has_post_boundary_brep_op: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshBooleanBoundary {
    pub boolean_node_id: NodeId,
    pub operation: CoreBooleanOp,
    pub operand_node_ids: Vec<NodeId>,
    pub mesh_operand_indices: Vec<usize>,
}

impl MeshBooleanBoundary {
    pub fn supports_batch_boolean(&self) -> bool {
        self.operand_node_ids.len() >= 2 && self.operation != CoreBooleanOp::Xor
    }
}

impl PartPartition {
    pub fn is_hybrid(&self) -> bool {
        self.strategy == PartRenderStrategy::Hybrid
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshOriginSurfaceOpAdmissionIssue {
    pub part_index: usize,
    pub node_id: NodeId,
    pub operation: &'static str,
    pub selector: String,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Top-level entry points
// ---------------------------------------------------------------------------

/// Classify every part in a program. Returns one [`PartPartition`] per part,
/// in the same order as `program.parts`.
pub fn analyze_program(program: &CoreProgram) -> Vec<PartPartition> {
    program
        .parts
        .iter()
        .map(|p| analyze_part(&p.root))
        .collect()
}

/// True when one OCCT render must include geometry produced by the mesh
/// renderer. Hybrid parts always need the bridge. Separate PureMesh and
/// PureOcct parts also need it so the whole program can finish in OCCT.
pub fn requires_poly_brep_bridge(partitions: &[PartPartition]) -> bool {
    let has_hybrid = partitions.iter().any(PartPartition::is_hybrid);
    let has_pure_mesh = partitions
        .iter()
        .any(|partition| partition.strategy == PartRenderStrategy::PureMesh);
    let has_non_mesh = partitions
        .iter()
        .any(|partition| partition.strategy != PartRenderStrategy::PureMesh);
    has_hybrid || (has_pure_mesh && has_non_mesh)
}

/// Mesh-renderer outputs imported by the OCCT phase for one part.
pub fn mesh_bridge_output_node_ids(
    program: &CoreProgram,
    part_index: usize,
    partition: &PartPartition,
) -> Vec<NodeId> {
    match partition.strategy {
        PartRenderStrategy::PureOcct => Vec::new(),
        PartRenderStrategy::PureMesh => program
            .parts
            .get(part_index)
            .map(|part| vec![part.root.id])
            .unwrap_or_default(),
        PartRenderStrategy::Hybrid => partition.mesh_output_node_ids.clone(),
    }
}

/// Classify a single part by walking its root node tree.
pub fn analyze_part(root: &CoreNode) -> PartPartition {
    let analysis = analyze_node(root);
    let strategy = if analysis.boundary_node_ids.is_empty() {
        PartRenderStrategy::PureOcct
    } else if analysis.has_post_boundary_brep_op {
        PartRenderStrategy::Hybrid
    } else {
        PartRenderStrategy::PureMesh
    };
    let mut mesh_boolean_boundaries = Vec::new();
    collect_mesh_boolean_boundaries(
        root,
        &MeshFlowBindings::default(),
        &mut mesh_boolean_boundaries,
    );
    PartPartition {
        strategy,
        boundary_node_ids: analysis.boundary_node_ids,
        mesh_output_node_ids: mesh_phase_output_node_ids(root),
        mesh_boolean_boundaries,
        has_post_boundary_brep_op: analysis.has_post_boundary_brep_op,
    }
}

pub fn mesh_origin_surface_op_admission_issues(
    program: &CoreProgram,
) -> Vec<MeshOriginSurfaceOpAdmissionIssue> {
    let mut issues = Vec::new();
    for (part_index, part) in program.parts.iter().enumerate() {
        collect_mesh_origin_surface_op_admission_issues(
            &part.root,
            &MeshFlowBindings::default(),
            part_index,
            &mut issues,
        );
    }
    issues
}

// ---------------------------------------------------------------------------
// Internal: per-node recursive analysis (bottom-up)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct NodeAnalysis {
    /// Node IDs of mesh-only ops found in this subtree.
    boundary_node_ids: Vec<NodeId>,
    /// True if this subtree contains a BRep-required op whose result is
    /// influenced by a mesh-only op (directly or transitively).
    has_post_boundary_brep_op: bool,
    /// True if this node's RESULT is mesh-displaced (it IS a mesh-only op, or
    /// it consumes a mesh-displaced result).
    post_boundary: bool,
    /// True if this subtree contains at least one mesh-only op.
    has_mesh_op: bool,
}

#[derive(Clone, Default)]
struct PostBoundaryBindings<'a> {
    names: std::collections::HashMap<&'a str, bool>,
    nodes: std::collections::HashMap<NodeId, bool>,
}

fn analyze_node(node: &CoreNode) -> NodeAnalysis {
    analyze_node_with_bindings(node, &PostBoundaryBindings::default())
}

/// Analyze a node with known bindings that are post-boundary. The compiler
/// emits both name references and direct node references for Build/Let values.
fn analyze_node_with_bindings(
    node: &CoreNode,
    post_boundary_bindings: &PostBoundaryBindings<'_>,
) -> NodeAnalysis {
    match &node.kind {
        // Leaf-like nodes: no ops, no children.
        CoreNodeKind::Literal(_) | CoreNodeKind::Range { .. } => NodeAnalysis::default(),

        CoreNodeKind::Reference(reference) => {
            let post_boundary = match reference {
                CoreReference::Local(name) => post_boundary_bindings
                    .names
                    .get(name.as_str())
                    .copied()
                    .unwrap_or(false),
                CoreReference::Node(node_id) => post_boundary_bindings
                    .nodes
                    .get(node_id)
                    .copied()
                    .unwrap_or(false),
                _ => false,
            };
            NodeAnalysis {
                post_boundary,
                has_mesh_op: false,
                ..Default::default()
            }
        }

        CoreNodeKind::Build { bindings, result } => {
            // Two-pass: analyze each binding in order, tracking which names
            // are post-boundary, so later bindings and the result can resolve
            // references to earlier bindings.
            let mut local_post_boundary = post_boundary_bindings.clone();
            let mut combined = NodeAnalysis::default();

            for binding in bindings.iter() {
                let ba = analyze_node_with_bindings(&binding.value, &local_post_boundary);
                local_post_boundary
                    .names
                    .insert(binding.name.as_str(), ba.post_boundary);
                local_post_boundary
                    .nodes
                    .insert(binding.value.id, ba.post_boundary);
                combined.boundary_node_ids.extend(ba.boundary_node_ids);
                combined.has_post_boundary_brep_op |= ba.has_post_boundary_brep_op;
                combined.post_boundary |= ba.post_boundary;
                combined.has_mesh_op |= ba.has_mesh_op;
            }

            let ra = analyze_node_with_bindings(result, &local_post_boundary);
            combined.boundary_node_ids.extend(ra.boundary_node_ids);
            combined.has_post_boundary_brep_op |= ra.has_post_boundary_brep_op;
            combined.post_boundary |= ra.post_boundary;
            combined.has_mesh_op |= ra.has_mesh_op;

            apply_op_post_boundary(node, None, &mut combined);
            combined
        }

        CoreNodeKind::Let { bindings, body } => {
            let mut local_post_boundary = post_boundary_bindings.clone();
            let mut combined = NodeAnalysis::default();

            for binding in bindings.iter() {
                let ba = analyze_node_with_bindings(&binding.value, &local_post_boundary);
                local_post_boundary
                    .names
                    .insert(binding.name.as_str(), ba.post_boundary);
                local_post_boundary
                    .nodes
                    .insert(binding.value.id, ba.post_boundary);
                combined.boundary_node_ids.extend(ba.boundary_node_ids);
                combined.has_post_boundary_brep_op |= ba.has_post_boundary_brep_op;
                combined.post_boundary |= ba.post_boundary;
                combined.has_mesh_op |= ba.has_mesh_op;
            }

            let ba = analyze_node_with_bindings(body, &local_post_boundary);
            combined.boundary_node_ids.extend(ba.boundary_node_ids);
            combined.has_post_boundary_brep_op |= ba.has_post_boundary_brep_op;
            combined.post_boundary |= ba.post_boundary;
            combined.has_mesh_op |= ba.has_mesh_op;

            apply_op_post_boundary(node, None, &mut combined);
            combined
        }

        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let mut combined = collect_children_with_bindings(
                [
                    condition.as_ref(),
                    then_branch.as_ref(),
                    else_branch.as_ref(),
                ],
                post_boundary_bindings,
            );
            apply_op_post_boundary(node, None, &mut combined);
            combined
        }

        CoreNodeKind::Call { op, args, keywords } => {
            let kw_nodes = keywords.iter().map(|kw| kw.source_node());
            let mut combined =
                collect_children_with_bindings(args.iter().chain(kw_nodes), post_boundary_bindings);
            apply_op_post_boundary(node, Some(op), &mut combined);
            combined
        }

        CoreNodeKind::Map { sources, body, .. } => {
            let mut combined = collect_children_with_bindings(
                sources.iter().chain(std::iter::once(body.as_ref())),
                post_boundary_bindings,
            );
            apply_op_post_boundary(node, None, &mut combined);
            combined
        }

        CoreNodeKind::Apply { op, args, list } => {
            let mut combined = collect_children_with_bindings(
                args.iter().chain(std::iter::once(list.as_ref())),
                post_boundary_bindings,
            );
            apply_op_post_boundary(node, Some(op), &mut combined);
            combined
        }

        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            let mut combined = collect_children_with_bindings(items.iter(), post_boundary_bindings);
            apply_op_post_boundary(node, None, &mut combined);
            combined
        }
    }
}

/// Analyse a slice of child nodes and merge their results, resolving
/// references through the provided binding map.
fn collect_children_with_bindings<'a>(
    children: impl IntoIterator<Item = &'a CoreNode>,
    post_boundary_bindings: &PostBoundaryBindings<'_>,
) -> NodeAnalysis {
    let mut combined = NodeAnalysis::default();
    for child in children {
        let ca = analyze_node_with_bindings(child, post_boundary_bindings);
        combined.boundary_node_ids.extend(ca.boundary_node_ids);
        combined.has_post_boundary_brep_op |= ca.has_post_boundary_brep_op;
        combined.post_boundary |= ca.post_boundary;
        combined.has_mesh_op |= ca.has_mesh_op;
    }
    combined
}

/// Apply the op-level classification after children have been analysed.
///
/// Sets `post_boundary` / `has_mesh_op` when this node IS a mesh-only op,
/// and sets `has_post_boundary_brep_op` when this node is a BRep-required op
/// whose input is already post-boundary.
fn apply_op_post_boundary(
    node: &CoreNode,
    op: Option<&CoreOperation>,
    combined: &mut NodeAnalysis,
) {
    let Some(op) = op else {
        return;
    };

    let is_mesh_only = operation_is_mesh_only(op) || node_is_raster_extrude(node);
    let is_brep_required = operation_requires_brep(op);

    if is_mesh_only {
        combined.boundary_node_ids.push(node.id);
        combined.has_mesh_op = true;
        combined.post_boundary = true;
    }

    // The critical check: this node is a BRep-required op AND its result
    // depends on a mesh-displaced input → it needs hybrid rendering.
    if is_brep_required && combined.post_boundary {
        combined.has_post_boundary_brep_op = true;
    }
}

fn node_is_raster_extrude(node: &CoreNode) -> bool {
    matches!(
        &node.kind,
        CoreNodeKind::Call {
            op: CoreOperation::Surface(CoreSurfaceOp::Extrude),
            args,
            keywords,
        } if args.first().is_some_and(|arg| arg.value_kind == CoreValueKind::Text)
            || keywords.iter().any(|keyword| {
                matches!(keyword.name.as_str(), "width" | "depth" | "threshold" | "foreground")
            })
    )
}

// ---------------------------------------------------------------------------
// Op classification helpers
// ---------------------------------------------------------------------------

/// True for ops that can ONLY be evaluated by the mesh renderer (wall-pattern,
/// future import-mesh, relief-from-image, …).
fn operation_is_mesh_only(op: &CoreOperation) -> bool {
    match op {
        CoreOperation::Primitive(CorePrimitive::Stl) => true,
        CoreOperation::Custom(name) => crate::ecky_ir::is_ecky_rust_only_cad_head(name),
        _ => false,
    }
}

/// True for ops that require exact BRep topology to produce reliable results
/// and therefore trigger hybrid rendering when they consume mesh-displaced
/// input.
///
/// - Booleans (difference, union, intersection, xor): CSG over displaced
///   meshes is unreliable in the mesh renderer.
/// - Chamfer / fillet: edge operations that need real topology.
/// - Shell / offset: require face/surface data.
fn operation_requires_brep(op: &CoreOperation) -> bool {
    matches!(
        op,
        CoreOperation::Boolean(_)
            | CoreOperation::Surface(
                CoreSurfaceOp::Chamfer
                    | CoreSurfaceOp::Fillet
                    | CoreSurfaceOp::Shell
                    | CoreSurfaceOp::Offset
                    | CoreSurfaceOp::OffsetRounded,
            )
    ) || matches!(op, CoreOperation::Frame(CoreFrameOp::ClipPlane))
        || matches!(op, CoreOperation::Custom(name) if name == "solidify")
}

#[derive(Clone, Default)]
struct MeshPhaseFlow {
    depends_on_mesh: bool,
    stopped: bool,
    output_node_ids: Vec<NodeId>,
}

#[derive(Clone, Default)]
struct MeshFlowBindings<'a> {
    names: std::collections::HashMap<&'a str, MeshPhaseFlow>,
    nodes: std::collections::HashMap<NodeId, MeshPhaseFlow>,
}

fn mesh_phase_output_node_ids(root: &CoreNode) -> Vec<NodeId> {
    let flow = mesh_phase_flow(root, &MeshFlowBindings::default());
    let mut ids = flow.output_node_ids;
    ids.sort_by_key(|id| id.raw());
    ids.dedup();
    ids
}

fn mesh_phase_flow(node: &CoreNode, bindings: &MeshFlowBindings<'_>) -> MeshPhaseFlow {
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Range { .. } => MeshPhaseFlow::default(),
        CoreNodeKind::Reference(CoreReference::Local(name)) => bindings
            .names
            .get(name.as_str())
            .cloned()
            .unwrap_or_default(),
        CoreNodeKind::Reference(CoreReference::Node(node_id)) => {
            bindings.nodes.get(node_id).cloned().unwrap_or_default()
        }
        CoreNodeKind::Reference(_) => MeshPhaseFlow::default(),
        CoreNodeKind::Build {
            bindings: local_bindings,
            result,
        } => {
            let mut local = bindings.clone();
            for binding in local_bindings {
                let flow = mesh_phase_flow(&binding.value, &local);
                local.names.insert(binding.name.as_str(), flow.clone());
                local.nodes.insert(binding.value.id, flow);
            }
            mesh_phase_flow(result, &local)
        }
        CoreNodeKind::Let {
            bindings: local_bindings,
            body,
        } => {
            let mut local = bindings.clone();
            for binding in local_bindings {
                let flow = mesh_phase_flow(&binding.value, &local);
                local.names.insert(binding.name.as_str(), flow.clone());
                local.nodes.insert(binding.value.id, flow);
            }
            mesh_phase_flow(body, &local)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => extend_mesh_phase_node(
            node,
            merge_mesh_phase_flows([
                mesh_phase_flow(condition, bindings),
                mesh_phase_flow(then_branch, bindings),
                mesh_phase_flow(else_branch, bindings),
            ]),
            false,
            false,
        ),
        CoreNodeKind::Call { op, args, keywords } => {
            let children = args
                .iter()
                .map(|child| mesh_phase_flow(child, bindings))
                .chain(
                    keywords
                        .iter()
                        .map(|keyword| mesh_phase_flow(keyword.source_node(), bindings)),
                );
            extend_mesh_phase_node(
                node,
                merge_mesh_phase_flows(children),
                operation_is_mesh_only(op) || node_is_raster_extrude(node),
                operation_stops_mesh_phase(op),
            )
        }
        CoreNodeKind::Map { sources, body, .. } => extend_mesh_phase_node(
            node,
            merge_mesh_phase_flows(
                sources
                    .iter()
                    .map(|child| mesh_phase_flow(child, bindings))
                    .chain(std::iter::once(mesh_phase_flow(body, bindings))),
            ),
            false,
            false,
        ),
        CoreNodeKind::Apply { op, args, list } => extend_mesh_phase_node(
            node,
            merge_mesh_phase_flows(
                args.iter()
                    .map(|child| mesh_phase_flow(child, bindings))
                    .chain(std::iter::once(mesh_phase_flow(list, bindings))),
            ),
            operation_is_mesh_only(op),
            operation_stops_mesh_phase(op),
        ),
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => extend_mesh_phase_node(
            node,
            merge_mesh_phase_flows(items.iter().map(|child| mesh_phase_flow(child, bindings))),
            false,
            false,
        ),
    }
}

fn merge_mesh_phase_flows(flows: impl IntoIterator<Item = MeshPhaseFlow>) -> MeshPhaseFlow {
    let mut merged = MeshPhaseFlow::default();
    for flow in flows {
        merged.depends_on_mesh |= flow.depends_on_mesh;
        merged.stopped |= flow.stopped;
        merged.output_node_ids.extend(flow.output_node_ids);
    }
    merged
}

fn extend_mesh_phase_node(
    node: &CoreNode,
    mut flow: MeshPhaseFlow,
    is_mesh_only: bool,
    stops_mesh_phase: bool,
) -> MeshPhaseFlow {
    flow.depends_on_mesh |= is_mesh_only;
    if flow.depends_on_mesh && stops_mesh_phase {
        flow.stopped = true;
    } else if flow.depends_on_mesh && !flow.stopped {
        flow.output_node_ids.clear();
        flow.output_node_ids.push(node.id);
    }
    flow
}

fn collect_mesh_origin_surface_op_admission_issues<'a>(
    node: &'a CoreNode,
    bindings: &MeshFlowBindings<'a>,
    part_index: usize,
    issues: &mut Vec<MeshOriginSurfaceOpAdmissionIssue>,
) {
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Range { .. } | CoreNodeKind::Reference(_) => {}
        CoreNodeKind::Build {
            bindings: local_bindings,
            result,
        } => {
            let mut local = bindings.clone();
            for binding in local_bindings {
                collect_mesh_origin_surface_op_admission_issues(
                    &binding.value,
                    &local,
                    part_index,
                    issues,
                );
                let flow = mesh_phase_flow(&binding.value, &local);
                local.names.insert(binding.name.as_str(), flow.clone());
                local.nodes.insert(binding.value.id, flow);
            }
            collect_mesh_origin_surface_op_admission_issues(result, &local, part_index, issues);
        }
        CoreNodeKind::Let {
            bindings: local_bindings,
            body,
        } => {
            let mut local = bindings.clone();
            for binding in local_bindings {
                collect_mesh_origin_surface_op_admission_issues(
                    &binding.value,
                    &local,
                    part_index,
                    issues,
                );
                let flow = mesh_phase_flow(&binding.value, &local);
                local.names.insert(binding.name.as_str(), flow.clone());
                local.nodes.insert(binding.value.id, flow);
            }
            collect_mesh_origin_surface_op_admission_issues(body, &local, part_index, issues);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_mesh_origin_surface_op_admission_issues(
                condition, bindings, part_index, issues,
            );
            collect_mesh_origin_surface_op_admission_issues(
                then_branch,
                bindings,
                part_index,
                issues,
            );
            collect_mesh_origin_surface_op_admission_issues(
                else_branch,
                bindings,
                part_index,
                issues,
            );
        }
        CoreNodeKind::Call { op, args, keywords } => {
            if let Some(operation) = surface_operation_label(op) {
                let body_depends_on_mesh = args
                    .last()
                    .map(|body| mesh_phase_flow(body, bindings).depends_on_mesh)
                    .unwrap_or(false);
                if body_depends_on_mesh && !mesh_origin_surface_selector_admitted(keywords) {
                    issues.push(MeshOriginSurfaceOpAdmissionIssue {
                        part_index,
                        node_id: node.id,
                        operation,
                        selector: mesh_origin_surface_selector_label(keywords),
                        reason: "mesh-origin faceted BRep surface operations require exact edge target ids; broad selectors are rejected before OCCT kernel execution"
                            .to_string(),
                    });
                }
            }
            for arg in args {
                collect_mesh_origin_surface_op_admission_issues(arg, bindings, part_index, issues);
            }
            for keyword in keywords {
                collect_mesh_origin_surface_op_admission_issues(
                    keyword.source_node(),
                    bindings,
                    part_index,
                    issues,
                );
            }
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                collect_mesh_origin_surface_op_admission_issues(
                    source, bindings, part_index, issues,
                );
            }
            collect_mesh_origin_surface_op_admission_issues(body, bindings, part_index, issues);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                collect_mesh_origin_surface_op_admission_issues(arg, bindings, part_index, issues);
            }
            collect_mesh_origin_surface_op_admission_issues(list, bindings, part_index, issues);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                collect_mesh_origin_surface_op_admission_issues(item, bindings, part_index, issues);
            }
        }
    }
}

fn surface_operation_label(op: &CoreOperation) -> Option<&'static str> {
    match op {
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => Some("chamfer"),
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => Some("fillet"),
        _ => None,
    }
}

fn mesh_origin_surface_selector_admitted(keywords: &[CoreKeywordArg]) -> bool {
    keywords
        .iter()
        .find(|keyword| keyword.name == "edges")
        .and_then(CoreKeywordArg::selector_payload)
        .is_some_and(|selector| matches!(selector, CoreSelectorPayload::EdgeTargetIds(ids) if !ids.is_empty()))
}

fn mesh_origin_surface_selector_label(keywords: &[CoreKeywordArg]) -> String {
    match keywords
        .iter()
        .find(|keyword| keyword.name == "edges")
        .and_then(CoreKeywordArg::selector_payload)
    {
        Some(CoreSelectorPayload::EdgeAll) | None => "all".to_string(),
        Some(CoreSelectorPayload::EdgeTargetIds(ids)) => {
            format!("target-ids({})", ids.len())
        }
        Some(CoreSelectorPayload::EdgeClauses(_)) => "edge-clauses".to_string(),
        Some(CoreSelectorPayload::EdgeTag(tag)) => format!("edge-tag:{tag}"),
        Some(CoreSelectorPayload::FaceClauses(_)) => "face-clauses".to_string(),
        Some(CoreSelectorPayload::FaceTag(tag)) => format!("face-tag:{tag}"),
        Some(CoreSelectorPayload::FaceTargetIds(ids)) => format!("face-target-ids({})", ids.len()),
    }
}

/// OCCT handles booleans and topology-changing surface operations after the
/// mesh island. Chamfer/fillet stop the mesh phase so they run against exact
/// BRep or an explicitly solidified mesh-origin poly BRep.
fn operation_stops_mesh_phase(op: &CoreOperation) -> bool {
    matches!(
        op,
        CoreOperation::Boolean(_)
            | CoreOperation::Surface(
                CoreSurfaceOp::Chamfer
                    | CoreSurfaceOp::Fillet
                    | CoreSurfaceOp::Shell
                    | CoreSurfaceOp::Offset
                    | CoreSurfaceOp::OffsetRounded,
            )
    ) || matches!(op, CoreOperation::Frame(CoreFrameOp::ClipPlane))
        || matches!(op, CoreOperation::Custom(name) if name == "solidify")
}

fn collect_mesh_boolean_boundaries<'a>(
    node: &'a CoreNode,
    bindings: &MeshFlowBindings<'a>,
    boundaries: &mut Vec<MeshBooleanBoundary>,
) {
    match &node.kind {
        CoreNodeKind::Build {
            bindings: local_bindings,
            result,
        } => {
            let mut local = bindings.clone();
            for binding in local_bindings {
                collect_mesh_boolean_boundaries(&binding.value, &local, boundaries);
                let flow = mesh_phase_flow(&binding.value, &local);
                local.names.insert(binding.name.as_str(), flow.clone());
                local.nodes.insert(binding.value.id, flow);
            }
            collect_mesh_boolean_boundaries(result, &local, boundaries);
        }
        CoreNodeKind::Let {
            bindings: local_bindings,
            body,
        } => {
            let mut local = bindings.clone();
            for binding in local_bindings {
                collect_mesh_boolean_boundaries(&binding.value, &local, boundaries);
                let flow = mesh_phase_flow(&binding.value, &local);
                local.names.insert(binding.name.as_str(), flow.clone());
                local.nodes.insert(binding.value.id, flow);
            }
            collect_mesh_boolean_boundaries(body, &local, boundaries);
        }
        CoreNodeKind::Call { op, args, keywords } => {
            for arg in args {
                collect_mesh_boolean_boundaries(arg, bindings, boundaries);
            }
            for keyword in keywords {
                collect_mesh_boolean_boundaries(keyword.source_node(), bindings, boundaries);
            }
            if let CoreOperation::Boolean(operation) = op {
                let mesh_operand_indices = args
                    .iter()
                    .enumerate()
                    .filter_map(|(index, arg)| {
                        mesh_phase_flow(arg, bindings)
                            .depends_on_mesh
                            .then_some(index)
                    })
                    .collect::<Vec<_>>();
                if !mesh_operand_indices.is_empty() {
                    boundaries.push(MeshBooleanBoundary {
                        boolean_node_id: node.id,
                        operation: operation.clone(),
                        operand_node_ids: args.iter().map(|arg| arg.id).collect(),
                        mesh_operand_indices,
                    });
                }
            }
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                collect_mesh_boolean_boundaries(arg, bindings, boundaries);
            }
            collect_mesh_boolean_boundaries(list, bindings, boundaries);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_mesh_boolean_boundaries(condition, bindings, boundaries);
            collect_mesh_boolean_boundaries(then_branch, bindings, boundaries);
            collect_mesh_boolean_boundaries(else_branch, bindings, boundaries);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                collect_mesh_boolean_boundaries(source, bindings, boundaries);
            }
            collect_mesh_boolean_boundaries(body, bindings, boundaries);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                collect_mesh_boolean_boundaries(item, bindings, boundaries);
            }
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// Tree slicing for hybrid dispatch
// ---------------------------------------------------------------------------

use crate::ecky_core_ir::{CoreLiteral, CoreValueKind};

/// Clone a program for the **mesh phase**: for each Hybrid part, replace the
/// root with just the first boundary subtree (the mesh-only op + everything
/// beneath it). This strips all post-boundary boolean ops so the mesh renderer
/// produces only the displaced geometry, no CSG garbage.
///
/// Non-Hybrid parts are passed through unchanged.
pub fn clone_program_for_mesh_phase(
    program: &CoreProgram,
    partitions: &[PartPartition],
) -> CoreProgram {
    let mut clone = program.clone();
    // Only keep Hybrid parts — PureOcct parts are handled by the OCCT phase,
    // and PureMesh parts render unchanged. Including PureOcct parts in the
    // mesh phase causes unnecessary union operations that can panic earcutr
    // when the displaced mesh has degenerate polygons.
    clone.parts = clone
        .parts
        .drain(..)
        .zip(partitions.iter())
        .filter_map(|(mut part, partition)| {
            if partition.strategy != PartRenderStrategy::Hybrid {
                return None;
            }
            let output_ids: std::collections::HashSet<NodeId> =
                partition.mesh_output_node_ids.iter().copied().collect();
            let new_root = slice_mesh_phase_root(&part.root, &output_ids);
            if let Some(new_root) = new_root {
                part.root = new_root;
            }
            Some(part)
        })
        .collect();
    clone
}

/// Build one mesh-renderer job for one mesh island. Separate jobs preserve
/// independent STL assets when a part contains multiple mesh branches.
pub fn clone_program_for_mesh_output(
    program: &CoreProgram,
    part_index: usize,
    output_node_id: NodeId,
) -> Option<CoreProgram> {
    let mut clone = program.clone();
    let mut part = clone.parts.get(part_index)?.clone();
    if part.root.id != output_node_id {
        let output_ids = std::collections::HashSet::from([output_node_id]);
        part.root = slice_mesh_phase_root(&part.root, &output_ids)?;
    }
    clone.parts = vec![part];
    Some(clone)
}

pub fn exact_mesh_prelude_node_ids(program: &CoreProgram) -> Vec<(usize, NodeId)> {
    let mut preludes = Vec::new();
    for (part_index, part) in program.parts.iter().enumerate() {
        collect_exact_mesh_preludes(&part.root, false, part_index, &mut preludes);
    }
    preludes.sort_by_key(|(_, node_id)| node_id.raw());
    preludes.dedup();
    preludes
}

fn collect_exact_mesh_preludes(
    node: &CoreNode,
    inside_mesh_target: bool,
    part_index: usize,
    preludes: &mut Vec<(usize, NodeId)>,
) {
    match &node.kind {
        CoreNodeKind::Call { op, args, keywords } => {
            if inside_mesh_target
                && matches!(
                    op,
                    CoreOperation::Surface(CoreSurfaceOp::Chamfer | CoreSurfaceOp::Fillet)
                )
            {
                preludes.push((part_index, node.id));
                return;
            }
            let mesh_only = operation_is_mesh_only(op);
            for (index, arg) in args.iter().enumerate() {
                collect_exact_mesh_preludes(
                    arg,
                    inside_mesh_target || (mesh_only && index + 1 == args.len()),
                    part_index,
                    preludes,
                );
            }
            for keyword in keywords {
                collect_exact_mesh_preludes(
                    keyword.source_node(),
                    inside_mesh_target,
                    part_index,
                    preludes,
                );
            }
        }
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings {
                collect_exact_mesh_preludes(
                    &binding.value,
                    inside_mesh_target,
                    part_index,
                    preludes,
                );
            }
            collect_exact_mesh_preludes(result, inside_mesh_target, part_index, preludes);
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                collect_exact_mesh_preludes(
                    &binding.value,
                    inside_mesh_target,
                    part_index,
                    preludes,
                );
            }
            collect_exact_mesh_preludes(body, inside_mesh_target, part_index, preludes);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_exact_mesh_preludes(condition, inside_mesh_target, part_index, preludes);
            collect_exact_mesh_preludes(then_branch, inside_mesh_target, part_index, preludes);
            collect_exact_mesh_preludes(else_branch, inside_mesh_target, part_index, preludes);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                collect_exact_mesh_preludes(source, inside_mesh_target, part_index, preludes);
            }
            collect_exact_mesh_preludes(body, inside_mesh_target, part_index, preludes);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                collect_exact_mesh_preludes(arg, inside_mesh_target, part_index, preludes);
            }
            collect_exact_mesh_preludes(list, inside_mesh_target, part_index, preludes);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                collect_exact_mesh_preludes(item, inside_mesh_target, part_index, preludes);
            }
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => {}
    }
}

pub fn clone_program_for_exact_mesh_prelude(
    program: &CoreProgram,
    part_index: usize,
    prelude_node_id: NodeId,
) -> Option<CoreProgram> {
    let mut clone = program.clone();
    let mut part = clone.parts.get(part_index)?.clone();
    part.root = slice_program_to_node(&part.root, prelude_node_id)?;
    prune_unused_local_bindings(&mut part.root);
    clone.parts = vec![part];
    Some(clone)
}

fn slice_program_to_node(root: &CoreNode, target: NodeId) -> Option<CoreNode> {
    if root.id == target {
        return Some(root.clone());
    }
    match &root.kind {
        CoreNodeKind::Build { bindings, result } => {
            if let Some(index) = bindings.iter().position(|binding| {
                node_contains_any(&binding.value, &std::collections::HashSet::from([target]))
            }) {
                let mut kept = bindings[..=index].to_vec();
                kept[index].value = slice_program_to_node(&kept[index].value, target)?;
                let name = kept[index].name.clone();
                return Some(CoreNode::new(
                    root.id,
                    CoreNodeKind::Build {
                        bindings: kept,
                        result: Box::new(CoreNode::new(
                            NodeId::new(3_000_000 + target.raw()),
                            CoreNodeKind::Reference(CoreReference::Local(name)),
                            CoreValueKind::Solid,
                        )),
                    },
                    CoreValueKind::Solid,
                ));
            }
            Some(CoreNode::new(
                root.id,
                CoreNodeKind::Build {
                    bindings: bindings.clone(),
                    result: Box::new(slice_program_to_node(result, target)?),
                },
                CoreValueKind::Solid,
            ))
        }
        CoreNodeKind::Let { bindings, body } => {
            if let Some(index) = bindings.iter().position(|binding| {
                node_contains_any(&binding.value, &std::collections::HashSet::from([target]))
            }) {
                let mut kept = bindings[..=index].to_vec();
                kept[index].value = slice_program_to_node(&kept[index].value, target)?;
                let name = kept[index].name.clone();
                return Some(CoreNode::new(
                    root.id,
                    CoreNodeKind::Let {
                        bindings: kept,
                        body: Box::new(CoreNode::new(
                            NodeId::new(3_000_000 + target.raw()),
                            CoreNodeKind::Reference(CoreReference::Local(name)),
                            CoreValueKind::Solid,
                        )),
                    },
                    CoreValueKind::Solid,
                ));
            }
            Some(CoreNode::new(
                root.id,
                CoreNodeKind::Let {
                    bindings: bindings.clone(),
                    body: Box::new(slice_program_to_node(body, target)?),
                },
                CoreValueKind::Solid,
            ))
        }
        _ => find_node(root, target).cloned(),
    }
}

pub fn replace_node_with_mesh_asset(program: &mut CoreProgram, node_id: NodeId, path: &str) {
    let mut counter = NodeId::new(4_000_000 + node_id.raw());
    for part in &mut program.parts {
        replace_node_with_import_stl(&mut part.root, node_id, &mut counter, path);
    }
}

fn replace_node_with_import_stl(
    node: &mut CoreNode,
    target: NodeId,
    id_counter: &mut NodeId,
    path: &str,
) {
    if node.id == target {
        let import_node = make_import_stl_node(
            {
                *id_counter = NodeId::new(id_counter.raw() + 1);
                *id_counter
            },
            path,
            id_counter,
        );
        *node = CoreNode::new(
            node.id,
            CoreNodeKind::Call {
                op: CoreOperation::Custom("mesh-bridge-overlap".to_string()),
                args: vec![import_node],
                keywords: vec![],
            },
            CoreValueKind::Solid,
        );
        return;
    }
    match &mut node.kind {
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings {
                replace_node_with_import_stl(&mut binding.value, target, id_counter, path);
            }
            replace_node_with_import_stl(result, target, id_counter, path);
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                replace_node_with_import_stl(&mut binding.value, target, id_counter, path);
            }
            replace_node_with_import_stl(body, target, id_counter, path);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            replace_node_with_import_stl(condition, target, id_counter, path);
            replace_node_with_import_stl(then_branch, target, id_counter, path);
            replace_node_with_import_stl(else_branch, target, id_counter, path);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                replace_node_with_import_stl(arg, target, id_counter, path);
            }
            for keyword in keywords {
                replace_node_with_import_stl(keyword.source_node_mut(), target, id_counter, path);
            }
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                replace_node_with_import_stl(source, target, id_counter, path);
            }
            replace_node_with_import_stl(body, target, id_counter, path);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                replace_node_with_import_stl(arg, target, id_counter, path);
            }
            replace_node_with_import_stl(list, target, id_counter, path);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                replace_node_with_import_stl(item, target, id_counter, path);
            }
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => {}
    }
}

/// Slice a part root for the mesh phase. For Build/Let, keep bindings up to
/// the last boundary-containing binding and point the result at it. For other
/// node types, replace the root with the first boundary node found.
fn slice_mesh_phase_root(
    root: &CoreNode,
    boundary_ids: &std::collections::HashSet<NodeId>,
) -> Option<CoreNode> {
    match &root.kind {
        CoreNodeKind::Build { bindings, result } => {
            if let Some(last_boundary_idx) = bindings
                .iter()
                .rposition(|b| node_contains_any(&b.value, boundary_ids))
            {
                let mut kept_bindings: Vec<_> = bindings[..=last_boundary_idx].to_vec();
                kept_bindings[last_boundary_idx].value =
                    slice_mesh_phase_root(&kept_bindings[last_boundary_idx].value, boundary_ids)?;
                let binding_name = kept_bindings[last_boundary_idx].name.clone();
                let binding_id = kept_bindings[last_boundary_idx].value.id;
                return Some(CoreNode::new(
                    root.id,
                    CoreNodeKind::Build {
                        bindings: kept_bindings,
                        result: Box::new(CoreNode::new(
                            binding_id,
                            CoreNodeKind::Reference(CoreReference::Local(binding_name)),
                            CoreValueKind::Solid,
                        )),
                    },
                    root.value_kind,
                ));
            }
            let sliced_result = slice_mesh_phase_root(result, boundary_ids)?;
            Some(CoreNode::new(
                root.id,
                CoreNodeKind::Build {
                    bindings: bindings.clone(),
                    result: Box::new(sliced_result),
                },
                root.value_kind,
            ))
        }
        CoreNodeKind::Let { bindings, body } => {
            if let Some(last_boundary_idx) = bindings
                .iter()
                .rposition(|b| node_contains_any(&b.value, boundary_ids))
            {
                let mut kept_bindings: Vec<_> = bindings[..=last_boundary_idx].to_vec();
                kept_bindings[last_boundary_idx].value =
                    slice_mesh_phase_root(&kept_bindings[last_boundary_idx].value, boundary_ids)?;
                let binding_name = kept_bindings[last_boundary_idx].name.clone();
                let binding_id = kept_bindings[last_boundary_idx].value.id;
                return Some(CoreNode::new(
                    root.id,
                    CoreNodeKind::Let {
                        bindings: kept_bindings,
                        body: Box::new(CoreNode::new(
                            binding_id,
                            CoreNodeKind::Reference(CoreReference::Local(binding_name)),
                            CoreValueKind::Solid,
                        )),
                    },
                    root.value_kind,
                ));
            }
            let sliced_body = slice_mesh_phase_root(body, boundary_ids)?;
            Some(CoreNode::new(
                root.id,
                CoreNodeKind::Let {
                    bindings: bindings.clone(),
                    body: Box::new(sliced_body),
                },
                root.value_kind,
            ))
        }
        _ => {
            // Direct nesting: find the first boundary node and use it.
            let first = boundary_ids.iter().next().copied()?;
            find_node(root, first).cloned()
        }
    }
}

/// Clone a program for the **OCCT phase**. Hybrid mesh islands and complete
/// PureMesh parts become `solidify(import-stl(mesh_stl_path))`. PureOcct parts
/// pass through unchanged.
pub fn clone_program_for_occt_phase(
    program: &CoreProgram,
    partitions: &[PartPartition],
    mesh_stl_path: &str,
    next_node_id: NodeId,
) -> CoreProgram {
    let mesh_paths = partitions
        .iter()
        .enumerate()
        .flat_map(|(part_index, partition)| {
            mesh_bridge_output_node_ids(program, part_index, partition)
        })
        .map(|node_id| (node_id, mesh_stl_path.to_string()))
        .collect();
    clone_program_for_occt_phase_with_paths(program, partitions, &mesh_paths, next_node_id)
}

pub fn clone_program_for_occt_phase_with_mesh_assets(
    program: &CoreProgram,
    partitions: &[PartPartition],
    mesh_assets: &std::collections::HashMap<NodeId, crate::ecky_ir::mesh_asset::MeshAsset>,
    next_node_id: NodeId,
) -> crate::contracts::AppResult<CoreProgram> {
    let mut mesh_paths = std::collections::HashMap::new();
    for (part_index, partition) in partitions.iter().enumerate() {
        for output_node_id in mesh_bridge_output_node_ids(program, part_index, partition) {
            let asset = mesh_assets.get(&output_node_id).ok_or_else(|| {
                crate::contracts::AppError::internal(format!(
                    "Poly BRep bridge mesh asset missing for Core node {}.",
                    output_node_id.raw()
                ))
            })?;
            mesh_paths.insert(
                output_node_id,
                asset.stl_path().to_string_lossy().to_string(),
            );
        }
    }
    Ok(clone_program_for_occt_phase_with_paths(
        program,
        partitions,
        &mesh_paths,
        next_node_id,
    ))
}

fn clone_program_for_occt_phase_with_paths(
    program: &CoreProgram,
    partitions: &[PartPartition],
    mesh_paths: &std::collections::HashMap<NodeId, String>,
    next_node_id: NodeId,
) -> CoreProgram {
    let mut clone = program.clone();
    for (part_index, (part, partition)) in clone.parts.iter_mut().zip(partitions.iter()).enumerate()
    {
        let mut id_counter = next_node_id;
        for output_node_id in mesh_bridge_output_node_ids(program, part_index, partition) {
            if let Some(path) = mesh_paths.get(&output_node_id) {
                replace_node(&mut part.root, output_node_id, &mut id_counter, path);
            }
        }
        prune_unused_local_bindings(&mut part.root);
    }
    clone
}

fn prune_unused_local_bindings(node: &mut CoreNode) {
    match &mut node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => {}
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings.iter_mut() {
                prune_unused_local_bindings(&mut binding.value);
            }
            prune_unused_local_bindings(result);
            let mut required = local_references(result);
            let mut required_nodes = node_references(result);
            let mut kept = Vec::with_capacity(bindings.len());
            for binding in bindings.drain(..).rev() {
                if required.remove(binding.name.as_str())
                    || required_nodes.remove(&binding.value.id)
                {
                    required.extend(local_references(&binding.value));
                    required_nodes.extend(node_references(&binding.value));
                    kept.push(binding);
                }
            }
            kept.reverse();
            *bindings = kept;
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings.iter_mut() {
                prune_unused_local_bindings(&mut binding.value);
            }
            prune_unused_local_bindings(body);
            let mut required = local_references(body);
            let mut required_nodes = node_references(body);
            let mut kept = Vec::with_capacity(bindings.len());
            for binding in bindings.drain(..).rev() {
                if required.remove(binding.name.as_str())
                    || required_nodes.remove(&binding.value.id)
                {
                    required.extend(local_references(&binding.value));
                    required_nodes.extend(node_references(&binding.value));
                    kept.push(binding);
                }
            }
            kept.reverse();
            *bindings = kept;
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            prune_unused_local_bindings(condition);
            prune_unused_local_bindings(then_branch);
            prune_unused_local_bindings(else_branch);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                prune_unused_local_bindings(arg);
            }
            for keyword in keywords {
                prune_unused_local_bindings(keyword.source_node_mut());
            }
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                prune_unused_local_bindings(source);
            }
            prune_unused_local_bindings(body);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                prune_unused_local_bindings(arg);
            }
            prune_unused_local_bindings(list);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                prune_unused_local_bindings(item);
            }
        }
    }
}

fn local_references(node: &CoreNode) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    collect_local_references(node, &mut names);
    names
}

fn node_references(node: &CoreNode) -> std::collections::HashSet<NodeId> {
    let mut ids = std::collections::HashSet::new();
    collect_node_references(node, &mut ids);
    ids
}

fn collect_node_references(node: &CoreNode, ids: &mut std::collections::HashSet<NodeId>) {
    if let CoreNodeKind::Reference(CoreReference::Node(id)) = &node.kind {
        ids.insert(*id);
    }
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => {}
        CoreNodeKind::Range { start, end } => {
            collect_node_references(start, ids);
            collect_node_references(end, ids);
        }
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings {
                collect_node_references(&binding.value, ids);
            }
            collect_node_references(result, ids);
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                collect_node_references(&binding.value, ids);
            }
            collect_node_references(body, ids);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_node_references(condition, ids);
            collect_node_references(then_branch, ids);
            collect_node_references(else_branch, ids);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                collect_node_references(arg, ids);
            }
            for keyword in keywords {
                collect_node_references(keyword.source_node(), ids);
            }
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                collect_node_references(source, ids);
            }
            collect_node_references(body, ids);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                collect_node_references(arg, ids);
            }
            collect_node_references(list, ids);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                collect_node_references(item, ids);
            }
        }
    }
}

fn collect_local_references(node: &CoreNode, names: &mut std::collections::HashSet<String>) {
    match &node.kind {
        CoreNodeKind::Reference(CoreReference::Local(name)) => {
            names.insert(name.clone());
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => {}
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings {
                collect_local_references(&binding.value, names);
            }
            collect_local_references(result, names);
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                collect_local_references(&binding.value, names);
            }
            collect_local_references(body, names);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_local_references(condition, names);
            collect_local_references(then_branch, names);
            collect_local_references(else_branch, names);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                collect_local_references(arg, names);
            }
            for keyword in keywords {
                collect_local_references(keyword.source_node(), names);
            }
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                collect_local_references(source, names);
            }
            collect_local_references(body, names);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                collect_local_references(arg, names);
            }
            collect_local_references(list, names);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                collect_local_references(item, names);
            }
        }
    }
}

/// Check if a node's subtree contains any of the given IDs.
fn node_contains_any(node: &CoreNode, ids: &std::collections::HashSet<NodeId>) -> bool {
    if ids.contains(&node.id) {
        return true;
    }
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => false,
        CoreNodeKind::Build { bindings, result } => {
            bindings.iter().any(|b| node_contains_any(&b.value, ids))
                || node_contains_any(result, ids)
        }
        CoreNodeKind::Let { bindings, body } => {
            bindings.iter().any(|b| node_contains_any(&b.value, ids))
                || node_contains_any(body, ids)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            node_contains_any(condition, ids)
                || node_contains_any(then_branch, ids)
                || node_contains_any(else_branch, ids)
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            args.iter().any(|a| node_contains_any(a, ids))
                || keywords
                    .iter()
                    .any(|kw| node_contains_any(kw.source_node(), ids))
        }
        CoreNodeKind::Map { sources, body, .. } => {
            sources.iter().any(|s| node_contains_any(s, ids)) || node_contains_any(body, ids)
        }
        CoreNodeKind::Apply { args, list, .. } => {
            args.iter().any(|a| node_contains_any(a, ids)) || node_contains_any(list, ids)
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().any(|i| node_contains_any(i, ids))
        }
    }
}

/// Find a node by ID anywhere in the tree.
fn find_node(node: &CoreNode, target: NodeId) -> Option<&CoreNode> {
    if node.id == target {
        return Some(node);
    }
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => None,
        CoreNodeKind::Build { bindings, result } => find_node(result, target)
            .or_else(|| bindings.iter().find_map(|b| find_node(&b.value, target))),
        CoreNodeKind::Let { bindings, body } => find_node(body, target)
            .or_else(|| bindings.iter().find_map(|b| find_node(&b.value, target))),
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => find_node(condition, target)
            .or_else(|| find_node(then_branch, target))
            .or_else(|| find_node(else_branch, target)),
        CoreNodeKind::Call { args, keywords, .. } => {
            args.iter().find_map(|a| find_node(a, target)).or_else(|| {
                keywords
                    .iter()
                    .find_map(|kw| find_node(kw.source_node(), target))
            })
        }
        CoreNodeKind::Map { sources, body, .. } => sources
            .iter()
            .find_map(|s| find_node(s, target))
            .or_else(|| find_node(body, target)),
        CoreNodeKind::Apply { args, list, .. } => args
            .iter()
            .find_map(|a| find_node(a, target))
            .or_else(|| find_node(list, target)),
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().find_map(|i| find_node(i, target))
        }
    }
}

/// Whether a mesh-phase output depends on author-declared open-surface
/// geometry. Such output may preview as mesh, but must never enter OCCT
/// solidification or BRep booleans.
pub fn mesh_output_contains_open_mesh(
    program: &CoreProgram,
    part_index: usize,
    output_node_id: NodeId,
) -> bool {
    program
        .parts
        .get(part_index)
        .and_then(|part| find_node(&part.root, output_node_id))
        .is_some_and(node_contains_open_mesh)
}

/// Source operation that first requires BRep topology above an open mesh.
/// Used by render diagnostics so rejection names the blocked consumer.
pub fn open_mesh_brep_consumer_operation(
    program: &CoreProgram,
    part_index: usize,
) -> Option<&'static str> {
    program
        .parts
        .get(part_index)
        .and_then(|part| first_open_mesh_brep_consumer(&part.root))
}

fn first_open_mesh_brep_consumer(node: &CoreNode) -> Option<&'static str> {
    match &node.kind {
        CoreNodeKind::Call { op, args, keywords } => {
            let has_open_input = args.iter().any(node_contains_open_mesh)
                || keywords
                    .iter()
                    .any(|keyword| node_contains_open_mesh(keyword.source_node()));
            if operation_requires_brep(op) && has_open_input {
                return brep_operation_label(op);
            }
            args.iter()
                .find_map(first_open_mesh_brep_consumer)
                .or_else(|| {
                    keywords
                        .iter()
                        .find_map(|keyword| first_open_mesh_brep_consumer(keyword.source_node()))
                })
        }
        CoreNodeKind::Apply { op, args, list } => {
            let has_open_input =
                args.iter().any(node_contains_open_mesh) || node_contains_open_mesh(list);
            if operation_requires_brep(op) && has_open_input {
                return brep_operation_label(op);
            }
            args.iter()
                .find_map(first_open_mesh_brep_consumer)
                .or_else(|| first_open_mesh_brep_consumer(list))
        }
        CoreNodeKind::Build { bindings, result } => bindings
            .iter()
            .find_map(|binding| first_open_mesh_brep_consumer(&binding.value))
            .or_else(|| first_open_mesh_brep_consumer(result)),
        CoreNodeKind::Let { bindings, body } => bindings
            .iter()
            .find_map(|binding| first_open_mesh_brep_consumer(&binding.value))
            .or_else(|| first_open_mesh_brep_consumer(body)),
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => first_open_mesh_brep_consumer(condition)
            .or_else(|| first_open_mesh_brep_consumer(then_branch))
            .or_else(|| first_open_mesh_brep_consumer(else_branch)),
        CoreNodeKind::Map { sources, body, .. } => sources
            .iter()
            .find_map(first_open_mesh_brep_consumer)
            .or_else(|| first_open_mesh_brep_consumer(body)),
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().find_map(first_open_mesh_brep_consumer)
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => None,
    }
}

fn brep_operation_label(op: &CoreOperation) -> Option<&'static str> {
    match op {
        CoreOperation::Boolean(CoreBooleanOp::Union) => Some("union"),
        CoreOperation::Boolean(CoreBooleanOp::Difference) => Some("difference"),
        CoreOperation::Boolean(CoreBooleanOp::Intersection) => Some("intersection"),
        CoreOperation::Boolean(CoreBooleanOp::Xor) => Some("xor"),
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => Some("chamfer"),
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => Some("fillet"),
        CoreOperation::Surface(CoreSurfaceOp::Shell) => Some("shell"),
        CoreOperation::Surface(CoreSurfaceOp::Offset) => Some("offset"),
        CoreOperation::Surface(CoreSurfaceOp::OffsetRounded) => Some("offset-rounded"),
        CoreOperation::Custom(name) if name == "solidify" => Some("solidify"),
        _ => None,
    }
}

fn node_contains_open_mesh(node: &CoreNode) -> bool {
    match &node.kind {
        CoreNodeKind::Call { op, args, keywords } => {
            call_declares_open_mesh(op, keywords)
                || args.iter().any(node_contains_open_mesh)
                || keywords
                    .iter()
                    .any(|keyword| node_contains_open_mesh(keyword.source_node()))
        }
        CoreNodeKind::Build { bindings, result } => {
            bindings
                .iter()
                .any(|binding| node_contains_open_mesh(&binding.value))
                || node_contains_open_mesh(result)
        }
        CoreNodeKind::Let { bindings, body } => {
            bindings
                .iter()
                .any(|binding| node_contains_open_mesh(&binding.value))
                || node_contains_open_mesh(body)
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            node_contains_open_mesh(condition)
                || node_contains_open_mesh(then_branch)
                || node_contains_open_mesh(else_branch)
        }
        CoreNodeKind::Map { sources, body, .. } => {
            sources.iter().any(node_contains_open_mesh) || node_contains_open_mesh(body)
        }
        CoreNodeKind::Apply { op, args, list } => {
            matches!(op, CoreOperation::Custom(name) if name == "mesh")
                || args.iter().any(node_contains_open_mesh)
                || node_contains_open_mesh(list)
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            items.iter().any(node_contains_open_mesh)
        }
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => false,
    }
}

fn call_declares_open_mesh(op: &CoreOperation, keywords: &[CoreKeywordArg]) -> bool {
    match op {
        CoreOperation::Custom(name) if name == "mesh" => true,
        CoreOperation::Custom(name) if name == "surface-trim" => keywords
            .iter()
            .find(|keyword| keyword.name == "cap")
            .is_some_and(|keyword| {
                matches!(
                    &keyword.source_node().kind,
                    CoreNodeKind::Literal(CoreLiteral::Text(value)) if value == "open"
                )
            }),
        _ => false,
    }
}

/// Replace a node by ID with `solidify(import-stl(path))` everywhere it appears.
fn replace_node(node: &mut CoreNode, target: NodeId, id_counter: &mut NodeId, path: &str) {
    if node.id == target {
        *node = make_poly_mesh_node(node.id, path, id_counter);
        return;
    }
    match &mut node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => {}
        CoreNodeKind::Build { bindings, result } => {
            for b in bindings.iter_mut() {
                replace_node(&mut b.value, target, id_counter, path);
            }
            replace_node(result, target, id_counter, path);
        }
        CoreNodeKind::Let { bindings, body } => {
            for b in bindings.iter_mut() {
                replace_node(&mut b.value, target, id_counter, path);
            }
            replace_node(body, target, id_counter, path);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            replace_node(condition, target, id_counter, path);
            replace_node(then_branch, target, id_counter, path);
            replace_node(else_branch, target, id_counter, path);
        }
        CoreNodeKind::Call { op, args, keywords } => {
            let authored_solidify = matches!(op, CoreOperation::Custom(name) if name == "solidify");
            for a in args.iter_mut() {
                if authored_solidify && a.id == target {
                    *a = make_import_stl_node(a.id, path, id_counter);
                } else {
                    replace_node(a, target, id_counter, path);
                }
            }
            for kw in keywords.iter_mut() {
                let source = kw.source_node_mut();
                replace_node(source, target, id_counter, path);
            }
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for s in sources.iter_mut() {
                replace_node(s, target, id_counter, path);
            }
            replace_node(body, target, id_counter, path);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for a in args.iter_mut() {
                replace_node(a, target, id_counter, path);
            }
            replace_node(list, target, id_counter, path);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for i in items.iter_mut() {
                replace_node(i, target, id_counter, path);
            }
        }
    }
}

/// Construct a `solidify(import-stl(path))` node tree, reusing the boundary
/// node's ID for the outermost (solidify) node so existing topology references
/// stay valid.
fn make_poly_mesh_node(reuse_id: NodeId, path: &str, id_counter: &mut NodeId) -> CoreNode {
    let import_node = make_import_stl_node(
        {
            *id_counter = NodeId::new(id_counter.raw() + 1);
            *id_counter
        },
        path,
        id_counter,
    );

    CoreNode::new(
        reuse_id,
        CoreNodeKind::Call {
            op: CoreOperation::Custom("solidify".to_string()),
            args: vec![import_node],
            keywords: vec![],
        },
        CoreValueKind::Solid,
    )
}

fn make_import_stl_node(reuse_id: NodeId, path: &str, id_counter: &mut NodeId) -> CoreNode {
    let path_node_id = {
        *id_counter = NodeId::new(id_counter.raw() + 1);
        *id_counter
    };

    let path_node = CoreNode::new(
        path_node_id,
        CoreNodeKind::Literal(CoreLiteral::Text(path.to_string())),
        CoreValueKind::Text,
    );

    CoreNode::new(
        reuse_id,
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Stl),
            args: vec![path_node],
            keywords: vec![],
        },
        CoreValueKind::Solid,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_scheme::try_compile_to_core_program;

    fn partition_of(source: &str) -> PartRenderStrategy {
        let program = try_compile_to_core_program(source)
            .expect("compiled path")
            .expect("program");
        let partitions = analyze_program(&program);
        assert_eq!(partitions.len(), 1, "expected exactly one part");
        partitions[0].strategy
    }

    // -----------------------------------------------------------------------
    // Pure OCCT (no mesh-only ops)
    // -----------------------------------------------------------------------

    #[test]
    fn pure_occt_part_has_no_boundary() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (difference (extrude (circle 10) 20) (cylinder 3 30))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::PureOcct);
    }

    #[test]
    fn pure_occt_part_with_chamfer_and_fillet() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (chamfer 2 (fillet 3 (extrude (rectangle 20 20) 10)))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::PureOcct);
    }

    // -----------------------------------------------------------------------
    // Pure mesh (mesh-only ops, but no BRep-required consumer)
    // -----------------------------------------------------------------------

    #[test]
    fn wall_pattern_alone_is_pure_mesh() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::PureMesh);
    }

    #[test]
    fn mesh_literal_alone_is_pure_mesh() {
        let strategy = partition_of(
            r#"(model
                (part surface
                  (mesh
                    :vertices ((0 0 0) (10 0 0) (0 10 0))
                    :triangles ((0 1 2)))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::PureMesh);
    }

    #[test]
    fn imported_stl_alone_is_pure_mesh() {
        let strategy = partition_of(
            r#"(model
                (part scanned
                  (import-stl "/tmp/scanned.stl")))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::PureMesh);
    }

    #[test]
    fn multipart_pure_mesh_and_pure_occt_require_poly_brep_bridge() {
        let program = try_compile_to_core_program(
            r#"(model
                (part relief
                  (heightfield "/tmp/relief.png"
                    :width 12
                    :depth 8
                    :relief-height 2
                    :base-thickness 1))
                (part handle
                  (helical-ridge
                    :radius 4
                    :pitch 2
                    :height 8
                    :depth 0.8
                    :base-width 1.6
                    :crest-width 0.4)))"#,
        )
        .expect("compiled")
        .expect("program");
        let partitions = analyze_program(&program);

        assert_eq!(
            partitions
                .iter()
                .map(|partition| partition.strategy)
                .collect::<Vec<_>>(),
            vec![PartRenderStrategy::PureMesh, PartRenderStrategy::PureOcct]
        );
        assert!(requires_poly_brep_bridge(&partitions));
    }

    #[test]
    fn imported_stl_consumed_by_difference_is_hybrid() {
        let strategy = partition_of(
            r#"(model
                (part scanned
                  (difference
                    (import-stl "/tmp/scanned.stl")
                    (cylinder 3 20))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::Hybrid);
    }

    #[test]
    fn explicit_solidify_promotes_imported_stl_to_hybrid_brep() {
        let strategy = partition_of(
            r#"(model
                (part scanned
                  (solidify (import-stl "/tmp/scanned.stl"))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::Hybrid);
    }

    #[test]
    fn explicit_solidify_stays_out_of_mesh_phase() {
        let program = try_compile_to_core_program(
            r#"(model
                (part scanned
                  (solidify (import-stl "/tmp/scanned.stl"))))"#,
        )
        .expect("compiled")
        .expect("program");
        let partitions = analyze_program(&program);
        let mesh_program = clone_program_for_mesh_phase(&program, &partitions);

        assert!(matches!(
            &mesh_program.parts[0].root.kind,
            CoreNodeKind::Call {
                op: CoreOperation::Primitive(CorePrimitive::Stl),
                ..
            }
        ));
    }

    #[test]
    fn let_bound_explicit_solidify_stays_out_of_mesh_phase() {
        let program = try_compile_to_core_program(
            r#"(model
                (params
                  (number neck_cut_z 95 :label "Neck cut Z" :min 88 :max 102 :step 0.5))
                (part head_only
                  (let* ((donor (solidify (import-stl "/tmp/scanned.stl")))
                         (crop_box
                           (translate -10 -10 neck_cut_z
                             (box 100 100 200 :align '(min min min)))))
                    (translate 0 0 (- 0 neck_cut_z)
                      (intersection donor crop_box)))))"#,
        )
        .expect("compiled")
        .expect("program");
        let partitions = analyze_program(&program);
        assert_eq!(partitions[0].mesh_output_node_ids, vec![NodeId::new(3)]);
        let mesh_program = clone_program_for_mesh_phase(&program, &partitions);

        fn contains_solidify(node: &CoreNode) -> bool {
            match &node.kind {
                CoreNodeKind::Call { op, args, keywords } => {
                    matches!(op, CoreOperation::Custom(name) if name == "solidify")
                        || args.iter().any(contains_solidify)
                        || keywords
                            .iter()
                            .any(|keyword| contains_solidify(keyword.source_node()))
                }
                CoreNodeKind::Build { bindings, result } => {
                    bindings
                        .iter()
                        .any(|binding| contains_solidify(&binding.value))
                        || contains_solidify(result)
                }
                CoreNodeKind::Let { bindings, body } => {
                    bindings
                        .iter()
                        .any(|binding| contains_solidify(&binding.value))
                        || contains_solidify(body)
                }
                CoreNodeKind::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    contains_solidify(condition)
                        || contains_solidify(then_branch)
                        || contains_solidify(else_branch)
                }
                CoreNodeKind::Map { sources, body, .. } => {
                    sources.iter().any(contains_solidify) || contains_solidify(body)
                }
                CoreNodeKind::Apply { args, list, .. } => {
                    args.iter().any(contains_solidify) || contains_solidify(list)
                }
                CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
                    items.iter().any(contains_solidify)
                }
                CoreNodeKind::Literal(_)
                | CoreNodeKind::Reference(_)
                | CoreNodeKind::Range { .. } => false,
            }
        }

        assert!(
            !contains_solidify(&mesh_program.parts[0].root),
            "mesh root: {:#?}; partitions: {:#?}",
            mesh_program.parts[0].root,
            partitions
        );
    }

    #[test]
    fn explicit_solidify_is_not_duplicated_in_occt_phase() {
        fn count_solidify(node: &CoreNode) -> usize {
            match &node.kind {
                CoreNodeKind::Call { op, args, keywords } => {
                    usize::from(matches!(op, CoreOperation::Custom(name) if name == "solidify"))
                        + args.iter().map(count_solidify).sum::<usize>()
                        + keywords
                            .iter()
                            .map(|keyword| count_solidify(keyword.source_node()))
                            .sum::<usize>()
                }
                CoreNodeKind::Build { bindings, result } => {
                    bindings
                        .iter()
                        .map(|binding| count_solidify(&binding.value))
                        .sum::<usize>()
                        + count_solidify(result)
                }
                CoreNodeKind::Let { bindings, body } => {
                    bindings
                        .iter()
                        .map(|binding| count_solidify(&binding.value))
                        .sum::<usize>()
                        + count_solidify(body)
                }
                CoreNodeKind::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    count_solidify(condition)
                        + count_solidify(then_branch)
                        + count_solidify(else_branch)
                }
                CoreNodeKind::Map { sources, body, .. } => {
                    sources.iter().map(count_solidify).sum::<usize>() + count_solidify(body)
                }
                CoreNodeKind::Apply { args, list, .. } => {
                    args.iter().map(count_solidify).sum::<usize>() + count_solidify(list)
                }
                CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
                    items.iter().map(count_solidify).sum()
                }
                CoreNodeKind::Literal(_)
                | CoreNodeKind::Reference(_)
                | CoreNodeKind::Range { .. } => 0,
            }
        }

        let program = try_compile_to_core_program(
            r#"(model
                (part scanned
                  (solidify (import-stl "/tmp/scanned.stl"))))"#,
        )
        .expect("compiled")
        .expect("program");
        let partitions = analyze_program(&program);
        let occt_program = clone_program_for_occt_phase(
            &program,
            &partitions,
            "/tmp/mesh-phase.stl",
            NodeId::new(999_999),
        );

        assert_eq!(count_solidify(&occt_program.parts[0].root), 1);
    }

    #[test]
    fn occt_phase_preserves_build_dependencies_for_solidified_stl_difference() {
        let program = try_compile_to_core_program(
            r#"(model
                (params
                  (number bore-x 1mm :min -4mm :max 4mm :step 0.1mm)
                  (number bore-z 4.9mm :min 3mm :max 7mm :step 0.1mm))
                (part dog-cap
                  (let* ((scale-factor 0.30)
                         (floor-z 14.67918mm)
                         (bore-r 3.24mm)
                         (lead-r (+ bore-r 0.15mm)))
                    (build
                      (shape dog
                        (translate 0 0 floor-z
                          (scale scale-factor scale-factor scale-factor
                            (solidify (import-stl "/tmp/dog.stl")))))
                      (shape lead
                        (cone lead-r bore-r 1mm))
                      (shape shaft
                        (translate 0 0 0.8mm
                          (cylinder bore-r 6mm)))
                      (shape cutter
                        (union lead shaft))
                      (shape placed-cutter
                        (translate bore-x 1.5mm bore-z cutter))
                      (result
                        (difference dog placed-cutter))))))"#,
        )
        .expect("compiled")
        .expect("program");
        let partitions = analyze_program(&program);
        assert_eq!(partitions[0].strategy, PartRenderStrategy::Hybrid);

        let occt_program = clone_program_for_occt_phase(
            &program,
            &partitions,
            "/tmp/dog-mesh-phase.stl",
            NodeId::new(1_000_000),
        );

        crate::ecky_cad_host::direct_occt::plan_core_program(&occt_program)
            .expect("solidified dog build dependencies should remain resolvable");
    }

    #[test]
    fn wall_pattern_then_translate_is_pure_mesh() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (translate 0 0 5
                    (wall-pattern (:mode gyroid :depth 0.6)
                      (extrude (circle 10) 20)))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::PureMesh);
    }

    #[test]
    fn wall_pattern_then_scale_is_pure_mesh() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (scale 1.2
                    (wall-pattern (:mode cellular :depth 0.5)
                      (extrude (rectangle 20 20) 10)))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::PureMesh);
    }

    #[test]
    fn chained_wall_patterns_are_pure_mesh() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.3)
                    (wall-pattern (:mode gyroid :depth 0.4)
                      (extrude (circle 5) 18)))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::PureMesh);
    }

    // -----------------------------------------------------------------------
    // Hybrid (mesh-only op consumed by BRep-required op)
    // -----------------------------------------------------------------------

    #[test]
    fn wall_pattern_then_difference_is_hybrid() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (difference
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20))
                    (cylinder 3 30))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::Hybrid);
    }

    #[test]
    fn polyhedron_then_difference_is_hybrid() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (difference
                    (polyhedron
                      :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10))
                      :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))
                    (sphere 1))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::Hybrid);
    }

    #[test]
    fn open_mesh_hybrid_output_is_identified_for_solidification_rejection() {
        let program = try_compile_to_core_program(
            r#"(model
                (part body
                  (difference
                    (translate 0 0 1
                      (mesh
                        :vertices ((0 0 0) (10 0 0) (0 10 0))
                        :triangles ((0 1 2))))
                    (sphere 1))))"#,
        )
        .expect("compiled path")
        .expect("program");
        let partition = &analyze_program(&program)[0];
        assert_eq!(partition.strategy, PartRenderStrategy::Hybrid);
        assert!(mesh_output_contains_open_mesh(
            &program,
            0,
            partition.mesh_output_node_ids[0]
        ));
    }

    #[test]
    fn wall_pattern_then_chamfer_is_hybrid() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (chamfer 2
                    (wall-pattern (:mode gyroid :depth 0.6)
                      (extrude (circle 10) 20)))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::Hybrid);
    }

    #[test]
    fn wall_pattern_then_fillet_is_hybrid() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (fillet 2
                    (wall-pattern (:mode cellular :depth 0.5)
                      (extrude (circle 10) 20)))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::Hybrid);
    }

    #[test]
    fn wall_pattern_then_union_is_hybrid() {
        let strategy = partition_of(
            r#"(model
                (part body
                  (union
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20))
                    (box 5 5 5))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::Hybrid);
    }

    #[test]
    fn mesh_boolean_boundary_preserves_ordered_difference_operands() {
        let program = try_compile_to_core_program(
            r#"(model
                (part body
                  (difference
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20))
                    (cylinder 3 30))))"#,
        )
        .expect("compiled path")
        .expect("program");

        let partition = &analyze_program(&program)[0];
        let boundary = partition
            .mesh_boolean_boundaries
            .first()
            .expect("mesh boolean boundary");
        let CoreNodeKind::Call { args, .. } = &program.parts[0].root.kind else {
            panic!("difference root");
        };

        assert_eq!(boundary.operation, CoreBooleanOp::Difference);
        assert_eq!(
            boundary.operand_node_ids,
            args.iter().map(|arg| arg.id).collect::<Vec<_>>()
        );
        assert_eq!(boundary.mesh_operand_indices, vec![0]);
        assert!(boundary.supports_batch_boolean());
    }

    #[test]
    fn mesh_boolean_boundary_never_batches_across_parts() {
        let program = try_compile_to_core_program(
            r#"(model
                (part first
                  (union
                    (wall-pattern (:mode ribs :depth 0.4) (box 10 10 10))
                    (box 2 2 2)))
                (part second
                  (difference
                    (box 8 8 8)
                    (wall-pattern (:mode cellular :depth 0.3) (sphere 2)))))"#,
        )
        .expect("compiled path")
        .expect("program");

        let partitions = analyze_program(&program);
        assert_eq!(partitions.len(), 2);
        assert_eq!(partitions[0].mesh_boolean_boundaries.len(), 1);
        assert_eq!(partitions[1].mesh_boolean_boundaries.len(), 1);
        assert_eq!(
            partitions[0].mesh_boolean_boundaries[0].operation,
            CoreBooleanOp::Union
        );
        assert_eq!(
            partitions[1].mesh_boolean_boundaries[0].operation,
            CoreBooleanOp::Difference
        );
        assert_ne!(
            partitions[0].mesh_boolean_boundaries[0].boolean_node_id,
            partitions[1].mesh_boolean_boundaries[0].boolean_node_id
        );
    }

    // -----------------------------------------------------------------------
    // Wall-pattern in a sibling branch (cutter side of difference)
    // -----------------------------------------------------------------------

    #[test]
    fn wall_pattern_in_cutter_branch_is_hybrid() {
        // The difference consumes wall-pattern output from arg1 (the cutter).
        // This is still hybrid because difference needs to boolean against
        // a poly face shape.
        let strategy = partition_of(
            r#"(model
                (part body
                  (difference
                    (extrude (circle 10) 20)
                    (wall-pattern (:mode ribs :depth 0.4)
                      (cylinder 3 30)))))"#,
        );
        assert_eq!(strategy, PartRenderStrategy::Hybrid);
    }

    // -----------------------------------------------------------------------
    // Multi-part programs
    // -----------------------------------------------------------------------

    #[test]
    fn multi_part_program_classifies_each_part_independently() {
        let program = try_compile_to_core_program(
            r#"(model
                (part exact-body (extrude (circle 10) 20))
                (part mesh-body
                  (wall-pattern (:mode ribs :depth 0.4)
                    (extrude (circle 5) 18)))
                (part hybrid-body
                  (difference
                    (wall-pattern (:mode gyroid :depth 0.6)
                      (extrude (circle 8) 15))
                    (cylinder 2 20))))"#,
        )
        .expect("compiled path")
        .expect("program");

        let partitions = analyze_program(&program);
        assert_eq!(partitions.len(), 3);
        assert_eq!(partitions[0].strategy, PartRenderStrategy::PureOcct);
        assert_eq!(partitions[1].strategy, PartRenderStrategy::PureMesh);
        assert_eq!(partitions[2].strategy, PartRenderStrategy::Hybrid);
    }

    // -----------------------------------------------------------------------
    // Boundary node IDs
    // -----------------------------------------------------------------------

    #[test]
    fn hybrid_partition_records_boundary_node_ids() {
        let program = try_compile_to_core_program(
            r#"(model
                (part body
                  (difference
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20))
                    (cylinder 3 30))))"#,
        )
        .expect("compiled path")
        .expect("program");

        let partitions = analyze_program(&program);
        assert_eq!(partitions[0].boundary_node_ids.len(), 1);
        assert!(partitions[0].has_post_boundary_brep_op);
    }

    // -----------------------------------------------------------------------
    // Tree slicing for hybrid dispatch
    // -----------------------------------------------------------------------

    #[test]
    fn mesh_phase_program_strips_post_boundary_ops() {
        let program = try_compile_to_core_program(
            r#"(model
                (part body
                  (difference
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20))
                    (cylinder 3 30))))"#,
        )
        .expect("compiled")
        .expect("program");

        let partitions = analyze_program(&program);
        let mesh_program = clone_program_for_mesh_phase(&program, &partitions);

        // The mesh phase root should be the wall-pattern node, not the
        // difference node. So the root op is wall-pattern (Custom).
        match &mesh_program.parts[0].root.kind {
            CoreNodeKind::Call { op, .. } => {
                assert!(matches!(
                    op,
                    CoreOperation::Custom(name) if name == "wall-pattern"
                ));
            }
            other => panic!("expected wall-pattern call, got {other:?}"),
        }
    }

    #[test]
    fn occt_phase_program_replaces_boundary_with_poly_mesh() {
        let program = try_compile_to_core_program(
            r#"(model
                (part body
                  (difference
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20))
                    (cylinder 3 30))))"#,
        )
        .expect("compiled")
        .expect("program");

        let partitions = analyze_program(&program);
        let occt_program = clone_program_for_occt_phase(
            &program,
            &partitions,
            "/tmp/test-mesh.stl",
            NodeId::new(999_999),
        );

        // Root should still be difference, but arg[0] should be
        // solidify(import-stl(path)), not wall-pattern.
        match &occt_program.parts[0].root.kind {
            CoreNodeKind::Call {
                op: CoreOperation::Boolean(crate::ecky_core_ir::CoreBooleanOp::Difference),
                args,
                ..
            } => {
                // arg[0] = solidify(import-stl(...))
                match &args[0].kind {
                    CoreNodeKind::Call {
                        op: CoreOperation::Custom(name),
                        args: solidify_args,
                        ..
                    } if name == "solidify" => match &solidify_args[0].kind {
                        CoreNodeKind::Call {
                            op: CoreOperation::Primitive(CorePrimitive::Stl),
                            args: import_args,
                            ..
                        } => match &import_args[0].kind {
                            CoreNodeKind::Literal(CoreLiteral::Text(path)) => {
                                assert_eq!(path, "/tmp/test-mesh.stl");
                            }
                            other => panic!("expected text path, got {other:?}"),
                        },
                        other => panic!("expected import-stl, got {other:?}"),
                    },
                    other => panic!("expected solidify, got {other:?}"),
                }
            }
            other => panic!("expected difference, got {other:?}"),
        }
    }

    #[test]
    fn mesh_phase_program_passes_through_pure_occt_parts() {
        let program = try_compile_to_core_program(
            r#"(model
                (part exact (box 10 10 10))
                (part hybrid
                  (difference
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20))
                    (cylinder 3 30))))"#,
        )
        .expect("compiled")
        .expect("program");

        let partitions = analyze_program(&program);
        let mesh_program = clone_program_for_mesh_phase(&program, &partitions);

        // PureOcct parts are stripped from the mesh phase (OCCT handles them).
        // Only Hybrid parts remain.
        assert_eq!(
            mesh_program.parts.len(),
            1,
            "mesh phase should only contain Hybrid parts"
        );
        // The remaining part (hybrid) should be stripped to wall-pattern.
        assert!(matches!(
            &mesh_program.parts[0].root.kind,
            CoreNodeKind::Call {
                op: CoreOperation::Custom(name),
                ..
            } if name == "wall-pattern"
        ));
    }

    #[test]
    fn closed_flat_surface_trim_inside_solidify_then_difference_is_hybrid_and_outputs_stay_closed()
    {
        let program = try_compile_to_core_program(
            r#"(model
                (part scanned
                  (difference
                    (solidify
                      (surface-trim
                        (import-stl "/tmp/scanned.stl")
                        :schema-version 1
                        :source-digest "sha256:fixture"
                        :loop ((mesh-anchor 0 1 0 0) (mesh-anchor 1 0 1 0) (mesh-anchor 2 0 0 1))
                        :keep-seed (mesh-anchor 3 1 0 0)
                        :path-mode "feature"
                        :cap "flat"))
                    (cylinder 3 20))))"#,
        )
        .expect("compiled")
        .expect("program");

        let partitions = analyze_program(&program);
        assert_eq!(partitions.len(), 1);
        assert_eq!(partitions[0].strategy, PartRenderStrategy::Hybrid);
        assert!(!partitions[0].mesh_output_node_ids.is_empty());
        assert!(partitions[0]
            .mesh_output_node_ids
            .iter()
            .all(|&node_id| { !mesh_output_contains_open_mesh(&program, 0, node_id) }));
    }

    #[test]
    fn open_surface_trim_inside_solidify_is_hybrid_and_reports_open_mesh_consumer() {
        let program = try_compile_to_core_program(
            r#"(model
                (part scanned
                  (solidify
                    (surface-trim
                      (import-stl "/tmp/scanned.stl")
                      :schema-version 1
                      :source-digest "sha256:fixture"
                      :loop ((mesh-anchor 0 1 0 0) (mesh-anchor 1 0 1 0) (mesh-anchor 2 0 0 1))
                      :keep-seed (mesh-anchor 3 1 0 0)
                      :path-mode "feature"
                      :cap "open"))))"#,
        )
        .expect("compiled")
        .expect("program");

        let partitions = analyze_program(&program);
        assert_eq!(partitions.len(), 1);
        assert_eq!(partitions[0].strategy, PartRenderStrategy::Hybrid);
        assert!(!partitions[0].mesh_output_node_ids.is_empty());
        assert!(partitions[0]
            .mesh_output_node_ids
            .iter()
            .any(|&node_id| mesh_output_contains_open_mesh(&program, 0, node_id)));
        assert_eq!(
            open_mesh_brep_consumer_operation(&program, 0),
            Some("solidify")
        );
    }

    #[test]
    fn clone_program_for_mesh_phase_keeps_surface_trim_and_removes_solidify() {
        fn count_custom_call(node: &CoreNode, target: &str) -> usize {
            match &node.kind {
                CoreNodeKind::Call { op, args, keywords } => {
                    usize::from(matches!(op, CoreOperation::Custom(name) if name == target))
                        + args
                            .iter()
                            .map(|arg| count_custom_call(arg, target))
                            .sum::<usize>()
                        + keywords
                            .iter()
                            .map(|keyword| count_custom_call(keyword.source_node(), target))
                            .sum::<usize>()
                }
                CoreNodeKind::Build { bindings, result } => {
                    bindings
                        .iter()
                        .map(|binding| count_custom_call(&binding.value, target))
                        .sum::<usize>()
                        + count_custom_call(result, target)
                }
                CoreNodeKind::Let { bindings, body } => {
                    bindings
                        .iter()
                        .map(|binding| count_custom_call(&binding.value, target))
                        .sum::<usize>()
                        + count_custom_call(body, target)
                }
                CoreNodeKind::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    count_custom_call(condition, target)
                        + count_custom_call(then_branch, target)
                        + count_custom_call(else_branch, target)
                }
                CoreNodeKind::Map { sources, body, .. } => {
                    sources
                        .iter()
                        .map(|source| count_custom_call(source, target))
                        .sum::<usize>()
                        + count_custom_call(body, target)
                }
                CoreNodeKind::Apply { args, list, .. } => {
                    args.iter()
                        .map(|arg| count_custom_call(arg, target))
                        .sum::<usize>()
                        + count_custom_call(list, target)
                }
                CoreNodeKind::List(items) | CoreNodeKind::Group(items) => items
                    .iter()
                    .map(|item| count_custom_call(item, target))
                    .sum(),
                CoreNodeKind::Literal(_)
                | CoreNodeKind::Reference(_)
                | CoreNodeKind::Range { .. } => 0,
            }
        }

        let program = try_compile_to_core_program(
            r#"(model
                (part scanned
                  (difference
                    (solidify
                      (surface-trim
                        (import-stl "/tmp/scanned.stl")
                        :schema-version 1
                        :source-digest "sha256:fixture"
                        :loop ((mesh-anchor 0 1 0 0) (mesh-anchor 1 0 1 0) (mesh-anchor 2 0 0 1))
                        :keep-seed (mesh-anchor 3 1 0 0)
                        :path-mode "feature"
                        :cap "flat"))
                    (cylinder 3 20))))"#,
        )
        .expect("compiled")
        .expect("program");
        let partitions = analyze_program(&program);
        let mesh_program = clone_program_for_mesh_phase(&program, &partitions);

        assert!(count_custom_call(&mesh_program.parts[0].root, "surface-trim") >= 1);
        assert_eq!(
            count_custom_call(&mesh_program.parts[0].root, "solidify"),
            0
        );
    }

    #[test]
    fn mesh_phase_stops_before_post_boundary_chamfer() {
        let program = try_compile_to_core_program(
            r#"(model
                (part hybrid
                  (let* (
                    (raw (extrude (circle 10) 20))
                    (patterned
                      (if true
                        (wall-pattern (:mode ribs :depth 0.4) raw)
                        raw))
                    (finished (chamfer 0.5 :edges "bottom" patterned))
                    (body (union finished (box 4 4 4))))
                    (difference body (cylinder 3 30)))))"#,
        )
        .expect("compiled")
        .expect("program");

        let partitions = analyze_program(&program);
        let mesh_program = clone_program_for_mesh_phase(&program, &partitions);

        fn terminal_reference(node: &CoreNode) -> Option<&str> {
            match &node.kind {
                CoreNodeKind::Build { result, .. } => terminal_reference(result),
                CoreNodeKind::Let { body, .. } => terminal_reference(body),
                CoreNodeKind::Reference(CoreReference::Local(name)) => Some(name.as_str()),
                _ => None,
            }
        }

        fn binding_node_id(node: &CoreNode, needle: &str) -> Option<NodeId> {
            match &node.kind {
                CoreNodeKind::Build { bindings, result } => bindings
                    .iter()
                    .find(|binding| binding.name.contains(needle))
                    .map(|binding| binding.value.id)
                    .or_else(|| binding_node_id(result, needle)),
                CoreNodeKind::Let { bindings, body } => bindings
                    .iter()
                    .find(|binding| binding.name.contains(needle))
                    .map(|binding| binding.value.id)
                    .or_else(|| binding_node_id(body, needle)),
                _ => None,
            }
        }

        fn binding_value<'a>(node: &'a CoreNode, needle: &str) -> Option<&'a CoreNode> {
            match &node.kind {
                CoreNodeKind::Build { bindings, result } => bindings
                    .iter()
                    .find(|binding| binding.name.contains(needle))
                    .map(|binding| &binding.value)
                    .or_else(|| binding_value(result, needle)),
                CoreNodeKind::Let { bindings, body } => bindings
                    .iter()
                    .find(|binding| binding.name.contains(needle))
                    .map(|binding| &binding.value)
                    .or_else(|| binding_value(body, needle)),
                _ => None,
            }
        }

        let patterned_id =
            binding_node_id(&program.parts[0].root, "patterned").expect("patterned binding");
        let finished_id =
            binding_node_id(&program.parts[0].root, "finished").expect("finished binding");
        assert!(
            partitions[0].mesh_output_node_ids.contains(&patterned_id),
            "mesh output must stop at mesh-origin node before chamfer: {:?}",
            partitions[0]
        );
        assert!(
            !partitions[0].mesh_output_node_ids.contains(&finished_id),
            "post-boundary chamfer must not be a mesh output: {:?}",
            partitions[0]
        );
        assert!(
            terminal_reference(&mesh_program.parts[0].root)
                .is_some_and(|name| name.contains("patterned")),
            "root={:?}, partition={:?}",
            mesh_program.parts[0].root,
            partitions[0]
        );
        assert!(
            binding_value(&mesh_program.parts[0].root, "finished").is_none(),
            "mesh phase must not include post-boundary chamfer"
        );
    }

    #[test]
    fn occt_phase_applies_chamfer_after_mesh_solidification() {
        let program = try_compile_to_core_program(
            r#"(model
                (part hybrid
                  (difference
                    (chamfer 0.5 :edges "bottom"
                      (wall-pattern (:mode ribs :depth 0.4)
                        (extrude (circle 10) 20)))
                    (cylinder 3 30))))"#,
        )
        .expect("compiled")
        .expect("program");
        let partitions = analyze_program(&program);
        let output_id = partitions[0].mesh_output_node_ids[0];
        let boundary_id = partitions[0].boundary_node_ids[0];
        assert_eq!(
            output_id, boundary_id,
            "mesh output must be the mesh-origin boundary, not the chamfer node"
        );

        let occt_program = clone_program_for_occt_phase(
            &program,
            &partitions,
            "/tmp/generated-mesh.stl",
            NodeId::new(1_000_000),
        );

        assert!(matches!(
            &find_node(&occt_program.parts[0].root, output_id)
                .expect("mesh output replacement")
                .kind,
            CoreNodeKind::Call {
                op: CoreOperation::Custom(name),
                ..
            } if name == "solidify"
        ));
        assert!(
            find_node(&occt_program.parts[0].root, boundary_id).is_some(),
            "solidified mesh boundary must remain as chamfer operand in OCCT phase"
        );
        assert!(
            has_surface_op(&occt_program.parts[0].root, &CoreSurfaceOp::Chamfer),
            "OCCT phase must keep chamfer above solidified mesh"
        );
    }

    #[test]
    fn mesh_origin_surface_admission_rejects_broad_chamfer() {
        let program = try_compile_to_core_program(
            r#"(model
                (part hybrid
                  (chamfer 0.5 :edges "all"
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20)))))"#,
        )
        .expect("compiled")
        .expect("program");

        let issues = mesh_origin_surface_op_admission_issues(&program);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].operation, "chamfer");
        assert_eq!(issues[0].selector, "all");
        assert!(
            issues[0].reason.contains("broad selectors are rejected"),
            "{:?}",
            issues[0]
        );
    }

    #[test]
    fn mesh_origin_surface_admission_accepts_exact_edge_targets() {
        let program = try_compile_to_core_program(
            r#"(model
                (part hybrid
                  (chamfer 0.5 :edges "target-id:body:edge:0:0-0-0_1-0-0"
                    (wall-pattern (:mode ribs :depth 0.4)
                      (extrude (circle 10) 20)))))"#,
        )
        .expect("compiled")
        .expect("program");

        let issues = mesh_origin_surface_op_admission_issues(&program);

        assert!(issues.is_empty(), "{issues:?}");
    }

    fn has_surface_op(node: &CoreNode, expected: &CoreSurfaceOp) -> bool {
        match &node.kind {
            CoreNodeKind::Call { op, args, keywords } => {
                matches!(op, CoreOperation::Surface(surface) if surface == expected)
                    || args.iter().any(|arg| has_surface_op(arg, expected))
                    || keywords
                        .iter()
                        .any(|keyword| has_surface_op(keyword.source_node(), expected))
            }
            CoreNodeKind::Build { bindings, result } => {
                bindings
                    .iter()
                    .any(|binding| has_surface_op(&binding.value, expected))
                    || has_surface_op(result, expected)
            }
            CoreNodeKind::Let { bindings, body } => {
                bindings
                    .iter()
                    .any(|binding| has_surface_op(&binding.value, expected))
                    || has_surface_op(body, expected)
            }
            CoreNodeKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                has_surface_op(condition, expected)
                    || has_surface_op(then_branch, expected)
                    || has_surface_op(else_branch, expected)
            }
            CoreNodeKind::Map { sources, body, .. } => {
                sources
                    .iter()
                    .any(|source| has_surface_op(source, expected))
                    || has_surface_op(body, expected)
            }
            CoreNodeKind::Apply { args, list, .. } => {
                args.iter().any(|arg| has_surface_op(arg, expected))
                    || has_surface_op(list, expected)
            }
            CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
                items.iter().any(|item| has_surface_op(item, expected))
            }
            CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) | CoreNodeKind::Range { .. } => {
                false
            }
        }
    }

    #[test]
    fn occt_phase_prunes_dead_mesh_bindings_after_asset_injection() {
        let program = try_compile_to_core_program(
            r#"(model
                (part hybrid
                  (let* (
                    (raw (extrude (circle 10) 20))
                    (patterned (wall-pattern (:mode ribs :depth 0.4) raw))
                    (finished (chamfer 0.5 :edges "bottom" patterned)))
                    (difference finished (cylinder 3 30)))))"#,
        )
        .expect("compiled")
        .expect("program");
        let partitions = analyze_program(&program);
        let boundary_id = partitions[0].boundary_node_ids[0];

        let occt_program = clone_program_for_occt_phase(
            &program,
            &partitions,
            "/tmp/generated-mesh.stl",
            NodeId::new(1_000_000),
        );

        assert!(
            find_node(&occt_program.parts[0].root, boundary_id).is_some(),
            "mesh boundary should be replaced by solidify and remain reachable"
        );
    }
}
