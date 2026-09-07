#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::direct_occt::{OcctArg, OcctOp};
use super::direct_occt_sdk::{DirectOcctSdkLayout, NativeExportOutcome};
use crate::contracts::{
    AnalysisDeclarationBinding, AppError, AppResult, ArtifactBundle, DesignParams,
    DocumentMetadata, EngineKind, EnrichmentStatus, ExportArtifact, FeatureGraph, FeatureNode,
    GeometryBackend, GeometryProvenance, GeometryRepresentation, ManifestBounds,
    ManifestEnrichmentState, ModelManifest, ModelSourceKind, ParameterGroup, PartBinding,
    PreviewView, PreviewViewOffset, SelectionTarget, SelectionTargetKind, SourceLanguage,
    SourceRef, ViewerEdgePoint, ViewerEdgeTarget, ViewerFaceTarget, MODEL_RUNTIME_SCHEMA_VERSION,
};
use crate::ecky_core_ir::{
    CoreMetadataValue, CoreNode, CoreNodeKind, CoreOperation, CorePart, CorePrimitive, CoreProgram,
    CoreReference, CoreSelectorTagDecl,
};
use crate::ecky_ir::mesh_asset::{IndexedMeshAsset, MeshAssetSource};
use crate::models::PathResolver;
use crate::topology_target_ids::{
    durable_edge_target_id, durable_edge_target_id_for_stable_node_key, durable_face_target_id,
    durable_face_target_id_for_stable_node_key, durable_vertex_target_id,
    durable_vertex_target_id_for_stable_node_key, preferred_public_topology_target_id,
    resolve_tagged_anchors_with_authored_bindings, stable_edge_target_id, stable_face_target_id,
    stable_vertex_target_id, topology_target_aliases, viewer_target_alias_ids,
};

const SOURCE_FILE_NAME: &str = "source.ecky";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const MODEL_STL_FILE_NAME: &str = "model.stl";
const STEP_FILE_NAME: &str = "model.step";
const TOPOLOGY_FILE_NAME: &str = "topology.json";
const DIRECT_OCCT_TEXT_FONT_ENV: &str = "ECKYCAD_FONT_PATH";
const DIRECT_OCCT_HOT_CACHE_CAPACITY: usize = 2;
/// Resident on-disk artifact bytes the process-hot Direct OCCT cache may hold
/// before byte-budgeted LRU eviction drops entries. This is a secondary bound
/// on top of [`DIRECT_OCCT_HOT_CACHE_CAPACITY`]; it guards a couple of
/// pathological oversized renders from pinning the hot cache.
const DIRECT_OCCT_HOT_CACHE_BYTE_BUDGET: u64 = 128 * 1024 * 1024;
const DIRECT_OCCT_CACHE_SCHEMA: &str = "direct-occt-v10-svg-fill-parity";
const DIRECT_OCCT_GEOMETRY_CACHE_SCHEMA: &str = "direct-occt-geometry-v1";
const DIRECT_OCCT_GEOMETRY_CACHE_DIR: &str = "direct-occt-geometry";
const DIRECT_OCCT_GEOMETRY_CACHE_FILE: &str = "geometry-cache.json";
const DIGESTS_FILE_NAME: &str = "digests.json";
const CACHED_ARTIFACT_DIGEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctGeometryCacheManifest {
    schema: String,
    has_step: bool,
    tessellated_step: bool,
    source_mesh_digests: Vec<String>,
    part_assets: Vec<DirectOcctGeometryCachePartAsset>,
    digests: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctGeometryCachePartAsset {
    part_key: String,
    file_name: String,
}

fn direct_occt_analytic_brep_provenance() -> GeometryProvenance {
    GeometryProvenance {
        representation: GeometryRepresentation::AnalyticBrep,
        source_mesh_digests: Vec::new(),
        closed: None,
        boundary_or_non_manifold_edge_count: None,
    }
}

fn direct_occt_provenance(
    representation: GeometryRepresentation,
    source_mesh_digests: Vec<String>,
) -> GeometryProvenance {
    GeometryProvenance {
        representation,
        source_mesh_digests,
        closed: None,
        boundary_or_non_manifold_edge_count: None,
    }
}

#[derive(Clone)]
struct DirectOcctHotCacheEntry {
    bundle_dir: PathBuf,
    content_hash: String,
    bundle: ArtifactBundle,
    manifest: ModelManifest,
    /// Resident on-disk artifact bytes for [`bundle`], recorded once at
    /// insertion via [`resident_artifact_bytes`] so byte-budgeted eviction
    /// never re-stats files. See [`DIRECT_OCCT_HOT_CACHE_BYTE_BUDGET`].
    resident_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectOcctTopologyReport {
    #[serde(default)]
    parts: Vec<DirectOcctTopologyPart>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyPart {
    part_id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    vertices: Vec<DirectOcctTopologyVertex>,
    #[serde(default)]
    edges: Vec<DirectOcctTopologyEdge>,
    #[serde(default)]
    faces: Vec<DirectOcctTopologyFace>,
    #[serde(default)]
    source_geometry_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectOcctSolidDiagnostics {
    pub part_count: usize,
    pub solid_count: u64,
    pub all_breps_valid: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyVertex {
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    vertex_index: Option<u32>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    point: Option<DirectOcctTopologyPoint>,
    #[serde(default)]
    authored_bindings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyEdge {
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    edge_index: Option<u32>,
    #[serde(default)]
    originating_slot_index: Option<u64>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    start: Option<DirectOcctTopologyPoint>,
    #[serde(default)]
    end: Option<DirectOcctTopologyPoint>,
    #[serde(default)]
    authored_bindings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyFace {
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    face_index: Option<u32>,
    #[serde(default)]
    originating_slot_index: Option<u64>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    center: Option<DirectOcctTopologyPoint>,
    #[serde(default)]
    normal: Option<[f64; 3]>,
    #[serde(default)]
    area: Option<f64>,
    #[serde(default)]
    authored_bindings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyPoint {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone, Default)]
struct DirectOcctPartProvenance {
    parameter_keys: Vec<String>,
    named_shapes: Vec<(String, Vec<String>)>,
    faceted_mesh_root: bool,
}

fn direct_occt_program_provenance(
    program: &CoreProgram,
) -> BTreeMap<String, DirectOcctPartProvenance> {
    let param_names = program
        .parameters
        .iter()
        .map(|parameter| (parameter.id.raw(), parameter.key.clone()))
        .collect::<BTreeMap<_, _>>();
    program
        .parts
        .iter()
        .map(|part| {
            (
                part.key.clone(),
                direct_occt_part_provenance(part, &param_names),
            )
        })
        .collect()
}

fn direct_occt_part_provenance(
    part: &CorePart,
    param_names: &BTreeMap<u64, String>,
) -> DirectOcctPartProvenance {
    let mut node_index = BTreeMap::new();
    let mut shapes = Vec::new();
    direct_occt_index_nodes(&part.root, &BTreeMap::new(), &mut node_index, &mut shapes);
    let mut reachable = BTreeSet::new();
    let parameter_keys = direct_occt_node_dependencies(
        &part.root,
        param_names,
        &node_index,
        &BTreeMap::new(),
        &mut reachable,
    );
    let mut counts = BTreeMap::<String, usize>::new();
    for (name, _, _) in &shapes {
        *counts.entry(name.clone()).or_default() += 1;
    }
    let named_shapes = shapes
        .into_iter()
        .filter(|(name, node, _)| {
            counts.get(name) == Some(&1) && reachable.contains(&node.id.raw())
        })
        .map(|(name, node, locals)| {
            let mut shape_reachable = BTreeSet::new();
            let keys = direct_occt_node_dependencies(
                node,
                param_names,
                &node_index,
                &locals,
                &mut shape_reachable,
            );
            (name, keys)
        })
        .collect();
    DirectOcctPartProvenance {
        parameter_keys,
        named_shapes,
        faceted_mesh_root: direct_occt_part_is_faceted_mesh_root(&part.root),
    }
}

fn direct_occt_part_is_faceted_mesh_root(root: &CoreNode) -> bool {
    let CoreNodeKind::Call { op, args, .. } = &root.kind else {
        return false;
    };
    if !matches!(op, CoreOperation::Custom(name) if name == "solidify") {
        return false;
    }
    matches!(
        args.as_slice(),
        [CoreNode {
            kind: CoreNodeKind::Call {
                op: CoreOperation::Primitive(CorePrimitive::Stl),
                ..
            },
            ..
        }]
    )
}

fn direct_occt_index_nodes<'a>(
    node: &'a CoreNode,
    locals: &BTreeMap<String, u64>,
    node_index: &mut BTreeMap<u64, &'a CoreNode>,
    shapes: &mut Vec<(String, &'a CoreNode, BTreeMap<String, u64>)>,
) {
    node_index.insert(node.id.raw(), node);
    match &node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => {}
        CoreNodeKind::Build { bindings, result } => {
            let mut nested = locals.clone();
            for binding in bindings {
                shapes.push((binding.name.clone(), &binding.value, nested.clone()));
                direct_occt_index_nodes(&binding.value, &nested, node_index, shapes);
                nested.insert(binding.name.clone(), binding.value.id.raw());
            }
            direct_occt_index_nodes(result, &nested, node_index, shapes);
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested = locals.clone();
            for binding in bindings {
                direct_occt_index_nodes(&binding.value, &nested, node_index, shapes);
                nested.insert(binding.name.clone(), binding.value.id.raw());
            }
            direct_occt_index_nodes(body, &nested, node_index, shapes);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            direct_occt_index_nodes(condition, locals, node_index, shapes);
            direct_occt_index_nodes(then_branch, locals, node_index, shapes);
            direct_occt_index_nodes(else_branch, locals, node_index, shapes);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                direct_occt_index_nodes(arg, locals, node_index, shapes);
            }
            for keyword in keywords {
                direct_occt_index_nodes(keyword.source_node(), locals, node_index, shapes);
            }
        }
        CoreNodeKind::Range { start, end } => {
            direct_occt_index_nodes(start, locals, node_index, shapes);
            direct_occt_index_nodes(end, locals, node_index, shapes);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                direct_occt_index_nodes(source, locals, node_index, shapes);
            }
            direct_occt_index_nodes(body, locals, node_index, shapes);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                direct_occt_index_nodes(arg, locals, node_index, shapes);
            }
            direct_occt_index_nodes(list, locals, node_index, shapes);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                direct_occt_index_nodes(item, locals, node_index, shapes);
            }
        }
    }
}

fn direct_occt_node_dependencies(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    node_index: &BTreeMap<u64, &CoreNode>,
    locals: &BTreeMap<String, u64>,
    reachable: &mut BTreeSet<u64>,
) -> Vec<String> {
    let mut keys = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    direct_occt_collect_node_dependencies(
        node,
        param_names,
        node_index,
        locals,
        reachable,
        &mut visiting,
        &mut keys,
    );
    keys.into_iter().collect()
}

fn direct_occt_collect_node_dependencies(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    node_index: &BTreeMap<u64, &CoreNode>,
    locals: &BTreeMap<String, u64>,
    reachable: &mut BTreeSet<u64>,
    visiting: &mut BTreeSet<u64>,
    keys: &mut BTreeSet<String>,
) {
    let node_id = node.id.raw();
    reachable.insert(node_id);
    if !visiting.insert(node_id) {
        return;
    }
    match &node.kind {
        CoreNodeKind::Literal(_) => {}
        CoreNodeKind::Reference(CoreReference::Parameter(param_id)) => {
            if let Some(key) = param_names.get(&param_id.raw()) {
                keys.insert(key.clone());
            }
        }
        CoreNodeKind::Reference(CoreReference::Node(id)) => {
            if let Some(target) = node_index.get(&id.raw()) {
                direct_occt_collect_node_dependencies(
                    target,
                    param_names,
                    node_index,
                    locals,
                    reachable,
                    visiting,
                    keys,
                );
            }
        }
        CoreNodeKind::Reference(CoreReference::Local(name)) => {
            if let Some(target) = locals.get(name).and_then(|id| node_index.get(id)) {
                direct_occt_collect_node_dependencies(
                    target,
                    param_names,
                    node_index,
                    locals,
                    reachable,
                    visiting,
                    keys,
                );
            }
        }
        CoreNodeKind::Reference(CoreReference::Part(_)) => {}
        CoreNodeKind::Build { bindings, result } => {
            let mut nested = locals.clone();
            for binding in bindings {
                nested.insert(binding.name.clone(), binding.value.id.raw());
            }
            direct_occt_collect_node_dependencies(
                result,
                param_names,
                node_index,
                &nested,
                reachable,
                visiting,
                keys,
            );
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested = locals.clone();
            for binding in bindings {
                nested.insert(binding.name.clone(), binding.value.id.raw());
            }
            direct_occt_collect_node_dependencies(
                body,
                param_names,
                node_index,
                &nested,
                reachable,
                visiting,
                keys,
            );
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            for child in [
                condition.as_ref(),
                then_branch.as_ref(),
                else_branch.as_ref(),
            ] {
                direct_occt_collect_node_dependencies(
                    child,
                    param_names,
                    node_index,
                    locals,
                    reachable,
                    visiting,
                    keys,
                );
            }
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for child in args
                .iter()
                .chain(keywords.iter().map(|keyword| keyword.source_node()))
            {
                direct_occt_collect_node_dependencies(
                    child,
                    param_names,
                    node_index,
                    locals,
                    reachable,
                    visiting,
                    keys,
                );
            }
        }
        CoreNodeKind::Range { start, end } => {
            for child in [start.as_ref(), end.as_ref()] {
                direct_occt_collect_node_dependencies(
                    child,
                    param_names,
                    node_index,
                    locals,
                    reachable,
                    visiting,
                    keys,
                );
            }
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for child in sources.iter().chain(std::iter::once(body.as_ref())) {
                direct_occt_collect_node_dependencies(
                    child,
                    param_names,
                    node_index,
                    locals,
                    reachable,
                    visiting,
                    keys,
                );
            }
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for child in args.iter().chain(std::iter::once(list.as_ref())) {
                direct_occt_collect_node_dependencies(
                    child,
                    param_names,
                    node_index,
                    locals,
                    reachable,
                    visiting,
                    keys,
                );
            }
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for child in items {
                direct_occt_collect_node_dependencies(
                    child,
                    param_names,
                    node_index,
                    locals,
                    reachable,
                    visiting,
                    keys,
                );
            }
        }
    }
    visiting.remove(&node_id);
}

pub(crate) fn render_core_program_runtime_bundle(
    program: &CoreProgram,
    source_identity: &str,
    parameters: &DesignParams,
    layout: &DirectOcctSdkLayout,
    app: &dyn PathResolver,
) -> AppResult<(ArtifactBundle, ModelManifest)> {
    render_core_program_runtime_bundle_with_font_path(
        program,
        source_identity,
        parameters,
        layout,
        app,
        None,
    )
}

pub(crate) fn render_core_program_runtime_bundle_with_font_path(
    program: &CoreProgram,
    source_identity: &str,
    parameters: &DesignParams,
    layout: &DirectOcctSdkLayout,
    app: &dyn PathResolver,
    cad_text_font_path: Option<&str>,
) -> AppResult<(ArtifactBundle, ModelManifest)> {
    let params_json =
        serde_json::to_string(parameters).map_err(|err| AppError::validation(err.to_string()))?;
    let content_hash = content_hash_with_runtime_inputs(
        program,
        source_identity,
        &params_json,
        parameters,
        cad_text_font_path,
    )?;
    let model_id = model_id_from_hash(&content_hash);
    if let Some(cached) = read_complete_cached_bundle(app, &model_id, &content_hash) {
        return Ok(cached);
    }
    let bundle_dir = crate::model_runtime::runtime_bundle_dir(app, &model_id)?;

    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;
    let source_path = bundle_dir.join(SOURCE_FILE_NAME);
    fs::write(&source_path, source_identity)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let geometry_hash =
        direct_occt_geometry_hash(program, &params_json, parameters, cad_text_font_path, app)?;
    let export_outcome = match read_cached_direct_occt_geometry(app, &geometry_hash, &bundle_dir)? {
        Some(outcome) => outcome,
        None => match with_direct_occt_text_font_path(cad_text_font_path, || {
            super::direct_occt_executor::export_core_program_step_stl_with_params_runner_first(
                program,
                parameters,
                layout,
                &bundle_dir,
                app,
            )
        }) {
            Ok(outcome) => {
                write_cached_direct_occt_geometry(app, &geometry_hash, &bundle_dir, &outcome)?;
                outcome
            }
            Err(err) => {
                let _ = fs::remove_dir_all(&bundle_dir);
                return Err(err);
            }
        },
    };

    let (step_path, stl_path, part_stl_paths, geometry_provenance) = match export_outcome {
        NativeExportOutcome::Exported {
            step_path,
            stl_path,
            part_stl_paths,
            tessellated_step,
            source_mesh_digests,
        } => (
            Some(step_path),
            stl_path,
            part_stl_paths,
            if tessellated_step {
                direct_occt_provenance(GeometryRepresentation::Hybrid, source_mesh_digests)
            } else {
                direct_occt_analytic_brep_provenance()
            },
        ),
        NativeExportOutcome::MeshExported {
            stl_path,
            part_stl_paths,
            source_mesh_digests,
        } => (
            None,
            stl_path,
            part_stl_paths,
            direct_occt_provenance(GeometryRepresentation::MeshNative, source_mesh_digests),
        ),
        NativeExportOutcome::Blocked { blockers } => {
            let _ = fs::remove_dir_all(&bundle_dir);
            return Err(AppError::render(format!(
                "Direct OCCT runtime blocked: {}",
                if blockers.is_empty() {
                    "unknown runtime blocker".to_string()
                } else {
                    blockers.join("; ")
                }
            )));
        }
    };

    if program.parts.is_empty() {
        return Err(AppError::validation(
            "Direct OCCT runtime requires at least one Core IR part.",
        ));
    }
    let topology_path = bundle_dir.join(TOPOLOGY_FILE_NAME);
    let topology_report = read_direct_occt_topology_report(&topology_path)?;
    let topology_report = Some(&topology_report);
    let parameter_keys = program
        .parameters
        .iter()
        .map(|parameter| parameter.key.clone())
        .collect::<Vec<_>>();
    let part_specs = program
        .parts
        .iter()
        .map(|part| (part.key.clone(), part.label.clone()))
        .collect::<Vec<_>>();
    let part_root_node_ids = program
        .parts
        .iter()
        .map(|part| (part.key.clone(), part.root.id.raw()))
        .collect::<HashMap<_, _>>();
    let part_stable_node_keys = program
        .parts
        .iter()
        .filter_map(|part| {
            direct_occt_source_stable_node_key(source_identity, part)
                .map(|stable_node_key| (part.key.clone(), stable_node_key))
        })
        .collect::<HashMap<_, _>>();
    // Build a map of part_key -> bundle-relative STL path. Per-part STL
    // files are written by the executor into `parts/NNN-label.stl`. When
    // the backend only produced a merged preview, fall back to that.
    let part_asset_paths = part_stl_paths
        .iter()
        .filter_map(|(key, abs_path)| {
            let rel = abs_path.strip_prefix(&bundle_dir).ok()?;
            Some((
                key.clone(),
                path_to_string(rel).unwrap_or_else(|_| rel.to_string_lossy().to_string()),
            ))
        })
        .collect::<HashMap<_, _>>();
    let part_bounds =
        direct_occt_part_bounds(&part_specs, &part_asset_paths, &bundle_dir, &stl_path);
    let mut program_provenance = direct_occt_program_provenance(program);
    for part in &program.parts {
        let Some(feature) = program.feature_decls.get(&part.key) else {
            continue;
        };
        let Some(provenance) = program_provenance.get_mut(&part.key) else {
            continue;
        };
        let mut keys = provenance
            .parameter_keys
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        keys.extend(feature.param_keys.iter().cloned());
        provenance.parameter_keys = keys.into_iter().collect();
    }
    let mut manifest = build_direct_occt_manifest_with_program_provenance(
        &model_id,
        &source_path,
        &part_specs,
        &parameter_keys,
        &program.selector_tags,
        topology_report,
        &part_stable_node_keys,
        &part_root_node_ids,
        &part_asset_paths,
        &part_bounds,
        &program_provenance,
    )?;
    apply_direct_occt_model_metadata(&mut manifest.document, program);
    apply_direct_occt_program_provenance(&mut manifest, program, &program_provenance);
    manifest.analysis_declarations = program
        .analyses
        .iter()
        .map(|analysis| AnalysisDeclarationBinding {
            analysis_id: analysis.name.clone(),
            kind: "linearStatic".to_string(),
            part_id: analysis.part.clone(),
            element_kind: analysis.element.clone(),
            source_start: analysis.span.map(|span| span.start),
            source_end: analysis.span.map(|span| span.end),
        })
        .collect();
    manifest.geometry_provenance = Some(geometry_provenance.clone());
    let thread_warnings = super::direct_occt::thread_printability_warnings(&program, parameters)?;
    manifest
        .document
        .warnings
        .extend(thread_warnings.iter().cloned());
    manifest.warnings.extend(thread_warnings);
    let fallback_step_path = bundle_dir.join(STEP_FILE_NAME);
    let mut bundle = build_direct_occt_bundle(
        &model_id,
        &content_hash,
        &source_path,
        &stl_path,
        step_path.as_deref().unwrap_or(&fallback_step_path),
        topology_report,
        &manifest,
    )?;
    bundle.geometry_provenance = Some(geometry_provenance.clone());
    for artifact in &mut bundle.export_artifacts {
        artifact.geometry_provenance = Some(geometry_provenance.clone());
    }
    if step_path.is_none() {
        bundle.export_artifacts.clear();
    }
    let stored = crate::model_runtime::write_runtime_bundle(app, &model_id, &bundle, &manifest)?;
    write_complete_cached_bundle_digests(&bundle_dir, &stored.0)?;
    remember_complete_cached_bundle(&bundle_dir, &content_hash, &stored);
    Ok(stored)
}

fn apply_direct_occt_model_metadata(document: &mut DocumentMetadata, program: &CoreProgram) {
    let title = program
        .metadata
        .get(":title")
        .or_else(|| program.metadata.get("title"));
    if let Some(CoreMetadataValue::Text(title) | CoreMetadataValue::Symbol(title)) = title {
        let title = title.trim();
        if !title.is_empty() {
            document.document_label = title.to_string();
        }
    }
}

fn read_complete_cached_bundle(
    app: &dyn PathResolver,
    model_id: &str,
    content_hash: &str,
) -> Option<(ArtifactBundle, ModelManifest)> {
    let bundle_dir = crate::model_runtime::runtime_bundle_dir(app, model_id).ok()?;
    if let Some(cached) = read_hot_cached_bundle(&bundle_dir, content_hash) {
        if cached_direct_occt_bundle_is_current(&cached.0, &cached.1, model_id, content_hash) {
            return Some(cached);
        }
        forget_hot_cached_bundle(&bundle_dir);
    }
    let (bundle, manifest) = crate::model_runtime::read_runtime_bundle(app, model_id).ok()?;
    if !cached_direct_occt_bundle_is_current(&bundle, &manifest, model_id, content_hash) {
        return None;
    }
    if !complete_cached_bundle_digests_match(&bundle_dir, &bundle) {
        return None;
    }
    let cached = (bundle, manifest);
    remember_complete_cached_bundle(&bundle_dir, content_hash, &cached);
    Some(cached)
}

fn cached_direct_occt_bundle_is_current(
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    model_id: &str,
    content_hash: &str,
) -> bool {
    let bundle_provenance = bundle.geometry_provenance.as_ref();
    let manifest_provenance = manifest.geometry_provenance.as_ref();
    let supported = |provenance: Option<&GeometryProvenance>| {
        matches!(
            provenance.map(|value| &value.representation),
            Some(
                GeometryRepresentation::AnalyticBrep
                    | GeometryRepresentation::Hybrid
                    | GeometryRepresentation::MeshNative
            )
        )
    };

    bundle.content_hash == content_hash
        && bundle.model_id == model_id
        && manifest.model_id == model_id
        && supported(bundle_provenance)
        && bundle_provenance == manifest_provenance
        && bundle
            .export_artifacts
            .iter()
            .filter(|artifact| artifact.format.eq_ignore_ascii_case("step"))
            .all(|artifact| artifact.geometry_provenance.as_ref() == bundle_provenance)
        && runtime_bundle_artifacts_ready(bundle)
}

fn runtime_bundle_artifacts_ready(bundle: &ArtifactBundle) -> bool {
    let path_ready = |path: &str| {
        !path.trim().is_empty()
            && fs::metadata(path)
                .map(|metadata| metadata.is_file() && metadata.len() > 0)
                .unwrap_or(false)
    };
    path_ready(&bundle.model_stl_path)
        && bundle
            .viewer_assets
            .iter()
            .all(|asset| path_ready(&asset.path))
        && bundle
            .export_artifacts
            .iter()
            .all(|artifact| path_ready(&artifact.path))
        && bundle.macro_path.as_deref().is_none_or(path_ready)
}

/// Collects the on-disk artifact paths that participate in warm reuse, in the
/// same set covered by [`runtime_bundle_artifacts_ready`]. Each of these paths
/// owns an immutable geometry or source artifact that must not change between
/// the verified render and a later warm reuse.
fn cached_artifact_paths(bundle: &ArtifactBundle) -> Vec<String> {
    let mut paths = Vec::new();
    let push_trimmed = |paths: &mut Vec<String>, path: &str| {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            paths.push(trimmed.to_string());
        }
    };
    push_trimmed(&mut paths, &bundle.model_stl_path);
    for asset in &bundle.viewer_assets {
        push_trimmed(&mut paths, &asset.path);
    }
    for artifact in &bundle.export_artifacts {
        push_trimmed(&mut paths, &artifact.path);
    }
    if let Some(macro_path) = bundle.macro_path.as_deref() {
        push_trimmed(&mut paths, macro_path);
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Resident on-disk bytes used by a cached bundle. Computed once from the same
/// [`cached_artifact_paths`] metadata that warm-reuse readiness and the
/// per-artifact digest sidecar cover, then stored on the hot entry so eviction
/// never re-stats files. Missing files contribute zero bytes.
fn resident_artifact_bytes(bundle: &ArtifactBundle) -> u64 {
    cached_artifact_paths(bundle)
        .iter()
        .filter_map(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .sum()
}

fn file_sha256_hex(path: &str) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedArtifactDigestSidecar {
    schema_version: u32,
    digests: std::collections::BTreeMap<String, String>,
}

/// Writes a per-artifact content digest sidecar next to a freshly rendered
/// complete bundle. The render path calls this after the verified bundle is
/// persisted so that later warm reuse can detect on-disk mutation (including
/// same-size edits that a size-only readiness check would miss).
fn write_complete_cached_bundle_digests(
    bundle_dir: &Path,
    bundle: &ArtifactBundle,
) -> AppResult<()> {
    let mut digests = std::collections::BTreeMap::new();
    for path in cached_artifact_paths(bundle) {
        let digest = file_sha256_hex(&path).ok_or_else(|| {
            AppError::persistence(format!(
                "Cannot compute artifact digest for cached bundle '{}': missing artifact '{}'",
                bundle_dir.display(),
                path
            ))
        })?;
        digests.insert(path, digest);
    }
    let sidecar = CachedArtifactDigestSidecar {
        schema_version: CACHED_ARTIFACT_DIGEST_SCHEMA_VERSION,
        digests,
    };
    let data = serde_json::to_string_pretty(&sidecar)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    // Publish the sidecar atomically: write a temp file in the bundle
    // directory, then rename it into place as the final step. Readers only
    // ever observe the previous complete sidecar or the new complete one,
    // never a partially-written `digests.json`. On any failure the temp file
    // is removed so no partial residue lingers.
    let final_path = bundle_dir.join(DIGESTS_FILE_NAME);
    let temp_path = bundle_dir.join(format!("{DIGESTS_FILE_NAME}.tmp"));
    if let Err(err) = fs::write(&temp_path, &data) {
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::persistence(format!(
            "Failed to write cached artifact digests '{}': {}",
            temp_path.display(),
            err
        )));
    }
    if let Err(err) = fs::rename(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::persistence(format!(
            "Failed to publish cached artifact digests '{}': {}",
            final_path.display(),
            err
        )));
    }
    Ok(())
}

/// Returns `true` when the on-disk artifact bytes still match the stored
/// per-artifact digests. The sidecar is the trust anchor for cold reuse of a
/// complete bundle: a missing or unreadable sidecar is a cache miss (the
/// bundle is re-rendered), which is what happens when sidecar persistence
/// failed after `write_runtime_bundle` succeeded. A present sidecar is
/// enforced strictly and rejects any byte mismatch or uncovered artifact,
/// including a same-size mutation that a size-only readiness check cannot
/// detect.
fn complete_cached_bundle_digests_match(bundle_dir: &Path, bundle: &ArtifactBundle) -> bool {
    let sidecar_path = bundle_dir.join(DIGESTS_FILE_NAME);
    let raw = match fs::read_to_string(&sidecar_path) {
        Ok(raw) => raw,
        Err(_) => return false,
    };
    let sidecar: CachedArtifactDigestSidecar = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };
    if sidecar.schema_version != CACHED_ARTIFACT_DIGEST_SCHEMA_VERSION {
        return false;
    }
    for path in cached_artifact_paths(bundle) {
        let expected = match sidecar.digests.get(&path) {
            Some(digest) => digest,
            None => return false,
        };
        let actual = match file_sha256_hex(&path) {
            Some(digest) => digest,
            None => return false,
        };
        if &actual != expected {
            return false;
        }
    }
    true
}

fn direct_occt_hot_cache() -> &'static Mutex<VecDeque<DirectOcctHotCacheEntry>> {
    static CACHE: OnceLock<Mutex<VecDeque<DirectOcctHotCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn read_hot_cached_bundle(
    bundle_dir: &Path,
    content_hash: &str,
) -> Option<(ArtifactBundle, ModelManifest)> {
    let mut cache = direct_occt_hot_cache().lock().ok()?;
    let position = cache
        .iter()
        .position(|entry| entry.bundle_dir == bundle_dir && entry.content_hash == content_hash)?;
    let entry = cache.remove(position)?;
    if !runtime_bundle_artifacts_ready(&entry.bundle) {
        return None;
    }
    let cached = (entry.bundle.clone(), entry.manifest.clone());
    cache.push_front(entry);
    Some(cached)
}

fn forget_hot_cached_bundle(bundle_dir: &Path) {
    let Ok(mut cache) = direct_occt_hot_cache().lock() else {
        return;
    };
    cache.retain(|entry| entry.bundle_dir != bundle_dir);
}

fn remember_complete_cached_bundle(
    bundle_dir: &Path,
    content_hash: &str,
    cached: &(ArtifactBundle, ModelManifest),
) {
    let Ok(mut cache) = direct_occt_hot_cache().lock() else {
        return;
    };
    cache.retain(|entry| entry.bundle_dir != bundle_dir);
    let resident_bytes = resident_artifact_bytes(&cached.0);
    cache.push_front(DirectOcctHotCacheEntry {
        bundle_dir: bundle_dir.to_path_buf(),
        content_hash: content_hash.to_string(),
        bundle: cached.0.clone(),
        manifest: cached.1.clone(),
        resident_bytes,
    });
    cache.truncate(DIRECT_OCCT_HOT_CACHE_CAPACITY);
    evict_hot_cache_to_byte_budget(&mut cache, DIRECT_OCCT_HOT_CACHE_BYTE_BUDGET);
}

/// Total resident artifact bytes held by `entries`.
fn total_resident_bytes(entries: &VecDeque<DirectOcctHotCacheEntry>) -> u64 {
    entries.iter().map(|entry| entry.resident_bytes).sum()
}

/// Evicts least-recently-used hot cache entries from the back until the total
/// resident artifact bytes are within `byte_budget`. Because eviction
/// continues while the running total exceeds the budget, an entry larger than
/// the whole budget is never retained either, even at the most-recently-used
/// position. Pure over `entries`: it never touches the process-global cache,
/// so it is safe to unit-test deterministically without cache races.
fn evict_hot_cache_to_byte_budget(
    entries: &mut VecDeque<DirectOcctHotCacheEntry>,
    byte_budget: u64,
) {
    let mut total: u64 = total_resident_bytes(entries);
    while total > byte_budget {
        match entries.pop_back() {
            Some(evicted) => total = total.saturating_sub(evicted.resident_bytes),
            None => break,
        }
    }
}

pub(crate) fn build_direct_occt_manifest(
    model_id: &str,
    source_path: &Path,
    parts: &[(String, String)],
    parameter_keys: &[String],
    selector_tags: &[CoreSelectorTagDecl],
    topology_report: Option<&DirectOcctTopologyReport>,
    part_root_node_ids: &HashMap<String, u64>,
) -> AppResult<ModelManifest> {
    let part_stable_node_keys = HashMap::new();
    let part_asset_paths = HashMap::new();
    build_direct_occt_manifest_with_stable_node_keys(
        model_id,
        source_path,
        parts,
        parameter_keys,
        selector_tags,
        topology_report,
        &part_stable_node_keys,
        part_root_node_ids,
        &part_asset_paths,
        &HashMap::new(),
    )
}

pub(crate) fn build_direct_occt_manifest_with_stable_node_keys(
    model_id: &str,
    source_path: &Path,
    parts: &[(String, String)],
    parameter_keys: &[String],
    selector_tags: &[CoreSelectorTagDecl],
    topology_report: Option<&DirectOcctTopologyReport>,
    part_stable_node_keys: &HashMap<String, String>,
    part_root_node_ids: &HashMap<String, u64>,
    part_asset_paths: &HashMap<String, String>,
    part_bounds: &HashMap<String, ManifestBounds>,
) -> AppResult<ModelManifest> {
    let fallback_provenance = parts
        .iter()
        .map(|(part_id, _)| {
            (
                part_id.clone(),
                DirectOcctPartProvenance {
                    parameter_keys: parameter_keys.to_vec(),
                    named_shapes: Vec::new(),
                    faceted_mesh_root: false,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    build_direct_occt_manifest_with_program_provenance(
        model_id,
        source_path,
        parts,
        parameter_keys,
        selector_tags,
        topology_report,
        part_stable_node_keys,
        part_root_node_ids,
        part_asset_paths,
        part_bounds,
        &fallback_provenance,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_direct_occt_manifest_with_program_provenance(
    model_id: &str,
    source_path: &Path,
    parts: &[(String, String)],
    parameter_keys: &[String],
    selector_tags: &[CoreSelectorTagDecl],
    topology_report: Option<&DirectOcctTopologyReport>,
    part_stable_node_keys: &HashMap<String, String>,
    part_root_node_ids: &HashMap<String, u64>,
    part_asset_paths: &HashMap<String, String>,
    part_bounds: &HashMap<String, ManifestBounds>,
    program_provenance: &BTreeMap<String, DirectOcctPartProvenance>,
) -> AppResult<ModelManifest> {
    let part_bindings = direct_occt_part_bindings_with_provenance(
        parts,
        program_provenance,
        part_asset_paths,
        part_bounds,
    );
    let part_ids = part_bindings
        .iter()
        .map(|part| part.part_id.clone())
        .collect::<Vec<_>>();
    let selection_targets = direct_occt_selection_targets(
        &part_bindings,
        topology_report,
        part_stable_node_keys,
        part_root_node_ids,
        program_provenance,
    )?;
    let tagged_anchor_edge_targets =
        direct_occt_tagged_anchor_edge_targets(topology_report, &selection_targets);
    let tagged_anchor_face_targets =
        direct_occt_tagged_anchor_face_targets(topology_report, &selection_targets);
    let authored_binding_target_ids = topology_report
        .map(direct_occt_authored_face_binding_target_ids_from_report)
        .transpose()?
        .unwrap_or_default();
    let tagged_anchors = resolve_tagged_anchors_with_authored_bindings(
        selector_tags,
        &selection_targets,
        &tagged_anchor_edge_targets,
        &tagged_anchor_face_targets,
        &authored_binding_target_ids,
    )?;

    Ok(ModelManifest {
        geometry_provenance: Some(direct_occt_analytic_brep_provenance()),
        component_import_origins: Vec::new(),
        component_placement_evidence: Vec::new(),
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind: ModelSourceKind::Generated,
        source_digest: None,
        core_digest: None,
        ast_schema_version: None,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        document: DocumentMetadata {
            document_name: "Direct OCCT".to_string(),
            document_label: "Direct OCCT".to_string(),
            source_path: Some(path_to_string(source_path)?),
            object_count: part_bindings.len(),
            warnings: Vec::new(),
        },
        parts: part_bindings,
        parameter_groups: vec![ParameterGroup {
            group_id: "core".to_string(),
            label: "Core".to_string(),
            parameter_keys: parameter_keys.to_vec(),
            part_ids,
            editable: true,
            presentation: Some("primary".to_string()),
            order: Some(0),
        }],
        control_primitives: Vec::new(),
        control_relations: Vec::new(),
        control_views: Vec::new(),
        preview_views: Vec::new(),
        advisories: Vec::new(),
        selection_targets,
        measurement_annotations: Vec::new(),
        tagged_anchors,
        feature_graph: None,
        correspondence_graph: None,
        analysis_declarations: Vec::new(),
        warnings: Vec::new(),
        enrichment_state: ManifestEnrichmentState {
            status: EnrichmentStatus::None,
            proposals: Vec::new(),
        },
    })
}

fn apply_direct_occt_program_provenance(
    manifest: &mut ModelManifest,
    program: &CoreProgram,
    provenance: &BTreeMap<String, DirectOcctPartProvenance>,
) {
    for part in &mut manifest.parts {
        part.parameter_keys = provenance
            .get(&part.part_id)
            .map(|projection| projection.parameter_keys.clone())
            .unwrap_or_default();
    }
    let mut claim_counts = BTreeMap::<String, usize>::new();
    for projection in provenance.values() {
        for key in &projection.parameter_keys {
            *claim_counts.entry(key.clone()).or_default() += 1;
        }
    }
    let all_parameter_keys = program
        .parameters
        .iter()
        .map(|parameter| parameter.key.clone())
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    let model_keys = all_parameter_keys
        .iter()
        .filter(|key| claim_counts.get(*key).copied().unwrap_or_default() != 1)
        .cloned()
        .collect::<Vec<_>>();
    if !model_keys.is_empty() {
        groups.push(ParameterGroup {
            group_id: "model:parameters".to_string(),
            label: "Model Parameters".to_string(),
            parameter_keys: model_keys,
            part_ids: manifest
                .parts
                .iter()
                .map(|part| part.part_id.clone())
                .collect(),
            editable: true,
            presentation: Some("primary".to_string()),
            order: Some(groups.len() as u32),
        });
    }
    let mut nodes = Vec::new();
    for part in &program.parts {
        let Some(projection) = provenance.get(&part.key) else {
            continue;
        };
        let feature = program.feature_decls.get(&part.key);
        let primary_keys = feature
            .filter(|feature| !feature.param_keys.is_empty())
            .map(|feature| feature.param_keys.clone())
            .unwrap_or_else(|| {
                projection
                    .parameter_keys
                    .iter()
                    .filter(|key| claim_counts.get(*key) == Some(&1))
                    .cloned()
                    .collect()
            });
        if !primary_keys.is_empty() {
            groups.push(ParameterGroup {
                group_id: feature
                    .map(|feature| feature.feature_id.clone())
                    .unwrap_or_else(|| format!("part:{}", part.key)),
                label: part.label.clone(),
                parameter_keys: primary_keys,
                part_ids: vec![part.key.clone()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(groups.len() as u32),
            });
        }
        nodes.push(FeatureNode {
            feature_id: feature
                .map(|feature| feature.feature_id.clone())
                .unwrap_or_else(|| format!("part:{}", part.key)),
            kind: feature
                .map(|feature| feature.role.clone())
                .unwrap_or_else(|| "part".to_string()),
            label: part.label.clone(),
            source_ref: Some(SourceRef {
                source_id: None,
                path: Some(format!("/parts/{}/root", part.key)),
                start_byte: part.root.span.map(|span| span.start),
                end_byte: part.root.span.map(|span| span.end),
            }),
            dependency_ids: projection.parameter_keys.clone(),
            output_refs: Vec::new(),
            ports: Vec::new(),
        });
        for (shape_name, parameter_keys) in &projection.named_shapes {
            if !parameter_keys.is_empty() {
                groups.push(ParameterGroup {
                    group_id: format!("shape:{}:{}", part.key, shape_name),
                    label: humanize_direct_occt_name(shape_name),
                    parameter_keys: parameter_keys.clone(),
                    part_ids: vec![part.key.clone()],
                    editable: true,
                    presentation: Some("advanced".to_string()),
                    order: Some(groups.len() as u32),
                });
            }
            nodes.push(FeatureNode {
                feature_id: format!("shape:{}:{}", part.key, shape_name),
                kind: "shape".to_string(),
                label: humanize_direct_occt_name(shape_name),
                source_ref: Some(SourceRef {
                    source_id: None,
                    path: Some(format!("/parts/{}/build/{}", part.key, shape_name)),
                    start_byte: None,
                    end_byte: None,
                }),
                dependency_ids: parameter_keys.clone(),
                output_refs: Vec::new(),
                ports: Vec::new(),
            });
        }
    }
    manifest.parameter_groups = groups;
    manifest.feature_graph = Some(FeatureGraph { nodes });
    manifest.control_views.clear();
    manifest.preview_views = program
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
        .collect();
}

fn humanize_direct_occt_name(name: &str) -> String {
    name.split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn direct_occt_part_bindings(
    parts: &[(String, String)],
    parameter_keys: &[String],
    part_asset_paths: &HashMap<String, String>,
    part_bounds: &HashMap<String, ManifestBounds>,
) -> Vec<PartBinding> {
    let provenance = parts
        .iter()
        .map(|(part_id, _)| {
            (
                part_id.clone(),
                DirectOcctPartProvenance {
                    parameter_keys: parameter_keys.to_vec(),
                    named_shapes: Vec::new(),
                    faceted_mesh_root: false,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    direct_occt_part_bindings_with_provenance(parts, &provenance, part_asset_paths, part_bounds)
}

fn direct_occt_part_bindings_with_provenance(
    parts: &[(String, String)],
    provenance: &BTreeMap<String, DirectOcctPartProvenance>,
    part_asset_paths: &HashMap<String, String>,
    part_bounds: &HashMap<String, ManifestBounds>,
) -> Vec<PartBinding> {
    let specs = if parts.is_empty() {
        vec![("body".to_string(), "Body".to_string())]
    } else {
        parts.to_vec()
    };

    specs
        .into_iter()
        .enumerate()
        .map(|(index, (key, label))| {
            let fallback_id = if index == 0 {
                "body".to_string()
            } else {
                format!("part_{}", index + 1)
            };
            let part_id = if key.trim().is_empty() {
                fallback_id
            } else {
                key
            };
            let label = if label.trim().is_empty() {
                part_id.clone()
            } else {
                label
            };
            // Prefer a per-part STL when the executor wrote one; fall back to the
            // merged preview so single-part and legacy backends keep working.
            let viewer_asset_path = part_asset_paths
                .get(&part_id)
                .cloned()
                .unwrap_or_else(|| MODEL_STL_FILE_NAME.to_string());
            PartBinding {
                part_id: part_id.clone(),
                freecad_object_name: part_id.clone(),
                label,
                kind: "solid".to_string(),
                semantic_role: Some("generated".to_string()),
                viewer_asset_path: Some(viewer_asset_path),
                viewer_node_ids: vec![part_id.clone()],
                parameter_keys: provenance
                    .get(&part_id)
                    .map(|projection| projection.parameter_keys.clone())
                    .unwrap_or_default(),
                editable: true,
                bounds: part_bounds.get(&part_id).cloned(),
                volume: None,
                area: None,
            }
        })
        .collect()
}

fn direct_occt_part_bounds(
    parts: &[(String, String)],
    part_asset_paths: &HashMap<String, String>,
    bundle_dir: &Path,
    model_stl_path: &Path,
) -> HashMap<String, ManifestBounds> {
    let mut bounds_by_part = HashMap::new();
    let fallback_bounds = manifest_bounds_from_stl(model_stl_path);

    for (index, (key, _label)) in parts.iter().enumerate() {
        let fallback_id = if index == 0 {
            "body".to_string()
        } else {
            format!("part_{}", index + 1)
        };
        let part_id = if key.trim().is_empty() {
            fallback_id
        } else {
            key.clone()
        };
        if let Some(part_stl_path) = part_asset_paths.get(&part_id) {
            let abs_part_stl_path = bundle_dir.join(part_stl_path);
            if let Some(bounds) = manifest_bounds_from_stl(&abs_part_stl_path) {
                bounds_by_part.insert(part_id.clone(), bounds);
                continue;
            }
        }
        if let Some(bounds) = fallback_bounds.clone() {
            bounds_by_part.insert(part_id.clone(), bounds);
        }
    }

    bounds_by_part
}

fn manifest_bounds_from_stl(stl_path: &Path) -> Option<ManifestBounds> {
    let mesh = IndexedMeshAsset::from_stl(MeshAssetSource::Imported, stl_path).ok()?;
    let mut vertices = mesh.vertices().iter();
    let [x_min, y_min, z_min] = *vertices.next()?;

    let mut x_min = x_min;
    let mut y_min = y_min;
    let mut z_min = z_min;
    let mut x_max = x_min;
    let mut y_max = y_min;
    let mut z_max = z_min;

    for vertex in vertices {
        x_min = x_min.min(vertex[0]);
        y_min = y_min.min(vertex[1]);
        z_min = z_min.min(vertex[2]);
        x_max = x_max.max(vertex[0]);
        y_max = y_max.max(vertex[1]);
        z_max = z_max.max(vertex[2]);
    }

    Some(ManifestBounds {
        x_min,
        y_min,
        z_min,
        x_max,
        y_max,
        z_max,
    })
}

pub(crate) fn build_direct_occt_bundle(
    model_id: &str,
    content_hash: &str,
    source_path: &Path,
    model_stl_path: &Path,
    step_path: &Path,
    topology_report: Option<&DirectOcctTopologyReport>,
    manifest: &ModelManifest,
) -> AppResult<ArtifactBundle> {
    Ok(ArtifactBundle {
        geometry_provenance: Some(direct_occt_analytic_brep_provenance()),
        component_dependency_lock: None,
        component_dependency_lock_digest: None,
        component_import_origins: Vec::new(),
        component_placement_evidence: Vec::new(),
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash: content_hash.to_string(),
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: MANIFEST_FILE_NAME.to_string(),
        macro_path: Some(path_to_string(source_path)?),
        model_stl_path: path_to_string(model_stl_path)?,
        viewer_assets: Vec::new(),
        edge_targets: direct_occt_edge_targets(topology_report, manifest),
        face_targets: direct_occt_face_targets(topology_report, manifest),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: direct_occt_step_export_artifacts(step_path)?,
    })
}

pub(crate) fn direct_occt_step_export_artifacts(
    step_path: &Path,
) -> AppResult<Vec<ExportArtifact>> {
    Ok(vec![ExportArtifact {
        geometry_provenance: Some(direct_occt_analytic_brep_provenance()),
        label: "STEP".to_string(),
        format: "step".to_string(),
        path: path_to_string(step_path)?,
        role: "primary".to_string(),
    }])
}

#[cfg(test)]
fn content_hash(source_identity: &str, params_json: &str) -> String {
    let program = crate::ecky_scheme::compile_to_core_program(source_identity)
        .expect("test source must compile for runtime cache key");
    let parameters = serde_json::from_str(params_json)
        .expect("test parameter JSON must deserialize for runtime cache key");
    content_hash_with_runtime_inputs(&program, source_identity, params_json, &parameters, None)
        .expect("test runtime cache key")
}

fn content_hash_with_font_path(
    source_identity: &str,
    params_json: &str,
    cad_text_font_path: Option<&str>,
) -> String {
    content_hash_with_backend_version(
        DIRECT_OCCT_CACHE_SCHEMA,
        source_identity,
        params_json,
        cad_text_font_path,
    )
}

/// Content-addressed cache key seeded by the backend cache schema, which is the
/// backend-version component of cache identity. Production always passes
/// [`DIRECT_OCCT_CACHE_SCHEMA`]; the `backend_version` parameter exists so a
/// schema bump can be proven to invalidate the key without adding legacy or
/// version branches to the render path.
fn content_hash_with_backend_version(
    backend_version: &str,
    source_identity: &str,
    params_json: &str,
    cad_text_font_path: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(backend_version.as_bytes());
    hasher.update(b"|");
    hasher.update(source_identity.as_bytes());
    hasher.update(b"|");
    hasher.update(params_json.as_bytes());
    if let Some(cad_text_font_path) = normalized_cad_text_font_path(cad_text_font_path) {
        hasher.update(b"|font|");
        hasher.update(cad_text_font_path.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Hash every input that can affect the produced artifact. Source text alone is
/// insufficient: normalization/expansion can change the executable plan and an
/// `import-stl` command dereferences bytes outside that source text.
fn content_hash_with_runtime_inputs(
    program: &CoreProgram,
    source_identity: &str,
    params_json: &str,
    parameters: &DesignParams,
    cad_text_font_path: Option<&str>,
) -> AppResult<String> {
    const RUNNER_PLAN_SCHEMA_VERSION: &str = "1";
    let plan = super::direct_occt::plan_core_program_with_params(program, parameters)?;
    let mut hasher = Sha256::new();
    hasher.update(content_hash_with_font_path(
        source_identity,
        params_json,
        cad_text_font_path,
    ));
    hasher.update(b"|planned-input-v1|");
    hasher.update(format!("{plan:#?}").as_bytes());
    hasher.update(b"|runner-plan-schema|");
    hasher.update(RUNNER_PLAN_SCHEMA_VERSION.as_bytes());

    for (part_index, part) in plan.parts.iter().enumerate() {
        for (command_index, command) in part.commands.iter().enumerate() {
            if command.op != OcctOp::ImportStl {
                continue;
            }
            let Some(OcctArg::Text(path) | OcctArg::Symbol(path)) = command.args.first() else {
                continue;
            };
            let bytes = fs::read(path).map_err(|err| {
                AppError::validation(format!(
                    "Direct OCCT cache key could not read imported STL '{}': {}",
                    path, err
                ))
            })?;
            hasher.update(b"|import-stl|");
            hasher.update(part_index.to_le_bytes());
            hasher.update(command_index.to_le_bytes());
            hasher.update(path.as_bytes());
            hasher.update(b"|");
            hasher.update(bytes);
            let indexed_sidecar = Path::new(path).with_extension("indexed-mesh.json");
            if indexed_sidecar.is_file() {
                let indexed_bytes = fs::read(&indexed_sidecar).map_err(|err| {
                    AppError::validation(format!(
                        "Direct OCCT cache key could not read indexed mesh '{}': {}",
                        indexed_sidecar.display(),
                        err
                    ))
                })?;
                hasher.update(b"|indexed-mesh|");
                hasher.update(indexed_sidecar.to_string_lossy().as_bytes());
                hasher.update(b"|");
                hasher.update(indexed_bytes);
            }
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn direct_occt_geometry_hash(
    program: &CoreProgram,
    params_json: &str,
    parameters: &DesignParams,
    cad_text_font_path: Option<&str>,
    app: &dyn PathResolver,
) -> AppResult<String> {
    let plan = super::direct_occt::plan_core_program_with_params(program, parameters)?;
    let runner_path = super::direct_occt_runner::discover_direct_occt_runner_with_mode(app, true)
        .ok_or_else(|| {
        AppError::render("Direct OCCT runner unavailable for geometry cache identity.")
    })?;
    let runner_bytes = fs::read(&runner_path).map_err(|err| {
        AppError::persistence(format!(
            "Cannot read Direct OCCT runner '{}' for geometry cache identity: {}",
            runner_path.display(),
            err
        ))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(DIRECT_OCCT_GEOMETRY_CACHE_SCHEMA.as_bytes());
    hasher.update(b"|plan|");
    hasher.update(format!("{plan:#?}").as_bytes());
    hasher.update(b"|params|");
    hasher.update(params_json.as_bytes());
    hasher.update(b"|runner|");
    hasher.update(runner_bytes);
    if let Some(cad_text_font_path) = normalized_cad_text_font_path(cad_text_font_path) {
        hasher.update(b"|font|");
        hasher.update(cad_text_font_path.as_bytes());
    }
    hash_direct_occt_import_payloads(&mut hasher, &plan)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_direct_occt_import_payloads(
    hasher: &mut Sha256,
    plan: &super::direct_occt::OcctPlan,
) -> AppResult<()> {
    for (part_index, part) in plan.parts.iter().enumerate() {
        for (command_index, command) in part.commands.iter().enumerate() {
            if command.op != OcctOp::ImportStl {
                continue;
            }
            let Some(OcctArg::Text(path) | OcctArg::Symbol(path)) = command.args.first() else {
                continue;
            };
            let bytes = fs::read(path).map_err(|err| {
                AppError::validation(format!(
                    "Direct OCCT geometry cache key could not read imported STL '{}': {}",
                    path, err
                ))
            })?;
            hasher.update(b"|import-stl|");
            hasher.update(part_index.to_le_bytes());
            hasher.update(command_index.to_le_bytes());
            hasher.update(path.as_bytes());
            hasher.update(b"|");
            hasher.update(bytes);
            let indexed_sidecar = Path::new(path).with_extension("indexed-mesh.json");
            if indexed_sidecar.is_file() {
                let indexed_bytes = fs::read(&indexed_sidecar).map_err(|err| {
                    AppError::validation(format!(
                        "Direct OCCT geometry cache key could not read indexed mesh '{}': {}",
                        indexed_sidecar.display(),
                        err
                    ))
                })?;
                hasher.update(b"|indexed-mesh|");
                hasher.update(indexed_sidecar.to_string_lossy().as_bytes());
                hasher.update(b"|");
                hasher.update(indexed_bytes);
            }
        }
    }
    Ok(())
}

fn direct_occt_geometry_cache_dir(
    app: &dyn PathResolver,
    geometry_hash: &str,
) -> AppResult<PathBuf> {
    Ok(crate::model_runtime::runtime_root(app)?
        .join(DIRECT_OCCT_GEOMETRY_CACHE_DIR)
        .join(geometry_hash))
}

fn direct_occt_geometry_cache_digest(path: &Path) -> AppResult<String> {
    let bytes = fs::read(path).map_err(|err| {
        AppError::persistence(format!(
            "Cannot read Direct OCCT geometry cache artifact '{}': {}",
            path.display(),
            err
        ))
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn copy_direct_occt_geometry_cache_artifact(
    source: &Path,
    target: &Path,
    relative_name: &str,
    digests: &mut BTreeMap<String, String>,
) -> AppResult<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| AppError::persistence(err.to_string()))?;
    }
    fs::copy(source, target).map_err(|err| {
        AppError::persistence(format!(
            "Cannot cache Direct OCCT geometry artifact '{}' as '{}': {}",
            source.display(),
            target.display(),
            err
        ))
    })?;
    digests.insert(
        relative_name.to_string(),
        direct_occt_geometry_cache_digest(target)?,
    );
    Ok(())
}

fn validated_direct_occt_geometry_cache_manifest(
    cache_dir: &Path,
) -> Option<DirectOcctGeometryCacheManifest> {
    let raw = fs::read_to_string(cache_dir.join(DIRECT_OCCT_GEOMETRY_CACHE_FILE)).ok()?;
    let manifest = serde_json::from_str::<DirectOcctGeometryCacheManifest>(&raw).ok()?;
    if manifest.schema != DIRECT_OCCT_GEOMETRY_CACHE_SCHEMA
        || !manifest.digests.contains_key(MODEL_STL_FILE_NAME)
        || !manifest.digests.contains_key(TOPOLOGY_FILE_NAME)
        || (manifest.has_step && !manifest.digests.contains_key(STEP_FILE_NAME))
    {
        return None;
    }
    for (relative_name, expected_digest) in &manifest.digests {
        let actual_digest =
            direct_occt_geometry_cache_digest(&cache_dir.join(relative_name)).ok()?;
        if &actual_digest != expected_digest {
            return None;
        }
    }
    read_direct_occt_topology_report(&cache_dir.join(TOPOLOGY_FILE_NAME)).ok()?;
    Some(manifest)
}

fn write_cached_direct_occt_geometry(
    app: &dyn PathResolver,
    geometry_hash: &str,
    bundle_dir: &Path,
    outcome: &NativeExportOutcome,
) -> AppResult<()> {
    let (step_path, stl_path, part_stl_paths, tessellated_step, source_mesh_digests) = match outcome
    {
        NativeExportOutcome::Exported {
            step_path,
            stl_path,
            part_stl_paths,
            tessellated_step,
            source_mesh_digests,
        } => (
            Some(step_path),
            stl_path,
            part_stl_paths,
            *tessellated_step,
            source_mesh_digests,
        ),
        NativeExportOutcome::MeshExported {
            stl_path,
            part_stl_paths,
            source_mesh_digests,
        } => (None, stl_path, part_stl_paths, false, source_mesh_digests),
        NativeExportOutcome::Blocked { .. } => return Ok(()),
    };
    let topology_path = bundle_dir.join(TOPOLOGY_FILE_NAME);
    read_direct_occt_topology_report(&topology_path)?;
    let cache_dir = direct_occt_geometry_cache_dir(app, geometry_hash)?;
    if validated_direct_occt_geometry_cache_manifest(&cache_dir).is_some() {
        return Ok(());
    }
    let parent = cache_dir
        .parent()
        .ok_or_else(|| AppError::persistence("Direct OCCT geometry cache has no parent."))?;
    fs::create_dir_all(parent).map_err(|err| AppError::persistence(err.to_string()))?;
    let staging = parent.join(format!("{geometry_hash}.tmp-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&staging).map_err(|err| AppError::persistence(err.to_string()))?;
    let write_result = (|| {
        let mut digests = BTreeMap::new();
        copy_direct_occt_geometry_cache_artifact(
            stl_path,
            &staging.join(MODEL_STL_FILE_NAME),
            MODEL_STL_FILE_NAME,
            &mut digests,
        )?;
        copy_direct_occt_geometry_cache_artifact(
            &topology_path,
            &staging.join(TOPOLOGY_FILE_NAME),
            TOPOLOGY_FILE_NAME,
            &mut digests,
        )?;
        if let Some(step_path) = step_path {
            copy_direct_occt_geometry_cache_artifact(
                step_path,
                &staging.join(STEP_FILE_NAME),
                STEP_FILE_NAME,
                &mut digests,
            )?;
        }
        let mut part_assets = Vec::new();
        for (index, (part_key, path)) in part_stl_paths.iter().enumerate() {
            let file_name = format!("{index}.stl");
            let relative_name = format!("parts/{file_name}");
            copy_direct_occt_geometry_cache_artifact(
                path,
                &staging.join(&relative_name),
                &relative_name,
                &mut digests,
            )?;
            part_assets.push(DirectOcctGeometryCachePartAsset {
                part_key: part_key.clone(),
                file_name,
            });
        }
        let manifest = DirectOcctGeometryCacheManifest {
            schema: DIRECT_OCCT_GEOMETRY_CACHE_SCHEMA.to_string(),
            has_step: step_path.is_some(),
            tessellated_step,
            source_mesh_digests: source_mesh_digests.clone(),
            part_assets,
            digests,
        };
        let data = serde_json::to_string_pretty(&manifest)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        fs::write(staging.join(DIRECT_OCCT_GEOMETRY_CACHE_FILE), data)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        if cache_dir.exists() && validated_direct_occt_geometry_cache_manifest(&cache_dir).is_none()
        {
            fs::remove_dir_all(&cache_dir).map_err(|err| {
                AppError::persistence(format!(
                    "Cannot replace corrupt Direct OCCT geometry cache '{}': {}",
                    cache_dir.display(),
                    err
                ))
            })?;
        }
        match fs::rename(&staging, &cache_dir) {
            Ok(()) => Ok(()),
            Err(_) if validated_direct_occt_geometry_cache_manifest(&cache_dir).is_some() => Ok(()),
            Err(err) => Err(AppError::persistence(format!(
                "Cannot publish Direct OCCT geometry cache '{}': {}",
                cache_dir.display(),
                err
            ))),
        }
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    write_result
}

fn read_cached_direct_occt_geometry(
    app: &dyn PathResolver,
    geometry_hash: &str,
    bundle_dir: &Path,
) -> AppResult<Option<NativeExportOutcome>> {
    let cache_dir = direct_occt_geometry_cache_dir(app, geometry_hash)?;
    let Some(manifest) = validated_direct_occt_geometry_cache_manifest(&cache_dir) else {
        return Ok(None);
    };
    fs::copy(
        cache_dir.join(MODEL_STL_FILE_NAME),
        bundle_dir.join(MODEL_STL_FILE_NAME),
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::copy(
        cache_dir.join(TOPOLOGY_FILE_NAME),
        bundle_dir.join(TOPOLOGY_FILE_NAME),
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;
    let mut part_stl_paths = Vec::new();
    for (index, part) in manifest.part_assets.iter().enumerate() {
        let destination = bundle_dir.join("parts").join(format!("{index}.stl"));
        fs::create_dir_all(destination.parent().expect("part cache parent"))
            .map_err(|err| AppError::persistence(err.to_string()))?;
        fs::copy(cache_dir.join("parts").join(&part.file_name), &destination)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        part_stl_paths.push((part.part_key.clone(), destination));
    }
    let stl_path = bundle_dir.join(MODEL_STL_FILE_NAME);
    if manifest.has_step {
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::copy(cache_dir.join(STEP_FILE_NAME), &step_path)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        Ok(Some(NativeExportOutcome::Exported {
            step_path,
            stl_path,
            part_stl_paths,
            tessellated_step: manifest.tessellated_step,
            source_mesh_digests: manifest.source_mesh_digests,
        }))
    } else {
        Ok(Some(NativeExportOutcome::MeshExported {
            stl_path,
            part_stl_paths,
            source_mesh_digests: manifest.source_mesh_digests,
        }))
    }
}

fn normalized_cad_text_font_path(cad_text_font_path: Option<&str>) -> Option<&str> {
    cad_text_font_path
        .map(str::trim)
        .filter(|cad_text_font_path| !cad_text_font_path.is_empty())
}

fn direct_occt_text_font_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct DirectOcctTextFontEnvGuard {
    previous: Option<OsString>,
}

impl DirectOcctTextFontEnvGuard {
    fn install(cad_text_font_path: &str) -> Self {
        let previous = std::env::var_os(DIRECT_OCCT_TEXT_FONT_ENV);
        unsafe {
            std::env::set_var(DIRECT_OCCT_TEXT_FONT_ENV, cad_text_font_path);
        }
        Self { previous }
    }
}

impl Drop for DirectOcctTextFontEnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(previous) => unsafe {
                std::env::set_var(DIRECT_OCCT_TEXT_FONT_ENV, previous);
            },
            None => unsafe {
                std::env::remove_var(DIRECT_OCCT_TEXT_FONT_ENV);
            },
        }
    }
}

fn with_direct_occt_text_font_path<T>(
    cad_text_font_path: Option<&str>,
    run: impl FnOnce() -> AppResult<T>,
) -> AppResult<T> {
    let Some(cad_text_font_path) = normalized_cad_text_font_path(cad_text_font_path) else {
        return run();
    };
    let _env_lock = direct_occt_text_font_env_lock().lock().unwrap();
    let _env_guard = DirectOcctTextFontEnvGuard::install(cad_text_font_path);
    run()
}

fn direct_occt_source_stable_node_key(source_identity: &str, part: &CorePart) -> Option<String> {
    let span = part.root.span?;
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end
        || end > source_identity.len()
        || !source_identity.is_char_boundary(start)
        || !source_identity.is_char_boundary(end)
    {
        return None;
    }

    let mut hasher = Sha256::new();
    hasher.update(b"direct-occt-part-root|");
    hasher.update(part.key.as_bytes());
    hasher.update(b"|");
    hasher.update(&source_identity.as_bytes()[start..end]);
    Some(format!("sha256:{:x}", hasher.finalize()))
}

fn model_id_from_hash(hash: &str) -> String {
    format!("generated-direct-occt-{}", &hash[..12])
}

fn read_direct_occt_topology_report(path: &Path) -> AppResult<DirectOcctTopologyReport> {
    let contents = fs::read_to_string(path).map_err(|err| {
        AppError::persistence(format!(
            "Direct OCCT topology report could not be read '{}': {}",
            path.display(),
            err
        ))
    })?;
    serde_json::from_str(&contents)
        .map_err(|err| AppError::validation(format!("Direct OCCT topology report invalid: {err}")))
}

pub fn direct_occt_solid_diagnostics(
    bundle: &ArtifactBundle,
) -> AppResult<DirectOcctSolidDiagnostics> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SolidReport {
        #[serde(default)]
        parts: Vec<SolidPart>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SolidPart {
        part_id: String,
        solid_count: Option<u64>,
        brep_valid: Option<bool>,
    }

    let bundle_dir = Path::new(&bundle.manifest_path).parent().ok_or_else(|| {
        AppError::validation("Direct OCCT artifact manifest has no containing directory.")
    })?;
    let topology_path = bundle_dir.join(TOPOLOGY_FILE_NAME);
    let contents = fs::read_to_string(&topology_path).map_err(|error| {
        AppError::persistence(format!(
            "Direct OCCT topology report could not be read '{}': {error}",
            topology_path.display()
        ))
    })?;
    let report: SolidReport = serde_json::from_str(&contents).map_err(|error| {
        AppError::validation(format!(
            "Direct OCCT solid-validity report invalid: {error}"
        ))
    })?;
    if report.parts.is_empty() {
        return Err(AppError::validation(
            "Direct OCCT topology report has no exact BRep parts.",
        ));
    }
    let mut solid_count = 0u64;
    let mut all_breps_valid = true;
    for part in &report.parts {
        let part_id = part.part_id.trim();
        let (Some(part_solid_count), Some(brep_valid)) = (part.solid_count, part.brep_valid) else {
            return Err(AppError::validation(format!(
                "Direct OCCT part '{part_id}' has no exact solid-validity diagnostics."
            )));
        };
        if part_id.is_empty() {
            return Err(AppError::validation(
                "Direct OCCT topology report has an empty part identity.",
            ));
        }
        solid_count = solid_count.checked_add(part_solid_count).ok_or_else(|| {
            AppError::validation("Direct OCCT solid count exceeds the admitted numeric bound.")
        })?;
        all_breps_valid &= brep_valid;
    }
    Ok(DirectOcctSolidDiagnostics {
        part_count: report.parts.len(),
        solid_count,
        all_breps_valid,
    })
}

pub(crate) fn direct_occt_part_source_geometry_digests(
    topology_path: &Path,
) -> AppResult<std::collections::BTreeMap<String, String>> {
    let report = read_direct_occt_topology_report(topology_path)?;
    let mut digests = std::collections::BTreeMap::new();
    for part in report.parts {
        let part_id = part.part_id.trim();
        let digest = part
            .source_geometry_digest
            .as_deref()
            .unwrap_or_default()
            .trim();
        if part_id.is_empty() || !digest.starts_with("sha256:") || digest.len() <= "sha256:".len() {
            return Err(AppError::validation(
                "Direct OCCT topology report part source geometry identity is missing or invalid.",
            ));
        }
        if digests
            .insert(part_id.to_string(), digest.to_string())
            .is_some()
        {
            return Err(AppError::validation(format!(
                "Direct OCCT topology report repeats partId '{part_id}'."
            )));
        }
    }
    if digests.is_empty() {
        return Err(AppError::validation(
            "Direct OCCT topology report has no analytic BRep parts.",
        ));
    }
    Ok(digests)
}

pub(crate) fn validate_direct_occt_guided_expected_solids(topology_path: &Path) -> AppResult<()> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SolidReport {
        #[serde(default)]
        parts: Vec<SolidPart>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SolidPart {
        part_id: String,
        solid_count: Option<u64>,
        brep_valid: Option<bool>,
    }

    let contents = fs::read_to_string(topology_path).map_err(|error| {
        AppError::persistence(format!(
            "Direct OCCT topology report could not be read '{}': {error}",
            topology_path.display()
        ))
    })?;
    let report: SolidReport = serde_json::from_str(&contents).map_err(|error| {
        AppError::validation(format!(
            "Direct OCCT solid-validity report invalid: {error}"
        ))
    })?;
    if report.parts.is_empty() {
        return Err(AppError::validation(
            "Guided reconstruction exact preview has no parts.",
        ));
    }
    for part in report.parts {
        let part_id = part.part_id.trim();
        if part_id.is_empty() {
            return Err(AppError::validation(
                "Guided reconstruction exact preview has empty part identity.",
            ));
        }
        let (Some(solid_count), Some(brep_valid)) = (part.solid_count, part.brep_valid) else {
            return Err(AppError::validation(format!(
                "Guided reconstruction part '{part_id}' has no solid-validity proof from exact OCCT runtime."
            )));
        };
        if solid_count == 0 {
            return Err(AppError::validation(format!(
                "Guided reconstruction part '{part_id}' contains no exact solid; open/surface-only result cannot be committed."
            )));
        }
        if !brep_valid {
            return Err(AppError::validation(format!(
                "Guided reconstruction part '{part_id}' failed exact BRep validity check."
            )));
        }
    }
    Ok(())
}

pub(crate) fn direct_occt_exact_target_geometries(
    topology_path: &Path,
) -> AppResult<
    std::collections::BTreeMap<String, crate::capture_brep_validation::ExactBrepTargetGeometry>,
> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ExactReport {
        #[serde(default)]
        parts: Vec<ExactPart>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ExactPart {
        #[serde(default)]
        vertices: Vec<ExactTarget>,
        #[serde(default)]
        edges: Vec<ExactTarget>,
        #[serde(default)]
        faces: Vec<ExactTarget>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ExactTarget {
        #[serde(default)]
        target_id: String,
        #[serde(default)]
        exact_geometry: Option<RawExactGeometry>,
    }
    #[derive(Deserialize)]
    #[serde(
        tag = "kind",
        rename_all = "camelCase",
        rename_all_fields = "camelCase"
    )]
    enum RawExactGeometry {
        Vertex {
            point: DirectOcctTopologyPoint,
        },
        LineEdge {
            start: DirectOcctTopologyPoint,
            end: DirectOcctTopologyPoint,
        },
        CircleEdge {
            center: DirectOcctTopologyPoint,
            normal: [f64; 3],
            x_direction: [f64; 3],
            radius: f64,
            first_parameter: f64,
            last_parameter: f64,
        },
        PlaneFace {
            origin: DirectOcctTopologyPoint,
            normal: [f64; 3],
            #[serde(default)]
            boundary_edge_target_ids: Vec<Vec<String>>,
        },
        CylinderFace {
            axis_origin: DirectOcctTopologyPoint,
            axis_direction: [f64; 3],
            radius: f64,
            #[serde(default)]
            boundary_edge_target_ids: Vec<Vec<String>>,
        },
    }
    fn point(value: DirectOcctTopologyPoint) -> [f64; 3] {
        [value.x, value.y, value.z]
    }
    let contents = fs::read_to_string(topology_path).map_err(|error| {
        AppError::persistence(format!(
            "Direct OCCT topology report could not be read '{}': {error}",
            topology_path.display()
        ))
    })?;
    let report: ExactReport = serde_json::from_str(&contents).map_err(|error| {
        AppError::validation(format!(
            "Direct OCCT exact topology report invalid: {error}"
        ))
    })?;
    let mut result = std::collections::BTreeMap::new();
    for target in report.parts.into_iter().flat_map(|part| {
        part.vertices
            .into_iter()
            .chain(part.edges)
            .chain(part.faces)
    }) {
        let Some(raw) = target.exact_geometry else {
            continue;
        };
        let target_id = target.target_id.trim();
        if target_id.is_empty() {
            return Err(AppError::validation(
                "Direct OCCT exact target geometry has no targetId.",
            ));
        }
        let geometry = match raw {
            RawExactGeometry::Vertex { point: value } => {
                crate::capture_brep_validation::ExactBrepTargetGeometry::Vertex {
                    point: point(value),
                }
            }
            RawExactGeometry::LineEdge { start, end } => {
                crate::capture_brep_validation::ExactBrepTargetGeometry::LineEdge {
                    start: point(start),
                    end: point(end),
                }
            }
            RawExactGeometry::CircleEdge {
                center,
                normal,
                x_direction,
                radius,
                first_parameter,
                last_parameter,
            } => crate::capture_brep_validation::ExactBrepTargetGeometry::CircleEdge {
                center: point(center),
                normal,
                x_direction,
                radius,
                first_parameter,
                last_parameter,
            },
            RawExactGeometry::PlaneFace {
                origin,
                normal,
                boundary_edge_target_ids,
            } => crate::capture_brep_validation::ExactBrepTargetGeometry::PlaneFace {
                origin: point(origin),
                normal,
                boundary_edge_target_ids,
            },
            RawExactGeometry::CylinderFace {
                axis_origin,
                axis_direction,
                radius,
                boundary_edge_target_ids,
            } => crate::capture_brep_validation::ExactBrepTargetGeometry::CylinderFace {
                axis_origin: point(axis_origin),
                axis_direction,
                radius,
                boundary_edge_target_ids,
            },
        };
        if result.insert(target_id.to_string(), geometry).is_some() {
            return Err(AppError::validation(format!(
                "Direct OCCT exact topology repeats targetId '{target_id}'."
            )));
        }
    }
    Ok(result)
}

pub(crate) fn direct_occt_authored_binding_target_ids(
    topology_path: &Path,
) -> AppResult<std::collections::BTreeMap<(String, String), Vec<String>>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BindingReport {
        #[serde(default)]
        parts: Vec<BindingPart>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BindingPart {
        part_id: String,
        #[serde(default)]
        vertices: Vec<BindingTarget>,
        #[serde(default)]
        edges: Vec<BindingTarget>,
        #[serde(default)]
        faces: Vec<BindingTarget>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BindingTarget {
        target_id: String,
        #[serde(default)]
        authored_bindings: Vec<String>,
    }

    let contents = fs::read_to_string(topology_path).map_err(|error| {
        AppError::persistence(format!(
            "Direct OCCT topology report could not be read '{}': {error}",
            topology_path.display()
        ))
    })?;
    let report: BindingReport = serde_json::from_str(&contents).map_err(|error| {
        AppError::validation(format!(
            "Direct OCCT authored binding topology report invalid: {error}"
        ))
    })?;
    let mut result = std::collections::BTreeMap::<(String, String), Vec<String>>::new();
    for part in report.parts {
        let part_id = part.part_id.trim();
        if part_id.is_empty() {
            return Err(AppError::validation(
                "Direct OCCT authored binding topology has empty partId.",
            ));
        }
        for target in part
            .vertices
            .into_iter()
            .chain(part.edges)
            .chain(part.faces)
        {
            let target_id = target.target_id.trim();
            if target_id.is_empty() && !target.authored_bindings.is_empty() {
                return Err(AppError::validation(
                    "Direct OCCT authored binding target has empty targetId.",
                ));
            }
            for binding in target.authored_bindings {
                let binding = binding.trim();
                if binding.is_empty() {
                    return Err(AppError::validation(
                        "Direct OCCT authored binding name is empty.",
                    ));
                }
                let targets = result
                    .entry((part_id.to_string(), binding.to_string()))
                    .or_default();
                if !targets.iter().any(|existing| existing == target_id) {
                    targets.push(target_id.to_string());
                }
            }
        }
    }
    Ok(result)
}

fn direct_occt_authored_face_binding_target_ids_from_report(
    topology_report: &DirectOcctTopologyReport,
) -> AppResult<BTreeMap<(String, String), Vec<String>>> {
    let mut result = BTreeMap::<(String, String), Vec<String>>::new();
    for part in &topology_report.parts {
        let part_id = part.part_id.trim();
        if part_id.is_empty() {
            return Err(AppError::validation(
                "Direct OCCT authored binding topology has empty partId.",
            ));
        }
        for target in part.faces.iter().map(|target| {
            (
                direct_occt_face_target_id(part_id, target),
                target.authored_bindings.as_slice(),
            )
        }) {
            for binding in target.1 {
                let binding = binding.trim();
                if binding.is_empty() {
                    return Err(AppError::validation(
                        "Direct OCCT authored binding name is empty.",
                    ));
                }
                let targets = result
                    .entry((part_id.to_string(), binding.to_string()))
                    .or_default();
                if !targets.iter().any(|existing| existing == &target.0) {
                    targets.push(target.0.clone());
                }
            }
        }
    }
    Ok(result)
}

pub(crate) fn direct_occt_authored_binding_ordered_edge_target_ids(
    topology_path: &Path,
) -> AppResult<std::collections::BTreeMap<(String, String), Vec<String>>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct OrderedBindingReport {
        #[serde(default)]
        parts: Vec<OrderedBindingPart>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct OrderedBindingPart {
        part_id: String,
        #[serde(default)]
        authored_binding_edge_order: Vec<OrderedBindingEntry>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct OrderedBindingEntry {
        name: String,
        target_ids: Vec<String>,
    }

    let contents = fs::read_to_string(topology_path).map_err(|error| {
        AppError::persistence(format!(
            "Direct OCCT topology report could not be read '{}': {error}",
            topology_path.display()
        ))
    })?;
    let report: OrderedBindingReport = serde_json::from_str(&contents).map_err(|error| {
        AppError::validation(format!(
            "Direct OCCT authored binding edge order report invalid: {error}"
        ))
    })?;
    let mut result = std::collections::BTreeMap::new();
    for part in report.parts {
        let part_id = part.part_id.trim();
        if part_id.is_empty() {
            return Err(AppError::validation(
                "Direct OCCT authored binding edge order has empty partId.",
            ));
        }
        for entry in part.authored_binding_edge_order {
            let name = entry.name.trim();
            if name.is_empty() || entry.target_ids.is_empty() {
                return Err(AppError::validation(
                    "Direct OCCT authored binding edge order is empty.",
                ));
            }
            let mut target_ids = Vec::with_capacity(entry.target_ids.len());
            for target_id in entry.target_ids {
                let target_id = target_id.trim();
                if target_id.is_empty() || target_ids.iter().any(|existing| existing == target_id) {
                    return Err(AppError::validation(format!(
                        "Direct OCCT authored binding '{name}' edge order contains empty or duplicate target identity."
                    )));
                }
                target_ids.push(target_id.to_string());
            }
            if result
                .insert((part_id.to_string(), name.to_string()), target_ids)
                .is_some()
            {
                return Err(AppError::validation(format!(
                    "Direct OCCT authored binding edge order repeats '{part_id}/{name}'."
                )));
            }
        }
    }
    Ok(result)
}

fn direct_occt_selection_targets(
    part_bindings: &[PartBinding],
    topology_report: Option<&DirectOcctTopologyReport>,
    part_stable_node_keys: &HashMap<String, String>,
    part_root_node_ids: &HashMap<String, u64>,
    program_provenance: &BTreeMap<String, DirectOcctPartProvenance>,
) -> AppResult<Vec<SelectionTarget>> {
    let Some(topology_report) = topology_report else {
        return Ok(Vec::new());
    };
    let mut selection_targets = Vec::new();
    let mut seen_canonical_target_ids = std::collections::HashSet::new();
    let mut seen_public_target_ids = std::collections::HashSet::new();
    let mut seen_manifest_ids = std::collections::HashSet::new();

    for topology_part in &topology_report.parts {
        let part_id = topology_part.part_id.trim();
        if program_provenance
            .get(part_id)
            .is_some_and(|provenance| provenance.faceted_mesh_root)
        {
            continue;
        }
        let Some(part_binding) = part_bindings.iter().find(|part| part.part_id == part_id) else {
            return Err(AppError::validation(format!(
                "Direct OCCT topology report references unknown partId '{}'.",
                topology_part.part_id
            )));
        };
        let viewer_node_id = part_binding
            .viewer_node_ids
            .first()
            .cloned()
            .unwrap_or_else(|| part_binding.part_id.clone());
        for vertex in topology_part
            .vertices
            .iter()
            .filter(|vertex| vertex.point.is_some())
        {
            let topology_parameter_keys = direct_occt_topology_target_parameter_keys(
                part_id,
                &vertex.authored_bindings,
                program_provenance,
            );
            let canonical_target_id = direct_occt_vertex_target_id(part_id, vertex);
            if !seen_canonical_target_ids.insert(canonical_target_id.clone()) {
                continue;
            }
            let stable_target_id = stable_vertex_target_id(&canonical_target_id);
            let preferred_public_target_id = if stable_target_id.is_empty() {
                canonical_target_id.clone()
            } else {
                stable_target_id
            };
            let mut public_target_id =
                if seen_public_target_ids.insert(preferred_public_target_id.clone()) {
                    preferred_public_target_id
                } else {
                    canonical_target_id.clone()
                };
            if !seen_manifest_ids.insert(public_target_id.clone()) {
                if public_target_id != canonical_target_id
                    && seen_manifest_ids.insert(canonical_target_id.clone())
                {
                    public_target_id = canonical_target_id.clone();
                } else {
                    continue;
                }
            }
            let durable_target_id = direct_occt_durable_vertex_target_id(
                part_id,
                part_stable_node_keys.get(part_id).map(String::as_str),
                part_root_node_ids.get(part_id).copied(),
                &public_target_id,
            )
            .filter(|durable_target_id| seen_manifest_ids.insert(durable_target_id.clone()));
            let canonical_target_id_value = if canonical_target_id != public_target_id
                && seen_manifest_ids.insert(canonical_target_id.clone())
            {
                Some(canonical_target_id.clone())
            } else {
                None
            };
            selection_targets.push(SelectionTarget {
                target_id: Some(public_target_id.clone()),
                durable_target_id,
                canonical_target_id: canonical_target_id_value.clone(),
                alias_ids: canonical_target_id_value
                    .clone()
                    .map(|canonical_target_id| {
                        topology_target_aliases(&public_target_id, canonical_target_id)
                    })
                    .unwrap_or_default(),
                part_id: part_binding.part_id.clone(),
                viewer_node_id: viewer_node_id.clone(),
                label: direct_occt_vertex_label(topology_part, vertex),
                kind: SelectionTargetKind::Vertex,
                editable: !topology_parameter_keys.is_empty(),
                parameter_keys: topology_parameter_keys,
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            });
        }

        for edge in topology_part
            .edges
            .iter()
            .filter(|edge| edge.start.is_some() && edge.end.is_some())
        {
            let topology_parameter_keys = direct_occt_topology_target_parameter_keys(
                part_id,
                &edge.authored_bindings,
                program_provenance,
            );
            let canonical_target_id = direct_occt_edge_target_id(part_id, edge);
            if !seen_canonical_target_ids.insert(canonical_target_id.clone()) {
                continue;
            }
            let stable_target_id = direct_occt_stable_edge_target_id(&canonical_target_id);
            let preferred_public_target_id = if stable_target_id.is_empty() {
                canonical_target_id.clone()
            } else {
                stable_target_id
            };
            let mut public_target_id =
                if seen_public_target_ids.insert(preferred_public_target_id.clone()) {
                    preferred_public_target_id
                } else {
                    canonical_target_id.clone()
                };
            if !seen_manifest_ids.insert(public_target_id.clone()) {
                if public_target_id != canonical_target_id
                    && seen_manifest_ids.insert(canonical_target_id.clone())
                {
                    public_target_id = canonical_target_id.clone();
                } else {
                    continue;
                }
            }
            let durable_target_id = direct_occt_durable_edge_target_id(
                part_id,
                part_stable_node_keys.get(part_id).map(String::as_str),
                part_root_node_ids.get(part_id).copied(),
                &public_target_id,
            )
            .filter(|durable_target_id| seen_manifest_ids.insert(durable_target_id.clone()));
            let canonical_target_id_value = if canonical_target_id != public_target_id
                && seen_manifest_ids.insert(canonical_target_id.clone())
            {
                Some(canonical_target_id.clone())
            } else {
                None
            };
            selection_targets.push(SelectionTarget {
                target_id: Some(public_target_id.clone()),
                durable_target_id,
                canonical_target_id: canonical_target_id_value.clone(),
                alias_ids: canonical_target_id_value
                    .clone()
                    .map(|canonical_target_id| {
                        topology_target_aliases(&public_target_id, canonical_target_id)
                    })
                    .unwrap_or_default(),
                part_id: part_binding.part_id.clone(),
                viewer_node_id: viewer_node_id.clone(),
                label: direct_occt_edge_label(topology_part, edge),
                kind: SelectionTargetKind::Edge,
                editable: !topology_parameter_keys.is_empty(),
                parameter_keys: topology_parameter_keys.clone(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            });
        }

        for face in topology_part
            .faces
            .iter()
            .filter(|face| face.center.is_some())
        {
            let topology_parameter_keys = direct_occt_topology_target_parameter_keys(
                part_id,
                &face.authored_bindings,
                program_provenance,
            );
            let canonical_target_id = direct_occt_face_target_id(part_id, face);
            if !seen_canonical_target_ids.insert(canonical_target_id.clone()) {
                continue;
            }
            let stable_target_id = direct_occt_stable_face_target_id(&canonical_target_id);
            let preferred_public_target_id = if stable_target_id.is_empty() {
                canonical_target_id.clone()
            } else {
                stable_target_id
            };
            let mut public_target_id =
                if seen_public_target_ids.insert(preferred_public_target_id.clone()) {
                    preferred_public_target_id
                } else {
                    canonical_target_id.clone()
                };
            if !seen_manifest_ids.insert(public_target_id.clone()) {
                if public_target_id != canonical_target_id
                    && seen_manifest_ids.insert(canonical_target_id.clone())
                {
                    public_target_id = canonical_target_id.clone();
                } else {
                    continue;
                }
            }
            let durable_target_id = direct_occt_durable_face_target_id(
                part_id,
                part_stable_node_keys.get(part_id).map(String::as_str),
                part_root_node_ids.get(part_id).copied(),
                &public_target_id,
            )
            .filter(|durable_target_id| seen_manifest_ids.insert(durable_target_id.clone()));
            let canonical_target_id_value = if canonical_target_id != public_target_id
                && seen_manifest_ids.insert(canonical_target_id.clone())
            {
                Some(canonical_target_id.clone())
            } else {
                None
            };
            selection_targets.push(SelectionTarget {
                target_id: Some(public_target_id.clone()),
                durable_target_id,
                canonical_target_id: canonical_target_id_value.clone(),
                alias_ids: canonical_target_id_value
                    .clone()
                    .map(|canonical_target_id| {
                        topology_target_aliases(&public_target_id, canonical_target_id)
                    })
                    .unwrap_or_default(),
                part_id: part_binding.part_id.clone(),
                viewer_node_id: viewer_node_id.clone(),
                label: direct_occt_face_label(topology_part, face),
                kind: SelectionTargetKind::Face,
                editable: !topology_parameter_keys.is_empty(),
                parameter_keys: topology_parameter_keys.clone(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            });
        }
    }

    Ok(selection_targets)
}

fn direct_occt_topology_target_parameter_keys(
    part_id: &str,
    authored_bindings: &[String],
    program_provenance: &BTreeMap<String, DirectOcctPartProvenance>,
) -> Vec<String> {
    let Some(part) = program_provenance.get(part_id) else {
        return Vec::new();
    };
    let bindings = authored_bindings
        .iter()
        .map(|binding| binding.trim())
        .filter(|binding| !binding.is_empty())
        .collect::<BTreeSet<_>>();
    let mut keys = BTreeSet::new();
    for (shape_name, shape_keys) in &part.named_shapes {
        if bindings.contains(shape_name.as_str()) {
            keys.extend(shape_keys.iter().cloned());
        }
    }
    keys.into_iter().collect()
}

fn direct_occt_tagged_anchor_edge_targets(
    topology_report: Option<&DirectOcctTopologyReport>,
    selection_targets: &[SelectionTarget],
) -> Vec<ViewerEdgeTarget> {
    let Some(topology_report) = topology_report else {
        return Vec::new();
    };
    let selection_targets_by_id = selection_targets
        .iter()
        .filter(|target| target.kind == SelectionTargetKind::Edge)
        .flat_map(|target| {
            target
                .target_id
                .iter()
                .map(String::as_str)
                .chain(target.durable_target_id.iter().map(String::as_str))
                .chain(target.canonical_target_id.iter().map(String::as_str))
                .chain(target.alias_ids.iter().map(String::as_str))
                .map(move |target_id| (target_id, target))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut edge_targets = Vec::new();

    for topology_part in &topology_report.parts {
        let part_id = topology_part.part_id.trim();
        for edge in topology_part
            .edges
            .iter()
            .filter(|edge| edge.start.is_some() && edge.end.is_some())
        {
            let target_id = direct_occt_edge_target_id(part_id, edge);
            let Some(selection_target) = selection_targets_by_id.get(target_id.as_str()) else {
                continue;
            };
            let Some(start) = edge.start.as_ref() else {
                continue;
            };
            let Some(end) = edge.end.as_ref() else {
                continue;
            };

            edge_targets.push(ViewerEdgeTarget {
                target_id: preferred_public_topology_target_id(selection_target, &target_id),
                durable_target_id: selection_target.durable_target_id.clone(),
                canonical_target_id: Some(target_id.clone()),
                alias_ids: viewer_target_alias_ids(selection_target, &target_id),
                part_id: selection_target.part_id.clone(),
                viewer_node_id: selection_target.viewer_node_id.clone(),
                label: direct_occt_edge_label(topology_part, edge),
                editable: selection_target.editable,
                start: direct_occt_point_to_viewer(start),
                end: direct_occt_point_to_viewer(end),
            });
        }
    }

    edge_targets
}

fn direct_occt_tagged_anchor_face_targets(
    topology_report: Option<&DirectOcctTopologyReport>,
    selection_targets: &[SelectionTarget],
) -> Vec<ViewerFaceTarget> {
    let Some(topology_report) = topology_report else {
        return Vec::new();
    };
    let selection_targets_by_id = selection_targets
        .iter()
        .filter(|target| target.kind == SelectionTargetKind::Face)
        .flat_map(|target| {
            target
                .target_id
                .iter()
                .map(String::as_str)
                .chain(target.durable_target_id.iter().map(String::as_str))
                .chain(target.canonical_target_id.iter().map(String::as_str))
                .chain(target.alias_ids.iter().map(String::as_str))
                .map(move |target_id| (target_id, target))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut face_targets = Vec::new();

    for topology_part in &topology_report.parts {
        let part_id = topology_part.part_id.trim();
        for face in topology_part
            .faces
            .iter()
            .filter(|face| face.center.is_some())
        {
            let target_id = direct_occt_face_target_id(part_id, face);
            let Some(selection_target) = selection_targets_by_id.get(target_id.as_str()) else {
                continue;
            };
            let Some(center) = face.center.as_ref() else {
                continue;
            };

            face_targets.push(ViewerFaceTarget {
                target_id: preferred_public_topology_target_id(selection_target, &target_id),
                durable_target_id: selection_target.durable_target_id.clone(),
                canonical_target_id: Some(target_id.clone()),
                alias_ids: viewer_target_alias_ids(selection_target, &target_id),
                part_id: selection_target.part_id.clone(),
                viewer_node_id: selection_target.viewer_node_id.clone(),
                label: direct_occt_face_label(topology_part, face),
                editable: selection_target.editable,
                center: direct_occt_point_to_viewer(center),
                normal: face.normal,
                area: face.area,
            });
        }
    }

    face_targets
}

fn direct_occt_edge_targets(
    topology_report: Option<&DirectOcctTopologyReport>,
    manifest: &ModelManifest,
) -> Vec<ViewerEdgeTarget> {
    let Some(topology_report) = topology_report else {
        return Vec::new();
    };
    let selection_targets_by_id = manifest
        .selection_targets
        .iter()
        .filter(|target| target.kind == SelectionTargetKind::Edge)
        .flat_map(|target| {
            target
                .target_id
                .iter()
                .map(String::as_str)
                .chain(target.durable_target_id.iter().map(String::as_str))
                .chain(target.canonical_target_id.iter().map(String::as_str))
                .chain(target.alias_ids.iter().map(String::as_str))
                .map(move |target_id| (target_id, target))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut edge_targets = Vec::new();

    for topology_part in &topology_report.parts {
        let part_id = topology_part.part_id.trim();
        for edge in topology_part
            .edges
            .iter()
            .filter(|edge| edge.start.is_some() && edge.end.is_some())
        {
            let target_id = direct_occt_edge_target_id(part_id, edge);
            let Some(selection_target) = selection_targets_by_id.get(target_id.as_str()) else {
                continue;
            };
            let Some(start) = edge.start.as_ref() else {
                continue;
            };
            let Some(end) = edge.end.as_ref() else {
                continue;
            };

            edge_targets.push(ViewerEdgeTarget {
                target_id: preferred_public_topology_target_id(selection_target, &target_id),
                durable_target_id: selection_target.durable_target_id.clone(),
                canonical_target_id: Some(target_id.clone()),
                alias_ids: viewer_target_alias_ids(selection_target, &target_id),
                part_id: selection_target.part_id.clone(),
                viewer_node_id: selection_target.viewer_node_id.clone(),
                label: direct_occt_edge_label(topology_part, edge),
                editable: selection_target.editable,
                start: direct_occt_point_to_viewer(start),
                end: direct_occt_point_to_viewer(end),
            });
        }
    }

    edge_targets
}

fn direct_occt_face_targets(
    topology_report: Option<&DirectOcctTopologyReport>,
    manifest: &ModelManifest,
) -> Vec<ViewerFaceTarget> {
    let Some(topology_report) = topology_report else {
        return Vec::new();
    };
    let selection_targets_by_id = manifest
        .selection_targets
        .iter()
        .filter(|target| target.kind == SelectionTargetKind::Face)
        .flat_map(|target| {
            target
                .target_id
                .iter()
                .map(String::as_str)
                .chain(target.durable_target_id.iter().map(String::as_str))
                .chain(target.canonical_target_id.iter().map(String::as_str))
                .chain(target.alias_ids.iter().map(String::as_str))
                .map(move |target_id| (target_id, target))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut face_targets = Vec::new();

    for topology_part in &topology_report.parts {
        let part_id = topology_part.part_id.trim();
        for face in topology_part
            .faces
            .iter()
            .filter(|face| face.center.is_some())
        {
            let target_id = direct_occt_face_target_id(part_id, face);
            let Some(selection_target) = selection_targets_by_id.get(target_id.as_str()) else {
                continue;
            };
            let Some(center) = face.center.as_ref() else {
                continue;
            };

            face_targets.push(ViewerFaceTarget {
                target_id: preferred_public_topology_target_id(selection_target, &target_id),
                durable_target_id: selection_target.durable_target_id.clone(),
                canonical_target_id: Some(target_id.clone()),
                alias_ids: viewer_target_alias_ids(selection_target, &target_id),
                part_id: selection_target.part_id.clone(),
                viewer_node_id: selection_target.viewer_node_id.clone(),
                label: direct_occt_face_label(topology_part, face),
                editable: selection_target.editable,
                center: direct_occt_point_to_viewer(center),
                normal: face.normal,
                area: face.area,
            });
        }
    }

    face_targets
}

fn direct_occt_edge_target_id(part_id: &str, edge: &DirectOcctTopologyEdge) -> String {
    let explicit_target_id = edge.target_id.as_deref().unwrap_or_default().trim();
    if !explicit_target_id.is_empty() {
        return explicit_target_id.to_string();
    }

    let edge_index = edge
        .edge_index
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let edge_signature = edge
        .start
        .as_ref()
        .zip(edge.end.as_ref())
        .map(|(start, end)| direct_occt_edge_signature(start, end));
    match edge_signature {
        Some(signature) => format!("{part_id}:edge:{edge_index}:{signature}"),
        None => format!("{part_id}:edge:{edge_index}"),
    }
}

fn direct_occt_vertex_target_id(part_id: &str, vertex: &DirectOcctTopologyVertex) -> String {
    let explicit_target_id = vertex.target_id.as_deref().unwrap_or_default().trim();
    if !explicit_target_id.is_empty() {
        return explicit_target_id.to_string();
    }
    let vertex_index = vertex
        .vertex_index
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    match vertex.point.as_ref() {
        Some(point) => format!(
            "{part_id}:vertex:{vertex_index}:{}",
            direct_occt_point_signature(point)
        ),
        None => format!("{part_id}:vertex:{vertex_index}"),
    }
}

fn direct_occt_durable_vertex_target_id(
    part_id: &str,
    stable_node_key: Option<&str>,
    root_node_id: Option<u64>,
    target_id: &str,
) -> Option<String> {
    stable_node_key
        .and_then(|stable_node_key| {
            durable_vertex_target_id_for_stable_node_key(part_id, stable_node_key, target_id)
        })
        .or_else(|| {
            root_node_id
                .and_then(|root_node_id| durable_vertex_target_id(part_id, root_node_id, target_id))
        })
}

fn direct_occt_vertex_label(
    topology_part: &DirectOcctTopologyPart,
    vertex: &DirectOcctTopologyVertex,
) -> String {
    let label = vertex.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    let part_label = if topology_part.label.trim().is_empty() {
        topology_part.part_id.trim()
    } else {
        topology_part.label.trim()
    };
    let vertex_index = vertex
        .vertex_index
        .map(|index| index.saturating_add(1).to_string())
        .unwrap_or_else(|| "?".to_string());
    format!("{part_label}.Vertex{vertex_index}")
}

fn direct_occt_stable_edge_target_id(target_id: &str) -> String {
    stable_edge_target_id(target_id)
}

fn direct_occt_durable_edge_target_id(
    part_id: &str,
    stable_node_key: Option<&str>,
    root_node_id: Option<u64>,
    target_id: &str,
) -> Option<String> {
    stable_node_key
        .and_then(|stable_node_key| {
            durable_edge_target_id_for_stable_node_key(part_id, stable_node_key, target_id)
        })
        .or_else(|| {
            root_node_id
                .and_then(|root_node_id| durable_edge_target_id(part_id, root_node_id, target_id))
        })
}

fn direct_occt_edge_label(
    topology_part: &DirectOcctTopologyPart,
    edge: &DirectOcctTopologyEdge,
) -> String {
    let label = edge.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }

    let part_label = if topology_part.label.trim().is_empty() {
        topology_part.part_id.trim()
    } else {
        topology_part.label.trim()
    };
    let edge_index = edge
        .edge_index
        .map(|index| index.saturating_add(1).to_string())
        .unwrap_or_else(|| "?".to_string());
    format!("{part_label}.Edge{edge_index}")
}

fn direct_occt_face_target_id(part_id: &str, face: &DirectOcctTopologyFace) -> String {
    let explicit_target_id = face.target_id.as_deref().unwrap_or_default().trim();
    if !explicit_target_id.is_empty() {
        return explicit_target_id.to_string();
    }

    let face_index = face
        .face_index
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let face_signature = face.center.as_ref().map(|center| {
        let center_signature = direct_occt_point_signature(center);
        let area_signature = face
            .area
            .map(format_topology_coordinate)
            .unwrap_or_else(|| "unknown".to_string());
        format!("{center_signature}:{area_signature}")
    });

    match face_signature {
        Some(signature) => format!("{part_id}:face:{face_index}:{signature}"),
        None => format!("{part_id}:face:{face_index}"),
    }
}

fn direct_occt_stable_face_target_id(target_id: &str) -> String {
    stable_face_target_id(target_id)
}

fn direct_occt_durable_face_target_id(
    part_id: &str,
    stable_node_key: Option<&str>,
    root_node_id: Option<u64>,
    target_id: &str,
) -> Option<String> {
    stable_node_key
        .and_then(|stable_node_key| {
            durable_face_target_id_for_stable_node_key(part_id, stable_node_key, target_id)
        })
        .or_else(|| {
            root_node_id
                .and_then(|root_node_id| durable_face_target_id(part_id, root_node_id, target_id))
        })
}

fn direct_occt_face_label(
    topology_part: &DirectOcctTopologyPart,
    face: &DirectOcctTopologyFace,
) -> String {
    let label = face.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }

    let part_label = if topology_part.label.trim().is_empty() {
        topology_part.part_id.trim()
    } else {
        topology_part.label.trim()
    };
    let face_index = face
        .face_index
        .map(|index| index.saturating_add(1).to_string())
        .unwrap_or_else(|| "?".to_string());
    format!("{part_label}.Face{face_index}")
}

fn direct_occt_point_to_viewer(point: &DirectOcctTopologyPoint) -> ViewerEdgePoint {
    ViewerEdgePoint {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}

fn direct_occt_point_signature(point: &DirectOcctTopologyPoint) -> String {
    [point.x, point.y, point.z]
        .into_iter()
        .map(format_topology_coordinate)
        .collect::<Vec<_>>()
        .join("-")
}

fn direct_occt_edge_signature(
    start: &DirectOcctTopologyPoint,
    end: &DirectOcctTopologyPoint,
) -> String {
    let mut endpoints = [
        direct_occt_point_signature(start),
        direct_occt_point_signature(end),
    ];
    endpoints.sort();
    endpoints.join("_")
}

fn format_topology_coordinate(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let formatted = format!("{value:.3}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" || trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn path_to_string(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| AppError::internal("Non-UTF-8 path encountered."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        validate_model_runtime_bundle, ParamValue, SelectionTargetKind, TaggedAnchorKind,
        ViewerAssetFormat,
    };
    use crate::ecky_cad_host::direct_occt_executor::export_core_program_step_stl_with_params;
    use crate::ecky_cad_host::direct_occt_sdk::{
        bundled_occt_runtime_root_from_repo, inspect_occt_runtime,
    };
    use crate::ecky_core_ir::CoreSelectorTagKind;
    use crate::models::PathResolver;
    use std::path::PathBuf;

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
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

    #[derive(Clone)]
    struct ResourceResolver {
        root: PathBuf,
    }

    impl PathResolver for ResourceResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, path: &str) -> Option<PathBuf> {
            let candidate = self.root.join("resources").join(path);
            candidate.exists().then_some(candidate)
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{label}-{}", uuid::Uuid::new_v4()))
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write script");
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }

    fn compile(source: &str) -> CoreProgram {
        crate::ecky_scheme::compile_to_core_program(source).expect("compile")
    }

    #[test]
    fn model_title_metadata_labels_direct_occt_document() {
        let program = compile(r#"(model (meta :title "Pasta Curl") (part body (box 1 1 1)))"#);
        let mut document = DocumentMetadata {
            document_name: "Direct OCCT".into(),
            document_label: "Direct OCCT".into(),
            source_path: None,
            object_count: 1,
            warnings: Vec::new(),
        };

        apply_direct_occt_model_metadata(&mut document, &program);

        assert_eq!(document.document_label, "Pasta Curl");
    }

    #[cfg(unix)]
    fn part_bindings_without_bounds(parts: &[PartBinding]) -> Vec<PartBinding> {
        parts
            .iter()
            .cloned()
            .map(|mut part| {
                part.bounds = None;
                part
            })
            .collect()
    }

    #[cfg(unix)]
    fn assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
        label: &str,
        source: &str,
    ) {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root(label);
        let resolver = ResourceResolver { root: root.clone() };
        let program = compile(source);
        let params = DesignParams::new();
        let source_identity = source.to_string();
        let params_json = serde_json::to_string(&params).expect("params json");
        let content_hash = content_hash_with_runtime_inputs(
            &program,
            &source_identity,
            &params_json,
            &params,
            None,
        )
        .expect("content hash");
        let model_id = model_id_from_hash(&content_hash);
        let direct_dir = root.join("direct");
        fs::create_dir_all(&direct_dir).expect("direct dir");

        let direct_outcome =
            export_core_program_step_stl_with_params(&program, &params, &layout, &direct_dir)
                .expect("direct export");
        let NativeExportOutcome::Exported {
            step_path: direct_step_path,
            stl_path: direct_stl_path,
            ..
        } = direct_outcome
        else {
            panic!("expected generated-runner export");
        };
        let direct_topology_path = direct_dir.join(TOPOLOGY_FILE_NAME);
        let direct_topology = read_direct_occt_topology_report(&direct_topology_path)
            .expect("direct topology report");
        let direct_source_path = direct_dir.join(SOURCE_FILE_NAME);
        fs::write(&direct_source_path, source).expect("direct source");
        let direct_part_stable_node_keys = program
            .parts
            .iter()
            .filter_map(|part| {
                direct_occt_source_stable_node_key(source, part)
                    .map(|stable_node_key| (part.key.clone(), stable_node_key))
            })
            .collect::<HashMap<_, _>>();
        let direct_part_root_node_ids = program
            .parts
            .iter()
            .map(|part| (part.key.clone(), part.root.id.raw()))
            .collect::<HashMap<_, _>>();
        let direct_part_specs = program
            .parts
            .iter()
            .map(|part| (part.key.clone(), part.label.clone()))
            .collect::<Vec<_>>();
        let direct_parameter_keys = program
            .parameters
            .iter()
            .map(|parameter| parameter.key.clone())
            .collect::<Vec<_>>();
        let direct_manifest = build_direct_occt_manifest_with_stable_node_keys(
            &model_id,
            &direct_source_path,
            &direct_part_specs,
            &direct_parameter_keys,
            &program.selector_tags,
            Some(&direct_topology),
            &direct_part_stable_node_keys,
            &direct_part_root_node_ids,
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("direct manifest");
        let direct_bundle = build_direct_occt_bundle(
            &model_id,
            &content_hash,
            &direct_source_path,
            &direct_stl_path,
            &direct_step_path,
            Some(&direct_topology),
            &direct_manifest,
        )
        .expect("direct bundle");
        validate_model_runtime_bundle(&direct_manifest, &direct_bundle).expect("direct contract");

        let runner_source_dir = root.join("runner-source");
        fs::create_dir_all(&runner_source_dir).expect("runner source dir");
        fs::copy(&direct_step_path, runner_source_dir.join(STEP_FILE_NAME)).expect("runner step");
        fs::copy(
            &direct_stl_path,
            runner_source_dir.join(MODEL_STL_FILE_NAME),
        )
        .expect("runner stl");
        fs::copy(
            &direct_topology_path,
            runner_source_dir.join(TOPOLOGY_FILE_NAME),
        )
        .expect("runner topology");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let invoked_marker = root.join("runner-invoked.txt");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("runner parent dir");
        let runner_script = format!(
            r#"#!/bin/sh
set -eu
source_dir='{}'
invoked_marker='{}'
plan=""
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --plan)
      plan=$2
      shift 2
      ;;
    --out)
      out=$2
      shift 2
      ;;
    *)
      echo "unexpected arg: $1" >&2
      exit 1
      ;;
  esac
done
mkdir -p "$out"
cp "$source_dir/model.step" "$out/model.step"
cp "$source_dir/model.stl" "$out/model.stl"
cp "$source_dir/topology.json" "$out/topology.json"
cat > "$out/stage-report.json" <<'EOF'
{{"schemaVersion":1,"totalElapsedMs":0,"stages":[{{"name":"import","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"validate","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"solidify","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"boolean","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"cleanup","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"mesh","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"verify","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"export","status":"skipped","executionCount":0,"elapsedMs":0}}]}}
EOF
printf '%s\n' "$plan" > "$invoked_marker"
echo "fake runner plan: $plan"
"#,
            runner_source_dir.display(),
            invoked_marker.display()
        );
        write_executable(&runner, &runner_script);

        let (runner_bundle, runner_manifest) =
            render_core_program_runtime_bundle(&program, source, &params, &layout, &resolver)
                .expect("runner-first bundle");
        let runner_bundle_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &model_id)
            .expect("runner bundle dir");
        let runner_topology_path = runner_bundle_dir.join(TOPOLOGY_FILE_NAME);

        assert!(
            invoked_marker.is_file(),
            "expected fake runner invocation marker"
        );
        assert_eq!(
            fs::read(&runner_bundle.model_stl_path).expect("runner preview"),
            fs::read(&direct_stl_path).expect("direct preview")
        );
        assert_eq!(
            fs::read(&runner_bundle.export_artifacts[0].path).expect("runner step"),
            fs::read(&direct_step_path).expect("direct step")
        );
        assert_eq!(
            fs::read_to_string(&runner_topology_path).expect("runner topology"),
            fs::read_to_string(&direct_topology_path).expect("direct topology")
        );
        assert_eq!(runner_bundle.model_id, direct_bundle.model_id);
        assert_eq!(runner_bundle.content_hash, direct_bundle.content_hash);
        assert_eq!(runner_bundle.export_artifacts.len(), 1);
        assert_eq!(runner_bundle.export_artifacts[0].label, "STEP");
        assert_eq!(runner_bundle.export_artifacts[0].format, "step");
        assert_eq!(runner_bundle.export_artifacts[0].role, "primary");
        assert_eq!(runner_bundle.edge_targets, direct_bundle.edge_targets);
        assert_eq!(runner_bundle.face_targets, direct_bundle.face_targets);
        assert_eq!(
            runner_manifest.document.object_count,
            direct_manifest.document.object_count
        );
        assert_eq!(
            part_bindings_without_bounds(&runner_manifest.parts),
            part_bindings_without_bounds(&direct_manifest.parts)
        );
        assert_eq!(
            runner_manifest.parameter_groups,
            direct_manifest.parameter_groups
        );
        assert_eq!(
            runner_manifest.selection_targets,
            direct_manifest.selection_targets
        );
        validate_model_runtime_bundle(&runner_manifest, &runner_bundle).expect("runner contract");

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_artifacts_for_coarse_edge_selector_fixture() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-runner-parity-coarse-edge");
        let resolver = ResourceResolver { root: root.clone() };
        let source = r#"
(model
  (part body
    (fillet 1.5 :edges "left+vertical" (box 20 20 10))))
"#;
        let program = compile(source);
        let params = DesignParams::new();
        let source_identity = source.to_string();
        let params_json = serde_json::to_string(&params).expect("params json");
        let content_hash = content_hash_with_runtime_inputs(
            &program,
            &source_identity,
            &params_json,
            &params,
            None,
        )
        .expect("content hash");
        let model_id = model_id_from_hash(&content_hash);
        let direct_dir = root.join("direct");
        fs::create_dir_all(&direct_dir).expect("direct dir");

        let direct_outcome =
            export_core_program_step_stl_with_params(&program, &params, &layout, &direct_dir)
                .expect("direct export");
        let NativeExportOutcome::Exported {
            step_path: direct_step_path,
            stl_path: direct_stl_path,
            ..
        } = direct_outcome
        else {
            panic!("expected generated-runner export");
        };
        let direct_topology_path = direct_dir.join(TOPOLOGY_FILE_NAME);
        let direct_topology = read_direct_occt_topology_report(&direct_topology_path)
            .expect("direct topology report");
        let direct_source_path = direct_dir.join(SOURCE_FILE_NAME);
        fs::write(&direct_source_path, source).expect("direct source");
        let direct_part_stable_node_keys = program
            .parts
            .iter()
            .filter_map(|part| {
                direct_occt_source_stable_node_key(source, part)
                    .map(|stable_node_key| (part.key.clone(), stable_node_key))
            })
            .collect::<HashMap<_, _>>();
        let direct_part_root_node_ids = program
            .parts
            .iter()
            .map(|part| (part.key.clone(), part.root.id.raw()))
            .collect::<HashMap<_, _>>();
        let direct_part_specs = program
            .parts
            .iter()
            .map(|part| (part.key.clone(), part.label.clone()))
            .collect::<Vec<_>>();
        let direct_parameter_keys = program
            .parameters
            .iter()
            .map(|parameter| parameter.key.clone())
            .collect::<Vec<_>>();
        let direct_manifest = build_direct_occt_manifest_with_stable_node_keys(
            &model_id,
            &direct_source_path,
            &direct_part_specs,
            &direct_parameter_keys,
            &program.selector_tags,
            Some(&direct_topology),
            &direct_part_stable_node_keys,
            &direct_part_root_node_ids,
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("direct manifest");
        let direct_bundle = build_direct_occt_bundle(
            &model_id,
            &content_hash,
            &direct_source_path,
            &direct_stl_path,
            &direct_step_path,
            Some(&direct_topology),
            &direct_manifest,
        )
        .expect("direct bundle");
        validate_model_runtime_bundle(&direct_manifest, &direct_bundle).expect("direct contract");

        let runner_source_dir = root.join("runner-source");
        fs::create_dir_all(&runner_source_dir).expect("runner source dir");
        fs::copy(&direct_step_path, runner_source_dir.join(STEP_FILE_NAME)).expect("runner step");
        fs::copy(
            &direct_stl_path,
            runner_source_dir.join(MODEL_STL_FILE_NAME),
        )
        .expect("runner stl");
        fs::copy(
            &direct_topology_path,
            runner_source_dir.join(TOPOLOGY_FILE_NAME),
        )
        .expect("runner topology");
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let invoked_marker = root.join("runner-invoked.txt");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("runner parent dir");
        let runner_script = format!(
            r#"#!/bin/sh
set -eu
source_dir='{}'
invoked_marker='{}'
plan=""
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --plan)
      plan=$2
      shift 2
      ;;
    --out)
      out=$2
      shift 2
      ;;
    *)
      echo "unexpected arg: $1" >&2
      exit 1
      ;;
  esac
done
mkdir -p "$out"
cp "$source_dir/model.step" "$out/model.step"
cp "$source_dir/model.stl" "$out/model.stl"
cp "$source_dir/topology.json" "$out/topology.json"
cat > "$out/stage-report.json" <<'EOF'
{{"schemaVersion":1,"totalElapsedMs":0,"stages":[{{"name":"import","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"validate","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"solidify","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"boolean","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"cleanup","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"mesh","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"verify","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"export","status":"skipped","executionCount":0,"elapsedMs":0}}]}}
EOF
printf '%s\n' "$plan" > "$invoked_marker"
echo "fake runner plan: $plan"
"#,
            runner_source_dir.display(),
            invoked_marker.display()
        );
        write_executable(&runner, &runner_script);

        let (runner_bundle, runner_manifest) =
            render_core_program_runtime_bundle(&program, source, &params, &layout, &resolver)
                .expect("runner-first bundle");
        let runner_bundle_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &model_id)
            .expect("runner bundle dir");
        let runner_topology_path = runner_bundle_dir.join(TOPOLOGY_FILE_NAME);

        assert!(
            invoked_marker.is_file(),
            "expected fake runner invocation marker"
        );
        assert_eq!(
            fs::read(&runner_bundle.model_stl_path).expect("runner preview"),
            fs::read(&direct_stl_path).expect("direct preview")
        );
        assert_eq!(
            fs::read(&runner_bundle.export_artifacts[0].path).expect("runner step"),
            fs::read(&direct_step_path).expect("direct step")
        );
        assert_eq!(
            fs::read_to_string(&runner_topology_path).expect("runner topology"),
            fs::read_to_string(&direct_topology_path).expect("direct topology")
        );
        assert_eq!(runner_bundle.model_id, direct_bundle.model_id);
        assert_eq!(runner_bundle.content_hash, direct_bundle.content_hash);
        assert_eq!(runner_bundle.export_artifacts.len(), 1);
        assert_eq!(runner_bundle.export_artifacts[0].label, "STEP");
        assert_eq!(runner_bundle.export_artifacts[0].format, "step");
        assert_eq!(runner_bundle.export_artifacts[0].role, "primary");
        assert_eq!(runner_bundle.edge_targets, direct_bundle.edge_targets);
        assert_eq!(runner_bundle.face_targets, direct_bundle.face_targets);
        assert_eq!(
            runner_manifest.document.object_count,
            direct_manifest.document.object_count
        );
        assert_eq!(
            part_bindings_without_bounds(&runner_manifest.parts),
            part_bindings_without_bounds(&direct_manifest.parts)
        );
        assert_eq!(
            runner_manifest.parameter_groups,
            direct_manifest.parameter_groups
        );
        assert_eq!(
            runner_manifest.selection_targets,
            direct_manifest.selection_targets
        );
        validate_model_runtime_bundle(&runner_manifest, &runner_bundle).expect("runner contract");

        let _ = fs::remove_dir_all(root);
    }

    fn blocked_layout(root: PathBuf) -> DirectOcctSdkLayout {
        DirectOcctSdkLayout {
            runtime_root: root,
            dylib_dir: None,
            include_dir: None,
            missing_headers: vec!["BRepPrimAPI_MakeBox.hxx".to_string()],
            missing_libs: vec!["TKernel".to_string()],
            install_name_prefix: "@rpath",
        }
    }

    #[cfg(unix)]
    #[test]
    fn semantic_only_tag_face_edit_reuses_geometry_without_invoking_native_runner() {
        let root = temp_root("direct-occt-semantic-geometry-cache");
        let resolver = ResourceResolver { root: root.clone() };
        let runner = root
            .join("resources")
            .join("bin")
            .join("direct-occt-runner");
        let invoked_marker = root.join("runner-invocations.txt");
        fs::create_dir_all(runner.parent().expect("runner parent")).expect("runner parent dir");
        write_executable(
            &runner,
            &format!(
                r#"#!/bin/sh
set -eu
invoked_marker='{}'
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --plan) shift 2 ;;
    --out) out=$2; shift 2 ;;
    *) exit 1 ;;
  esac
done
mkdir -p "$out"
printf 'step-geometry\n' > "$out/model.step"
printf 'solid cached\nendsolid cached\n' > "$out/model.stl"
cat > "$out/topology.json" <<'EOF'
{{"schemaVersion":1,"parts":[{{"partId":"body","label":"Body","sourceGeometryDigest":"sha256:unchanged","vertices":[],"edges":[],"faces":[{{"targetId":"body:face:0:0-0-0:100","faceIndex":0,"label":"Body.Face1","center":{{"x":0,"y":0,"z":0}},"normal":[0,0,1],"area":100,"authoredBindings":["base"]}},{{"targetId":"body:face:1:0-0-10:100","faceIndex":1,"label":"Body.Face2","center":{{"x":0,"y":0,"z":10}},"normal":[0,0,1],"area":100,"authoredBindings":["bore"]}}]}}]}}
EOF
cat > "$out/stage-report.json" <<'EOF'
{{"schemaVersion":1,"totalElapsedMs":0,"stages":[{{"name":"import","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"validate","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"solidify","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"boolean","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"cleanup","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"mesh","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"verify","status":"skipped","executionCount":0,"elapsedMs":0}},{{"name":"export","status":"skipped","executionCount":0,"elapsedMs":0}}]}}
EOF
printf 'run\n' >> "$invoked_marker"
"#,
                invoked_marker.display()
            ),
        );
        let layout = blocked_layout(root.join("unused-sdk"));
        let source = |tag_name: &str, binding: &str| {
            format!(
                r#"(model
                  (tag-face {tag_name} :faces "created-by:{binding}" body)
                  (part body (build
                    (shape base (box 10 10 10))
                    (shape bore (cylinder 2 10))
                    (result (difference base bore)))))"#
            )
        };
        let cold_source = source("load_base", "base");
        let cold_program = compile(&cold_source);
        let warm_source = source("load_bore", "bore");
        let warm_program = compile(&warm_source);
        let params_json = serde_json::to_string(&DesignParams::new()).expect("params JSON");
        assert_eq!(
            direct_occt_geometry_hash(
                &cold_program,
                &params_json,
                &DesignParams::new(),
                None,
                &resolver,
            )
            .expect("cold geometry hash"),
            direct_occt_geometry_hash(
                &warm_program,
                &params_json,
                &DesignParams::new(),
                None,
                &resolver,
            )
            .expect("warm geometry hash"),
            "tag declarations must not participate in evaluated geometry identity"
        );
        let (cold_bundle, cold_manifest) = render_core_program_runtime_bundle(
            &cold_program,
            &cold_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("cold render");
        let cold_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &cold_bundle.model_id)
            .expect("cold dir");
        let cold_topology = fs::read(cold_dir.join(TOPOLOGY_FILE_NAME)).expect("cold topology");

        let (warm_bundle, warm_manifest) = render_core_program_runtime_bundle(
            &warm_program,
            &warm_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("semantic-only render");
        let warm_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &warm_bundle.model_id)
            .expect("warm dir");

        assert_ne!(cold_bundle.model_id, warm_bundle.model_id);
        assert_eq!(
            fs::read_to_string(&invoked_marker)
                .expect("invocation marker")
                .lines()
                .count(),
            1,
            "semantic-only render must not invoke native geometry execution"
        );
        assert_eq!(
            cold_topology,
            fs::read(warm_dir.join(TOPOLOGY_FILE_NAME)).expect("warm topology")
        );
        assert_eq!(
            fs::read(&cold_bundle.model_stl_path).expect("cold preview"),
            fs::read(&warm_bundle.model_stl_path).expect("warm preview")
        );
        assert!(cold_manifest.tagged_anchors.contains_key("load_base"));
        assert!(!warm_manifest.tagged_anchors.contains_key("load_base"));
        assert!(warm_manifest.tagged_anchors.contains_key("load_bore"));
        assert_ne!(
            cold_manifest.tagged_anchors["load_base"].canonical_target_ids,
            warm_manifest.tagged_anchors["load_bore"].canonical_target_ids,
            "cached topology must be re-resolved against the new authored selector"
        );

        let mut changed_runner = fs::read_to_string(&runner).expect("runner source");
        changed_runner.push_str("\n# runner-v2\n");
        fs::write(&runner, changed_runner).expect("replace runner binary fixture");
        let after_runner_change_source = source("load_base_v2", "base");
        render_core_program_runtime_bundle(
            &compile(&after_runner_change_source),
            &after_runner_change_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("first render after runner change");
        let after_runner_change_warm_source = source("load_bore_v2", "bore");
        render_core_program_runtime_bundle(
            &compile(&after_runner_change_warm_source),
            &after_runner_change_warm_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("warm render after runner change");
        assert_eq!(
            fs::read_to_string(&invoked_marker)
                .expect("runner-change invocation marker")
                .lines()
                .count(),
            2,
            "runner binary change must force exactly one new cold geometry render"
        );

        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params JSON");
        let geometry_hash = direct_occt_geometry_hash(
            &compile(&after_runner_change_source),
            &params_json,
            &params,
            None,
            &resolver,
        )
        .expect("geometry hash");
        let geometry_cache_dir =
            direct_occt_geometry_cache_dir(&resolver, &geometry_hash).expect("geometry cache dir");
        fs::write(
            geometry_cache_dir.join(MODEL_STL_FILE_NAME),
            "corrupt-cache-entry",
        )
        .expect("corrupt cached preview");
        for tag_name in ["load_after_corruption", "load_after_repair"] {
            let source_after_corruption = source(tag_name, "base");
            render_core_program_runtime_bundle(
                &compile(&source_after_corruption),
                &source_after_corruption,
                &DesignParams::new(),
                &layout,
                &resolver,
            )
            .expect("render after cache corruption");
        }
        assert_eq!(
            fs::read_to_string(&invoked_marker)
                .expect("corruption invocation marker")
                .lines()
                .count(),
            3,
            "corrupt geometry cache must cause one cold repair, then become warm again"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn builds_valid_direct_occt_bundle_manifest_for_exported_box() {
        let root = temp_root("direct-occt-bundle");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        fs::write(&preview_path, b"solid preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21;").expect("step");

        let manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        let (stored, stored_manifest) =
            crate::model_runtime::write_runtime_bundle(&resolver, &model_id, &bundle, &manifest)
                .expect("write runtime bundle");

        assert!(stored.fcstd_path.is_empty());
        assert_eq!(stored.geometry_backend, GeometryBackend::EckyRust);
        assert_eq!(
            stored
                .geometry_provenance
                .as_ref()
                .map(|p| &p.representation),
            Some(&GeometryRepresentation::AnalyticBrep)
        );
        assert_eq!(stored.export_artifacts[0].format, "step");
        assert_eq!(
            stored.export_artifacts[0]
                .geometry_provenance
                .as_ref()
                .map(|p| &p.representation),
            Some(&GeometryRepresentation::AnalyticBrep)
        );
        assert_eq!(stored.viewer_assets.len(), 1);
        assert_eq!(stored.viewer_assets[0].format, ViewerAssetFormat::Stl);
        assert_eq!(stored_manifest.parts[0].viewer_node_ids, vec!["body"]);
        assert_eq!(
            stored_manifest
                .geometry_provenance
                .as_ref()
                .map(|p| &p.representation),
            Some(&GeometryRepresentation::AnalyticBrep)
        );

        let (read_bundle, read_manifest) =
            crate::model_runtime::read_runtime_bundle(&resolver, &model_id).expect("read");
        assert_eq!(read_bundle.model_id, model_id);
        assert_eq!(read_manifest.model_id, model_id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_cached_direct_occt_bundle_without_explicit_provenance() {
        let root = temp_root("direct-occt-cache-legacy-provenance");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        fs::write(&preview_path, b"solid legacy preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21; legacy step").expect("step");

        let mut manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        let mut bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        bundle.geometry_provenance = None;
        manifest.geometry_provenance = None;
        for artifact in &mut bundle.export_artifacts {
            artifact.geometry_provenance = None;
        }
        let (stored_bundle, stored_manifest) =
            crate::model_runtime::write_runtime_bundle(&resolver, &model_id, &bundle, &manifest)
                .expect("write legacy runtime bundle");
        assert_eq!(stored_bundle.content_hash, hash);
        assert_eq!(stored_manifest.model_id, model_id);
        assert!(stored_bundle.geometry_provenance.is_none());
        assert!(stored_manifest.geometry_provenance.is_none());
        assert!(runtime_bundle_artifacts_ready(&stored_bundle));

        let (read_bundle, read_manifest) =
            crate::model_runtime::read_runtime_bundle(&resolver, &model_id)
                .expect("read legacy runtime bundle");
        assert_eq!(read_bundle.content_hash, hash);
        assert_eq!(read_manifest.model_id, model_id);
        assert!(runtime_bundle_artifacts_ready(&read_bundle));

        assert!(
            read_complete_cached_bundle(&resolver, &model_id, &hash).is_none(),
            "Direct OCCT cache must reject artifacts without explicit provenance"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reuses_cached_hybrid_bundle_when_all_provenance_matches() {
        let root = temp_root("direct-occt-cache-hybrid-provenance");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        fs::write(&preview_path, b"solid hybrid preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21; hybrid step").expect("step");

        let provenance = direct_occt_provenance(
            GeometryRepresentation::Hybrid,
            vec!["sha256:relief".to_string()],
        );
        let mut manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        manifest.geometry_provenance = Some(provenance.clone());
        let mut bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        bundle.geometry_provenance = Some(provenance.clone());
        for artifact in &mut bundle.export_artifacts {
            artifact.geometry_provenance = Some(provenance.clone());
        }
        let stored =
            crate::model_runtime::write_runtime_bundle(&resolver, &model_id, &bundle, &manifest)
                .expect("write hybrid runtime bundle");
        write_complete_cached_bundle_digests(&bundle_dir, &stored.0).expect("write digests");

        let cached = read_complete_cached_bundle(&resolver, &model_id, &hash)
            .expect("matching hybrid cache must be reusable");
        assert_eq!(cached.0.geometry_provenance, Some(provenance));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_cached_hybrid_bundle_when_step_provenance_mismatches() {
        let root = temp_root("direct-occt-cache-hybrid-mismatch");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        fs::write(&preview_path, b"solid hybrid preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21; hybrid step").expect("step");

        let provenance = direct_occt_provenance(
            GeometryRepresentation::Hybrid,
            vec!["sha256:relief".to_string()],
        );
        let mut manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        manifest.geometry_provenance = Some(provenance.clone());
        let mut bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        bundle.geometry_provenance = Some(provenance);
        let stored =
            crate::model_runtime::write_runtime_bundle(&resolver, &model_id, &bundle, &manifest)
                .expect("write mismatched hybrid runtime bundle");
        write_complete_cached_bundle_digests(&bundle_dir, &stored.0).expect("write digests");

        assert!(read_complete_cached_bundle(&resolver, &model_id, &hash).is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reuses_complete_content_matched_bundle_before_starting_kernel() {
        let root = temp_root("direct-occt-content-cache-hit");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let program = compile(source);
        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params");
        let hash = content_hash_with_runtime_inputs(&program, source, &params_json, &params, None)
            .expect("content hash");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        fs::write(&preview_path, b"solid cached preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21; cached step").expect("step");

        let manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        crate::model_runtime::write_runtime_bundle(&resolver, &model_id, &bundle, &manifest)
            .expect("write cached runtime bundle");
        // A production render records stored per-artifact digests after the
        // bundle is persisted; the cold-reuse path requires the sidecar before
        // trusting the bundle, so mirror that here.
        write_complete_cached_bundle_digests(&bundle_dir, &bundle).expect("write digests");

        let _runner_guard =
            crate::ecky_cad_host::direct_occt_runner::test_discovery::CwdFallbackGuard::disable();
        let (cached, cached_manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &params,
            &blocked_layout(root.clone()),
            &resolver,
        )
        .expect("complete cache hit must not start blocked kernel");

        assert_eq!(cached.model_id, model_id);
        assert_eq!(cached.content_hash, hash);
        assert_eq!(cached_manifest.model_id, model_id);
        assert_eq!(
            fs::read(&preview_path).expect("cached preview"),
            b"solid cached preview"
        );

        fs::write(bundle_dir.join("bundle.json"), b"not valid json")
            .expect("corrupt cold cache metadata after validated read");
        let (hot_cached, hot_manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &params,
            &blocked_layout(root.clone()),
            &resolver,
        )
        .expect("hot cache hit must not parse the large disk bundle again");
        assert_eq!(hot_cached.model_id, model_id);
        assert_eq!(hot_manifest.model_id, model_id);

        fs::remove_file(&preview_path).expect("remove cached preview");
        assert!(
            read_complete_cached_bundle(&resolver, &model_id, &hash).is_none(),
            "missing artifact must invalidate cache hit"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cached_bundle_rejects_same_size_preview_mutation_via_stored_digest() {
        let root = temp_root("direct-occt-content-cache-same-size-mutation");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let program = compile(source);
        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params");
        let hash = content_hash_with_runtime_inputs(&program, source, &params_json, &params, None)
            .expect("content hash");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        // Same-length payload so a size-only readiness check cannot detect it.
        let original: &[u8] = b"solid cached preview payload";
        let mutated: &[u8] = b"SOLID cached preview payload";
        assert_eq!(
            original.len(),
            mutated.len(),
            "fixture must mutate preview at the same byte length"
        );
        fs::write(&preview_path, original).expect("preview");
        fs::write(&step_path, b"ISO-10303-21; cached step").expect("step");

        let manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        let stored =
            crate::model_runtime::write_runtime_bundle(&resolver, &model_id, &bundle, &manifest)
                .expect("write cached runtime bundle");
        // The render path records stored per-artifact digests after a verified
        // render, which is the trust anchor for later warm reuse.
        write_complete_cached_bundle_digests(&bundle_dir, &stored.0).expect("write digests");

        // Sanity: the unmutated bundle is a warm-reuse hit.
        assert!(
            read_complete_cached_bundle(&resolver, &model_id, &hash).is_some(),
            "unmutated cached bundle with matching digests must hit"
        );
        // The hot in-memory cache would mask a disk mutation; simulate a cold
        // reuse (e.g. after a process restart) by evicting the hot entry.
        forget_hot_cached_bundle(&bundle_dir);

        // Mutate model.stl in place, preserving the exact byte length so only
        // a content digest can detect it.
        fs::write(&preview_path, mutated).expect("mutate preview in place");

        assert!(
            read_complete_cached_bundle(&resolver, &model_id, &hash).is_none(),
            "same-size preview mutation must miss the artifact cache via the stored digest"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cached_bundle_without_digest_sidecar_is_a_cold_cache_miss() {
        // BDD: a complete bundle persisted without its digest sidecar (the
        // state left behind when sidecar persistence fails after
        // write_runtime_bundle succeeds) MUST NOT be trusted on cold reuse.
        // The cold read must miss and the render must re-execute. We prove the
        // re-execution by using a blocked Direct OCCT layout: a cache hit would
        // skip the kernel and return the bundle, while a cache miss starts the
        // blocked kernel and fails.
        let root = temp_root("direct-occt-content-cache-missing-sidecar");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let program = compile(source);
        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params");
        let hash = content_hash_with_runtime_inputs(&program, source, &params_json, &params, None)
            .expect("content hash");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        fs::write(&preview_path, b"solid cached preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21; cached step").expect("step");

        let manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        // Persist a complete bundle + manifest, exactly as write_runtime_bundle
        // would, but deliberately omit the digest sidecar.
        let _ =
            crate::model_runtime::write_runtime_bundle(&resolver, &model_id, &bundle, &manifest)
                .expect("write cached runtime bundle");
        assert!(
            !bundle_dir.join(DIGESTS_FILE_NAME).exists(),
            "test setup: sidecar must be absent"
        );

        let _runner_guard =
            crate::ecky_cad_host::direct_occt_runner::test_discovery::CwdFallbackGuard::disable();
        let miss = render_core_program_runtime_bundle(
            &program,
            source,
            &params,
            &blocked_layout(root.clone()),
            &resolver,
        );

        assert!(
            miss.is_err(),
            "missing digest sidecar must be a cold cache miss and re-render, not trusted reuse"
        );

        let _ = fs::remove_dir_all(root);
    }

    fn digest_match_fixture(label: &str) -> (PathBuf, PathBuf, ArtifactBundle) {
        let root = temp_root(label);
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let program = compile(source);
        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params");
        let hash = content_hash_with_runtime_inputs(&program, source, &params_json, &params, None)
            .expect("content hash");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        fs::write(&preview_path, b"solid cached preview payload").expect("preview");
        fs::write(&step_path, b"ISO-10303-21; cached step").expect("step");
        let manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        (root, bundle_dir, bundle)
    }

    #[test]
    fn complete_cached_bundle_digests_match_accepts_unmutated_sidecar() {
        let (root, bundle_dir, bundle) = digest_match_fixture("direct-occt-digest-match-unmutated");
        write_complete_cached_bundle_digests(&bundle_dir, &bundle).expect("write digests");
        assert!(
            complete_cached_bundle_digests_match(&bundle_dir, &bundle),
            "unmutated artifacts with a matching sidecar must verify"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn complete_cached_bundle_digests_match_rejects_different_size_mutation() {
        let (root, bundle_dir, bundle) =
            digest_match_fixture("direct-occt-digest-match-different-size");
        write_complete_cached_bundle_digests(&bundle_dir, &bundle).expect("write digests");
        fs::write(
            bundle_dir.join(MODEL_STL_FILE_NAME),
            b"solid a completely different and longer preview payload",
        )
        .expect("mutate preview");
        assert!(
            !complete_cached_bundle_digests_match(&bundle_dir, &bundle),
            "different-size preview mutation must miss via the stored digest"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn complete_cached_bundle_digests_match_rejects_absent_sidecar() {
        let (root, bundle_dir, bundle) =
            digest_match_fixture("direct-occt-digest-match-absent-sidecar");
        // No sidecar written: a bundle that lacks its stored per-artifact
        // digests must NOT be trusted for reuse (e.g. when sidecar persistence
        // failed after the bundle itself was written). It is a cache miss.
        assert!(
            !complete_cached_bundle_digests_match(&bundle_dir, &bundle),
            "absent digest sidecar must be a cache miss, not trusted legacy reuse"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_complete_cached_bundle_digests_publishes_atomically_without_temp_residue() {
        let (root, bundle_dir, bundle) = digest_match_fixture("direct-occt-digest-write-atomic");
        write_complete_cached_bundle_digests(&bundle_dir, &bundle).expect("write digests");

        let final_path = bundle_dir.join(DIGESTS_FILE_NAME);
        let temp_path = bundle_dir.join(format!("{DIGESTS_FILE_NAME}.tmp"));
        assert!(
            final_path.is_file(),
            "atomic publish must produce digests.json"
        );
        assert!(
            !temp_path.exists(),
            "temp file must be renamed away, not left behind"
        );
        // The published sidecar must be complete, parseable JSON.
        let raw = fs::read_to_string(&final_path).expect("read sidecar");
        let sidecar: CachedArtifactDigestSidecar =
            serde_json::from_str(&raw).expect("published sidecar must be valid JSON");
        assert_eq!(
            sidecar.schema_version,
            CACHED_ARTIFACT_DIGEST_SCHEMA_VERSION
        );
        // Every on-disk artifact is covered by a stored digest.
        for path in cached_artifact_paths(&bundle) {
            assert!(
                sidecar.digests.contains_key(&path),
                "artifact '{path}' must be covered"
            );
        }
        assert!(
            complete_cached_bundle_digests_match(&bundle_dir, &bundle),
            "atomically published digests must verify"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn complete_cached_bundle_digests_match_rejects_corrupt_sidecar() {
        let (root, bundle_dir, bundle) =
            digest_match_fixture("direct-occt-digest-match-corrupt-sidecar");
        // Simulate a torn/corrupt sidecar (e.g. a non-atomic write that was
        // interrupted). Readers must treat it as a cache miss, not a hit.
        fs::write(bundle_dir.join(DIGESTS_FILE_NAME), b"{ not valid json ")
            .expect("write corrupt sidecar");
        assert!(
            !complete_cached_bundle_digests_match(&bundle_dir, &bundle),
            "corrupt digest sidecar must be a cache miss"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn complete_cached_bundle_digests_match_rejects_uncovered_artifact() {
        let (root, bundle_dir, bundle) = digest_match_fixture("direct-occt-digest-match-uncovered");
        write_complete_cached_bundle_digests(&bundle_dir, &bundle).expect("write digests");
        // Drop the preview entry from the sidecar so the artifact is present on
        // disk but no longer covered by a stored digest.
        let sidecar_path = bundle_dir.join(DIGESTS_FILE_NAME);
        let mut sidecar: CachedArtifactDigestSidecar =
            serde_json::from_str(&fs::read_to_string(&sidecar_path).expect("sidecar"))
                .expect("parse sidecar");
        sidecar.digests.remove(&bundle.model_stl_path);
        fs::write(
            &sidecar_path,
            serde_json::to_string_pretty(&sidecar).expect("reserialize"),
        )
        .expect("write sidecar");
        assert!(
            !complete_cached_bundle_digests_match(&bundle_dir, &bundle),
            "an artifact missing from the sidecar must invalidate reuse"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn content_key_changes_when_imported_stl_bytes_change_at_same_path() {
        let root = temp_root("direct-occt-cache-import-stl-bytes");
        fs::create_dir_all(&root).expect("fixture dir");
        let stl_path = root.join("fixture.stl");
        fs::write(&stl_path, b"solid fixture\nendsolid fixture\n").expect("first stl");
        let source = format!(
            "(model (part body (import-stl {:?})))",
            stl_path.to_string_lossy()
        );
        let program = compile(&source);
        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params");

        let first =
            content_hash_with_runtime_inputs(&program, &source, &params_json, &params, None)
                .expect("first content key");
        fs::write(
            &stl_path,
            b"solid fixture changed\nendsolid fixture changed\n",
        )
        .expect("mutated stl");
        let second =
            content_hash_with_runtime_inputs(&program, &source, &params_json, &params, None)
                .expect("second content key");

        assert_ne!(
            first, second,
            "mutated imported STL must miss artifact cache"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn content_key_changes_when_indexed_sidecar_bytes_change_at_same_path() {
        let root = temp_root("direct-occt-cache-indexed-sidecar-bytes");
        fs::create_dir_all(&root).expect("fixture dir");
        let stl_path = root.join("fixture.stl");
        let sidecar_path = stl_path.with_extension("indexed-mesh.json");
        fs::write(&stl_path, b"solid fixture\nendsolid fixture\n").expect("stl");
        fs::write(&sidecar_path, b"indexed-v1").expect("first sidecar");
        let source = format!(
            "(model (part body (import-stl {:?})))",
            stl_path.to_string_lossy()
        );
        let program = compile(&source);
        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params");

        let first =
            content_hash_with_runtime_inputs(&program, &source, &params_json, &params, None)
                .expect("first content key");
        fs::write(&sidecar_path, b"indexed-v2").expect("mutated sidecar");
        let second =
            content_hash_with_runtime_inputs(&program, &source, &params_json, &params, None)
                .expect("second content key");

        assert_ne!(
            first, second,
            "mutated indexed sidecar must miss artifact cache"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_artifacts_for_same_fixture() {
        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity",
            "(model (part body (box 10 20 30)))",
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_for_nary_boolean_fixture() {
        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity-nary-boolean",
            r#"
            (model
              (part body
                (difference
                  (union
                    (box 20 20 8)
                    (translate 8 0 4 (sphere 6))
                    (translate -8 0 0 (cylinder 5 12)))
                  (translate 5 0 -1 (cylinder 2 14))
                  (translate -5 0 -1 (cylinder 2 14)))))
            "#,
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_for_coarse_edge_selector_fixture() {
        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity-coarse-edge-selector",
            r#"(model (part body (fillet 1.5 :edges "left+vertical" (box 20 20 10))))"#,
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_for_edge_all_selector_fixture() {
        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity-edge-all-selector",
            r#"(model (part body (fillet 1.5 :edges "all" (box 20 20 10))))"#,
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_for_shell_clause_fixture() {
        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity-shell-clause-selector",
            r#"(model (part body (shell 1.5 :faces "planar+normal-z+area-max" (box 20 20 10))))"#,
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_for_keywordless_shell_fixture() {
        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity-shell-default",
            r#"(model (part body (shell 1.5 (box 20 20 10))))"#,
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_for_coarse_chamfer_selector_fixture() {
        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity-coarse-chamfer-selector",
            r#"(model (part body (chamfer 1.25 :edges "left+vertical" (box 20 20 10))))"#,
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_for_exact_edge_target_id_fixture() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-runner-parity-exact-edge-target-source");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (base_bundle, _) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let edge_target_id = base_bundle
            .edge_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box edge target");
        let drifted_edge_target_id = edge_target_id.replacen(":edge:0:", ":edge:999:", 1);
        assert_ne!(drifted_edge_target_id, edge_target_id);
        let exact_source = format!(
            r#"(model (part body (fillet 1.5 :edges "target-id:{drifted_edge_target_id}" (box 20 20 10))))"#
        );
        let _ = fs::remove_dir_all(root);

        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity-exact-edge-target-id",
            &exact_source,
        );
    }

    #[cfg(unix)]
    #[test]
    fn runner_first_bundle_matches_generated_runner_for_exact_face_target_id_fixture() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-runner-parity-exact-face-target-source");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (base_bundle, _) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let face_target_id = base_bundle
            .face_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box face target");
        let drifted_face_target_id = face_target_id.replacen(":face:0:", ":face:999:", 1);
        assert_ne!(drifted_face_target_id, face_target_id);
        let exact_source = format!(
            r#"(model (part body (shell 1.5 :faces "target-id:{drifted_face_target_id}" (box 20 20 10))))"#
        );
        let _ = fs::remove_dir_all(root);

        assert_runner_first_bundle_matches_generated_runner_artifacts_for_fixture(
            "direct-occt-runner-parity-exact-face-target-id",
            &exact_source,
        );
    }

    #[test]
    fn direct_occt_manifest_preserves_multi_part_bindings() {
        let root = temp_root("direct-occt-multipart-manifest");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model)").expect("source");

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[
                ("base".to_string(), "Base".to_string()),
                ("post".to_string(), "Post".to_string()),
            ],
            &["width".to_string()],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");

        assert_eq!(manifest.document.object_count, 2);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "post"]
        );
        assert_eq!(
            manifest.parameter_groups[0].part_ids,
            vec!["base".to_string(), "post".to_string()]
        );
        assert!(manifest
            .parts
            .iter()
            .all(|part| part.viewer_asset_path.as_deref() == Some(MODEL_STL_FILE_NAME)));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_runtime_maps_topology_report_to_face_targets() {
        let root = temp_root("direct-occt-face-topology");
        let source_path = root.join(SOURCE_FILE_NAME);
        let preview_path = root.join(MODEL_STL_FILE_NAME);
        let step_path = root.join(STEP_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        fs::write(&preview_path, b"solid preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21;").expect("step");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                edges: Vec::new(),
                faces: vec![DirectOcctTopologyFace {
                    target_id: None,
                    face_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    center: Some(DirectOcctTopologyPoint {
                        x: 5.0,
                        y: 10.0,
                        z: 15.0,
                    }),
                    normal: Some([0.0, 0.0, 1.0]),
                    area: Some(200.0),
                    authored_bindings: Vec::new(),
                }],
                source_geometry_digest: None,
            }],
        };

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            &[],
            Some(&topology),
            &HashMap::from([(String::from("body"), 42_u64)]),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            "model-1",
            "hash",
            &source_path,
            &preview_path,
            &step_path,
            Some(&topology),
            &manifest,
        )
        .expect("bundle");

        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        let face_target_id = "body:face:5-10-15:200";
        let face_durable_id = "body:node:42:face:5-10-15:200";
        let face_alias_id = "body:face:0:5-10-15:200";
        assert!(manifest.selection_targets.iter().any(|target| {
            target.kind == SelectionTargetKind::Face
                && target.target_id.as_deref() == Some(face_target_id)
                && target.durable_target_id.as_deref() == Some(face_durable_id)
                && target.canonical_target_id.as_deref() == Some(face_alias_id)
                && target.alias_ids.is_empty()
        }));
        assert_eq!(bundle.face_targets.len(), 1);
        assert_eq!(bundle.face_targets[0].target_id, "body:face:5-10-15:200");
        assert_eq!(
            bundle.face_targets[0].durable_target_id.as_deref(),
            Some(face_durable_id)
        );
        assert_eq!(
            bundle.face_targets[0].canonical_target_id.as_deref(),
            Some(face_alias_id)
        );
        assert_eq!(
            bundle.face_targets[0].alias_ids,
            vec![face_alias_id.to_string(), face_durable_id.to_string()]
        );
        assert_eq!(bundle.face_targets[0].part_id, "body");
        assert_eq!(bundle.face_targets[0].viewer_node_id, "body");
        assert_eq!(bundle.face_targets[0].label, "Body.Face1");
        assert_eq!(bundle.face_targets[0].center.x, 5.0);
        assert_eq!(bundle.face_targets[0].normal, Some([0.0, 0.0, 1.0]));
        assert_eq!(bundle.face_targets[0].area, Some(200.0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_runtime_maps_exact_vertices_to_vertex_selection_targets() {
        let root = temp_root("direct-occt-vertex-topology");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: vec![DirectOcctTopologyVertex {
                    target_id: Some("body:vertex:0:0-0-0".to_string()),
                    vertex_index: Some(0),
                    label: "Body.Vertex1".to_string(),
                    point: Some(DirectOcctTopologyPoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    authored_bindings: Vec::new(),
                }],
                edges: Vec::new(),
                faces: Vec::new(),
                source_geometry_digest: Some("sha256:geometry".to_string()),
            }],
        };

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            Some(&topology),
            &HashMap::new(),
        )
        .expect("manifest");

        let vertex = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Vertex)
            .expect("vertex target");
        assert_eq!(vertex.part_id, "body");
        assert_eq!(vertex.target_id.as_deref(), Some("body:vertex:0-0-0"));
        assert_eq!(
            vertex.canonical_target_id.as_deref(),
            Some("body:vertex:0:0-0-0")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_part_geometry_digest_reader_is_exact_and_rejects_missing_identity() {
        let root = temp_root("direct-occt-part-geometry-digests");
        fs::create_dir_all(&root).expect("root");
        let topology_path = root.join(TOPOLOGY_FILE_NAME);
        fs::write(
            &topology_path,
            r#"{"schemaVersion":1,"parts":[{"partId":"body","sourceGeometryDigest":"sha256:body","authoredBindingEdgeOrder":[{"name":"outline","targetIds":["body:edge:0:0-0-0_1-0-0"]}],"vertices":[{"targetId":"body:vertex:0:0-0-0","authoredBindings":["base"],"exactGeometry":{"kind":"vertex","point":{"x":0,"y":0,"z":0}}}],"edges":[{"targetId":"body:edge:0:0-0-0_1-0-0","authoredBindings":["base"],"exactGeometry":{"kind":"lineEdge","start":{"x":0,"y":0,"z":0},"end":{"x":1,"y":0,"z":0}}}],"faces":[{"targetId":"body:face:0:0-0-0:1","authoredBindings":["base","top"],"exactGeometry":{"kind":"planeFace","origin":{"x":0,"y":0,"z":0},"normal":[0,0,1]}}]}]}"#,
        )
        .expect("topology");
        assert_eq!(
            direct_occt_part_source_geometry_digests(&topology_path).expect("digests"),
            std::collections::BTreeMap::from([("body".into(), "sha256:body".into())])
        );
        let exact = direct_occt_exact_target_geometries(&topology_path).expect("exact geometry");
        assert_eq!(
            exact.get("body:vertex:0:0-0-0"),
            Some(
                &crate::capture_brep_validation::ExactBrepTargetGeometry::Vertex {
                    point: [0.0, 0.0, 0.0]
                }
            )
        );
        assert!(matches!(
            exact.get("body:edge:0:0-0-0_1-0-0"),
            Some(crate::capture_brep_validation::ExactBrepTargetGeometry::LineEdge { .. })
        ));
        assert!(matches!(
            exact.get("body:face:0:0-0-0:1"),
            Some(crate::capture_brep_validation::ExactBrepTargetGeometry::PlaneFace { .. })
        ));
        let bindings =
            direct_occt_authored_binding_target_ids(&topology_path).expect("authored bindings");
        assert_eq!(
            bindings.get(&("body".into(), "base".into())),
            Some(&vec![
                "body:vertex:0:0-0-0".into(),
                "body:edge:0:0-0-0_1-0-0".into(),
                "body:face:0:0-0-0:1".into(),
            ])
        );
        assert_eq!(
            bindings.get(&("body".into(), "top".into())),
            Some(&vec!["body:face:0:0-0-0:1".into()])
        );
        let ordered = direct_occt_authored_binding_ordered_edge_target_ids(&topology_path)
            .expect("authored binding edge order");
        assert_eq!(
            ordered.get(&("body".into(), "outline".into())),
            Some(&vec!["body:edge:0:0-0-0_1-0-0".into()])
        );

        fs::write(
            &topology_path,
            r#"{"schemaVersion":1,"parts":[{"partId":"body","vertices":[],"edges":[],"faces":[]}]}"#,
        )
        .expect("topology");
        let error = direct_occt_part_source_geometry_digests(&topology_path)
            .expect_err("missing digest")
            .message;
        assert!(error.contains("identity is missing or invalid"), "{error}");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn guided_expected_solid_gate_rejects_open_invalid_or_unproven_parts() {
        let root = temp_root("direct-occt-guided-solid-validity");
        fs::create_dir_all(&root).expect("root");
        let topology_path = root.join(TOPOLOGY_FILE_NAME);

        fs::write(
            &topology_path,
            r#"{"schemaVersion":1,"parts":[{"partId":"body","solidCount":0,"brepValid":true}]}"#,
        )
        .expect("open topology");
        let error = validate_direct_occt_guided_expected_solids(&topology_path)
            .expect_err("surface-only result")
            .message;
        assert!(error.contains("no exact solid"), "{error}");

        fs::write(
            &topology_path,
            r#"{"schemaVersion":1,"parts":[{"partId":"body","solidCount":1,"brepValid":false}]}"#,
        )
        .expect("invalid topology");
        let error = validate_direct_occt_guided_expected_solids(&topology_path)
            .expect_err("invalid solid")
            .message;
        assert!(error.contains("failed exact BRep validity"), "{error}");

        fs::write(
            &topology_path,
            r#"{"schemaVersion":1,"parts":[{"partId":"body"}]}"#,
        )
        .expect("unproven topology");
        let error = validate_direct_occt_guided_expected_solids(&topology_path)
            .expect_err("missing proof")
            .message;
        assert!(error.contains("no solid-validity proof"), "{error}");

        fs::write(
            &topology_path,
            r#"{"schemaVersion":1,"parts":[{"partId":"body","solidCount":2,"brepValid":true}]}"#,
        )
        .expect("valid topology");
        validate_direct_occt_guided_expected_solids(&topology_path).expect("valid solids");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_runtime_maps_topology_report_to_edge_targets() {
        let root = temp_root("direct-occt-edge-topology");
        let source_path = root.join(SOURCE_FILE_NAME);
        let preview_path = root.join(MODEL_STL_FILE_NAME);
        let step_path = root.join(STEP_FILE_NAME);
        let topology_path = root.join(TOPOLOGY_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        fs::write(&preview_path, b"solid preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21;").expect("step");
        fs::write(
            &topology_path,
            r#"{"parts":[{"partId":"body","label":"Body","edges":[{"edgeIndex":0,"start":{"x":0.0,"y":0.0,"z":0.0},"end":{"x":10.0,"y":0.0,"z":0.0}}],"faces":[]}]}"#,
        )
        .expect("topology");
        let topology = read_direct_occt_topology_report(&topology_path).expect("read topology");

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            &[],
            Some(&topology),
            &HashMap::from([(String::from("body"), 42_u64)]),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            "model-1",
            "hash",
            &source_path,
            &preview_path,
            &step_path,
            Some(&topology),
            &manifest,
        )
        .expect("bundle");

        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        let edge_target_id = "body:edge:0-0-0_10-0-0";
        let edge_durable_id = "body:node:42:edge:0-0-0_10-0-0";
        let edge_alias_id = "body:edge:0:0-0-0_10-0-0";
        assert!(manifest.selection_targets.iter().any(|target| {
            target.kind == SelectionTargetKind::Edge
                && target.target_id.as_deref() == Some(edge_target_id)
                && target.durable_target_id.as_deref() == Some(edge_durable_id)
                && target.canonical_target_id.as_deref() == Some(edge_alias_id)
                && target.alias_ids.is_empty()
        }));
        assert_eq!(bundle.edge_targets.len(), 1);
        assert_eq!(bundle.edge_targets[0].target_id, "body:edge:0-0-0_10-0-0");
        assert_eq!(
            bundle.edge_targets[0].durable_target_id.as_deref(),
            Some(edge_durable_id)
        );
        assert_eq!(
            bundle.edge_targets[0].canonical_target_id.as_deref(),
            Some(edge_alias_id)
        );
        assert_eq!(
            bundle.edge_targets[0].alias_ids,
            vec![edge_alias_id.to_string(), edge_durable_id.to_string()]
        );
        assert_eq!(bundle.edge_targets[0].part_id, "body");
        assert_eq!(bundle.edge_targets[0].viewer_node_id, "body");
        assert_eq!(bundle.edge_targets[0].label, "Body.Edge1");
        assert_eq!(bundle.edge_targets[0].start.x, 0.0);
        assert_eq!(bundle.edge_targets[0].end.x, 10.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_manifest_records_exact_face_tagged_anchor_ids() {
        let root = temp_root("direct-occt-tagged-face-anchor");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                source_geometry_digest: None,
                edges: Vec::new(),
                faces: vec![DirectOcctTopologyFace {
                    target_id: None,
                    face_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    center: Some(DirectOcctTopologyPoint {
                        x: 5.0,
                        y: 10.0,
                        z: 15.0,
                    }),
                    normal: Some([0.0, 0.0, 1.0]),
                    area: Some(200.0),
                    authored_bindings: Vec::new(),
                }],
            }],
        };

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            &[CoreSelectorTagDecl {
                name: "mounting_top".to_string(),
                kind: CoreSelectorTagKind::Face,
                authored_selector: "target-id:body:face:5-10-15:200".to_string(),
                target: "body".to_string(),
            }],
            Some(&topology),
            &HashMap::from([(String::from("body"), 42_u64)]),
        )
        .expect("manifest");

        let anchor = manifest
            .tagged_anchors
            .get("mounting_top")
            .expect("tagged anchor");
        assert_eq!(anchor.kind, TaggedAnchorKind::Face);
        assert_eq!(anchor.authored_selector, "target-id:body:face:5-10-15:200");
        assert_eq!(anchor.target, "body");
        assert_eq!(anchor.target_ids, vec!["body:face:5-10-15:200".to_string()]);
        assert_eq!(
            anchor.durable_target_ids,
            vec!["body:node:42:face:5-10-15:200".to_string()]
        );
        assert_eq!(
            anchor.canonical_target_ids,
            vec!["body:face:0:5-10-15:200".to_string()]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_manifest_rebinds_created_by_face_tag_after_topology_change() {
        let root = temp_root("direct-occt-created-by-face-anchor");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(
            &source_path,
            "(model (part body (build (shape load_pad (box 10 20 30)) (result load_pad))))",
        )
        .expect("source");
        let selector_tags = [CoreSelectorTagDecl {
            name: "bottle_load".to_string(),
            kind: CoreSelectorTagKind::Face,
            authored_selector: "created-by:load_pad".to_string(),
            target: "body".to_string(),
        }];

        let render_manifest = |face_index, x, area| {
            let topology = DirectOcctTopologyReport {
                parts: vec![DirectOcctTopologyPart {
                    part_id: "body".to_string(),
                    label: "Body".to_string(),
                    vertices: Vec::new(),
                    source_geometry_digest: None,
                    edges: Vec::new(),
                    faces: vec![DirectOcctTopologyFace {
                        target_id: None,
                        face_index: Some(face_index),
                        originating_slot_index: None,
                        label: String::new(),
                        center: Some(DirectOcctTopologyPoint {
                            x,
                            y: 10.0,
                            z: 15.0,
                        }),
                        normal: Some([1.0, 0.0, 0.0]),
                        area: Some(area),
                        authored_bindings: vec![
                            "load_pad".to_string(),
                            "contact_surface".to_string(),
                        ],
                    }],
                }],
            };
            build_direct_occt_manifest(
                "model-1",
                &source_path,
                &[("body".to_string(), "Body".to_string())],
                &[],
                &selector_tags,
                Some(&topology),
                &HashMap::from([(String::from("body"), 42_u64)]),
            )
            .expect("manifest")
        };

        let before = render_manifest(5, 5.0, 200.0);
        let after = render_manifest(19, 7.0, 260.0);
        let before_anchor = before.tagged_anchors.get("bottle_load").expect("before");
        let after_anchor = after.tagged_anchors.get("bottle_load").expect("after");

        assert_eq!(before_anchor.authored_selector, "created-by:load_pad");
        assert_eq!(
            before_anchor.canonical_target_ids,
            ["body:face:5:5-10-15:200"]
        );
        assert_eq!(
            after_anchor.canonical_target_ids,
            ["body:face:19:7-10-15:260"]
        );
        assert_ne!(before_anchor.target_ids, after_anchor.target_ids);

        let combined_selector_tags = [CoreSelectorTagDecl {
            name: "combined_load".to_string(),
            kind: CoreSelectorTagKind::Face,
            authored_selector: "created-by:load_pad|contact_surface".to_string(),
            target: "body".to_string(),
        }];
        let combined_topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                source_geometry_digest: None,
                edges: Vec::new(),
                faces: vec![DirectOcctTopologyFace {
                    target_id: None,
                    face_index: Some(23),
                    originating_slot_index: None,
                    label: String::new(),
                    center: Some(DirectOcctTopologyPoint {
                        x: 8.0,
                        y: 10.0,
                        z: 15.0,
                    }),
                    normal: Some([1.0, 0.0, 0.0]),
                    area: Some(280.0),
                    authored_bindings: vec!["load_pad".to_string(), "contact_surface".to_string()],
                }],
            }],
        };
        let combined = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &combined_selector_tags,
            Some(&combined_topology),
            &HashMap::from([(String::from("body"), 42_u64)]),
        )
        .expect("combined manifest");
        assert_eq!(combined.tagged_anchors["combined_load"].target_ids.len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_manifest_records_exact_vertex_tagged_anchor_ids() {
        let root = temp_root("direct-occt-tagged-vertex-anchor");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: vec![DirectOcctTopologyVertex {
                    target_id: Some("body:vertex:0:0-0-0".to_string()),
                    vertex_index: Some(0),
                    label: "Body.Vertex1".to_string(),
                    point: Some(DirectOcctTopologyPoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    authored_bindings: Vec::new(),
                }],
                edges: Vec::new(),
                faces: Vec::new(),
                source_geometry_digest: Some("sha256:body".to_string()),
            }],
        };
        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[CoreSelectorTagDecl {
                name: "datum_origin".to_string(),
                kind: CoreSelectorTagKind::Vertex,
                authored_selector: "target-id:body:vertex:0:0-0-0".to_string(),
                target: "body".to_string(),
            }],
            Some(&topology),
            &HashMap::from([(String::from("body"), 42_u64)]),
        )
        .expect("manifest");

        let anchor = manifest
            .tagged_anchors
            .get("datum_origin")
            .expect("vertex anchor");
        assert_eq!(anchor.kind, TaggedAnchorKind::Vertex);
        assert_eq!(anchor.target_ids, vec!["body:vertex:0-0-0"]);
        assert_eq!(anchor.durable_target_ids, vec!["body:node:42:vertex:0-0-0"]);
        assert_eq!(anchor.canonical_target_ids, vec!["body:vertex:0:0-0-0"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_manifest_records_clause_face_tagged_anchor_ids() {
        let root = temp_root("direct-occt-tagged-clause-face-anchor");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                source_geometry_digest: None,
                edges: Vec::new(),
                faces: vec![
                    DirectOcctTopologyFace {
                        target_id: None,
                        face_index: Some(2),
                        originating_slot_index: None,
                        label: String::new(),
                        center: Some(DirectOcctTopologyPoint {
                            x: 5.0,
                            y: 10.0,
                            z: 0.0,
                        }),
                        normal: Some([0.0, 0.0, -1.0]),
                        area: Some(200.0),
                        authored_bindings: Vec::new(),
                    },
                    DirectOcctTopologyFace {
                        target_id: None,
                        face_index: Some(5),
                        originating_slot_index: None,
                        label: String::new(),
                        center: Some(DirectOcctTopologyPoint {
                            x: 5.0,
                            y: 10.0,
                            z: 15.0,
                        }),
                        normal: Some([0.0, 0.0, 1.0]),
                        area: Some(200.0),
                        authored_bindings: Vec::new(),
                    },
                ],
            }],
        };

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            &[CoreSelectorTagDecl {
                name: "mounting_top".to_string(),
                kind: CoreSelectorTagKind::Face,
                authored_selector: "top".to_string(),
                target: "body".to_string(),
            }],
            Some(&topology),
            &HashMap::from([(String::from("body"), 42_u64)]),
        )
        .expect("manifest");

        let anchor = manifest
            .tagged_anchors
            .get("mounting_top")
            .expect("tagged anchor");
        assert_eq!(anchor.kind, TaggedAnchorKind::Face);
        assert_eq!(anchor.authored_selector, "top");
        assert_eq!(anchor.target, "body");
        assert_eq!(anchor.target_ids, vec!["body:face:5-10-15:200".to_string()]);
        assert_eq!(
            anchor.durable_target_ids,
            vec!["body:node:42:face:5-10-15:200".to_string()]
        );
        assert_eq!(
            anchor.canonical_target_ids,
            vec!["body:face:5:5-10-15:200".to_string()]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_runtime_read_direct_occt_topology_report_fails_without_file() {
        let root = temp_root("direct-occt-missing-topology");
        let topology_path = root.join(TOPOLOGY_FILE_NAME);
        let err = read_direct_occt_topology_report(&topology_path)
            .expect_err("missing topology should fail");
        assert!(
            err.to_string()
                .contains("Direct OCCT topology report could not be read"),
            "{err}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_runtime_read_direct_occt_topology_report_fails_with_invalid_json() {
        let root = temp_root("direct-occt-invalid-topology");
        let topology_path = root.join(TOPOLOGY_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&topology_path, "{ \"parts\": ").expect("invalid topology");

        let err = read_direct_occt_topology_report(&topology_path)
            .expect_err("invalid topology should fail");
        assert!(
            err.to_string()
                .contains("Direct OCCT topology report invalid"),
            "{err}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_runtime_read_direct_occt_topology_report_reads_originating_slot_indexes() {
        let root = temp_root("direct-occt-topology-originating-slot-indexes");
        let topology_path = root.join(TOPOLOGY_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(
            &topology_path,
            r#"{
  "parts": [
    {
      "partId": "body",
      "label": "Body",
      "edges": [
        {
          "edgeIndex": 0,
          "originatingSlotIndex": 7,
          "label": "Body.Edge1",
          "start": { "x": 0, "y": 0, "z": 0 },
          "end": { "x": 10, "y": 0, "z": 0 }
        }
      ],
      "faces": [
        {
          "faceIndex": 1,
          "originatingSlotIndex": 9,
          "label": "Body.Face2",
          "center": { "x": 5, "y": 5, "z": 10 },
          "normal": [0, 0, 1],
          "area": 100
        }
      ]
    }
  ]
}"#,
        )
        .expect("topology");

        let topology = read_direct_occt_topology_report(&topology_path).expect("read topology");

        assert_eq!(topology.parts[0].edges[0].originating_slot_index, Some(7));
        assert_eq!(topology.parts[0].faces[0].originating_slot_index, Some(9));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_authored_face_binding_maps_only_named_shape_parameter_keys() {
        let source = r#"
            (model
              (params
                (number width 20 :label "Width")
                (number bore_diameter 4 :label "Bore Diameter"))
              (part body
                (build
                  (shape base (box width 20 10))
                  (shape bore (cylinder (/ bore_diameter 2) 10))
                  (result (difference base bore)))))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let provenance = direct_occt_program_provenance(&program);
        let root = temp_root("direct-occt-authored-face-parameter-keys");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, source).expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                edges: Vec::new(),
                faces: vec![DirectOcctTopologyFace {
                    target_id: Some("body:face:0:5-5-5:25".to_string()),
                    face_index: Some(0),
                    originating_slot_index: None,
                    label: "Body.Face1".to_string(),
                    center: Some(DirectOcctTopologyPoint {
                        x: 5.0,
                        y: 5.0,
                        z: 5.0,
                    }),
                    normal: Some([0.0, 0.0, 1.0]),
                    area: Some(25.0),
                    authored_bindings: vec!["bore".to_string()],
                }],
                source_geometry_digest: None,
            }],
        };
        let parts = vec![("body".to_string(), "Body".to_string())];
        let mut manifest = build_direct_occt_manifest_with_program_provenance(
            "model-1",
            &source_path,
            &parts,
            &["width".to_string(), "bore_diameter".to_string()],
            &[],
            Some(&topology),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &provenance,
        )
        .expect("manifest");
        apply_direct_occt_program_provenance(&mut manifest, &program, &provenance);

        let face = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .expect("face");
        assert_eq!(face.parameter_keys, ["bore_diameter"]);
        assert!(face.editable);
        assert_eq!(manifest.parts[0].parameter_keys, ["bore_diameter", "width"]);
        assert!(manifest.parameter_groups.iter().any(|group| {
            group.group_id == "shape:body:bore" && group.parameter_keys == ["bore_diameter"]
        }));
        let feature_graph = manifest.feature_graph.as_ref().expect("feature graph");
        assert!(feature_graph.nodes.iter().any(|node| {
            node.feature_id == "part:body" && node.dependency_ids == ["bore_diameter", "width"]
        }));
        assert!(feature_graph.nodes.iter().any(|node| {
            node.feature_id == "shape:body:bore" && node.dependency_ids == ["bore_diameter"]
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_provenance_marks_whole_part_mesh_bridge_roots() {
        let source = r#"(model
          (part relief (solidify (import-stl "/tmp/relief.stl")))
          (part handle (box 20 20 20)))"#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let provenance = direct_occt_program_provenance(&program);

        assert!(provenance["relief"].faceted_mesh_root);
        assert!(!provenance["handle"].faceted_mesh_root);
    }

    #[test]
    fn direct_occt_selection_targets_skip_whole_part_mesh_bridge_facets() {
        let source = r#"(model
          (part relief (solidify (import-stl "/tmp/relief.stl")))
          (part handle (box 20 20 20)))"#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let provenance = direct_occt_program_provenance(&program);
        let parts = vec![
            ("relief".to_string(), "Relief".to_string()),
            ("handle".to_string(), "Handle".to_string()),
        ];
        let bindings = direct_occt_part_bindings_with_provenance(
            &parts,
            &provenance,
            &HashMap::new(),
            &HashMap::new(),
        );
        let face = |index| DirectOcctTopologyFace {
            target_id: None,
            face_index: Some(index),
            originating_slot_index: None,
            label: String::new(),
            center: Some(DirectOcctTopologyPoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            normal: Some([0.0, 0.0, 1.0]),
            area: Some(1.0),
            authored_bindings: Vec::new(),
        };
        let topology = DirectOcctTopologyReport {
            parts: vec![
                DirectOcctTopologyPart {
                    part_id: "relief".to_string(),
                    label: "Relief".to_string(),
                    vertices: Vec::new(),
                    edges: Vec::new(),
                    faces: vec![face(0)],
                    source_geometry_digest: None,
                },
                DirectOcctTopologyPart {
                    part_id: "handle".to_string(),
                    label: "Handle".to_string(),
                    vertices: Vec::new(),
                    edges: Vec::new(),
                    faces: vec![face(0)],
                    source_geometry_digest: None,
                },
            ],
        };

        let targets = direct_occt_selection_targets(
            &bindings,
            Some(&topology),
            &HashMap::new(),
            &HashMap::new(),
            &provenance,
        )
        .expect("selection targets");

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].part_id, "handle");
    }

    #[test]
    fn direct_occt_program_provenance_keeps_component_named_shape_dependencies() {
        let source = r#"
            (define-component dryer-shell
              ((number shell_width) (number bore_diameter))
              (build
                (shape base (box shell_width 20 10))
                (shape bore (cylinder (/ bore_diameter 2) 10))
                (result (difference base bore))))
            (model
              (params
                (number width 40 :label "Width")
                (number bore 6 :label "Bore"))
              (part body (dryer-shell :shell_width width :bore_diameter bore)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let provenance = direct_occt_program_provenance(&program);
        let body = provenance.get("body").expect("body provenance");

        assert_eq!(body.parameter_keys, ["bore", "width"]);
        assert_eq!(
            body.named_shapes
                .iter()
                .find(|(name, _)| name == "base")
                .map(|(_, keys)| keys.clone()),
            Some(vec!["width".to_string()])
        );
        assert_eq!(
            body.named_shapes
                .iter()
                .find(|(name, _)| name == "bore")
                .map(|(_, keys)| keys.clone()),
            Some(vec!["bore".to_string()])
        );
    }

    #[test]
    fn direct_occt_selection_targets_remain_non_editable_without_exact_binding() {
        let root = temp_root("direct-occt-non-editable-targets");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model)").expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                source_geometry_digest: None,
                edges: vec![DirectOcctTopologyEdge {
                    target_id: None,
                    edge_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    start: Some(DirectOcctTopologyPoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    end: Some(DirectOcctTopologyPoint {
                        x: 10.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    authored_bindings: Vec::new(),
                }],
                faces: vec![DirectOcctTopologyFace {
                    target_id: None,
                    face_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    center: Some(DirectOcctTopologyPoint {
                        x: 5.0,
                        y: 5.0,
                        z: 5.0,
                    }),
                    normal: Some([0.0, 0.0, 1.0]),
                    area: Some(100.0),
                    authored_bindings: Vec::new(),
                }],
            }],
        };

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &["width".to_string()],
            &[],
            Some(&topology),
            &HashMap::new(),
        )
        .expect("manifest");

        assert!(
            manifest
                .selection_targets
                .iter()
                .all(|target| !target.editable),
            "selection targets should be non-editable"
        );
        assert!(
            manifest
                .selection_targets
                .iter()
                .any(|target| target.parameter_keys.is_empty()),
            "selection targets should not require param locks"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_multi_param_topology_targets_have_empty_parameter_keys() {
        let root = temp_root("direct-occt-multi-param-topology-keys");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model)").expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                source_geometry_digest: None,
                edges: vec![DirectOcctTopologyEdge {
                    target_id: None,
                    edge_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    start: Some(DirectOcctTopologyPoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    end: Some(DirectOcctTopologyPoint {
                        x: 10.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    authored_bindings: Vec::new(),
                }],
                faces: vec![DirectOcctTopologyFace {
                    target_id: None,
                    face_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    center: Some(DirectOcctTopologyPoint {
                        x: 5.0,
                        y: 5.0,
                        z: 5.0,
                    }),
                    normal: Some([0.0, 0.0, 1.0]),
                    area: Some(100.0),
                    authored_bindings: Vec::new(),
                }],
            }],
        };

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &["width".to_string(), "height".to_string()],
            &[],
            Some(&topology),
            &HashMap::new(),
        )
        .expect("manifest");

        assert_eq!(
            manifest.parts[0].parameter_keys,
            vec!["width".to_string(), "height".to_string()]
        );
        let edge = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .expect("edge target");
        let face = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .expect("face target");
        assert!(edge.parameter_keys.is_empty());
        assert!(!edge.editable);
        assert!(face.parameter_keys.is_empty());
        assert!(!face.editable);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_single_param_topology_targets_still_require_exact_binding() {
        let root = temp_root("direct-occt-single-param-topology-key");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model)").expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                source_geometry_digest: None,
                edges: vec![DirectOcctTopologyEdge {
                    target_id: None,
                    edge_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    start: Some(DirectOcctTopologyPoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    end: Some(DirectOcctTopologyPoint {
                        x: 10.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    authored_bindings: Vec::new(),
                }],
                faces: vec![DirectOcctTopologyFace {
                    target_id: None,
                    face_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    center: Some(DirectOcctTopologyPoint {
                        x: 5.0,
                        y: 5.0,
                        z: 5.0,
                    }),
                    normal: Some([0.0, 0.0, 1.0]),
                    area: Some(100.0),
                    authored_bindings: Vec::new(),
                }],
            }],
        };

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &["width".to_string()],
            &[],
            Some(&topology),
            &HashMap::new(),
        )
        .expect("manifest");

        assert_eq!(manifest.parts[0].parameter_keys, vec!["width".to_string()]);
        let edge = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .expect("edge target");
        let face = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .expect("face target");
        assert!(edge.parameter_keys.is_empty());
        assert!(!edge.editable);
        assert!(face.parameter_keys.is_empty());
        assert!(!face.editable);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_manifest_prefers_stable_node_key_for_durable_topology_ids() {
        let root = temp_root("direct-occt-stable-node-key-topology");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                vertices: Vec::new(),
                source_geometry_digest: None,
                edges: vec![DirectOcctTopologyEdge {
                    target_id: None,
                    edge_index: Some(0),
                    originating_slot_index: None,
                    label: String::new(),
                    start: Some(DirectOcctTopologyPoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    end: Some(DirectOcctTopologyPoint {
                        x: 10.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    authored_bindings: Vec::new(),
                }],
                faces: Vec::new(),
            }],
        };

        let manifest = build_direct_occt_manifest_with_stable_node_keys(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            &[],
            Some(&topology),
            &HashMap::from([("body".to_string(), "sha256:abcdef".to_string())]),
            &HashMap::from([(String::from("body"), 42_u64)]),
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("manifest");

        assert!(manifest.selection_targets.iter().any(|target| {
            target.kind == SelectionTargetKind::Edge
                && target.durable_target_id.as_deref()
                    == Some("body:stable-node-key:sha256:abcdef:edge:0-0-0_10-0-0")
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_source_stable_node_key_ignores_prior_traversal_allocations() {
        let base_source = "(model (part body (box 10 20 30)))";
        let shifted_source = "(model (part spacer (box 1 1 1)) (part body (box 10 20 30)))";
        let base_program = compile(base_source);
        let shifted_program = compile(shifted_source);
        let base_part = base_program
            .parts
            .iter()
            .find(|part| part.key == "body")
            .expect("base part");
        let shifted_part = shifted_program
            .parts
            .iter()
            .find(|part| part.key == "body")
            .expect("shifted part");

        assert_ne!(base_part.root.id.raw(), shifted_part.root.id.raw());
        assert_eq!(
            direct_occt_source_stable_node_key(base_source, base_part),
            direct_occt_source_stable_node_key(shifted_source, shifted_part)
        );
    }

    #[test]
    fn direct_occt_edge_target_id_sorts_endpoint_signature() {
        let forward = DirectOcctTopologyEdge {
            target_id: None,
            edge_index: Some(0),
            originating_slot_index: None,
            label: String::new(),
            start: Some(DirectOcctTopologyPoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            end: Some(DirectOcctTopologyPoint {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            }),
            authored_bindings: Vec::new(),
        };
        let reversed = DirectOcctTopologyEdge {
            target_id: None,
            edge_index: Some(0),
            originating_slot_index: None,
            label: String::new(),
            start: forward.end.clone(),
            end: forward.start.clone(),
            authored_bindings: Vec::new(),
        };

        assert_eq!(
            direct_occt_edge_target_id("body", &forward),
            "body:edge:0:0-0-0_10-0-0"
        );
        assert_eq!(
            direct_occt_stable_edge_target_id("body:edge:0:0-0-0_10-0-0"),
            "body:edge:0-0-0_10-0-0"
        );
        assert_eq!(
            direct_occt_edge_target_id("body", &forward),
            direct_occt_edge_target_id("body", &reversed)
        );
    }

    #[test]
    fn blocked_direct_occt_sdk_returns_error_without_bundle() {
        let root = temp_root("direct-occt-blocked");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let program = compile(source);

        let result = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &blocked_layout(root.clone()),
            &resolver,
        );

        // Runner-first export legitimately bypasses a blocked SDK when a
        // precompiled runner is discoverable (dev machines with a built
        // `.dist/runtime/occt`). Only without a runner must the blocked SDK
        // surface as an error with no bundle left behind.
        let runner_available =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &resolver, true,
            )
            .is_some();
        if runner_available {
            result.expect("runner-first export bypasses blocked SDK");
        } else {
            let err = result.expect_err("blocked runtime");
            assert!(
                err.to_string().contains("Direct OCCT runtime blocked"),
                "{err}"
            );
            let bundle_dir =
                crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
            assert!(!bundle_dir.exists());
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_export_errors_remove_partial_bundle_dir() {
        let root = temp_root("direct-occt-export-error");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model)";
        let program = CoreProgram::new(
            crate::ecky_core_ir::ProgramId::new(1),
            Vec::new(),
            Vec::new(),
        );
        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params");
        let hash = content_hash_with_runtime_inputs(&program, source, &params_json, &params, None)
            .expect("content hash");
        let model_id = model_id_from_hash(&hash);

        let err = render_core_program_runtime_bundle(
            &program,
            source,
            &params,
            &blocked_layout(root.clone()),
            &resolver,
        )
        .expect_err("invalid program");

        assert!(
            err.to_string().contains("requires one part") || err.to_string().contains("part"),
            "{err}"
        );
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        assert!(!bundle_dir.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_model_id_includes_parameter_values() {
        let source = "(model (params (number width 10)) (part body (box width 12 14)))";
        let params_a = DesignParams::from([("width".to_string(), ParamValue::Number(10.0))]);
        let params_b = DesignParams::from([("width".to_string(), ParamValue::Number(24.0))]);
        let params_a_json = serde_json::to_string(&params_a).expect("params a");
        let params_b_json = serde_json::to_string(&params_b).expect("params b");

        let model_id_a = model_id_from_hash(&content_hash(source, &params_a_json));
        let model_id_b = model_id_from_hash(&content_hash(source, &params_b_json));

        assert_ne!(model_id_a, model_id_b);
    }

    #[test]
    fn direct_occt_model_id_includes_explicit_font_path() {
        let source = "(model (part body (text \"A\" 12)))";
        let params_json = serde_json::to_string(&DesignParams::new()).expect("params");

        let default_model_id = model_id_from_hash(&content_hash(source, &params_json));
        let configured_font_model_id = model_id_from_hash(&content_hash_with_font_path(
            source,
            &params_json,
            Some("/tmp/fonts/technical.ttf"),
        ));

        assert_ne!(default_model_id, configured_font_model_id);
    }

    #[test]
    fn live_direct_occt_runtime_writes_bundle_manifest_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-bundle");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (union (box 10 10 10) (translate 8 0 0 (sphere 4)))))";
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT runtime bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        assert!(bundle_dir.join(TOPOLOGY_FILE_NAME).is_file());
        assert!(
            std::fs::metadata(&bundle.model_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert!(
            std::fs::metadata(bundle_dir.join(TOPOLOGY_FILE_NAME))
                .expect("topology")
                .len()
                > 16
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Vertex));
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Edge));
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Face));
        let exact_geometries =
            direct_occt_exact_target_geometries(&bundle_dir.join(TOPOLOGY_FILE_NAME))
                .expect("exact target geometries");
        assert!(exact_geometries.values().any(|geometry| matches!(
            geometry,
            crate::capture_brep_validation::ExactBrepTargetGeometry::Vertex { .. }
        )));
        assert!(exact_geometries.values().any(|geometry| matches!(
            geometry,
            crate::capture_brep_validation::ExactBrepTargetGeometry::LineEdge { .. }
        )));
        assert!(exact_geometries.values().any(|geometry| matches!(
            geometry,
            crate::capture_brep_validation::ExactBrepTargetGeometry::PlaneFace { .. }
        )));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_quarter_guided_source_uses_two_mirrors_and_diagnostics_leave_artifacts_immutable() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let source = r#"(model
          (params (number insert_depth 2 :label "Insert depth" :min 1 :max 10 :step 0.5))
          (tag-face support-face :faces "bottom" part-1)
          (part part-1 (build
            (shape quarter (translate 5 5 0 (box 10 10 insert_depth)))
            (shape x-half (union quarter (mirror "x" 0 quarter)))
            (result (union x-half (mirror "y" 0 x-half))))))"#;
        assert_eq!(source.matches("(box ").count(), 1, "one authored quarter");
        assert_eq!(
            source.matches("(mirror ").count(),
            2,
            "explicit X/Y completion"
        );

        let mut guide = crate::contracts::CaptureReconstructionGuide::test_fixture();
        guide.symmetry_completion = crate::contracts::CaptureSymmetryCompletion::Quarter {
            first_plane_id: "plane-x".into(),
            second_plane_id: "plane-y".into(),
        };
        let symmetry_fit = crate::contracts::CaptureFitResidual {
            rms_mm: 0.0,
            max_mm: 0.0,
            tolerance_mm: 0.1,
        };
        guide.planes.extend([
            crate::contracts::CaptureNamedPlane {
                plane_id: "plane-x".into(),
                label: "X symmetry".into(),
                role: crate::contracts::CapturePlaneRole::Symmetry,
                landmark_ids: vec![
                    "landmark-1".into(),
                    "landmark-2".into(),
                    "landmark-3".into(),
                ],
                origin_mm: [0.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                fit: symmetry_fit.clone(),
            },
            crate::contracts::CaptureNamedPlane {
                plane_id: "plane-y".into(),
                label: "Y symmetry".into(),
                role: crate::contracts::CapturePlaneRole::Symmetry,
                landmark_ids: vec![
                    "landmark-1".into(),
                    "landmark-2".into(),
                    "landmark-3".into(),
                ],
                origin_mm: [0.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                fit: symmetry_fit,
            },
        ]);
        guide.feature_expectations[0].expected_authored_selector =
            crate::contracts::CaptureAuthoredSelector::Tag {
                name: "support-face".into(),
            };
        guide.feature_expectations[0].guide_item_ids = vec!["profile-1".into(), "plane-1".into()];
        guide.feature_expectations[0].cardinality =
            crate::contracts::CaptureSelectorCardinality::OneOrMore;
        guide
            .measurements
            .push(crate::contracts::CaptureNamedMeasurement {
                measurement_id: "depth".into(),
                label: "insert depth".into(),
                landmark_ids: Vec::new(),
                value: 2.0,
                unit: "mm".into(),
                fit_critical: true,
                authored_parameter_name: Some("insert_depth".into()),
                constraint_kind: Some(crate::contracts::CaptureConstraintKind::Extent),
            });
        guide.reconstructed_profiles = vec![crate::contracts::CaptureReconstructedProfile {
            candidate_id: "profile-candidate:profile-1:polyline".into(),
            source_profile_id: "profile-1".into(),
            support_plane_id: "plane-1".into(),
            segments: [
                ("landmark-1", "landmark-2", [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ("landmark-2", "landmark-3", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                ("landmark-3", "landmark-1", [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (start_id, end_id, start_mm, end_mm))| {
                crate::contracts::CaptureProfileSegment {
                    segment_id: format!("profile-1:segment:{index}"),
                    source_landmark_ids: vec![start_id.into(), end_id.into()],
                    neighborhood_ids: Vec::new(),
                    parameter_range: [index as f64, index as f64 + 1.0],
                    geometry: crate::contracts::CaptureProfileSegmentGeometry::Line {
                        start_mm,
                        end_mm,
                    },
                    fit: crate::contracts::CaptureFitResidual {
                        rms_mm: 0.0,
                        max_mm: 0.0,
                        tolerance_mm: 0.1,
                    },
                }
            })
            .collect(),
            closed: true,
            continuous: true,
            closure_error_mm: 0.0,
            maximum_continuity_gap_mm: 0.0,
            support_plane_max_mm: 0.0,
            supporting_evidence_ids: vec!["profile-1".into()],
            rejected_hypotheses: Vec::new(),
        }];
        guide.feature_plan_candidates = vec![crate::contracts::CaptureFeaturePlanCandidate {
            plan_id: "plan:quarter-insert".into(),
            label: "Quarter insert extrusion and symmetry".into(),
            operations: vec![crate::contracts::CaptureFeatureOperation::Extrude {
                profile_candidate_id: "profile-candidate:profile-1:polyline".into(),
                distance_dimension_id: "depth".into(),
            }],
            supporting_evidence_ids: vec!["profile-1".into(), "depth".into()],
            rejecting_evidence: Vec::new(),
            score: 1.0,
            status: crate::contracts::CaptureFeaturePlanStatus::Supported,
        }];
        guide.selected_feature_plan_id = Some("plan:quarter-insert".into());
        guide.constraint_graph = crate::capture_guidance::build_capture_constraint_graph(&guide)
            .expect("quarter constraint graph");
        crate::capture_brep_validation::validate_capture_guided_source_semantics(&guide, source)
            .expect("quarter source semantics");
        let copied_source = r#"(model
          (params (number insert_depth 2 :label "Insert depth" :min 1 :max 10 :step 0.5))
          (part part-1 (build
            (shape quarter (translate 5 5 0 (box 10 10 insert_depth)))
            (shape copied-2 (translate -5 5 0 (box 10 10 insert_depth)))
            (shape copied-3 (translate 5 -5 0 (box 10 10 insert_depth)))
            (shape copied-4 (translate -5 -5 0 (box 10 10 insert_depth)))
            (result (union quarter copied-2 copied-3 copied-4)))))"#;
        let copied_error =
            crate::capture_brep_validation::validate_capture_guided_source_semantics(
                &guide,
                copied_source,
            )
            .expect_err("copied quarters must not satisfy explicit symmetry")
            .message;
        assert!(
            copied_error.contains("requires 2 explicit mirror"),
            "{copied_error}"
        );

        let root = temp_root("quarter-guided-artifact-invariance");
        let resolver = TestResolver { root: root.clone() };
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let (bundle, mut manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("quarter exact OCCT bundle");
        manifest.source_digest = Some(crate::services::render_snapshot::canonical_source_digest(
            source,
        ));
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("bundle dir");
        validate_direct_occt_guided_expected_solids(&bundle_dir.join(TOPOLOGY_FILE_NAME))
            .expect("quarter result is valid solid");

        let design = crate::contracts::DesignOutput {
            title: "Quarter insert".into(),
            version_name: "V1".into(),
            response: String::new(),
            interaction_mode: crate::contracts::InteractionMode::Design,
            macro_code: source.into(),
            macro_dialect: crate::contracts::MacroDialect::EckyIrV0,
            engine_kind: bundle.engine_kind,
            source_language: bundle.source_language,
            geometry_backend: bundle.geometry_backend,
            ui_spec: crate::contracts::UiSpec::default(),
            initial_params: crate::contracts::DesignParams::new(),
            post_processing: None,
        };
        let snapshot = crate::services::render_snapshot::build_render_snapshot(
            crate::services::render_snapshot::RenderSnapshotInput {
                design: &design,
                effective_params: &crate::contracts::DesignParams::new(),
                artifact_bundle: &bundle,
                model_manifest: &manifest,
            },
        )
        .expect("exact quarter snapshot");
        guide.target_source_digest = snapshot.source_digest.clone();
        guide.canonical_digest = guide.compute_canonical_digest().expect("guide digest");
        let exact_provenance =
            crate::capture_brep_validation::validate_capture_direct_occt_snapshot(
                &guide,
                &snapshot,
                &bundle_dir.join(TOPOLOGY_FILE_NAME),
            )
            .expect("selected plan exact runtime trace");
        assert_eq!(
            exact_provenance.selected_feature_plan_id.as_deref(),
            Some("plan:quarter-insert")
        );
        assert_eq!(exact_provenance.feature_operation_traces.len(), 1);
        assert!(!exact_provenance.feature_operation_traces[0]
            .authored_node_keys
            .is_empty());
        assert!(!exact_provenance.feature_operation_traces[0]
            .brep_target_ids
            .is_empty());

        let preview_path = Path::new(&bundle.model_stl_path);
        let step_path = Path::new(&bundle.export_artifacts[0].path);
        let preview_before = fs::read(preview_path).expect("preview bytes");
        let step_before = fs::read(step_path).expect("STEP bytes");
        let artifact_digest_before =
            crate::services::render_snapshot::artifact_bundle_digest(&bundle)
                .expect("artifact digest");
        guide.source_mesh.content_digest =
            crate::capture_guidance::source_mesh_content_digest(preview_path)
                .expect("preview mesh digest");
        guide.canonical_digest = guide.compute_canonical_digest().expect("guide digest");
        let boundary =
            crate::ecky_cad_host::analysis_boundary::load_direct_occt_analysis_boundary_surface(
                &bundle_dir,
                "part-1",
            )
            .expect("exact analysis boundary");
        let report = crate::capture_deviation::compute_observed_mesh_to_brep_deviation(
            preview_path,
            &guide,
            &boundary,
            &artifact_digest_before,
            128,
            0.1,
        )
        .expect("display-only deviation");
        assert_eq!(
            report.generated_geometry_digest,
            exact_provenance.geometry_digest
        );
        assert_eq!(report.evidence_scope, "observedRegionOnly");
        assert_eq!(
            fs::read(preview_path).expect("preview after"),
            preview_before
        );
        assert_eq!(fs::read(step_path).expect("STEP after"), step_before);
        assert_eq!(
            crate::services::render_snapshot::artifact_bundle_digest(&bundle)
                .expect("artifact digest after"),
            artifact_digest_before
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_writes_bundle_manifest_from_standalone_occt_sdk_when_available() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }

        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-standalone-live-bundle");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 10 10)))";
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT standalone SDK bundle");

        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(bundle_dir.join(TOPOLOGY_FILE_NAME).is_file());
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_some()
        {
            assert!(
                !bundle_dir.join("direct_occt_executor.cpp").exists(),
                "runner-first export should not emit precompiled runner"
            );
        }

        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_exact_edge_target_id_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-exact-edge-target-id");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (base_bundle, _) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let edge_target_id = base_bundle
            .edge_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box edge target");
        let drifted_edge_target_id = edge_target_id.replacen(":edge:0:", ":edge:999:", 1);
        assert_ne!(drifted_edge_target_id, edge_target_id);

        let exact_source = format!(
            r#"(model (part body (fillet 1.5 :edges "target-id:{drifted_edge_target_id}" (box 20 20 10))))"#
        );
        let exact_program = compile(&exact_source);
        let exact_hash = content_hash(&exact_source, "{}");
        let exact_model_id = model_id_from_hash(&exact_hash);
        let (bundle, manifest) = render_core_program_runtime_bundle(
            &exact_program,
            &exact_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT exact-target fillet bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(
            std::fs::metadata(&bundle.model_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        assert!(
            edge_target_id.starts_with("body:edge:"),
            "unexpected edge target id: {edge_target_id}"
        );
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &exact_model_id).expect("dir");
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_some()
        {
            assert!(bundle_dir.join("plan.json").is_file());
            assert!(!bundle_dir.join("direct_occt_executor.cpp").exists());
        }
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_exact_edge_target_id_for_chamfer_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-exact-edge-target-id-chamfer");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (base_bundle, _) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let edge_target_id = base_bundle
            .edge_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box edge target");
        let drifted_edge_target_id = edge_target_id.replacen(":edge:0:", ":edge:999:", 1);
        assert_ne!(drifted_edge_target_id, edge_target_id);

        let exact_source = format!(
            r#"(model (part body (chamfer 1.25 :edges "target-id:{drifted_edge_target_id}" (box 20 20 10))))"#
        );
        let exact_program = compile(&exact_source);
        let exact_hash = content_hash(&exact_source, "{}");
        let exact_model_id = model_id_from_hash(&exact_hash);
        let (bundle, manifest) = render_core_program_runtime_bundle(
            &exact_program,
            &exact_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT exact-target chamfer bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert_eq!(manifest.parts[0].part_id, "body");
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &exact_model_id).expect("dir");
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_some()
        {
            assert!(bundle_dir.join("plan.json").is_file());
            assert!(!bundle_dir.join("direct_occt_executor.cpp").exists());
        }
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_coarse_edge_selector_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-coarse-edge-selector");
        let resolver = TestResolver { root: root.clone() };
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_none()
        {
            return;
        }

        let source = r#"(model (part body (fillet 1.5 :edges "left+vertical" (box 20 20 10))))"#;
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT coarse selector bundle");

        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(bundle_dir.join(TOPOLOGY_FILE_NAME).is_file());
        assert!(
            bundle_dir.join("plan.json").is_file(),
            "runner-first export should persist runner plan"
        );
        assert!(
            !bundle_dir.join("direct_occt_executor.cpp").exists(),
            "runner-first coarse selector export should not emit precompiled runner"
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_edge_all_selector_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-edge-all-selector");
        let resolver = TestResolver { root: root.clone() };
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_none()
        {
            return;
        }

        let source = r#"(model (part body (fillet 1.5 :edges "all" (box 20 20 10))))"#;
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT edge-all selector bundle");

        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(bundle_dir.join(TOPOLOGY_FILE_NAME).is_file());
        assert!(
            bundle_dir.join("plan.json").is_file(),
            "runner-first export should persist runner plan"
        );
        assert!(
            !bundle_dir.join("direct_occt_executor.cpp").exists(),
            "runner-first edge-all export should not emit precompiled runner"
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_shell_clause_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-shell-clause-selector");
        let resolver = TestResolver { root: root.clone() };
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_none()
        {
            return;
        }

        let source =
            r#"(model (part body (shell 1.5 :faces "planar+normal-z+area-max" (box 20 20 10))))"#;
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT shell clause bundle");

        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(bundle_dir.join(TOPOLOGY_FILE_NAME).is_file());
        assert!(
            bundle_dir.join("plan.json").is_file(),
            "runner-first export should persist runner plan"
        );
        assert!(
            !bundle_dir.join("direct_occt_executor.cpp").exists(),
            "runner-first shell clause export should not emit precompiled runner"
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_keywordless_shell_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-shell-default");
        let resolver = TestResolver { root: root.clone() };
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_none()
        {
            return;
        }

        let source = r#"(model (part body (shell 1.5 (box 20 20 10))))"#;
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT shell default bundle");

        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(bundle_dir.join(TOPOLOGY_FILE_NAME).is_file());
        assert!(
            bundle_dir.join("plan.json").is_file(),
            "runner-first export should persist runner plan"
        );
        assert!(
            !bundle_dir.join("direct_occt_executor.cpp").exists(),
            "runner-first shell default export should not emit precompiled runner"
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_exports_xor_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-xor");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"
            (model
              (part body
                (xor
                  (box 20 20 10)
                  (translate 10 0 0 (box 20 20 10)))))
        "#;
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT xor bundle");

        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(bundle_dir.join(TOPOLOGY_FILE_NAME).is_file());
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_some()
        {
            assert!(bundle_dir.join("plan.json").is_file());
            assert!(!bundle_dir.join("direct_occt_executor.cpp").exists());
        }
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_coarse_chamfer_selector_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-coarse-chamfer-selector");
        let resolver = TestResolver { root: root.clone() };
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_none()
        {
            return;
        }

        let source = r#"(model (part body (chamfer 1.25 :edges "left+vertical" (box 20 20 10))))"#;
        let program = compile(source);
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT coarse chamfer selector bundle");

        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(bundle_dir.join(TOPOLOGY_FILE_NAME).is_file());
        assert!(
            bundle_dir.join("plan.json").is_file(),
            "runner-first export should persist runner plan"
        );
        assert!(
            !bundle_dir.join("direct_occt_executor.cpp").exists(),
            "runner-first coarse chamfer export should not emit precompiled runner"
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_exact_edge_alias_target_id_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-exact-edge-alias-target-id");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (_base_bundle, base_manifest) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let edge_alias_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box edge alias target");

        let exact_source = format!(
            r#"(model (part body (fillet 1.5 :edges "target-id:{edge_alias_target_id}" (box 20 20 10))))"#
        );
        let exact_program = compile(&exact_source);
        let (bundle, manifest) = render_core_program_runtime_bundle(
            &exact_program,
            &exact_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT exact-alias fillet bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_stable_and_durable_edge_alias_target_id_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-stable-durable-edge-alias-target-id");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (_base_bundle, base_manifest) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let edge_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .and_then(|target| target.target_id.clone())
            .expect("box edge stable target");
        let edge_durable_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .and_then(|target| target.durable_target_id.clone())
            .expect("box edge durable target");

        for requested_edge_target_id in [edge_target_id, edge_durable_target_id] {
            let exact_source = format!(
                r#"(model (part body (fillet 1.5 :edges "target-id:{requested_edge_target_id}" (box 20 20 10))))"#
            );
            let exact_program = compile(&exact_source);
            let (bundle, manifest) = render_core_program_runtime_bundle(
                &exact_program,
                &exact_source,
                &DesignParams::new(),
                &layout,
                &resolver,
            )
            .expect("direct OCCT edge alias fillet bundle");

            assert!(Path::new(&bundle.model_stl_path).is_file());
            assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
            assert_eq!(manifest.parts[0].part_id, "body");
            validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_stable_and_durable_face_alias_target_id_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-stable-durable-face-alias-target-id");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (_base_bundle, base_manifest) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let face_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .and_then(|target| target.target_id.clone())
            .expect("box face stable target");
        let face_durable_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .and_then(|target| target.durable_target_id.clone())
            .expect("box face durable target");

        for requested_face_target_id in [face_target_id, face_durable_target_id] {
            let exact_source = format!(
                r#"(model (part body (shell 1.5 :faces "target-id:{requested_face_target_id}" (box 20 20 10))))"#
            );
            let exact_program = compile(&exact_source);
            let (bundle, manifest) = render_core_program_runtime_bundle(
                &exact_program,
                &exact_source,
                &DesignParams::new(),
                &layout,
                &resolver,
            )
            .expect("direct OCCT face alias shell bundle");

            assert!(Path::new(&bundle.model_stl_path).is_file());
            assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
            assert_eq!(manifest.parts[0].part_id, "body");
            validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_exact_face_target_id_for_shell_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-exact-face-target-id");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (base_bundle, _) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let face_target_id = base_bundle
            .face_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box face target");
        let drifted_face_target_id = face_target_id.replacen(":face:0:", ":face:999:", 1);
        assert_ne!(drifted_face_target_id, face_target_id);

        let exact_source = format!(
            r#"(model (part body (shell 1.5 :faces "target-id:{drifted_face_target_id}" (box 20 20 10))))"#
        );
        let exact_program = compile(&exact_source);
        let exact_hash = content_hash(&exact_source, "{}");
        let exact_model_id = model_id_from_hash(&exact_hash);
        let (bundle, manifest) = render_core_program_runtime_bundle(
            &exact_program,
            &exact_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT exact-target shell bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(
            std::fs::metadata(&bundle.model_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        assert!(
            face_target_id.starts_with("body:face:"),
            "unexpected face target id: {face_target_id}"
        );
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &exact_model_id).expect("dir");
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_some()
        {
            assert!(bundle_dir.join("plan.json").is_file());
            assert!(!bundle_dir.join("direct_occt_executor.cpp").exists());
        }
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_applies_exact_face_alias_target_id_for_shell_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-exact-face-alias-target-id");
        let resolver = TestResolver { root: root.clone() };
        let base_source = "(model (part body (box 20 20 10)))";
        let base_program = compile(base_source);
        let (_base_bundle, base_manifest) = render_core_program_runtime_bundle(
            &base_program,
            base_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT base bundle");
        let face_alias_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box face alias target");

        let exact_source = format!(
            r#"(model (part body (shell 1.5 :faces "target-id:{face_alias_target_id}" (box 20 20 10))))"#
        );
        let exact_program = compile(&exact_source);
        let (bundle, manifest) = render_core_program_runtime_bundle(
            &exact_program,
            &exact_source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT exact-alias shell bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_writes_multi_part_manifest_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-multipart-bundle");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"
            (model
              (part base (box 20 14 3))
              (part post (translate 0 0 3 (cylinder 3 12))))
        "#;
        let program = compile(source);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT runtime bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert_eq!(manifest.document.object_count, 2);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "post"]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_uses_parameter_overrides_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-params");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"
            (model
              (params (number width 10))
              (part body (box width 12 14)))
        "#;
        let program = compile(source);
        let params = DesignParams::from([("width".to_string(), ParamValue::Number(24.0))]);

        let (bundle, manifest) =
            render_core_program_runtime_bundle(&program, source, &params, &layout, &resolver)
                .expect("direct OCCT runtime bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert_eq!(manifest.parts[0].parameter_keys, vec!["width"]);
        assert_eq!(manifest.parameter_groups[0].parameter_keys, vec!["width"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_renders_front_side_and_mirrored_latch_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-component-placement");
        let resolver = TestResolver { root: root.clone() };
        let source =
            include_str!("../../tests/fixtures/component-placement/dryer-latch-front-side.ecky");
        let program = compile(source);
        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("component placement runtime bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(
            bundle
                .export_artifacts
                .iter()
                .any(|artifact| artifact.format == "step" && Path::new(&artifact.path).is_file()),
            "placed fixture must produce STEP through native OCC"
        );
        assert_eq!(bundle.viewer_assets.len(), 4);
        assert!(
            bundle
                .viewer_assets
                .iter()
                .all(|asset| Path::new(&asset.path).is_file()),
            "placed multipart STL assets must exist"
        );
        assert_eq!(manifest.document.object_count, 4);
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
        if let Some(provenance) = bundle.geometry_provenance.as_ref() {
            if let Some(edge_count) = provenance.boundary_or_non_manifold_edge_count {
                assert_eq!(edge_count, 0);
            }
            if let Some(closed) = provenance.closed {
                assert!(closed);
            }
        }
        let evidence = crate::ecky_scheme::compiler::inspect_component_placement_evidence(
            source,
            &BTreeMap::new(),
        )
        .expect("placement evidence");
        assert_eq!(evidence.len(), 3);
        assert_eq!(evidence[1].target_port_id, "side-left");
        assert_eq!(
            evidence[2].mirror_axis,
            Some(ecky_render::component_placement::MirrorAxis::X)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_exports_snap_clip_fixture_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-snap-clip");
        let resolver = TestResolver { root: root.clone() };
        let source = include_str!("../../tests/fixtures/cad/surface/direct_occt_snap_clip.ecky");
        let program = compile(source);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT snap clip runtime bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(
            std::fs::metadata(&bundle.model_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert_eq!(manifest.document.object_count, 2);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["saddle", "latch"]
        );
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_bounds_male_thread_fixture_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-bounded-male-thread");
        let resolver = TestResolver { root: root.clone() };
        let source = include_str!("../../tests/fixtures/cad/surface/bounded_thread_pair.ecky");
        let program = compile(source);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("bounded male thread runtime bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        let bounds = manifest.parts[0]
            .bounds
            .as_ref()
            .expect("male thread bounds");
        let tolerance = 1.0e-3;
        assert!(
            bounds.x_min >= -9.0 - tolerance
                && bounds.x_max <= 9.0 + tolerance
                && bounds.y_min >= -9.0 - tolerance
                && bounds.y_max <= 9.0 + tolerance,
            "male thread escaped radius-9 envelope: {bounds:?}"
        );
        assert!(
            bounds.z_min >= -tolerance && bounds.z_max <= 12.0 + tolerance,
            "male thread escaped z=0..12 envelope: {bounds:?}"
        );
        let female_bounds = manifest.parts[1]
            .bounds
            .as_ref()
            .expect("female threaded tube bounds");
        assert!(
            female_bounds.z_min >= -tolerance && female_bounds.z_max <= 12.0 + tolerance,
            "female subtraction escaped host z=0..12 bounds: {female_bounds:?}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_exports_frame_array_bracket_fixture_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-frame-array-bracket");
        let resolver = TestResolver { root: root.clone() };
        let source =
            include_str!("../../tests/fixtures/cad/surface/direct_occt_frame_array_bracket.ecky");
        let program = compile(source);

        let (bundle, manifest) = match render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        ) {
            Ok(value) => value,
            Err(err)
                if err.code == crate::contracts::AppErrorCode::Validation
                    && err
                        .message
                        .contains("does not support plan; generated-C++ fallback was removed") =>
            {
                return;
            }
            Err(err) => panic!("direct OCCT frame/array bracket runtime bundle: {err:?}"),
        };

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(
            std::fs::metadata(&bundle.model_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert_eq!(manifest.document.object_count, 1);
        assert_eq!(manifest.parts[0].part_id, "bracket");
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_exports_voronoi_perforated_panel_fixture_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_occt_runtime(&runtime_root);
        if !layout.runtime_complete() {
            return;
        }

        let root = temp_root("direct-occt-live-voronoi-perforated-panel");
        let resolver = TestResolver { root: root.clone() };
        let source = include_str!("../../tests/fixtures/cad/surface/voronoi_perforated_panel.ecky");
        let program = compile(source);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT voronoi panel runtime bundle");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(
            std::fs::metadata(&bundle.model_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert_eq!(manifest.document.object_count, 1);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["panel"]
        );

        let bundle_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &bundle.model_id)
            .expect("bundle dir");
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_some()
        {
            assert!(bundle_dir.join("plan.json").is_file());
            assert!(!bundle_dir.join("direct_occt_executor.cpp").exists());
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_part_bindings_uses_per_part_asset_paths_when_available() {
        let parts = vec![
            ("base".to_string(), "Base".to_string()),
            ("lid".to_string(), "Lid".to_string()),
        ];
        let param_keys = vec!["width".to_string()];
        let mut asset_paths = std::collections::HashMap::new();
        asset_paths.insert("base".to_string(), "parts/base.stl".to_string());
        asset_paths.insert("lid".to_string(), "parts/lid.stl".to_string());

        let bindings =
            direct_occt_part_bindings(&parts, &param_keys, &asset_paths, &HashMap::new());
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].part_id, "base");
        assert_eq!(
            bindings[0].viewer_asset_path,
            Some("parts/base.stl".to_string())
        );
        assert_eq!(bindings[1].part_id, "lid");
        assert_eq!(
            bindings[1].viewer_asset_path,
            Some("parts/lid.stl".to_string())
        );
    }

    #[test]
    fn direct_occt_part_bindings_falls_back_to_model_stl_without_per_part_paths() {
        let parts = vec![("body".to_string(), "Body".to_string())];
        let param_keys = vec![];
        let asset_paths = std::collections::HashMap::new();

        let bindings =
            direct_occt_part_bindings(&parts, &param_keys, &asset_paths, &HashMap::new());
        assert_eq!(bindings.len(), 1);
        assert_eq!(
            bindings[0].viewer_asset_path,
            Some(MODEL_STL_FILE_NAME.to_string())
        );
    }

    // --- Byte-budgeted hot-cache eviction -------------------------------------

    fn hot_cache_entry(content_hash: &str, resident_bytes: u64) -> DirectOcctHotCacheEntry {
        // Minimal bundle whose artifact paths point at non-existent files. The
        // eviction helper trusts the stored `resident_bytes` field (mirroring
        // production, which records bytes once at insertion time), so no disk
        // access is needed and the unit tests stay deterministic and free of
        // global-cache races.
        let bundle_dir = PathBuf::from(format!("/tmp/ecky-hot-cache-evict-fixture-{content_hash}"));
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        let manifest = build_direct_occt_manifest(
            content_hash,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            content_hash,
            content_hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        DirectOcctHotCacheEntry {
            bundle_dir,
            content_hash: content_hash.to_string(),
            bundle,
            manifest,
            resident_bytes,
        }
    }

    fn hot_cache_content_hashes(entries: &VecDeque<DirectOcctHotCacheEntry>) -> Vec<String> {
        entries
            .iter()
            .map(|entry| entry.content_hash.clone())
            .collect()
    }

    #[test]
    fn evict_hot_cache_to_byte_budget_preserves_order_when_within_budget() {
        // BDD: when resident bytes are already within the budget, no entry is
        // evicted and MRU->LRU (front->back) ordering is preserved.
        let mut entries: VecDeque<DirectOcctHotCacheEntry> = vec![
            hot_cache_entry("mru", 10),
            hot_cache_entry("mid", 20),
            hot_cache_entry("lru", 30),
        ]
        .into_iter()
        .collect();
        evict_hot_cache_to_byte_budget(&mut entries, 1_000);
        assert_eq!(
            hot_cache_content_hashes(&entries),
            vec!["mru".to_string(), "mid".to_string(), "lru".to_string()],
            "entries within budget must be retained in MRU->LRU order"
        );
    }

    #[test]
    fn evict_hot_cache_to_byte_budget_evicts_lru_from_back_until_under_budget() {
        // BDD: when total resident bytes exceed the budget, only the
        // least-recently-used entries (back of the deque) are dropped until the
        // total fits; the most-recent entries are kept.
        let mut entries: VecDeque<DirectOcctHotCacheEntry> = vec![
            hot_cache_entry("mru", 40),
            hot_cache_entry("mid", 30),
            hot_cache_entry("lru", 20),
        ]
        .into_iter()
        .collect();
        // total 90; budget 70; drop LRU (20) -> 70 which fits. Keep [mru, mid].
        evict_hot_cache_to_byte_budget(&mut entries, 70);
        assert_eq!(
            hot_cache_content_hashes(&entries),
            vec!["mru".to_string(), "mid".to_string()],
            "LRU entries must be evicted from the back until within byte budget"
        );
    }

    #[test]
    fn evict_hot_cache_to_byte_budget_does_not_retain_oversized_single_entry() {
        // BDD: a single entry whose resident bytes exceed the whole budget is
        // never retained, even when it is the only entry. This prevents one
        // pathological render from pinning the hot cache.
        let mut entries: VecDeque<DirectOcctHotCacheEntry> =
            vec![hot_cache_entry("huge", 200)].into_iter().collect();
        evict_hot_cache_to_byte_budget(&mut entries, 100);
        assert!(
            entries.is_empty(),
            "an oversized single entry must not be retained"
        );
    }

    #[test]
    fn evict_hot_cache_to_byte_budget_drops_mru_when_only_oversized_remain() {
        // BDD: eviction is total-driven, not position-shielded. After the older
        // LRU entries are evicted, an oversized most-recent entry is dropped
        // too, so the MRU slot is never a safe harbor for an oversized bundle.
        let mut entries: VecDeque<DirectOcctHotCacheEntry> = vec![
            hot_cache_entry("huge-mru", 150),
            hot_cache_entry("small-lru", 10),
        ]
        .into_iter()
        .collect();
        // total 160; budget 100; drop LRU (10) -> 150 still over; drop huge-mru.
        evict_hot_cache_to_byte_budget(&mut entries, 100);
        assert!(
            entries.is_empty(),
            "an oversized MRU entry must also be evicted once LRU entries are gone"
        );
    }

    #[test]
    fn resident_artifact_bytes_sums_cached_artifact_path_sizes() {
        // BDD: the resident byte count that drives byte-budgeted eviction is the
        // sum of the on-disk artifact sizes reported by `cached_artifact_paths`
        // (model STL, STEP export, and source macro).
        let root = temp_root("direct-occt-resident-bytes");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let program = compile(source);
        let params = DesignParams::new();
        let params_json = serde_json::to_string(&params).expect("params");
        let hash = content_hash_with_runtime_inputs(&program, source, &params_json, &params, None)
            .expect("content hash");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(MODEL_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        let source_bytes = source.as_bytes();
        let preview_bytes: &[u8] = b"solid resident preview payload";
        let step_bytes: &[u8] = b"ISO-10303-21; resident step";
        fs::write(&source_path, source_bytes).expect("source");
        fs::write(&preview_path, preview_bytes).expect("preview");
        fs::write(&step_path, step_bytes).expect("step");
        let manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &[],
            &[],
            None,
            &HashMap::new(),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");

        let expected = (source_bytes.len() + preview_bytes.len() + step_bytes.len()) as u64;
        assert_eq!(
            resident_artifact_bytes(&bundle),
            expected,
            "resident bytes must sum the cached artifact path sizes"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn content_key_changes_when_backend_cache_schema_changes() {
        // BDD: the cache key is seeded by the backend cache schema (the backend
        // version). Identical source and parameters under two different schemas
        // MUST yield different keys, so a schema bump invalidates the hot and
        // disk caches. Production keys on DIRECT_OCCT_CACHE_SCHEMA with no
        // legacy/version branch.
        let source = "(model (part body (box 10 20 30)))";
        let params_json = "{}";
        let previous_schema = "direct-occt-v4-hypothetical-prior-cache-contract";
        let current =
            content_hash_with_backend_version(DIRECT_OCCT_CACHE_SCHEMA, source, params_json, None);
        let previous =
            content_hash_with_backend_version(previous_schema, source, params_json, None);
        assert_ne!(
            current, previous,
            "bumping the backend cache schema must invalidate the cache key for identical inputs"
        );
        assert_eq!(
            content_hash_with_font_path(source, params_json, None),
            current,
            "production must key on DIRECT_OCCT_CACHE_SCHEMA with no legacy branch"
        );
    }

    #[test]
    fn svg_counter_fill_fix_invalidates_v9_cached_artifacts() {
        let source = r#"(model (part body (extrude (svg "<svg/>" 10 10 "contain") 2)))"#;
        let params_json = "{}";
        let stale = content_hash_with_backend_version(
            "direct-occt-v9-lexical-shape-provenance",
            source,
            params_json,
            None,
        );
        let current =
            content_hash_with_backend_version(DIRECT_OCCT_CACHE_SCHEMA, source, params_json, None);

        assert_ne!(current, stale, "SVG fill repair must not reuse v9 geometry");
    }
}
