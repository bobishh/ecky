use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use crate::contracts::{AppError, AppResult, SourceLanguage};
use crate::models::{AppState, PathResolver};
use crate::services::manual_code::{ManualCodeApplyRequest, ManualCodeApplyResponse};

const MAX_SECTION_TRIANGLES: usize = 2_000_000;
const MIN_LOOP_POINTS: usize = 3;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProfilePlane {
    Xy,
    Xz,
    Yz,
}

impl ProfilePlane {
    fn axis(self) -> usize {
        match self {
            Self::Yz => 0,
            Self::Xz => 1,
            Self::Xy => 2,
        }
    }

    fn project(self, point: [f64; 3]) -> [f64; 2] {
        match self {
            Self::Xy => [point[0], point[1]],
            Self::Xz => [point[0], point[2]],
            Self::Yz => [point[1], point[2]],
        }
    }

    fn translation(self, offset: f64) -> ([f64; 3], [f64; 3]) {
        match self {
            Self::Xy => ([0.0, 0.0, offset], [0.0, 0.0, 0.0]),
            Self::Xz => ([0.0, offset, 0.0], [90.0, 0.0, 0.0]),
            Self::Yz => ([offset, 0.0, 0.0], [90.0, 0.0, 90.0]),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProfileLoopRole {
    Outer,
    Hole,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileLoop {
    pub points: Vec<[f64; 2]>,
    pub role: ProfileLoopRole,
    pub parent_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProfileEvidenceAccuracy {
    ExactBrep,
    ApproximateMesh,
    ApproximatePolygon,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProfileDerivationOperation {
    #[specta(rename_all = "camelCase")]
    Section { plane: ProfilePlane, offset_mm: f64 },
    #[specta(rename_all = "camelCase")]
    Projection { view: ProfileView },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProfileView {
    Front,
    Top,
    Side,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileGeometryRequest {
    pub artifact_path: String,
    pub operation: ProfileDerivationOperation,
    pub tolerance_mm: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAsset {
    pub artifact_path: String,
    pub artifact_digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectProfileDrawingRequest {
    pub artifact_path: String,
    pub view: ProfileView,
    pub tolerance_mm: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDrawingProjectionResponse {
    pub source_artifact_path: String,
    pub source_artifact_digest: String,
    pub view: ProfileView,
    pub tolerance_mm: f64,
    pub units: String,
    pub projection_asset: ProfileAsset,
    pub display_edges: Vec<Vec<[f64; 2]>>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileGeometryResponse {
    pub source_artifact_path: String,
    pub source_artifact_digest: String,
    pub operation: ProfileDerivationOperation,
    pub tolerance_mm: f64,
    pub units: String,
    pub accuracy: ProfileEvidenceAccuracy,
    pub profile_asset: Option<ProfileAsset>,
    pub display_accuracy: String,
    pub loops: Vec<ProfileLoop>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileSectionRequest {
    pub artifact_path: String,
    pub plane: ProfilePlane,
    pub offset_mm: f64,
    pub tolerance_mm: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSectionResponse {
    pub artifact_path: String,
    pub artifact_digest: String,
    pub plane: ProfilePlane,
    pub offset_mm: f64,
    pub tolerance_mm: f64,
    pub accuracy: ProfileEvidenceAccuracy,
    pub loops: Vec<ProfileLoop>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileArtifactReference {
    pub source_artifact_path: String,
    pub source_artifact_digest: String,
    pub operation: ProfileDerivationOperation,
    pub tolerance_mm: f64,
    pub accuracy: ProfileEvidenceAccuracy,
    #[serde(default)]
    pub profile_asset: Option<ProfileAsset>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileFeatureApplyRequest {
    pub thread_id: String,
    #[serde(default)]
    pub base_message_id: Option<String>,
    pub expected_source: String,
    #[serde(default)]
    pub loops: Option<Vec<ProfileLoop>>,
    pub plane: ProfilePlane,
    pub offset_mm: f64,
    pub clearance_mm: f64,
    pub depth_mm: f64,
    pub part_name: String,
    #[serde(default)]
    pub reference: Option<ProfileArtifactReference>,
    #[serde(default)]
    pub drawing_profile: Option<crate::services::drawing_import::DrawingProfileApplyInput>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFeatureApplyResponse {
    pub source: String,
    pub draft: ManualCodeApplyResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PointKey([i64; 2]);

pub fn extract_profile_section(
    request: ProfileSectionRequest,
) -> AppResult<ProfileSectionResponse> {
    validate_section_request(&request)?;
    let path = Path::new(&request.artifact_path);
    if !path.is_file() {
        return Err(AppError::not_found(format!(
            "Profile source STL does not exist: {}",
            path.display()
        )));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("stl"))
    {
        return Err(AppError::validation(
            "Profile section currently supports STL mesh artifacts only; exact BRep sectioning is unavailable.",
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        AppError::not_found(format!("Profile source STL could not be read: {error}"))
    })?;
    let source_artifact_digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let mesh = crate::ecky_ir::mesh_asset::IndexedMeshAsset::from_stl(
        crate::ecky_ir::mesh_asset::MeshAssetSource::Imported,
        path,
    )?;
    if artifact_digest(path.to_string_lossy().as_ref())? != source_artifact_digest {
        return Err(AppError::conflict(
            "Profile source STL changed while its section was being derived; retry sectioning.",
        ));
    }
    if mesh.triangles().len() > MAX_SECTION_TRIANGLES {
        return Err(AppError::validation(format!(
            "Profile source has {} triangles; section limit is {MAX_SECTION_TRIANGLES}.",
            mesh.triangles().len()
        )));
    }
    let segments = intersect_mesh_plane(
        mesh.vertices(),
        mesh.triangles(),
        request.plane,
        request.offset_mm,
        request.tolerance_mm,
    )?;
    let loops = stitch_section_loops(&segments, request.tolerance_mm)?;
    Ok(ProfileSectionResponse {
        artifact_path: request.artifact_path,
        artifact_digest: source_artifact_digest,
        plane: request.plane,
        offset_mm: request.offset_mm,
        tolerance_mm: request.tolerance_mm,
        accuracy: ProfileEvidenceAccuracy::ApproximateMesh,
        loops,
        warnings: vec![
            "Section is derived from a faceted STL mesh and is approximate, even when the source model was authored as exact BRep.".into(),
            "STL coordinates are interpreted as millimeters; STL carries no unit metadata.".into(),
        ],
    })
}

pub fn derive_profile_geometry(
    request: ProfileGeometryRequest,
    app: &dyn PathResolver,
) -> AppResult<ProfileGeometryResponse> {
    validate_geometry_request(&request)?;
    let source_path = Path::new(&request.artifact_path);
    if !source_path.is_file() {
        return Err(AppError::not_found(format!(
            "Profile source artifact does not exist: {}",
            source_path.display()
        )));
    }
    let source_digest = artifact_digest(&request.artifact_path)?;
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let is_mesh = extension.eq_ignore_ascii_case("stl");
    if is_mesh {
        let ProfileDerivationOperation::Section { plane, offset_mm } = &request.operation else {
            return Err(AppError::validation("STL profile projection is unsupported; choose an exact STEP/STP source for projections."));
        };
        let response = extract_profile_section(ProfileSectionRequest {
            artifact_path: request.artifact_path.clone(),
            plane: *plane,
            offset_mm: *offset_mm,
            tolerance_mm: request.tolerance_mm,
        })?;
        return Ok(ProfileGeometryResponse {
            source_artifact_path: request.artifact_path,
            source_artifact_digest: source_digest,
            operation: request.operation,
            tolerance_mm: request.tolerance_mm,
            units: "mm".into(),
            accuracy: ProfileEvidenceAccuracy::ApproximateMesh,
            profile_asset: None,
            display_accuracy: "approximateDisplayOnly".into(),
            loops: response.loops,
            warnings: response.warnings,
        });
    }
    if !extension.eq_ignore_ascii_case("step") && !extension.eq_ignore_ascii_case("stp") {
        return Err(AppError::validation(
            "Exact profile geometry requires a STEP/STP BRep artifact. Export FCStd as STEP first; no mesh or FreeCAD fallback is used.",
        ));
    }
    derive_exact_profile_geometry(&request, app, source_digest)
}

pub fn project_profile_drawing(
    request: ProjectProfileDrawingRequest,
    app: &dyn PathResolver,
) -> AppResult<ProfileDrawingProjectionResponse> {
    validate_drawing_projection_request(&request)?;
    let source_path = Path::new(&request.artifact_path);
    if !source_path.is_file() {
        return Err(AppError::not_found(format!(
            "Projection source artifact does not exist: {}",
            source_path.display()
        )));
    }
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("step") && !extension.eq_ignore_ascii_case("stp") {
        return Err(AppError::validation(
            "Exact CAD drawing projections need a STEP/STP BRep file. Export FCStd as STEP first.",
        ));
    }
    let source_digest = artifact_digest(&request.artifact_path)?;
    let runner =
        crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(app, true)
            .ok_or_else(|| {
                AppError::validation(
            "Bundled native Direct OCCT runner is unavailable for exact drawing projection.",
        )
            })?;
    let asset_root = app.app_data_dir().join("projection-assets");
    std::fs::create_dir_all(&asset_root)
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let work = asset_root.join(format!("project-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work).map_err(|error| AppError::persistence(error.to_string()))?;
    let _workspace = ProfileDerivationWorkspace(work.clone());
    let request_path = work.join("request.json");
    let request_json = serde_json::json!({
        "sourceArtifactPath": request.artifact_path,
        "view": request.view,
        "toleranceMm": request.tolerance_mm,
    });
    let serialized_request = serde_json::to_vec(&request_json).map_err(|error| {
        AppError::validation(format!(
            "Could not serialize exact drawing projection request: {error}"
        ))
    })?;
    std::fs::write(&request_path, serialized_request).map_err(|error| {
        AppError::persistence(format!(
            "Could not write drawing projection request: {error}"
        ))
    })?;
    let stdout_path = work.join("runner.stdout.log");
    let stderr_path = work.join("runner.stderr.log");
    let output = run_profile_native_command(
        &runner,
        "--project-drawing",
        "drawing projection",
        &request_path,
        &work,
        &stdout_path,
        &stderr_path,
        180,
    )?;
    if !output.success {
        return Err(AppError::with_details(
            crate::contracts::AppErrorCode::Render,
            "Native Direct OCCT drawing projection failed.",
            format!(
                "{}\n\nstdout:\n{}\n\nstderr:\n{}",
                output.status, output.stdout, output.stderr
            ),
        ));
    }
    let report_path = work.join("display-edges.json");
    let report: ExactDrawingReport =
        serde_json::from_slice(&std::fs::read(&report_path).map_err(|error| {
            AppError::persistence(format!("Native drawing preview edges are missing: {error}"))
        })?)
        .map_err(|error| {
            AppError::parse(format!("Native drawing preview edges are invalid: {error}"))
        })?;
    let display_edges = validate_display_edges(report.display_edges)?;
    if artifact_digest(&request.artifact_path)? != source_digest {
        return Err(AppError::conflict(
            "Projection source STEP changed while the exact drawing was being calculated; retry.",
        ));
    }
    let staged_asset = work.join("projection.step");
    let bytes = std::fs::read(&staged_asset).map_err(|error| {
        AppError::persistence(format!(
            "Native exact drawing STEP asset is missing: {error}"
        ))
    })?;
    let asset_digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let asset_path = asset_root.join(format!(
        "{}.step",
        asset_digest.trim_start_matches("sha256:")
    ));
    if asset_path.exists() {
        if artifact_digest(asset_path.to_string_lossy().as_ref())? != asset_digest {
            return Err(AppError::conflict(
                "Content-addressed drawing asset already exists with unexpected bytes.",
            ));
        }
    } else {
        std::fs::rename(&staged_asset, &asset_path).map_err(|error| {
            AppError::persistence(format!(
                "Could not persist exact drawing STEP asset: {error}"
            ))
        })?;
    }
    let mut warnings = report.warnings;
    warnings.push(
        "Drawing curves remain exact in the STEP asset. Display edges are sampled independently; they are not joined into closed profiles.".into(),
    );
    Ok(ProfileDrawingProjectionResponse {
        source_artifact_path: request.artifact_path,
        source_artifact_digest: source_digest,
        view: request.view,
        tolerance_mm: request.tolerance_mm,
        units: "mm".into(),
        projection_asset: ProfileAsset {
            artifact_path: asset_path.to_string_lossy().into_owned(),
            artifact_digest: asset_digest,
        },
        display_edges,
        warnings,
    })
}

struct ProfileNativeRunnerOutput {
    success: bool,
    status: String,
    stdout: String,
    stderr: String,
}

fn run_profile_native_command(
    runner: &Path,
    mode: &str,
    operation: &str,
    request_path: &Path,
    work: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout_seconds: u64,
) -> AppResult<ProfileNativeRunnerOutput> {
    let stdout = std::fs::File::create(stdout_path).map_err(|error| {
        AppError::persistence(format!("Could not capture projection output: {error}"))
    })?;
    let stderr = std::fs::File::create(stderr_path).map_err(|error| {
        AppError::persistence(format!("Could not capture projection errors: {error}"))
    })?;
    let mut child = Command::new(runner)
        .arg(mode)
        .arg(request_path)
        .arg("--out")
        .arg(work)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| {
            AppError::render(format!(
                "Could not start native drawing projection: {error}"
            ))
        })?;
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::render(format!(
                    "Could not monitor native {operation}; runner was stopped: {error}"
                )));
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::render(format!(
                "Native {operation} exceeded its {timeout_seconds}-second limit. Try a lighter STEP model or a different view."
            )));
        }
        thread::sleep(Duration::from_millis(100));
    };
    let read_log = |path: &Path| -> AppResult<String> {
        std::fs::read_to_string(path).map_err(|error| {
            AppError::persistence(format!("Could not read native projection log: {error}"))
        })
    };
    Ok(ProfileNativeRunnerOutput {
        success: status.success(),
        status: status.to_string(),
        stdout: read_log(stdout_path)?,
        stderr: read_log(stderr_path)?,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExactDrawingReport {
    display_edges: Vec<Vec<[f64; 2]>>,
    #[serde(default)]
    warnings: Vec<String>,
}

fn validate_drawing_projection_request(request: &ProjectProfileDrawingRequest) -> AppResult<()> {
    if request.artifact_path.trim().is_empty() {
        return Err(AppError::validation(
            "Projection source artifact path is required.",
        ));
    }
    if !request.tolerance_mm.is_finite() || !(1.0e-6..=1.0).contains(&request.tolerance_mm) {
        return Err(AppError::validation(
            "Drawing tolerance must be finite and between 0.000001 and 1 mm.",
        ));
    }
    Ok(())
}

fn validate_display_edges(edges: Vec<Vec<[f64; 2]>>) -> AppResult<Vec<Vec<[f64; 2]>>> {
    if edges.is_empty() {
        return Err(AppError::validation(
            "Exact drawing projection returned no visible curves.",
        ));
    }
    for edge in &edges {
        if edge.len() < 2
            || edge
                .iter()
                .flatten()
                .any(|coordinate| !coordinate.is_finite())
        {
            return Err(AppError::validation(
                "Exact drawing projection contains an invalid display edge.",
            ));
        }
    }
    Ok(edges)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExactProfileReport {
    loops: Vec<ProfileLoop>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ProfileAssetEvidence {
    profile_asset_digest: String,
    source_artifact_path: String,
    source_artifact_digest: String,
    operation: ProfileDerivationOperation,
    tolerance_mm: f64,
}

struct ProfileDerivationWorkspace(PathBuf);

impl Drop for ProfileDerivationWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn validate_geometry_request(request: &ProfileGeometryRequest) -> AppResult<()> {
    if request.artifact_path.trim().is_empty() {
        return Err(AppError::validation(
            "Profile source artifact path is required.",
        ));
    }
    if !request.tolerance_mm.is_finite() || !(1.0e-6..=1.0).contains(&request.tolerance_mm) {
        return Err(AppError::validation(
            "Profile tolerance must be finite and between 0.000001 and 1 mm.",
        ));
    }
    if let ProfileDerivationOperation::Section { offset_mm, .. } = &request.operation {
        if !offset_mm.is_finite() {
            return Err(AppError::validation(
                "Section offset must be finite millimeters.",
            ));
        }
    }
    Ok(())
}

fn derive_exact_profile_geometry(
    request: &ProfileGeometryRequest,
    app: &dyn PathResolver,
    source_digest: String,
) -> AppResult<ProfileGeometryResponse> {
    let runner =
        crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(app, true)
            .ok_or_else(|| {
                AppError::validation(
            "Bundled native Direct OCCT runner is unavailable for exact profile derivation.",
        )
            })?;
    let asset_root = app.app_data_dir().join("profile-assets");
    std::fs::create_dir_all(&asset_root)
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let work = asset_root.join(format!("derive-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work).map_err(|error| AppError::persistence(error.to_string()))?;
    let _workspace = ProfileDerivationWorkspace(work.clone());
    let staged_step = work.join("profile.step");
    let report_path = work.join("display-loops.json");
    let request_path = work.join("request.json");
    let derive_request = serde_json::json!({
        "sourceArtifactPath": request.artifact_path,
        "operation": request.operation,
        "toleranceMm": request.tolerance_mm,
    });
    let serialized_request = serde_json::to_vec(&derive_request).map_err(|error| {
        AppError::validation(format!(
            "Could not serialize exact profile request: {error}"
        ))
    })?;
    std::fs::write(&request_path, serialized_request).map_err(|error| {
        AppError::persistence(format!("Could not write exact profile request: {error}"))
    })?;
    let stdout_path = work.join("runner.stdout.log");
    let stderr_path = work.join("runner.stderr.log");
    let output = run_profile_native_command(
        &runner,
        "--derive-profile",
        "exact profile derivation",
        &request_path,
        &work,
        &stdout_path,
        &stderr_path,
        120,
    )?;
    if !output.success {
        let _ = std::fs::remove_dir_all(&work);
        return Err(AppError::with_details(
            crate::contracts::AppErrorCode::Render,
            "Native Direct OCCT exact profile derivation failed.",
            format!(
                "{}\n\nstdout:\n{}\n\nstderr:\n{}",
                output.status, output.stdout, output.stderr
            ),
        ));
    }
    let report: ExactProfileReport =
        serde_json::from_slice(&std::fs::read(&report_path).map_err(|error| {
            AppError::persistence(format!("Native profile display loops are missing: {error}"))
        })?)
        .map_err(|error| {
            AppError::parse(format!("Native profile display loops are invalid: {error}"))
        })?;
    let loops = validate_profile_loops(&report.loops)?;
    let mut warnings = report.warnings;
    warnings.push("Exact BRep curves and holes are stored in the profile asset. Display loop points are sampled for preview only.".into());
    if artifact_digest(&request.artifact_path)? != source_digest {
        let _ = std::fs::remove_dir_all(&work);
        return Err(AppError::conflict(
            "Profile source artifact changed while exact geometry was being derived; retry.",
        ));
    }
    let bytes = std::fs::read(&staged_step).map_err(|error| {
        AppError::persistence(format!(
            "Native exact profile STEP asset is missing: {error}"
        ))
    })?;
    let asset_digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let asset_path = asset_root.join(format!(
        "{}.step",
        asset_digest.trim_start_matches("sha256:")
    ));
    if asset_path.exists() {
        if artifact_digest(asset_path.to_string_lossy().as_ref())? != asset_digest {
            let _ = std::fs::remove_dir_all(&work);
            return Err(AppError::conflict(
                "Content-addressed profile asset already exists with unexpected bytes.",
            ));
        }
    } else {
        std::fs::rename(&staged_step, &asset_path).map_err(|error| {
            AppError::persistence(format!("Could not persist exact profile asset: {error}"))
        })?;
    }
    let evidence = ProfileAssetEvidence {
        profile_asset_digest: asset_digest.clone(),
        source_artifact_path: request.artifact_path.clone(),
        source_artifact_digest: source_digest.clone(),
        operation: request.operation.clone(),
        tolerance_mm: request.tolerance_mm,
    };
    let evidence_path = profile_asset_evidence_path(&asset_root, &evidence)?;
    if evidence_path.exists() {
        let existing: ProfileAssetEvidence = serde_json::from_slice(
            &std::fs::read(&evidence_path)
                .map_err(|error| AppError::persistence(error.to_string()))?,
        )
        .map_err(|error| {
            AppError::parse(format!("Exact profile provenance is invalid: {error}"))
        })?;
        if existing != evidence {
            let _ = std::fs::remove_dir_all(&work);
            return Err(AppError::conflict(
                "Exact profile provenance key contains mismatched evidence.",
            ));
        }
    } else {
        let bytes = serde_json::to_vec(&evidence)
            .map_err(|error| AppError::persistence(error.to_string()))?;
        let staged_evidence = work.join("profile-evidence.json");
        std::fs::write(&staged_evidence, bytes).map_err(|error| {
            AppError::persistence(format!("Could not persist profile provenance: {error}"))
        })?;
        if !evidence_path.exists() {
            std::fs::rename(&staged_evidence, &evidence_path).map_err(|error| {
                AppError::persistence(format!("Could not commit profile provenance: {error}"))
            })?;
        }
    }
    let _ = std::fs::remove_dir_all(work);
    Ok(ProfileGeometryResponse {
        source_artifact_path: request.artifact_path.clone(),
        source_artifact_digest: source_digest,
        operation: request.operation.clone(),
        tolerance_mm: request.tolerance_mm,
        units: "mm".into(),
        accuracy: ProfileEvidenceAccuracy::ExactBrep,
        profile_asset: Some(ProfileAsset {
            artifact_path: asset_path.to_string_lossy().into_owned(),
            artifact_digest: asset_digest,
        }),
        display_accuracy: "approximateDisplayOnly".into(),
        loops,
        warnings,
    })
}

pub async fn apply_profile_feature(
    request: ProfileFeatureApplyRequest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ProfileFeatureApplyResponse> {
    validate_feature_request(&request)?;
    let drawing_evidence = if let Some(drawing) = request.drawing_profile.as_ref() {
        let evidence = crate::services::drawing_import::load_verified_evidence(
            &app.app_data_dir(),
            &drawing.evidence_id,
        )?;
        let expected_plane = match &evidence.view {
            crate::contracts::SketchView::Front => ProfilePlane::Xz,
            crate::contracts::SketchView::Top => ProfilePlane::Xy,
            crate::contracts::SketchView::Side => ProfilePlane::Yz,
            crate::contracts::SketchView::Custom => {
                return Err(AppError::validation(
                    "Drawing profile apply supports front, top, or side views. Choose a principal view and recognize again.",
                ));
            }
        };
        if request.plane != expected_plane {
            return Err(AppError::validation(
                "Drawing view and extrusion plane do not match. Choose the plane assigned to this view.",
            ));
        }
        Some(evidence)
    } else {
        None
    };
    if let Some(reference) = request.reference.as_ref() {
        let actual = artifact_digest(&reference.source_artifact_path)?;
        if actual != reference.source_artifact_digest {
            return Err(AppError::conflict(format!(
                "Profile source artifact changed: expected {}, found {}.",
                reference.source_artifact_digest, actual
            )));
        }
        if let Some(asset) = reference.profile_asset.as_ref() {
            let expected_asset = app.app_data_dir().join("profile-assets").join(format!(
                "{}.step",
                asset.artifact_digest.trim_start_matches("sha256:")
            ));
            if !same_file_path(
                expected_asset.to_string_lossy().as_ref(),
                &asset.artifact_path,
            ) {
                return Err(AppError::validation(
                    "Exact profile asset must use its persistent content-addressed app-data path.",
                ));
            }
            let actual = artifact_digest(&asset.artifact_path)?;
            if actual != asset.artifact_digest {
                return Err(AppError::conflict(format!(
                    "Exact profile asset changed: expected {}, found {}.",
                    asset.artifact_digest, actual
                )));
            }
            let evidence = ProfileAssetEvidence {
                profile_asset_digest: asset.artifact_digest.clone(),
                source_artifact_path: reference.source_artifact_path.clone(),
                source_artifact_digest: reference.source_artifact_digest.clone(),
                operation: reference.operation.clone(),
                tolerance_mm: reference.tolerance_mm,
            };
            let evidence_path =
                profile_asset_evidence_path(&app.app_data_dir().join("profile-assets"), &evidence)?;
            let stored: ProfileAssetEvidence =
                serde_json::from_slice(&std::fs::read(&evidence_path).map_err(|_| {
                    AppError::conflict(
                        "Exact profile asset has no server-issued derivation provenance.",
                    )
                })?)
                .map_err(|error| {
                    AppError::parse(format!("Exact profile provenance is invalid: {error}"))
                })?;
            if stored != evidence {
                return Err(AppError::conflict(
                    "Exact profile provenance does not match its source and operation.",
                ));
            }
        }
    }

    let (head, output) = {
        let db = state.db.lock().await;
        let head = crate::services::history::get_thread_latest_version(&db, &request.thread_id)?;
        let output = head.as_ref().and_then(|message| message.output.clone());
        (head, output)
    };
    let head_id = head.as_ref().map(|message| message.id.as_str());
    if request.base_message_id.as_deref() != head_id {
        return Err(AppError::conflict(
            "Profile target version changed; reload the current model before applying the profile.",
        ));
    }
    let output = output.ok_or_else(|| {
        AppError::validation("Profile authoring requires a current Ecky model source.")
    })?;
    if output.source_language != SourceLanguage::EckyIrV0 {
        return Err(AppError::validation(format!(
            "Profile authoring cannot append Ecky geometry to source language {}.",
            output.source_language.as_str()
        )));
    }
    if output.macro_code != request.expected_source {
        return Err(AppError::conflict(format!(
            "Profile target source diverged: expected {}, found {}.",
            crate::services::render_snapshot::canonical_source_digest(&request.expected_source),
            crate::services::render_snapshot::canonical_source_digest(&output.macro_code),
        )));
    }
    if let Some(reference) = request.reference.as_ref() {
        let bundle = head
            .as_ref()
            .and_then(|message| message.artifact_bundle.as_ref())
            .ok_or_else(|| {
                AppError::conflict(
                    "Profile evidence is not attached to the current version's artifact bundle.",
                )
            })?;
        let mut eligible = vec![bundle.model_stl_path.as_str(), bundle.fcstd_path.as_str()];
        eligible.extend(
            bundle
                .export_artifacts
                .iter()
                .filter(|artifact| {
                    matches!(
                        artifact.format.to_ascii_lowercase().as_str(),
                        "step" | "stp" | "fcstd" | "stl"
                    )
                })
                .map(|artifact| artifact.path.as_str()),
        );
        let bound_path = eligible
            .iter()
            .any(|path| same_file_path(path, &reference.source_artifact_path));
        if !bound_path {
            return Err(AppError::conflict(
                "Profile evidence source does not belong to the current version's artifact bundle.",
            ));
        }
        let bundle_digest = artifact_digest(&reference.source_artifact_path)?;
        if bundle_digest != reference.source_artifact_digest {
            return Err(AppError::conflict(
                "Profile evidence digest does not match the current version artifact.",
            ));
        }
    }

    let loops = request.loops.as_deref();
    let source = build_profile_feature_source(
        &request.expected_source,
        loops,
        request.plane,
        request.offset_mm,
        request.clearance_mm,
        request.depth_mm,
        &request.part_name,
        request.reference.as_ref(),
        request.base_message_id.as_deref(),
        request.drawing_profile.as_ref(),
        drawing_evidence.as_ref(),
    )?;
    let exact_profile = request
        .reference
        .as_ref()
        .is_some_and(|reference| reference.accuracy == ProfileEvidenceAccuracy::ExactBrep)
        || request.drawing_profile.is_some();
    let geometry_backend = if exact_profile {
        let program = crate::ecky_scheme::compile_to_core_program(&source).map_err(|error| {
            AppError::parse(format!("Exact profile source cannot be compiled: {error}"))
        })?;
        let (_, preflight_params, _) = crate::services::manual_code::reconcile_controls(
            &source,
            &output.ui_spec,
            &output.initial_params,
        );
        crate::ecky_cad_host::direct_occt::plan_core_program_with_params(
            &program,
            &preflight_params,
        )
        .map_err(|error| {
            AppError::validation(format!(
                "This exact or image-derived profile requires the complete model to render through Direct OCCT: {error}"
            ))
        })?;
        Some(crate::contracts::GeometryBackend::EckyRust)
    } else {
        Some(output.geometry_backend)
    };
    let expected_head_id = request.base_message_id.clone();
    let expected_source = request.expected_source.clone();
    let draft = crate::services::manual_code::apply_manual_code_guarded(
        ManualCodeApplyRequest {
            thread_id: request.thread_id,
            base_message_id: expected_head_id.clone(),
            source: source.clone(),
            persist: true,
            title: Some(output.title),
            version_name: Some(format!("{} profile", request.part_name)),
            ui_spec: output.ui_spec,
            parameters: output.initial_params,
            post_processing: output.post_processing,
            source_language: Some(SourceLanguage::EckyIrV0),
            geometry_backend,
        },
        expected_head_id,
        expected_source,
        state,
        app,
    )
    .await?;
    Ok(ProfileFeatureApplyResponse { source, draft })
}

fn validate_section_request(request: &ProfileSectionRequest) -> AppResult<()> {
    if !request.offset_mm.is_finite() {
        return Err(AppError::validation(
            "Section offset must be finite millimeters.",
        ));
    }
    if !request.tolerance_mm.is_finite() || !(1.0e-6..=1.0).contains(&request.tolerance_mm) {
        return Err(AppError::validation(
            "Section tolerance must be finite and between 0.000001 and 1 mm.",
        ));
    }
    if request.artifact_path.trim().is_empty() {
        return Err(AppError::validation(
            "Profile source artifact path is required.",
        ));
    }
    Ok(())
}

fn validate_feature_request(request: &ProfileFeatureApplyRequest) -> AppResult<()> {
    if !request.offset_mm.is_finite() {
        return Err(AppError::validation(
            "Profile plane offset must be finite millimeters.",
        ));
    }
    if !request.clearance_mm.is_finite() || request.clearance_mm < 0.0 {
        return Err(AppError::validation(
            "Profile clearance must be finite and non-negative millimeters.",
        ));
    }
    if !request.depth_mm.is_finite() || request.depth_mm <= 0.0 {
        return Err(AppError::validation(
            "Profile extrusion depth must be finite and greater than zero millimeters.",
        ));
    }
    if !valid_symbol(&request.part_name) {
        return Err(AppError::validation(
            "Profile part name must start with a letter or underscore and contain only letters, digits, underscores, or hyphens.",
        ));
    }
    if let Some(drawing) = request.drawing_profile.as_ref() {
        if request.reference.is_some() || request.loops.is_some() {
            return Err(AppError::validation(
                "Choose one profile source: drawing, CAD reference, or manual outline.",
            ));
        }
        if drawing.evidence_id.trim().is_empty() {
            return Err(AppError::validation(
                "Drawing recognition evidence is required.",
            ));
        }
        validate_drawing_primitives(&drawing.primitives)?;
        return Ok(());
    }
    match request.reference.as_ref() {
        Some(reference) => {
            if !reference.tolerance_mm.is_finite()
                || !(1.0e-6..=1.0).contains(&reference.tolerance_mm)
            {
                return Err(AppError::validation(
                    "Profile reference tolerance must be finite and between 0.000001 and 1 mm.",
                ));
            }
            let source_extension = Path::new(&reference.source_artifact_path)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            match reference.accuracy {
                ProfileEvidenceAccuracy::ExactBrep
                    if !matches!(source_extension.as_str(), "step" | "stp") =>
                {
                    return Err(AppError::validation(
                        "Exact CAD profiles need a STEP or STP file. Export FCStd to STEP first.",
                    ));
                }
                ProfileEvidenceAccuracy::ApproximateMesh if source_extension != "stl" => {
                    return Err(AppError::validation(
                        "Approximate mesh evidence must come from STL.",
                    ));
                }
                ProfileEvidenceAccuracy::ApproximatePolygon
                    if !matches!(source_extension.as_str(), "step" | "stp") =>
                {
                    return Err(AppError::validation(
                        "Approximate polygon conversion must retain its STEP/STP source provenance.",
                    ));
                }
                _ => {}
            }
            let (evidence_plane, evidence_offset) = match &reference.operation {
                ProfileDerivationOperation::Section { plane, offset_mm } => (*plane, *offset_mm),
                ProfileDerivationOperation::Projection { view } => match view {
                    ProfileView::Front => (ProfilePlane::Xz, 0.0),
                    ProfileView::Top => (ProfilePlane::Xy, 0.0),
                    ProfileView::Side => (ProfilePlane::Yz, 0.0),
                },
            };
            if evidence_plane != request.plane || evidence_offset != request.offset_mm {
                return Err(AppError::conflict(
                    "Profile plane and offset are locked to the derivation operation; derive again to change them.",
                ));
            }
            match reference.accuracy {
                ProfileEvidenceAccuracy::ExactBrep => {
                    let asset = reference.profile_asset.as_ref().ok_or_else(|| {
                        AppError::validation(
                            "Exact profile data is missing. Derive the profile again from the STEP file.",
                        )
                    })?;
                    if asset.artifact_path.trim().is_empty()
                        || !asset.artifact_path.ends_with(".step")
                    {
                        return Err(AppError::validation(
                            "Exact profile data must point to a saved STEP file.",
                        ));
                    }
                    if request.loops.as_ref().is_some_and(|loops| loops.is_empty()) {
                        return Err(AppError::validation(
                            "Exact profile display loops cannot be an empty editable profile.",
                        ));
                    }
                    Ok(())
                }
                ProfileEvidenceAccuracy::ApproximateMesh => {
                    if reference.profile_asset.is_some() {
                        return Err(AppError::validation(
                            "An approximate STL outline cannot use exact STEP profile data.",
                        ));
                    }
                    if !matches!(
                        &reference.operation,
                        ProfileDerivationOperation::Section { .. }
                    ) {
                        return Err(AppError::validation(
                            "Approximate mesh projection is unsupported.",
                        ));
                    }
                    validate_profile_loops(request.loops.as_deref().ok_or_else(|| {
                        AppError::validation("Approximate mesh profile requires its derived loops.")
                    })?)
                    .map(|_| ())
                }
                ProfileEvidenceAccuracy::ApproximatePolygon => {
                    if reference.profile_asset.is_some() {
                        return Err(AppError::validation(
                            "A converted outline cannot use exact STEP profile data.",
                        ));
                    }
                    validate_profile_loops(request.loops.as_deref().ok_or_else(|| {
                        AppError::validation(
                            "Converted approximate polygon profile requires explicit loops.",
                        )
                    })?)
                    .map(|_| ())
                }
            }
        }
        None => validate_profile_loops(request.loops.as_deref().ok_or_else(|| {
            AppError::validation("Manual profile requires at least one closed loop.")
        })?)
        .map(|_| ()),
    }
}

fn validate_drawing_primitives(
    primitives: &[crate::services::drawing_import::DrawingPrimitive],
) -> AppResult<Vec<crate::services::drawing_import::DrawingPrimitive>> {
    crate::services::drawing_import::validate_accepted_profile_primitives(primitives)
}

fn drawing_geometry_expression(
    primitives: &[crate::services::drawing_import::DrawingPrimitive],
    clearance_name: &str,
    depth_name: &str,
) -> AppResult<String> {
    use crate::services::drawing_import::DrawingProfileRole;
    let loops = validate_drawing_primitives(primitives)?;
    let profiles = crate::services::drawing_import::assemble_profile_candidates(&loops);
    let primitive_by_id = loops
        .iter()
        .map(|primitive| (drawing_primitive_id(primitive), primitive))
        .collect::<BTreeMap<_, _>>();
    let regions = profiles
        .iter()
        .filter(|profile| profile.role == DrawingProfileRole::Outer && profile.parent_id.is_none())
        .map(|profile| {
            drawing_region_expression(
                profile,
                &profiles,
                &primitive_by_id,
                clearance_name,
                depth_name,
            )
        })
        .collect::<AppResult<Vec<_>>>()?;
    if regions.is_empty() {
        return Err(AppError::validation(
            "Drawing profile needs a closed outer boundary.",
        ));
    }
    Ok(union_expression(regions))
}

fn drawing_region_expression(
    profile: &crate::services::drawing_import::DrawingProfileCandidate,
    profiles: &[crate::services::drawing_import::DrawingProfileCandidate],
    primitive_by_id: &BTreeMap<&str, &crate::services::drawing_import::DrawingPrimitive>,
    clearance_name: &str,
    depth_name: &str,
) -> AppResult<String> {
    use crate::services::drawing_import::DrawingProfileRole;
    let primitive_id = profile
        .primitive_ids
        .first()
        .ok_or_else(|| AppError::validation("Drawing profile candidate has no source geometry."))?;
    let primitive = primitive_by_id.get(primitive_id.as_str()).ok_or_else(|| {
        AppError::validation("Drawing profile candidate references missing geometry.")
    })?;
    let base = extruded_drawing_primitive(primitive, clearance_name, depth_name)?;
    let mut result = base;
    for hole in profiles.iter().filter(|candidate| {
        candidate.role == DrawingProfileRole::Hole
            && candidate.parent_id.as_deref() == Some(profile.id.as_str())
    }) {
        let hole_id = hole
            .primitive_ids
            .first()
            .ok_or_else(|| AppError::validation("Drawing hole has no source geometry."))?;
        let hole_primitive = primitive_by_id
            .get(hole_id.as_str())
            .ok_or_else(|| AppError::validation("Drawing hole references missing geometry."))?;
        let hole_shape = extruded_drawing_primitive(
            hole_primitive,
            &format!("(- {clearance_name})"),
            depth_name,
        )?;
        let islands = profiles
            .iter()
            .filter(|candidate| {
                candidate.role == DrawingProfileRole::Outer
                    && candidate.parent_id.as_deref() == Some(hole.id.as_str())
            })
            .map(|candidate| {
                drawing_region_expression(
                    candidate,
                    profiles,
                    primitive_by_id,
                    clearance_name,
                    depth_name,
                )
            })
            .collect::<AppResult<Vec<_>>>()?;
        let void = if islands.is_empty() {
            hole_shape
        } else {
            format!("(difference {} {})", hole_shape, union_expression(islands))
        };
        result = format!("(difference {} {})", result, void);
    }
    Ok(result)
}

fn extruded_drawing_primitive(
    primitive: &crate::services::drawing_import::DrawingPrimitive,
    offset: &str,
    depth_name: &str,
) -> AppResult<String> {
    use crate::services::drawing_import::DrawingPrimitive;
    let shape = match primitive {
        DrawingPrimitive::Circle {
            center_mm,
            radius_mm,
            ..
        } => format!(
            "(translate {} {} 0 (offset {offset} (circle {})))",
            number(center_mm[0]),
            number(center_mm[1]),
            number(*radius_mm)
        ),
        DrawingPrimitive::Polyline {
            points_mm,
            closed: true,
            ..
        } => {
            let points = points_mm
                .iter()
                .map(|point| format!("({} {})", number(point[0]), number(point[1])))
                .collect::<Vec<_>>()
                .join(" ");
            format!("(offset {offset} (polygon ({points})))")
        }
        _ => {
            return Err(AppError::validation(
                "Drawing profile contains open or unsupported geometry.",
            ))
        }
    };
    Ok(format!("(extrude {shape} {depth_name})"))
}

fn union_expression(expressions: Vec<String>) -> String {
    if expressions.len() == 1 {
        expressions.into_iter().next().unwrap_or_default()
    } else {
        format!("(union {})", expressions.join(" "))
    }
}

fn drawing_primitive_id(primitive: &crate::services::drawing_import::DrawingPrimitive) -> &str {
    use crate::services::drawing_import::DrawingPrimitive;
    match primitive {
        DrawingPrimitive::Line { id, .. }
        | DrawingPrimitive::Circle { id, .. }
        | DrawingPrimitive::Polyline { id, .. } => id,
    }
}

fn validate_profile_loops(loops: &[ProfileLoop]) -> AppResult<Vec<ProfileLoop>> {
    if loops.is_empty() {
        return Err(AppError::validation(
            "Profile needs at least one closed loop.",
        ));
    }
    let mut normalized = Vec::with_capacity(loops.len());
    for (index, loop_) in loops.iter().enumerate() {
        let mut points = loop_.points.clone();
        if points
            .iter()
            .flatten()
            .any(|coordinate| !coordinate.is_finite())
        {
            return Err(AppError::validation(format!(
                "Profile loop {index} contains a non-finite point."
            )));
        }
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < MIN_LOOP_POINTS {
            return Err(AppError::validation(format!(
                "Profile loop {index} needs at least three distinct points."
            )));
        }
        if points.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(AppError::validation(format!(
                "Profile loop {index} contains consecutive duplicate points."
            )));
        }
        if loop_self_intersects(&points) {
            return Err(AppError::validation(format!(
                "Profile loop {index} self-intersects."
            )));
        }
        let area = signed_area(&points);
        if area.abs() <= 1.0e-12 {
            return Err(AppError::validation(format!(
                "Profile loop {index} has zero area."
            )));
        }
        if let Some(parent) = loop_.parent_index {
            if parent >= loops.len() || parent == index {
                return Err(AppError::validation(format!(
                    "Profile loop {index} has an invalid parent index."
                )));
            }
            if !point_in_loop(points[0], &loops[parent].points) {
                return Err(AppError::validation(format!(
                    "Profile loop {index} is not contained by parent loop {parent}."
                )));
            }
        } else if loop_.role == ProfileLoopRole::Hole {
            return Err(AppError::validation(format!(
                "Profile hole loop {index} must identify its containing outer loop."
            )));
        }
        normalized.push(ProfileLoop {
            points,
            role: loop_.role,
            parent_index: loop_.parent_index,
        });
    }
    for left in 0..normalized.len() {
        for right in (left + 1)..normalized.len() {
            if loops_intersect(&normalized[left].points, &normalized[right].points) {
                return Err(AppError::validation(format!(
                    "Profile loops {left} and {right} intersect or touch."
                )));
            }
        }
    }
    for (index, loop_) in normalized.iter().enumerate() {
        let containing = normalized
            .iter()
            .enumerate()
            .filter(|(candidate, parent)| {
                *candidate != index && point_in_loop(loop_.points[0], &parent.points)
            })
            .min_by(|(_, left), (_, right)| {
                signed_area(&left.points)
                    .abs()
                    .total_cmp(&signed_area(&right.points).abs())
            })
            .map(|(candidate, _)| candidate);
        if loop_.parent_index != containing {
            return Err(AppError::validation(format!(
                "Profile loop {index} must name its nearest containing parent {containing:?}."
            )));
        }
        let expected_role = match containing.map(|parent| normalized[parent].role) {
            None | Some(ProfileLoopRole::Hole) => ProfileLoopRole::Outer,
            Some(ProfileLoopRole::Outer) => ProfileLoopRole::Hole,
        };
        if loop_.role != expected_role {
            return Err(AppError::validation(format!(
                "Profile loop {index} has role {:?}; containment requires {:?}.",
                loop_.role, expected_role
            )));
        }
    }
    Ok(normalized)
}

fn intersect_mesh_plane(
    vertices: &[[f64; 3]],
    triangles: &[[u32; 3]],
    plane: ProfilePlane,
    offset: f64,
    tolerance: f64,
) -> AppResult<Vec<[[f64; 2]; 2]>> {
    let axis = plane.axis();
    let mut segments = Vec::new();
    let mut coplanar_edge = false;
    for (triangle_index, triangle) in triangles.iter().enumerate() {
        let points = triangle.map(|index| vertices[index as usize]);
        let distances = points.map(|point| point[axis] - offset);
        let on = distances.map(|distance| distance.abs() <= tolerance);
        if on.iter().all(|value| *value) {
            return Err(AppError::validation(format!(
                "Section plane coincides with triangle {triangle_index}; move the offset away from coplanar mesh faces."
            )));
        }
        if on.iter().filter(|value| **value).count() >= 2 {
            coplanar_edge = true;
            continue;
        }
        let mut intersections = Vec::<[f64; 3]>::with_capacity(2);
        for edge in [[0, 1], [1, 2], [2, 0]] {
            let left = edge[0];
            let right = edge[1];
            let dl = distances[left];
            let dr = distances[right];
            if on[left] {
                intersections.push(points[left]);
            } else if on[right] {
                intersections.push(points[right]);
            } else if (dl < 0.0 && dr > 0.0) || (dl > 0.0 && dr < 0.0) {
                let t = dl / (dl - dr);
                let mut intersection = [0.0; 3];
                for coordinate in 0..3 {
                    intersection[coordinate] = points[left][coordinate]
                        + (points[right][coordinate] - points[left][coordinate]) * t;
                }
                intersection[axis] = offset;
                intersections.push(intersection);
            }
        }
        let mut unique = Vec::<[f64; 3]>::with_capacity(3);
        for point in intersections {
            if unique
                .iter()
                .all(|existing| distance3(*existing, point) > tolerance)
            {
                unique.push(point);
            }
        }
        let intersections = unique;
        if intersections.len() == 2 {
            let first = plane.project(intersections[0]);
            let second = plane.project(intersections[1]);
            if distance2(first, second) > tolerance {
                segments.push([first, second]);
            }
        } else if intersections.len() > 2 {
            return Err(AppError::validation(format!(
                "Section triangle {triangle_index} has ambiguous plane intersection."
            )));
        }
    }
    if coplanar_edge {
        return Err(AppError::validation(
            "Section plane follows a mesh edge; move the offset to avoid ambiguous coplanar topology.",
        ));
    }
    if segments.is_empty() {
        return Err(AppError::validation(
            "Section plane does not intersect source mesh in a closed profile.",
        ));
    }
    Ok(segments)
}

fn stitch_section_loops(segments: &[[[f64; 2]; 2]], tolerance: f64) -> AppResult<Vec<ProfileLoop>> {
    let mut points = Vec::<[f64; 2]>::new();
    let mut buckets = BTreeMap::<PointKey, Vec<usize>>::new();
    let mut edges = BTreeSet::<(usize, usize)>::new();
    let cell = tolerance;
    let intern = |point: [f64; 2],
                  points: &mut Vec<[f64; 2]>,
                  buckets: &mut BTreeMap<PointKey, Vec<usize>>| {
        let key = PointKey([
            (point[0] / cell).floor() as i64,
            (point[1] / cell).floor() as i64,
        ]);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let neighbor = PointKey([key.0[0] + dx, key.0[1] + dy]);
                if let Some(indices) = buckets.get(&neighbor) {
                    if let Some(index) = indices
                        .iter()
                        .copied()
                        .find(|index| distance2(points[*index], point) <= tolerance)
                    {
                        return index;
                    }
                }
            }
        }
        let index = points.len();
        points.push(point);
        buckets.entry(key).or_default().push(index);
        index
    };
    for segment in segments {
        let left = intern(segment[0], &mut points, &mut buckets);
        let right = intern(segment[1], &mut points, &mut buckets);
        if left == right {
            continue;
        }
        let edge = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        if !edges.insert(edge) {
            return Err(AppError::validation(
                "Section contains duplicate mesh intersection edges; topology is ambiguous.",
            ));
        }
    }
    let mut adjacency = vec![Vec::<usize>::new(); points.len()];
    for (left, right) in &edges {
        adjacency[*left].push(*right);
        adjacency[*right].push(*left);
    }
    for (index, neighbors) in adjacency.iter_mut().enumerate() {
        neighbors.sort_unstable();
        match neighbors.len() {
            0 => {}
            2 => {}
            1 => {
                return Err(AppError::validation(format!(
                    "Section profile is open at contour vertex {index} (one connected edge)."
                )))
            }
            degree => {
                return Err(AppError::validation(format!(
                    "Section profile branches at contour vertex {index} (degree {degree})."
                )))
            }
        }
    }
    let mut used_edges = BTreeSet::<(usize, usize)>::new();
    let mut raw_loops = Vec::<Vec<[f64; 2]>>::new();
    for start in 0..points.len() {
        if adjacency[start].is_empty()
            || adjacency[start]
                .iter()
                .all(|neighbor| used_edges.contains(&ordered_edge(start, *neighbor)))
        {
            continue;
        }
        let mut contour = vec![start];
        let mut previous = usize::MAX;
        let mut current = start;
        loop {
            let next = adjacency[current]
                .iter()
                .copied()
                .find(|neighbor| {
                    *neighbor != previous && !used_edges.contains(&ordered_edge(current, *neighbor))
                })
                .or_else(|| {
                    adjacency[current]
                        .iter()
                        .copied()
                        .find(|neighbor| *neighbor == start)
                })
                .ok_or_else(|| {
                    AppError::validation("Section contour traversal ended before closure.")
                })?;
            used_edges.insert(ordered_edge(current, next));
            if next == start {
                break;
            }
            contour.push(next);
            previous = current;
            current = next;
            if contour.len() > edges.len() {
                return Err(AppError::validation(
                    "Section contour traversal did not close.",
                ));
            }
        }
        let mut loop_points = contour
            .into_iter()
            .map(|index| points[index])
            .collect::<Vec<_>>();
        if loop_points.len() < MIN_LOOP_POINTS || signed_area(&loop_points).abs() <= 1.0e-12 {
            return Err(AppError::validation(
                "Section produced a degenerate closed loop.",
            ));
        }
        if loop_self_intersects(&loop_points) {
            return Err(AppError::validation(
                "Section produced a self-intersecting loop.",
            ));
        }
        canonicalize_loop(&mut loop_points);
        raw_loops.push(loop_points);
    }
    if used_edges.len() != edges.len() || raw_loops.is_empty() {
        return Err(AppError::validation(
            "Section mesh intersections did not form closed loops.",
        ));
    }
    classify_loops(raw_loops)
}

fn classify_loops(mut loops: Vec<Vec<[f64; 2]>>) -> AppResult<Vec<ProfileLoop>> {
    let parents = loops
        .iter()
        .enumerate()
        .map(|(index, loop_points)| {
            loops
                .iter()
                .enumerate()
                .filter(|(candidate, _)| *candidate != index)
                .filter(|(_, candidate)| point_in_loop(loop_points[0], candidate))
                .min_by(|(_, left), (_, right)| {
                    signed_area(left).abs().total_cmp(&signed_area(right).abs())
                })
                .map(|(candidate, _)| candidate)
        })
        .collect::<Vec<_>>();
    let mut depths = vec![0usize; loops.len()];
    for index in 0..loops.len() {
        let mut parent = parents[index];
        let mut seen = BTreeSet::new();
        while let Some(parent_index) = parent {
            if !seen.insert(parent_index) || parent_index == index {
                return Err(AppError::validation(
                    "Section loop containment is ambiguous.",
                ));
            }
            depths[index] += 1;
            parent = parents[parent_index];
        }
    }
    let mut order = (0..loops.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        depths[*left].cmp(&depths[*right]).then_with(|| {
            let l = loop_sort_key(&loops[*left]);
            let r = loop_sort_key(&loops[*right]);
            l.cmp(&r)
        })
    });
    let remap = order
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect::<BTreeMap<_, _>>();
    let sorted = order
        .iter()
        .map(|index| loops[*index].clone())
        .collect::<Vec<_>>();
    loops = sorted;
    let mut result = Vec::with_capacity(loops.len());
    for (new_index, old_index) in order.into_iter().enumerate() {
        let role = if depths[old_index].is_multiple_of(2) {
            ProfileLoopRole::Outer
        } else {
            ProfileLoopRole::Hole
        };
        let parent_index = parents[old_index].map(|parent| remap[&parent]);
        let area = signed_area(&loops[new_index]);
        if (role == ProfileLoopRole::Outer && area < 0.0)
            || (role == ProfileLoopRole::Hole && area > 0.0)
        {
            loops[new_index].reverse();
            canonicalize_loop(&mut loops[new_index]);
        }
        result.push(ProfileLoop {
            points: loops[new_index].clone(),
            role,
            parent_index,
        });
    }
    Ok(result)
}

fn build_profile_feature_source(
    source: &str,
    loops: Option<&[ProfileLoop]>,
    plane: ProfilePlane,
    offset: f64,
    clearance: f64,
    depth: f64,
    part_name: &str,
    reference: Option<&ProfileArtifactReference>,
    base_message_id: Option<&str>,
    drawing_profile: Option<&crate::services::drawing_import::DrawingProfileApplyInput>,
    drawing_evidence: Option<&crate::services::drawing_import::DrawingImportEvidence>,
) -> AppResult<String> {
    let profile = if drawing_profile.is_some() {
        String::new()
    } else if let Some(reference) =
        reference.filter(|reference| reference.accuracy == ProfileEvidenceAccuracy::ExactBrep)
    {
        let asset = reference
            .profile_asset
            .as_ref()
            .ok_or_else(|| AppError::validation("Exact profile asset is required."))?;
        format!(
            "(import-profile \"{}\")",
            asset
                .artifact_path
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        )
    } else {
        profile_expression(&validate_profile_loops(loops.ok_or_else(|| {
            AppError::validation("Profile loops are required for approximate or manual geometry.")
        })?)?)?
    };
    let clearance_name = format!("{}_clearance", part_name.replace('-', "_"));
    let depth_name = format!("{}_depth", part_name.replace('-', "_"));
    let (translation, rotation) = plane.translation(offset);
    let part = if let Some(drawing) = drawing_profile {
        let geometry =
            drawing_geometry_expression(&drawing.primitives, &clearance_name, &depth_name)?;
        format!(
            "(part {part_name}\n    (translate {} {} {}\n      (rotate {} {} {}\n        {geometry})))",
            number(translation[0]), number(translation[1]), number(translation[2]),
            number(rotation[0]), number(rotation[1]), number(rotation[2])
        )
    } else {
        format!(
            "(part {part_name}\n    (translate {} {} {}\n      (rotate {} {} {}\n        (extrude\n          PROFILE_EXPRESSION\n          {depth_name}))))",
            number(translation[0]), number(translation[1]), number(translation[2]),
            number(rotation[0]), number(rotation[1]), number(rotation[2])
        )
    };
    let nodes = crate::commands::macro_ast::macro_ast_source_map_impl(source)?;
    let model = nodes
        .iter()
        .find(|node| node.id == "model" && node.kind == "model")
        .ok_or_else(|| AppError::parse("Profile target has no parser-derived model AST range."))?;
    let insert_at = (model.end_byte as usize)
        .checked_sub(1)
        .ok_or_else(|| AppError::parse("Profile model AST range is empty."))?;
    if source.as_bytes().get(insert_at) != Some(&b')') {
        return Err(AppError::parse(
            "Profile model AST range does not end at the model closing parenthesis.",
        ));
    }
    let mut used = BTreeSet::new();
    for node in &nodes {
        if let Some((_, name)) = node.id.split_once(':') {
            used.insert(name.to_string());
        }
    }
    if used.contains(part_name) {
        return Err(AppError::conflict(format!(
            "Profile part name '{part_name}' conflicts with an existing model symbol."
        )));
    }
    let program = crate::ecky_scheme::compile_to_core_program(source).map_err(|error| {
        AppError::parse(format!("Profile target source cannot be compiled: {error}"))
    })?;
    let parameter_names = program
        .parameters
        .iter()
        .map(|parameter| parameter.key.as_str())
        .collect::<BTreeSet<_>>();
    if parameter_names.contains(clearance_name.as_str())
        || parameter_names.contains(depth_name.as_str())
        || parameter_names.contains(part_name)
        || used.contains(&clearance_name)
        || used.contains(&depth_name)
    {
        return Err(AppError::conflict(format!(
            "Profile parameter name conflicts with an existing model parameter ('{clearance_name}' or '{depth_name}')."
        )));
    }
    let declarations = format!(
        "\n  (params\n    (number {clearance_name} {} :label \"{} clearance\" :min 0 :max 100 :step 0.1)\n    (number {depth_name} {} :label \"{} depth\" :min 0.001 :max 1000 :step 0.1))",
        number(clearance), part_name, number(depth), part_name
    );
    let part = if drawing_profile.is_some() {
        part
    } else {
        let profile = format!("(offset {clearance_name} {profile})");
        part.replace("PROFILE_EXPRESSION", &profile)
    };
    let evidence = if let (Some(drawing), Some(evidence)) = (drawing_profile, drawing_evidence) {
        let source_digest = crate::services::render_snapshot::canonical_source_digest(source);
        let value = serde_json::json!({
            "kind": "drawingImport",
            "accuracy": "imageDerived",
            "evidenceId": drawing.evidence_id,
            "baseMessageId": base_message_id,
            "sourceDigest": source_digest,
            "image": evidence.image,
            "view": evidence.view,
            "region": evidence.region,
            "rectification": evidence.rectification,
            "calibration": evidence.calibration,
            "toleranceMm": evidence.tolerance_mm,
            "extractorVersion": evidence.extractor_version,
            "acceptedPrimitives": drawing.primitives,
            "units": "mm",
        });
        format!(
            "; profileEvidence {}\n  ",
            serde_json::to_string(&value)
                .map_err(|error| AppError::validation(error.to_string()))?
        )
    } else if let Some(reference) = reference {
        let source_digest = crate::services::render_snapshot::canonical_source_digest(source);
        let value = serde_json::json!({
            "accuracy": match reference.accuracy { ProfileEvidenceAccuracy::ExactBrep => "exactBrep", ProfileEvidenceAccuracy::ApproximateMesh => "approximateMesh", ProfileEvidenceAccuracy::ApproximatePolygon => "approximatePolygon" },
            "baseMessageId": base_message_id,
            "sourceDigest": source_digest,
            "sourceArtifactPath": reference.source_artifact_path,
            "sourceArtifactDigest": reference.source_artifact_digest,
            "operation": reference.operation,
            "toleranceMm": reference.tolerance_mm,
            "profileAsset": reference.profile_asset,
            "units": "mm",
        });
        format!(
            "; profileEvidence {}\n  ",
            serde_json::to_string(&value)
                .map_err(|error| AppError::validation(error.to_string()))?
        )
    } else {
        String::new()
    };
    Ok(format!(
        "{}{}{}\n  {}{}",
        &source[..insert_at],
        evidence,
        declarations,
        part,
        &source[insert_at..]
    ))
}

fn profile_expression(loops: &[ProfileLoop]) -> AppResult<String> {
    let mut outer = loops
        .iter()
        .enumerate()
        .filter(|(_, loop_)| loop_.role == ProfileLoopRole::Outer && loop_.parent_index.is_none())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    outer.sort_unstable();
    if outer.is_empty() {
        return Err(AppError::validation(
            "Profile needs at least one outer boundary.",
        ));
    }
    let regions = outer
        .into_iter()
        .map(|index| profile_region_expression(index, loops))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(if regions.len() == 1 {
        regions[0].clone()
    } else {
        format!("(union {})", regions.join(" "))
    })
}

fn profile_region_expression(index: usize, loops: &[ProfileLoop]) -> AppResult<String> {
    let holes = loops
        .iter()
        .filter(|loop_| loop_.role == ProfileLoopRole::Hole && loop_.parent_index == Some(index))
        .map(|loop_| loop_expression(&loop_.points))
        .collect::<Vec<_>>();
    let outer = loop_expression(&loops[index].points);
    let profile = if holes.is_empty() {
        outer
    } else {
        let holes = if holes.len() == 1 {
            holes[0].clone()
        } else {
            format!("(list {})", holes.join(" "))
        };
        format!("(profile :outer {outer} :holes {holes})")
    };
    let islands = loops
        .iter()
        .enumerate()
        .filter(|(_, loop_)| {
            loop_.role == ProfileLoopRole::Outer
                && loop_.parent_index.is_some_and(|parent| {
                    loops[parent].role == ProfileLoopRole::Hole
                        && loops[parent].parent_index == Some(index)
                })
        })
        .map(|(child, _)| profile_region_expression(child, loops))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(if islands.is_empty() {
        profile
    } else {
        format!("(union {profile} {})", islands.join(" "))
    })
}

fn loop_expression(points: &[[f64; 2]]) -> String {
    let points = points
        .iter()
        .map(|point| format!("({} {})", number(point[0]), number(point[1])))
        .collect::<Vec<_>>();
    format!("(polygon ({}))", points.join(" "))
}

fn artifact_digest(path: &str) -> AppResult<String> {
    let bytes = std::fs::read(path).map_err(|error| {
        AppError::not_found(format!(
            "Profile source artifact could not be read: {error}"
        ))
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn profile_asset_evidence_path(
    asset_root: &Path,
    evidence: &ProfileAssetEvidence,
) -> AppResult<PathBuf> {
    let serialized =
        serde_json::to_vec(evidence).map_err(|error| AppError::persistence(error.to_string()))?;
    let evidence_digest = format!("{:x}", Sha256::digest(serialized));
    let asset_digest = evidence.profile_asset_digest.trim_start_matches("sha256:");
    Ok(asset_root.join(format!("{asset_digest}.{evidence_digest}.json")))
}

fn same_file_path(left: &str, right: &str) -> bool {
    let left = Path::new(left);
    let right = Path::new(right);
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn valid_symbol(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn number(value: f64) -> String {
    let text = format!("{value:.8}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn signed_area(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let next = points[(index + 1) % points.len()];
            point[0] * next[1] - next[0] * point[1]
        })
        .sum::<f64>()
        * 0.5
}

fn loop_sort_key(points: &[[f64; 2]]) -> (i64, i64, i64) {
    let min_x = points
        .iter()
        .map(|point| point[0])
        .fold(f64::INFINITY, f64::min);
    let min_y = points
        .iter()
        .map(|point| point[1])
        .fold(f64::INFINITY, f64::min);
    let area = signed_area(points).abs();
    (
        (min_x * 1_000_000.0).round() as i64,
        (min_y * 1_000_000.0).round() as i64,
        (-area * 1_000_000.0).round() as i64,
    )
}

fn canonicalize_loop(points: &mut [[f64; 2]]) {
    if let Some((start, _)) = points.iter().enumerate().min_by(|(_, left), (_, right)| {
        left[0]
            .total_cmp(&right[0])
            .then_with(|| left[1].total_cmp(&right[1]))
    }) {
        points.rotate_left(start);
    }
}

fn point_in_loop(point: [f64; 2], loop_points: &[[f64; 2]]) -> bool {
    let mut inside = false;
    for (index, current) in loop_points.iter().enumerate() {
        let previous = loop_points[(index + loop_points.len() - 1) % loop_points.len()];
        if ((current[1] > point[1]) != (previous[1] > point[1]))
            && point[0]
                < (previous[0] - current[0]) * (point[1] - current[1]) / (previous[1] - current[1])
                    + current[0]
        {
            inside = !inside;
        }
    }
    inside
}

fn loop_self_intersects(points: &[[f64; 2]]) -> bool {
    let len = points.len();
    (0..len).any(|left| {
        let left_next = (left + 1) % len;
        (left + 1..len).any(|right| {
            let right_next = (right + 1) % len;
            if left == right || left_next == right || right_next == left {
                false
            } else {
                segments_intersect(
                    points[left],
                    points[left_next],
                    points[right],
                    points[right_next],
                )
            }
        })
    })
}

fn loops_intersect(left: &[[f64; 2]], right: &[[f64; 2]]) -> bool {
    (0..left.len()).any(|left_index| {
        let left_next = (left_index + 1) % left.len();
        (0..right.len()).any(|right_index| {
            let right_next = (right_index + 1) % right.len();
            segments_intersect(
                left[left_index],
                left[left_next],
                right[right_index],
                right[right_next],
            )
        })
    })
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let orient = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let ab_c = orient(a, b, c);
    let ab_d = orient(a, b, d);
    let cd_a = orient(c, d, a);
    let cd_b = orient(c, d, b);
    let eps = 1.0e-10;
    (ab_c.abs() <= eps && point_on_segment(c, a, b))
        || (ab_d.abs() <= eps && point_on_segment(d, a, b))
        || (cd_a.abs() <= eps && point_on_segment(a, c, d))
        || (cd_b.abs() <= eps && point_on_segment(b, c, d))
        || ((ab_c > 0.0) != (ab_d > 0.0) && (cd_a > 0.0) != (cd_b > 0.0))
}

fn point_on_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> bool {
    point[0] >= start[0].min(end[0]) - 1.0e-10
        && point[0] <= start[0].max(end[0]) + 1.0e-10
        && point[1] >= start[1].min(end[1]) - 1.0e-10
        && point[1] <= start[1].max(end[1]) + 1.0e-10
}

fn distance2(left: [f64; 2], right: [f64; 2]) -> f64 {
    (left[0] - right[0]).hypot(left[1] - right[1])
}

fn distance3(left: [f64; 3], right: [f64; 3]) -> f64 {
    ((left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2))
        .sqrt()
}

fn ordered_edge(left: usize, right: usize) -> (usize, usize) {
    if left < right {
        (left, right)
    } else {
        (right, left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::DesignParams;

    fn view_name(view: ProfileView) -> &'static str {
        match view {
            ProfileView::Front => "front",
            ProfileView::Top => "top",
            ProfileView::Side => "side",
        }
    }

    fn square_mesh(z_min: f64, z_max: f64) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        let vertices = vec![
            [0.0, 0.0, z_min],
            [2.0, 0.0, z_min],
            [2.0, 2.0, z_min],
            [0.0, 2.0, z_min],
            [0.0, 0.0, z_max],
            [2.0, 0.0, z_max],
            [2.0, 2.0, z_max],
            [0.0, 2.0, z_max],
        ];
        let triangles = vec![
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
            [0, 3, 2],
            [0, 2, 1],
            [4, 5, 6],
            [4, 6, 7],
        ];
        (vertices, triangles)
    }

    fn cube(offset: [f64; 3]) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        let (mut vertices, triangles) = square_mesh(-1.0, 1.0);
        for vertex in &mut vertices {
            for axis in 0..3 {
                vertex[axis] += offset[axis];
            }
        }
        (vertices, triangles)
    }

    fn loop_bounds(points: &[[f64; 2]]) -> [f64; 4] {
        [
            points
                .iter()
                .map(|point| point[0])
                .fold(f64::INFINITY, f64::min),
            points
                .iter()
                .map(|point| point[1])
                .fold(f64::INFINITY, f64::min),
            points
                .iter()
                .map(|point| point[0])
                .fold(f64::NEG_INFINITY, f64::max),
            points
                .iter()
                .map(|point| point[1])
                .fold(f64::NEG_INFINITY, f64::max),
        ]
    }

    #[test]
    fn drawing_projection_validation_accepts_independent_edges_and_rejects_invalid_display_points()
    {
        assert_eq!(
            validate_display_edges(vec![vec![[0.0, 0.0], [0.0, 0.0]]]).unwrap(),
            vec![vec![[0.0, 0.0], [0.0, 0.0]]],
            "point-degenerate exact edges stay independent; validator does not fabricate a contour"
        );
        assert!(validate_display_edges(vec![vec![[0.0, 0.0]]]).is_err());
        assert!(validate_display_edges(vec![vec![[0.0, f64::NAN], [1.0, 1.0]]]).is_err());
        assert!(validate_display_edges(Vec::new()).is_err());
    }

    #[test]
    fn exact_front_drawing_projection_persists_verified_asset_outside_profile_apply_namespace() {
        struct Resolver {
            root: PathBuf,
            runner: Option<PathBuf>,
        }
        impl PathResolver for Resolver {
            fn app_config_dir(&self) -> PathBuf {
                self.root.clone()
            }

            fn app_data_dir(&self) -> PathBuf {
                self.root.clone()
            }

            fn resource_path(&self, path: &str) -> Option<PathBuf> {
                if path == "runtime/occt/bin/direct-occt-runner" {
                    self.runner.clone()
                } else {
                    None
                }
            }
        }

        let root =
            std::env::temp_dir().join(format!("ecky-drawing-projection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temporary app data");
        let resolver = Resolver {
            root: root.clone(),
            runner: std::env::var_os("ECKY_PROFILE_PROJECTION_TEST_RUNNER").map(PathBuf::from),
        };
        let Some(runner) =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &resolver, true,
            )
        else {
            let _ = std::fs::remove_dir_all(root);
            return;
        };
        let supports_drawing_projection = Command::new(&runner)
            .arg("--help")
            .output()
            .ok()
            .is_some_and(|output| {
                String::from_utf8_lossy(&output.stdout).contains("--project-drawing")
            });
        if !supports_drawing_projection {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let source_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fem/exact-sphere.step");
        let source_path_text = source_path.to_string_lossy();
        let response = project_profile_drawing(
            ProjectProfileDrawingRequest {
                artifact_path: source_path_text.to_string(),
                view: ProfileView::Front,
                tolerance_mm: 0.01,
            },
            &resolver,
        )
        .expect("exact visible drawing edges");
        assert_eq!(
            response.source_artifact_digest,
            artifact_digest(&source_path_text).unwrap()
        );
        assert_eq!(response.units, "mm");
        assert!(!response.display_edges.is_empty());
        assert!(response.display_edges.iter().all(|edge| edge.len() >= 2));
        assert!(response
            .projection_asset
            .artifact_path
            .starts_with(root.join("projection-assets").to_string_lossy().as_ref()));
        assert!(std::path::Path::new(&response.projection_asset.artifact_path).is_file());
        assert_eq!(
            artifact_digest(&response.projection_asset.artifact_path).unwrap(),
            response.projection_asset.artifact_digest
        );
        assert!(!root.join("profile-assets").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn given_closed_mesh_section_when_sliced_then_returns_deterministic_closed_outer_loop() {
        let (vertices, triangles) = square_mesh(-1.0, 1.0);
        let segments =
            intersect_mesh_plane(&vertices, &triangles, ProfilePlane::Xy, 0.0, 1.0e-5).unwrap();
        let loops = stitch_section_loops(&segments, 1.0e-5).unwrap();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].role, ProfileLoopRole::Outer);
        assert!(loops[0].parent_index.is_none());
        assert_eq!(
            loops[0].points.len(),
            8,
            "faceted side split points remain observable"
        );
        assert_eq!(loop_bounds(&loops[0].points), [0.0, 0.0, 2.0, 2.0]);
    }

    #[test]
    fn given_open_section_edges_when_stitched_then_reports_open_topology() {
        let segments = vec![[[0.0, 0.0], [1.0, 0.0]], [[1.0, 0.0], [1.0, 1.0]]];
        let error = stitch_section_loops(&segments, 1.0e-5).unwrap_err();
        assert!(error.message.contains("is open"));
    }

    #[test]
    fn given_principal_plane_and_nonzero_offset_when_sectioned_then_preserves_plane_coordinates() {
        let (vertices, triangles) = cube([0.0; 3]);
        for (plane, expected) in [
            (ProfilePlane::Xy, [0.0, 0.0, 2.0, 2.0]),
            (ProfilePlane::Xz, [0.0, -1.0, 2.0, 1.0]),
            (ProfilePlane::Yz, [0.0, -1.0, 2.0, 1.0]),
        ] {
            let segments = intersect_mesh_plane(&vertices, &triangles, plane, 0.5, 1.0e-5).unwrap();
            let loops = stitch_section_loops(&segments, 1.0e-5).unwrap();
            assert_eq!(loop_bounds(&loops[0].points), expected, "plane {plane:?}");
        }
    }

    #[test]
    fn given_hollow_mesh_section_when_classified_then_preserves_hole_and_parent() {
        let (mut vertices, mut triangles) = cube([0.0; 3]);
        let (inner_vertices, inner_triangles) = cube([0.0; 3]);
        let base = vertices.len() as u32;
        vertices.extend(
            inner_vertices
                .into_iter()
                .map(|point| [0.6 + point[0] * 0.4, 0.6 + point[1] * 0.4, point[2]]),
        );
        triangles.extend(
            inner_triangles
                .into_iter()
                .map(|triangle| triangle.map(|index| index + base)),
        );
        let segments =
            intersect_mesh_plane(&vertices, &triangles, ProfilePlane::Xy, 0.0, 1.0e-5).unwrap();
        let loops = stitch_section_loops(&segments, 1.0e-5).unwrap();
        assert_eq!(loops.len(), 2);
        assert_eq!(loops[0].role, ProfileLoopRole::Outer);
        assert_eq!(loops[0].parent_index, None);
        assert_eq!(loops[1].role, ProfileLoopRole::Hole);
        assert_eq!(loops[1].parent_index, Some(0));
    }

    #[test]
    fn given_mesh_section_endpoint_drift_within_tolerance_when_stitched_then_closes_loop() {
        let segments = vec![
            [[0.0, 0.0], [1.0, 0.0]],
            [[1.0 + 5.0e-5, 0.0], [1.0, 1.0]],
            [[1.0, 1.0 + 5.0e-5], [0.0, 1.0]],
            [[0.0, 1.0], [0.0, 0.0]],
        ];
        let loops = stitch_section_loops(&segments, 1.0e-4).unwrap();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].points.len(), 4);
    }

    #[test]
    fn given_plane_on_coplanar_boundary_when_sectioned_then_reports_ambiguity() {
        let (vertices, triangles) = cube([0.0; 3]);
        let error =
            intersect_mesh_plane(&vertices, &triangles, ProfilePlane::Xy, 1.0, 1.0e-5).unwrap_err();
        assert!(
            error.message.contains("coincides with triangle")
                || error.message.contains("coplanar topology")
        );
    }

    #[test]
    fn given_nested_and_disjoint_material_regions_when_built_then_keeps_all_regions_and_island() {
        let loops = vec![
            ProfileLoop {
                points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
                role: ProfileLoopRole::Outer,
                parent_index: None,
            },
            ProfileLoop {
                points: vec![[2.0, 2.0], [2.0, 8.0], [8.0, 8.0], [8.0, 2.0]],
                role: ProfileLoopRole::Hole,
                parent_index: Some(0),
            },
            ProfileLoop {
                points: vec![[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]],
                role: ProfileLoopRole::Outer,
                parent_index: Some(1),
            },
            ProfileLoop {
                points: vec![[20.0, 0.0], [22.0, 0.0], [22.0, 2.0], [20.0, 2.0]],
                role: ProfileLoopRole::Outer,
                parent_index: None,
            },
        ];
        let expression = profile_expression(&loops).unwrap();
        assert!(expression.starts_with("(union "));
        assert_eq!(expression.matches("(polygon ").count(), 4);
        assert!(expression.contains(":holes"));
    }

    #[test]
    fn given_profile_with_hole_when_source_built_then_emits_named_part_and_hole_profile() {
        let loops = vec![
            ProfileLoop {
                points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 8.0], [0.0, 8.0]],
                role: ProfileLoopRole::Outer,
                parent_index: None,
            },
            ProfileLoop {
                points: vec![[2.0, 2.0], [2.0, 4.0], [4.0, 4.0], [4.0, 2.0]],
                role: ProfileLoopRole::Hole,
                parent_index: Some(0),
            },
        ];
        let source = build_profile_feature_source(
            "(model (part body (box 1 1 1)))",
            Some(&loops),
            ProfilePlane::Xz,
            3.0,
            0.0,
            4.0,
            "profile_feature",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(source.contains("(part profile_feature"));
        assert!(source.contains(":holes"));
        assert!(source.contains("profile_feature_depth"));
        assert!(source.contains("(rotate 90 0 0"));
    }

    #[test]
    fn given_exact_profile_asset_when_source_built_then_keeps_asset_reference_and_native_op() {
        let reference = ProfileArtifactReference {
            source_artifact_path: "/current/source.step".into(),
            source_artifact_digest: "sha256:source".into(),
            operation: ProfileDerivationOperation::Section {
                plane: ProfilePlane::Xy,
                offset_mm: 3.0,
            },
            tolerance_mm: 0.01,
            accuracy: ProfileEvidenceAccuracy::ExactBrep,
            profile_asset: Some(ProfileAsset {
                artifact_path: "/app/profile-assets/aabb.step".into(),
                artifact_digest: "sha256:aabb".into(),
            }),
        };
        let source = build_profile_feature_source(
            "(model (part body (box 1 1 1)))",
            None,
            ProfilePlane::Xy,
            3.0,
            0.5,
            8.0,
            "exact_profile",
            Some(&reference),
            Some("base-version"),
            None,
            None,
        )
        .unwrap();
        assert!(source.contains("(import-profile \"/app/profile-assets/aabb.step\")"));
        assert!(source.contains("\"accuracy\":\"exactBrep\""));
        let program = crate::ecky_scheme::compile_to_core_program(&source).unwrap();
        crate::ecky_cad_host::direct_occt::plan_core_program(&program)
            .expect("generated exact feature uses the native Direct OCCT plan");
    }

    #[tokio::test]
    async fn given_exact_annulus_section_when_applied_then_native_render_exports_hole_and_clearance(
    ) {
        struct Resolver(std::path::PathBuf);
        impl PathResolver for Resolver {
            fn app_config_dir(&self) -> std::path::PathBuf {
                self.0.clone()
            }
            fn app_data_dir(&self) -> std::path::PathBuf {
                self.0.clone()
            }
            fn resource_path(&self, _path: &str) -> Option<std::path::PathBuf> {
                None
            }
        }
        let root =
            std::env::temp_dir().join(format!("ecky-profile-native-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = Resolver(root.clone());
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_none()
        {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let layout = crate::ecky_cad_host::direct_occt_sdk::DirectOcctSdkLayout {
            runtime_root: root.join("runtime/occt"),
            dylib_dir: None,
            include_dir: None,
            missing_headers: Vec::new(),
            missing_libs: Vec::new(),
            install_name_prefix: "@rpath",
        };
        let base_source = "(model (part ring (difference (cylinder 12 8) (cylinder 4 8))))";
        let base_program = crate::ecky_scheme::compile_to_core_program(base_source).unwrap();
        let source_dir = root.join("source");
        let source_bundle = crate::ecky_cad_host::direct_occt_executor::export_core_program_step_stl_with_params_runner_first(
            &base_program,
            &crate::contracts::DesignParams::new(),
            &layout,
            &source_dir,
            &resolver,
        )
        .unwrap()
        ;
        let (source_step, source_stl) = match source_bundle {
            crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
                step_path,
                stl_path,
                ..
            } => (step_path, stl_path),
            other => panic!("expected exact source STEP export, got {other:?}"),
        };

        let conn = crate::db::init_db(&root.join("history.sqlite")).expect("history db");
        let state = AppState::new(
            crate::contracts::Config {
                cairn: None,
                engines: Vec::new(),
                selected_engine_id: String::new(),
                freecad_cmd: String::new(),
                cad_text_font_path: String::new(),
                freecad_library_roots: Vec::new(),
                assets: Vec::new(),
                microwave: None,
                voice: crate::contracts::VoiceConfig::default(),
                mcp: crate::contracts::McpConfig::default(),
                fem_compute: crate::contracts::FemComputeConfig::default(),
                has_seen_onboarding: true,
                connection_type: None,
                provider_models: crate::contracts::ProviderModels::default(),
                jev_classifier: Default::default(),
                default_engine_kind: crate::contracts::EngineKind::EckyIrV0,
                default_source_language: SourceLanguage::EckyIrV0,
                default_geometry_backend: crate::contracts::GeometryBackend::EckyRust,
                max_generation_attempts: 3,
                max_verify_attempts: 0,
                projects_root: None,
            },
            None,
            conn,
        );
        let base_bundle = crate::contracts::ArtifactBundle {
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "profile-base".into(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            content_hash: artifact_digest(&source_step.to_string_lossy()).unwrap(),
            artifact_version: 1,
            fcstd_path: root.join("base.fcstd").to_string_lossy().into_owned(),
            manifest_path: root
                .join("base.manifest.json")
                .to_string_lossy()
                .into_owned(),
            macro_path: None,
            model_stl_path: source_stl.to_string_lossy().into_owned(),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: vec![crate::contracts::ExportArtifact {
                label: "Exact source model".into(),
                format: "step".into(),
                path: source_step.to_string_lossy().into_owned(),
                role: "model".into(),
                geometry_provenance: None,
            }],
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
        };
        let base_message_id = crate::services::design::add_manual_version(
            crate::services::design::AddManualVersionRequest {
                thread_id: "profile-apply".into(),
                title: "Ring".into(),
                version_name: "V1".into(),
                macro_code: base_source.into(),
                source_language: Some(SourceLanguage::EckyIrV0),
                geometry_backend: Some(crate::contracts::GeometryBackend::EckyRust),
                parameters: DesignParams::new(),
                ui_spec: crate::contracts::UiSpec { fields: Vec::new() },
                post_processing: None,
                artifact_bundle: Some(base_bundle),
                model_manifest: None,
                response_text: None,
                agent_origin: None,
                status: None,
                error_message: None,
            },
            &state,
            &resolver,
        )
        .await
        .expect("persisted base model with exact STEP artifact");

        let derived = derive_profile_geometry(
            ProfileGeometryRequest {
                artifact_path: source_step.to_string_lossy().into_owned(),
                operation: ProfileDerivationOperation::Section {
                    plane: ProfilePlane::Xy,
                    offset_mm: 3.0,
                },
                tolerance_mm: 0.01,
            },
            &resolver,
        )
        .expect("native exact section derives closed annular profile");
        assert_eq!(derived.accuracy, ProfileEvidenceAccuracy::ExactBrep);
        assert_eq!(
            derived.loops.len(),
            2,
            "native section retains hole boundary"
        );
        assert_eq!(derived.loops[0].role, ProfileLoopRole::Outer);
        assert_eq!(derived.loops[1].role, ProfileLoopRole::Hole);
        assert!(derived
            .profile_asset
            .as_ref()
            .is_some_and(|asset| { Path::new(&asset.artifact_path).is_file() }));
        let reference = ProfileArtifactReference {
            source_artifact_path: derived.source_artifact_path,
            source_artifact_digest: derived.source_artifact_digest,
            operation: derived.operation,
            tolerance_mm: derived.tolerance_mm,
            accuracy: derived.accuracy,
            profile_asset: derived.profile_asset,
        };
        let applied = apply_profile_feature(
            ProfileFeatureApplyRequest {
                thread_id: "profile-apply".into(),
                base_message_id: Some(base_message_id),
                expected_source: base_source.into(),
                loops: None,
                plane: ProfilePlane::Xy,
                offset_mm: 3.0,
                clearance_mm: 1.0,
                depth_mm: 5.0,
                part_name: "profile_rib".into(),
                reference: Some(reference),
                drawing_profile: None,
            },
            &state,
            &resolver,
        )
        .await
        .expect("real apply path resolves generated profile parameters and renders");
        assert_eq!(
            applied.draft.status,
            crate::contracts::MessageStatus::Success
        );
        assert!(applied.draft.message_id.is_some());
        assert_eq!(
            applied
                .draft
                .design_output
                .initial_params
                .get("profile_rib_clearance"),
            Some(&crate::contracts::ParamValue::Number(1.0))
        );
        assert_eq!(
            applied
                .draft
                .design_output
                .initial_params
                .get("profile_rib_depth"),
            Some(&crate::contracts::ParamValue::Number(5.0))
        );
        let source = applied.source;
        let program = crate::ecky_scheme::compile_to_core_program(&source).unwrap();
        let params = applied.draft.design_output.initial_params;
        crate::ecky_cad_host::direct_occt::plan_core_program(&program).unwrap();
        let output_dir = root.join("profile-version");
        let output = crate::ecky_cad_host::direct_occt_executor::export_core_program_step_stl_with_params_runner_first(
            &program,
            &params,
            &layout,
            &output_dir,
            &resolver,
        )
        .unwrap()
        ;
        let (step_path, output_stl_path) = match output {
            crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
                step_path,
                stl_path,
                ..
            } => (step_path, stl_path),
            other => panic!("expected exact profile STEP export, got {other:?}"),
        };
        let step = std::fs::read_to_string(step_path).unwrap();
        assert!(step.contains("CIRCLE") || step.contains("B_SPLINE_CURVE"));
        assert!(step.contains("ADVANCED_FACE"));
        assert!(step.contains("FACE_BOUND"));
        assert!(Path::new(&output_stl_path).is_file());

        let photo_path = root.join("photo-plate.png");
        crate::services::drawing_import::tests::plate_fixture(&photo_path, false, true);
        let prepared = crate::services::drawing_import::prepare_drawing_image(
            crate::services::drawing_import::PrepareDrawingImageRequest {
                source_path: photo_path.to_string_lossy().into_owned(),
            },
            &resolver,
        )
        .expect("prepare raster plate");
        let recognized = crate::services::drawing_import::recognize_drawing_profile(
            crate::services::drawing_import::RecognizeDrawingProfileRequest {
                image: prepared.image,
                view: crate::contracts::SketchView::Top,
                region: crate::services::drawing_import::DrawingRegion {
                    left_px: 80.0,
                    top_px: 70.0,
                    width_px: 480.0,
                    height_px: 310.0,
                    rotation_degrees: 0.0,
                },
                rectification: None,
                calibration: crate::services::drawing_import::DrawingScaleCalibration {
                    start_px: [120.0, 100.0],
                    end_px: [520.0, 100.0],
                    distance_mm: 80.0,
                },
                excluded_regions: Vec::new(),
                tolerance_mm: 0.5,
            },
            &resolver,
        )
        .expect("recognize raster plate");
        let accepted_ids = recognized
            .profiles
            .iter()
            .filter(|profile| profile.closed && profile.issues.is_empty())
            .flat_map(|profile| profile.primitive_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let accepted_primitives = recognized
            .primitives
            .into_iter()
            .filter(|primitive| accepted_ids.contains(super::drawing_primitive_id(primitive)))
            .collect::<Vec<_>>();
        let photo_hole_radius = accepted_primitives
            .iter()
            .find_map(|primitive| match primitive {
                crate::services::drawing_import::DrawingPrimitive::Circle { radius_mm, .. } => {
                    Some(*radius_mm)
                }
                _ => None,
            })
            .expect("recognized analytic photo hole");
        assert!(accepted_primitives.iter().any(|primitive| matches!(
            primitive,
            crate::services::drawing_import::DrawingPrimitive::Circle { radius_mm, .. }
                if (*radius_mm - 5.0).abs() < 0.5
        )));
        let photo_applied = apply_profile_feature(
            ProfileFeatureApplyRequest {
                thread_id: "profile-apply".into(),
                base_message_id: applied.draft.message_id.clone(),
                expected_source: source.clone(),
                loops: None,
                plane: ProfilePlane::Xy,
                offset_mm: 0.0,
                clearance_mm: 0.0,
                depth_mm: 3.0,
                part_name: "photo_plate".into(),
                reference: None,
                drawing_profile: Some(crate::services::drawing_import::DrawingProfileApplyInput {
                    evidence_id: recognized.evidence_id,
                    primitives: accepted_primitives,
                }),
            },
            &state,
            &resolver,
        )
        .await
        .expect("guarded image-derived profile apply");
        assert_eq!(
            photo_applied.draft.status,
            crate::contracts::MessageStatus::Success
        );
        assert!(
            photo_applied.draft.message_id.is_some(),
            "image-derived apply must persist a normal version"
        );
        assert!(photo_applied
            .source
            .contains("\"accuracy\":\"imageDerived\""));
        let photo_step = photo_applied
            .draft
            .artifact_bundle
            .as_ref()
            .and_then(|bundle| {
                bundle
                    .export_artifacts
                    .iter()
                    .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
            })
            .map(|artifact| Path::new(&artifact.path))
            .expect("native STEP export for applied drawing profile");
        let photo_step_text =
            std::fs::read_to_string(photo_step).expect("read native drawing STEP");
        assert!(
            photo_step_text.contains("CIRCLE"),
            "analytic circle missing from STEP export"
        );
        assert!(photo_step_text.contains("ADVANCED_FACE"));
        let circle_radii = |step: &str| {
            step.lines()
                .filter_map(|line| {
                    let body = line
                        .split_once("CIRCLE(")?
                        .1
                        .trim_end_matches(';')
                        .trim_end_matches(')');
                    body.rsplit_once(',')?.1.trim().parse::<f64>().ok()
                })
                .collect::<Vec<_>>()
        };
        let baseline_radii = circle_radii(&step);
        let photo_radii = circle_radii(&photo_step_text);
        assert!(
            photo_radii.len() > baseline_radii.len(),
            "photo extrusion must add native circular edges: {baseline_radii:?} -> {photo_radii:?}"
        );
        assert!(
            photo_radii
                .iter()
                .any(|radius| (*radius - photo_hole_radius).abs() < 1.0e-6),
            "native STEP lacks the fitted {:.6} mm hole: {photo_radii:?}",
            photo_hole_radius
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn given_front_top_and_side_projection_when_applied_then_preserves_view_axes_and_normal()
    {
        struct Resolver(PathBuf);
        impl PathResolver for Resolver {
            fn app_config_dir(&self) -> PathBuf {
                self.0.clone()
            }
            fn app_data_dir(&self) -> PathBuf {
                self.0.clone()
            }
            fn resource_path(&self, _path: &str) -> Option<PathBuf> {
                None
            }
        }

        let root =
            std::env::temp_dir().join(format!("ecky-profile-view-apply-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = Resolver(root.clone());
        if crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
            &resolver, true,
        )
        .is_none()
        {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let layout = crate::ecky_cad_host::direct_occt_sdk::DirectOcctSdkLayout {
            runtime_root: root.join("runtime/occt"),
            dylib_dir: None,
            include_dir: None,
            missing_headers: Vec::new(),
            missing_libs: Vec::new(),
            install_name_prefix: "@rpath",
        };
        let base_source = "(model (part body (translate 10 20 6 (difference (box 17 13 21) (translate 6 5 0 (cylinder 2 21))))))";
        let base_program = crate::ecky_scheme::compile_to_core_program(base_source).unwrap();
        let source_dir = root.join("source");
        let source_bundle = crate::ecky_cad_host::direct_occt_executor::export_core_program_step_stl_with_params_runner_first(
            &base_program,
            &DesignParams::new(),
            &layout,
            &source_dir,
            &resolver,
        )
        .unwrap();
        let (source_step, source_stl) = match source_bundle {
            crate::ecky_cad_host::direct_occt_sdk::NativeExportOutcome::Exported {
                step_path,
                stl_path,
                ..
            } => (step_path, stl_path),
            other => panic!("expected exact source STEP export, got {other:?}"),
        };

        let conn = crate::db::init_db(&root.join("history.sqlite")).expect("history db");
        let state = AppState::new(
            crate::contracts::Config {
                cairn: None,
                engines: Vec::new(),
                selected_engine_id: String::new(),
                freecad_cmd: String::new(),
                cad_text_font_path: String::new(),
                freecad_library_roots: Vec::new(),
                assets: Vec::new(),
                microwave: None,
                voice: crate::contracts::VoiceConfig::default(),
                mcp: crate::contracts::McpConfig::default(),
                fem_compute: crate::contracts::FemComputeConfig::default(),
                has_seen_onboarding: true,
                connection_type: None,
                provider_models: crate::contracts::ProviderModels::default(),
                jev_classifier: Default::default(),
                default_engine_kind: crate::contracts::EngineKind::EckyIrV0,
                default_source_language: SourceLanguage::EckyIrV0,
                default_geometry_backend: crate::contracts::GeometryBackend::EckyRust,
                max_generation_attempts: 3,
                max_verify_attempts: 0,
                projects_root: None,
            },
            None,
            conn,
        );
        let bundle = crate::contracts::ArtifactBundle {
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "asymmetric-profile-base".into(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            content_hash: artifact_digest(&source_step.to_string_lossy()).unwrap(),
            artifact_version: 1,
            fcstd_path: root.join("base.fcstd").to_string_lossy().into_owned(),
            manifest_path: root
                .join("base.manifest.json")
                .to_string_lossy()
                .into_owned(),
            macro_path: None,
            model_stl_path: source_stl.to_string_lossy().into_owned(),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: vec![crate::contracts::ExportArtifact {
                label: "Exact source model".into(),
                format: "step".into(),
                path: source_step.to_string_lossy().into_owned(),
                role: "model".into(),
                geometry_provenance: None,
            }],
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
        };

        let cases = [
            (
                ProfileView::Front,
                ProfilePlane::Xz,
                [1.5, 18.5, -5.0, 26.5, 6.0, 27.0],
            ),
            (
                ProfileView::Top,
                ProfilePlane::Xy,
                [1.5, 18.5, 13.5, 26.5, 0.0, 27.0],
            ),
            (
                ProfileView::Side,
                ProfilePlane::Yz,
                [0.0, 18.5, 13.5, 26.5, 6.0, 27.0],
            ),
        ];
        for (view, plane, expected_bounds) in cases {
            let thread_id = format!("profile-view-{}", view_name(view));
            let base_message_id = crate::services::design::add_manual_version(
                crate::services::design::AddManualVersionRequest {
                    thread_id: thread_id.clone(),
                    title: "Asymmetric block".into(),
                    version_name: "V1".into(),
                    macro_code: base_source.into(),
                    source_language: Some(SourceLanguage::EckyIrV0),
                    geometry_backend: Some(crate::contracts::GeometryBackend::EckyRust),
                    parameters: DesignParams::new(),
                    ui_spec: crate::contracts::UiSpec { fields: Vec::new() },
                    post_processing: None,
                    artifact_bundle: Some(bundle.clone()),
                    model_manifest: None,
                    response_text: None,
                    agent_origin: None,
                    status: None,
                    error_message: None,
                },
                &state,
                &resolver,
            )
            .await
            .expect("persisted base view version");
            let drawing = project_profile_drawing(
                ProjectProfileDrawingRequest {
                    artifact_path: source_step.to_string_lossy().into_owned(),
                    view,
                    tolerance_mm: 0.01,
                },
                &resolver,
            )
            .expect("exact visible drawing projection succeeds independently of profile closure");
            assert!(Path::new(&drawing.projection_asset.artifact_path).is_file());
            assert_eq!(
                artifact_digest(&drawing.projection_asset.artifact_path).unwrap(),
                drawing.projection_asset.artifact_digest
            );
            assert!(!drawing.display_edges.is_empty());
            assert!(drawing.display_edges.iter().all(|edge| edge.len() >= 2));

            let derived = derive_profile_geometry(
                ProfileGeometryRequest {
                    artifact_path: source_step.to_string_lossy().into_owned(),
                    operation: ProfileDerivationOperation::Projection { view },
                    tolerance_mm: 0.01,
                },
                &resolver,
            )
            .expect("exact view profile derives");
            let reference = ProfileArtifactReference {
                source_artifact_path: derived.source_artifact_path,
                source_artifact_digest: derived.source_artifact_digest,
                operation: derived.operation,
                tolerance_mm: derived.tolerance_mm,
                accuracy: derived.accuracy,
                profile_asset: derived.profile_asset,
            };
            let applied = apply_profile_feature(
                ProfileFeatureApplyRequest {
                    thread_id,
                    base_message_id: Some(base_message_id),
                    expected_source: base_source.into(),
                    loops: None,
                    plane,
                    offset_mm: 0.0,
                    clearance_mm: 0.0,
                    depth_mm: 5.0,
                    part_name: "view_profile".into(),
                    reference: Some(reference),
                    drawing_profile: None,
                },
                &state,
                &resolver,
            )
            .await
            .unwrap_or_else(|error| panic!("{} view apply failed: {error}", view_name(view)));
            assert_eq!(
                applied.draft.status,
                crate::contracts::MessageStatus::Success,
                "{} view render failed: {:?}",
                view_name(view),
                applied.draft.error
            );
            let stl = applied
                .draft
                .artifact_bundle
                .as_ref()
                .expect("rendered profile artifact")
                .model_stl_path
                .as_str();
            let mesh = crate::ecky_ir::mesh_asset::IndexedMeshAsset::from_imported_file(
                crate::ecky_ir::mesh_asset::MeshAssetSource::Imported,
                Path::new(stl),
            )
            .expect("rendered STL bounds");
            let actual_bounds = mesh.vertices().iter().fold(
                [
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                ],
                |mut bounds, vertex| {
                    for axis in 0..3 {
                        bounds[axis * 2] = bounds[axis * 2].min(vertex[axis]);
                        bounds[axis * 2 + 1] = bounds[axis * 2 + 1].max(vertex[axis]);
                    }
                    bounds
                },
            );
            for (actual, expected) in actual_bounds.iter().zip(expected_bounds) {
                assert!(
                    (actual - expected).abs() < 0.3,
                    "{} projection placed on wrong axes/direction: expected {expected_bounds:?}, got {actual_bounds:?}",
                    view_name(view)
                );
            }
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn given_reordered_nested_loops_when_validated_then_uses_declared_parent_independent_of_order()
    {
        let loops = vec![
            ProfileLoop {
                points: vec![[2.0, 2.0], [2.0, 4.0], [4.0, 4.0], [4.0, 2.0]],
                role: ProfileLoopRole::Hole,
                parent_index: Some(1),
            },
            ProfileLoop {
                points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
                role: ProfileLoopRole::Outer,
                parent_index: None,
            },
        ];
        assert_eq!(validate_profile_loops(&loops).unwrap().len(), 2);
        let wrong_parent = vec![
            loops[1].clone(),
            ProfileLoop {
                parent_index: None,
                ..loops[0].clone()
            },
        ];
        assert!(validate_profile_loops(&wrong_parent).is_err());
    }

    #[test]
    fn given_fcstd_source_when_exact_profile_is_requested_then_requires_step_export_without_fallback(
    ) {
        struct Resolver(std::path::PathBuf);
        impl PathResolver for Resolver {
            fn app_config_dir(&self) -> std::path::PathBuf {
                self.0.clone()
            }
            fn app_data_dir(&self) -> std::path::PathBuf {
                self.0.clone()
            }
            fn resource_path(&self, _path: &str) -> Option<std::path::PathBuf> {
                None
            }
        }
        let root =
            std::env::temp_dir().join(format!("ecky-profile-fcstd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("model.FCStd");
        std::fs::write(&source, b"not consumed").unwrap();
        let error = derive_profile_geometry(
            ProfileGeometryRequest {
                artifact_path: source.to_string_lossy().into_owned(),
                operation: ProfileDerivationOperation::Section {
                    plane: ProfilePlane::Xy,
                    offset_mm: 0.0,
                },
                tolerance_mm: 0.01,
            },
            &Resolver(root.clone()),
        )
        .unwrap_err();
        assert!(error.message.contains("Export FCStd as STEP"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn given_same_profile_asset_from_two_sources_when_evidence_is_keyed_then_keeps_both_lineages() {
        let first = ProfileAssetEvidence {
            profile_asset_digest: "sha256:asset".into(),
            source_artifact_path: "/model/one.step".into(),
            source_artifact_digest: "sha256:one".into(),
            operation: ProfileDerivationOperation::Section {
                plane: ProfilePlane::Xy,
                offset_mm: 0.0,
            },
            tolerance_mm: 0.01,
        };
        let second = ProfileAssetEvidence {
            source_artifact_path: "/model/two.step".into(),
            source_artifact_digest: "sha256:two".into(),
            ..first.clone()
        };
        let root = Path::new("/app/profile-assets");
        assert_ne!(
            profile_asset_evidence_path(root, &first).unwrap(),
            profile_asset_evidence_path(root, &second).unwrap()
        );
    }

    #[test]
    fn given_profile_operation_tauri_json_when_deserialized_then_uses_camel_case_fields() {
        let section: ProfileDerivationOperation =
            serde_json::from_str(r#"{"kind":"section","plane":"xy","offsetMm":2.5}"#).unwrap();
        assert_eq!(
            section,
            ProfileDerivationOperation::Section {
                plane: ProfilePlane::Xy,
                offset_mm: 2.5,
            }
        );
        let section_json = serde_json::to_value(section).unwrap();
        assert_eq!(section_json["offsetMm"], 2.5);
        assert!(section_json.get("offset_mm").is_none());

        let projection: ProfileDerivationOperation =
            serde_json::from_str(r#"{"kind":"projection","view":"front"}"#).unwrap();
        assert_eq!(
            projection,
            ProfileDerivationOperation::Projection {
                view: ProfileView::Front,
            }
        );
        assert_eq!(
            serde_json::to_value(projection).unwrap()["kind"],
            "projection"
        );
    }

    #[test]
    fn given_profile_geometry_response_when_serialized_then_exposes_camel_case_contract_fields() {
        let response = ProfileGeometryResponse {
            source_artifact_path: "/bundle/model.step".into(),
            source_artifact_digest: "sha256:source".into(),
            operation: ProfileDerivationOperation::Section {
                plane: ProfilePlane::Xy,
                offset_mm: 2.5,
            },
            tolerance_mm: 0.01,
            units: "mm".into(),
            accuracy: ProfileEvidenceAccuracy::ExactBrep,
            profile_asset: Some(ProfileAsset {
                artifact_path: "/app/profile-assets/abc.step".into(),
                artifact_digest: "sha256:abc".into(),
            }),
            display_accuracy: "approximateDisplayOnly".into(),
            loops: vec![ProfileLoop {
                points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                role: ProfileLoopRole::Outer,
                parent_index: None,
            }],
            warnings: Vec::new(),
        };
        let value = serde_json::to_value(response).unwrap();
        assert!(value.get("sourceArtifactPath").is_some());
        assert!(value.get("source_artifact_path").is_none());
        assert_eq!(value["operation"]["offsetMm"], 2.5);
        assert!(value["loops"][0].get("parentIndex").is_some());
        assert!(value["loops"][0].get("parent_index").is_none());
        assert_eq!(value["accuracy"], "exactBrep");
    }

    #[test]
    fn given_profile_with_hole_clearance_and_offset_when_rendered_then_produces_mesh() {
        struct Resolver(std::path::PathBuf);
        impl PathResolver for Resolver {
            fn app_config_dir(&self) -> std::path::PathBuf {
                self.0.clone()
            }
            fn app_data_dir(&self) -> std::path::PathBuf {
                self.0.clone()
            }
            fn resource_path(&self, _path: &str) -> Option<std::path::PathBuf> {
                None
            }
        }
        let loops = vec![
            ProfileLoop {
                points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 8.0], [0.0, 8.0]],
                role: ProfileLoopRole::Outer,
                parent_index: None,
            },
            ProfileLoop {
                points: vec![[2.0, 2.0], [2.0, 4.0], [4.0, 4.0], [4.0, 2.0]],
                role: ProfileLoopRole::Hole,
                parent_index: Some(0),
            },
        ];
        let source = build_profile_feature_source(
            "(model (part body (box 1 1 1)))",
            Some(&loops),
            ProfilePlane::Xy,
            2.0,
            0.2,
            4.0,
            "profile_feature",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        crate::ecky_scheme::compile_to_core_program(&source).expect("generated source compiles");
        let root =
            std::env::temp_dir().join(format!("ecky-profile-render-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let bundle =
            crate::ecky_ir::render_model(&source, &DesignParams::new(), &Resolver(root.clone()))
                .unwrap_or_else(|error| {
                    panic!("generated profile renders; source:\n{source}\n{error}")
                });
        assert!(Path::new(&bundle.model_stl_path).is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn given_self_intersecting_profile_when_applied_then_rejects_without_source() {
        let loops = vec![ProfileLoop {
            points: vec![[0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]],
            role: ProfileLoopRole::Outer,
            parent_index: None,
        }];
        let error = validate_profile_loops(&loops).unwrap_err();
        assert!(error.message.contains("self-intersects"));
    }
}
