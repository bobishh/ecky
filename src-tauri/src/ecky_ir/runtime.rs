use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use csgrs::float_types::parry3d::na::Vector3;
use csgrs::traits::CSG;
use sha2::{Digest, Sha256};

use crate::contracts::{
    AnalysisDeclarationBinding, AppError, AppResult, ArtifactBundle, DesignParams,
    DocumentMetadata, EngineKind, ExportArtifact, FeatureGraph, FeatureNode, FeatureOutputRef,
    GeometryBackend, GeometryProvenance, GeometryRepresentation, ManifestBounds, ModelManifest,
    ModelSourceKind, ParamValue, ParameterGroup, ParsedParamsResult, PartBinding, PreviewView,
    PreviewViewOffset, SelectionTarget, SourceLanguage, SourceRef, ViewerAsset, ViewerAssetFormat,
    MODEL_RUNTIME_SCHEMA_VERSION,
};
use crate::models::PathResolver;

use super::eval_scalar::eval_stringish;
use super::mesh_ops::{eval_geometry_expr, sanitize_mesh_for_export};
use super::model::{
    build_param_env, core_program_param_defaults, materialize_selector_nodes, parse_model,
    parsed_params_from_core_program, parsed_params_from_model, IrExpr, IrModel,
};
use super::shared::{unsupported, validation, IrMesh};
use super::syntax::canonicalize;
use crate::ecky_core_ir::{
    CoreArrayOp, CoreBooleanOp, CoreFeatureDecl, CoreFrameOp, CoreLiteral, CoreMetaOp, CoreNode,
    CoreNodeKind, CoreOperation, CorePart, CorePathOp, CorePrimitive, CoreProgram, CoreReference,
    CoreSelectorPayload, CoreSurfaceOp, CoreSymbol, CoreTransformOp, CoreValueKind, SourceSpan,
};
use crate::ecky_ir::edge_ops::{
    edge_selector_spec_from_core_payload, face_selector_spec_from_core_payload,
};

pub(super) const MODEL_RUNTIME_ROOT: &str = "model-runtime";
pub(super) const GENERATED_ARTIFACT_DIR: &str = "generated";
pub(super) const BUNDLE_FILE_NAME: &str = "bundle.json";
pub(super) const MANIFEST_FILE_NAME: &str = "manifest.json";
pub(super) const SOURCE_FILE_NAME: &str = "source.ecky";
pub(super) const MODEL_STL_FILE_NAME: &str = "model.stl";
pub(super) const PARTS_DIR_NAME: &str = "parts";
const CORE_AST_SCHEMA_VERSION: u32 = 1;
pub(super) fn mesh_volume(mesh: &IrMesh) -> Option<f64> {
    let tri_mesh = mesh.triangulate();
    if tri_mesh.polygons.is_empty() {
        return None;
    }
    let mut volume = 0.0f64;
    for poly in &tri_mesh.polygons {
        if poly.vertices.len() != 3 {
            continue;
        }
        let a = &poly.vertices[0].pos;
        let b = &poly.vertices[1].pos;
        let c = &poly.vertices[2].pos;
        // Signed volume of tetrahedron formed with origin
        let cross = Vector3::new(
            b.y * c.z - b.z * c.y,
            b.z * c.x - b.x * c.z,
            b.x * c.y - b.y * c.x,
        );
        volume += a.x * cross.x + a.y * cross.y + a.z * cross.z;
    }
    let vol = (volume / 6.0).abs();
    if vol.is_finite() && vol > 0.0 {
        Some(vol)
    } else {
        None
    }
}

/// Compute the total surface area of a triangulated mesh.
///
/// For each triangle with vertices (a, b, c):
///   area = ||(b - a) × (c - a)|| / 2
pub(super) fn mesh_area(mesh: &IrMesh) -> Option<f64> {
    let tri_mesh = mesh.triangulate();
    if tri_mesh.polygons.is_empty() {
        return None;
    }
    let mut area = 0.0f64;
    for poly in &tri_mesh.polygons {
        if poly.vertices.len() != 3 {
            continue;
        }
        let a = &poly.vertices[0].pos;
        let b = &poly.vertices[1].pos;
        let c = &poly.vertices[2].pos;
        let ab = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let ac = Vector3::new(c.x - a.x, c.y - a.y, c.z - a.z);
        let cross = ab.cross(&ac);
        area += cross.norm();
    }
    let result = area / 2.0;
    if result.is_finite() && result > 0.0 {
        Some(result)
    } else {
        None
    }
}

pub(super) fn bounds_from_mesh(mesh: &IrMesh) -> ManifestBounds {
    let bb = mesh.bounding_box();
    ManifestBounds {
        x_min: bb.mins.x,
        y_min: bb.mins.y,
        z_min: bb.mins.z,
        x_max: bb.maxs.x,
        y_max: bb.maxs.y,
        z_max: bb.maxs.z,
    }
}

pub(super) fn runtime_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    let root = app.app_data_dir().join(MODEL_RUNTIME_ROOT);
    fs::create_dir_all(&root).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(root)
}

pub(super) fn bundle_dir(app: &dyn PathResolver, model_id: &str) -> AppResult<PathBuf> {
    let path = runtime_root(app)?
        .join(GENERATED_ARTIFACT_DIR)
        .join(model_id);
    fs::create_dir_all(&path).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(path)
}

pub(super) fn write_bundle(path: &Path, bundle: &ArtifactBundle) -> AppResult<()> {
    let data = serde_json::to_string_pretty(bundle)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

pub(super) fn write_manifest(path: &Path, manifest: &ModelManifest) -> AppResult<()> {
    let data = serde_json::to_string_pretty(manifest)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

pub fn derive_controls(source: &str) -> AppResult<ParsedParamsResult> {
    let model = parse_model(source)?;
    derive_controls_from_model(&model)
}

pub(crate) fn derive_controls_from_core_program(
    program: &CoreProgram,
) -> AppResult<ParsedParamsResult> {
    parsed_params_from_core_program(program)
}

pub(crate) fn derive_controls_from_model(model: &IrModel) -> AppResult<ParsedParamsResult> {
    Ok(parsed_params_from_model(model))
}

pub(super) fn load_cached_bundle(bundle_dir: &Path) -> AppResult<Option<ArtifactBundle>> {
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    if !bundle_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&bundle_path)
        .map_err(|e| AppError::persistence(format!("Failed to read bundle: {}", e)))?;
    let bundle: ArtifactBundle = serde_json::from_str(&raw)
        .map_err(|e| AppError::parse(format!("Failed to parse bundle: {}", e)))?;
    if !Path::new(&bundle.manifest_path).exists() || !Path::new(&bundle.model_stl_path).exists() {
        return Ok(None);
    }
    Ok(Some(bundle))
}

fn cached_indexed_mesh_assets_are_valid(bundle: &ArtifactBundle) -> bool {
    bundle.viewer_assets.iter().all(|asset| {
        crate::ecky_ir::mesh_asset::IndexedMeshAsset::read_cache(
            &Path::new(&asset.path).with_extension("indexed-mesh.json"),
        )
        .is_ok()
    })
}

fn indexed_mesh_topology_evidence(
    topology: &crate::ecky_ir::mesh_asset::IndexedMeshTopology,
) -> (u64, bool) {
    (
        (topology.boundary_edge_count + topology.non_manifold_edge_count) as u64,
        topology.closed,
    )
}

pub fn render_model(
    source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let model = parse_model(source)?;
    let canonical_source = canonicalize(source)?;
    render_model_from_model(&model, &canonical_source, parameters, app)
}

#[derive(Clone)]
struct RuntimePart {
    part_id: String,
    label: String,
    expr: IrExpr,
    feature_decl: Option<CoreFeatureDecl>,
    source_ref: Option<SourceRef>,
    dependency_ids: Vec<String>,
    named_shapes: Vec<RuntimeNamedShape>,
}

#[derive(Clone)]
struct RuntimeNamedShape {
    name: String,
    source_ref: Option<SourceRef>,
    dependency_ids: Vec<String>,
}

#[derive(Clone)]
struct CoreAstIdentity {
    core_digest: String,
    ast_schema_version: u32,
}

fn runtime_part_feature_id(part_id: &str) -> String {
    format!("part:{}", part_id)
}

fn runtime_part_source_ref(part_id: &str, span: Option<SourceSpan>) -> Option<SourceRef> {
    if part_id.trim().is_empty() {
        return None;
    }

    Some(SourceRef {
        source_id: None,
        path: Some(format!("/parts/{}/root", part_id)),
        start_byte: span.map(|span| span.start),
        end_byte: span.map(|span| span.end),
    })
}

fn runtime_shape_source_ref(
    part_id: &str,
    shape_name: &str,
    span: Option<SourceSpan>,
) -> Option<SourceRef> {
    if part_id.trim().is_empty() || shape_name.trim().is_empty() {
        return None;
    }

    Some(SourceRef {
        source_id: None,
        path: Some(format!("/parts/{part_id}/build/{shape_name}")),
        start_byte: span.map(|span| span.start),
        end_byte: span.map(|span| span.end),
    })
}

fn runtime_part_feature_graph(
    parts: &[RuntimePart],
    selection_targets: &[SelectionTarget],
) -> FeatureGraph {
    let nodes = parts
        .iter()
        .flat_map(|part| {
            let fallback_feature_id = runtime_part_feature_id(&part.part_id);
            let feature_id = part
                .feature_decl
                .as_ref()
                .map(|decl| decl.feature_id.trim())
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| fallback_feature_id.clone());
            let kind = part
                .feature_decl
                .as_ref()
                .map(|decl| decl.role.trim())
                .filter(|role| !role.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "part".to_string());
            let target_ids = selection_targets
                .iter()
                .filter(|target| target.part_id == part.part_id)
                .filter_map(runtime_selection_target_output_id)
                .map(str::to_string)
                .collect::<Vec<_>>();
            let output_refs = if target_ids.is_empty() {
                Vec::new()
            } else {
                vec![FeatureOutputRef {
                    feature_id: feature_id.clone(),
                    output_id: "selectionTargets".to_string(),
                    target_ids,
                }]
            };

            let mut nodes = vec![FeatureNode {
                feature_id,
                kind,
                label: if part.label.trim().is_empty() {
                    part.part_id.clone()
                } else {
                    part.label.clone()
                },
                source_ref: part.source_ref.clone(),
                dependency_ids: runtime_feature_dependency_ids(part),
                output_refs,
                ports: Vec::new(),
            }];
            nodes.extend(part.named_shapes.iter().map(|shape| FeatureNode {
                feature_id: format!("shape:{}:{}", part.part_id, shape.name),
                kind: "shape".to_string(),
                label: humanize_runtime_name(&shape.name),
                source_ref: shape.source_ref.clone(),
                dependency_ids: shape.dependency_ids.clone(),
                output_refs: Vec::new(),
                ports: Vec::new(),
            }));
            nodes
        })
        .collect();

    FeatureGraph { nodes }
}

fn humanize_runtime_name(name: &str) -> String {
    let words = name
        .split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>();
    if words.is_empty() {
        name.to_string()
    } else {
        words.join(" ")
    }
}

fn runtime_selection_target_output_id(target: &SelectionTarget) -> Option<&str> {
    target
        .target_id
        .as_deref()
        .or(target.durable_target_id.as_deref())
        .or(target.canonical_target_id.as_deref())
}

fn runtime_feature_dependency_ids(part: &RuntimePart) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();

    if let Some(feature_decl) = &part.feature_decl {
        for key in &feature_decl.param_keys {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            let normalized = key.to_string();
            if seen.insert(normalized.clone()) {
                ids.push(normalized);
            }
        }
    }

    for key in &part.dependency_ids {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let normalized = key.to_string();
        if seen.insert(normalized.clone()) {
            ids.push(normalized);
        }
    }

    ids
}

fn runtime_parameter_groups(
    parts: &[RuntimePart],
    parameter_keys: &[String],
) -> Vec<ParameterGroup> {
    let part_keys = parts
        .iter()
        .map(|part| (part.part_id.as_str(), runtime_feature_dependency_ids(part)))
        .collect::<Vec<_>>();
    let mut claim_counts = BTreeMap::<String, usize>::new();
    for (_, keys) in &part_keys {
        for key in keys {
            *claim_counts.entry(key.clone()).or_default() += 1;
        }
    }

    let mut groups = Vec::new();
    let model_keys = parameter_keys
        .iter()
        .filter(|key| claim_counts.get(*key).copied().unwrap_or_default() != 1)
        .cloned()
        .collect::<Vec<_>>();
    if !model_keys.is_empty() {
        groups.push(ParameterGroup {
            group_id: "model:parameters".to_string(),
            label: "Model Parameters".to_string(),
            parameter_keys: model_keys,
            part_ids: parts.iter().map(|part| part.part_id.clone()).collect(),
            editable: true,
            presentation: Some("primary".to_string()),
            order: Some(groups.len() as u32),
        });
    }

    for part in parts {
        let inferred_keys = part_keys
            .iter()
            .find(|(part_id, _)| *part_id == part.part_id)
            .map(|(_, keys)| keys.as_slice())
            .unwrap_or_default();
        let (group_id, primary_keys) = match part.feature_decl.as_ref() {
            Some(feature) if !feature.param_keys.is_empty() => (
                feature.feature_id.clone(),
                feature
                    .param_keys
                    .iter()
                    .filter(|key| parameter_keys.contains(key))
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            _ => (
                format!("part:{}", part.part_id),
                inferred_keys
                    .iter()
                    .filter(|key| claim_counts.get(*key) == Some(&1))
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
        };
        if !primary_keys.is_empty() {
            groups.push(ParameterGroup {
                group_id,
                label: if part.label.trim().is_empty() {
                    humanize_runtime_name(&part.part_id)
                } else {
                    part.label.clone()
                },
                parameter_keys: primary_keys,
                part_ids: vec![part.part_id.clone()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(groups.len() as u32),
            });
        }
        for shape in &part.named_shapes {
            if shape.dependency_ids.is_empty() {
                continue;
            }
            groups.push(ParameterGroup {
                group_id: format!("shape:{}:{}", part.part_id, shape.name),
                label: humanize_runtime_name(&shape.name),
                parameter_keys: shape.dependency_ids.clone(),
                part_ids: vec![part.part_id.clone()],
                editable: true,
                presentation: Some("advanced".to_string()),
                order: Some(groups.len() as u32),
            });
        }
    }

    groups
}

pub(crate) fn build_core_program_param_env_for_eval(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AppResult<BTreeMap<String, ParamValue>> {
    let mut env = core_program_param_defaults(program)?;
    for (key, value) in parameters {
        env.insert(key.clone(), value.clone());
    }
    Ok(env)
}

pub(crate) fn eval_core_number_with_locals(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<f64> {
    let expr = core_node_to_eval_ir_expr(node, param_names, env)?;
    super::eval_scalar::eval_number(&expr, env)
}

pub(crate) fn eval_core_bool_with_locals(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<bool> {
    let expr = core_node_to_eval_ir_expr(node, param_names, env)?;
    super::eval_scalar::eval_bool(&expr, env)
}

pub(crate) fn eval_core_stringish_with_locals(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<String> {
    let expr = core_node_to_eval_ir_expr(node, param_names, env)?;
    super::eval_scalar::eval_stringish(&expr, env)
}

fn core_node_to_eval_ir_expr(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<IrExpr> {
    let mut used_local_names = BTreeMap::new();
    let locals = env
        .keys()
        .map(|key| (key.clone(), key.clone()))
        .collect::<BTreeMap<_, _>>();
    runtime_core_node_to_ir_expr(
        node,
        param_names,
        &BTreeMap::new(),
        &locals,
        &mut used_local_names,
    )
}

fn runtime_core_part_to_runtime_part(
    part: &CorePart,
    param_names: &BTreeMap<u64, String>,
    feature_decls: &BTreeMap<String, CoreFeatureDecl>,
) -> AppResult<RuntimePart> {
    let provenance = core_part_dependency_projection(part, param_names);
    let mut used_local_names = BTreeMap::new();
    Ok(RuntimePart {
        part_id: part.key.clone(),
        label: part.label.clone(),
        expr: materialize_selector_nodes(runtime_core_node_to_ir_expr(
            &part.root,
            param_names,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut used_local_names,
        )?)?,
        feature_decl: feature_decls.get(&part.key).cloned(),
        source_ref: runtime_part_source_ref(&part.key, part.root.span),
        dependency_ids: provenance.dependency_ids,
        named_shapes: provenance.named_shapes,
    })
}

struct CorePartDependencyProjection {
    dependency_ids: Vec<String>,
    named_shapes: Vec<RuntimeNamedShape>,
}

fn core_part_dependency_projection(
    part: &CorePart,
    param_names: &BTreeMap<u64, String>,
) -> CorePartDependencyProjection {
    let mut node_index = BTreeMap::new();
    let mut shape_bindings = Vec::new();
    index_core_nodes_and_shapes(
        &part.root,
        &BTreeMap::new(),
        &mut node_index,
        &mut shape_bindings,
    );

    let mut reachable_node_ids = BTreeSet::new();
    let dependency_ids = core_node_reachable_parameter_dependencies(
        &part.root,
        param_names,
        &node_index,
        &BTreeMap::new(),
        &mut reachable_node_ids,
    );
    let mut shape_name_counts = BTreeMap::<String, usize>::new();
    for (name, _, _) in &shape_bindings {
        *shape_name_counts.entry(name.clone()).or_default() += 1;
    }
    let named_shapes = shape_bindings
        .into_iter()
        .filter(|(name, value, _)| {
            shape_name_counts.get(name) == Some(&1) && reachable_node_ids.contains(&value.id.raw())
        })
        .map(|(name, value, locals)| {
            let mut shape_reachable = BTreeSet::new();
            RuntimeNamedShape {
                source_ref: runtime_shape_source_ref(&part.key, &name, value.span),
                dependency_ids: core_node_reachable_parameter_dependencies(
                    value,
                    param_names,
                    &node_index,
                    &locals,
                    &mut shape_reachable,
                ),
                name,
            }
        })
        .collect();

    CorePartDependencyProjection {
        dependency_ids,
        named_shapes,
    }
}

fn index_core_nodes_and_shapes<'a>(
    node: &'a CoreNode,
    locals: &BTreeMap<String, u64>,
    node_index: &mut BTreeMap<u64, &'a CoreNode>,
    shape_bindings: &mut Vec<(String, &'a CoreNode, BTreeMap<String, u64>)>,
) {
    node_index.insert(node.id.raw(), node);
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => {}
        CoreNodeKind::Build { bindings, result } => {
            let mut nested = locals.clone();
            for binding in bindings {
                shape_bindings.push((binding.name.clone(), &binding.value, nested.clone()));
                index_core_nodes_and_shapes(&binding.value, &nested, node_index, shape_bindings);
                nested.insert(binding.name.clone(), binding.value.id.raw());
            }
            index_core_nodes_and_shapes(result, &nested, node_index, shape_bindings);
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested = locals.clone();
            for binding in bindings {
                index_core_nodes_and_shapes(&binding.value, &nested, node_index, shape_bindings);
                nested.insert(binding.name.clone(), binding.value.id.raw());
            }
            index_core_nodes_and_shapes(body, &nested, node_index, shape_bindings);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            index_core_nodes_and_shapes(condition, locals, node_index, shape_bindings);
            index_core_nodes_and_shapes(then_branch, locals, node_index, shape_bindings);
            index_core_nodes_and_shapes(else_branch, locals, node_index, shape_bindings);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                index_core_nodes_and_shapes(arg, locals, node_index, shape_bindings);
            }
            for keyword in keywords {
                index_core_nodes_and_shapes(
                    keyword.source_node(),
                    locals,
                    node_index,
                    shape_bindings,
                );
            }
        }
        CoreNodeKind::Range { start, end } => {
            index_core_nodes_and_shapes(start, locals, node_index, shape_bindings);
            index_core_nodes_and_shapes(end, locals, node_index, shape_bindings);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                index_core_nodes_and_shapes(source, locals, node_index, shape_bindings);
            }
            index_core_nodes_and_shapes(body, locals, node_index, shape_bindings);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                index_core_nodes_and_shapes(arg, locals, node_index, shape_bindings);
            }
            index_core_nodes_and_shapes(list, locals, node_index, shape_bindings);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                index_core_nodes_and_shapes(item, locals, node_index, shape_bindings);
            }
        }
    }
}

fn core_node_reachable_parameter_dependencies(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    node_index: &BTreeMap<u64, &CoreNode>,
    locals: &BTreeMap<String, u64>,
    reachable_node_ids: &mut BTreeSet<u64>,
) -> Vec<String> {
    let mut keys = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    collect_reachable_core_node_dependencies(
        node,
        param_names,
        node_index,
        locals,
        reachable_node_ids,
        &mut visiting,
        &mut keys,
    );
    keys.into_iter().collect()
}

fn collect_reachable_core_node_dependencies(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    node_index: &BTreeMap<u64, &CoreNode>,
    locals: &BTreeMap<String, u64>,
    reachable_node_ids: &mut BTreeSet<u64>,
    visiting: &mut BTreeSet<u64>,
    keys: &mut BTreeSet<String>,
) {
    let node_id = node.id.raw();
    reachable_node_ids.insert(node_id);
    if !visiting.insert(node_id) {
        return;
    }

    let mut visit = |child: &CoreNode, child_locals: &BTreeMap<String, u64>| {
        collect_reachable_core_node_dependencies(
            child,
            param_names,
            node_index,
            child_locals,
            reachable_node_ids,
            visiting,
            keys,
        );
    };
    match &node.kind {
        CoreNodeKind::Literal(_) => {}
        CoreNodeKind::Reference(CoreReference::Parameter(param_id)) => {
            if let Some(key) = param_names.get(&param_id.raw()) {
                keys.insert(key.clone());
            }
        }
        CoreNodeKind::Reference(CoreReference::Node(id)) => {
            if let Some(target) = node_index.get(&id.raw()) {
                visit(target, locals);
            }
        }
        CoreNodeKind::Reference(CoreReference::Local(name)) => {
            if let Some(id) = locals.get(name).and_then(|id| node_index.get(id)) {
                visit(id, locals);
            }
        }
        CoreNodeKind::Reference(CoreReference::Part(_)) => {}
        CoreNodeKind::Build { bindings, result } => {
            let mut nested = locals.clone();
            for binding in bindings {
                nested.insert(binding.name.clone(), binding.value.id.raw());
            }
            visit(result, &nested);
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested = locals.clone();
            for binding in bindings {
                nested.insert(binding.name.clone(), binding.value.id.raw());
            }
            visit(body, &nested);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit(condition, locals);
            visit(then_branch, locals);
            visit(else_branch, locals);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                visit(arg, locals);
            }
            for keyword in keywords {
                visit(keyword.source_node(), locals);
            }
        }
        CoreNodeKind::Range { start, end } => {
            visit(start, locals);
            visit(end, locals);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                visit(source, locals);
            }
            visit(body, locals);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                visit(arg, locals);
            }
            visit(list, locals);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                visit(item, locals);
            }
        }
    }
    visiting.remove(&node_id);
}

fn ir_expr_parameter_dependencies(expr: &IrExpr, parameter_keys: &[String]) -> Vec<String> {
    let parameter_key_set = parameter_keys.iter().cloned().collect::<BTreeSet<_>>();
    let mut used = BTreeSet::new();
    collect_ir_expr_parameter_dependencies(expr, &parameter_key_set, &mut used);
    used.into_iter().collect()
}

fn collect_ir_expr_parameter_dependencies(
    expr: &IrExpr,
    parameter_keys: &BTreeSet<String>,
    used: &mut BTreeSet<String>,
) {
    match expr {
        IrExpr::Symbol(symbol) => {
            if parameter_keys.contains(symbol) {
                used.insert(symbol.clone());
            }
        }
        IrExpr::List(items) => {
            for (index, item) in items.iter().enumerate() {
                if index == 0 && matches!(item, IrExpr::Symbol(_)) {
                    continue;
                }
                collect_ir_expr_parameter_dependencies(item, parameter_keys, used);
            }
        }
        IrExpr::Number(_)
        | IrExpr::Boolean(_)
        | IrExpr::String(_)
        | IrExpr::Keyword(_)
        | IrExpr::Selector(_) => {}
    }
}

fn runtime_ir_expr_from_core_selector_payload(payload: &CoreSelectorPayload) -> AppResult<IrExpr> {
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

fn runtime_core_node_to_ir_expr(
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
            Ok(IrExpr::symbol(runtime_core_symbol_name(symbol)))
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
            "Unsupported Core IR reference in runtime bridge: {:?}.",
            other
        ))),
        CoreNodeKind::Build { bindings, result } => {
            let mut items = vec![IrExpr::symbol("build")];
            let mut nested = refs.clone();
            let mut nested_locals = locals.clone();
            for binding in bindings {
                let ir_name = runtime_allocate_local_name(&binding.name, used_local_names);
                let mut shape_items = vec![
                    IrExpr::symbol("shape"),
                    IrExpr::symbol(ir_name.clone()),
                    runtime_core_node_to_ir_expr(
                        &binding.value,
                        param_names,
                        &nested,
                        &nested_locals,
                        used_local_names,
                    )?,
                ];
                if binding.value.value_kind != CoreValueKind::Any {
                    shape_items.push(IrExpr::keyword("value-kind"));
                    shape_items.push(IrExpr::symbol(runtime_core_value_kind_tag(
                        binding.value.value_kind,
                    )));
                }
                items.push(IrExpr::list(shape_items));
                nested.insert(binding.value.id.raw(), ir_name.clone());
                nested_locals.insert(binding.name.clone(), ir_name);
            }
            items.push(IrExpr::list(vec![
                IrExpr::symbol("result"),
                runtime_core_node_to_ir_expr(
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
            let mut nested_refs = refs.clone();
            let mut nested_locals = locals.clone();
            let ir_binding_names = bindings
                .iter()
                .map(|binding| {
                    (
                        binding.name.clone(),
                        runtime_allocate_local_name(&binding.name, used_local_names),
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
                        runtime_core_node_to_ir_expr(
                            &binding.value,
                            param_names,
                            refs,
                            locals,
                            used_local_names,
                        )?,
                    ];
                    if binding.value.value_kind != CoreValueKind::Any {
                        pair.push(IrExpr::keyword("value-kind"));
                        pair.push(IrExpr::symbol(runtime_core_value_kind_tag(
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
                runtime_core_node_to_ir_expr(
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
            runtime_core_node_to_ir_expr(condition, param_names, refs, locals, used_local_names)?,
            runtime_core_node_to_ir_expr(then_branch, param_names, refs, locals, used_local_names)?,
            runtime_core_node_to_ir_expr(else_branch, param_names, refs, locals, used_local_names)?,
        ])),
        CoreNodeKind::Call { op, args, keywords } => {
            let mut items = vec![IrExpr::symbol(runtime_core_operation_name(op))];
            for arg in args {
                items.push(runtime_core_node_to_ir_expr(
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
                            crate::ecky_core_ir::CoreSelectorPayload::FaceClauses(_)
                            | crate::ecky_core_ir::CoreSelectorPayload::FaceTargetIds(_),
                        ),
                    ) => {
                        return Err(validation(
                            "CoreProgram `:edges` keyword requires edge selector payload.",
                        ))
                    }
                    (
                        "faces",
                        Some(
                            crate::ecky_core_ir::CoreSelectorPayload::EdgeAll
                            | crate::ecky_core_ir::CoreSelectorPayload::EdgeClauses(_)
                            | crate::ecky_core_ir::CoreSelectorPayload::EdgeTargetIds(_),
                        ),
                    ) => {
                        return Err(validation(
                            "CoreProgram `:faces` keyword requires face selector payload.",
                        ))
                    }
                    (_, Some(selector)) => runtime_ir_expr_from_core_selector_payload(selector)?,
                    (_, None) => runtime_core_node_to_ir_expr(
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
            runtime_core_node_to_ir_expr(start, param_names, refs, locals, used_local_names)?,
            runtime_core_node_to_ir_expr(end, param_names, refs, locals, used_local_names)?,
        ])),
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => {
            let mut nested_locals = locals.clone();
            let mut ir_params = Vec::new();
            for param in params {
                let ir_name = runtime_allocate_local_name(param, used_local_names);
                nested_locals.insert(param.clone(), ir_name.clone());
                ir_params.push(IrExpr::symbol(ir_name));
            }
            let mut items = vec![
                IrExpr::symbol("map"),
                IrExpr::list(vec![
                    IrExpr::symbol("lambda"),
                    IrExpr::list(ir_params),
                    runtime_core_node_to_ir_expr(
                        body,
                        param_names,
                        refs,
                        &nested_locals,
                        used_local_names,
                    )?,
                ]),
            ];
            for source in sources {
                items.push(runtime_core_node_to_ir_expr(
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
                IrExpr::symbol(runtime_core_operation_name(op)),
            ];
            for arg in args {
                items.push(runtime_core_node_to_ir_expr(
                    arg,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
            }
            items.push(runtime_core_node_to_ir_expr(
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
                .map(|item| {
                    runtime_core_node_to_ir_expr(item, param_names, refs, locals, used_local_names)
                })
                .collect::<AppResult<Vec<_>>>()?,
        )),
        CoreNodeKind::Group(items) => Ok(IrExpr::list(
            items
                .iter()
                .map(|item| {
                    runtime_core_node_to_ir_expr(item, param_names, refs, locals, used_local_names)
                })
                .collect::<AppResult<Vec<_>>>()?,
        )),
    }
}

fn runtime_allocate_local_name(name: &str, used: &mut BTreeMap<String, usize>) -> String {
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

fn runtime_core_symbol_name(symbol: &CoreSymbol) -> &'static str {
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

fn runtime_core_value_kind_tag(kind: CoreValueKind) -> &'static str {
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
        CoreValueKind::Mesh => "mesh",
        CoreValueKind::Compound => "compound",
        CoreValueKind::Solid => "solid",
    }
}

fn runtime_core_operation_name(op: &CoreOperation) -> String {
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
        CoreOperation::Frame(CoreFrameOp::ClipPlane) => "clip-plane".to_string(),
        CoreOperation::Meta(CoreMetaOp::Group) => "compound".to_string(),
        CoreOperation::Meta(CoreMetaOp::Comment) => "meta".to_string(),
        CoreOperation::Meta(CoreMetaOp::Annotate) => "build".to_string(),
        CoreOperation::Custom(name) => name.clone(),
    }
}

fn core_ast_identity(program: &CoreProgram) -> CoreAstIdentity {
    let mut canonical = program.clone();
    clear_core_program_spans(&mut canonical);

    let mut hasher = Sha256::new();
    hasher.update(b"ecky-core-ast");
    hasher.update(CORE_AST_SCHEMA_VERSION.to_string().as_bytes());
    hasher.update(format!("{canonical:#?}").as_bytes());

    CoreAstIdentity {
        core_digest: format!("sha256:{:x}", hasher.finalize()),
        ast_schema_version: CORE_AST_SCHEMA_VERSION,
    }
}

fn clear_core_program_spans(program: &mut CoreProgram) {
    for part in &mut program.parts {
        clear_core_node_spans(&mut part.root);
    }
}

fn clear_core_node_spans(node: &mut CoreNode) {
    node.span = None;
    match &mut node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => {}
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings {
                clear_core_node_spans(&mut binding.value);
            }
            clear_core_node_spans(result);
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                clear_core_node_spans(&mut binding.value);
            }
            clear_core_node_spans(body);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            clear_core_node_spans(condition);
            clear_core_node_spans(then_branch);
            clear_core_node_spans(else_branch);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                clear_core_node_spans(arg);
            }
            for keyword in keywords {
                clear_core_node_spans(keyword.source_node_mut());
            }
        }
        CoreNodeKind::Range { start, end } => {
            clear_core_node_spans(start);
            clear_core_node_spans(end);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                clear_core_node_spans(source);
            }
            clear_core_node_spans(body);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                clear_core_node_spans(arg);
            }
            clear_core_node_spans(list);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                clear_core_node_spans(item);
            }
        }
    }
}

fn cached_bundle_satisfies_manifest_identity(
    bundle: &ArtifactBundle,
    source_digest: &str,
    ast_identity: Option<&CoreAstIdentity>,
    parts: &[RuntimePart],
    expected_parameter_groups: &[ParameterGroup],
) -> bool {
    let Ok(raw) = fs::read_to_string(&bundle.manifest_path) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<ModelManifest>(&raw) else {
        return false;
    };
    if manifest.source_digest.as_deref() != Some(source_digest) {
        return false;
    }
    if manifest.feature_graph.is_none() {
        return false;
    }
    if parts.iter().any(|expected| {
        let expected_keys = runtime_feature_dependency_ids(expected);
        manifest
            .parts
            .iter()
            .find(|part| part.part_id == expected.part_id)
            .map(|part| &part.parameter_keys)
            != Some(&expected_keys)
    }) {
        return false;
    }
    if manifest.parameter_groups.len() != expected_parameter_groups.len()
        || manifest
            .parameter_groups
            .iter()
            .zip(expected_parameter_groups)
            .any(|(actual, expected)| {
                actual.group_id != expected.group_id
                    || actual.parameter_keys != expected.parameter_keys
                    || actual.part_ids != expected.part_ids
            })
    {
        return false;
    }

    match ast_identity {
        Some(identity) => {
            manifest.core_digest.as_deref() == Some(identity.core_digest.as_str())
                && manifest.ast_schema_version == Some(identity.ast_schema_version)
        }
        None => manifest.core_digest.is_none() && manifest.ast_schema_version.is_none(),
    }
}

fn render_prepared_parts(
    parts: &[RuntimePart],
    parameter_keys: &[String],
    source_identity: &str,
    parameters: &DesignParams,
    env: &BTreeMap<String, ParamValue>,
    app: &dyn PathResolver,
    ast_identity: Option<CoreAstIdentity>,
    analysis_declarations: Vec<AnalysisDeclarationBinding>,
    preview_views: Vec<PreviewView>,
) -> AppResult<ArtifactBundle> {
    let exposes_mesh_literal = parts
        .iter()
        .any(|part| ir_expr_contains_mesh_literal(&part.expr));
    let part_parameter_keys = parts
        .iter()
        .map(|part| (part.part_id.clone(), runtime_feature_dependency_ids(part)))
        .collect::<BTreeMap<_, _>>();
    let parameter_groups = runtime_parameter_groups(parts, parameter_keys);
    let params_json = serde_json::to_string(parameters).unwrap_or_default();
    let raster_geometry_digest = raster_geometry_asset_digest(parts, env)?;
    let mut hasher = Sha256::new();
    hasher.update(source_identity.as_bytes());
    hasher.update(b"|");
    hasher.update(params_json.as_bytes());
    if let Some(digest) = raster_geometry_digest.as_deref() {
        hasher.update(b"|raster-geometry-assets-v2|");
        hasher.update(digest.as_bytes());
    }
    let hash = format!("{:x}", hasher.finalize());
    let mut source_hasher = Sha256::new();
    source_hasher.update(source_identity.as_bytes());
    if let Some(digest) = raster_geometry_digest.as_deref() {
        source_hasher.update(b"|raster-geometry-assets-v2|");
        source_hasher.update(digest.as_bytes());
    }
    let source_digest = format!("sha256:{:x}", source_hasher.finalize());
    let model_id = format!("generated-ir-{}", &hash[..12]);
    let dir = bundle_dir(app, &model_id)?;

    if let Some(cached) = load_cached_bundle(&dir)? {
        if cached_bundle_satisfies_manifest_identity(
            &cached,
            &source_digest,
            ast_identity.as_ref(),
            parts,
            &parameter_groups,
        ) && cached_indexed_mesh_assets_are_valid(&cached)
            && (!exposes_mesh_literal
                || cached
                    .export_artifacts
                    .iter()
                    .any(|artifact| artifact.format == "stl"))
        {
            return Ok(cached);
        }
    }

    let core_digest = ast_identity
        .as_ref()
        .map(|identity| identity.core_digest.clone());
    let ast_schema_version = ast_identity
        .as_ref()
        .map(|identity| identity.ast_schema_version);

    let parts_dir = dir.join(PARTS_DIR_NAME);
    fs::create_dir_all(&parts_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let mut part_bindings = Vec::new();
    let mut viewer_assets = Vec::new();
    let mut mesh_warnings = Vec::new();
    let mut source_mesh_digests = Vec::new();
    let mut boundary_or_non_manifold_edge_count = 0_u64;
    let mut mesh_literal_topology_closed = true;
    let mut preview_mesh: Option<IrMesh> = None;

    for (index, part) in parts.iter().enumerate() {
        let mesh =
            sanitize_mesh_for_export(&eval_geometry_expr(&part.expr, env)?.into_mesh("part")?);
        let part_path = parts_dir.join(format!("{}-{}.stl", index + 1, part.part_id));
        let indexed_asset = crate::ecky_ir::mesh_asset::IndexedMeshAsset::from_ir_mesh(
            crate::ecky_ir::mesh_asset::MeshAssetSource::Generated {
                provider: "ecky-rust".to_string(),
                model: None,
            },
            &mesh,
        )?;
        indexed_asset.write_cache(&part_path.with_extension("indexed-mesh.json"))?;
        let stl_bytes = mesh
            .to_stl_binary(&part.part_id)
            .map_err(|err| AppError::persistence(format!("Failed to encode STL: {}", err)))?;
        fs::write(&part_path, &stl_bytes).map_err(|err| AppError::persistence(err.to_string()))?;

        if ir_expr_contains_mesh_literal(&part.expr) {
            let digest = indexed_asset.content_digest().to_string();
            let topology = indexed_asset.topology();
            let (boundary_or_non_manifold, topology_closed) =
                indexed_mesh_topology_evidence(topology);
            source_mesh_digests.push(digest.clone());
            boundary_or_non_manifold_edge_count += boundary_or_non_manifold;
            mesh_literal_topology_closed &= topology_closed;
            mesh_warnings.push(format!(
                "Mesh evidence: part={} digest={} triangles={} boundaryOrNonManifoldEdges={} windingMismatches={} topology={}",
                part.part_id,
                digest,
                mesh.polygons.len(),
                boundary_or_non_manifold,
                topology.winding_mismatch_count,
                if topology_closed {
                    "closed"
                } else {
                    "open-or-non-manifold"
                }
            ));
        }

        preview_mesh = Some(match preview_mesh.take() {
            Some(existing) => existing.union(&mesh),
            None => mesh.clone(),
        });

        let asset_path = part_path.to_string_lossy().to_string();
        viewer_assets.push(ViewerAsset {
            part_id: part.part_id.clone(),
            node_id: part.part_id.clone(),
            object_name: part.part_id.clone(),
            label: part.label.clone(),
            path: asset_path.clone(),
            format: ViewerAssetFormat::Stl,
        });
        part_bindings.push(PartBinding {
            part_id: part.part_id.clone(),
            freecad_object_name: part.part_id.clone(),
            label: part.label.clone(),
            kind: if ir_expr_contains_open_mesh_literal(&part.expr) {
                "mesh".to_string()
            } else {
                "solid".to_string()
            },
            semantic_role: Some("generated".to_string()),
            viewer_asset_path: Some(asset_path),
            viewer_node_ids: vec![part.part_id.clone()],
            parameter_keys: part_parameter_keys
                .get(&part.part_id)
                .cloned()
                .unwrap_or_default(),
            editable: true,
            bounds: Some(bounds_from_mesh(&mesh)),
            volume: mesh_volume(&mesh),
            area: mesh_area(&mesh),
        });
    }

    let preview_mesh =
        preview_mesh.ok_or_else(|| validation("`.ecky` model produced no printable parts."))?;
    let preview_mesh = sanitize_mesh_for_export(&preview_mesh);
    let preview_path = dir.join(MODEL_STL_FILE_NAME);
    fs::write(
        &preview_path,
        preview_mesh
            .to_stl_binary("preview")
            .map_err(|err| AppError::persistence(format!("Failed to encode model STL: {}", err)))?,
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;

    let macro_path = dir.join(SOURCE_FILE_NAME);
    fs::write(&macro_path, source_identity.as_bytes())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let selection_targets = Vec::new();
    let feature_graph = runtime_part_feature_graph(parts, &selection_targets);
    let geometry_provenance = exposes_mesh_literal.then_some(GeometryProvenance {
        representation: GeometryRepresentation::MeshNative,
        source_mesh_digests,
        closed: Some(mesh_literal_topology_closed),
        boundary_or_non_manifold_edge_count: Some(boundary_or_non_manifold_edge_count),
    });
    let manifest = ModelManifest {
        geometry_provenance: geometry_provenance.clone(),
        component_import_origins: Vec::new(),
        component_placement_evidence: Vec::new(),
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.clone(),
        source_kind: ModelSourceKind::Generated,
        source_digest: Some(source_digest),
        core_digest,
        ast_schema_version,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        document: DocumentMetadata {
            document_name: "Ecky".to_string(),
            document_label: "Ecky".to_string(),
            source_path: Some(macro_path.to_string_lossy().to_string()),
            object_count: part_bindings.len(),
            warnings: mesh_warnings.clone(),
        },
        parts: part_bindings,
        parameter_groups,
        control_primitives: Vec::new(),
        control_relations: Vec::new(),
        control_views: Vec::new(),
        preview_views,
        advisories: Vec::new(),
        selection_targets,
        measurement_annotations: Vec::new(),
        tagged_anchors: std::collections::BTreeMap::new(),
        feature_graph: Some(feature_graph),
        correspondence_graph: None,
        analysis_declarations,
        warnings: mesh_warnings,
        enrichment_state: crate::contracts::ManifestEnrichmentState {
            status: crate::contracts::EnrichmentStatus::None,
            proposals: Vec::new(),
        },
    };

    let manifest_path = dir.join(MANIFEST_FILE_NAME);
    write_manifest(&manifest_path, &manifest)?;

    let export_artifacts = if exposes_mesh_literal {
        vec![ExportArtifact {
            geometry_provenance: geometry_provenance.clone(),
            label: "STL".to_string(),
            format: "stl".to_string(),
            path: preview_path.to_string_lossy().to_string(),
            role: "primary".to_string(),
        }]
    } else {
        Vec::new()
    };
    let bundle = ArtifactBundle {
        geometry_provenance,
        component_dependency_lock: None,
        component_dependency_lock_digest: None,
        component_import_origins: Vec::new(),
        component_placement_evidence: Vec::new(),
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id,
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash: hash,
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        macro_path: Some(macro_path.to_string_lossy().to_string()),
        model_stl_path: preview_path.to_string_lossy().to_string(),
        viewer_assets,
        edge_targets: Vec::new(),
        face_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts,
    };
    write_bundle(&dir.join(BUNDLE_FILE_NAME), &bundle)?;
    Ok(bundle)
}

fn ir_expr_contains_mesh_literal(expr: &IrExpr) -> bool {
    match expr {
        IrExpr::List(items) => {
            items
                .first()
                .and_then(IrExpr::as_symbol)
                .is_some_and(|name| {
                    matches!(
                        name,
                        "mesh" | "polyhedron" | "heightfield" | "protrude" | "import-stl"
                    ) || (name == "extrude"
                        && items.get(1).is_some_and(|source| source.as_str().is_some()))
                })
                || items.iter().any(ir_expr_contains_mesh_literal)
        }
        IrExpr::Number(_)
        | IrExpr::Boolean(_)
        | IrExpr::String(_)
        | IrExpr::Symbol(_)
        | IrExpr::Keyword(_)
        | IrExpr::Selector(_) => false,
    }
}

fn ir_expr_contains_open_mesh_literal(expr: &IrExpr) -> bool {
    match expr {
        IrExpr::List(items) => {
            items.first().and_then(IrExpr::as_symbol) == Some("mesh")
                || items.iter().any(ir_expr_contains_open_mesh_literal)
        }
        IrExpr::Number(_)
        | IrExpr::Boolean(_)
        | IrExpr::String(_)
        | IrExpr::Symbol(_)
        | IrExpr::Keyword(_)
        | IrExpr::Selector(_) => false,
    }
}

fn raster_geometry_asset_digest(
    parts: &[RuntimePart],
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<Option<String>> {
    let mut paths = Vec::new();
    for part in parts {
        collect_raster_geometry_asset_paths(&part.expr, env, &mut paths)?;
    }
    if paths.is_empty() {
        return Ok(None);
    }
    let mut hasher = Sha256::new();
    for path in paths {
        if path.trim().is_empty() {
            return Err(AppError::with_details(
                crate::contracts::AppErrorCode::Validation,
                "Invalid image geometry.",
                "image path is empty; image selection remains pending",
            )
            .with_operation("raster-geometry"));
        }
        let bytes = fs::read(&path).map_err(|error| {
            AppError::with_details(
                crate::contracts::AppErrorCode::Validation,
                "Invalid image geometry.",
                format!("failed to read '{path}': {error}"),
            )
            .with_operation("raster-geometry")
        })?;
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
    }
    Ok(Some(format!("sha256:{:x}", hasher.finalize())))
}

fn collect_raster_geometry_asset_paths(
    expr: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
    paths: &mut Vec<String>,
) -> AppResult<()> {
    let IrExpr::List(items) = expr else {
        return Ok(());
    };
    let head = items.first().and_then(IrExpr::as_symbol);
    let raster_extrude = head == Some("extrude")
        && items.get(1).is_some_and(|image| {
            image.as_str().is_some()
                || image
                    .as_symbol()
                    .and_then(|symbol| env.get(symbol))
                    .is_some_and(|value| matches!(value, ParamValue::String(_)))
        });
    if matches!(head, Some("heightfield" | "protrude")) || raster_extrude {
        let image = items
            .get(1)
            .ok_or_else(|| validation("raster geometry expects an image path."))?;
        paths.push(eval_stringish(image, env)?);
    }
    for item in items {
        collect_raster_geometry_asset_paths(item, env, paths)?;
    }
    Ok(())
}

pub(crate) fn render_model_from_model(
    model: &IrModel,
    source_identity: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let env = build_param_env(model, parameters);
    let parameter_keys = model
        .params
        .iter()
        .map(|param| param.field.key().to_string())
        .collect::<Vec<_>>();
    let parts = model
        .parts
        .iter()
        .map(|part| RuntimePart {
            part_id: part.part_id.clone(),
            label: part.label.clone(),
            expr: part.expr.clone(),
            feature_decl: None,
            source_ref: runtime_part_source_ref(&part.part_id, None),
            dependency_ids: ir_expr_parameter_dependencies(&part.expr, &parameter_keys),
            named_shapes: Vec::new(),
        })
        .collect::<Vec<_>>();
    render_prepared_parts(
        &parts,
        &parameter_keys,
        source_identity,
        parameters,
        &env,
        app,
        None,
        Vec::new(),
        Vec::new(),
    )
}

#[allow(dead_code)]
pub(crate) fn render_core_program(
    program: &CoreProgram,
    source_identity: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let parts = program
        .parts
        .iter()
        .map(|part| runtime_core_part_to_runtime_part(part, &param_names, &program.feature_decls))
        .collect::<AppResult<Vec<_>>>()?;
    let parameter_keys = program
        .parameters
        .iter()
        .map(|param| param.key.clone())
        .collect::<Vec<_>>();
    let env = build_core_program_param_env_for_eval(program, parameters)?;
    render_prepared_parts(
        &parts,
        &parameter_keys,
        source_identity,
        parameters,
        &env,
        app,
        Some(core_ast_identity(program)),
        program
            .analyses
            .iter()
            .map(|analysis| AnalysisDeclarationBinding {
                analysis_id: analysis.name.clone(),
                kind: "linearStatic".into(),
                part_id: analysis.part.clone(),
                element_kind: analysis.element.clone(),
                source_start: analysis.span.map(|span| span.start),
                source_end: analysis.span.map(|span| span.end),
            })
            .collect(),
        program
            .preview_views
            .iter()
            .map(|view| PreviewView {
                view_id: view.name.clone(),
                label: view.name.clone(),
                offsets: view
                    .part_offsets
                    .iter()
                    .map(|offset| PreviewViewOffset {
                        part_id: offset.part_key.clone(),
                        dx: offset.dx,
                        dy: offset.dy,
                        dz: offset.dz,
                    })
                    .collect(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::contracts::ModelManifest;
    use crate::ecky_core_ir::{
        CoreNode, CoreNodeKind, CoreOperation, CoreParameter, CoreParameterConstraints,
        CoreParameterKind, CoreParameterValue, CorePart, CorePrimitive, CoreProgram, CoreValueKind,
        NodeId, ParamId, PartId, ProgramId, SourceFileId, SourceSpan,
    };
    use crate::ecky_ir::model::{core_part_to_ir_part, core_program_to_model, parse_model};

    fn render_root() -> PathBuf {
        std::env::temp_dir().join(format!("ecky-ir-runtime-test-{}", uuid::Uuid::new_v4()))
    }

    fn example_fixture(name: &str) -> String {
        let path = format!(
            "{}/../model-runtime/examples/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
    }

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl crate::models::PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn read_manifest(bundle: &ArtifactBundle) -> ModelManifest {
        serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest file"),
        )
        .expect("parse manifest")
    }

    fn contains_edge_selector(expr: &IrExpr) -> bool {
        match expr {
            IrExpr::Selector(crate::ecky_ir::model::IrSelectorExpr::Edge(_)) => true,
            IrExpr::List(items) => items.iter().any(contains_edge_selector),
            _ => false,
        }
    }

    fn contains_face_selector(expr: &IrExpr) -> bool {
        match expr {
            IrExpr::Selector(crate::ecky_ir::model::IrSelectorExpr::Face(_)) => true,
            IrExpr::List(items) => items.iter().any(contains_face_selector),
            _ => false,
        }
    }

    #[test]
    fn render_model_from_model_renders_typed_build_expr() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = r#"(model
            (part body
              (build
                (shape base (box 20 20 20))
                (shape cut (translate 0 0 10 (cylinder 4 12)))
                (result (difference base cut)))))"#;
        let model = parse_model(source).expect("model");

        let bundle = render_model_from_model(&model, source, &DesignParams::new(), &resolver)
            .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.model_stl_path).exists());
    }

    #[test]
    fn render_model_from_model_film_gap_coupon_fixture_has_stable_parts_and_export_readiness() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("film-adapter-film-gap-coupon.ecky");
        let model = parse_model(&source).expect("model");

        let bundle = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);
        let part_ids = manifest
            .parts
            .iter()
            .map(|part| part.part_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(bundle.viewer_assets.len(), 2);
        assert_eq!(manifest.document.object_count, 2);
        assert_eq!(manifest.parts.len(), 2);
        assert_eq!(part_ids, vec!["film_gate", "lens_adapter"]);
        assert!(bundle.export_artifacts.is_empty());
    }

    #[test]
    fn render_model_from_model_film_adapter_golden_closest_fixture_keeps_deterministic_count_and_step_readiness_signal(
    ) {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("film-adapter-film-gap-coupon.ecky");
        let model = parse_model(&source).expect("model");
        let model_part_ids = model
            .parts
            .iter()
            .map(|part| part.part_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            model_part_ids,
            vec!["film_gate", "lens_adapter"],
            "film adapter golden closest fixture has deterministic runtime part ids/count=2 (not trench-doc 6 for integrated helicoid path)"
        );

        let bundle = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);
        let manifest_part_ids = manifest
            .parts
            .iter()
            .map(|part| part.part_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            manifest_part_ids,
            vec!["film_gate", "lens_adapter"],
            "manifest part ids must stay deterministic for film adapter golden closest runtime render"
        );
        assert_eq!(
            manifest.parts.len(),
            2,
            "manifest deterministic part count for film adapter golden closest fixture is 2 on this backend path"
        );
        assert_eq!(
            manifest.document.object_count, 2,
            "document object count stays aligned with deterministic manifest part count"
        );
        assert!(bundle.export_artifacts.is_empty());
        assert!(
            matches!(
                manifest.enrichment_state.status,
                crate::contracts::EnrichmentStatus::None
            ),
            "manifest enrichment state stays none on EckyRust backend path (STEP export not materialized)"
        );
    }

    #[test]
    fn render_model_from_model_film_path_gap_coupon_fixture_has_stable_parts_and_count() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("film-path-gap-coupon.ecky");
        let model = parse_model(&source).expect("model");

        let model_part_ids = model
            .parts
            .iter()
            .map(|part| part.part_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            model_part_ids,
            vec![
                "film_path_lower_035",
                "film_path_upper_clamp_035",
                "film_path_lower_045",
                "film_path_upper_clamp_045",
                "film_path_lower_055",
                "film_path_upper_clamp_055"
            ]
        );

        let bundle = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);
        let manifest_part_ids = manifest
            .parts
            .iter()
            .map(|part| part.part_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(bundle.viewer_assets.len(), 6);
        assert_eq!(manifest.document.object_count, 6);
        assert_eq!(manifest.parts.len(), 6);
        assert_eq!(
            manifest_part_ids,
            vec![
                "film_path_lower_035",
                "film_path_upper_clamp_035",
                "film_path_lower_045",
                "film_path_upper_clamp_045",
                "film_path_lower_055",
                "film_path_upper_clamp_055"
            ]
        );
        assert!(bundle.export_artifacts.is_empty());
    }

    #[test]
    fn raw_ecky_runtime_rejects_helicoid_thread_coupon_without_direct_occt_bridge() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("helicoid-thread-coupon.ecky");
        let model = parse_model(&source).expect("model");
        let part_ids = model
            .parts
            .iter()
            .map(|part| part.part_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            part_ids,
            vec![
                "coupon_male_020",
                "coupon_female_020",
                "coupon_male_025",
                "coupon_female_025",
                "coupon_male_030",
                "coupon_female_030",
                "coupon_male_035",
                "coupon_female_035"
            ]
        );
        let err = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect_err("raw ecky runtime should reject helical-ridge without direct OCCT bridge");
        assert!(
            err.message
                .contains("Unsupported on current geometry backend"),
            "{err:?}"
        );
        assert!(
            err.details
                .as_deref()
                .unwrap_or_default()
                .contains("helical-ridge"),
            "{err:?}"
        );
    }

    #[test]
    fn render_model_from_model_magnet_clamp_coupon_fixture_has_stable_parts_and_count() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("magnet-clamp-coupon.ecky");
        let model = parse_model(&source).expect("model");

        let model_part_ids = model
            .parts
            .iter()
            .map(|part| part.part_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            model_part_ids,
            vec![
                "magnet_clamp_base_n",
                "magnet_clamp_base_s",
                "magnet_polarity_mask_n",
                "magnet_polarity_mask_s"
            ]
        );

        let bundle = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);
        let manifest_part_ids = manifest
            .parts
            .iter()
            .map(|part| part.part_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(bundle.viewer_assets.len(), 4);
        assert_eq!(manifest.document.object_count, 4);
        assert_eq!(manifest.parts.len(), 4);
        assert_eq!(
            manifest_part_ids,
            vec![
                "magnet_clamp_base_n",
                "magnet_clamp_base_s",
                "magnet_polarity_mask_n",
                "magnet_polarity_mask_s"
            ]
        );
        assert!(bundle.export_artifacts.is_empty());
    }

    #[test]
    fn render_core_program_matches_public_render_entrypoint() {
        let source = r#"
            (define base-radius 14)
            (model
              (params
                (number radius base-radius :label "Radius")
                (toggle vents true :label "Vents"))
              (part body
                (difference
                  (extrude (circle radius) 20)
                  (translate 0 0 2 (extrude (circle (- radius 2)) 18)))))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let mut params = DesignParams::new();
        params.insert("radius".into(), ParamValue::Number(16.0));

        let direct_root = render_root();
        std::fs::create_dir_all(&direct_root).expect("direct root");
        let direct = render_core_program(
            &program,
            source,
            &params,
            &TestResolver { root: direct_root },
        )
        .expect("direct render");

        let public_root = render_root();
        std::fs::create_dir_all(&public_root).expect("public root");
        let public =
            crate::ecky_ir::render_model(source, &params, &TestResolver { root: public_root })
                .expect("public render");

        let direct_manifest = read_manifest(&direct);
        let public_manifest = read_manifest(&public);

        assert_eq!(direct.content_hash, public.content_hash);
        assert_eq!(direct.viewer_assets.len(), public.viewer_assets.len());
        assert_eq!(
            direct_manifest.parameter_groups,
            public_manifest.parameter_groups
        );
        assert_eq!(direct_manifest.parts.len(), public_manifest.parts.len());
        assert_eq!(
            direct_manifest.parts[0].bounds,
            public_manifest.parts[0].bounds
        );
        assert_eq!(
            direct_manifest.parts[0].volume,
            public_manifest.parts[0].volume
        );
        assert_eq!(direct_manifest.parts[0].area, public_manifest.parts[0].area);
    }

    #[test]
    fn render_core_program_manifest_includes_ast_identity() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width"))
              (part body (box width 8 6))
              (analysis body-static
                (linear-static :part body)
                (material steel :young-modulus 210000MPa :poisson-ratio 0.3
                  :density 7850kg-per-m3 :yield-strength 250MPa)
                (volume-mesh :element tet4 :size 2mm)
                (fixed :faces (tag mounting))
                (surface-force :faces (tag load-pad) :total [0N 0N -10N])
                (solve :method sparse-direct)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");

        let manifest = read_manifest(&bundle);

        assert!(manifest.source_digest.is_some());
        assert!(manifest.core_digest.is_some());
        assert_eq!(manifest.ast_schema_version, Some(1));
        assert_eq!(manifest.analysis_declarations.len(), 1);
        assert_eq!(manifest.analysis_declarations[0].analysis_id, "body-static");
        assert_eq!(manifest.analysis_declarations[0].part_id, "body");
        assert_eq!(manifest.analysis_declarations[0].element_kind, "tet4");
        assert!(manifest.analysis_declarations[0].source_start.is_some());
        assert!(manifest.analysis_declarations[0].source_end.is_some());
        let indexed_path =
            Path::new(&bundle.viewer_assets[0].path).with_extension("indexed-mesh.json");
        let indexed = crate::ecky_ir::mesh_asset::IndexedMeshAsset::read_cache(&indexed_path)
            .expect("indexed mesh handoff cache");
        assert!(indexed.topology().closed);
    }

    #[test]
    fn render_component_placement_fixture_has_orthogonal_closed_meshes() {
        let source =
            include_str!("../../tests/fixtures/component-placement/dryer-latch-front-side.ecky");
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root: root.clone() },
        )
        .expect("render component placement fixture");
        let manifest = read_manifest(&bundle);

        assert_eq!(manifest.document.object_count, 4);
        assert_eq!(bundle.viewer_assets.len(), 4);
        let bounds = |part_id: &str| {
            manifest
                .parts
                .iter()
                .find(|part| part.part_id == part_id)
                .and_then(|part| part.bounds.as_ref())
                .expect("placed part bounds")
        };
        let front = bounds("front-latch");
        let side = bounds("side-latch");
        assert!(front.x_max - front.x_min > front.y_max - front.y_min);
        assert!(side.y_max - side.y_min > side.x_max - side.x_min);

        for viewer_asset in &bundle.viewer_assets {
            let indexed_path = Path::new(&viewer_asset.path).with_extension("indexed-mesh.json");
            let indexed = crate::ecky_ir::mesh_asset::IndexedMeshAsset::read_cache(&indexed_path)
                .expect("indexed mesh handoff cache");
            let topology = indexed.topology();
            assert_eq!(topology.boundary_edge_count, 0, "{}", viewer_asset.part_id);
            assert_eq!(
                topology.non_manifold_edge_count, 0,
                "{}",
                viewer_asset.part_id
            );
            assert_eq!(
                topology.winding_mismatch_count, 0,
                "{}",
                viewer_asset.part_id
            );
            assert!(topology.closed, "{}", viewer_asset.part_id);
        }

        let evidence = crate::ecky_scheme::compiler::inspect_component_placement_evidence(
            source,
            &std::collections::BTreeMap::new(),
        )
        .expect("placement evidence");
        assert_eq!(evidence.len(), 3);
        assert_eq!(evidence[0].target_port_id, "front");
        assert_eq!(evidence[1].target_port_id, "side-left");
        assert_eq!(evidence[2].target_port_id, "side-right");
        assert_eq!(
            evidence[2].mirror_axis,
            Some(ecky_render::component_placement::MirrorAxis::X)
        );
        let frame = evidence[2].placement_frame;
        let cross = [
            frame.x_axis[1] * frame.y_axis[2] - frame.x_axis[2] * frame.y_axis[1],
            frame.x_axis[2] * frame.y_axis[0] - frame.x_axis[0] * frame.y_axis[2],
            frame.x_axis[0] * frame.y_axis[1] - frame.x_axis[1] * frame.y_axis[0],
        ];
        let handedness = cross
            .iter()
            .zip(frame.z_axis)
            .map(|(left, right)| left * right)
            .sum::<f64>();
        assert!(handedness > 0.999_999, "right-handed frame: {frame:?}");

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn legacy_source_without_ports_keeps_core_identity_bounds_and_emission() {
        let source = "(model\n  (part body (translate 1 2 3 (box 10 20 30))))\n";
        let first_program = crate::ecky_scheme::compile_to_core_program(source).expect("first");
        let second_program = crate::ecky_scheme::compile_to_core_program(source).expect("second");
        assert_eq!(
            first_program
                .parts
                .iter()
                .map(|part| part.key.as_str())
                .collect::<Vec<_>>(),
            second_program
                .parts
                .iter()
                .map(|part| part.key.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            crate::ecky_scheme::compile_to_legacy_source(source).expect("first emission"),
            crate::ecky_scheme::compile_to_legacy_source(source).expect("second emission")
        );

        let first_root = render_root();
        let second_root = render_root();
        std::fs::create_dir_all(&first_root).expect("first root");
        std::fs::create_dir_all(&second_root).expect("second root");
        let first_bundle = render_core_program(
            &first_program,
            source,
            &DesignParams::new(),
            &TestResolver {
                root: first_root.clone(),
            },
        )
        .expect("first render");
        let second_bundle = render_core_program(
            &second_program,
            source,
            &DesignParams::new(),
            &TestResolver {
                root: second_root.clone(),
            },
        )
        .expect("second render");
        let first_manifest = read_manifest(&first_bundle);
        let second_manifest = read_manifest(&second_bundle);
        assert_eq!(first_manifest.core_digest, second_manifest.core_digest);
        assert_eq!(
            first_manifest.parts[0].bounds,
            second_manifest.parts[0].bounds
        );

        std::fs::remove_dir_all(first_root).expect("first cleanup");
        std::fs::remove_dir_all(second_root).expect("second cleanup");
    }

    #[test]
    fn exploded_component_view_is_manifest_only_and_keeps_mesh_bytes() {
        let source = |with_view: bool| {
            format!(
                r#"
                (define-component latch ()
                  (ports (port mount :type "mount.v1" :frame
                    (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1))))
                  (box 20 4 2))
                (model
                  (part enclosure
                    (ports (port side :type "mount.v1" :frame
                      (frame :origin '(50 0 15) :x-axis '(0 1 0) :z-axis '(1 0 0))))
                    (box 100 50 30))
                  (part side-latch
                    (place-component (latch) :from mount
                      :to (port-ref enclosure side) :normal opposed))
                  {})
                "#,
                if with_view {
                    "(view exploded (offset-part side-latch 0 0 40))"
                } else {
                    ""
                }
            )
        };
        let normal_source = source(false);
        let exploded_source = source(true);
        let normal_program =
            crate::ecky_scheme::compile_to_core_program(&normal_source).expect("normal program");
        let exploded_program = crate::ecky_scheme::compile_to_core_program(&exploded_source)
            .expect("exploded program");
        let normal_root = render_root();
        let exploded_root = render_root();
        std::fs::create_dir_all(&normal_root).expect("normal root");
        std::fs::create_dir_all(&exploded_root).expect("exploded root");
        let normal_bundle = render_core_program(
            &normal_program,
            &normal_source,
            &DesignParams::new(),
            &TestResolver {
                root: normal_root.clone(),
            },
        )
        .expect("normal render");
        let exploded_bundle = render_core_program(
            &exploded_program,
            &exploded_source,
            &DesignParams::new(),
            &TestResolver {
                root: exploded_root.clone(),
            },
        )
        .expect("exploded render");

        assert_eq!(
            std::fs::read(&normal_bundle.model_stl_path).expect("normal STL"),
            std::fs::read(&exploded_bundle.model_stl_path).expect("exploded STL")
        );
        let normal_manifest = read_manifest(&normal_bundle);
        let exploded_manifest = read_manifest(&exploded_bundle);
        assert!(normal_manifest.preview_views.is_empty());
        assert_eq!(exploded_manifest.preview_views.len(), 1);
        assert_eq!(exploded_manifest.preview_views[0].view_id, "exploded");
        assert_eq!(
            exploded_manifest.preview_views[0].offsets[0].part_id,
            "side-latch"
        );
        assert_eq!(exploded_manifest.preview_views[0].offsets[0].dz, 40.0);

        std::fs::remove_dir_all(normal_root).expect("normal cleanup");
        std::fs::remove_dir_all(exploded_root).expect("exploded cleanup");
    }

    #[test]
    fn render_model_rebuilds_tampered_indexed_mesh_cache() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 2 2 2)))";
        let first = render_model(source, &DesignParams::new(), &resolver).expect("first render");
        let indexed_path =
            Path::new(&first.viewer_assets[0].path).with_extension("indexed-mesh.json");
        std::fs::write(&indexed_path, b"not indexed mesh json").expect("tamper sidecar");

        let second = render_model(source, &DesignParams::new(), &resolver)
            .expect("invalid indexed sidecar must rebuild, not be returned from cache");
        let restored_path =
            Path::new(&second.viewer_assets[0].path).with_extension("indexed-mesh.json");
        crate::ecky_ir::mesh_asset::IndexedMeshAsset::read_cache(&restored_path)
            .expect("rebuilt indexed sidecar validates");

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn indexed_mesh_provenance_keeps_winding_mismatches_open() {
        let topology = crate::ecky_ir::mesh_asset::IndexedMeshTopology {
            boundary_edge_count: 0,
            non_manifold_edge_count: 0,
            winding_mismatch_count: 1,
            component_count: 1,
            closed: false,
        };

        assert_eq!(indexed_mesh_topology_evidence(&topology), (0, false));
    }

    #[test]
    fn render_core_program_manifest_includes_part_feature_graph_provenance() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width"))
              (part body (box width 8 6)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");

        let manifest = read_manifest(&bundle);
        let feature_graph = manifest.feature_graph.expect("feature graph");

        assert_eq!(feature_graph.nodes.len(), 1);
        assert_eq!(feature_graph.nodes[0].feature_id, "part:body");
        assert_eq!(feature_graph.nodes[0].kind, "part");
        assert_eq!(feature_graph.nodes[0].label, "Body");
        assert_eq!(
            feature_graph.nodes[0]
                .source_ref
                .as_ref()
                .and_then(|source_ref| source_ref.path.as_deref()),
            Some("/parts/body/root")
        );
        assert_eq!(feature_graph.nodes[0].dependency_ids, vec!["width"]);
    }

    #[test]
    fn render_core_program_manifest_scopes_parameter_keys_to_reachable_parts() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width")
                (number radius 2 :label "Radius"))
              (part enclosure (box width 8 6))
              (part axle (cylinder radius 12)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");

        let manifest = read_manifest(&bundle);
        let enclosure = manifest
            .parts
            .iter()
            .find(|part| part.part_id == "enclosure")
            .expect("enclosure");
        let axle = manifest
            .parts
            .iter()
            .find(|part| part.part_id == "axle")
            .expect("axle");

        assert_eq!(enclosure.parameter_keys, ["width"]);
        assert_eq!(axle.parameter_keys, ["radius"]);
        assert!(manifest
            .parameter_groups
            .iter()
            .any(|group| group.group_id == "part:enclosure" && group.parameter_keys == ["width"]));
        assert!(manifest
            .parameter_groups
            .iter()
            .any(|group| group.group_id == "part:axle" && group.parameter_keys == ["radius"]));
        assert!(manifest.control_views.is_empty());
    }

    #[test]
    fn render_core_program_manifest_tracks_transitive_reachable_named_shapes() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width")
                (number height 8 :label "Height")
                (number radius 2 :label "Radius")
                (number unused 99 :label "Unused"))
              (part body
                (build
                  (shape base (box width height 5))
                  (shape rounded (translate radius 0 0 base))
                  (shape discarded (box unused 1 1))
                  (result rounded))))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");

        let manifest = read_manifest(&bundle);
        assert_eq!(
            manifest.parts[0].parameter_keys,
            ["height", "radius", "width"]
        );
        let base = manifest
            .parameter_groups
            .iter()
            .find(|group| group.group_id == "shape:body:base")
            .expect("base group");
        let rounded = manifest
            .parameter_groups
            .iter()
            .find(|group| group.group_id == "shape:body:rounded")
            .expect("rounded group");
        assert_eq!(base.parameter_keys, ["height", "width"]);
        assert_eq!(rounded.parameter_keys, ["height", "radius", "width"]);
        assert!(!manifest
            .parameter_groups
            .iter()
            .any(|group| group.group_id == "shape:body:discarded"));

        let graph = manifest.feature_graph.expect("feature graph");
        assert!(graph.nodes.iter().any(|node| {
            node.feature_id == "shape:body:rounded"
                && node.dependency_ids == ["height", "radius", "width"]
        }));
        assert!(!graph
            .nodes
            .iter()
            .any(|node| node.feature_id == "shape:body:discarded"));
    }

    #[test]
    fn render_core_program_manifest_uses_feature_metadata_for_feature_graph_nodes() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width"))
              (feature shell-cutout :role subtraction :params (width gap) (box width 8 6)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");

        let manifest = read_manifest(&bundle);
        let feature_graph = manifest.feature_graph.expect("feature graph");

        assert_eq!(feature_graph.nodes.len(), 1);
        assert_eq!(feature_graph.nodes[0].feature_id, "shell-cutout");
        assert_eq!(feature_graph.nodes[0].kind, "subtraction");
        assert_eq!(feature_graph.nodes[0].dependency_ids, vec!["width", "gap"]);
    }

    #[test]
    fn render_core_program_manifest_keeps_explicit_feature_params_primary() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width")
                (number gap 1 :label "Gap"))
              (feature shell-cutout :role subtraction :params (gap) (box width 8 6)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");
        let manifest = read_manifest(&bundle);

        assert_eq!(manifest.parts[0].parameter_keys, ["gap", "width"]);
        let group = manifest
            .parameter_groups
            .iter()
            .find(|group| group.group_id == "shell-cutout")
            .expect("feature primary group");
        assert_eq!(group.parameter_keys, ["gap"]);
        let feature = manifest
            .feature_graph
            .expect("feature graph")
            .nodes
            .into_iter()
            .find(|node| node.feature_id == "shell-cutout")
            .expect("feature");
        assert_eq!(feature.dependency_ids, ["gap", "width"]);
    }

    #[test]
    fn runtime_part_feature_graph_links_selection_target_outputs() {
        let parts = vec![RuntimePart {
            part_id: "body".to_string(),
            label: "Body".to_string(),
            expr: IrExpr::symbol("body"),
            feature_decl: None,
            source_ref: runtime_part_source_ref("body", None),
            dependency_ids: vec!["width".to_string()],
            named_shapes: Vec::new(),
        }];
        let selection_targets = vec![crate::contracts::SelectionTarget {
            target_id: Some("target-body".to_string()),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body".to_string(),
            kind: crate::contracts::SelectionTargetKind::Object,
            editable: true,
            parameter_keys: vec!["width".to_string()],
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        }];

        let graph = runtime_part_feature_graph(&parts, &selection_targets);

        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].output_refs.len(), 1);
        assert_eq!(graph.nodes[0].output_refs[0].feature_id, "part:body");
        assert_eq!(
            graph.nodes[0].output_refs[0].target_ids,
            vec!["target-body"]
        );
    }

    #[test]
    fn core_ast_identity_is_deterministic_and_ignores_spans() {
        let source = r#"(model (part body (box 1 2 3)))"#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let repeated = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let changed =
            crate::ecky_scheme::compile_to_core_program(r#"(model (part body (box 1 2 4)))"#)
                .expect("changed program");
        let mut with_span = program.clone();
        with_span.parts[0].root.span = Some(SourceSpan::new(Some(SourceFileId::new(9)), 12, 34));

        let identity = core_ast_identity(&program);

        assert_eq!(
            identity.core_digest,
            core_ast_identity(&repeated).core_digest
        );
        assert_eq!(
            identity.core_digest,
            core_ast_identity(&with_span).core_digest
        );
        assert_ne!(
            identity.core_digest,
            core_ast_identity(&changed).core_digest
        );
    }

    #[test]
    fn render_model_from_model_manifest_keeps_ast_identity_empty() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = r#"(model (part body (box 10 8 6)))"#;
        let model = parse_model(source).expect("model");

        let bundle = render_model_from_model(&model, source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);

        assert!(manifest.source_digest.is_some());
        assert_eq!(manifest.core_digest, None);
        assert_eq!(manifest.ast_schema_version, None);
        assert_eq!(
            manifest
                .feature_graph
                .as_ref()
                .and_then(|graph| graph.nodes.first())
                .and_then(|node| node.source_ref.as_ref())
                .and_then(|source_ref| source_ref.path.as_deref()),
            Some("/parts/body/root")
        );
    }

    #[test]
    fn render_core_program_builds_param_env_from_core_program_defaults_and_overrides() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width"))
              (part body (box width 10 10)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };

        let default_bundle = render_core_program(&program, source, &DesignParams::new(), &resolver)
            .expect("default render");
        let default_manifest = read_manifest(&default_bundle);
        let default_volume = default_manifest.parts[0].volume.expect("default volume");

        let mut override_params = DesignParams::new();
        override_params.insert("width".into(), ParamValue::Number(20.0));
        let override_bundle = render_core_program(&program, source, &override_params, &resolver)
            .expect("override render");
        let override_manifest = read_manifest(&override_bundle);
        let override_volume = override_manifest.parts[0].volume.expect("override volume");

        assert!(
            (default_volume - 1000.0).abs() < 1e-6,
            "default volume {default_volume}"
        );
        assert!(
            (override_volume - 2000.0).abs() < 1e-6,
            "override volume {override_volume}"
        );
        assert_eq!(
            override_manifest.parameter_groups[0].parameter_keys,
            vec!["width".to_string()]
        );
    }

    #[test]
    fn render_core_program_bypasses_full_model_bridge_for_text_params() {
        fn num(id: u64, value: f64) -> CoreNode {
            CoreNode::new(
                NodeId::new(id),
                CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(value)),
                CoreValueKind::Number,
            )
        }

        let source = "(model (params (text-param label \"hello\" :label \"Label\")) (part body (box 10 10 10)))";
        let program = CoreProgram::new(
            ProgramId::new(1),
            vec![CoreParameter {
                id: ParamId::new(2),
                key: "label".into(),
                label: "Label".into(),
                kind: CoreParameterKind::Text,
                default_value: CoreParameterValue::Text("hello".into()),
                frozen: false,
                constraints: CoreParameterConstraints::default(),
            }],
            vec![CorePart {
                id: PartId::new(3),
                key: "body".into(),
                label: "Body".into(),
                root: CoreNode::new(
                    NodeId::new(4),
                    CoreNodeKind::Call {
                        op: CoreOperation::Primitive(CorePrimitive::Box),
                        args: vec![num(5, 10.0), num(6, 10.0), num(7, 10.0)],
                        keywords: vec![],
                    },
                    CoreValueKind::Solid,
                ),
            }],
        );

        let bridge_err = match core_program_to_model(&program) {
            Ok(_) => panic!("legacy bridge should fail"),
            Err(err) => err,
        };
        assert!(
            bridge_err
                .details
                .as_deref()
                .unwrap_or("")
                .contains("Text params are not yet supported by the legacy IR bridge."),
            "unexpected bridge error: {}",
            bridge_err.message
        );

        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("direct render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.model_stl_path).exists());
    }

    #[test]
    fn runtime_core_part_conversion_matches_legacy_bridge() {
        let source = r#"
            (define base-radius 14)
            (model
              (params
                (number radius base-radius :label "Radius")
                (toggle vents true :label "Vents"))
              (part body
                (build
                  (shape outer (extrude (circle radius) 20))
                  (shape inner (translate 0 0 2 (extrude (circle (- radius 2)) 18)))
                  (result
                    (if vents
                      (difference outer inner)
                      outer)))))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let legacy = core_part_to_ir_part(&program.parts[0], &param_names).expect("legacy");
        let runtime = runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        )
        .expect("runtime");

        assert_eq!(runtime.part_id, legacy.part_id);
        assert_eq!(runtime.label, legacy.label);
        assert_eq!(runtime.expr, legacy.expr);
    }

    #[test]
    fn runtime_core_part_conversion_materializes_selector_nodes() {
        let source =
            "(model (part body (fillet 1 :edges \"target-id:body:edge:0:0-0-0_1-0-0\" (box 1 1 1))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let runtime = runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        )
        .expect("runtime");
        assert!(
            contains_edge_selector(&runtime.expr),
            "expected typed selector in {:?}",
            runtime.expr
        );
    }

    #[test]
    fn runtime_core_part_conversion_materializes_face_selector_nodes() {
        let source =
            "(model (part body (shell 1 :faces \"target-id:body:face:0:0-0-1:1\" (box 1 1 1))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let runtime = runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        )
        .expect("runtime");
        assert!(
            contains_face_selector(&runtime.expr),
            "expected typed face selector in {:?}",
            runtime.expr
        );
    }

    #[test]
    fn runtime_core_part_conversion_rejects_missing_selector_payload_on_edges_keyword() {
        let source = "(model (part body (fillet 1 :edges \"left+vertical\" (box 1 1 1))))";
        let mut program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } =
            &mut program.parts[0].root.kind
        else {
            panic!("expected call");
        };
        keywords[0].set_selector_payload(None);
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let err = match runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        ) {
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
    fn runtime_core_part_conversion_rejects_wrong_kind_selector_payload_on_edges_keyword() {
        let source = "(model (part body (fillet 1 :edges \"left+vertical\" (box 1 1 1))))";
        let mut program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
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
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let err = match runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        ) {
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
