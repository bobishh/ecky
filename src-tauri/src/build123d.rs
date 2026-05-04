use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::freecad::resolve_resource_path;
use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignParams, DocumentMetadata, EnrichmentStatus,
    ExportArtifact, GeometryBackend, ManifestBounds, ManifestEnrichmentState, ModelManifest,
    ModelSourceKind, PartBinding, PathResolver, SelectionTarget, SelectionTargetKind,
    SourceLanguage, ViewerAsset, ViewerAssetFormat, MODEL_RUNTIME_SCHEMA_VERSION,
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
    let manifest = build_manifest(
        &model_id,
        &report,
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
        edge_targets: Vec::new(),
        face_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: step_export_artifacts(&step_path)?,
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

fn build_manifest(
    model_id: &str,
    report: &RunnerReport,
    source_language: SourceLanguage,
    source_path: Option<String>,
) -> ModelManifest {
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
            }],
        };

        let manifest = build_manifest(
            "model",
            &report,
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
            }],
        };

        let manifest = build_manifest(
            "model",
            &report,
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
}
