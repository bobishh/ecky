use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::freecad::resolve_resource_path;
use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignParams, DocumentMetadata, EngineKind,
    EnrichmentStatus, GeometryBackend, ManifestBounds, ManifestEnrichmentState, ModelManifest,
    ModelSourceKind, PartBinding, PathResolver, SelectionTarget, SelectionTargetKind,
    SourceLanguage, ViewerAsset, ViewerAssetFormat, MODEL_RUNTIME_SCHEMA_VERSION,
};

const RUNNER_RESOURCE_PATH: &str = "server/build123d_runner.py";
const MODEL_RUNTIME_ROOT: &str = "model-runtime";
const GENERATED_ARTIFACT_DIR: &str = "generated";
const BUNDLE_FILE_NAME: &str = "bundle.json";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const SOURCE_FILE_NAME: &str = "source.py";
const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
const PARTS_DIR_NAME: &str = "parts";
const RUNNER_REPORT_FILE_NAME: &str = "runner-report.json";

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
    let params_json =
        serde_json::to_string(parameters).map_err(|e| AppError::validation(e.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
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

    let source_path = bundle_dir.join(SOURCE_FILE_NAME);
    fs::write(&source_path, source).map_err(|e| AppError::persistence(e.to_string()))?;

    let preview_stl_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
    let runner_report_path = bundle_dir.join(RUNNER_REPORT_FILE_NAME);

    run_runner(
        app,
        &source_path,
        &preview_stl_path,
        &parts_dir,
        &runner_report_path,
        &params_json,
    )?;

    let report = read_runner_report(&runner_report_path)?;
    let manifest = build_manifest(&model_id, &report);
    let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
    write_manifest(&manifest_path, &manifest)?;

    let viewer_assets = build_viewer_assets(&manifest.parts);
    let bundle = ArtifactBundle {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id,
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::Build123d,
        content_hash,
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: path_to_string(&manifest_path)?,
        macro_path: Some(path_to_string(&source_path)?),
        preview_stl_path: path_to_string(&preview_stl_path)?,
        viewer_assets,
        edge_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: Vec::new(),
    };
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
    let bundle: ArtifactBundle = serde_json::from_str(&raw)
        .map_err(|e| AppError::parse(format!("Failed to parse bundle: {}", e)))?;
    if !Path::new(&bundle.manifest_path).exists()
        || !Path::new(&bundle.preview_stl_path).exists()
        || bundle
            .viewer_assets
            .iter()
            .any(|a| !Path::new(&a.path).exists())
    {
        return Ok(None);
    }
    Ok(Some(bundle))
}

fn run_runner(
    app: &dyn PathResolver,
    source_path: &Path,
    stl_path: &Path,
    parts_dir: &Path,
    report_path: &Path,
    params_json: &str,
) -> AppResult<()> {
    let python_cmd = resolve_python_cmd()?;
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

pub fn resolve_python_cmd() -> AppResult<PathBuf> {
    for var in &["BUILD123D_PYTHON", "PYTHON_CMD"] {
        if let Ok(cmd) = std::env::var(var) {
            let cmd = cmd.trim().to_string();
            if !cmd.is_empty() {
                return Ok(PathBuf::from(cmd));
            }
        }
    }
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

fn build_manifest(model_id: &str, report: &RunnerReport) -> ModelManifest {
    let mut parts = Vec::new();
    let mut selection_targets = Vec::new();

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
            part_id: part_id.clone(),
            viewer_node_id: part_id.clone(),
            label: label.clone(),
            kind: SelectionTargetKind::Object,
            editable: false,
            parameter_keys: Vec::new(),
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        });
        let _ = index; // suppress unused warning
    }

    ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
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
            source_path: None,
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
}
