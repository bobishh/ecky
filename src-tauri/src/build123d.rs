use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ecky_scheme::compiler::try_compile_to_core_program;
use crate::freecad::resolve_resource_path;
use crate::models::{
    validate_model_runtime_bundle, AppError, AppResult, ArtifactBundle, DesignParams,
    DocumentMetadata, EnrichmentStatus, ExportArtifact, GeometryBackend, ManifestBounds,
    ManifestEnrichmentState, ModelManifest, ModelSourceKind, PartBinding, PathResolver,
    SelectionTarget, SelectionTargetKind, SourceLanguage, ViewerAsset, ViewerAssetFormat,
    ViewerEdgePoint, ViewerEdgeTarget, ViewerFaceTarget, MODEL_RUNTIME_SCHEMA_VERSION,
};
use crate::topology_target_ids::{
    durable_edge_target_id, durable_edge_target_id_for_stable_node_key, durable_face_target_id,
    durable_face_target_id_for_stable_node_key, preferred_public_topology_target_id,
    stable_edge_target_id, stable_face_target_id, topology_target_aliases, viewer_target_alias_ids,
};

const RUNNER_RESOURCE_PATH: &str = "server/build123d_runner.py";
const MODEL_RUNTIME_ROOT: &str = "model-runtime";
const GENERATED_ARTIFACT_DIR: &str = "generated";
const BUNDLE_FILE_NAME: &str = "bundle.json";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
const STEP_FILE_NAME: &str = "model.step";
const PARTS_DIR_NAME: &str = "parts";
const RUNNER_REPORT_FILE_NAME: &str = "runner-report.json";
const BUNDLED_PYTHON_RESOURCE_CANDIDATES: &[&str] = &[
    "runtime/build123d/bin/python3",
    "runtime/build123d/bin/python",
];
const BUNDLED_PYTHON_FALLBACK_CANDIDATES: &[&str] = &[
    ".dist/build123d-runtime/bin/python3",
    ".dist/build123d-runtime/bin/python",
];

#[derive(Debug, Clone, Deserialize)]
struct RunnerReport {
    #[serde(default)]
    document_name: String,
    #[serde(default)]
    document_label: String,
    #[serde(default)]
    warnings: Vec<String>,
    #[serde(default)]
    objects: Vec<RunnerObject>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunnerObject {
    object_name: String,
    #[serde(default)]
    label: String,
    export_path: String,
    #[serde(default)]
    bounds: Option<RunnerBounds>,
    #[serde(default)]
    volume: Option<f64>,
    #[serde(default)]
    area: Option<f64>,
    #[serde(default)]
    edges: Vec<RunnerEdgeTarget>,
    #[serde(default)]
    faces: Vec<RunnerFaceTarget>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunnerEdgeTarget {
    #[serde(default)]
    target_id: String,
    #[serde(default)]
    edge_index: Option<u32>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    start: Option<RunnerEdgePoint>,
    #[serde(default)]
    end: Option<RunnerEdgePoint>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunnerEdgePoint {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct RunnerFaceTarget {
    #[serde(default)]
    target_id: String,
    #[serde(default)]
    face_index: Option<u32>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    center: Option<RunnerEdgePoint>,
    #[serde(default)]
    normal: Option<RunnerEdgePoint>,
    #[serde(default)]
    area: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunnerBounds {
    x_min: f64,
    y_min: f64,
    z_min: f64,
    x_max: f64,
    y_max: f64,
    z_max: f64,
}

impl From<RunnerBounds> for ManifestBounds {
    fn from(b: RunnerBounds) -> Self {
        ManifestBounds {
            x_min: b.x_min,
            y_min: b.y_min,
            z_min: b.z_min,
            x_max: b.x_max,
            y_max: b.y_max,
            z_max: b.z_max,
        }
    }
}

pub fn render_model(
    source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    render_model_with_source_language(source, parameters, app, SourceLanguage::EckyIrV0)
}

pub fn render_model_with_source_language(
    source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
    source_language: SourceLanguage,
) -> AppResult<ArtifactBundle> {
    render_model_with_sources(source, None, parameters, app, source_language)
}

pub fn render_model_with_sources(
    executable_source: &str,
    authored_source: Option<&str>,
    parameters: &DesignParams,
    app: &dyn PathResolver,
    source_language: SourceLanguage,
) -> AppResult<ArtifactBundle> {
    let source_identity = authored_source.unwrap_or(executable_source);
    let params_json =
        serde_json::to_string(parameters).map_err(|e| AppError::validation(e.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(source_identity.as_bytes());
    hasher.update(b"|");
    hasher.update(params_json.as_bytes());
    let content_hash = format!("{:x}", hasher.finalize());
    let model_id = format!("generated-b123d-{}", &content_hash[..12]);

    let bundle_dir = artifact_dir(app, &model_id)?;

    if let Some(cached) = load_cached_bundle(&bundle_dir)? {
        return Ok(cached);
    }

    fs::create_dir_all(&bundle_dir).map_err(|e| AppError::persistence(e.to_string()))?;
    let parts_dir = bundle_dir.join(PARTS_DIR_NAME);
    fs::create_dir_all(&parts_dir).map_err(|e| AppError::persistence(e.to_string()))?;

    let authored_source_path = bundle_dir.join(crate::source_flavor::authored_source_file_name(
        source_language,
        GeometryBackend::Build123d,
    ));
    fs::write(&authored_source_path, source_identity)
        .map_err(|e| AppError::persistence(e.to_string()))?;

    let runner_source_path = if authored_source.is_some() {
        let path = bundle_dir.join(crate::source_flavor::lowered_source_file_name(
            GeometryBackend::Build123d,
        ));
        fs::write(&path, executable_source).map_err(|e| AppError::persistence(e.to_string()))?;
        path
    } else {
        authored_source_path.clone()
    };

    let preview_stl_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
    let step_path = bundle_dir.join(STEP_FILE_NAME);
    let runner_report_path = bundle_dir.join(RUNNER_REPORT_FILE_NAME);

    #[cfg(test)]
    let _build123d_env_guard = crate::build123d_test_env_lock().lock().unwrap();

    run_runner(
        app,
        &runner_source_path,
        &preview_stl_path,
        &step_path,
        &parts_dir,
        &runner_report_path,
        &params_json,
    )?;

    let report = read_runner_report(&runner_report_path)?;
    let part_topology_ids = authored_part_topology_ids(source_identity, source_language)?;
    let manifest = build_manifest_with_stable_node_keys(
        &model_id,
        &report,
        &part_topology_ids.root_node_ids,
        &part_topology_ids.stable_node_keys,
        source_language,
        Some(path_to_string(&authored_source_path)?),
    );
    let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
    write_manifest(&manifest_path, &manifest)?;

    let viewer_assets = build_viewer_assets(&manifest.parts);
    let bundle = ArtifactBundle {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id,
        source_kind: ModelSourceKind::Generated,
        engine_kind: source_language.to_engine_kind(),
        source_language,
        geometry_backend: GeometryBackend::Build123d,
        content_hash,
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: path_to_string(&manifest_path)?,
        macro_path: Some(path_to_string(&authored_source_path)?),
        preview_stl_path: path_to_string(&preview_stl_path)?,
        viewer_assets,
        edge_targets: edge_targets_from_report(&report, &manifest),
        face_targets: face_targets_from_report(&report, &manifest),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: step_export_artifacts(&step_path)?,
    };
    validate_model_runtime_bundle(&manifest, &bundle)?;
    write_bundle(&bundle_dir, &bundle)?;
    Ok(bundle)
}

fn artifact_dir(app: &dyn PathResolver, model_id: &str) -> AppResult<PathBuf> {
    let root = app.app_data_dir().join(MODEL_RUNTIME_ROOT);
    fs::create_dir_all(&root).map_err(|e| AppError::persistence(e.to_string()))?;
    Ok(root.join(GENERATED_ARTIFACT_DIR).join(model_id))
}

fn load_cached_bundle(bundle_dir: &Path) -> AppResult<Option<ArtifactBundle>> {
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    if !bundle_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&bundle_path)
        .map_err(|e| AppError::persistence(format!("Failed to read bundle: {}", e)))?;
    let mut bundle: ArtifactBundle = serde_json::from_str(&raw)
        .map_err(|e| AppError::parse(format!("Failed to parse bundle: {}", e)))?;
    if !Path::new(&bundle.manifest_path).exists()
        || !Path::new(&bundle.preview_stl_path).exists()
        || !bundle_step_path(bundle_dir).exists()
        || bundle
            .viewer_assets
            .iter()
            .any(|a| !Path::new(&a.path).exists())
    {
        return Ok(None);
    }
    bundle.export_artifacts = step_export_artifacts(&bundle_step_path(bundle_dir))?;
    Ok(Some(bundle))
}

fn run_runner(
    app: &dyn PathResolver,
    source_path: &Path,
    stl_path: &Path,
    step_path: &Path,
    parts_dir: &Path,
    report_path: &Path,
    params_json: &str,
) -> AppResult<()> {
    let python_cmd = resolve_python_cmd_with_app(app)?;
    let runner_path = resolve_resource_path(
        app,
        RUNNER_RESOURCE_PATH,
        &[
            "../server/build123d_runner.py",
            "server/build123d_runner.py",
        ],
    )?;
    let output = Command::new(&python_cmd)
        .arg(&runner_path)
        .env("ECKYCAD_SOURCE", path_to_string(source_path)?)
        .env("ECKYCAD_STL", path_to_string(stl_path)?)
        .env("ECKYCAD_STEP", path_to_string(step_path)?)
        .env("ECKYCAD_PARTS_DIR", path_to_string(parts_dir)?)
        .env("ECKYCAD_REPORT", path_to_string(report_path)?)
        .env("ECKYCAD_PARAMS", params_json)
        .output()
        .map_err(|e| AppError::render(format!("Failed to execute build123d runner: {}", e)))?;

    if !output.status.success() {
        return Err(AppError::with_details(
            crate::models::AppErrorCode::Render,
            "build123d runner failed.",
            format!(
                "stdout:\n{}\n\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ),
        ));
    }
    Ok(())
}

fn bundle_step_path(bundle_dir: &Path) -> PathBuf {
    bundle_dir.join(STEP_FILE_NAME)
}

fn step_export_artifacts(step_path: &Path) -> AppResult<Vec<ExportArtifact>> {
    Ok(vec![ExportArtifact {
        label: "STEP".to_string(),
        format: "step".to_string(),
        path: path_to_string(step_path)?,
        role: "primary".to_string(),
    }])
}

pub fn resolve_python_cmd() -> AppResult<PathBuf> {
    resolve_python_cmd_from_env_or_path()
}

pub fn resolve_python_cmd_with_app(app: &dyn PathResolver) -> AppResult<PathBuf> {
    if let Some(path) = resolve_python_cmd_from_env() {
        return Ok(path);
    }

    if let Some(path) = resolve_bundled_python_cmd(app) {
        return Ok(path);
    }

    resolve_python_cmd_from_path()
}

fn resolve_python_cmd_from_env_or_path() -> AppResult<PathBuf> {
    if let Some(path) = resolve_python_cmd_from_env() {
        return Ok(path);
    }

    resolve_python_cmd_from_path()
}

fn resolve_python_cmd_from_env() -> Option<PathBuf> {
    for var in &["BUILD123D_PYTHON", "PYTHON_CMD"] {
        if let Ok(cmd) = std::env::var(var) {
            let cmd = cmd.trim().to_string();
            if !cmd.is_empty() {
                return Some(PathBuf::from(cmd));
            }
        }
    }
    None
}

fn resolve_python_cmd_from_path() -> AppResult<PathBuf> {
    for candidate in &["python3", "python"] {
        if which_on_path(candidate).is_some() {
            return Ok(PathBuf::from(candidate));
        }
    }
    Err(AppError::render(
        "Python executable not found. Install build123d and ensure python3 is on PATH, \
         or set BUILD123D_PYTHON to a specific interpreter."
            .to_string(),
    ))
}

fn resolve_bundled_python_cmd(app: &dyn PathResolver) -> Option<PathBuf> {
    for resource in BUNDLED_PYTHON_RESOURCE_CANDIDATES {
        if let Some(path) = app.resource_path(resource) {
            if path.exists() {
                return Some(path);
            }
        }
    }

    for fallback in BUNDLED_PYTHON_FALLBACK_CANDIDATES {
        let path = PathBuf::from(fallback);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn read_runner_report(path: &Path) -> AppResult<RunnerReport> {
    let raw = fs::read_to_string(path).map_err(|e| {
        AppError::persistence(format!(
            "Failed to read build123d runner report '{}': {}",
            path.display(),
            e
        ))
    })?;
    let report: RunnerReport = serde_json::from_str(&raw)
        .map_err(|e| AppError::parse(format!("Failed to parse runner report: {}", e)))?;
    if report.objects.is_empty() {
        return Err(AppError::render(
            "build123d runner did not export any parts.".to_string(),
        ));
    }
    Ok(report)
}

#[cfg(test)]
fn build_manifest(
    model_id: &str,
    report: &RunnerReport,
    part_root_node_ids: &HashMap<String, u64>,
    source_language: SourceLanguage,
    source_path: Option<String>,
) -> ModelManifest {
    let part_stable_node_keys = HashMap::new();
    build_manifest_with_stable_node_keys(
        model_id,
        report,
        part_root_node_ids,
        &part_stable_node_keys,
        source_language,
        source_path,
    )
}

fn build_manifest_with_stable_node_keys(
    model_id: &str,
    report: &RunnerReport,
    part_root_node_ids: &HashMap<String, u64>,
    part_stable_node_keys: &HashMap<String, String>,
    source_language: SourceLanguage,
    source_path: Option<String>,
) -> ModelManifest {
    let mut parts = Vec::new();
    let mut selection_targets = Vec::new();
    let mut stable_edge_target_counts = HashMap::<String, usize>::new();
    let mut stable_face_target_counts = HashMap::<String, usize>::new();

    for obj in &report.objects {
        for edge in obj.edges.iter().filter(|edge| valid_runner_edge(edge)) {
            let canonical_target_id = runner_edge_target_id(&obj.object_name, edge);
            let stable_target_id = runner_stable_edge_target_id(&canonical_target_id);
            if !stable_target_id.is_empty() {
                *stable_edge_target_counts
                    .entry(stable_target_id)
                    .or_insert(0) += 1;
            }
        }
        for face in obj.faces.iter().filter(|face| valid_runner_face(face)) {
            let canonical_target_id = runner_face_target_id(&obj.object_name, face);
            let stable_target_id = runner_stable_face_target_id(&canonical_target_id);
            if !stable_target_id.is_empty() {
                *stable_face_target_counts
                    .entry(stable_target_id)
                    .or_insert(0) += 1;
            }
        }
    }

    for (index, obj) in report.objects.iter().enumerate() {
        let part_id = stable_part_id(&obj.object_name);
        let label = if obj.label.trim().is_empty() {
            obj.object_name.clone()
        } else {
            obj.label.clone()
        };
        parts.push(PartBinding {
            part_id: part_id.clone(),
            freecad_object_name: obj.object_name.clone(),
            label: label.clone(),
            kind: "solid".to_string(),
            semantic_role: Some("generated".to_string()),
            viewer_asset_path: Some(obj.export_path.clone()),
            viewer_node_ids: vec![part_id.clone()],
            parameter_keys: Vec::new(),
            editable: false,
            bounds: obj.bounds.clone().map(Into::into),
            volume: obj.volume,
            area: obj.area,
        });
        selection_targets.push(SelectionTarget {
            target_id: Some(format!("target-{}", part_id)),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: part_id.clone(),
            viewer_node_id: part_id.clone(),
            label: label.clone(),
            kind: SelectionTargetKind::Object,
            editable: false,
            parameter_keys: Vec::new(),
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        });
        for edge in obj.edges.iter().filter(|edge| valid_runner_edge(edge)) {
            let canonical_target_id = runner_edge_target_id(&obj.object_name, edge);
            let stable_target_id = runner_stable_edge_target_id(&canonical_target_id);
            let public_target_id = if stable_target_id.is_empty()
                || stable_edge_target_counts
                    .get(&stable_target_id)
                    .copied()
                    .unwrap_or(0)
                    > 1
            {
                canonical_target_id.clone()
            } else {
                stable_target_id
            };
            let durable_target_id = runner_durable_edge_target_id(
                part_id.as_str(),
                part_stable_node_keys
                    .get(part_id.as_str())
                    .map(String::as_str),
                part_root_node_ids.get(part_id.as_str()).copied(),
                &public_target_id,
            )
            .filter(|durable_target_id| durable_target_id != &public_target_id);
            selection_targets.push(SelectionTarget {
                target_id: Some(public_target_id.clone()),
                durable_target_id,
                canonical_target_id: (public_target_id != canonical_target_id)
                    .then(|| canonical_target_id.clone()),
                alias_ids: if public_target_id != canonical_target_id {
                    topology_target_aliases(&public_target_id, canonical_target_id)
                } else {
                    Vec::new()
                },
                part_id: part_id.clone(),
                viewer_node_id: part_id.clone(),
                label: runner_edge_label(&obj.object_name, edge),
                kind: SelectionTargetKind::Edge,
                editable: false,
                parameter_keys: Vec::new(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            });
        }
        for face in obj.faces.iter().filter(|face| valid_runner_face(face)) {
            let canonical_target_id = runner_face_target_id(&obj.object_name, face);
            let stable_target_id = runner_stable_face_target_id(&canonical_target_id);
            let public_target_id = if stable_target_id.is_empty()
                || stable_face_target_counts
                    .get(&stable_target_id)
                    .copied()
                    .unwrap_or(0)
                    > 1
            {
                canonical_target_id.clone()
            } else {
                stable_target_id
            };
            let durable_target_id = runner_durable_face_target_id(
                part_id.as_str(),
                part_stable_node_keys
                    .get(part_id.as_str())
                    .map(String::as_str),
                part_root_node_ids.get(part_id.as_str()).copied(),
                &public_target_id,
            )
            .filter(|durable_target_id| durable_target_id != &public_target_id);
            selection_targets.push(SelectionTarget {
                target_id: Some(public_target_id.clone()),
                durable_target_id,
                canonical_target_id: (public_target_id != canonical_target_id)
                    .then(|| canonical_target_id.clone()),
                alias_ids: if public_target_id != canonical_target_id {
                    topology_target_aliases(&public_target_id, canonical_target_id)
                } else {
                    Vec::new()
                },
                part_id: part_id.clone(),
                viewer_node_id: part_id.clone(),
                label: runner_face_label(&obj.object_name, face),
                kind: SelectionTargetKind::Face,
                editable: false,
                parameter_keys: Vec::new(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            });
        }
        let _ = index; // suppress unused warning
    }

    ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind: ModelSourceKind::Generated,
        source_digest: None,
        core_digest: None,
        ast_schema_version: None,
        engine_kind: source_language.to_engine_kind(),
        source_language,
        geometry_backend: GeometryBackend::Build123d,
        document: DocumentMetadata {
            document_name: if report.document_name.trim().is_empty() {
                "EckyCAD".to_string()
            } else {
                report.document_name.clone()
            },
            document_label: if report.document_label.trim().is_empty() {
                "EckyCAD".to_string()
            } else {
                report.document_label.clone()
            },
            source_path,
            object_count: parts.len(),
            warnings: report.warnings.clone(),
        },
        parts,
        parameter_groups: Vec::new(),
        control_primitives: Vec::new(),
        control_relations: Vec::new(),
        control_views: Vec::new(),
        advisories: Vec::new(),
        selection_targets,
        measurement_annotations: Vec::new(),
        feature_graph: None,
        correspondence_graph: None,
        warnings: report.warnings.clone(),
        enrichment_state: ManifestEnrichmentState {
            status: EnrichmentStatus::None,
            proposals: Vec::new(),
        },
    }
}

fn build_viewer_assets(parts: &[PartBinding]) -> Vec<ViewerAsset> {
    parts
        .iter()
        .flat_map(|part| {
            let Some(path) = part.viewer_asset_path.as_ref() else {
                return Vec::new();
            };
            part.viewer_node_ids
                .iter()
                .map(|node_id| ViewerAsset {
                    part_id: part.part_id.clone(),
                    node_id: node_id.clone(),
                    object_name: part.freecad_object_name.clone(),
                    label: part.label.clone(),
                    path: path.clone(),
                    format: ViewerAssetFormat::Stl,
                })
                .collect()
        })
        .collect()
}

fn edge_targets_from_report(
    report: &RunnerReport,
    manifest: &ModelManifest,
) -> Vec<ViewerEdgeTarget> {
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

    for object in &report.objects {
        for edge in object.edges.iter().filter(|edge| valid_runner_edge(edge)) {
            let target_id = runner_edge_target_id(&object.object_name, edge);
            let Some(selection_target) = selection_targets_by_id.get(target_id.as_str()) else {
                continue;
            };
            let (Some(start), Some(end)) = (edge.start.as_ref(), edge.end.as_ref()) else {
                continue;
            };
            edge_targets.push(ViewerEdgeTarget {
                target_id: preferred_public_topology_target_id(selection_target, &target_id),
                durable_target_id: selection_target.durable_target_id.clone(),
                canonical_target_id: Some(target_id.clone()),
                alias_ids: viewer_target_alias_ids(selection_target, &target_id),
                part_id: selection_target.part_id.clone(),
                viewer_node_id: selection_target.viewer_node_id.clone(),
                label: runner_edge_label(&object.object_name, edge),
                editable: selection_target.editable,
                start: runner_point_to_viewer(start),
                end: runner_point_to_viewer(end),
            });
        }
    }

    edge_targets
}

fn face_targets_from_report(
    report: &RunnerReport,
    manifest: &ModelManifest,
) -> Vec<ViewerFaceTarget> {
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

    for object in &report.objects {
        for face in object.faces.iter().filter(|face| valid_runner_face(face)) {
            let target_id = runner_face_target_id(&object.object_name, face);
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
                label: runner_face_label(&object.object_name, face),
                editable: selection_target.editable,
                center: runner_point_to_viewer(center),
                normal: face
                    .normal
                    .as_ref()
                    .map(|normal| [normal.x, normal.y, normal.z]),
                area: face.area,
            });
        }
    }

    face_targets
}

fn stable_part_id(object_name: &str) -> String {
    let mut sanitized = String::new();
    for ch in object_name.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            sanitized.push(ch.to_ascii_lowercase());
        } else {
            sanitized.push('-');
        }
    }
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }
    let result = sanitized.trim_matches('-').to_string();
    if result.is_empty() {
        "part".to_string()
    } else {
        result
    }
}

fn valid_runner_edge(edge: &RunnerEdgeTarget) -> bool {
    edge.start.is_some() && edge.end.is_some()
}

fn valid_runner_face(face: &RunnerFaceTarget) -> bool {
    face.center.is_some()
}

#[derive(Default)]
struct AuthoredPartTopologyIds {
    root_node_ids: HashMap<String, u64>,
    stable_node_keys: HashMap<String, String>,
}

fn authored_part_topology_ids(
    source: &str,
    source_language: SourceLanguage,
) -> AppResult<AuthoredPartTopologyIds> {
    if source_language != SourceLanguage::EckyIrV0 {
        return Ok(AuthoredPartTopologyIds::default());
    }
    let Some(compiled) = try_compile_to_core_program(source) else {
        return Ok(AuthoredPartTopologyIds::default());
    };
    let Ok(program) = compiled else {
        return Ok(AuthoredPartTopologyIds::default());
    };
    Ok(AuthoredPartTopologyIds {
        root_node_ids: program
            .parts
            .iter()
            .map(|part| (part.key.clone(), part.root.id.raw()))
            .collect(),
        stable_node_keys: program
            .parts
            .iter()
            .filter_map(|part| {
                build123d_source_stable_node_key(source, &part.key, part.root.span)
                    .map(|stable_node_key| (part.key.clone(), stable_node_key))
            })
            .collect(),
    })
}

fn build123d_source_stable_node_key(
    source_identity: &str,
    part_key: &str,
    span: Option<crate::ecky_core_ir::SourceSpan>,
) -> Option<String> {
    let span = span?;
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
    hasher.update(b"build123d-part-root|");
    hasher.update(part_key.as_bytes());
    hasher.update(b"|");
    hasher.update(&source_identity.as_bytes()[start..end]);
    Some(format!("sha256:{:x}", hasher.finalize()))
}

fn format_edge_coordinate(value: f64) -> String {
    let rounded = if value.abs() < 1e-9 { 0.0 } else { value };
    let mut text = format!("{rounded:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        "0".to_string()
    } else {
        text
    }
}

fn runner_edge_point_signature(point: &RunnerEdgePoint) -> String {
    format!(
        "{}-{}-{}",
        format_edge_coordinate(point.x),
        format_edge_coordinate(point.y),
        format_edge_coordinate(point.z)
    )
}

fn runner_edge_signature(start: &RunnerEdgePoint, end: &RunnerEdgePoint) -> String {
    let start_signature = runner_edge_point_signature(start);
    let end_signature = runner_edge_point_signature(end);
    if end_signature < start_signature {
        format!("{end_signature}_{start_signature}")
    } else {
        format!("{start_signature}_{end_signature}")
    }
}

fn runner_edge_target_id(object_name: &str, edge: &RunnerEdgeTarget) -> String {
    let target_id = edge.target_id.trim();
    if !target_id.is_empty() {
        return target_id.to_string();
    }
    let edge_index = edge
        .edge_index
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let edge_signature = edge
        .start
        .as_ref()
        .zip(edge.end.as_ref())
        .map(|(start, end)| runner_edge_signature(start, end));
    match edge_signature {
        Some(signature) => format!("{object_name}:edge:{edge_index}:{signature}"),
        None => format!("{object_name}:edge:{edge_index}"),
    }
}

fn runner_stable_edge_target_id(target_id: &str) -> String {
    stable_edge_target_id(target_id)
}

fn runner_durable_edge_target_id(
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

fn runner_edge_label(object_name: &str, edge: &RunnerEdgeTarget) -> String {
    let label = edge.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    let edge_index = edge
        .edge_index
        .map(|index| index.saturating_add(1).to_string())
        .unwrap_or_else(|| "?".to_string());
    format!("{object_name}.Edge{edge_index}")
}

fn runner_face_target_id(object_name: &str, face: &RunnerFaceTarget) -> String {
    let target_id = face.target_id.trim();
    if !target_id.is_empty() {
        return target_id.to_string();
    }
    let face_index = face
        .face_index
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let face_signature = face.center.as_ref().map(|center| {
        let center_signature = runner_edge_point_signature(center);
        let area_signature = face
            .area
            .map(format_edge_coordinate)
            .unwrap_or_else(|| "unknown".to_string());
        format!("{center_signature}:{area_signature}")
    });
    match face_signature {
        Some(signature) => format!("{object_name}:face:{face_index}:{signature}"),
        None => format!("{object_name}:face:{face_index}"),
    }
}

fn runner_stable_face_target_id(target_id: &str) -> String {
    stable_face_target_id(target_id)
}

fn runner_durable_face_target_id(
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

fn runner_face_label(object_name: &str, face: &RunnerFaceTarget) -> String {
    let label = face.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    let face_index = face
        .face_index
        .map(|index| index.saturating_add(1).to_string())
        .unwrap_or_else(|| "?".to_string());
    format!("{object_name}.Face{face_index}")
}

fn runner_point_to_viewer(point: &RunnerEdgePoint) -> ViewerEdgePoint {
    ViewerEdgePoint {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}

fn write_manifest(path: &Path, manifest: &ModelManifest) -> AppResult<()> {
    let data =
        serde_json::to_string_pretty(manifest).map_err(|e| AppError::persistence(e.to_string()))?;
    fs::write(path, data).map_err(|e| {
        AppError::persistence(format!(
            "Failed to write manifest '{}': {}",
            path.display(),
            e
        ))
    })
}

fn write_bundle(bundle_dir: &Path, bundle: &ArtifactBundle) -> AppResult<()> {
    let path = bundle_dir.join(BUNDLE_FILE_NAME);
    let data =
        serde_json::to_string_pretty(bundle).map_err(|e| AppError::persistence(e.to_string()))?;
    fs::write(path, data).map_err(|e| AppError::persistence(e.to_string()))
}

fn path_to_string(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::internal("Non-UTF-8 path encountered."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EngineKind;
    use std::collections::BTreeMap;

    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    struct RepoResourceResolver {
        root: PathBuf,
    }

    impl PathResolver for RepoResourceResolver {
        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, path: &str) -> Option<PathBuf> {
            let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
            match path {
                "server/build123d_runner.py" => {
                    Some(repo_root.join("server").join("build123d_runner.py"))
                }
                "runtime/build123d/bin/python3" => Some(
                    repo_root
                        .join(".dist")
                        .join("build123d-runtime")
                        .join("bin")
                        .join("python3"),
                ),
                "runtime/build123d/bin/python" => Some(
                    repo_root
                        .join(".dist")
                        .join("build123d-runtime")
                        .join("bin")
                        .join("python"),
                ),
                _ => None,
            }
        }
    }

    #[test]
    fn render_model_fails_without_python_or_runner() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };

        // With no build123d runner script available this should fail at runner resolution
        // or python execution, not with a "not yet implemented" panic.
        let result = render_model("_ecky_parts = []", &BTreeMap::new(), &resolver);
        assert!(
            result.is_err(),
            "expected an error when runner is unavailable"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            !err.contains("not yet implemented"),
            "stub error should be gone: {}",
            err
        );
    }

    #[test]
    fn stable_part_id_sanitizes_names() {
        assert_eq!(stable_part_id("MyPart"), "mypart");
        assert_eq!(stable_part_id("my part!"), "my-part");
        assert_eq!(stable_part_id("body"), "body");
        assert_eq!(stable_part_id(""), "part");
        assert_eq!(stable_part_id("---"), "part");
    }

    #[test]
    fn render_model_handles_fusing_placed_shape_lists() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = RepoResourceResolver { root };
        let source = r#"
from build123d import *
from _ecky_build123d_helpers import *

_base = _ecky_fuse_many(
    Cylinder(0.4, 1, align=(Align.CENTER, Align.CENTER, Align.MIN)),
    _ecky_place(
        Location((0, 0, 1)),
        Cone(0.4, 0, 0.4, align=(Align.CENTER, Align.CENTER, Align.MIN)),
    ),
)
_a = _ecky_place(Location((0, 0, 0)), _base)
_b = _ecky_place(Location((2, 0, 0)), _base)
_c = _ecky_place(Location((4, 0, 0)), _base)
_body = _ecky_fuse_many(_a, _b, _c)
_ecky_parts = [("body", _body)]
"#;

        let bundle = render_model_with_source_language(
            source,
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::Build123d,
        )
        .expect("placed ShapeList operands should fuse and render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert_eq!(bundle.export_artifacts.len(), 1);
        assert_eq!(bundle.export_artifacts[0].format, "step");
        assert_eq!(bundle.export_artifacts[0].role, "primary");
        assert!(Path::new(&bundle.export_artifacts[0].path).exists());
        assert!(bundle.export_artifacts[0].path.ends_with("model.step"));
    }

    #[test]
    fn build_manifest_preserves_raw_build123d_source_identity() {
        let report = RunnerReport {
            document_name: "Doc".to_string(),
            document_label: "Doc".to_string(),
            warnings: Vec::new(),
            objects: vec![RunnerObject {
                object_name: "body".to_string(),
                label: "body".to_string(),
                export_path: "/tmp/body.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
                edges: Vec::new(),
                faces: Vec::new(),
            }],
        };

        let manifest = build_manifest(
            "model",
            &report,
            &HashMap::new(),
            SourceLanguage::Build123d,
            Some("/tmp/source.py".to_string()),
        );

        assert_eq!(manifest.engine_kind, EngineKind::Build123d);
        assert_eq!(manifest.source_language, SourceLanguage::Build123d);
        assert_eq!(manifest.geometry_backend, GeometryBackend::Build123d);
        assert_eq!(
            manifest.document.source_path.as_deref(),
            Some("/tmp/source.py")
        );
    }

    #[test]
    fn build_manifest_preserves_ir_to_build123d_identity() {
        let report = RunnerReport {
            document_name: "Doc".to_string(),
            document_label: "Doc".to_string(),
            warnings: Vec::new(),
            objects: vec![RunnerObject {
                object_name: "body".to_string(),
                label: "body".to_string(),
                export_path: "/tmp/body.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
                edges: Vec::new(),
                faces: Vec::new(),
            }],
        };

        let manifest = build_manifest(
            "model",
            &report,
            &HashMap::new(),
            SourceLanguage::EckyIrV0,
            Some("/tmp/source.ecky".to_string()),
        );

        assert_eq!(manifest.engine_kind, EngineKind::EckyIrV0);
        assert_eq!(manifest.source_language, SourceLanguage::EckyIrV0);
        assert_eq!(manifest.geometry_backend, GeometryBackend::Build123d);
        assert_eq!(
            manifest.document.source_path.as_deref(),
            Some("/tmp/source.ecky")
        );
    }

    #[test]
    fn build_manifest_exposes_build123d_topology_targets() {
        let report = RunnerReport {
            document_name: "Doc".to_string(),
            document_label: "Doc".to_string(),
            warnings: Vec::new(),
            objects: vec![RunnerObject {
                object_name: "body".to_string(),
                label: "body".to_string(),
                export_path: "/tmp/body.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
                edges: vec![RunnerEdgeTarget {
                    target_id: "body:edge:0:-5--5-0_5--5-0".to_string(),
                    edge_index: Some(0),
                    label: "body.Edge1".to_string(),
                    start: Some(RunnerEdgePoint {
                        x: -5.0,
                        y: -5.0,
                        z: 0.0,
                    }),
                    end: Some(RunnerEdgePoint {
                        x: 5.0,
                        y: -5.0,
                        z: 0.0,
                    }),
                }],
                faces: vec![RunnerFaceTarget {
                    target_id: "body:face:5:0-0-10:100".to_string(),
                    face_index: Some(5),
                    label: "body.Face6".to_string(),
                    center: Some(RunnerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 10.0,
                    }),
                    normal: Some(RunnerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    }),
                    area: Some(100.0),
                }],
            }],
        };

        let manifest = build_manifest(
            "model",
            &report,
            &HashMap::new(),
            SourceLanguage::Build123d,
            Some("/tmp/source.py".to_string()),
        );
        let edge_targets = edge_targets_from_report(&report, &manifest);
        let face_targets = face_targets_from_report(&report, &manifest);

        let edge_target = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .expect("edge target");
        assert_eq!(
            edge_target.target_id.as_deref(),
            Some("body:edge:-5--5-0_5--5-0")
        );
        assert!(edge_target.alias_ids.is_empty());
        assert_eq!(
            edge_target.canonical_target_id.as_deref(),
            Some("body:edge:0:-5--5-0_5--5-0")
        );
        assert_eq!(edge_target.durable_target_id, None);

        let face_target = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .expect("face target");
        assert_eq!(
            face_target.target_id.as_deref(),
            Some("body:face:0-0-10:100")
        );
        assert!(face_target.alias_ids.is_empty());
        assert_eq!(
            face_target.canonical_target_id.as_deref(),
            Some("body:face:5:0-0-10:100")
        );
        assert_eq!(face_target.durable_target_id, None);
        assert_eq!(edge_targets[0].target_id, "body:edge:-5--5-0_5--5-0");
        assert_eq!(
            edge_targets[0].canonical_target_id.as_deref(),
            Some("body:edge:0:-5--5-0_5--5-0")
        );
        assert_eq!(
            edge_targets[0].alias_ids,
            vec!["body:edge:0:-5--5-0_5--5-0".to_string()]
        );
        assert_eq!(face_targets[0].target_id, "body:face:0-0-10:100");
        assert_eq!(
            face_targets[0].canonical_target_id.as_deref(),
            Some("body:face:5:0-0-10:100")
        );
        assert_eq!(
            face_targets[0].alias_ids,
            vec!["body:face:5:0-0-10:100".to_string()]
        );
    }

    #[test]
    fn build_manifest_emits_durable_topology_targets_for_ecky_parts() {
        let report = RunnerReport {
            document_name: "Doc".to_string(),
            document_label: "Doc".to_string(),
            warnings: Vec::new(),
            objects: vec![RunnerObject {
                object_name: "body".to_string(),
                label: "body".to_string(),
                export_path: "/tmp/body.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
                edges: vec![RunnerEdgeTarget {
                    target_id: "body:edge:0:-5--5-0_5--5-0".to_string(),
                    edge_index: Some(0),
                    label: "body.Edge1".to_string(),
                    start: Some(RunnerEdgePoint {
                        x: -5.0,
                        y: -5.0,
                        z: 0.0,
                    }),
                    end: Some(RunnerEdgePoint {
                        x: 5.0,
                        y: -5.0,
                        z: 0.0,
                    }),
                }],
                faces: vec![RunnerFaceTarget {
                    target_id: "body:face:5:0-0-10:100".to_string(),
                    face_index: Some(5),
                    label: "body.Face6".to_string(),
                    center: Some(RunnerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 10.0,
                    }),
                    normal: Some(RunnerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    }),
                    area: Some(100.0),
                }],
            }],
        };
        let part_root_node_ids = HashMap::from([("body".to_string(), 42_u64)]);

        let manifest = build_manifest(
            "model",
            &report,
            &part_root_node_ids,
            SourceLanguage::EckyIrV0,
            Some("/tmp/source.ecky".to_string()),
        );

        let edge_target = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .expect("edge target");
        let face_target = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .expect("face target");

        assert_eq!(
            edge_target.durable_target_id.as_deref(),
            Some("body:node:42:edge:-5--5-0_5--5-0")
        );
        assert_eq!(
            face_target.durable_target_id.as_deref(),
            Some("body:node:42:face:0-0-10:100")
        );
    }

    #[test]
    fn build_manifest_prefers_stable_source_node_key_for_durable_topology_targets() {
        let report = RunnerReport {
            document_name: "Doc".to_string(),
            document_label: "Doc".to_string(),
            warnings: Vec::new(),
            objects: vec![RunnerObject {
                object_name: "body".to_string(),
                label: "body".to_string(),
                export_path: "/tmp/body.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
                edges: vec![RunnerEdgeTarget {
                    target_id: "body:edge:0:-5--5-0_5--5-0".to_string(),
                    edge_index: Some(0),
                    label: "body.Edge1".to_string(),
                    start: Some(RunnerEdgePoint {
                        x: -5.0,
                        y: -5.0,
                        z: 0.0,
                    }),
                    end: Some(RunnerEdgePoint {
                        x: 5.0,
                        y: -5.0,
                        z: 0.0,
                    }),
                }],
                faces: vec![RunnerFaceTarget {
                    target_id: "body:face:5:0-0-10:100".to_string(),
                    face_index: Some(5),
                    label: "body.Face6".to_string(),
                    center: Some(RunnerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 10.0,
                    }),
                    normal: Some(RunnerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    }),
                    area: Some(100.0),
                }],
            }],
        };

        let manifest = build_manifest_with_stable_node_keys(
            "model",
            &report,
            &HashMap::from([("body".to_string(), 42_u64)]),
            &HashMap::from([("body".to_string(), "sha256:source-span".to_string())]),
            SourceLanguage::EckyIrV0,
            Some("/tmp/source.ecky".to_string()),
        );

        let edge_target = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .expect("edge target");
        let face_target = manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .expect("face target");

        assert_eq!(
            edge_target.durable_target_id.as_deref(),
            Some("body:stable-node-key:sha256:source-span:edge:-5--5-0_5--5-0")
        );
        assert_eq!(
            face_target.durable_target_id.as_deref(),
            Some("body:stable-node-key:sha256:source-span:face:0-0-10:100")
        );
    }

    #[test]
    fn authored_part_topology_ids_use_source_spans_for_stable_node_keys() {
        let base_source = "(model (part body (box 10 20 30)))";
        let shifted_source = "(model (part spacer (box 1 1 1)) (part body (box 10 20 30)))";
        let base_ids =
            authored_part_topology_ids(base_source, SourceLanguage::EckyIrV0).expect("base ids");
        let shifted_ids = authored_part_topology_ids(shifted_source, SourceLanguage::EckyIrV0)
            .expect("shifted ids");

        assert_ne!(
            base_ids.root_node_ids.get("body"),
            shifted_ids.root_node_ids.get("body")
        );
        assert_eq!(
            base_ids.stable_node_keys.get("body"),
            shifted_ids.stable_node_keys.get("body")
        );
    }

    #[test]
    fn build_manifest_keeps_canonical_edge_ids_when_stable_edge_id_would_collide() {
        let report = RunnerReport {
            document_name: "Doc".to_string(),
            document_label: "Doc".to_string(),
            warnings: Vec::new(),
            objects: vec![RunnerObject {
                object_name: "Flexi_Track".to_string(),
                label: "Flexi Track".to_string(),
                export_path: "/tmp/flexi.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
                edges: vec![
                    RunnerEdgeTarget {
                        target_id: String::new(),
                        edge_index: Some(1),
                        label: String::new(),
                        start: Some(RunnerEdgePoint {
                            x: 45.709,
                            y: -12.218,
                            z: 5.0,
                        }),
                        end: Some(RunnerEdgePoint {
                            x: 50.291,
                            y: -12.218,
                            z: 5.0,
                        }),
                    },
                    RunnerEdgeTarget {
                        target_id: String::new(),
                        edge_index: Some(2),
                        label: String::new(),
                        start: Some(RunnerEdgePoint {
                            x: 45.709,
                            y: -12.218,
                            z: 5.0,
                        }),
                        end: Some(RunnerEdgePoint {
                            x: 50.291,
                            y: -12.218,
                            z: 5.0,
                        }),
                    },
                ],
                faces: Vec::new(),
            }],
        };

        let manifest = build_manifest(
            "model",
            &report,
            &HashMap::new(),
            SourceLanguage::Build123d,
            Some("/tmp/source.py".to_string()),
        );

        let edge_targets = manifest
            .selection_targets
            .iter()
            .filter(|target| target.kind == SelectionTargetKind::Edge)
            .collect::<Vec<_>>();

        assert_eq!(edge_targets.len(), 2);
        assert_eq!(
            edge_targets[0].target_id.as_deref(),
            Some("Flexi_Track:edge:1:45.709--12.218-5_50.291--12.218-5")
        );
        assert_eq!(edge_targets[0].canonical_target_id, None);
        assert_eq!(
            edge_targets[1].target_id.as_deref(),
            Some("Flexi_Track:edge:2:45.709--12.218-5_50.291--12.218-5")
        );
        assert_eq!(edge_targets[1].canonical_target_id, None);
    }

    #[test]
    fn render_model_with_sources_applies_non_uniform_scale_via_build123d() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = RepoResourceResolver { root };
        let source = r#"(model
            (part body
              (scale 0.5 0.25 1
                (box 20 20 10))))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_build123d(source).expect("lower"),
            Some(source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render");

        let manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        let part = manifest.parts.first().expect("part");
        let bounds = part.bounds.as_ref().expect("bounds");
        assert!((bounds.x_min + 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.x_max - 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.y_min + 2.5).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.y_max - 2.5).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_min - 0.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_max - 10.0).abs() < 0.25, "bounds: {:?}", bounds);
    }

    #[test]
    fn render_model_with_sources_renders_exact_edge_target_id_selector_for_box() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = RepoResourceResolver { root };
        let source = r#"(model
            (part body
              (chamfer 0.25
                :edges "target-id:body:edge:0:-5--5-0_5--5-0"
                (box 10 10 10))))"#;

        let lowered = crate::ecky_ir::lower_to_build123d(source).expect("lower");
        assert!(
            lowered.contains(r#"_ecky_select_edges("#)
                && lowered.contains(
                    r#"{'kind': 'targetIds', 'targetIds': ["body:edge:0:-5--5-0_5--5-0"]}"#
                )
                && lowered.contains(r#", "body")"#),
            "exact selector lowering: {}",
            lowered
        );

        let bundle = render_model_with_sources(
            &lowered,
            Some(source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render exact selector");

        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert_eq!(bundle.export_artifacts.len(), 1);
        assert_eq!(bundle.export_artifacts[0].format, "step");
        assert!(Path::new(&bundle.export_artifacts[0].path).exists());

        let manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        let part = manifest.parts.first().expect("part");
        let bounds = part.bounds.as_ref().expect("bounds");
        assert_eq!(part.part_id, "body");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Edge));
        assert!(!bundle.edge_targets.is_empty());
        assert!((bounds.x_min + 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.x_max - 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.y_min + 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.y_max - 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_min - 0.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_max - 10.0).abs() < 0.25, "bounds: {:?}", bounds);
    }

    #[test]
    fn render_model_with_sources_renders_exact_edge_alias_target_id_selector_for_box() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = RepoResourceResolver { root };
        let base_source = r#"(model
            (part body
              (box 10 10 10)))"#;
        let base_bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_build123d(base_source).expect("lower base"),
            Some(base_source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render base box");
        let base_manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&base_bundle.manifest_path).expect("read base manifest"),
        )
        .expect("parse base manifest");
        let edge_alias_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Edge)
            .and_then(|target| target.canonical_target_id.clone())
            .expect("edge alias target");
        let source = format!(
            r#"(model
            (part body
              (chamfer 0.25
                :edges "target-id:{edge_alias_target_id}"
                (box 10 10 10))))"#
        );

        let lowered = crate::ecky_ir::lower_to_build123d(&source).expect("lower");
        let bundle = render_model_with_sources(
            &lowered,
            Some(&source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render exact alias selector");

        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step" && Path::new(&artifact.path).exists()));
        let manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
    }

    #[test]
    fn render_model_with_sources_renders_exact_face_target_id_shell_for_box() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = RepoResourceResolver { root };
        let source = r#"(model
            (part body
              (shell 1
                :faces "target-id:body:face:5:0-0-10:100"
                (box 10 10 10))))"#;

        let lowered = crate::ecky_ir::lower_to_build123d(source).expect("lower");
        assert!(
            lowered.contains(r#"_ecky_select_shell_faces("#)
                && lowered
                    .contains(r#"{'kind': 'targetIds', 'targetIds': ["body:face:5:0-0-10:100"]}"#)
                && lowered.contains(r#", "body")"#),
            "exact shell selector lowering: {}",
            lowered
        );

        let bundle = render_model_with_sources(
            &lowered,
            Some(source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render exact shell selector");

        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert_eq!(bundle.export_artifacts.len(), 1);
        assert_eq!(bundle.export_artifacts[0].format, "step");
        assert!(Path::new(&bundle.export_artifacts[0].path).exists());

        let manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        let part = manifest.parts.first().expect("part");
        let bounds = part.bounds.as_ref().expect("bounds");
        assert_eq!(part.part_id, "body");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Face));
        assert!(!bundle.face_targets.is_empty());
        assert!((bounds.x_min + 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.x_max - 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.y_min + 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.y_max - 5.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_min - 0.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_max - 10.0).abs() < 0.25, "bounds: {:?}", bounds);
    }

    #[test]
    fn render_model_with_sources_renders_exact_face_alias_target_id_shell_for_box() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = RepoResourceResolver { root };
        let base_source = r#"(model
            (part body
              (box 10 10 10)))"#;
        let base_bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_build123d(base_source).expect("lower base"),
            Some(base_source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render base box");
        let base_manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&base_bundle.manifest_path).expect("read base manifest"),
        )
        .expect("parse base manifest");
        let face_alias_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == SelectionTargetKind::Face)
            .and_then(|target| target.canonical_target_id.clone())
            .expect("face alias target");
        let source = format!(
            r#"(model
            (part body
              (shell 1
                :faces "target-id:{face_alias_target_id}"
                (box 10 10 10))))"#
        );

        let lowered = crate::ecky_ir::lower_to_build123d(&source).expect("lower");
        let bundle = render_model_with_sources(
            &lowered,
            Some(&source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render exact face alias selector");

        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step" && Path::new(&artifact.path).exists()));
        let manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
    }

    #[test]
    fn render_model_with_sources_renders_coarse_face_selector_shell_for_box() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = RepoResourceResolver { root };
        let source = r#"(model
            (part body
              (shell 1
                :faces "top"
                (box 10 10 10))))"#;

        let lowered = crate::ecky_ir::lower_to_build123d(source).expect("lower");
        assert!(
            lowered.contains(
                r#"{'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'z', 'bound': 'max'}]}"#
            ),
            "coarse face selector lowering: {}",
            lowered
        );

        let bundle = render_model_with_sources(
            &lowered,
            Some(source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render coarse face selector");

        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Face));
        assert!(!bundle.face_targets.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_richer_face_selector_shell_for_box() {
        let root =
            std::env::temp_dir().join(format!("ecky-build123d-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = RepoResourceResolver { root };
        let source = r#"(model
            (part body
              (shell 1
                :faces "planar+normal-z+area-max"
                (box 10 10 10))))"#;

        let lowered = crate::ecky_ir::lower_to_build123d(source).expect("lower");
        assert!(
            lowered.contains(
                r#"{'kind': 'clauses', 'clauses': [{'kind': 'planar'}, {'kind': 'normal', 'axis': 'z'}, {'kind': 'area', 'rank': 'max'}]}"#
            ),
            "richer face selector lowering: {}",
            lowered
        );

        let bundle = render_model_with_sources(
            &lowered,
            Some(source),
            &BTreeMap::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("render richer face selector");

        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: crate::models::ModelManifest = serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Face));
        assert!(!bundle.face_targets.is_empty());
    }
}
