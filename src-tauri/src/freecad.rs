use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{
    validate_model_manifest, AppError, AppResult, ArtifactBundle, BrepHiddenLineProjectionRequest,
    BrepHiddenLineProjectionResponse, BrepHiddenLineProjectionView, DesignParams, DocumentMetadata,
    EnrichmentProposal, EnrichmentStatus, ExportArtifact, ManifestBounds, ManifestEnrichmentState,
    ModelManifest, ModelSourceKind, ParameterGroup, PartBinding, PathResolver, SelectionTarget,
    SelectionTargetKind, SketchView, ViewerAsset, ViewerAssetFormat, MODEL_RUNTIME_SCHEMA_VERSION,
};

const RUNNER_RESOURCE_PATH: &str = "server/freecad_runner.py";
const DEFAULT_MACRO_RESOURCE_PATH: &str = "templates/cache_pot_default.FCMacro";
const CAD_SDK_RESOURCE_PATH: &str = "model-runtime/cad_sdk.py";
const MODEL_RUNTIME_ROOT: &str = "model-runtime";
const GENERATED_ARTIFACT_DIR: &str = "generated";
const IMPORTED_FCSTD_ARTIFACT_DIR: &str = "imported-fcstd";
const BUNDLE_FILE_NAME: &str = "bundle.json";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const RUNNER_REPORT_FILE_NAME: &str = "runner-report.json";
const HIDDEN_LINE_REPORT_FILE_NAME: &str = "hidden-line-projections.json";
const FCSTD_FILE_NAME: &str = "model.FCStd";
const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
const STEP_FILE_NAME: &str = "model.step";
const PARTS_DIR_NAME: &str = "parts";

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
    #[serde(default)]
    type_id: String,
    export_path: String,
    #[serde(default)]
    bounds: Option<RunnerBounds>,
    #[serde(default)]
    volume: Option<f64>,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HiddenLineProjectionReport {
    source_artifact_path: String,
    #[serde(default)]
    views: Vec<BrepHiddenLineProjectionView>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RunnerBinding {
    object_name: String,
    parameter_keys: Vec<String>,
}

pub fn render(
    macro_code: &str,
    parameters: &DesignParams,
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
) -> AppResult<String> {
    render_model(macro_code, parameters, configured_freecad_cmd, app)
        .map(|bundle| bundle.preview_stl_path)
}

pub fn render_model(
    macro_code: &str,
    parameters: &DesignParams,
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    render_model_with_sources(
        macro_code,
        None,
        parameters,
        configured_freecad_cmd,
        app,
        crate::models::SourceLanguage::LegacyPython,
    )
}

pub fn render_model_with_sources(
    executable_source: &str,
    authored_source: Option<&str>,
    parameters: &DesignParams,
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
    source_language: crate::models::SourceLanguage,
) -> AppResult<ArtifactBundle> {
    let params_json =
        serde_json::to_string(parameters).map_err(|err| AppError::validation(err.to_string()))?;
    let source_identity = authored_source.unwrap_or(executable_source);
    let content_hash = digest_segments([source_identity.as_bytes(), b"|", params_json.as_bytes()]);
    let short_hash = short_digest(&content_hash);
    let model_id = format!("generated-{}", short_hash);
    let bundle_dir = artifact_dir(app, ModelSourceKind::Generated, &model_id)?;

    if let Some(bundle) = load_cached_bundle(&bundle_dir)? {
        return Ok(bundle);
    }

    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let macro_path = bundle_dir.join(crate::source_flavor::authored_source_file_name(
        source_language,
        crate::models::GeometryBackend::Freecad,
    ));
    let runner_macro_path = if authored_source.is_some() {
        bundle_dir.join(crate::source_flavor::lowered_source_file_name(
            crate::models::GeometryBackend::Freecad,
        ))
    } else {
        macro_path.clone()
    };
    let fcstd_path = bundle_dir.join(FCSTD_FILE_NAME);
    let preview_stl_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
    let step_path = bundle_dir.join(STEP_FILE_NAME);
    let runner_report_path = bundle_dir.join(RUNNER_REPORT_FILE_NAME);
    let parts_dir = bundle_dir.join(PARTS_DIR_NAME);
    fs::create_dir_all(&parts_dir).map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(&macro_path, source_identity)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    if runner_macro_path != macro_path {
        fs::write(&runner_macro_path, executable_source)
            .map_err(|err| AppError::persistence(err.to_string()))?;
    }
    ensure_runtime_sdk(app, &bundle_dir)?;

    run_generate_runner(
        app,
        configured_freecad_cmd,
        &runner_macro_path,
        &preview_stl_path,
        &fcstd_path,
        &step_path,
        &parts_dir,
        &runner_report_path,
        &params_json,
    )?;

    let report =
        normalize_runner_report_paths(&bundle_dir, read_runner_report(&runner_report_path)?)?;
    let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
    let manifest = build_manifest(
        &model_id,
        ModelSourceKind::Generated,
        parameters.keys().cloned().collect(),
        &report,
        Some(path_to_string(&macro_path)?),
        source_language,
    )?;
    write_manifest(&manifest_path, &manifest)?;

    let bundle = build_bundle(
        &model_id,
        ModelSourceKind::Generated,
        &content_hash,
        1,
        &fcstd_path,
        &manifest_path,
        Some(&macro_path),
        &preview_stl_path,
        &step_path,
        &manifest,
        source_language,
    )?;
    write_bundle(&bundle_dir, &bundle)?;
    Ok(bundle)
}

pub fn import_fcstd(
    source_path: &str,
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let source_path = PathBuf::from(source_path);
    if !source_path.exists() {
        return Err(AppError::not_found(format!(
            "FCStd file '{}' was not found.",
            source_path.display()
        )));
    }

    let source_bytes =
        fs::read(&source_path).map_err(|err| AppError::persistence(err.to_string()))?;
    let content_hash = digest_segments([source_bytes.as_slice()]);
    let model_id = format!("imported-fcstd-{}", short_digest(&content_hash));
    let bundle_dir = artifact_dir(app, ModelSourceKind::ImportedFcstd, &model_id)?;

    if let Some(bundle) = load_cached_bundle(&bundle_dir)? {
        return Ok(bundle);
    }

    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let fcstd_path = bundle_dir.join(FCSTD_FILE_NAME);
    let preview_stl_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
    let step_path = bundle_dir.join(STEP_FILE_NAME);
    let runner_report_path = bundle_dir.join(RUNNER_REPORT_FILE_NAME);
    let parts_dir = bundle_dir.join(PARTS_DIR_NAME);
    fs::create_dir_all(&parts_dir).map_err(|err| AppError::persistence(err.to_string()))?;
    fs::copy(&source_path, &fcstd_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to persist imported FCStd '{}': {}",
            source_path.display(),
            err
        ))
    })?;

    run_import_runner(
        app,
        configured_freecad_cmd,
        &fcstd_path,
        &preview_stl_path,
        &step_path,
        &parts_dir,
        &runner_report_path,
    )?;

    let report =
        normalize_runner_report_paths(&bundle_dir, read_runner_report(&runner_report_path)?)?;
    let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
    let manifest = build_manifest(
        &model_id,
        ModelSourceKind::ImportedFcstd,
        Vec::new(),
        &report,
        Some(
            source_path
                .to_str()
                .ok_or_else(|| AppError::internal("Invalid FCStd source path."))?
                .to_string(),
        ),
        crate::models::SourceLanguage::LegacyPython,
    )?;
    write_manifest(&manifest_path, &manifest)?;

    let bundle = build_bundle(
        &model_id,
        ModelSourceKind::ImportedFcstd,
        &content_hash,
        1,
        &fcstd_path,
        &manifest_path,
        None,
        &preview_stl_path,
        &step_path,
        &manifest,
        crate::models::SourceLanguage::LegacyPython,
    )?;
    write_bundle(&bundle_dir, &bundle)?;
    Ok(bundle)
}

pub fn apply_imported_model(
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
    parameters: &DesignParams,
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
) -> AppResult<(ArtifactBundle, ModelManifest)> {
    if bundle.source_kind != ModelSourceKind::ImportedFcstd {
        return Err(AppError::validation(
            "apply_imported_model only supports imported FCStd bundles.",
        ));
    }
    if manifest.source_kind != ModelSourceKind::ImportedFcstd {
        return Err(AppError::validation(
            "apply_imported_model requires an imported FCStd manifest.",
        ));
    }
    if bundle.model_id != manifest.model_id {
        return Err(AppError::validation(
            "Imported artifact bundle does not match the provided manifest model id.",
        ));
    }

    let bundle_dir = bundle_dir_from_model_id(app, &bundle.model_id)?;
    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let fcstd_path = PathBuf::from(&bundle.fcstd_path);
    let preview_stl_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
    let step_path = bundle_dir.join(STEP_FILE_NAME);
    let runner_report_path = bundle_dir.join(RUNNER_REPORT_FILE_NAME);
    let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
    let parts_dir = bundle_dir.join(PARTS_DIR_NAME);
    fs::create_dir_all(&parts_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let params_json =
        serde_json::to_string(parameters).map_err(|err| AppError::validation(err.to_string()))?;
    let bindings_json = serde_json::to_string(&runner_bindings_from_manifest(manifest))
        .map_err(|err| AppError::validation(err.to_string()))?;

    run_apply_import_runner(
        app,
        configured_freecad_cmd,
        &fcstd_path,
        &preview_stl_path,
        &step_path,
        &parts_dir,
        &runner_report_path,
        &params_json,
        &bindings_json,
    )?;

    let report =
        normalize_runner_report_paths(&bundle_dir, read_runner_report(&runner_report_path)?)?;
    let next_manifest = rebuild_imported_manifest(manifest, &report)?;
    write_manifest(&manifest_path, &next_manifest)?;

    let content_hash = content_hash_for_path(&fcstd_path)?;
    let next_bundle = build_bundle(
        &bundle.model_id,
        ModelSourceKind::ImportedFcstd,
        &content_hash,
        bundle.artifact_version.saturating_add(1),
        &fcstd_path,
        &manifest_path,
        None,
        &preview_stl_path,
        &step_path,
        &next_manifest,
        crate::models::SourceLanguage::LegacyPython,
    )?;
    write_bundle(&bundle_dir, &next_bundle)?;
    Ok((next_bundle, next_manifest))
}

pub fn get_model_manifest(app: &dyn PathResolver, model_id: &str) -> AppResult<ModelManifest> {
    let manifest_path = bundle_dir_from_model_id(app, model_id)?.join(MANIFEST_FILE_NAME);
    let raw = fs::read_to_string(&manifest_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read model manifest '{}': {}",
            manifest_path.display(),
            err
        ))
    })?;
    let manifest: ModelManifest = serde_json::from_str(&raw)
        .map_err(|err| AppError::parse(format!("Failed to parse model manifest: {}", err)))?;
    validate_model_manifest(&manifest)?;
    Ok(manifest)
}

pub fn get_artifact_bundle(app: &dyn PathResolver, model_id: &str) -> AppResult<ArtifactBundle> {
    let bundle_dir = bundle_dir_from_model_id(app, model_id)?;
    load_cached_bundle(&bundle_dir)?
        .ok_or_else(|| AppError::not_found(format!("No artifact bundle for model '{}'.", model_id)))
}

pub fn extract_brep_hidden_line_projections(
    app: &dyn PathResolver,
    configured_freecad_cmd: Option<&str>,
    request: BrepHiddenLineProjectionRequest,
) -> AppResult<BrepHiddenLineProjectionResponse> {
    let artifact_bundle = request.artifact_bundle;
    let sketch_document = request.sketch_document;
    let tolerance = request.tolerance.unwrap_or(0.1);
    let fcstd_path = validate_hidden_line_artifact(&artifact_bundle)?;
    if request
        .views
        .iter()
        .any(|view| matches!(view, SketchView::Custom))
    {
        return Err(AppError::validation(
            "Exact BRep hidden-line supports front/top/side views only. Custom views need an explicit projection plane.",
        ));
    }

    let bundle_dir = fcstd_path
        .parent()
        .ok_or_else(|| AppError::validation("FCStd artifact path has no parent directory."))?;
    let report_path = bundle_dir.join(HIDDEN_LINE_REPORT_FILE_NAME);
    let views_json = serde_json::to_string(&request.views).map_err(|err| {
        AppError::validation(format!("Failed to serialize projection views: {}", err))
    })?;

    run_hidden_line_runner(
        app,
        configured_freecad_cmd,
        &fcstd_path,
        &report_path,
        &views_json,
        tolerance,
    )?;

    let report = read_hidden_line_projection_report(&report_path)?;
    if report.views.is_empty() {
        return Err(AppError::render(
            "FreeCAD hidden-line projection produced no views.",
        ));
    }
    let mut response = BrepHiddenLineProjectionResponse {
        model_id: artifact_bundle.model_id,
        source_artifact_path: report.source_artifact_path,
        views: report.views,
        warnings: report.warnings,
        validation: None,
    };
    if let Some(document) = sketch_document.as_ref() {
        response.validation = Some(
            crate::sketch_brep_validation::validate_sketch_brep_hidden_line_projection(
                document, &response, tolerance,
            ),
        );
    }
    Ok(response)
}

pub fn save_model_manifest(
    app: &dyn PathResolver,
    model_id: &str,
    manifest: &ModelManifest,
) -> AppResult<()> {
    if manifest.model_id != model_id {
        return Err(AppError::validation(format!(
            "Manifest modelId '{}' does not match requested model id '{}'.",
            manifest.model_id, model_id
        )));
    }
    validate_model_manifest(manifest)?;
    let manifest_path = bundle_dir_from_model_id(app, model_id)?.join(MANIFEST_FILE_NAME);
    write_manifest(&manifest_path, manifest)?;
    refresh_bundle_for_manifest(&manifest_path, manifest)
}

pub fn get_default_macro(app: &dyn PathResolver) -> AppResult<String> {
    let path = resolve_resource_path(
        app,
        DEFAULT_MACRO_RESOURCE_PATH,
        &[
            "../templates/cache_pot_default.FCMacro",
            "templates/cache_pot_default.FCMacro",
        ],
    )?;

    fs::read_to_string(path)
        .map_err(|err| AppError::persistence(format!("Failed to read default macro: {}", err)))
}

pub fn runtime_cache_dir(app: &dyn PathResolver) -> AppResult<PathBuf> {
    let app_dir = app.app_data_dir();
    let runtime_root = app_dir.join(MODEL_RUNTIME_ROOT);
    fs::create_dir_all(&runtime_root).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(runtime_root)
}

const MAX_CACHE_BYTES: u64 = 500 * 1024 * 1024;

pub fn evict_cache_if_needed(cache_dir: &Path) {
    let mut bundle_dirs = Vec::new();
    collect_bundle_dirs(cache_dir, &mut bundle_dirs);

    let mut total: u64 = 0;
    let mut entries: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();

    for bundle_dir in bundle_dirs {
        let size = dir_size(&bundle_dir);
        if size == 0 {
            continue;
        }
        let modified = fs::metadata(&bundle_dir)
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        total += size;
        entries.push((bundle_dir, size, modified));
    }

    if total <= MAX_CACHE_BYTES {
        return;
    }

    entries.sort_by_key(|(_, _, modified)| *modified);

    for (path, size, _) in entries {
        if total <= MAX_CACHE_BYTES {
            break;
        }
        if fs::remove_dir_all(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

pub fn resolve_resource_path(
    app: &dyn PathResolver,
    resource_path: &str,
    fallback_paths: &[&str],
) -> AppResult<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = app.resource_path(resource_path) {
        candidates.push(path);
    }

    for fallback in fallback_paths {
        candidates.push(PathBuf::from(fallback));
    }

    if let Some(path) = candidates.iter().find(|candidate| candidate.exists()) {
        return Ok(path.clone());
    }

    let checked = candidates
        .iter()
        .map(|candidate| candidate.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    Err(AppError::persistence(format!(
        "Required resource '{}' was not found. Checked: {}",
        resource_path, checked
    )))
}

fn ensure_runtime_sdk(app: &dyn PathResolver, bundle_dir: &Path) -> AppResult<()> {
    let source = resolve_resource_path(
        app,
        CAD_SDK_RESOURCE_PATH,
        &["../model-runtime/cad_sdk.py", "model-runtime/cad_sdk.py"],
    )?;
    let target = bundle_dir.join("cad_sdk.py");
    fs::copy(&source, &target).map_err(|err| {
        AppError::persistence(format!(
            "Failed to copy CAD SDK from '{}' to '{}': {}",
            source.display(),
            target.display(),
            err
        ))
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_bundle(
    model_id: &str,
    source_kind: ModelSourceKind,
    content_hash: &str,
    artifact_version: u32,
    fcstd_path: &Path,
    manifest_path: &Path,
    macro_path: Option<&Path>,
    preview_stl_path: &Path,
    step_path: &Path,
    manifest: &ModelManifest,
    source_language: crate::models::SourceLanguage,
) -> AppResult<ArtifactBundle> {
    let bundle_dir = manifest_path
        .parent()
        .ok_or_else(|| AppError::internal("Manifest path missing parent."))?;
    Ok(ArtifactBundle {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind,
        engine_kind: source_language.to_engine_kind(),
        source_language,
        geometry_backend: crate::models::GeometryBackend::Freecad,
        content_hash: content_hash.to_string(),
        artifact_version,
        fcstd_path: path_to_string(fcstd_path)?,
        manifest_path: path_to_string(manifest_path)?,
        macro_path: macro_path.map(path_to_string).transpose()?,
        preview_stl_path: path_to_string(preview_stl_path)?,
        viewer_assets: viewer_assets_from_manifest(bundle_dir, manifest)?,
        edge_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: step_export_artifacts(step_path)?,
    })
}

fn content_hash_for_path(path: &Path) -> AppResult<String> {
    let bytes = fs::read(path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read artifact content '{}': {}",
            path.display(),
            err
        ))
    })?;
    Ok(digest_segments([bytes.as_slice()]))
}

fn viewer_assets_from_manifest(
    bundle_dir: &Path,
    manifest: &ModelManifest,
) -> AppResult<Vec<ViewerAsset>> {
    let mut assets = Vec::new();
    for part in &manifest.parts {
        let Some(path) = part.viewer_asset_path.as_ref() else {
            continue;
        };
        let normalized_path =
            path_to_string(&normalize_bundle_relative_path(bundle_dir, Path::new(path)))?;
        assets.extend(part.viewer_node_ids.iter().map(|node_id| ViewerAsset {
            part_id: part.part_id.clone(),
            node_id: node_id.clone(),
            object_name: part.freecad_object_name.clone(),
            label: part.label.clone(),
            path: normalized_path.clone(),
            format: ViewerAssetFormat::Stl,
        }));
    }
    Ok(assets)
}

fn step_export_artifacts(step_path: &Path) -> AppResult<Vec<ExportArtifact>> {
    Ok(vec![ExportArtifact {
        label: "STEP".to_string(),
        format: "step".to_string(),
        path: path_to_string(step_path)?,
        role: "primary".to_string(),
    }])
}

fn build_manifest(
    model_id: &str,
    source_kind: ModelSourceKind,
    parameter_keys: Vec<String>,
    report: &RunnerReport,
    source_path: Option<String>,
    source_language: crate::models::SourceLanguage,
) -> AppResult<ModelManifest> {
    let mut parts = Vec::new();
    let mut selection_targets = Vec::new();
    let mut parameter_groups = Vec::new();
    let mut enrichment_proposals = Vec::new();
    let mut warnings = report.warnings.clone();

    let generated_is_editable =
        matches!(source_kind, ModelSourceKind::Generated) && !parameter_keys.is_empty();
    let parameter_assignments = if generated_is_editable {
        assign_generated_parameters(&parameter_keys, &report.objects)
    } else {
        vec![Vec::new(); report.objects.len()]
    };

    for (index, object) in report.objects.iter().enumerate() {
        let part_id = stable_part_id(&object.object_name);
        let node_id = object.object_name.clone();
        let label = if object.label.trim().is_empty() {
            object.object_name.clone()
        } else {
            object.label.clone()
        };
        let object_parameter_keys = parameter_assignments
            .get(index)
            .cloned()
            .unwrap_or_default();
        let is_part_editable = !object_parameter_keys.is_empty();

        parts.push(PartBinding {
            part_id: part_id.clone(),
            freecad_object_name: object.object_name.clone(),
            label: label.clone(),
            kind: object.type_id.clone(),
            semantic_role: Some(infer_semantic_role(&label, &object.object_name)),
            viewer_asset_path: Some(object.export_path.clone()),
            viewer_node_ids: vec![node_id.clone()],
            parameter_keys: object_parameter_keys.clone(),
            editable: is_part_editable,
            bounds: object.bounds.clone().map(Into::into),
            volume: object.volume,
            area: object.area,
        });

        selection_targets.push(SelectionTarget {
            target_id: Some(format!("target-{}", part_id)),
            part_id: part_id.clone(),
            viewer_node_id: node_id,
            label: label.clone(),
            kind: SelectionTargetKind::Object,
            editable: is_part_editable,
            parameter_keys: object_parameter_keys.clone(),
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        });

        if is_part_editable {
            parameter_groups.push(ParameterGroup {
                group_id: format!("group-{}", part_id),
                label: label.clone(),
                parameter_keys: object_parameter_keys,
                part_ids: vec![part_id.clone()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(index as u32),
            });
        }

        if matches!(source_kind, ModelSourceKind::ImportedFcstd) {
            enrichment_proposals.push(import_enrichment_proposal(&part_id, &label, object));
        }
    }

    if matches!(source_kind, ModelSourceKind::ImportedFcstd) {
        warnings.push(
            "Imported FCStd models are inspect-only until bindings are confirmed.".to_string(),
        );
    }

    let manifest = ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind,
        engine_kind: source_language.to_engine_kind(),
        source_language,
        geometry_backend: crate::models::GeometryBackend::Freecad,
        document: DocumentMetadata {
            document_name: if report.document_name.trim().is_empty() {
                "EckyCAD".to_string()
            } else {
                report.document_name.clone()
            },
            document_label: if report.document_label.trim().is_empty() {
                report.document_name.clone()
            } else {
                report.document_label.clone()
            },
            source_path,
            object_count: parts.len(),
            warnings: report.warnings.clone(),
        },
        parts,
        parameter_groups,
        control_primitives: Vec::new(),
        control_relations: Vec::new(),
        control_views: Vec::new(),
        advisories: Vec::new(),
        selection_targets,
        measurement_annotations: Vec::new(),
        warnings,
        enrichment_state: ManifestEnrichmentState {
            status: if enrichment_proposals.is_empty() {
                EnrichmentStatus::None
            } else {
                EnrichmentStatus::Pending
            },
            proposals: enrichment_proposals,
        },
    };

    validate_model_manifest(&manifest)?;
    Ok(manifest)
}

fn proposal_group_id(proposal_id: &str) -> String {
    format!("proposal-bind-{}", proposal_id)
}

fn derive_enrichment_status(proposals: &[EnrichmentProposal]) -> EnrichmentStatus {
    if proposals
        .iter()
        .any(|proposal| proposal.status == EnrichmentStatus::Pending)
    {
        EnrichmentStatus::Pending
    } else if proposals
        .iter()
        .any(|proposal| proposal.status == EnrichmentStatus::Accepted)
    {
        EnrichmentStatus::Accepted
    } else if proposals
        .iter()
        .any(|proposal| proposal.status == EnrichmentStatus::Rejected)
    {
        EnrichmentStatus::Rejected
    } else {
        EnrichmentStatus::None
    }
}

fn merge_imported_proposals(
    previous: &[EnrichmentProposal],
    current: Vec<EnrichmentProposal>,
) -> Vec<EnrichmentProposal> {
    let previous_by_id: HashMap<&str, &EnrichmentProposal> = previous
        .iter()
        .map(|proposal| (proposal.proposal_id.as_str(), proposal))
        .collect();

    current
        .into_iter()
        .map(|mut proposal| {
            if let Some(existing) = previous_by_id.get(proposal.proposal_id.as_str()) {
                proposal.status = existing.status.clone();
                proposal.confidence = existing.confidence;
            }
            proposal
        })
        .collect()
}

fn rebuild_imported_manifest(
    previous_manifest: &ModelManifest,
    report: &RunnerReport,
) -> AppResult<ModelManifest> {
    let mut base_manifest = build_manifest(
        &previous_manifest.model_id,
        ModelSourceKind::ImportedFcstd,
        Vec::new(),
        report,
        previous_manifest.document.source_path.clone(),
        crate::models::SourceLanguage::LegacyPython,
    )?;

    let merged_proposals = merge_imported_proposals(
        &previous_manifest.enrichment_state.proposals,
        base_manifest.enrichment_state.proposals.clone(),
    );
    let accepted = merged_proposals
        .iter()
        .filter(|proposal| proposal.status == EnrichmentStatus::Accepted)
        .cloned()
        .collect::<Vec<_>>();
    let preserved_groups = previous_manifest
        .parameter_groups
        .iter()
        .filter(|group| !group.group_id.starts_with("proposal-bind-"))
        .cloned()
        .collect::<Vec<_>>();
    let preserved_group_keys = preserved_keys_by_part(&preserved_groups);
    let accepted_keys = accepted_keys_by_part(&accepted);

    base_manifest.enrichment_state.proposals = merged_proposals.clone();
    base_manifest.enrichment_state.status = derive_enrichment_status(&merged_proposals);
    base_manifest.parameter_groups = preserved_groups.clone();
    base_manifest
        .parameter_groups
        .extend(accepted.iter().map(|proposal| ParameterGroup {
            group_id: proposal_group_id(&proposal.proposal_id),
            label: proposal.label.clone(),
            parameter_keys: unique_strings(proposal.parameter_keys.clone()),
            part_ids: unique_strings(proposal.part_ids.clone()),
            editable: true,
            presentation: Some("primary".to_string()),
            order: None,
        }));

    let editable_part_ids: HashSet<String> = base_manifest
        .parts
        .iter_mut()
        .map(|part| {
            let mut parameter_keys = preserved_group_keys
                .get(part.part_id.as_str())
                .cloned()
                .unwrap_or_default();
            parameter_keys.extend(
                accepted_keys
                    .get(part.part_id.as_str())
                    .cloned()
                    .unwrap_or_default(),
            );
            part.parameter_keys = unique_strings(parameter_keys);
            part.editable = !part.parameter_keys.is_empty();
            (part.part_id.clone(), part.editable)
        })
        .filter(|(_, editable)| *editable)
        .map(|(part_id, _)| part_id)
        .collect();

    for target in &mut base_manifest.selection_targets {
        target.editable = editable_part_ids.contains(&target.part_id);
    }

    base_manifest.warnings.retain(|warning| {
        warning != "Imported FCStd models are inspect-only until bindings are confirmed."
            && warning != "Imported FCStd bindings were accepted from heuristic proposals."
    });
    if accepted.is_empty() {
        base_manifest.warnings.push(
            "Imported FCStd models are inspect-only until bindings are confirmed.".to_string(),
        );
    } else {
        base_manifest
            .warnings
            .push("Imported FCStd bindings were accepted from heuristic proposals.".to_string());
    }

    validate_model_manifest(&base_manifest)?;
    Ok(base_manifest)
}

fn preserved_keys_by_part(groups: &[ParameterGroup]) -> HashMap<&str, Vec<String>> {
    let mut keys_by_part: HashMap<&str, Vec<String>> = HashMap::new();
    for group in groups {
        for part_id in &group.part_ids {
            let bucket = keys_by_part.entry(part_id.as_str()).or_default();
            bucket.extend(group.parameter_keys.clone());
        }
    }
    keys_by_part
}

fn accepted_keys_by_part(proposals: &[EnrichmentProposal]) -> HashMap<&str, Vec<String>> {
    let mut keys_by_part: HashMap<&str, Vec<String>> = HashMap::new();
    for proposal in proposals {
        for part_id in &proposal.part_ids {
            let bucket = keys_by_part.entry(part_id.as_str()).or_default();
            bucket.extend(proposal.parameter_keys.clone());
        }
    }
    keys_by_part
}

fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            unique.push(value);
        }
    }
    unique
}

fn runner_bindings_from_manifest(manifest: &ModelManifest) -> Vec<RunnerBinding> {
    manifest
        .parts
        .iter()
        .filter(|part| part.editable && !part.parameter_keys.is_empty())
        .map(|part| RunnerBinding {
            object_name: part.freecad_object_name.clone(),
            parameter_keys: part.parameter_keys.clone(),
        })
        .collect()
}

fn write_manifest(path: &Path, manifest: &ModelManifest) -> AppResult<()> {
    let data = serde_json::to_string_pretty(manifest)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write model manifest '{}': {}",
            path.display(),
            err
        ))
    })
}

fn write_bundle(bundle_dir: &Path, bundle: &ArtifactBundle) -> AppResult<()> {
    let path = bundle_dir.join(BUNDLE_FILE_NAME);
    let data = serde_json::to_string_pretty(bundle)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

fn load_cached_bundle(bundle_dir: &Path) -> AppResult<Option<ArtifactBundle>> {
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    if !bundle_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&bundle_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read artifact bundle '{}': {}",
            bundle_path.display(),
            err
        ))
    })?;
    let bundle: ArtifactBundle = serde_json::from_str(&raw)
        .map_err(|err| AppError::parse(format!("Failed to parse artifact bundle: {}", err)))?;
    let cached = normalize_cached_bundle(bundle_dir, bundle.clone())?;
    if let Some(ref repaired) = cached {
        if repaired != &bundle {
            write_bundle(bundle_dir, repaired)?;
        }
    }
    Ok(cached)
}

fn refresh_bundle_for_manifest(manifest_path: &Path, manifest: &ModelManifest) -> AppResult<()> {
    let Some(bundle_dir) = manifest_path.parent() else {
        return Ok(());
    };
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    if !bundle_path.exists() {
        return Ok(());
    }

    let raw = fs::read_to_string(&bundle_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read artifact bundle '{}': {}",
            bundle_path.display(),
            err
        ))
    })?;
    let bundle: ArtifactBundle = serde_json::from_str(&raw)
        .map_err(|err| AppError::parse(format!("Failed to parse artifact bundle: {}", err)))?;
    let refreshed = bundle_from_manifest(bundle_dir, bundle, manifest)?;
    write_bundle(bundle_dir, &refreshed)
}

fn normalize_cached_bundle(
    bundle_dir: &Path,
    bundle: ArtifactBundle,
) -> AppResult<Option<ArtifactBundle>> {
    let manifest = match load_manifest_for_bundle_dir(bundle_dir, &bundle)? {
        Some(manifest) => manifest,
        None => return Ok(None),
    };
    let bundle = bundle_from_manifest(bundle_dir, bundle, &manifest)?;
    if !Path::new(&bundle.fcstd_path).exists()
        || !Path::new(&bundle.manifest_path).exists()
        || !Path::new(&bundle.preview_stl_path).exists()
        || !bundle_step_path(bundle_dir).exists()
        || bundle
            .viewer_assets
            .iter()
            .any(|asset| !Path::new(&asset.path).exists())
    {
        return Ok(None);
    }
    Ok(Some(bundle))
}

fn load_manifest_for_bundle_dir(
    bundle_dir: &Path,
    bundle: &ArtifactBundle,
) -> AppResult<Option<ModelManifest>> {
    let manifest_path = canonical_manifest_path(bundle_dir, bundle);
    if !manifest_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&manifest_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read model manifest '{}': {}",
            manifest_path.display(),
            err
        ))
    })?;
    let manifest: ModelManifest = serde_json::from_str(&raw)
        .map_err(|err| AppError::parse(format!("Failed to parse model manifest: {}", err)))?;
    validate_model_manifest(&manifest)?;
    Ok(Some(manifest))
}

fn bundle_from_manifest(
    bundle_dir: &Path,
    mut bundle: ArtifactBundle,
    manifest: &ModelManifest,
) -> AppResult<ArtifactBundle> {
    if bundle.model_id != manifest.model_id
        || bundle.source_kind != manifest.source_kind
        || bundle.geometry_backend != crate::models::GeometryBackend::Freecad
        || manifest.geometry_backend != crate::models::GeometryBackend::Freecad
    {
        return Err(AppError::validation(
            "Cached FreeCAD bundle does not match the model manifest.",
        ));
    }

    bundle.schema_version = manifest.schema_version;
    bundle.engine_kind = manifest.engine_kind;
    bundle.source_language = manifest.source_language;
    bundle.geometry_backend = manifest.geometry_backend;
    bundle.fcstd_path = path_to_string(&canonical_fcstd_path(bundle_dir, &bundle))?;
    bundle.manifest_path = path_to_string(&canonical_manifest_path(bundle_dir, &bundle))?;
    bundle.preview_stl_path = path_to_string(&canonical_preview_path(bundle_dir, &bundle))?;
    bundle.export_artifacts = step_export_artifacts(&canonical_step_path(bundle_dir, &bundle))?;
    bundle.viewer_assets = viewer_assets_from_manifest(bundle_dir, manifest)?;
    crate::models::validate_model_runtime_bundle(manifest, &bundle)?;
    Ok(bundle)
}

fn canonical_fcstd_path(bundle_dir: &Path, bundle: &ArtifactBundle) -> PathBuf {
    let canonical = bundle_dir.join(FCSTD_FILE_NAME);
    if canonical.exists() {
        canonical
    } else {
        normalize_bundle_relative_path(bundle_dir, Path::new(&bundle.fcstd_path))
    }
}

fn canonical_manifest_path(bundle_dir: &Path, bundle: &ArtifactBundle) -> PathBuf {
    let canonical = bundle_dir.join(MANIFEST_FILE_NAME);
    if canonical.exists() {
        canonical
    } else {
        normalize_bundle_relative_path(bundle_dir, Path::new(&bundle.manifest_path))
    }
}

fn canonical_preview_path(bundle_dir: &Path, bundle: &ArtifactBundle) -> PathBuf {
    let canonical = bundle_dir.join(PREVIEW_STL_FILE_NAME);
    if canonical.exists() {
        canonical
    } else {
        normalize_bundle_relative_path(bundle_dir, Path::new(&bundle.preview_stl_path))
    }
}

fn bundle_step_path(bundle_dir: &Path) -> PathBuf {
    bundle_dir.join(STEP_FILE_NAME)
}

fn canonical_step_path(bundle_dir: &Path, bundle: &ArtifactBundle) -> PathBuf {
    let canonical = bundle_step_path(bundle_dir);
    if canonical.exists() {
        return canonical;
    }
    bundle
        .export_artifacts
        .iter()
        .find(|artifact| artifact.format == "step")
        .map(|artifact| normalize_bundle_relative_path(bundle_dir, Path::new(&artifact.path)))
        .unwrap_or(canonical)
}

fn normalize_bundle_relative_path(bundle_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        bundle_dir.join(path)
    }
}

fn normalize_runner_report_paths(
    bundle_dir: &Path,
    mut report: RunnerReport,
) -> AppResult<RunnerReport> {
    for object in &mut report.objects {
        object.export_path = path_to_string(&normalize_bundle_relative_path(
            bundle_dir,
            Path::new(&object.export_path),
        ))?;
    }
    Ok(report)
}

fn read_runner_report(path: &Path) -> AppResult<RunnerReport> {
    let raw = fs::read_to_string(path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read FreeCAD runner report '{}': {}",
            path.display(),
            err
        ))
    })?;
    let report: RunnerReport = serde_json::from_str(&raw)
        .map_err(|err| AppError::parse(format!("Failed to parse runner report: {}", err)))?;
    if report.objects.is_empty() {
        return Err(AppError::render(
            "FreeCAD runner did not export any objects.".to_string(),
        ));
    }
    Ok(report)
}

fn validate_hidden_line_artifact(bundle: &ArtifactBundle) -> AppResult<PathBuf> {
    let fcstd_path = bundle.fcstd_path.trim();
    if bundle.geometry_backend != crate::models::GeometryBackend::Freecad || fcstd_path.is_empty() {
        return Err(AppError::with_details(
            crate::models::AppErrorCode::Validation,
            "Exact BRep hidden-line requires a FreeCAD/OCCT FCStd artifact.",
            format!(
                "geometryBackend={}; fcstdPath={}",
                bundle.geometry_backend.as_str(),
                bundle.fcstd_path
            ),
        ));
    }

    let path = PathBuf::from(fcstd_path);
    if !path.exists() {
        return Err(AppError::not_found(format!(
            "FCStd artifact for exact BRep hidden-line was not found at '{}'.",
            path.display()
        )));
    }
    Ok(path)
}

fn read_hidden_line_projection_report(path: &Path) -> AppResult<HiddenLineProjectionReport> {
    let raw = fs::read_to_string(path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read FreeCAD hidden-line projection report '{}': {}",
            path.display(),
            err
        ))
    })?;
    serde_json::from_str(&raw).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse FreeCAD hidden-line projection report '{}': {}",
            path.display(),
            err
        ))
    })
}

#[allow(clippy::too_many_arguments)]
fn run_generate_runner(
    app: &dyn PathResolver,
    configured_freecad_cmd: Option<&str>,
    macro_path: &Path,
    preview_stl_path: &Path,
    fcstd_path: &Path,
    step_path: &Path,
    parts_dir: &Path,
    runner_report_path: &Path,
    params_json: &str,
) -> AppResult<()> {
    let mut command = base_runner_command(app, configured_freecad_cmd)?;
    command
        .env("ECKYCAD_MODE", "generate")
        .env("ECKYCAD_MACRO", path_to_string(macro_path)?)
        .env("ECKYCAD_STL", path_to_string(preview_stl_path)?)
        .env("ECKYCAD_FCSTD", path_to_string(fcstd_path)?)
        .env("ECKYCAD_STEP", path_to_string(step_path)?)
        .env("ECKYCAD_PARTS_DIR", path_to_string(parts_dir)?)
        .env("ECKYCAD_REPORT", path_to_string(runner_report_path)?)
        .env(
            "ECKYCAD_SDK_PATH",
            path_to_string(
                macro_path
                    .parent()
                    .ok_or_else(|| AppError::internal("Macro path missing parent."))?,
            )?,
        )
        .env("ECKYCAD_PARAMS", params_json);
    run_command(command)
}

fn run_import_runner(
    app: &dyn PathResolver,
    configured_freecad_cmd: Option<&str>,
    fcstd_path: &Path,
    preview_stl_path: &Path,
    step_path: &Path,
    parts_dir: &Path,
    runner_report_path: &Path,
) -> AppResult<()> {
    let mut command = base_runner_command(app, configured_freecad_cmd)?;
    command
        .env("ECKYCAD_MODE", "import_fcstd")
        .env("ECKYCAD_IMPORT_FCSTD", path_to_string(fcstd_path)?)
        .env("ECKYCAD_STL", path_to_string(preview_stl_path)?)
        .env("ECKYCAD_STEP", path_to_string(step_path)?)
        .env("ECKYCAD_PARTS_DIR", path_to_string(parts_dir)?)
        .env("ECKYCAD_REPORT", path_to_string(runner_report_path)?)
        .env("ECKYCAD_PARAMS", "{}");
    run_command(command)
}

fn run_hidden_line_runner(
    app: &dyn PathResolver,
    configured_freecad_cmd: Option<&str>,
    fcstd_path: &Path,
    projection_report_path: &Path,
    views_json: &str,
    tolerance: f64,
) -> AppResult<()> {
    let mut command = base_runner_command(app, configured_freecad_cmd)?;
    command
        .env("ECKYCAD_MODE", "hidden_line_projection")
        .env("ECKYCAD_IMPORT_FCSTD", path_to_string(fcstd_path)?)
        .env("ECKYCAD_REPORT", path_to_string(projection_report_path)?)
        .env("ECKYCAD_PROJECTION_VIEWS", views_json)
        .env("ECKYCAD_PROJECTION_TOLERANCE", tolerance.to_string())
        .env("ECKYCAD_PARAMS", "{}");
    run_command(command)
}

#[allow(clippy::too_many_arguments)]
fn run_apply_import_runner(
    app: &dyn PathResolver,
    configured_freecad_cmd: Option<&str>,
    fcstd_path: &Path,
    preview_stl_path: &Path,
    step_path: &Path,
    parts_dir: &Path,
    runner_report_path: &Path,
    params_json: &str,
    bindings_json: &str,
) -> AppResult<()> {
    let mut command = base_runner_command(app, configured_freecad_cmd)?;
    command
        .env("ECKYCAD_MODE", "apply_imported_fcstd")
        .env("ECKYCAD_IMPORT_FCSTD", path_to_string(fcstd_path)?)
        .env("ECKYCAD_FCSTD", path_to_string(fcstd_path)?)
        .env("ECKYCAD_STL", path_to_string(preview_stl_path)?)
        .env("ECKYCAD_STEP", path_to_string(step_path)?)
        .env("ECKYCAD_PARTS_DIR", path_to_string(parts_dir)?)
        .env("ECKYCAD_REPORT", path_to_string(runner_report_path)?)
        .env("ECKYCAD_PARAMS", params_json)
        .env("ECKYCAD_BINDINGS", bindings_json);
    run_command(command)
}

fn base_runner_command(
    app: &dyn PathResolver,
    configured_freecad_cmd: Option<&str>,
) -> AppResult<Command> {
    let freecad_cmd = resolve_freecad_path(configured_freecad_cmd)?;
    let runner_path = resolve_resource_path(
        app,
        RUNNER_RESOURCE_PATH,
        &["../server/freecad_runner.py", "server/freecad_runner.py"],
    )?;
    let mut command = Command::new(&freecad_cmd);
    command.arg(&runner_path);
    Ok(command)
}

fn run_command(mut command: Command) -> AppResult<()> {
    let output = command
        .output()
        .map_err(|err| AppError::render(format!("Failed to execute FreeCAD command: {}", err)))?;

    if !output.status.success() {
        return Err(AppError::with_details(
            crate::models::AppErrorCode::Render,
            "FreeCAD runner failed.",
            format!(
                "stdout:\n{}\n\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    Ok(())
}

fn artifact_dir(
    app: &dyn PathResolver,
    source_kind: ModelSourceKind,
    model_id: &str,
) -> AppResult<PathBuf> {
    let root = runtime_cache_dir(app)?;
    Ok(root.join(source_kind_dir_name(source_kind)).join(model_id))
}

fn bundle_dir_from_model_id(app: &dyn PathResolver, model_id: &str) -> AppResult<PathBuf> {
    let source_kind = if model_id.starts_with("generated-") {
        ModelSourceKind::Generated
    } else if model_id.starts_with("imported-fcstd-") {
        ModelSourceKind::ImportedFcstd
    } else {
        return Err(AppError::not_found(format!(
            "Unknown model id '{}'.",
            model_id
        )));
    };
    artifact_dir(app, source_kind, model_id)
}

fn source_kind_dir_name(source_kind: ModelSourceKind) -> &'static str {
    match source_kind {
        ModelSourceKind::Generated => GENERATED_ARTIFACT_DIR,
        ModelSourceKind::ImportedFcstd => IMPORTED_FCSTD_ARTIFACT_DIR,
    }
}

pub(crate) fn resolve_freecad_path(configured_freecad_cmd: Option<&str>) -> AppResult<PathBuf> {
    if let Some(configured_cmd) = configured_freecad_cmd {
        return Ok(normalize_freecad_cmd(configured_cmd));
    }

    if let Some(env_cmd) = std::env::var_os("FREECAD_CMD") {
        if !env_cmd.is_empty() {
            return Ok(normalize_freecad_cmd(env_cmd));
        }
    }

    if let Some(path) = find_on_path(&["freecadcmd", "FreeCADCmd"]) {
        return Ok(path);
    }

    #[cfg(target_os = "macos")]
    {
        let mac_candidates = [
            "/Applications/FreeCAD.app/Contents/Resources/bin/freecadcmd",
            "/Applications/FreeCAD.app/Contents/Resources/bin/FreeCADCmd",
            "/Applications/FreeCAD.app/Contents/MacOS/FreeCADCmd",
            "/Applications/FreeCAD.app/Contents/MacOS/FreeCAD",
        ];

        for candidate in mac_candidates {
            let path = Path::new(candidate);
            if path.exists() {
                return Ok(path.to_path_buf());
            }
        }
    }

    Err(AppError::render(
        "FreeCAD executable not found. Set FREECAD_CMD or install FreeCAD.app in /Applications."
            .to_string(),
    ))
}

fn normalize_freecad_cmd<T: Into<PathBuf>>(cmd: T) -> PathBuf {
    let path = cmd.into();
    resolve_freecad_bundle_path(&path).unwrap_or(path)
}

fn resolve_freecad_bundle_path(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }

    #[cfg(target_os = "macos")]
    {
        if path.extension().and_then(|value| value.to_str()) == Some("app") {
            let bundle_candidates = [
                path.join("Contents/Resources/bin/freecadcmd"),
                path.join("Contents/Resources/bin/FreeCADCmd"),
                path.join("Contents/MacOS/FreeCADCmd"),
                path.join("Contents/MacOS/FreeCAD"),
            ];

            for candidate in bundle_candidates {
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    [path.join("freecadcmd"), path.join("FreeCADCmd")]
        .into_iter()
        .find(|candidate| candidate.exists())
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;

    for dir in std::env::split_paths(&path_var) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn digest_segments<const N: usize>(segments: [&[u8]; N]) -> String {
    let mut hasher = Sha256::new();
    for segment in segments {
        hasher.update(segment);
    }
    format!("{:x}", hasher.finalize())
}

fn short_digest(digest: &str) -> &str {
    &digest[..32]
}

fn stable_part_id(object_name: &str) -> String {
    let object_hash = digest_segments([object_name.as_bytes()]);
    format!("part-{}-{}", sanitize_id(object_name), &object_hash[..10])
}

fn assign_generated_parameters(
    parameter_keys: &[String],
    objects: &[RunnerObject],
) -> Vec<Vec<String>> {
    if objects.is_empty() {
        return Vec::new();
    }

    let object_tokens: Vec<Vec<String>> = objects.iter().map(tokens_for_object).collect();
    let mut assignments = vec![Vec::new(); objects.len()];

    for key in parameter_keys {
        let param_tokens = tokenize_identifier(key);
        let scores: Vec<usize> = object_tokens
            .iter()
            .map(|tokens| parameter_match_score(&param_tokens, tokens))
            .collect();
        let best_score = scores.iter().copied().max().unwrap_or(0);

        if best_score == 0 {
            for bucket in &mut assignments {
                bucket.push(key.clone());
            }
            continue;
        }

        for (index, score) in scores.iter().enumerate() {
            if *score == best_score {
                assignments[index].push(key.clone());
            }
        }
    }

    assignments
}

fn import_enrichment_proposal(
    part_id: &str,
    part_label: &str,
    object: &RunnerObject,
) -> EnrichmentProposal {
    let normalized_label = humanize_label(part_label, &object.object_name);
    let proposal_slug = sanitize_id(&object.object_name);
    let parameter_keys =
        suggested_dimension_parameter_keys(&normalized_label, object.bounds.as_ref());

    EnrichmentProposal {
        proposal_id: format!("proposal-{}", proposal_slug),
        label: if parameter_keys.is_empty() {
            format!("Review bindings for {}", normalized_label)
        } else {
            format!("Expose {} dimensions", normalized_label)
        },
        part_ids: vec![part_id.to_string()],
        parameter_keys,
        confidence: 0.42,
        status: EnrichmentStatus::Pending,
        provenance: "heuristic.import.bounds".to_string(),
    }
}

fn tokens_for_object(object: &RunnerObject) -> Vec<String> {
    let mut tokens = tokenize_identifier(&object.object_name);
    for token in tokenize_identifier(&object.label) {
        if !tokens.contains(&token) {
            tokens.push(token);
        }
    }
    tokens
}

fn parameter_match_score(param_tokens: &[String], object_tokens: &[String]) -> usize {
    param_tokens
        .iter()
        .filter(|token| object_tokens.contains(token))
        .count()
}

fn tokenize_identifier(value: &str) -> Vec<String> {
    let mut normalized = String::new();
    let mut previous_was_word = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            let is_upper = ch.is_ascii_uppercase();
            if is_upper && previous_was_word {
                normalized.push(' ');
            }
            normalized.push(ch.to_ascii_lowercase());
            previous_was_word = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            normalized.push(' ');
            previous_was_word = false;
        }
    }

    normalized
        .split_whitespace()
        .map(|token| token.trim_end_matches(|ch: char| ch.is_ascii_digit()))
        .filter(|token| token.len() > 1)
        .filter(|token| token.chars().any(|ch| ch.is_ascii_alphabetic()))
        .filter(|token| {
            !matches!(
                *token,
                "part" | "feature" | "shape" | "mesh" | "body" | "obj"
            )
        })
        .map(ToString::to_string)
        .collect()
}

fn humanize_label(label: &str, object_name: &str) -> String {
    let source = if label.trim().is_empty() {
        object_name
    } else {
        label
    };
    let mut words = tokenize_identifier(source);
    if words.is_empty() {
        words = tokenize_identifier(object_name);
    }
    if words.is_empty() {
        return object_name.to_string();
    }

    words
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn infer_semantic_role(label: &str, object_name: &str) -> String {
    let signature = format!("{} {}", label, object_name).to_ascii_lowercase();
    if signature.contains("connector") || signature.contains("hose") || signature.contains("spout")
    {
        "connector".to_string()
    } else if signature.contains("lid") || signature.contains("cap") {
        "lid".to_string()
    } else if signature.contains("handle") || signature.contains("grip") {
        "handle".to_string()
    } else if signature.contains("base") || signature.contains("foot") {
        "base".to_string()
    } else if signature.contains("shell")
        || signature.contains("body")
        || signature.contains("pot")
        || signature.contains("cup")
        || signature.contains("container")
    {
        "body".to_string()
    } else if signature.contains("logo")
        || signature.contains("mesh")
        || signature.contains("pattern")
        || signature.contains("ornament")
    {
        "ornament".to_string()
    } else {
        "unknown".to_string()
    }
}

fn suggested_dimension_parameter_keys(
    part_label: &str,
    bounds: Option<&RunnerBounds>,
) -> Vec<String> {
    let Some(_) = bounds else {
        return Vec::new();
    };

    let slug = tokenize_identifier(part_label).join("_");
    if slug.is_empty() {
        return Vec::new();
    }

    vec![
        format!("{}_width", slug),
        format!("{}_depth", slug),
        format!("{}_height", slug),
    ]
}

fn sanitize_id(value: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }
    sanitized.trim_matches('-').to_string()
}

fn path_to_string(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| AppError::internal("Invalid UTF-8 path."))
}

fn collect_bundle_dirs(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join(BUNDLE_FILE_NAME).exists() {
            output.push(path);
            continue;
        }
        collect_bundle_dirs(&path, output);
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };

    let mut total = 0;
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            total += dir_size(&entry_path);
        } else if let Ok(meta) = fs::metadata(&entry_path) {
            total += meta.len();
        }
    }
    total
}

impl From<RunnerBounds> for ManifestBounds {
    fn from(value: RunnerBounds) -> Self {
        Self {
            x_min: value.x_min,
            y_min: value.y_min,
            z_min: value.z_min,
            x_max: value.x_max,
            y_max: value.y_max,
            z_max: value.z_max,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestResolver {
        root: PathBuf,
    }

    impl TestResolver {
        fn new(prefix: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("ecky-freecad-test-{prefix}-{nonce}"));
            fs::create_dir_all(&root).expect("create temp root");
            Self { root }
        }
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

    fn fixture_generated_report() -> RunnerReport {
        serde_json::from_str(include_str!(
            "../tests/fixtures/generated_runner_report.json"
        ))
        .expect("generated fixture should parse")
    }

    fn fixture_imported_report() -> RunnerReport {
        serde_json::from_str(include_str!(
            "../tests/fixtures/imported_runner_report.json"
        ))
        .expect("imported fixture should parse")
    }

    fn sample_report(objects: Vec<RunnerObject>) -> RunnerReport {
        RunnerReport {
            document_name: "Doc".to_string(),
            document_label: "Doc".to_string(),
            warnings: Vec::new(),
            objects,
        }
    }

    fn sample_part_binding(part_id: &str, object_name: &str, asset_path: &Path) -> PartBinding {
        PartBinding {
            part_id: part_id.to_string(),
            freecad_object_name: object_name.to_string(),
            label: object_name.to_string(),
            kind: "Part::Feature".to_string(),
            semantic_role: None,
            viewer_asset_path: Some(asset_path.to_string_lossy().to_string()),
            viewer_node_ids: vec![object_name.to_string()],
            parameter_keys: Vec::new(),
            editable: false,
            bounds: None,
            volume: None,
            area: None,
        }
    }

    fn sample_manifest(
        model_id: &str,
        source_kind: ModelSourceKind,
        asset_path: &Path,
    ) -> ModelManifest {
        let part = sample_part_binding("part-shell", "OuterShell", asset_path);
        ModelManifest {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            document: DocumentMetadata {
                document_name: "Doc".to_string(),
                document_label: "Doc".to_string(),
                source_path: None,
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: vec![part.clone()],
            parameter_groups: Vec::new(),
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: vec![SelectionTarget {
                target_id: Some("target-part-shell".to_string()),
                part_id: part.part_id.clone(),
                viewer_node_id: part.freecad_object_name.clone(),
                label: part.label.clone(),
                kind: SelectionTargetKind::Object,
                editable: false,
                parameter_keys: Vec::new(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            }],
            measurement_annotations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    fn sample_bundle(model_id: &str, source_kind: ModelSourceKind) -> ArtifactBundle {
        ArtifactBundle {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/stale.FCStd".to_string(),
            manifest_path: "/tmp/stale-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: "/tmp/stale-preview.stl".to_string(),
            viewer_assets: vec![ViewerAsset {
                part_id: "stale".to_string(),
                node_id: "stale".to_string(),
                object_name: "stale".to_string(),
                label: "stale".to_string(),
                path: "/tmp/stale-part.stl".to_string(),
                format: ViewerAssetFormat::Stl,
            }],
            edge_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        }
    }

    #[test]
    fn build_bundle_exposes_step_export_artifact() {
        let root = std::env::temp_dir().join(format!("ecky-step-bundle-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join(PARTS_DIR_NAME)).expect("bundle dirs");
        let fcstd_path = root.join(FCSTD_FILE_NAME);
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        let preview_stl_path = root.join(PREVIEW_STL_FILE_NAME);
        let step_path = root.join(STEP_FILE_NAME);
        let asset_path = root.join(PARTS_DIR_NAME).join("000-outershell.stl");
        let manifest = sample_manifest("generated-step", ModelSourceKind::Generated, &asset_path);

        let bundle = build_bundle(
            "generated-step",
            ModelSourceKind::Generated,
            "hash",
            1,
            &fcstd_path,
            &manifest_path,
            None,
            &preview_stl_path,
            &step_path,
            &manifest,
            crate::models::SourceLanguage::LegacyPython,
        )
        .expect("bundle");

        assert_eq!(bundle.export_artifacts.len(), 1);
        assert_eq!(bundle.export_artifacts[0].label, "STEP");
        assert_eq!(bundle.export_artifacts[0].format, "step");
        assert_eq!(bundle.export_artifacts[0].role, "primary");
        assert_eq!(bundle.export_artifacts[0].path, step_path.to_string_lossy());
    }

    #[test]
    fn hidden_line_validation_rejects_mesh_bundle_with_raw_backend_context() {
        let mut bundle = sample_bundle("mesh-preview", ModelSourceKind::Generated);
        bundle.geometry_backend = crate::models::GeometryBackend::EckyRust;
        bundle.fcstd_path = String::new();

        let err = validate_hidden_line_artifact(&bundle).expect_err("mesh bundle should fail");

        assert_eq!(err.code, crate::models::AppErrorCode::Validation);
        assert_eq!(
            err.message,
            "Exact BRep hidden-line requires a FreeCAD/OCCT FCStd artifact."
        );
        assert_eq!(
            err.details.as_deref(),
            Some("geometryBackend=mesh; fcstdPath=")
        );
    }

    #[test]
    fn hidden_line_validation_accepts_existing_freecad_fcstd_artifact() {
        let resolver = TestResolver::new("hidden-line-existing");
        let fcstd_path = resolver.root.join("model.FCStd");
        fs::write(&fcstd_path, b"fcstd").expect("write fcstd");
        let mut bundle = sample_bundle("freecad-preview", ModelSourceKind::Generated);
        bundle.fcstd_path = fcstd_path.to_string_lossy().to_string();

        let accepted = validate_hidden_line_artifact(&bundle).expect("valid hidden-line artifact");

        assert_eq!(accepted, fcstd_path);
    }

    #[test]
    fn hidden_line_report_parses_project_ex_visible_and_hidden_edges() {
        let resolver = TestResolver::new("hidden-line-report");
        let report_path = resolver.root.join("hidden-line-projections.json");
        fs::write(
            &report_path,
            r#"{
              "sourceArtifactPath": "/tmp/model.FCStd",
              "views": [
                {
                  "view": "front",
                  "direction": [0, -1, 0],
                  "visibleEdges": [
                    {"edgeId": "front-v-0", "points": [[0, 0], [10, 0]], "sourceClass": "V"}
                  ],
                  "hiddenEdges": [
                    {"edgeId": "front-h-0", "points": [[0, 5], [10, 5]], "sourceClass": "H"}
                  ]
                }
              ],
              "warnings": []
            }"#,
        )
        .expect("write hidden-line report");

        let report = read_hidden_line_projection_report(&report_path).expect("parse report");

        assert_eq!(report.source_artifact_path, "/tmp/model.FCStd");
        assert_eq!(report.views.len(), 1);
        assert_eq!(report.views[0].view, SketchView::Front);
        assert_eq!(report.views[0].visible_edges.len(), 1);
        assert_eq!(report.views[0].hidden_edges[0].source_class, "H");
    }

    #[test]
    fn stable_part_id_is_deterministic() {
        let first = stable_part_id("OuterShell");
        let second = stable_part_id("OuterShell");
        assert_eq!(first, second);
    }

    #[test]
    fn sanitize_id_collapses_noise() {
        assert_eq!(sanitize_id("Outer Shell::001"), "outer-shell-001");
    }

    #[test]
    fn build_manifest_assigns_matching_parameters_to_best_parts() {
        let report = sample_report(vec![
            RunnerObject {
                object_name: "OuterShell".to_string(),
                label: "Outer Shell".to_string(),
                type_id: "Part::Feature".to_string(),
                export_path: "/tmp/shell.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
            },
            RunnerObject {
                object_name: "Lid".to_string(),
                label: "Lid".to_string(),
                type_id: "Part::Feature".to_string(),
                export_path: "/tmp/lid.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
            },
        ]);

        let manifest = build_manifest(
            "generated-test",
            ModelSourceKind::Generated,
            vec![
                "shell_thickness".to_string(),
                "lid_height".to_string(),
                "overall_height".to_string(),
            ],
            &report,
            None,
            crate::models::SourceLanguage::LegacyPython,
        )
        .expect("manifest should build");

        let shell = manifest
            .parts
            .iter()
            .find(|part| part.freecad_object_name == "OuterShell")
            .expect("shell part should exist");
        let lid = manifest
            .parts
            .iter()
            .find(|part| part.freecad_object_name == "Lid")
            .expect("lid part should exist");

        assert_eq!(
            shell.parameter_keys,
            vec!["shell_thickness".to_string(), "overall_height".to_string()]
        );
        assert_eq!(
            lid.parameter_keys,
            vec!["lid_height".to_string(), "overall_height".to_string()]
        );
    }

    #[test]
    fn build_manifest_marks_unbound_generated_parts_as_inspect_only() {
        let report = sample_report(vec![
            RunnerObject {
                object_name: "Lid".to_string(),
                label: "Lid".to_string(),
                type_id: "Part::Feature".to_string(),
                export_path: "/tmp/lid.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
            },
            RunnerObject {
                object_name: "Spout".to_string(),
                label: "Spout".to_string(),
                type_id: "Part::Feature".to_string(),
                export_path: "/tmp/spout.stl".to_string(),
                bounds: None,
                volume: None,
                area: None,
            },
        ]);

        let manifest = build_manifest(
            "generated-test",
            ModelSourceKind::Generated,
            vec!["lid_height".to_string()],
            &report,
            None,
            crate::models::SourceLanguage::LegacyPython,
        )
        .expect("manifest should build");

        let lid = manifest
            .parts
            .iter()
            .find(|part| part.freecad_object_name == "Lid")
            .expect("lid part should exist");
        let spout = manifest
            .parts
            .iter()
            .find(|part| part.freecad_object_name == "Spout")
            .expect("spout part should exist");

        assert!(lid.editable);
        assert_eq!(lid.parameter_keys, vec!["lid_height".to_string()]);
        assert!(!spout.editable);
        assert!(spout.parameter_keys.is_empty());
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.part_id == spout.part_id && !target.editable));
        assert!(!manifest
            .parameter_groups
            .iter()
            .any(|group| group.part_ids.contains(&spout.part_id)));
    }

    #[test]
    fn build_manifest_for_imported_fcstd_seeds_pending_proposals() {
        let report = sample_report(vec![RunnerObject {
            object_name: "OuterShell001".to_string(),
            label: String::new(),
            type_id: "Part::Feature".to_string(),
            export_path: "/tmp/shell.stl".to_string(),
            bounds: Some(RunnerBounds {
                x_min: 0.0,
                y_min: 0.0,
                z_min: 0.0,
                x_max: 20.0,
                y_max: 10.0,
                z_max: 12.0,
            }),
            volume: None,
            area: None,
        }]);

        let manifest = build_manifest(
            "imported-fcstd-test",
            ModelSourceKind::ImportedFcstd,
            Vec::new(),
            &report,
            Some("/tmp/model.FCStd".to_string()),
            crate::models::SourceLanguage::LegacyPython,
        )
        .expect("manifest should build");

        assert_eq!(manifest.enrichment_state.status, EnrichmentStatus::Pending);
        assert_eq!(manifest.enrichment_state.proposals.len(), 1);
        let proposal = &manifest.enrichment_state.proposals[0];
        assert_eq!(proposal.part_ids, vec![manifest.parts[0].part_id.clone()]);
        assert_eq!(
            proposal.parameter_keys,
            vec![
                "outer_shell_width".to_string(),
                "outer_shell_depth".to_string(),
                "outer_shell_height".to_string(),
            ]
        );
        assert_eq!(proposal.status, EnrichmentStatus::Pending);
        assert_eq!(proposal.provenance, "heuristic.import.bounds");
    }

    #[test]
    fn build_manifest_matches_generated_fixture_contract() {
        let manifest = build_manifest(
            "generated-fixture",
            ModelSourceKind::Generated,
            vec![
                "outer_shell_width".to_string(),
                "outer_shell_height".to_string(),
                "lid_height".to_string(),
            ],
            &fixture_generated_report(),
            None,
            crate::models::SourceLanguage::LegacyPython,
        )
        .expect("generated fixture manifest should build");

        validate_model_manifest(&manifest).expect("fixture manifest should validate");
        assert_eq!(manifest.document.document_name, "Fixture Box");
        assert_eq!(manifest.parts.len(), 2);

        let shell = manifest
            .parts
            .iter()
            .find(|part| part.freecad_object_name == "OuterShell")
            .expect("fixture shell part should exist");
        let lid = manifest
            .parts
            .iter()
            .find(|part| part.freecad_object_name == "Lid")
            .expect("fixture lid part should exist");

        assert!(shell.editable);
        assert!(shell
            .parameter_keys
            .contains(&"outer_shell_width".to_string()));
        assert!(shell
            .parameter_keys
            .contains(&"outer_shell_height".to_string()));
        assert!(lid.editable);
        assert_eq!(lid.parameter_keys, vec!["lid_height".to_string()]);
    }

    #[test]
    fn rebuild_imported_manifest_preserves_accepted_bindings() {
        let base_report = fixture_imported_report();
        let mut previous_manifest = build_manifest(
            "imported-fcstd-fixture",
            ModelSourceKind::ImportedFcstd,
            Vec::new(),
            &base_report,
            Some("/tmp/imported.FCStd".to_string()),
            crate::models::SourceLanguage::LegacyPython,
        )
        .expect("imported fixture manifest should build");

        let accepted_proposal = previous_manifest.enrichment_state.proposals[0].clone();
        previous_manifest.enrichment_state.proposals[0].status = EnrichmentStatus::Accepted;
        previous_manifest.parameter_groups.push(ParameterGroup {
            group_id: proposal_group_id(&accepted_proposal.proposal_id),
            label: accepted_proposal.label.clone(),
            parameter_keys: accepted_proposal.parameter_keys.clone(),
            part_ids: accepted_proposal.part_ids.clone(),
            editable: true,
            presentation: Some("primary".to_string()),
            order: None,
        });
        previous_manifest.parts[0].parameter_keys = accepted_proposal.parameter_keys.clone();
        previous_manifest.parts[0].editable = true;
        previous_manifest.selection_targets[0].editable = true;
        previous_manifest.enrichment_state.status = EnrichmentStatus::Accepted;

        let rebuilt = rebuild_imported_manifest(&previous_manifest, &fixture_imported_report())
            .expect("rebuilt manifest should succeed");

        validate_model_manifest(&rebuilt).expect("rebuilt manifest should validate");
        assert_eq!(rebuilt.enrichment_state.status, EnrichmentStatus::Accepted);
        assert_eq!(
            rebuilt.enrichment_state.proposals[0].status,
            EnrichmentStatus::Accepted
        );
        assert!(rebuilt.parts[0].editable);
        assert_eq!(
            rebuilt.parts[0].parameter_keys,
            accepted_proposal.parameter_keys
        );
        assert!(rebuilt.selection_targets[0].editable);
        assert!(rebuilt
            .parameter_groups
            .iter()
            .any(|group| group.group_id == proposal_group_id(&accepted_proposal.proposal_id)));
        assert!(rebuilt.warnings.iter().any(|warning| {
            warning == "Imported FCStd bindings were accepted from heuristic proposals."
        }));
    }

    #[test]
    fn runner_bindings_skip_inspect_only_parts() {
        let mut manifest = build_manifest(
            "generated-fixture",
            ModelSourceKind::Generated,
            vec!["outer_shell_width".to_string(), "lid_height".to_string()],
            &fixture_generated_report(),
            None,
            crate::models::SourceLanguage::LegacyPython,
        )
        .expect("generated fixture manifest should build");
        manifest.parts[1].editable = false;
        manifest.parts[1].parameter_keys.clear();

        let bindings = runner_bindings_from_manifest(&manifest);

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].object_name, "OuterShell");
        assert_eq!(
            bindings[0].parameter_keys,
            vec!["outer_shell_width".to_string()]
        );
    }

    #[test]
    fn load_cached_bundle_repairs_paths_and_viewer_assets_from_manifest() {
        let resolver = TestResolver::new("cache-repair");
        let bundle_dir = artifact_dir(&resolver, ModelSourceKind::Generated, "generated-cache")
            .expect("bundle dir");
        fs::create_dir_all(bundle_dir.join(PARTS_DIR_NAME)).expect("parts dir");

        let fcstd_path = bundle_dir.join(FCSTD_FILE_NAME);
        let preview_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        let asset_path = bundle_dir.join(PARTS_DIR_NAME).join("000-outershell.stl");
        fs::write(&fcstd_path, b"fcstd").expect("fcstd");
        fs::write(&preview_path, b"preview").expect("preview");
        fs::write(&step_path, b"step").expect("step");
        fs::write(&asset_path, b"part").expect("part");

        let manifest = sample_manifest("generated-cache", ModelSourceKind::Generated, &asset_path);
        write_manifest(&bundle_dir.join(MANIFEST_FILE_NAME), &manifest).expect("manifest");

        let bundle = sample_bundle("generated-cache", ModelSourceKind::Generated);
        write_bundle(&bundle_dir, &bundle).expect("bundle");

        let cached = load_cached_bundle(&bundle_dir)
            .expect("load cached bundle")
            .expect("cached bundle");

        assert_eq!(cached.fcstd_path, fcstd_path.to_string_lossy());
        assert_eq!(
            cached.manifest_path,
            bundle_dir.join(MANIFEST_FILE_NAME).to_string_lossy()
        );
        assert_eq!(cached.preview_stl_path, preview_path.to_string_lossy());
        assert_eq!(cached.export_artifacts[0].path, step_path.to_string_lossy());
        assert_eq!(cached.viewer_assets.len(), 1);
        assert_eq!(cached.viewer_assets[0].part_id, "part-shell");
        assert_eq!(cached.viewer_assets[0].path, asset_path.to_string_lossy());

        let saved: ArtifactBundle = serde_json::from_str(
            &fs::read_to_string(bundle_dir.join(BUNDLE_FILE_NAME)).expect("read bundle"),
        )
        .expect("parse bundle");
        assert_eq!(saved.fcstd_path, fcstd_path.to_string_lossy());
        assert_eq!(
            saved.manifest_path,
            bundle_dir.join(MANIFEST_FILE_NAME).to_string_lossy()
        );
        assert_eq!(saved.preview_stl_path, preview_path.to_string_lossy());
        assert_eq!(saved.viewer_assets[0].path, asset_path.to_string_lossy());
    }

    #[test]
    fn load_cached_bundle_normalizes_relative_bundle_and_asset_paths() {
        let resolver = TestResolver::new("cache-relative");
        let model_id = "generated-relative";
        let bundle_dir =
            artifact_dir(&resolver, ModelSourceKind::Generated, model_id).expect("bundle dir");
        fs::create_dir_all(bundle_dir.join(PARTS_DIR_NAME)).expect("parts dir");

        fs::write(bundle_dir.join(FCSTD_FILE_NAME), b"fcstd").expect("fcstd");
        fs::write(bundle_dir.join(PREVIEW_STL_FILE_NAME), b"preview").expect("preview");
        fs::write(bundle_dir.join(STEP_FILE_NAME), b"step").expect("step");
        fs::write(
            bundle_dir.join(PARTS_DIR_NAME).join("000-outershell.stl"),
            b"part",
        )
        .expect("part");

        let mut manifest = sample_manifest(
            model_id,
            ModelSourceKind::Generated,
            Path::new("parts/000-outershell.stl"),
        );
        manifest.parts[0].viewer_asset_path = Some("parts/000-outershell.stl".to_string());
        write_manifest(&bundle_dir.join(MANIFEST_FILE_NAME), &manifest).expect("manifest");

        let mut bundle = sample_bundle(model_id, ModelSourceKind::Generated);
        bundle.fcstd_path = FCSTD_FILE_NAME.to_string();
        bundle.manifest_path = MANIFEST_FILE_NAME.to_string();
        bundle.preview_stl_path = PREVIEW_STL_FILE_NAME.to_string();
        bundle.viewer_assets[0].path = "parts/stale.stl".to_string();
        write_bundle(&bundle_dir, &bundle).expect("bundle");

        let cached = load_cached_bundle(&bundle_dir)
            .expect("load cached bundle")
            .expect("cached bundle");

        assert_eq!(
            cached.fcstd_path,
            bundle_dir.join(FCSTD_FILE_NAME).to_string_lossy()
        );
        assert_eq!(
            cached.manifest_path,
            bundle_dir.join(MANIFEST_FILE_NAME).to_string_lossy()
        );
        assert_eq!(
            cached.preview_stl_path,
            bundle_dir.join(PREVIEW_STL_FILE_NAME).to_string_lossy()
        );
        assert_eq!(
            cached.export_artifacts[0].path,
            bundle_dir.join(STEP_FILE_NAME).to_string_lossy()
        );
        assert_eq!(
            cached.viewer_assets[0].path,
            bundle_dir
                .join(PARTS_DIR_NAME)
                .join("000-outershell.stl")
                .to_string_lossy()
        );
    }

    #[test]
    fn load_cached_bundle_rejects_missing_preview_even_with_manifest_assets() {
        let resolver = TestResolver::new("cache-no-preview");
        let bundle_dir = artifact_dir(&resolver, ModelSourceKind::Generated, "generated-missing")
            .expect("bundle dir");
        fs::create_dir_all(bundle_dir.join(PARTS_DIR_NAME)).expect("parts dir");

        let fcstd_path = bundle_dir.join(FCSTD_FILE_NAME);
        let asset_path = bundle_dir.join(PARTS_DIR_NAME).join("000-outershell.stl");
        fs::write(&fcstd_path, b"fcstd").expect("fcstd");
        fs::write(&asset_path, b"part").expect("part");

        let manifest =
            sample_manifest("generated-missing", ModelSourceKind::Generated, &asset_path);
        write_manifest(&bundle_dir.join(MANIFEST_FILE_NAME), &manifest).expect("manifest");

        let mut bundle = sample_bundle("generated-missing", ModelSourceKind::Generated);
        bundle.fcstd_path = fcstd_path.to_string_lossy().to_string();
        write_bundle(&bundle_dir, &bundle).expect("bundle");

        assert!(load_cached_bundle(&bundle_dir)
            .expect("load cached bundle")
            .is_none());
    }

    #[test]
    fn save_model_manifest_refreshes_bundle_viewer_assets() {
        let resolver = TestResolver::new("save-refresh");
        let model_id = "generated-save-refresh";
        let bundle_dir =
            artifact_dir(&resolver, ModelSourceKind::Generated, model_id).expect("bundle dir");
        fs::create_dir_all(bundle_dir.join(PARTS_DIR_NAME)).expect("parts dir");

        let fcstd_path = bundle_dir.join(FCSTD_FILE_NAME);
        let preview_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        let asset_path = bundle_dir.join(PARTS_DIR_NAME).join("000-outershell.stl");
        fs::write(&fcstd_path, b"fcstd").expect("fcstd");
        fs::write(&preview_path, b"preview").expect("preview");
        fs::write(&step_path, b"step").expect("step");
        fs::write(&asset_path, b"part").expect("part");

        let mut manifest = sample_manifest(model_id, ModelSourceKind::Generated, &asset_path);
        write_manifest(&bundle_dir.join(MANIFEST_FILE_NAME), &manifest).expect("manifest");
        write_bundle(
            &bundle_dir,
            &sample_bundle(model_id, ModelSourceKind::Generated),
        )
        .expect("bundle");

        let updated_asset_path = bundle_dir.join(PARTS_DIR_NAME).join("001-lid.stl");
        fs::write(&updated_asset_path, b"part").expect("updated part");
        manifest.parts[0].part_id = "part-lid".to_string();
        manifest.parts[0].freecad_object_name = "Lid".to_string();
        manifest.parts[0].label = "Lid".to_string();
        manifest.parts[0].viewer_node_ids = vec!["Lid".to_string()];
        manifest.parts[0].viewer_asset_path =
            Some(updated_asset_path.to_string_lossy().to_string());
        manifest.selection_targets[0].part_id = "part-lid".to_string();
        manifest.selection_targets[0].viewer_node_id = "Lid".to_string();
        manifest.selection_targets[0].label = "Lid".to_string();

        save_model_manifest(&resolver, model_id, &manifest).expect("save manifest");

        let raw = fs::read_to_string(bundle_dir.join(BUNDLE_FILE_NAME)).expect("read bundle");
        let saved: ArtifactBundle = serde_json::from_str(&raw).expect("parse bundle");
        assert_eq!(saved.viewer_assets.len(), 1);
        assert_eq!(saved.viewer_assets[0].part_id, "part-lid");
        assert_eq!(saved.viewer_assets[0].object_name, "Lid");
        assert_eq!(
            saved.viewer_assets[0].path,
            updated_asset_path.to_string_lossy()
        );
        assert_eq!(saved.preview_stl_path, preview_path.to_string_lossy());
    }

    #[test]
    fn save_model_manifest_normalizes_relative_viewer_asset_paths() {
        let resolver = TestResolver::new("save-relative");
        let model_id = "generated-save-relative";
        let bundle_dir =
            artifact_dir(&resolver, ModelSourceKind::Generated, model_id).expect("bundle dir");
        fs::create_dir_all(bundle_dir.join(PARTS_DIR_NAME)).expect("parts dir");

        fs::write(bundle_dir.join(FCSTD_FILE_NAME), b"fcstd").expect("fcstd");
        fs::write(bundle_dir.join(PREVIEW_STL_FILE_NAME), b"preview").expect("preview");
        fs::write(bundle_dir.join(PARTS_DIR_NAME).join("001-lid.stl"), b"part").expect("part");

        let mut manifest = sample_manifest(
            model_id,
            ModelSourceKind::Generated,
            Path::new("parts/001-lid.stl"),
        );
        manifest.parts[0].part_id = "part-lid".to_string();
        manifest.parts[0].freecad_object_name = "Lid".to_string();
        manifest.parts[0].label = "Lid".to_string();
        manifest.parts[0].viewer_node_ids = vec!["Lid".to_string()];
        manifest.parts[0].viewer_asset_path = Some("parts/001-lid.stl".to_string());
        manifest.selection_targets[0].part_id = "part-lid".to_string();
        manifest.selection_targets[0].viewer_node_id = "Lid".to_string();
        manifest.selection_targets[0].label = "Lid".to_string();
        write_manifest(&bundle_dir.join(MANIFEST_FILE_NAME), &manifest).expect("manifest");
        write_bundle(
            &bundle_dir,
            &sample_bundle(model_id, ModelSourceKind::Generated),
        )
        .expect("bundle");

        save_model_manifest(&resolver, model_id, &manifest).expect("save manifest");

        let saved: ArtifactBundle = serde_json::from_str(
            &fs::read_to_string(bundle_dir.join(BUNDLE_FILE_NAME)).expect("read bundle"),
        )
        .expect("parse bundle");
        assert_eq!(
            saved.viewer_assets[0].path,
            bundle_dir
                .join(PARTS_DIR_NAME)
                .join("001-lid.stl")
                .to_string_lossy()
        );
    }

    #[test]
    fn load_cached_bundle_repairs_bundle_metadata_from_manifest() {
        let resolver = TestResolver::new("cache-metadata");
        let model_id = "generated-metadata";
        let bundle_dir =
            artifact_dir(&resolver, ModelSourceKind::Generated, model_id).expect("bundle dir");
        fs::create_dir_all(bundle_dir.join(PARTS_DIR_NAME)).expect("parts dir");

        let fcstd_path = bundle_dir.join(FCSTD_FILE_NAME);
        let preview_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        let asset_path = bundle_dir.join(PARTS_DIR_NAME).join("000-outershell.stl");
        fs::write(&fcstd_path, b"fcstd").expect("fcstd");
        fs::write(&preview_path, b"preview").expect("preview");
        fs::write(&step_path, b"step").expect("step");
        fs::write(&asset_path, b"part").expect("part");

        let mut manifest = sample_manifest(model_id, ModelSourceKind::Generated, &asset_path);
        manifest.schema_version = MODEL_RUNTIME_SCHEMA_VERSION + 3;
        manifest.engine_kind = crate::models::EngineKind::Build123d;
        manifest.source_language = crate::models::SourceLanguage::EckyIrV0;
        write_manifest(&bundle_dir.join(MANIFEST_FILE_NAME), &manifest).expect("manifest");

        let bundle = sample_bundle(model_id, ModelSourceKind::Generated);
        write_bundle(&bundle_dir, &bundle).expect("bundle");

        let cached = load_cached_bundle(&bundle_dir)
            .expect("load cached bundle")
            .expect("cached bundle");

        assert_eq!(cached.schema_version, manifest.schema_version);
        assert_eq!(cached.engine_kind, manifest.engine_kind);
        assert_eq!(cached.source_language, manifest.source_language);
        assert_eq!(cached.geometry_backend, manifest.geometry_backend);
        assert_eq!(cached.export_artifacts[0].path, step_path.to_string_lossy());

        let saved: ArtifactBundle = serde_json::from_str(
            &fs::read_to_string(bundle_dir.join(BUNDLE_FILE_NAME)).expect("read bundle"),
        )
        .expect("parse bundle");
        assert_eq!(saved.schema_version, manifest.schema_version);
        assert_eq!(saved.engine_kind, manifest.engine_kind);
        assert_eq!(saved.source_language, manifest.source_language);
        assert_eq!(saved.geometry_backend, manifest.geometry_backend);
    }

    #[test]
    fn normalize_runner_report_paths_resolves_relative_exports() {
        let bundle_dir = std::env::temp_dir().join("ecky-runner-report-relative");
        let report = sample_report(vec![RunnerObject {
            object_name: "OuterShell".to_string(),
            label: "Outer Shell".to_string(),
            type_id: "Part::Feature".to_string(),
            export_path: "parts/000-outershell.stl".to_string(),
            bounds: None,
            volume: None,
            area: None,
        }]);

        let normalized =
            normalize_runner_report_paths(&bundle_dir, report).expect("normalize runner report");

        assert_eq!(
            normalized.objects[0].export_path,
            bundle_dir
                .join(PARTS_DIR_NAME)
                .join("000-outershell.stl")
                .to_string_lossy()
        );
    }

    #[test]
    fn normalize_freecad_cmd_resolves_bundle_directory_binaries() {
        let root = TestResolver::new("freecad-cmd-dir").root;
        let freecad_dir = root.join("FreeCAD");
        fs::create_dir_all(&freecad_dir).expect("freecad dir");
        let binary = freecad_dir.join("freecadcmd");
        fs::write(&binary, b"#!/bin/sh\n").expect("binary");

        assert_eq!(normalize_freecad_cmd(&freecad_dir), binary);
    }

    #[test]
    fn render_model_with_sources_renders_ecky_canonical_cup_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("cup");
        let source = include_str!("../tests/fixtures/cad/surface/canonical_cup.ecky");

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle.fcstd_path.ends_with("model.FCStd"));
        assert!(bundle.preview_stl_path.ends_with("preview.stl"));
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.fcstd_path).exists());
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_thomas_body_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("thomas-body");
        let source = include_str!("../tests/fixtures/cad/surface/thomas_modular_ramp_body.ecky");

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_loft_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("loft");
        let source = r#"(model
            (part body
              (loft 30
                (polygon ((0 0) (20 0) (20 20) (0 20)))
                (polygon ((3 3) (17 3) (17 17) (3 17))))))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_circle_extrude_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("circle-extrude");
        let source = r#"(model
            (part body
              (extrude (circle 10) 8)))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_rounded_rect_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("rounded-rect");
        let source = r#"(model
            (part body
              (extrude (rounded-rect 20 10 2) 8)))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_rounded_polygon_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("rounded-polygon");
        let source = r#"(model
            (part body
              (extrude (rounded-polygon ((0 20) (20 0) (0 -20) (-20 0)) 4 8) 8)))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_profile_offset_and_bezier_path_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("profile-offset-bezier");
        let source = r#"(model
            (part body
              (union
                (extrude
                  (profile
                    :outer (polygon ((0 0) (24 0) (24 24) (0 24)))
                    :holes ((polygon ((8 8) (16 8) (16 16) (8 16)))))
                  4)
                (translate 40 0 0
                  (extrude
                    (offset 2 :openings ((polygon ((8 8) (16 8) (16 16) (8 16))))
                      (polygon ((0 0) (24 0) (24 24) (0 24))))
                    4))
                (translate 80 0 0
                  (sweep
                    (polygon ((0 0) (3 0) (3 2) (0 2)))
                    (bezier-path ((0 0 0) (10 0 0) (10 10 0) (20 10 0))))))))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_taper_twist_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("taper-twist");
        let source = r#"(model
            (part body
              (union
                (translate 20 0 0
                  (taper 20 0.6 (polygon ((0 0) (10 0) (10 10) (0 10)))))
                (translate 60 0 0
                  (twist 20 90 8 (polygon ((0 0) (10 0) (10 10) (0 10))))))))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_named_arrays_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("named-arrays");
        let source = r#"(model
            (part body
              (union
                (linear-array 3 14 0 0 (box 4 4 4))
                (translate 0 20 0 (grid-array 2 2 10 10 (box 2 2 2)))
                (translate 50 0 0 (radial-array 4 90 12 (cylinder 2 4)))
                (translate 90 0 0 (arc-array 3 16 0 180 (cylinder 2 4))))))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_place_frames_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("place-frames");
        let source = r#"(model
            (part body
              (union
                (build
                  (shape base (plane :origin (10 20 30) :x (0 1 0) :normal (0 0 1)))
                  (shape peg (box 4 6 2 :align '(min min min)))
                  (shape pose (location base :offset (5 0 0) :rotate (0 90 0)))
                  (result (place pose peg)))
                (build
                  (shape rail (path (0 0 0) (20 0 10) (20 10 10)))
                  (shape peg (box 4 2 6 :align '(min min min)))
                  (shape frame (path-frame rail :at 0.5))
                  (result (place frame peg :offset (1 2 3) :rotate (10 20 30)))))))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_rejects_parallel_path_frame_up_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("path-frame-up-error");
        let source = r#"(model
            (part body
              (build
                (shape rail (path (0 0 0) (20 0 10) (20 10 10)))
                (shape peg (box 4 2 6 :align '(min min min)))
                (shape frame (path-frame rail :at end :up (0 1 0)))
                (result (place frame peg :offset (1 2 3) :rotate (10 20 30))))))"#;

        let err = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect_err("parallel up should fail");

        let message = err.to_string();
        assert!(
            message.contains("`path-frame :up`") && message.contains("perpendicular"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn render_model_with_sources_rejects_parallel_plane_axes_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("plane-axis-error");
        let source = r#"(model
            (part body
              (build
                (shape base (plane :origin (0 0 0) :x (0 0 1) :normal (0 0 1)))
                (shape peg (box 4 2 6 :align '(min min min)))
                (result (place base peg)))))"#;

        let err = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect_err("parallel plane axes should fail");

        let message = err.to_string();
        assert!(
            message.contains("`plane :x`") && message.contains("perpendicular"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn render_model_with_sources_renders_ecky_xor_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("xor");
        let source = r#"(model
            (part body
              (union
                (xor
                  (box 10 10 10)
                  (translate 5 0 0 (box 10 10 10)))
                (translate 30 0 0
                  (extrude
                    (xor
                      (polygon ((0 0) (20 0) (20 20) (0 20)))
                      (polygon ((8 8) (16 8) (16 16) (8 16))))
                    6)))))"#;

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    #[test]
    fn render_model_with_sources_renders_ecky_thomas_ramp_via_freecad() {
        let freecad_cmd = match resolve_freecad_path(None) {
            Ok(path) => path,
            Err(_) => return,
        };
        let resolver = TestResolver::new("thomas-ramp");
        let source = include_str!("../tests/fixtures/cad/surface/thomas_modular_ramp.ecky");

        let bundle = render_model_with_sources(
            &crate::ecky_ir::lower_to_freecad(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            Some(freecad_cmd.to_string_lossy().as_ref()),
            &resolver,
            crate::models::SourceLanguage::EckyIrV0,
        )
        .expect("render");

        assert_eq!(
            bundle.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );
        assert!(bundle
            .macro_path
            .as_deref()
            .is_some_and(|path| path.ends_with("source.ecky")));
        assert!(Path::new(&bundle.preview_stl_path).exists());
        let manifest: ModelManifest = serde_json::from_str(
            &fs::read_to_string(&bundle.manifest_path).expect("read manifest"),
        )
        .expect("parse manifest");
        assert!(!manifest.parts.is_empty());
    }

    proptest! {
        #[test]
        fn unique_strings_is_idempotent(values in prop::collection::vec("[a-z_]{1,12}", 0..24)) {
            let once = unique_strings(values);
            let twice = unique_strings(once.clone());
            prop_assert_eq!(twice, once);
        }
    }
}
