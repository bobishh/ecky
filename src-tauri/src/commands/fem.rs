use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use ecky_fem::{
    advance_simp_state_traced_checkpointed, finalize_simp_state, format_gcmma_attempt_trace,
    initialize_simp_state, reconstruct_density_surface, FemApplicabilityCheckKind,
    FemApplicabilityStatus, FemBudgetLimits, FemConstraint, FemDensityAnchor,
    FemDensitySurfaceControls, FemEngineeringEvidenceLedger, FemEvidenceAuthority,
    FemEvidenceSubject, FemIdealizationArtifact, FemIdealizationKind, FemLoad, FemMaterial,
    FemResultExtremum, FemResultFieldKind, FemSafetyFactor, FemStudyAssumptionCategory,
    FemStudyAssumptionStatus, FemTopologyControls, FemTopologyLoadCase, FemTopologyTermination,
    FemValidationEvidenceKind, FEM_SCHEMA_VERSION,
};

use crate::contracts::{
    AppError, AppResult, FemAcceptanceEvaluationDto, FemAcceptanceEvidenceChainDto,
    FemApplicabilityCheckDto, FemAssumptionDto, FemCancelResponse, FemComputeConfig,
    FemConvergenceIntentInput, FemConvergenceLevelDto, FemConvergenceRequest,
    FemConvergenceResponse, FemEngineeringEvidenceDto, FemEngineeringQuestionDto, FemExtremumDto,
    FemIdealizationDto, FemInputEvidenceDto, FemMeshPreviewIntentResponse, FemMeshPreviewResponse,
    FemResultArrayDto, FemResultReadRequest, FemResultReadResponse, FemResultSummaryDto,
    FemRunIntentInput, FemRunIntentResponse, FemRunResponse, FemSensitivityEvidenceDto,
    FemSensitivityMetricDto, FemStudyRequest, FemStudyValidationResponse, FemSupportReactionDto,
    FemTopologyControlsDto, FemTopologyMaterialDto, FemTopologyReconstructRequest,
    FemTopologyReconstructResponse, FemTopologyRunRequest, FemTopologyRunResponse,
    FemTopologySurfaceLoadDto, FemValidationEvidenceDto, FemVerificationLayerDto,
    FemVtuExportIntentInput, FemVtuExportResponse,
};
use crate::ecky_cad_host::analysis_boundary::{
    load_direct_occt_analysis_boundary_surface, AnalysisBoundarySurface,
};
use crate::fem_engineering::{
    authored_study_from_core, engineering_ledger_from_core, resolve_fem_face_tags,
    FemAuthoredTopologyControls,
};
use crate::gmsh_mesher::{probe_gmsh_runtime, sha256_file, ExactBrepMesherRuntime};
use crate::models::{AppState, PathResolver};
use crate::netgen_mesher::probe_default_netgen_runtime;
use crate::services::fem::{
    execute_fem_mesh_pipeline, execute_fem_pipeline_with_mesh_size, FemPipelineControl,
    FemPipelineStage, FemProgressEvent,
};
use crate::services::fem_artifacts::{
    decode_fem_topology_mesh, export_fem_result_vtu as write_fem_result_vtu, load_fem_mesh_asset,
    load_fem_result_asset, publish_fem_mesh_asset, publish_fem_result_asset, FemMeshAsset,
    FemResultAsset, FemScalarType,
};
use crate::services::fem_topology_artifacts::{
    load_fem_topology_artifact, load_fem_topology_state, publish_fem_topology_artifact,
    publish_fem_topology_state_checkpoint,
};

const FEM_REQUEST_CACHE_ROOT: &str = "fem-request-cache-v1";
const FEM_MESH_REQUEST_CACHE_ROOT: &str = "fem-mesh-request-cache-v1";
const FEM_CONVERGENCE_CACHE_ROOT: &str = "fem-convergence-v1";
const FEM_REQUEST_CACHE_SCHEMA_VERSION: u32 = 2;
const FEM_SINGLEFLIGHT_LIMIT: usize = 256;
const FEM_REQUEST_CACHE_ENTRY_LIMIT: usize = 128;
const FEM_REQUEST_CACHE_BYTE_LIMIT: u64 = 2 * 1024 * 1024;
static FEM_REQUEST_CACHE_NONCE: AtomicU64 = AtomicU64::new(1);
static FEM_RUN_SINGLEFLIGHT: OnceLock<StdMutex<HashMap<String, Arc<FemSharedJob>>>> =
    OnceLock::new();

fn probe_system_exact_brep_mesher_runtime() -> AppResult<ExactBrepMesherRuntime> {
    let executable = std::env::var_os("ECKY_GMSH_EXECUTABLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("gmsh"));
    Ok(ExactBrepMesherRuntime {
        gmsh: probe_gmsh_runtime(Path::new(&executable))?,
        netgen: probe_default_netgen_runtime().ok(),
    })
}

struct FemSharedJob {
    gate: StdMutex<()>,
    execution_cancelled: AtomicBool,
    subscribers: StdMutex<HashMap<u64, Arc<AtomicBool>>>,
}

struct FemSharedSubscription {
    job: Arc<FemSharedJob>,
    subscriber_id: u64,
}

struct FemCancellationMonitor {
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FemSharedJob {
    fn new() -> Self {
        Self {
            gate: StdMutex::new(()),
            execution_cancelled: AtomicBool::new(false),
            subscribers: StdMutex::new(HashMap::new()),
        }
    }

    fn subscribe(
        self: &Arc<Self>,
        cancellation: Arc<AtomicBool>,
    ) -> AppResult<FemSharedSubscription> {
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| AppError::internal("FEM subscriber registry was poisoned."))?;
        if subscribers.is_empty() {
            self.execution_cancelled.store(false, Ordering::Release);
        }
        let subscriber_id = FEM_REQUEST_CACHE_NONCE.fetch_add(1, Ordering::Relaxed);
        subscribers.insert(subscriber_id, cancellation);
        drop(subscribers);
        self.refresh_cancellation()?;
        Ok(FemSharedSubscription {
            job: self.clone(),
            subscriber_id,
        })
    }

    fn refresh_cancellation(&self) -> AppResult<()> {
        let subscribers = self
            .subscribers
            .lock()
            .map_err(|_| AppError::internal("FEM subscriber registry was poisoned."))?;
        if !subscribers.is_empty()
            && subscribers
                .values()
                .all(|cancelled| cancelled.load(Ordering::Acquire))
        {
            self.execution_cancelled.store(true, Ordering::Release);
        }
        Ok(())
    }
}

impl FemSharedSubscription {
    fn start_monitor(&self) -> FemCancellationMonitor {
        let stopped = Arc::new(AtomicBool::new(false));
        let monitor_stopped = stopped.clone();
        let job = self.job.clone();
        let thread = std::thread::spawn(move || {
            while !monitor_stopped.load(Ordering::Acquire) {
                let _ = job.refresh_cancellation();
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = job.refresh_cancellation();
        });
        FemCancellationMonitor {
            stopped,
            thread: Some(thread),
        }
    }
}

impl Drop for FemSharedSubscription {
    fn drop(&mut self) {
        if let Ok(mut subscribers) = self.job.subscribers.lock() {
            subscribers.remove(&self.subscriber_id);
        }
        let _ = self.job.refresh_cancellation();
    }
}

impl Drop for FemCancellationMonitor {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FemRequestCacheEntry {
    schema_version: u32,
    request_digest: String,
    analysis_identity_digest: String,
    solution_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FemMeshRequestCacheEntry {
    schema_version: u32,
    request_digest: String,
    analysis_identity_digest: String,
    mesh_content_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FemConvergenceCacheEntry {
    schema_version: u32,
    request_digest: String,
    response: FemConvergenceResponse,
}

struct ResolvedFemRequest {
    program: ecky_render::core_ir::CoreProgram,
    boundary: AnalysisBoundarySurface,
    step_path: PathBuf,
    manifest: crate::contracts::ModelManifest,
    budgets: FemBudgetLimits,
    control: FemPipelineControl,
}

struct FemTopologyArtifactRunRequest {
    job_id: String,
    analysis_identity_digest: String,
    mesh_content_digest: String,
    material: FemTopologyMaterialDto,
    load_cases: Vec<FemTopologySurfaceLoadDto>,
    fixed_face_group_indices: Vec<u32>,
    passive_solid_regions: Vec<FemTopologyFaceRegion>,
    passive_void_regions: Vec<FemTopologyFaceRegion>,
    relative_solver_tolerance: f64,
    controls: crate::contracts::FemTopologyControlsDto,
    resume_state_digest: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FemTopologyRuntimePolicy {
    maximum_iterations: u64,
    maximum_new_iterations: u64,
    maximum_dimension: u64,
    maximum_elements: u64,
    maximum_working_memory_bytes: u64,
    maximum_result_bytes: u64,
    maximum_wall_time_ms: u64,
}

impl FemTopologyRuntimePolicy {
    pub(crate) fn from_compute(compute: &FemComputeConfig) -> Self {
        let maximum_iterations = compute.topology_iteration_limit();
        Self {
            maximum_iterations,
            maximum_new_iterations: maximum_iterations,
            maximum_dimension: compute.maximum_fem_dofs(),
            maximum_elements: compute.maximum_fem_elements(),
            maximum_working_memory_bytes: compute.maximum_working_memory_bytes(),
            maximum_result_bytes: compute.maximum_fem_result_bytes(),
            maximum_wall_time_ms: compute.maximum_wall_time_ms(),
        }
    }
}

pub(crate) fn apply_fem_compute_policy(compute: &FemComputeConfig, study: &mut FemStudyRequest) {
    let maximum_elements = compute.maximum_fem_elements();
    study.budgets.boundary_triangles = maximum_elements;
    study.budgets.tet4_cells = maximum_elements;
    study.budgets.nodes = compute.maximum_fem_nodes();
    study.budgets.dofs = compute.maximum_fem_dofs();
    study.budgets.sparse_nonzeros = compute.maximum_fem_sparse_nonzeros();
    study.budgets.result_bytes = compute.maximum_fem_result_bytes();
    study.budgets.convergence_levels = 3;
    study.control.maximum_runtime_ms = compute.maximum_wall_time_ms();
    study.control.thread_count = u32::from(compute.thread_count);
}

fn fem_study_request_from_intent(
    input: FemRunIntentInput,
    compute: &FemComputeConfig,
    job_id: String,
) -> FemStudyRequest {
    let maximum_elements = compute.maximum_fem_elements();
    FemStudyRequest {
        job_id,
        model_id: input.model_id,
        source: input.source,
        analysis_name: input.analysis_name,
        budgets: crate::contracts::FemBudgetLimitsDto {
            boundary_triangles: maximum_elements,
            tet4_cells: maximum_elements,
            nodes: compute.maximum_fem_nodes(),
            dofs: compute.maximum_fem_dofs(),
            sparse_nonzeros: compute.maximum_fem_sparse_nonzeros(),
            result_bytes: compute.maximum_fem_result_bytes(),
            convergence_levels: 3,
        },
        control: crate::contracts::FemPipelineControlDto {
            envelope_mm: 0.1,
            minimum_scaled_jacobian: 1.0e-6,
            maximum_runtime_ms: compute.maximum_wall_time_ms(),
            relative_solver_tolerance: 1.0e-8,
            thread_count: u32::from(compute.thread_count),
        },
    }
}

fn next_fem_run_job_id() -> String {
    format!("fem-run-{}", uuid::Uuid::new_v4())
}

fn next_fem_job_id(kind: &str) -> String {
    format!("fem-{kind}-{}", uuid::Uuid::new_v4())
}

fn fem_convergence_request_from_intent(
    input: FemConvergenceIntentInput,
    compute: &FemComputeConfig,
    job_id: String,
) -> FemConvergenceRequest {
    FemConvergenceRequest {
        study: fem_study_request_from_intent(
            FemRunIntentInput {
                model_id: input.model_id,
                source: input.source,
                analysis_name: input.analysis_name,
            },
            compute,
            job_id,
        ),
        mesh_sizes_mm: input.mesh_sizes_mm,
        displacement_relative_tolerance: 0.03,
        stress_relative_tolerance: 0.05,
    }
}

#[derive(Debug, Clone)]
struct FemTopologyFaceRegion {
    face_group_indices: Vec<u32>,
    depth_mm: f64,
}

fn fem_stage_progress(stage: FemPipelineStage) -> u64 {
    match stage {
        FemPipelineStage::Resolve => 1,
        FemPipelineStage::BoundaryMesh => 2,
        FemPipelineStage::VolumeMesh => 3,
        FemPipelineStage::ValidateMesh => 4,
        FemPipelineStage::Assemble => 5,
        FemPipelineStage::ApplyConstraints => 6,
        FemPipelineStage::Solve => 7,
        FemPipelineStage::Postprocess => 8,
        FemPipelineStage::Verify => 9,
        FemPipelineStage::Publish => 10,
    }
}

fn emit_ui_fem_long_task(
    state: &AppState,
    job_id: &str,
    analysis_name: &str,
    stage: &str,
    detail: Option<String>,
    progress_current: u64,
    progress_total: u64,
    expected_duration_ms: u64,
    task_state: crate::contracts::AgentActivityState,
) {
    let terminal = task_state != crate::contracts::AgentActivityState::Active;
    let summary = if terminal {
        match &task_state {
            crate::contracts::AgentActivityState::Resolved => {
                format!("{analysis_name} complete")
            }
            crate::contracts::AgentActivityState::Canceled => {
                format!("{analysis_name} canceled")
            }
            crate::contracts::AgentActivityState::Failed => format!("{analysis_name} failed"),
            crate::contracts::AgentActivityState::Active => analysis_name.to_string(),
        }
    } else {
        analysis_name.to_string()
    };
    crate::services::agent_activity::record_long_task_activity(
        state,
        crate::services::agent_activity::LongTaskActivityInput {
            session_id: "ui-fem".to_string(),
            thread_id: None,
            message_id: None,
            actor_kind: crate::contracts::AgentActivityActorKind::System,
            actor_id: "fem".to_string(),
            actor_label: "FEM".to_string(),
            job_id: job_id.to_string(),
            stage: stage.to_string(),
            summary,
            detail,
            progress_current,
            progress_total,
            expected_duration_ms,
            state: task_state,
            cancellable: true,
        },
    );
}

#[tauri::command]
#[specta::specta]
pub fn validate_fem_study(
    request: FemStudyRequest,
    app: AppHandle,
) -> AppResult<FemStudyValidationResponse> {
    validate_fem_study_with_resolver(request, &app)
}

#[tauri::command]
#[specta::specta]
pub fn validate_fem_study_intent(
    input: FemRunIntentInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<FemStudyValidationResponse> {
    let compute = state
        .config
        .lock()
        .map_err(|_| AppError::internal("FEM compute configuration lock was poisoned."))?
        .fem_compute
        .clone();
    let request = fem_study_request_from_intent(input, &compute, next_fem_job_id("validate"));
    validate_fem_study_with_resolver(request, &app)
}

#[tauri::command]
#[specta::specta]
pub async fn run_fem_study_intent(
    input: FemRunIntentInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<FemRunIntentResponse> {
    let compute = state
        .config
        .lock()
        .map_err(|_| AppError::internal("FEM compute configuration lock was poisoned."))?
        .fem_compute
        .clone();
    let mut request = fem_study_request_from_intent(input, &compute, next_fem_run_job_id());
    validate_job_id(&request.job_id)?;
    request.model_id = crate::services::render::ensure_production_bundle_for_model(
        &request.model_id,
        &state,
        &app,
    )
    .await?;

    let analysis_name = request.analysis_name.clone();
    let expected_duration_ms = request.control.maximum_runtime_ms;
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.fem_cancellations.lock().await;
        if jobs.contains_key(&request.job_id) {
            return Err(AppError::conflict(format!(
                "FEM job '{}' is already running.",
                request.job_id
            )));
        }
        jobs.insert(request.job_id.clone(), cancellation.clone());
    }

    let job_id = request.job_id.clone();
    emit_ui_fem_long_task(
        state.inner(),
        &job_id,
        &analysis_name,
        "QUEUED",
        Some("Native Tet4 study queued.".to_string()),
        0,
        10,
        expected_duration_ms,
        crate::contracts::AgentActivityState::Active,
    );

    let jobs = state.fem_cancellations.clone();
    let worker_app = app.clone();
    let worker_analysis_name = analysis_name.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let event_job_id = request.job_id.clone();
        let validation = validate_fem_study_with_resolver(request.clone(), &worker_app)?;
        let result = run_fem_study_with_resolver_subscribed(
            request,
            &worker_app,
            cancellation,
            |progress| {
                let progress_current = fem_stage_progress(progress.stage);
                emit_ui_fem_long_task(
                    worker_app.state::<AppState>().inner(),
                    &event_job_id,
                    &worker_analysis_name,
                    &format!("{:?}", progress.stage).to_ascii_uppercase(),
                    Some(progress.detail.clone()),
                    progress_current,
                    10,
                    expected_duration_ms,
                    crate::contracts::AgentActivityState::Active,
                );
                let _ = worker_app.emit(
                    "fem-progress",
                    serde_json::json!({"jobId": event_job_id, "progress": progress}),
                );
            },
        )?;
        Ok(FemRunIntentResponse { validation, result })
    })
    .await;

    jobs.lock().await.remove(&job_id);
    let outcome: AppResult<FemRunIntentResponse> =
        joined.map_err(|error| AppError::internal(format!("FEM intent thread failed: {error}")))?;
    match &outcome {
        Ok(_) => emit_ui_fem_long_task(
            state.inner(),
            &job_id,
            &analysis_name,
            "DONE",
            Some("Immutable FEM result published.".to_string()),
            10,
            10,
            expected_duration_ms,
            crate::contracts::AgentActivityState::Resolved,
        ),
        Err(error) => emit_ui_fem_long_task(
            state.inner(),
            &job_id,
            &analysis_name,
            "FAILED",
            Some(error.to_string()),
            0,
            10,
            expected_duration_ms,
            crate::contracts::AgentActivityState::Failed,
        ),
    }
    outcome
}

pub(crate) fn validate_fem_study_with_resolver(
    request: FemStudyRequest,
    app: &dyn PathResolver,
) -> AppResult<FemStudyValidationResponse> {
    let resolved = resolve_request(&request, app)?;
    let faces = resolve_fem_face_tags(&resolved.manifest.tagged_anchors, &resolved.boundary)?;
    let study = authored_study_from_core(
        &resolved.program,
        &request.analysis_name,
        &faces,
        resolved.budgets,
    )?;
    let ledger = engineering_ledger_from_core(
        &resolved.program,
        &request.analysis_name,
        &resolved.boundary.source_geometry_digest,
        &resolved.boundary.source_geometry_digest,
    )?;
    Ok(FemStudyValidationResponse {
        job_id: request.job_id,
        model_id: request.model_id,
        analysis_name: request.analysis_name,
        part_id: study.part_id,
        source_digest: crate::services::render_snapshot::canonical_source_digest(&request.source),
        source_geometry_digest: resolved.boundary.source_geometry_digest,
        boundary_digest: resolved.boundary.content_digest,
        boundary_node_count: resolved.boundary.vertices.len() as u64,
        boundary_triangle_count: resolved.boundary.triangles.len() as u64,
        face_group_count: resolved.boundary.face_groups.len() as u64,
        decision_readiness_error: ledger
            .validate_decision_readiness()
            .err()
            .map(|error| error.to_string()),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn run_fem_study(
    mut request: FemStudyRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<FemRunResponse> {
    validate_job_id(&request.job_id)?;
    request.model_id = crate::services::render::ensure_production_bundle_for_model(
        &request.model_id,
        &state,
        &app,
    )
    .await?;
    let analysis_name = request.analysis_name.clone();
    let expected_duration_ms = request.control.maximum_runtime_ms;
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.fem_cancellations.lock().await;
        if jobs.contains_key(&request.job_id) {
            return Err(AppError::conflict(format!(
                "FEM job '{}' is already running.",
                request.job_id
            )));
        }
        jobs.insert(request.job_id.clone(), cancellation.clone());
    }
    let job_id = request.job_id.clone();
    emit_ui_fem_long_task(
        state.inner(),
        &job_id,
        &analysis_name,
        "QUEUED",
        Some("Native Tet4 study queued.".to_string()),
        0,
        10,
        expected_duration_ms,
        crate::contracts::AgentActivityState::Active,
    );
    let jobs = state.fem_cancellations.clone();
    let worker_app = app.clone();
    let worker_analysis_name = analysis_name.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let job_id = request.job_id.clone();
        run_fem_study_with_resolver_subscribed(request, &worker_app, cancellation, |progress| {
            let progress_current = fem_stage_progress(progress.stage);
            emit_ui_fem_long_task(
                worker_app.state::<AppState>().inner(),
                &job_id,
                &worker_analysis_name,
                &format!("{:?}", progress.stage).to_ascii_uppercase(),
                Some(progress.detail.clone()),
                progress_current,
                10,
                expected_duration_ms,
                crate::contracts::AgentActivityState::Active,
            );
            let _ = worker_app.emit(
                "fem-progress",
                serde_json::json!({"jobId": job_id, "progress": progress}),
            );
        })
    })
    .await;
    jobs.lock().await.remove(&job_id);
    let outcome =
        joined.map_err(|error| AppError::internal(format!("FEM job thread failed: {error}")))?;
    match &outcome {
        Ok(_) => emit_ui_fem_long_task(
            state.inner(),
            &job_id,
            &analysis_name,
            "DONE",
            Some("Immutable FEM result published.".to_string()),
            10,
            10,
            expected_duration_ms,
            crate::contracts::AgentActivityState::Resolved,
        ),
        Err(error) => emit_ui_fem_long_task(
            state.inner(),
            &job_id,
            &analysis_name,
            "FAILED",
            Some(error.to_string()),
            0,
            10,
            expected_duration_ms,
            crate::contracts::AgentActivityState::Failed,
        ),
    }
    outcome
}

#[tauri::command]
#[specta::specta]
pub async fn preview_fem_mesh(
    mut request: FemStudyRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<FemMeshPreviewResponse> {
    validate_job_id(&request.job_id)?;
    request.model_id = crate::services::render::ensure_production_bundle_for_model(
        &request.model_id,
        &state,
        &app,
    )
    .await?;
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.fem_cancellations.lock().await;
        if jobs.contains_key(&request.job_id) {
            return Err(AppError::conflict(format!(
                "FEM job '{}' is already running.",
                request.job_id
            )));
        }
        jobs.insert(request.job_id.clone(), cancellation.clone());
    }
    let job_id = request.job_id.clone();
    let jobs = state.fem_cancellations.clone();
    let worker_app = app.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let event_job_id = request.job_id.clone();
        preview_fem_mesh_with_resolver_subscribed(request, &worker_app, cancellation, |progress| {
            let _ = worker_app.emit(
                "fem-progress",
                serde_json::json!({"jobId": event_job_id, "progress": progress}),
            );
        })
    })
    .await;
    jobs.lock().await.remove(&job_id);
    joined.map_err(|error| AppError::internal(format!("FEM mesh thread failed: {error}")))?
}

#[tauri::command]
#[specta::specta]
pub async fn preview_fem_mesh_intent(
    input: FemRunIntentInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<FemMeshPreviewIntentResponse> {
    let compute = state
        .config
        .lock()
        .map_err(|_| AppError::internal("FEM compute configuration lock was poisoned."))?
        .fem_compute
        .clone();
    let mut request = fem_study_request_from_intent(input, &compute, next_fem_job_id("preview"));
    validate_job_id(&request.job_id)?;
    request.model_id = crate::services::render::ensure_production_bundle_for_model(
        &request.model_id,
        &state,
        &app,
    )
    .await?;
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.fem_cancellations.lock().await;
        jobs.insert(request.job_id.clone(), cancellation.clone());
    }
    let job_id = request.job_id.clone();
    let jobs = state.fem_cancellations.clone();
    let worker_app = app.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let event_job_id = request.job_id.clone();
        let validation = validate_fem_study_with_resolver(request.clone(), &worker_app)?;
        let mesh = preview_fem_mesh_with_resolver_subscribed(
            request,
            &worker_app,
            cancellation,
            |progress| {
                let _ = worker_app.emit(
                    "fem-progress",
                    serde_json::json!({"jobId": event_job_id, "progress": progress}),
                );
            },
        )?;
        Ok(FemMeshPreviewIntentResponse { validation, mesh })
    })
    .await;
    jobs.lock().await.remove(&job_id);
    let outcome: AppResult<FemMeshPreviewIntentResponse> = joined
        .map_err(|error| AppError::internal(format!("FEM mesh intent thread failed: {error}")))?;
    outcome
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_fem_study(
    job_id: String,
    state: State<'_, AppState>,
) -> AppResult<FemCancelResponse> {
    validate_job_id(&job_id)?;
    let jobs = state.fem_cancellations.lock().await;
    let cancellation_requested = jobs.get(&job_id).is_some_and(|cancelled| {
        cancelled.store(true, Ordering::Release);
        true
    });
    Ok(FemCancelResponse {
        job_id,
        cancellation_requested,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn run_fem_topology_optimization(
    mut request: FemTopologyRunRequest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<FemTopologyRunResponse> {
    let compute = state.config.lock().unwrap().fem_compute.clone();
    apply_fem_compute_policy(&compute, &mut request.study);
    request.study.model_id = crate::services::render::ensure_production_bundle_for_model(
        &request.study.model_id,
        &state,
        &app,
    )
    .await?;
    let runtime_policy = FemTopologyRuntimePolicy::from_compute(&compute);
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.fem_cancellations.lock().await;
        if jobs.contains_key(&request.study.job_id) {
            return Err(AppError::conflict(format!(
                "FEM job '{}' is already running.",
                request.study.job_id
            )));
        }
        jobs.insert(request.study.job_id.clone(), cancellation.clone());
    }
    let jobs = state.fem_cancellations.clone();
    let job_id = request.study.job_id.clone();
    let response = tauri::async_runtime::spawn_blocking(move || {
        run_fem_topology_with_resolver(request, &runtime_policy, &app, cancellation.as_ref())
    })
    .await
    .map_err(|error| AppError::internal(format!("FEM topology worker failed: {error}")))?;
    jobs.lock().await.remove(&job_id);
    response
}

pub(crate) fn run_fem_topology_with_resolver(
    request: FemTopologyRunRequest,
    runtime_policy: &FemTopologyRuntimePolicy,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
) -> AppResult<FemTopologyRunResponse> {
    let resolved = resolve_request(&request.study, app)?;
    let resolved_faces =
        resolve_fem_face_tags(&resolved.manifest.tagged_anchors, &resolved.boundary)?;
    let authored = authored_study_from_core(
        &resolved.program,
        &request.study.analysis_name,
        &resolved_faces,
        resolved.budgets.clone(),
    )?;
    let model_controls = authored.topology_controls.as_ref().ok_or_else(|| {
        AppError::validation(format!(
            "FEM topology analysis '{}' is missing topology-controls.",
            request.study.analysis_name
        ))
    })?;
    let (
        material,
        load_cases,
        fixed_face_group_indices,
        passive_solid_regions,
        passive_void_regions,
    ) = topology_inputs_from_authored_study(&authored, &resolved.boundary)?;
    let mesh = preview_fem_mesh_with_resolver_and_subscription(
        request.study.clone(),
        app,
        cancellation,
        None,
        |_| {},
    )?;
    run_fem_topology_artifact_with_resolver(
        FemTopologyArtifactRunRequest {
            job_id: request.study.job_id,
            analysis_identity_digest: mesh.analysis_identity_digest,
            mesh_content_digest: mesh.mesh_content_digest,
            material,
            load_cases,
            fixed_face_group_indices,
            passive_solid_regions,
            passive_void_regions,
            relative_solver_tolerance: request.study.control.relative_solver_tolerance,
            controls: topology_controls_from_authored(model_controls, runtime_policy),
            resume_state_digest: request.resume_state_digest,
        },
        app,
        cancellation,
    )
}

#[allow(dead_code)]
pub(crate) fn reconstruct_fem_topology_with_resolver(
    request: FemTopologyReconstructRequest,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
) -> AppResult<FemTopologyReconstructResponse> {
    if !request.density_threshold.is_finite() || !(0.0..=1.0).contains(&request.density_threshold) {
        return Err(AppError::validation(
            "FEM topology densityThreshold must be finite and within [0, 1].",
        ));
    }
    let maximum_result_bytes = request.study.budgets.result_bytes;
    let current_mesh = preview_fem_mesh_with_resolver_and_subscription(
        request.study.clone(),
        app,
        cancellation,
        None,
        |_| {},
    )?;
    if current_mesh.analysis_identity_digest != request.analysis_identity_digest
        || current_mesh.mesh_content_digest != request.mesh_content_digest
    {
        return Err(AppError::conflict(
            "FEM topology reconstruction artifacts are stale for the current source, study, or mesh.",
        ));
    }
    let mesh_asset = load_fem_mesh_asset(
        app,
        &request.analysis_identity_digest,
        &request.mesh_content_digest,
        maximum_result_bytes,
    )?;
    let mesh_data = decode_fem_topology_mesh(&mesh_asset)?;
    let topology = load_fem_topology_artifact(
        app,
        &request.input_digest,
        &request.state_digest,
        maximum_result_bytes,
    )?;
    if topology.result.termination != FemTopologyTermination::Converged {
        return Err(AppError::conflict(format!(
            "FEM topology reconstruction requires converged density; termination was '{}'.",
            topology_termination_name(topology.result.termination)
        )));
    }
    if topology.result.densities.len() != mesh_data.mesh.cells.len() {
        return Err(AppError::conflict(
            "FEM topology density artifact does not match the bound Tet4 mesh.",
        ));
    }

    let resolved = resolve_request(&request.study, app)?;
    let resolved_faces =
        resolve_fem_face_tags(&resolved.manifest.tagged_anchors, &resolved.boundary)?;
    let authored = authored_study_from_core(
        &resolved.program,
        &request.study.analysis_name,
        &resolved_faces,
        resolved.budgets,
    )?;
    let anchors = topology_density_anchors(&authored, &resolved.boundary, &mesh_data)?;
    let surface = reconstruct_density_surface(
        &mesh_data.mesh,
        &topology.result.densities,
        &anchors,
        &FemDensitySurfaceControls {
            density_threshold: request.density_threshold,
            smoothing_passes: 0,
            maximum_smoothing_displacement_mm: 0.0,
        },
    )
    .map_err(fem_topology_error)?;
    let solid_expression =
        crate::fem_topology_reconstruction::density_surface_solid_expression(&surface)?;

    Ok(FemTopologyReconstructResponse {
        analysis_identity_digest: request.analysis_identity_digest,
        mesh_content_digest: request.mesh_content_digest,
        input_digest: request.input_digest,
        state_digest: request.state_digest,
        result_digest: topology.result.result_digest,
        solid_expression,
        vertex_count: surface.vertices.len() as u64,
        triangle_count: surface.triangles.len() as u64,
        discarded_cell_count: surface.discarded_cell_indices.len() as u64,
        discarded_active_volume_fraction: surface.discarded_active_volume_fraction,
        connected_anchor_ids: surface.connected_anchor_ids,
        signed_volume_mm3: surface.signed_volume_mm3,
        closed_manifold: true,
        exact_brep: false,
        independently_verified: false,
        scope_disclaimer: "Closed manifold solid expression only. Render through the faceted BRep bridge, preserve authored FEM tags, then independently remesh and solve before publication.".into(),
    })
}

fn topology_density_anchors(
    study: &crate::fem_engineering::FemAuthoredStudy,
    boundary: &AnalysisBoundarySurface,
    mesh: &crate::services::fem_artifacts::FemTopologyMeshData,
) -> AppResult<Vec<FemDensityAnchor>> {
    let mut anchors = Vec::new();
    for constraint in &study.constraints {
        let (name, faces) = match constraint {
            FemConstraint::Fixed { name, faces, .. }
            | FemConstraint::PrescribedDisplacement { name, faces, .. } => (name, faces),
        };
        let groups = topology_face_group_indices(boundary, faces)?;
        anchors.push(FemDensityAnchor {
            id: format!("support:{name}"),
            cells: topology_boundary_cells(mesh, &groups)?,
        });
    }
    for load in &study.loads {
        let (name, faces) = match load {
            FemLoad::SurfaceForce { name, faces, .. }
            | FemLoad::Traction { name, faces, .. }
            | FemLoad::Pressure { name, faces, .. } => (name, faces),
        };
        let groups = topology_face_group_indices(boundary, faces)?;
        anchors.push(FemDensityAnchor {
            id: format!("load:{name}"),
            cells: topology_boundary_cells(mesh, &groups)?,
        });
    }
    anchors.sort_by(|left, right| left.id.cmp(&right.id));
    if anchors.is_empty() {
        return Err(AppError::validation(
            "FEM topology reconstruction requires at least one authored support or load anchor.",
        ));
    }
    Ok(anchors)
}

fn topology_boundary_cells(
    mesh: &crate::services::fem_artifacts::FemTopologyMeshData,
    groups: &[u32],
) -> AppResult<Vec<usize>> {
    let groups =
        validate_topology_groups(groups, mesh.face_group_count, "anchors.faceGroupIndices")?;
    let mut owners = BTreeMap::<[u32; 3], Vec<usize>>::new();
    for (cell_index, cell) in mesh.mesh.cells.iter().enumerate() {
        for mut face in [
            [cell[0], cell[1], cell[2]],
            [cell[0], cell[1], cell[3]],
            [cell[0], cell[2], cell[3]],
            [cell[1], cell[2], cell[3]],
        ] {
            face.sort_unstable();
            owners.entry(face).or_default().push(cell_index);
        }
    }
    let mut cells = BTreeSet::new();
    for (triangle, _group) in mesh
        .boundary_triangles
        .iter()
        .zip(&mesh.boundary_face_group_indices)
        .filter(|(_, group)| groups.contains(group))
    {
        let mut key = *triangle;
        key.sort_unstable();
        let owner = owners.get(&key).ok_or_else(|| {
            AppError::validation("FEM topology anchor boundary triangle has no Tet4 owner.")
        })?;
        if owner.len() != 1 {
            return Err(AppError::validation(
                "FEM topology anchor boundary triangle must have exactly one Tet4 owner.",
            ));
        }
        cells.insert(owner[0]);
    }
    if cells.is_empty() {
        return Err(AppError::validation(
            "FEM topology anchor face groups resolve no boundary Tet4 cells.",
        ));
    }
    Ok(cells.into_iter().collect())
}

fn topology_controls_from_authored(
    authored: &FemAuthoredTopologyControls,
    runtime: &FemTopologyRuntimePolicy,
) -> FemTopologyControlsDto {
    FemTopologyControlsDto {
        volume_fraction: authored.volume_fraction,
        penalty: authored.penalty,
        minimum_density: authored.minimum_density,
        filter_radius_mm: authored.filter_radius_mm,
        move_limit: authored.move_limit,
        convergence_tolerance: authored.convergence_tolerance,
        maximum_iterations: runtime.maximum_iterations,
        maximum_new_iterations: runtime.maximum_new_iterations,
        maximum_dimension: runtime.maximum_dimension,
        maximum_elements: runtime.maximum_elements,
        maximum_solve_count: 0,
        maximum_working_memory_bytes: runtime.maximum_working_memory_bytes,
        maximum_result_bytes: runtime.maximum_result_bytes,
        maximum_wall_time_ms: runtime.maximum_wall_time_ms,
    }
}

fn run_fem_topology_artifact_with_resolver(
    request: FemTopologyArtifactRunRequest,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
) -> AppResult<FemTopologyRunResponse> {
    if request.job_id.trim().is_empty() {
        return Err(AppError::validation(
            "FEM topology jobId must not be empty.",
        ));
    }
    let maximum_result_bytes = request.controls.maximum_result_bytes;
    let mesh_asset = load_fem_mesh_asset(
        app,
        &request.analysis_identity_digest,
        &request.mesh_content_digest,
        maximum_result_bytes,
    )?;
    let mesh_data = decode_fem_topology_mesh(&mesh_asset)?;
    let material = FemMaterial {
        schema_version: FEM_SCHEMA_VERSION,
        name: request.material.name.clone(),
        young_modulus_mpa: request.material.young_modulus_mpa,
        poisson_ratio: request.material.poisson_ratio,
        density_kg_per_mm3: request.material.density_kg_per_mm3,
        yield_strength_mpa: request.material.yield_strength_mpa,
    };
    let load_cases = topology_surface_loads(&mesh_data, &request.load_cases)?;
    let constraints = topology_fixed_constraints(&mesh_data, &request.fixed_face_group_indices)?;
    let passive_solid_cells =
        topology_region_cells(&mesh_data, &request.passive_solid_regions, "passive-solid")?;
    let passive_void_cells =
        topology_region_cells(&mesh_data, &request.passive_void_regions, "passive-void")?;
    let runtime_identity_digest = sha256_text(&format!(
        "ecky-fem:{}:accelerate-sparse-system:tet4-simp-gcmma-normalized-objective-v9",
        env!("CARGO_PKG_VERSION")
    ));
    let maximum_new_iterations = bounded_usize(
        "maximumNewIterations",
        request.controls.maximum_new_iterations,
    )?;
    let maximum_solve_count =
        ecky_fem::topology_required_solve_capacity(maximum_new_iterations, load_cases.len());
    let controls = FemTopologyControls {
        volume_fraction: request.controls.volume_fraction,
        penalty: request.controls.penalty,
        minimum_density: request.controls.minimum_density,
        filter_radius_mm: request.controls.filter_radius_mm,
        move_limit: request.controls.move_limit,
        convergence_tolerance: request.controls.convergence_tolerance,
        relative_solver_tolerance: request.relative_solver_tolerance,
        require_parallel_solver: true,
        maximum_iterations: bounded_usize(
            "maximumIterations",
            request.controls.maximum_iterations,
        )?,
        maximum_dimension: bounded_usize("maximumDimension", request.controls.maximum_dimension)?,
        maximum_elements: bounded_usize("maximumElements", request.controls.maximum_elements)?,
        maximum_solve_count,
        maximum_working_memory_bytes: bounded_usize(
            "maximumWorkingMemoryBytes",
            request.controls.maximum_working_memory_bytes,
        )?,
        maximum_result_bytes: bounded_usize(
            "maximumResultBytes",
            request.controls.maximum_result_bytes,
        )?,
        maximum_wall_time_ms: request.controls.maximum_wall_time_ms,
        runtime_identity_digest,
        passive_solid_cells,
        passive_void_cells,
    };
    let initial = initialize_simp_state(
        &mesh_data.mesh,
        &material,
        &load_cases,
        &constraints,
        &controls,
    )
    .map_err(fem_topology_error)?;
    let mut state = if let Some(state_digest) = &request.resume_state_digest {
        load_fem_topology_state(
            app,
            &initial.input_digest,
            state_digest,
            maximum_result_bytes,
        )?
        .state
    } else {
        initial
    };
    let mut gcmma_trace = Vec::new();
    let checkpoint_error = std::cell::RefCell::new(None::<String>);
    let termination = match advance_simp_state_traced_checkpointed(
        &mesh_data.mesh,
        &material,
        &load_cases,
        &constraints,
        &controls,
        &mut state,
        maximum_new_iterations,
        || cancellation.load(Ordering::Acquire) || checkpoint_error.borrow().is_some(),
        |attempt| gcmma_trace.push(attempt),
        |checkpoint| {
            if checkpoint_error.borrow().is_some() {
                return;
            }
            if let Err(error) =
                publish_fem_topology_state_checkpoint(app, checkpoint, maximum_result_bytes)
            {
                *checkpoint_error.borrow_mut() = Some(error.to_string());
            }
        },
    ) {
        Ok(termination) => termination,
        Err(error) => {
            let artifact =
                publish_fem_topology_state_checkpoint(app, &state, maximum_result_bytes)?;
            return Err(AppError::validation(format!(
                "FEM topology {}: {}; resumableStateDigest={}; checkpointPath={}",
                error.field,
                error.message,
                state.state_digest,
                artifact.checkpoint_path.to_string_lossy(),
            )));
        }
    };
    if let Some(error) = checkpoint_error.into_inner() {
        return Err(AppError::persistence(format!(
            "FEM topology iteration checkpoint failed: {error}"
        )));
    }
    if matches!(
        termination,
        FemTopologyTermination::Cancelled | FemTopologyTermination::MaximumWallTime
    ) {
        let artifact = publish_fem_topology_state_checkpoint(app, &state, maximum_result_bytes)?;
        return Ok(FemTopologyRunResponse {
            job_id: request.job_id,
            analysis_identity_digest: request.analysis_identity_digest,
            mesh_content_digest: request.mesh_content_digest,
            input_digest: state.input_digest,
            state_digest: state.state_digest,
            result_digest: None,
            termination: topology_termination_name(termination).into(),
            iteration_count: state.iterations.len() as u64,
            initial_compliance: state.initial_compliance,
            final_compliance: None,
            final_volume_fraction: None,
            passive_solid_volume_fraction: None,
            passive_void_volume_fraction: None,
            gcmma_trace_edn: format_gcmma_attempt_trace(&gcmma_trace),
            checkpoint_path: artifact.checkpoint_path.to_string_lossy().into_owned(),
            density_path: None,
            preview_vtu_path: None,
            exact_brep: false,
            production_step: false,
            engineering_accepted: false,
            scope_disclaimer: "Topology run stopped at a safe boundary; resumable state only, no final analysis result.".into(),
        });
    }
    let result = finalize_simp_state(
        &mesh_data.mesh,
        &material,
        &load_cases,
        &constraints,
        &controls,
        &state,
        termination,
    )
    .map_err(fem_topology_error)?;
    let artifact =
        publish_fem_topology_artifact(app, &mesh_data.mesh, &state, &result, maximum_result_bytes)?;
    Ok(FemTopologyRunResponse {
        job_id: request.job_id,
        analysis_identity_digest: request.analysis_identity_digest,
        mesh_content_digest: request.mesh_content_digest,
        input_digest: state.input_digest,
        state_digest: state.state_digest,
        result_digest: Some(result.result_digest),
        termination: topology_termination_name(termination).into(),
        iteration_count: result.iterations.len() as u64,
        initial_compliance: Some(result.initial_compliance),
        final_compliance: Some(result.final_compliance),
        final_volume_fraction: Some(result.final_volume_fraction),
        passive_solid_volume_fraction: Some(result.passive_solid_volume_fraction),
        passive_void_volume_fraction: Some(result.passive_void_volume_fraction),
        gcmma_trace_edn: format_gcmma_attempt_trace(&gcmma_trace),
        checkpoint_path: artifact.checkpoint_path.to_string_lossy().into_owned(),
        density_path: Some(artifact.density_path.to_string_lossy().into_owned()),
        preview_vtu_path: Some(artifact.preview_vtu_path.to_string_lossy().into_owned()),
        exact_brep: false,
        production_step: false,
        engineering_accepted: false,
        scope_disclaimer: "Compliance topology evidence only; independent reconstructed-geometry FEM remains required.".into(),
    })
}

fn topology_inputs_from_authored_study(
    study: &crate::fem_engineering::FemAuthoredStudy,
    boundary: &AnalysisBoundarySurface,
) -> AppResult<(
    FemTopologyMaterialDto,
    Vec<FemTopologySurfaceLoadDto>,
    Vec<u32>,
    Vec<FemTopologyFaceRegion>,
    Vec<FemTopologyFaceRegion>,
)> {
    let material = FemTopologyMaterialDto {
        name: study.material.name.clone(),
        young_modulus_mpa: study.material.young_modulus_mpa,
        poisson_ratio: study.material.poisson_ratio,
        density_kg_per_mm3: study.material.density_kg_per_mm3,
        yield_strength_mpa: study.material.yield_strength_mpa,
    };
    let load_cases = study
        .loads
        .iter()
        .map(|load| match load {
            FemLoad::SurfaceForce {
                name,
                faces,
                total_force_n,
                ..
            } => Ok(FemTopologySurfaceLoadDto {
                id: name.clone(),
                weight: 1.0,
                face_group_indices: topology_face_group_indices(boundary, faces)?,
                total_force_n: [total_force_n.x_n, total_force_n.y_n, total_force_n.z_n],
            }),
            FemLoad::Traction { name, .. } | FemLoad::Pressure { name, .. } => {
                Err(AppError::validation(format!(
                    "FEM topology authored load '{name}' must be surface-force in this slice."
                )))
            }
        })
        .collect::<AppResult<Vec<_>>>()?;
    let mut fixed_face_group_indices = Vec::new();
    for constraint in &study.constraints {
        match constraint {
            FemConstraint::Fixed { faces, .. } => {
                fixed_face_group_indices.extend(topology_face_group_indices(boundary, faces)?);
            }
            FemConstraint::PrescribedDisplacement { name, .. } => {
                return Err(AppError::validation(format!(
                    "FEM topology authored constraint '{name}' must be fixed in this slice."
                )));
            }
        }
    }
    fixed_face_group_indices.sort_unstable();
    fixed_face_group_indices.dedup();
    if load_cases.is_empty() || fixed_face_group_indices.is_empty() {
        return Err(AppError::validation(
            "FEM topology authored study requires at least one surface-force and one fixed support.",
        ));
    }
    let map_regions = |regions: &[crate::fem_engineering::FemAuthoredTopologyRegion]| {
        regions
            .iter()
            .map(|region| {
                Ok(FemTopologyFaceRegion {
                    face_group_indices: topology_face_group_indices(boundary, &region.faces)?,
                    depth_mm: region.depth_mm,
                })
            })
            .collect::<AppResult<Vec<_>>>()
    };
    Ok((
        material,
        load_cases,
        fixed_face_group_indices,
        map_regions(&study.passive_solid_regions)?,
        map_regions(&study.passive_void_regions)?,
    ))
}

fn topology_face_group_indices(
    boundary: &AnalysisBoundarySurface,
    faces: &[ecky_fem::FemFaceTarget],
) -> AppResult<Vec<u32>> {
    let mut indices = Vec::new();
    for face in faces {
        let matches = boundary
            .face_groups
            .iter()
            .enumerate()
            .filter(|(_, group)| {
                group.part_id == face.part_id
                    && group.canonical_target_id == face.canonical_target_id
                    && group.durable_target_id.as_deref() == Some(face.durable_target_id.as_str())
            })
            .map(|(index, _)| index as u32)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [index] => indices.push(*index),
            _ => {
                return Err(AppError::validation(format!(
                    "FEM topology face target '{}' resolved to {} boundary groups; expected one.",
                    face.canonical_target_id,
                    matches.len()
                )));
            }
        }
    }
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

fn topology_region_cells(
    mesh: &crate::services::fem_artifacts::FemTopologyMeshData,
    regions: &[FemTopologyFaceRegion],
    label: &str,
) -> AppResult<Vec<usize>> {
    let mut selected = BTreeSet::new();
    for region in regions {
        if !region.depth_mm.is_finite() || region.depth_mm <= 0.0 {
            return Err(AppError::validation(format!(
                "FEM topology {label} depth must be finite and positive."
            )));
        }
        let groups = validate_topology_groups(
            &region.face_group_indices,
            mesh.face_group_count,
            &format!("{label}.faceGroupIndices"),
        )?;
        let triangles = mesh
            .boundary_triangles
            .iter()
            .zip(&mesh.boundary_face_group_indices)
            .filter(|(_, group)| groups.contains(group))
            .map(|(triangle, _)| triangle.map(|index| point_array(mesh.mesh.nodes[index as usize])))
            .collect::<Vec<_>>();
        if triangles.is_empty() {
            return Err(AppError::validation(format!(
                "FEM topology {label} region resolves no boundary triangles."
            )));
        }
        let centroids = mesh
            .mesh
            .cells
            .iter()
            .map(|cell| tet_centroid(cell.map(|index| mesh.mesh.nodes[index as usize])))
            .collect::<Vec<_>>();
        for cell_index in indexed_points_near_triangles(&centroids, &triangles, region.depth_mm) {
            selected.insert(cell_index);
        }
    }
    Ok(selected.into_iter().collect())
}

fn indexed_points_near_triangles(
    points: &[[f64; 3]],
    triangles: &[[[f64; 3]; 3]],
    distance: f64,
) -> Vec<usize> {
    let cell_of = |coordinate: f64| (coordinate / distance).floor() as i64;
    let mut triangle_grid = HashMap::<(i64, i64, i64), Vec<usize>>::new();
    for (triangle_index, triangle) in triangles.iter().enumerate() {
        let mut minimum = [f64::INFINITY; 3];
        let mut maximum = [f64::NEG_INFINITY; 3];
        for vertex in triangle {
            for axis in 0..3 {
                minimum[axis] = minimum[axis].min(vertex[axis]);
                maximum[axis] = maximum[axis].max(vertex[axis]);
            }
        }
        let lower = minimum.map(|value| cell_of(value - distance));
        let upper = maximum.map(|value| cell_of(value + distance));
        for x in lower[0]..=upper[0] {
            for y in lower[1]..=upper[1] {
                for z in lower[2]..=upper[2] {
                    triangle_grid
                        .entry((x, y, z))
                        .or_default()
                        .push(triangle_index);
                }
            }
        }
    }
    let maximum_distance_squared = distance * distance;
    points
        .iter()
        .enumerate()
        .filter_map(|(point_index, point)| {
            triangle_grid
                .get(&(cell_of(point[0]), cell_of(point[1]), cell_of(point[2])))
                .is_some_and(|candidates| {
                    candidates.iter().any(|triangle_index| {
                        point_triangle_distance_squared(*point, triangles[*triangle_index])
                            <= maximum_distance_squared
                    })
                })
                .then_some(point_index)
        })
        .collect()
}

fn point_array(point: ecky_fem::FemPoint3) -> [f64; 3] {
    [point.x_mm, point.y_mm, point.z_mm]
}

fn tet_centroid(points: [ecky_fem::FemPoint3; 4]) -> [f64; 3] {
    let mut centroid = [0.0; 3];
    for point in points {
        centroid[0] += point.x_mm * 0.25;
        centroid[1] += point.y_mm * 0.25;
        centroid[2] += point.z_mm * 0.25;
    }
    centroid
}

fn point_triangle_distance_squared(point: [f64; 3], triangle: [[f64; 3]; 3]) -> f64 {
    let [a, b, c] = triangle;
    let ab = sub3(b, a);
    let ac = sub3(c, a);
    let ap = sub3(point, a);
    let d1 = dot3(ab, ap);
    let d2 = dot3(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return dot3(ap, ap);
    }
    let bp = sub3(point, b);
    let d3 = dot3(ab, bp);
    let d4 = dot3(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return dot3(bp, bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let projection = add3(a, scale3(ab, d1 / (d1 - d3)));
        let delta = sub3(point, projection);
        return dot3(delta, delta);
    }
    let cp = sub3(point, c);
    let d5 = dot3(ab, cp);
    let d6 = dot3(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return dot3(cp, cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let projection = add3(a, scale3(ac, d2 / (d2 - d6)));
        let delta = sub3(point, projection);
        return dot3(delta, delta);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let bc = sub3(c, b);
        let projection = add3(b, scale3(bc, (d4 - d3) / ((d4 - d3) + (d5 - d6))));
        let delta = sub3(point, projection);
        return dot3(delta, delta);
    }
    let denominator = 1.0 / (va + vb + vc);
    let projection = add3(
        a,
        add3(scale3(ab, vb * denominator), scale3(ac, vc * denominator)),
    );
    let delta = sub3(point, projection);
    dot3(delta, delta)
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn topology_surface_loads(
    mesh: &crate::services::fem_artifacts::FemTopologyMeshData,
    loads: &[crate::contracts::FemTopologySurfaceLoadDto],
) -> AppResult<Vec<FemTopologyLoadCase>> {
    if loads.is_empty() {
        return Err(AppError::validation(
            "FEM topology loadCases must not be empty.",
        ));
    }
    loads
        .iter()
        .map(|load| {
            let groups = validate_topology_groups(
                &load.face_group_indices,
                mesh.face_group_count,
                "loadCases.faceGroupIndices",
            )?;
            let mut nodal_weights = BTreeMap::<usize, f64>::new();
            for (triangle, group) in mesh
                .boundary_triangles
                .iter()
                .zip(&mesh.boundary_face_group_indices)
            {
                if !groups.contains(group) {
                    continue;
                }
                let points = triangle.map(|index| mesh.mesh.nodes[index as usize]);
                let area = triangle_area(points);
                for node in triangle {
                    *nodal_weights.entry(*node as usize).or_default() += area / 3.0;
                }
            }
            let total_weight = nodal_weights.values().sum::<f64>();
            if !total_weight.is_finite() || total_weight <= 0.0 {
                return Err(AppError::validation(format!(
                    "FEM topology load '{}' resolves no positive-area boundary triangles.",
                    load.id
                )));
            }
            let mut rhs_n = vec![0.0; mesh.mesh.nodes.len() * 3];
            for (node, weight) in nodal_weights {
                for axis in 0..3 {
                    rhs_n[node * 3 + axis] += load.total_force_n[axis] * weight / total_weight;
                }
            }
            Ok(FemTopologyLoadCase {
                id: load.id.clone(),
                weight: load.weight,
                rhs_n,
            })
        })
        .collect()
}

fn topology_fixed_constraints(
    mesh: &crate::services::fem_artifacts::FemTopologyMeshData,
    groups: &[u32],
) -> AppResult<Vec<ecky_fem::FemDirichletConstraint>> {
    let groups = validate_topology_groups(groups, mesh.face_group_count, "fixedFaceGroupIndices")?;
    let nodes = mesh
        .boundary_triangles
        .iter()
        .zip(&mesh.boundary_face_group_indices)
        .filter(|(_, group)| groups.contains(group))
        .flat_map(|(triangle, _)| triangle.iter().copied())
        .collect::<BTreeSet<_>>();
    if nodes.is_empty() {
        return Err(AppError::validation(
            "FEM topology fixed face groups resolve no boundary nodes.",
        ));
    }
    Ok(nodes
        .into_iter()
        .flat_map(|node| {
            (0..3).map(move |axis| ecky_fem::FemDirichletConstraint {
                dof_index: node as usize * 3 + axis,
                value_mm: 0.0,
            })
        })
        .collect())
}

fn validate_topology_groups(
    groups: &[u32],
    group_count: u32,
    field: &str,
) -> AppResult<BTreeSet<u32>> {
    let unique = groups.iter().copied().collect::<BTreeSet<_>>();
    if unique.is_empty()
        || unique.len() != groups.len()
        || unique.iter().any(|value| *value >= group_count)
    {
        return Err(AppError::validation(format!(
            "FEM topology {field} must contain unique in-range groups."
        )));
    }
    Ok(unique)
}

fn triangle_area(points: [ecky_fem::FemPoint3; 3]) -> f64 {
    let a = [
        points[1].x_mm - points[0].x_mm,
        points[1].y_mm - points[0].y_mm,
        points[1].z_mm - points[0].z_mm,
    ];
    let b = [
        points[2].x_mm - points[0].x_mm,
        points[2].y_mm - points[0].y_mm,
        points[2].z_mm - points[0].z_mm,
    ];
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    0.5 * cross.iter().map(|value| value * value).sum::<f64>().sqrt()
}

fn bounded_usize(field: &str, value: u64) -> AppResult<usize> {
    usize::try_from(value)
        .map_err(|_| AppError::validation(format!("FEM topology {field} exceeds platform bounds.")))
}

fn fem_topology_error(error: ecky_fem::FemValidationError) -> AppError {
    AppError::validation(format!("FEM topology {}: {}", error.field, error.message))
}

fn topology_termination_name(value: FemTopologyTermination) -> &'static str {
    match value {
        FemTopologyTermination::Paused => "paused",
        FemTopologyTermination::Cancelled => "cancelled",
        FemTopologyTermination::Converged => "converged",
        FemTopologyTermination::MaximumIterations => "maximumIterations",
        FemTopologyTermination::MaximumWallTime => "maximumWallTime",
    }
}

fn sha256_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[tauri::command]
#[specta::specta]
pub async fn run_fem_convergence(
    mut request: FemConvergenceRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<FemConvergenceResponse> {
    validate_convergence_request(&request)?;
    request.study.model_id = crate::services::render::ensure_production_bundle_for_model(
        &request.study.model_id,
        &state,
        &app,
    )
    .await?;
    let job_id = request.study.job_id.clone();
    let analysis_name = request.study.analysis_name.clone();
    let level_count = request.mesh_sizes_mm.len() as u64;
    let progress_total = level_count.saturating_mul(10).max(1);
    let expected_duration_ms = request
        .study
        .control
        .maximum_runtime_ms
        .saturating_mul(level_count.max(1));
    let cancellation = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.fem_cancellations.lock().await;
        if jobs.contains_key(&job_id) {
            return Err(AppError::conflict(format!(
                "FEM job '{job_id}' is already running."
            )));
        }
        jobs.insert(job_id.clone(), cancellation.clone());
    }
    emit_ui_fem_long_task(
        state.inner(),
        &job_id,
        &analysis_name,
        "CONVERGENCE",
        Some(format!("{level_count} mesh refinement levels queued.")),
        0,
        progress_total,
        expected_duration_ms,
        crate::contracts::AgentActivityState::Active,
    );
    let jobs = state.fem_cancellations.clone();
    let worker_app = app.clone();
    let event_job_id = job_id.clone();
    let worker_analysis_name = analysis_name.clone();
    let completed_stages = Arc::new(AtomicU64::new(0));
    let worker_completed_stages = completed_stages.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        run_fem_convergence_with_resolver(request, &worker_app, cancellation.as_ref(), |progress| {
            let stage_progress = fem_stage_progress(progress.stage);
            let previous = worker_completed_stages.load(Ordering::Acquire);
            let level_base = previous / 10 * 10;
            let candidate = level_base + stage_progress;
            let progress_current = if candidate <= previous {
                previous.saturating_add(stage_progress)
            } else {
                candidate
            }
            .min(progress_total);
            worker_completed_stages.store(progress_current, Ordering::Release);
            emit_ui_fem_long_task(
                worker_app.state::<AppState>().inner(),
                &event_job_id,
                &worker_analysis_name,
                "CONVERGENCE",
                Some(progress.detail.clone()),
                progress_current,
                progress_total,
                expected_duration_ms,
                crate::contracts::AgentActivityState::Active,
            );
            let _ = worker_app.emit(
                "fem-progress",
                serde_json::json!({"jobId": event_job_id, "progress": progress}),
            );
        })
    })
    .await;
    jobs.lock().await.remove(&job_id);
    let outcome = joined
        .map_err(|error| AppError::internal(format!("FEM convergence thread failed: {error}")))?;
    match &outcome {
        Ok(_) => emit_ui_fem_long_task(
            state.inner(),
            &job_id,
            &analysis_name,
            "DONE",
            Some("Convergence evidence published.".to_string()),
            progress_total,
            progress_total,
            expected_duration_ms,
            crate::contracts::AgentActivityState::Resolved,
        ),
        Err(error) => emit_ui_fem_long_task(
            state.inner(),
            &job_id,
            &analysis_name,
            "FAILED",
            Some(error.to_string()),
            completed_stages.load(Ordering::Acquire),
            progress_total,
            expected_duration_ms,
            crate::contracts::AgentActivityState::Failed,
        ),
    }
    outcome
}

#[tauri::command]
#[specta::specta]
pub async fn run_fem_convergence_intent(
    input: FemConvergenceIntentInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<FemConvergenceResponse> {
    let compute = state
        .config
        .lock()
        .map_err(|_| AppError::internal("FEM compute configuration lock was poisoned."))?
        .fem_compute
        .clone();
    let request =
        fem_convergence_request_from_intent(input, &compute, next_fem_job_id("convergence"));
    run_fem_convergence(request, app, state).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_cached_fem_convergence(
    request: FemConvergenceRequest,
    app: AppHandle,
) -> AppResult<Option<FemConvergenceResponse>> {
    tauri::async_runtime::spawn_blocking(move || {
        get_cached_fem_convergence_with_resolver(request, &app)
    })
    .await
    .map_err(|error| {
        AppError::internal(format!(
            "FEM convergence cache lookup thread failed: {error}"
        ))
    })?
}

#[tauri::command]
#[specta::specta]
pub async fn get_cached_fem_convergence_intent(
    input: FemConvergenceIntentInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<FemConvergenceResponse>> {
    let compute = state
        .config
        .lock()
        .map_err(|_| AppError::internal("FEM compute configuration lock was poisoned."))?
        .fem_compute
        .clone();
    let request =
        fem_convergence_request_from_intent(input, &compute, next_fem_job_id("convergence-cache"));
    get_cached_fem_convergence(request, app).await
}

pub(crate) fn read_fem_result_with_resolver(
    request: FemResultReadRequest,
    app: &dyn PathResolver,
) -> AppResult<FemResultReadResponse> {
    if request.maximum_result_bytes == 0 {
        return Err(AppError::validation(
            "FEM result read byte budget must be positive.",
        ));
    }
    let asset = load_fem_result_asset(
        app,
        &request.analysis_identity_digest,
        &request.solution_digest,
        request.maximum_result_bytes,
    )?;
    Ok(asset_response(asset))
}

#[tauri::command]
#[specta::specta]
pub fn export_fem_result_vtu(
    request: FemResultReadRequest,
    target_path: String,
    app: AppHandle,
) -> AppResult<FemVtuExportResponse> {
    let asset = load_fem_result_asset(
        &app,
        &request.analysis_identity_digest,
        &request.solution_digest,
        request.maximum_result_bytes,
    )?;
    let (byte_length, sha256) =
        write_fem_result_vtu(&asset, &target_path, request.maximum_result_bytes)?;
    Ok(FemVtuExportResponse {
        path: target_path,
        byte_length,
        sha256,
        result_digest: asset.result_digest,
    })
}

#[tauri::command]
#[specta::specta]
pub fn export_fem_result_vtu_intent(
    input: FemVtuExportIntentInput,
    target_path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<FemVtuExportResponse> {
    let maximum_result_bytes = state
        .config
        .lock()
        .map_err(|_| AppError::internal("FEM compute configuration lock was poisoned."))?
        .fem_compute
        .maximum_fem_result_bytes();
    export_fem_result_vtu(
        FemResultReadRequest {
            analysis_identity_digest: input.analysis_identity_digest,
            solution_digest: input.solution_digest,
            maximum_result_bytes,
        },
        target_path,
        app,
    )
}

#[cfg(test)]
pub(crate) fn run_fem_study_with_resolver<F>(
    request: FemStudyRequest,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
    progress: F,
) -> AppResult<FemRunResponse>
where
    F: FnMut(FemProgressEvent),
{
    run_fem_study_with_resolver_and_mesh_size_and_subscription(
        request,
        app,
        cancellation,
        None,
        None,
        progress,
    )
}

pub(crate) fn run_fem_study_with_resolver_subscribed<F>(
    request: FemStudyRequest,
    app: &dyn PathResolver,
    cancellation: Arc<AtomicBool>,
    progress: F,
) -> AppResult<FemRunResponse>
where
    F: FnMut(FemProgressEvent),
{
    run_fem_study_with_resolver_and_mesh_size_and_subscription(
        request,
        app,
        cancellation.as_ref(),
        Some(cancellation.clone()),
        None,
        progress,
    )
}

#[cfg(test)]
pub(crate) fn preview_fem_mesh_with_resolver<F>(
    request: FemStudyRequest,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
    progress: F,
) -> AppResult<FemMeshPreviewResponse>
where
    F: FnMut(FemProgressEvent),
{
    preview_fem_mesh_with_resolver_and_subscription(request, app, cancellation, None, progress)
}

pub(crate) fn preview_fem_mesh_with_resolver_subscribed<F>(
    request: FemStudyRequest,
    app: &dyn PathResolver,
    cancellation: Arc<AtomicBool>,
    progress: F,
) -> AppResult<FemMeshPreviewResponse>
where
    F: FnMut(FemProgressEvent),
{
    preview_fem_mesh_with_resolver_and_subscription(
        request,
        app,
        cancellation.as_ref(),
        Some(cancellation.clone()),
        progress,
    )
}

fn preview_fem_mesh_with_resolver_and_subscription<F>(
    request: FemStudyRequest,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
    subscriber_cancellation: Option<Arc<AtomicBool>>,
    mut progress: F,
) -> AppResult<FemMeshPreviewResponse>
where
    F: FnMut(FemProgressEvent),
{
    let started = Instant::now();
    let resolved = resolve_request(&request, app)?;
    let runtime = probe_system_exact_brep_mesher_runtime()?;
    let request_digest = fem_request_cache_digest(&request, &resolved, &runtime, None)?;
    let singleflight = fem_singleflight_job(&format!("mesh:{request_digest}"))?;
    let subscription = subscriber_cancellation
        .map(|cancellation| singleflight.subscribe(cancellation))
        .transpose()?;
    let _cancellation_monitor = subscription
        .as_ref()
        .map(FemSharedSubscription::start_monitor);
    let execution_cancellation = subscription
        .as_ref()
        .map(|subscription| &subscription.job.execution_cancelled)
        .unwrap_or(cancellation);
    let _singleflight_guard = singleflight
        .gate
        .lock()
        .map_err(|_| AppError::internal("FEM mesh singleflight lock was poisoned."))?;
    if execution_cancellation.load(Ordering::Acquire) {
        return Err(AppError::conflict(format!(
            "FEM mesh preview '{}' was cancelled while waiting for an identical run.",
            request.analysis_name
        )));
    }
    if let Some(asset) =
        load_fem_mesh_request_cache(app, &request_digest, request.budgets.result_bytes)?
    {
        progress(FemProgressEvent {
            stage: FemPipelineStage::Publish,
            elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            node_count: asset
                .arrays
                .iter()
                .find(|array| array.name == "nodesMm")
                .and_then(|array| array.shape.first())
                .copied(),
            tet4_cell_count: asset
                .arrays
                .iter()
                .find(|array| array.name == "tet4Cells")
                .and_then(|array| array.shape.first())
                .copied(),
            detail: "Loaded exact immutable FEM mesh cache; Gmsh HXT skipped.".to_string(),
            cancellation_boundary: true,
        });
        return subscriber_result(
            mesh_asset_response(&request, asset),
            cancellation,
            &request.analysis_name,
        );
    }
    let scratch = app.app_data_dir().join("fem-scratch").join(&request.job_id);
    if scratch.exists() {
        return Err(AppError::conflict(format!(
            "FEM scratch directory '{}' already exists.",
            scratch.display()
        )));
    }
    fs::create_dir_all(&scratch).map_err(|error| {
        AppError::persistence(format!(
            "FEM scratch directory '{}' could not be created: {error}",
            scratch.display()
        ))
    })?;
    let result = execute_fem_mesh_pipeline(
        &resolved.program,
        &request.analysis_name,
        &resolved.manifest.tagged_anchors,
        &resolved.boundary,
        &resolved.step_path,
        resolved.budgets,
        &runtime,
        &scratch,
        &resolved.control,
        None,
        execution_cancellation,
        &mut progress,
    );
    let cleanup_error = fs::remove_dir_all(&scratch).err();
    let result = result?;
    if let Some(error) = cleanup_error {
        return Err(AppError::persistence(format!(
            "FEM scratch cleanup '{}' failed: {error}",
            scratch.display()
        )));
    }
    let asset = publish_fem_mesh_asset(app, &result, request.budgets.result_bytes)?;
    store_fem_mesh_request_cache(app, &request_digest, &asset)?;
    progress(FemProgressEvent {
        stage: FemPipelineStage::Publish,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        node_count: Some(result.mesh.nodes.len() as u64),
        tet4_cell_count: Some(result.mesh.cells.len() as u64),
        detail: "Published immutable FEM mesh artifact.".to_string(),
        cancellation_boundary: true,
    });
    subscriber_result(
        mesh_asset_response(&request, asset),
        cancellation,
        &request.analysis_name,
    )
}

pub(crate) fn run_fem_study_with_resolver_and_mesh_size<F>(
    request: FemStudyRequest,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
    mesh_size_override_mm: Option<f64>,
    progress: F,
) -> AppResult<FemRunResponse>
where
    F: FnMut(FemProgressEvent),
{
    run_fem_study_with_resolver_and_mesh_size_and_subscription(
        request,
        app,
        cancellation,
        None,
        mesh_size_override_mm,
        progress,
    )
}

fn run_fem_study_with_resolver_and_mesh_size_and_subscription<F>(
    request: FemStudyRequest,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
    subscriber_cancellation: Option<Arc<AtomicBool>>,
    mesh_size_override_mm: Option<f64>,
    mut progress: F,
) -> AppResult<FemRunResponse>
where
    F: FnMut(FemProgressEvent),
{
    let started = Instant::now();
    let resolved = resolve_request(&request, app)?;
    let runtime = probe_system_exact_brep_mesher_runtime()?;
    let request_digest =
        fem_request_cache_digest(&request, &resolved, &runtime, mesh_size_override_mm)?;
    let singleflight = fem_singleflight_job(&request_digest)?;
    let subscription = subscriber_cancellation
        .map(|cancellation| singleflight.subscribe(cancellation))
        .transpose()?;
    let _cancellation_monitor = subscription
        .as_ref()
        .map(FemSharedSubscription::start_monitor);
    let execution_cancellation = subscription
        .as_ref()
        .map(|subscription| &subscription.job.execution_cancelled)
        .unwrap_or(cancellation);
    let _singleflight_guard = singleflight
        .gate
        .lock()
        .map_err(|_| AppError::internal("FEM singleflight lock was poisoned."))?;
    if execution_cancellation.load(Ordering::Acquire) {
        return Err(AppError::conflict(format!(
            "FEM study '{}' was cancelled while waiting for an identical run.",
            request.analysis_name
        )));
    }
    if let Some(asset) = load_fem_request_cache(app, &request_digest, request.budgets.result_bytes)?
    {
        progress(FemProgressEvent {
            stage: FemPipelineStage::Publish,
            elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            node_count: asset
                .arrays
                .iter()
                .find(|array| array.name == "nodesMm")
                .and_then(|array| array.shape.first())
                .copied(),
            tet4_cell_count: asset
                .arrays
                .iter()
                .find(|array| array.name == "tet4Cells")
                .and_then(|array| array.shape.first())
                .copied(),
            detail: "Loaded exact immutable FEM result cache; mesh and solve skipped.".to_string(),
            cancellation_boundary: true,
        });
        return subscriber_result(
            result_response(&request, asset),
            cancellation,
            &request.analysis_name,
        );
    }
    let scratch = app.app_data_dir().join("fem-scratch").join(&request.job_id);
    if scratch.exists() {
        return Err(AppError::conflict(format!(
            "FEM scratch directory '{}' already exists.",
            scratch.display()
        )));
    }
    fs::create_dir_all(&scratch).map_err(|error| {
        AppError::persistence(format!(
            "FEM scratch directory '{}' could not be created: {error}",
            scratch.display()
        ))
    })?;
    let result = execute_fem_pipeline_with_mesh_size(
        &resolved.program,
        &request.analysis_name,
        &resolved.manifest.tagged_anchors,
        &resolved.boundary,
        &resolved.step_path,
        resolved.budgets,
        &runtime,
        &scratch,
        &resolved.control,
        mesh_size_override_mm,
        execution_cancellation,
        &mut progress,
    );
    let cleanup_error = fs::remove_dir_all(&scratch).err();
    let result = result?;
    if let Some(error) = cleanup_error {
        return Err(AppError::persistence(format!(
            "FEM scratch cleanup '{}' failed: {error}",
            scratch.display()
        )));
    }
    let source_digest = crate::services::render_snapshot::canonical_source_digest(&request.source);
    let asset =
        publish_fem_result_asset(app, &result, &source_digest, request.budgets.result_bytes)?;
    store_fem_request_cache(app, &request_digest, &asset)?;
    progress(FemProgressEvent {
        stage: FemPipelineStage::Publish,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        node_count: Some(result.mesh.nodes.len() as u64),
        tet4_cell_count: Some(result.mesh.cells.len() as u64),
        detail: "Published immutable FEM result artifact.".to_string(),
        cancellation_boundary: true,
    });
    subscriber_result(
        result_response(&request, asset),
        cancellation,
        &request.analysis_name,
    )
}

fn subscriber_result<T>(value: T, cancellation: &AtomicBool, analysis_name: &str) -> AppResult<T> {
    if cancellation.load(Ordering::Acquire) {
        return Err(AppError::conflict(format!(
            "FEM subscriber for study '{analysis_name}' was cancelled."
        )));
    }
    Ok(value)
}

fn fem_request_cache_digest(
    request: &FemStudyRequest,
    resolved: &ResolvedFemRequest,
    runtime: &ExactBrepMesherRuntime,
    mesh_size_override_mm: Option<f64>,
) -> AppResult<String> {
    let step_sha256 = sha256_file(&resolved.step_path)?;
    fem_request_cache_digest_components(
        request,
        &resolved.boundary.content_digest,
        &resolved.boundary.source_geometry_digest,
        &step_sha256,
        &resolved.manifest.tagged_anchors,
        runtime,
        mesh_size_override_mm,
    )
}

fn fem_request_cache_digest_components<T: Serialize>(
    request: &FemStudyRequest,
    boundary_digest: &str,
    source_geometry_digest: &str,
    step_sha256: &str,
    tagged_anchors: &T,
    runtime: &ExactBrepMesherRuntime,
    mesh_size_override_mm: Option<f64>,
) -> AppResult<String> {
    let value = serde_json::json!({
        "schemaVersion": FEM_REQUEST_CACHE_SCHEMA_VERSION,
        "modelId": request.model_id,
        "sourceDigest": crate::services::render_snapshot::canonical_source_digest(&request.source),
        "analysisName": request.analysis_name,
        "boundaryDigest": boundary_digest,
        "sourceGeometryDigest": source_geometry_digest,
        "stepSha256": step_sha256,
        "taggedAnchors": tagged_anchors,
        "budgets": request.budgets,
        "control": request.control,
        "meshSizeOverrideMm": mesh_size_override_mm,
        "runtime": {
            "runtimeName": "exact-BRep HXT plus Netgen fallback",
            "runtimeVersion": runtime.gmsh.version,
            "platform": runtime.gmsh.platform,
            "arch": runtime.gmsh.architecture,
            "adapterProtocolVersion": 1,
            "executableSha256": runtime.gmsh.executable_sha256,
            "netgenVersion": runtime.netgen.as_ref().map(|value| &value.version),
            "netgenRuntimeDigest": runtime.netgen.as_ref().map(|value| &value.runtime_digest),
            "distributionBoundary": "external-not-redistributed",
        }
    });
    let bytes = serde_json::to_vec(&value).map_err(|error| {
        AppError::internal(format!(
            "FEM request cache identity serialization failed: {error}"
        ))
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn fem_singleflight_job(request_digest: &str) -> AppResult<Arc<FemSharedJob>> {
    let registry = FEM_RUN_SINGLEFLIGHT.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut registry = registry
        .lock()
        .map_err(|_| AppError::internal("FEM singleflight registry was poisoned."))?;
    if registry.len() >= FEM_SINGLEFLIGHT_LIMIT {
        registry.retain(|_, job| Arc::strong_count(job) > 1);
    }
    if registry.len() >= FEM_SINGLEFLIGHT_LIMIT && !registry.contains_key(request_digest) {
        return Err(AppError::conflict(format!(
            "FEM singleflight budget exceeded: observed {}, allowed {FEM_SINGLEFLIGHT_LIMIT}.",
            registry.len() + 1
        )));
    }
    let replace_cancelled_generation = registry
        .get(request_digest)
        .is_some_and(|job| job.execution_cancelled.load(Ordering::Acquire));
    if replace_cancelled_generation {
        registry.insert(request_digest.to_string(), Arc::new(FemSharedJob::new()));
    }
    Ok(registry
        .entry(request_digest.to_string())
        .or_insert_with(|| Arc::new(FemSharedJob::new()))
        .clone())
}

fn load_fem_request_cache(
    app: &dyn PathResolver,
    request_digest: &str,
    maximum_result_bytes: u64,
) -> AppResult<Option<FemResultAsset>> {
    let path = fem_request_cache_path(app, request_digest)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| {
        AppError::persistence(format!(
            "FEM request cache '{}' read failed: {error}",
            path.display()
        ))
    })?;
    if bytes.len() > 16 * 1024 {
        return Err(AppError::validation(
            "FEM request cache entry exceeds 16 KiB.",
        ));
    }
    let entry: FemRequestCacheEntry = crate::strict_edn::from_slice(&bytes).map_err(|error| {
        AppError::validation(format!(
            "FEM request cache '{}' is invalid: {error}",
            path.display()
        ))
    })?;
    if entry.schema_version != FEM_REQUEST_CACHE_SCHEMA_VERSION
        || entry.request_digest != request_digest
    {
        return Err(AppError::conflict("FEM request cache identity is stale."));
    }
    load_fem_result_asset(
        app,
        &entry.analysis_identity_digest,
        &entry.solution_digest,
        maximum_result_bytes,
    )
    .map(Some)
}

fn load_fem_mesh_request_cache(
    app: &dyn PathResolver,
    request_digest: &str,
    maximum_result_bytes: u64,
) -> AppResult<Option<FemMeshAsset>> {
    let path = fem_cache_path(app, FEM_MESH_REQUEST_CACHE_ROOT, request_digest)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| {
        AppError::persistence(format!(
            "FEM mesh request cache '{}' read failed: {error}",
            path.display()
        ))
    })?;
    if bytes.len() > 16 * 1024 {
        return Err(AppError::validation(
            "FEM mesh request cache entry exceeds 16 KiB.",
        ));
    }
    let entry: FemMeshRequestCacheEntry =
        crate::strict_edn::from_slice(&bytes).map_err(|error| {
            AppError::validation(format!(
                "FEM mesh request cache '{}' is invalid: {error}",
                path.display()
            ))
        })?;
    if entry.schema_version != FEM_REQUEST_CACHE_SCHEMA_VERSION
        || entry.request_digest != request_digest
    {
        return Err(AppError::conflict(
            "FEM mesh request cache identity is stale.",
        ));
    }
    load_fem_mesh_asset(
        app,
        &entry.analysis_identity_digest,
        &entry.mesh_content_digest,
        maximum_result_bytes,
    )
    .map(Some)
}

fn store_fem_request_cache(
    app: &dyn PathResolver,
    request_digest: &str,
    asset: &FemResultAsset,
) -> AppResult<()> {
    let path = fem_request_cache_path(app, request_digest)?;
    let parent = path.parent().expect("FEM request cache path has parent");
    fs::create_dir_all(parent).map_err(|error| {
        AppError::persistence(format!(
            "FEM request cache root '{}' create failed: {error}",
            parent.display()
        ))
    })?;
    let bytes = crate::strict_edn::to_vec(&FemRequestCacheEntry {
        schema_version: FEM_REQUEST_CACHE_SCHEMA_VERSION,
        request_digest: request_digest.to_string(),
        analysis_identity_digest: asset.analysis_identity_digest.clone(),
        solution_digest: asset.solution_digest.clone(),
    })
    .map_err(|error| {
        AppError::internal(format!("FEM request cache serialization failed: {error}"))
    })?;
    let temporary = parent.join(format!(
        ".publishing-{}-{}",
        std::process::id(),
        FEM_REQUEST_CACHE_NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::File::create(&temporary).map_err(|error| {
        AppError::persistence(format!(
            "FEM request cache temporary create failed: {error}"
        ))
    })?;
    file.write_all(&bytes).map_err(|error| {
        AppError::persistence(format!("FEM request cache temporary write failed: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        AppError::persistence(format!("FEM request cache temporary sync failed: {error}"))
    })?;
    fs::rename(&temporary, &path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        AppError::persistence(format!("FEM request cache atomic publish failed: {error}"))
    })?;
    prune_fem_cache_directory(
        parent,
        FEM_REQUEST_CACHE_ENTRY_LIMIT,
        FEM_REQUEST_CACHE_BYTE_LIMIT,
    )
}

fn store_fem_mesh_request_cache(
    app: &dyn PathResolver,
    request_digest: &str,
    asset: &FemMeshAsset,
) -> AppResult<()> {
    let bytes = crate::strict_edn::to_vec(&FemMeshRequestCacheEntry {
        schema_version: FEM_REQUEST_CACHE_SCHEMA_VERSION,
        request_digest: request_digest.to_string(),
        analysis_identity_digest: asset.analysis_identity_digest.clone(),
        mesh_content_digest: asset.mesh_content_digest.clone(),
    })
    .map_err(|error| {
        AppError::internal(format!(
            "FEM mesh request cache serialization failed: {error}"
        ))
    })?;
    store_fem_cache_bytes(app, FEM_MESH_REQUEST_CACHE_ROOT, request_digest, &bytes)
}

fn fem_request_cache_path(app: &dyn PathResolver, request_digest: &str) -> AppResult<PathBuf> {
    fem_cache_path(app, FEM_REQUEST_CACHE_ROOT, request_digest)
}

fn fem_cache_path(app: &dyn PathResolver, root: &str, request_digest: &str) -> AppResult<PathBuf> {
    let hex = request_digest.strip_prefix("sha256:").ok_or_else(|| {
        AppError::validation("FEM request cache identity must use sha256 prefix.")
    })?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::validation(
            "FEM request cache identity must contain 64 hexadecimal characters.",
        ));
    }
    Ok(app.app_data_dir().join(root).join(format!("{hex}.edn")))
}

fn store_fem_cache_bytes(
    app: &dyn PathResolver,
    root: &str,
    request_digest: &str,
    bytes: &[u8],
) -> AppResult<()> {
    let path = fem_cache_path(app, root, request_digest)?;
    let parent = path.parent().expect("FEM cache path has parent");
    fs::create_dir_all(parent).map_err(|error| {
        AppError::persistence(format!(
            "FEM cache root '{}' create failed: {error}",
            parent.display()
        ))
    })?;
    let temporary = parent.join(format!(
        ".publishing-{}-{}",
        std::process::id(),
        FEM_REQUEST_CACHE_NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::File::create(&temporary).map_err(|error| {
        AppError::persistence(format!("FEM cache temporary create failed: {error}"))
    })?;
    file.write_all(bytes).map_err(|error| {
        AppError::persistence(format!("FEM cache temporary write failed: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        AppError::persistence(format!("FEM cache temporary sync failed: {error}"))
    })?;
    fs::rename(&temporary, &path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        AppError::persistence(format!("FEM cache atomic publish failed: {error}"))
    })?;
    prune_fem_cache_directory(
        parent,
        FEM_REQUEST_CACHE_ENTRY_LIMIT,
        FEM_REQUEST_CACHE_BYTE_LIMIT,
    )
}

fn prune_fem_cache_directory(
    directory: &std::path::Path,
    maximum_entries: usize,
    maximum_bytes: u64,
) -> AppResult<()> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| {
            AppError::persistence(format!(
                "FEM cache root '{}' read failed: {error}",
                directory.display()
            ))
        })?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("edn") {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
            Some((modified, path, metadata.len()))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut retained_bytes = 0_u64;
    for (index, (_, path, bytes)) in entries.into_iter().enumerate() {
        let retain = index < maximum_entries
            && retained_bytes
                .checked_add(bytes)
                .is_some_and(|total| total <= maximum_bytes);
        if retain {
            retained_bytes += bytes;
        } else {
            fs::remove_file(&path).map_err(|error| {
                AppError::persistence(format!(
                    "FEM cache entry '{}' prune failed: {error}",
                    path.display()
                ))
            })?;
        }
    }
    Ok(())
}

pub(crate) fn run_fem_convergence_with_resolver<F>(
    request: FemConvergenceRequest,
    app: &dyn PathResolver,
    cancellation: &AtomicBool,
    mut progress: F,
) -> AppResult<FemConvergenceResponse>
where
    F: FnMut(FemProgressEvent),
{
    validate_convergence_request(&request)?;
    let request_digest = fem_convergence_cache_digest(&request)?;
    let response = run_fem_convergence_sequence(request, cancellation, |study, mesh_size_mm| {
        run_fem_study_with_resolver_and_mesh_size(
            study,
            app,
            cancellation,
            Some(mesh_size_mm),
            &mut progress,
        )
    })?;
    store_fem_convergence_cache(app, &request_digest, &response)?;
    Ok(response)
}

pub(crate) fn get_cached_fem_convergence_with_resolver(
    request: FemConvergenceRequest,
    app: &dyn PathResolver,
) -> AppResult<Option<FemConvergenceResponse>> {
    validate_convergence_request(&request)?;
    let request_digest = fem_convergence_cache_digest(&request)?;
    if let Some(response) = load_fem_convergence_cache(app, &request_digest)? {
        return Ok(Some(response));
    }

    let runtime = probe_system_exact_brep_mesher_runtime()?;
    let resolved = resolve_request(&request.study, app)?;
    let mut cached_levels = Vec::with_capacity(request.mesh_sizes_mm.len());
    for (index, mesh_size_mm) in request.mesh_sizes_mm.iter().copied().enumerate() {
        let mut study = request.study.clone();
        study.job_id = format!("{}-cache-level-{}", request.study.job_id, index + 1);
        let level_digest =
            fem_request_cache_digest(&study, &resolved, &runtime, Some(mesh_size_mm))?;
        let Some(asset) = load_fem_request_cache(app, &level_digest, study.budgets.result_bytes)?
        else {
            return Ok(None);
        };
        cached_levels.push(result_response(&study, asset));
    }

    let mut cached_levels = cached_levels.into_iter();
    run_fem_convergence_sequence(request, &AtomicBool::new(false), |_study, _mesh_size_mm| {
        cached_levels.next().ok_or_else(|| {
            AppError::internal("FEM convergence cache level sequence ended unexpectedly.")
        })
    })
    .map(Some)
}

fn fem_convergence_cache_digest(request: &FemConvergenceRequest) -> AppResult<String> {
    let mut identity = request.clone();
    identity.study.job_id.clear();
    identity.study.source =
        crate::services::render_snapshot::canonical_source_digest(&identity.study.source);
    let bytes = crate::strict_edn::to_vec(&identity).map_err(|error| {
        AppError::internal(format!(
            "FEM convergence cache identity serialization failed: {error}"
        ))
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn load_fem_convergence_cache(
    app: &dyn PathResolver,
    request_digest: &str,
) -> AppResult<Option<FemConvergenceResponse>> {
    let path = fem_cache_path(app, FEM_CONVERGENCE_CACHE_ROOT, request_digest)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| {
        AppError::persistence(format!(
            "FEM convergence cache '{}' read failed: {error}",
            path.display()
        ))
    })?;
    if bytes.len() as u64 > FEM_REQUEST_CACHE_BYTE_LIMIT {
        return Err(AppError::validation(
            "FEM convergence cache entry exceeds 2 MiB.",
        ));
    }
    let entry: FemConvergenceCacheEntry =
        crate::strict_edn::from_slice(&bytes).map_err(|error| {
            AppError::validation(format!(
                "FEM convergence cache '{}' is invalid: {error}",
                path.display()
            ))
        })?;
    if entry.schema_version != 1 || entry.request_digest != request_digest {
        return Err(AppError::conflict(
            "FEM convergence cache identity is stale.",
        ));
    }
    Ok(Some(entry.response))
}

fn store_fem_convergence_cache(
    app: &dyn PathResolver,
    request_digest: &str,
    response: &FemConvergenceResponse,
) -> AppResult<()> {
    let bytes = crate::strict_edn::to_vec(&FemConvergenceCacheEntry {
        schema_version: 1,
        request_digest: request_digest.to_string(),
        response: response.clone(),
    })
    .map_err(|error| {
        AppError::internal(format!(
            "FEM convergence cache serialization failed: {error}"
        ))
    })?;
    if bytes.len() as u64 > FEM_REQUEST_CACHE_BYTE_LIMIT {
        return Err(AppError::validation(
            "FEM convergence cache entry exceeds 2 MiB.",
        ));
    }
    store_fem_cache_bytes(app, FEM_CONVERGENCE_CACHE_ROOT, request_digest, &bytes)
}

fn run_fem_convergence_sequence<R>(
    request: FemConvergenceRequest,
    cancellation: &AtomicBool,
    mut run_level: R,
) -> AppResult<FemConvergenceResponse>
where
    R: FnMut(FemStudyRequest, f64) -> AppResult<FemRunResponse>,
{
    let base_job_id = request.study.job_id.clone();
    let model_id = request.study.model_id.clone();
    let analysis_name = request.study.analysis_name.clone();
    let mut levels = Vec::with_capacity(request.mesh_sizes_mm.len());
    let mut latest_acceptance_evaluations = Vec::new();
    let mut sequence_status = "completed".to_string();
    for (index, mesh_size_mm) in request.mesh_sizes_mm.iter().copied().enumerate() {
        if cancellation.load(Ordering::Acquire) {
            sequence_status = "cancelled".to_string();
            levels.push(failed_convergence_level(
                mesh_size_mm,
                "cancelled",
                format!(
                    "FEM convergence '{base_job_id}' was cancelled before level {}.",
                    index + 1
                ),
            ));
            break;
        }
        let mut study = request.study.clone();
        study.job_id = format!("{base_job_id}-level-{}", index + 1);
        let response = match run_level(study, mesh_size_mm) {
            Ok(response) => response,
            Err(error) => {
                let cancelled = cancellation.load(Ordering::Acquire);
                sequence_status = if cancelled { "cancelled" } else { "failed" }.to_string();
                levels.push(failed_convergence_level(
                    mesh_size_mm,
                    if cancelled { "cancelled" } else { "failed" },
                    error.to_string(),
                ));
                break;
            }
        };
        let previous = levels
            .iter()
            .rev()
            .find(|level| level.status == "completed");
        latest_acceptance_evaluations = response.acceptance_evaluations.clone();
        let displacement_relative_delta = previous.and_then(|level| {
            level
                .maximum_displacement_mm
                .map(|previous| relative_delta(previous, response.summary.maximum_displacement_mm))
        });
        let stress_relative_delta = previous.and_then(|level| {
            level
                .maximum_von_mises_mpa
                .map(|previous| relative_delta(previous, response.summary.maximum_von_mises_mpa))
        });
        levels.push(FemConvergenceLevelDto {
            mesh_size_mm,
            status: "completed".to_string(),
            error: None,
            analysis_identity_digest: Some(response.analysis_identity_digest),
            solution_digest: Some(response.solution_digest),
            result_digest: Some(response.result_digest),
            mesh_content_digest: Some(response.mesh_content_digest),
            node_count: Some(response.summary.node_count),
            tet4_cell_count: Some(response.summary.tet4_cell_count),
            minimum_scaled_jacobian: Some(response.summary.minimum_scaled_jacobian),
            equilibrium_relative_imbalance: Some(response.summary.equilibrium_relative_imbalance),
            solver_relative_residual: Some(response.summary.solver_relative_residual),
            maximum_displacement_mm: Some(response.summary.maximum_displacement_mm),
            maximum_von_mises_mpa: Some(response.summary.maximum_von_mises_mpa),
            displacement_relative_delta,
            stress_relative_delta,
        });
    }
    let failed = sequence_status != "completed";
    let displacement_status = if failed {
        "failed".to_string()
    } else {
        convergence_status(
            levels
                .iter()
                .filter_map(|level| level.displacement_relative_delta),
            request.displacement_relative_tolerance,
            false,
        )
    };
    let stress_values = levels
        .iter()
        .filter_map(|level| level.maximum_von_mises_mpa)
        .collect::<Vec<_>>();
    let stress_rising = stress_values.windows(2).all(|pair| pair[1] > pair[0]);
    let stress_status = if failed {
        "failed".to_string()
    } else {
        convergence_status(
            levels
                .iter()
                .filter_map(|level| level.stress_relative_delta),
            request.stress_relative_tolerance,
            stress_rising,
        )
    };
    let acceptance_evaluations = latest_acceptance_evaluations
        .into_iter()
        .map(|evaluation| {
            complete_converged_acceptance_evaluation(
                evaluation,
                &sequence_status,
                &displacement_status,
                &stress_status,
            )
        })
        .collect();
    Ok(FemConvergenceResponse {
        job_id: base_job_id,
        model_id,
        analysis_name,
        sequence_status,
        levels,
        displacement_status,
        stress_status,
        acceptance_evaluations,
    })
}

fn complete_converged_acceptance_evaluation(
    mut evaluation: FemAcceptanceEvaluationDto,
    sequence_status: &str,
    displacement_status: &str,
    stress_status: &str,
) -> FemAcceptanceEvaluationDto {
    if evaluation.status != "pending" {
        return evaluation;
    }
    let metric_status = match evaluation.field.as_str() {
        "maximum-displacement" | "maximumDisplacement" => Some(displacement_status),
        "von-mises-stress"
        | "vonMisesStress"
        | "maximum-principal-stress"
        | "maximumPrincipalStress" => Some(stress_status),
        _ => None,
    };
    let Some(metric_status) = metric_status else {
        evaluation.detail = format!(
            "FEM acceptance metric '{}' requires convergence, but field '{}' has no convergence series.",
            evaluation.metric_id, evaluation.field
        );
        return evaluation;
    };
    evaluation.convergence_status = Some(metric_status.to_string());
    evaluation.evidence_chain.convergence_status = Some(metric_status.to_string());
    if sequence_status != "completed" || metric_status != "converged" {
        evaluation.detail = format!(
            "FEM acceptance metric '{}' cannot pass: convergence sequence '{}', metric status '{}'.",
            evaluation.metric_id, sequence_status, metric_status
        );
        return evaluation;
    }
    let Some(observed) = evaluation.observed else {
        evaluation.status = "failed".to_string();
        evaluation.detail = format!(
            "FEM acceptance metric '{}' has no current numeric result.",
            evaluation.metric_id
        );
        return evaluation;
    };
    let passed = match evaluation.comparison.as_str() {
        "lessThanOrEqual" => observed <= evaluation.threshold,
        "greaterThanOrEqual" => observed >= evaluation.threshold,
        _ => false,
    };
    evaluation.status = if passed { "passed" } else { "failed" }.to_string();
    evaluation.detail = format!(
        "FEM acceptance metric '{}' {} after '{}' convergence: observed {} {}, threshold {} {}.",
        evaluation.metric_id,
        evaluation.status,
        metric_status,
        observed,
        evaluation.unit,
        evaluation.threshold,
        evaluation.unit
    );
    evaluation
}

fn failed_convergence_level(
    mesh_size_mm: f64,
    status: &str,
    error: String,
) -> FemConvergenceLevelDto {
    FemConvergenceLevelDto {
        mesh_size_mm,
        status: status.to_string(),
        error: Some(error),
        analysis_identity_digest: None,
        solution_digest: None,
        result_digest: None,
        mesh_content_digest: None,
        node_count: None,
        tet4_cell_count: None,
        minimum_scaled_jacobian: None,
        equilibrium_relative_imbalance: None,
        solver_relative_residual: None,
        maximum_displacement_mm: None,
        maximum_von_mises_mpa: None,
        displacement_relative_delta: None,
        stress_relative_delta: None,
    }
}

fn validate_convergence_request(request: &FemConvergenceRequest) -> AppResult<()> {
    validate_job_id(&request.study.job_id)?;
    if request.mesh_sizes_mm.len() < 3
        || request.mesh_sizes_mm.len() as u64 > request.study.budgets.convergence_levels
    {
        return Err(AppError::validation(format!(
            "FEM convergence requires 3..={} explicit mesh sizes; observed {}.",
            request.study.budgets.convergence_levels,
            request.mesh_sizes_mm.len()
        )));
    }
    if request
        .mesh_sizes_mm
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
        || request
            .mesh_sizes_mm
            .windows(2)
            .any(|pair| pair[1] >= pair[0])
    {
        return Err(AppError::validation(
            "FEM convergence meshSizesMm must be finite, positive, and strictly coarse-to-fine.",
        ));
    }
    for (name, tolerance) in [
        (
            "displacementRelativeTolerance",
            request.displacement_relative_tolerance,
        ),
        ("stressRelativeTolerance", request.stress_relative_tolerance),
    ] {
        if !tolerance.is_finite() || tolerance <= 0.0 || tolerance >= 1.0 {
            return Err(AppError::validation(format!(
                "FEM convergence {name} must be finite between 0 and 1."
            )));
        }
    }
    Ok(())
}

fn relative_delta(previous: f64, current: f64) -> f64 {
    (current - previous).abs() / current.abs().max(previous.abs()).max(f64::EPSILON)
}

fn convergence_status(
    deltas: impl Iterator<Item = f64>,
    tolerance: f64,
    suspected_singularity: bool,
) -> String {
    let deltas = deltas.collect::<Vec<_>>();
    if deltas.iter().all(|delta| *delta <= tolerance) {
        "converged".to_string()
    } else if suspected_singularity {
        "suspectedSingularity".to_string()
    } else {
        "unconverged".to_string()
    }
}

fn resolve_request(
    request: &FemStudyRequest,
    app: &dyn PathResolver,
) -> AppResult<ResolvedFemRequest> {
    validate_job_id(&request.job_id)?;
    if request.model_id.trim().is_empty()
        || request.analysis_name.trim().is_empty()
        || request.source.trim().is_empty()
    {
        return Err(AppError::validation(
            "FEM request requires modelId, source, and analysisName.",
        ));
    }
    let (_bundle, manifest) = crate::model_runtime::read_runtime_bundle(app, &request.model_id)?;
    let source_digest = crate::services::render_snapshot::canonical_source_digest(&request.source);
    if manifest.source_digest.as_deref() != Some(source_digest.as_str()) {
        return Err(AppError::conflict(format!(
            "FEM source is stale for model '{}': request digest '{}', manifest digest '{}'.",
            request.model_id,
            source_digest,
            manifest.source_digest.as_deref().unwrap_or("missing")
        )));
    }
    let program = crate::ecky_scheme::compile_to_core_program(&request.source)
        .map_err(|error| AppError::validation(format!("FEM source compilation failed: {error}")))?;
    let analyses = program
        .analyses
        .iter()
        .filter(|analysis| analysis.name == request.analysis_name)
        .collect::<Vec<_>>();
    let analysis = match analyses.as_slice() {
        [analysis] => *analysis,
        [] => {
            return Err(AppError::not_found(format!(
                "FEM analysis '{}' was not found.",
                request.analysis_name
            )))
        }
        _ => {
            return Err(AppError::validation(format!(
                "FEM analysis '{}' is duplicate.",
                request.analysis_name
            )))
        }
    };
    let bundle_dir = crate::model_runtime::runtime_bundle_dir(app, &request.model_id)?;
    let step_path = bundle_dir.join("model.step");
    if !step_path.is_file() {
        return Err(AppError::validation(format!(
            "Exact-BRep FEM requires generated STEP '{}'.",
            step_path.display()
        )));
    }
    let boundary = load_direct_occt_analysis_boundary_surface(&bundle_dir, &analysis.part)?;
    let budgets = FemBudgetLimits {
        schema_version: FEM_SCHEMA_VERSION,
        boundary_triangles: request.budgets.boundary_triangles,
        tet4_cells: request.budgets.tet4_cells,
        nodes: request.budgets.nodes,
        dofs: request.budgets.dofs,
        sparse_nonzeros: request.budgets.sparse_nonzeros,
        result_bytes: request.budgets.result_bytes,
        convergence_levels: request.budgets.convergence_levels,
    };
    budgets
        .validate()
        .map_err(|error| AppError::validation(format!("FEM budgets are invalid: {error}")))?;
    let control = FemPipelineControl {
        envelope_mm: request.control.envelope_mm,
        minimum_scaled_jacobian: request.control.minimum_scaled_jacobian,
        maximum_runtime_ms: request.control.maximum_runtime_ms,
        relative_solver_tolerance: request.control.relative_solver_tolerance,
        thread_count: resolved_fem_thread_count(request.control.thread_count),
    };
    control.validate()?;
    Ok(ResolvedFemRequest {
        program,
        boundary,
        step_path,
        manifest,
        budgets,
        control,
    })
}

fn resolved_fem_thread_count(requested: u32) -> u32 {
    let available = std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
        .clamp(1, 64);
    if requested == 0 {
        available
    } else {
        requested.clamp(1, available)
    }
}

fn result_response(request: &FemStudyRequest, asset: FemResultAsset) -> FemRunResponse {
    let response = asset_response(asset);
    FemRunResponse {
        job_id: request.job_id.clone(),
        model_id: request.model_id.clone(),
        analysis_name: request.analysis_name.clone(),
        source_digest: response.source_digest.clone(),
        analysis_identity_digest: response.analysis_identity_digest,
        solution_digest: response.solution_digest,
        result_digest: response.result_digest,
        mesh_content_digest: response.mesh_content_digest,
        source_boundary_digest: response.source_boundary_digest,
        decision_ready: response.decision_ready,
        decision_readiness_error: response.decision_readiness_error,
        manifest_path: response.manifest_path,
        arrays: response.arrays,
        summary: response.summary,
        support_reactions: response.support_reactions,
        engineering_evidence: response.engineering_evidence,
        acceptance_evaluations: response.acceptance_evaluations,
    }
}

fn asset_response(asset: FemResultAsset) -> FemResultReadResponse {
    let asset_dir = asset
        .manifest_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let minimum_yield_safety_factor = match asset.summary.minimum_yield_safety_factor {
        FemSafetyFactor::Finite { value } => Some(value),
        FemSafetyFactor::Infinite => None,
    };
    let node_count = asset
        .arrays
        .iter()
        .find(|array| array.name == "nodesMm")
        .and_then(|array| array.shape.first())
        .copied()
        .unwrap_or(0);
    let tet4_cell_count = asset
        .arrays
        .iter()
        .find(|array| array.name == "tet4Cells")
        .and_then(|array| array.shape.first())
        .copied()
        .unwrap_or(0);
    let engineering_evidence = engineering_evidence_response(
        &asset.engineering_evidence,
        &asset.idealization_artifact,
        &asset.idealization_artifact_digest,
    );
    FemResultReadResponse {
        source_digest: asset.source_digest,
        analysis_identity_digest: asset.analysis_identity_digest,
        solution_digest: asset.solution_digest,
        result_digest: asset.result_digest,
        mesh_content_digest: asset.mesh_content_digest,
        source_boundary_digest: asset.source_boundary_digest,
        decision_ready: asset.decision_ready,
        decision_readiness_error: asset.decision_readiness_error,
        manifest_path: asset.manifest_path.to_string_lossy().into_owned(),
        arrays: asset
            .arrays
            .into_iter()
            .map(|array| FemResultArrayDto {
                name: array.name,
                path: asset_dir.join(array.path).to_string_lossy().into_owned(),
                scalar_type: match array.scalar_type {
                    FemScalarType::Float64Le => "float64Le".to_string(),
                    FemScalarType::Uint32Le => "uint32Le".to_string(),
                },
                shape: array.shape,
                byte_length: array.byte_length,
                sha256: array.sha256,
            })
            .collect(),
        summary: FemResultSummaryDto {
            maximum_displacement_mm: asset.summary.maximum_displacement.value,
            maximum_von_mises_mpa: asset.summary.maximum_von_mises.value,
            maximum_principal_stress_mpa: asset.summary.maximum_principal_stress.value,
            volume_mm3: asset.summary.volume_mm3,
            mass_kg: asset.summary.mass_kg,
            minimum_yield_safety_factor,
            equilibrium_relative_imbalance: asset.equilibrium_relative_imbalance,
            solver_relative_residual: asset.solver_relative_residual,
            minimum_scaled_jacobian: asset.mesh_quality.minimum_scaled_jacobian,
            node_count,
            tet4_cell_count,
            extrema: [
                &asset.summary.maximum_displacement,
                &asset.summary.maximum_von_mises,
                &asset.summary.maximum_principal_stress,
            ]
            .into_iter()
            .map(extremum_response)
            .collect(),
        },
        support_reactions: asset
            .support_reactions
            .into_iter()
            .map(|reaction| FemSupportReactionDto {
                name: reaction.name,
                face_group_indices: reaction.face_group_indices,
                resultant_n: reaction.resultant_n,
            })
            .collect(),
        engineering_evidence,
        acceptance_evaluations: asset
            .acceptance_evaluations
            .into_iter()
            .map(|evaluation| {
                let evidence_chain =
                    acceptance_evidence_chain(&asset.engineering_evidence, &evaluation);
                FemAcceptanceEvaluationDto {
                    study_name: evaluation.study_name,
                    metric_id: evaluation.metric_id,
                    field: evaluation.field,
                    status: evaluation.status,
                    observed: evaluation.observed,
                    unit: evaluation.unit,
                    threshold: evaluation.threshold,
                    comparison: evaluation.comparison,
                    mesh_size_mm: evaluation.mesh_size_mm,
                    node_id: evaluation.node_id,
                    element_id: evaluation.element_id,
                    coordinate_mm: evaluation.coordinate_mm,
                    analysis_identity_digest: evaluation.analysis_identity_digest,
                    mesh_content_digest: evaluation.mesh_content_digest,
                    result_digest: evaluation.result_digest,
                    convergence_status: evaluation.convergence_status,
                    evidence_chain,
                    detail: evaluation.detail,
                }
            })
            .collect(),
    }
}

fn engineering_evidence_response(
    ledger: &FemEngineeringEvidenceLedger,
    idealization_artifact: &FemIdealizationArtifact,
    idealization_artifact_digest: &str,
) -> FemEngineeringEvidenceDto {
    let inputs = ledger
        .input_bindings
        .iter()
        .filter_map(|binding| {
            ledger
                .evidence
                .iter()
                .find(|record| record.evidence_id == binding.evidence_id)
                .map(|record| FemInputEvidenceDto {
                    input_name: binding.input_name.clone(),
                    evidence_id: binding.evidence_id.clone(),
                    subject: evidence_subject_name(record.subject).into(),
                    source: record.source.clone(),
                    authority: evidence_authority_name(record.authority).into(),
                    uncertainty_percent: record.uncertainty_percent,
                    decision_critical: record.decision_critical,
                })
        })
        .collect();
    let validation_evidence = ledger
        .validation_evidence
        .iter()
        .map(|evidence| FemValidationEvidenceDto {
            validation_id: evidence.validation_id.clone(),
            kind: validation_kind_name(evidence.kind).into(),
            source: evidence.source.clone(),
            result_digest: evidence.result_digest.clone(),
        })
        .collect::<Vec<_>>();
    FemEngineeringEvidenceDto {
        question: FemEngineeringQuestionDto {
            question_id: ledger.question.question_id.clone(),
            statement: ledger.question.statement.clone(),
            decision: ledger.question.decision.clone(),
            acceptance_metric_ids: ledger.question.acceptance_metric_ids.clone(),
        },
        idealization: FemIdealizationDto {
            artifact_digest: idealization_artifact_digest.to_string(),
            kind: match idealization_artifact.kind {
                FemIdealizationKind::ExactSolid => "exactSolid",
                FemIdealizationKind::DefeaturedSolid => "defeaturedSolid",
            }
            .into(),
            source_geometry_digest: ledger.idealization.source_geometry_digest.clone(),
            analysis_geometry_digest: ledger.idealization.analysis_geometry_digest.clone(),
            manufacturing_geometry_digest: idealization_artifact
                .manufacturing_geometry_digest
                .clone(),
            affected_topology_ids: ledger.idealization.affected_topology_ids.clone(),
            justification: ledger.idealization.justification.clone(),
            expected_influence_percent: ledger.idealization.expected_influence_percent,
            accepted_by_user: ledger.idealization.accepted_by_user,
        },
        inputs,
        assumptions: ledger
            .assumptions
            .iter()
            .map(|assumption| FemAssumptionDto {
                assumption_id: assumption.assumption_id.clone(),
                category: assumption_category_name(assumption.category).into(),
                statement: assumption.statement.clone(),
                status: assumption_status_name(assumption.status).into(),
                evidence_ids: assumption.evidence_ids.clone(),
            })
            .collect(),
        applicability: ledger
            .applicability_checks
            .iter()
            .map(|check| FemApplicabilityCheckDto {
                check_id: check.check_id.clone(),
                kind: applicability_kind_name(check.kind).into(),
                status: applicability_status_name(check.status).into(),
                observed: check.observed,
                limit: check.limit,
                unit: check.unit.clone(),
                evidence_ids: check.evidence_ids.clone(),
                detail: check.detail.clone(),
            })
            .collect(),
        sensitivity: ledger
            .sensitivity
            .as_ref()
            .map(|sensitivity| FemSensitivityEvidenceDto {
                completed: sensitivity.completed,
                case_result_digests: sensitivity.case_result_digests.clone(),
                metric_ranges: sensitivity
                    .metric_ranges
                    .iter()
                    .map(|metric| FemSensitivityMetricDto {
                        metric_id: metric.metric_id.clone(),
                        nominal: metric.nominal,
                        minimum: metric.minimum,
                        maximum: metric.maximum,
                        unit: metric.unit.clone(),
                        dominant_input_name: metric.dominant_input_name.clone(),
                        decision_changed: metric.decision_changed,
                    })
                    .collect(),
            }),
        validation_evidence,
        verification_layers: verification_layers(ledger),
    }
}

fn acceptance_evidence_chain(
    ledger: &FemEngineeringEvidenceLedger,
    evaluation: &crate::services::fem::FemAcceptanceEvaluation,
) -> FemAcceptanceEvidenceChainDto {
    let mut gaps = Vec::new();
    if !ledger.idealization.accepted_by_user {
        gaps.push("analysis idealization is not user accepted".into());
    }
    for evidence in &ledger.evidence {
        if matches!(
            evidence.authority,
            FemEvidenceAuthority::Unknown | FemEvidenceAuthority::Proposed
        ) {
            gaps.push(format!(
                "input evidence '{}' is not authoritative",
                evidence.evidence_id
            ));
        }
    }
    for check in &ledger.applicability_checks {
        if matches!(
            check.status,
            FemApplicabilityStatus::Blocked | FemApplicabilityStatus::NotEvaluated
        ) {
            gaps.push(format!(
                "applicability check '{}' is not passing",
                check.check_id
            ));
        }
    }
    if evaluation.status == "pending" {
        gaps.push("acceptance metric requires unresolved current evidence".into());
    }
    if !ledger.validation_evidence.iter().any(|evidence| {
        matches!(
            evidence.kind,
            FemValidationEvidenceKind::QualifiedReference | FemValidationEvidenceKind::PhysicalTest
        )
    }) {
        gaps.push("physical or qualified-reference validation is missing".into());
    }
    FemAcceptanceEvidenceChainDto {
        source_geometry_digest: ledger.idealization.source_geometry_digest.clone(),
        analysis_geometry_digest: ledger.idealization.analysis_geometry_digest.clone(),
        idealization_accepted: ledger.idealization.accepted_by_user,
        input_evidence_ids: ledger
            .input_bindings
            .iter()
            .map(|binding| binding.evidence_id.clone())
            .collect(),
        applicability_check_ids: ledger
            .applicability_checks
            .iter()
            .map(|check| check.check_id.clone())
            .collect(),
        convergence_status: evaluation.convergence_status.clone(),
        sensitivity_result_digests: ledger
            .sensitivity
            .as_ref()
            .map(|sensitivity| sensitivity.case_result_digests.clone())
            .unwrap_or_default(),
        validation_evidence_ids: ledger
            .validation_evidence
            .iter()
            .map(|evidence| evidence.validation_id.clone())
            .collect(),
        gaps,
    }
}

fn verification_layers(ledger: &FemEngineeringEvidenceLedger) -> Vec<FemVerificationLayerDto> {
    let layer = |name: &str,
                 kind: Option<FemValidationEvidenceKind>,
                 missing_status: &str,
                 passed_detail: &str,
                 missing_detail: &str| {
        let evidence_ids = kind
            .map(|kind| {
                ledger
                    .validation_evidence
                    .iter()
                    .filter(|evidence| evidence.kind == kind)
                    .map(|evidence| evidence.validation_id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        FemVerificationLayerDto {
            layer: name.into(),
            status: if evidence_ids.is_empty() {
                missing_status.into()
            } else {
                "passed".into()
            },
            evidence_ids,
            detail: if kind.is_some_and(|kind| {
                ledger
                    .validation_evidence
                    .iter()
                    .any(|evidence| evidence.kind == kind)
            }) {
                passed_detail.into()
            } else {
                missing_detail.into()
            },
        }
    };
    vec![
        layer(
            "analyticalUnit",
            Some(FemValidationEvidenceKind::Analytical),
            "missing",
            "Analytical or unit-level mechanics proof is recorded.",
            "No result-bound analytical or unit proof is recorded.",
        ),
        layer(
            "differentialSolver",
            Some(FemValidationEvidenceKind::DifferentialSolver),
            "missing",
            "Independent solver comparison is recorded.",
            "No independent solver comparison is recorded.",
        ),
        layer(
            "meshConvergence",
            None,
            "pending",
            "Current mesh convergence evidence is attached.",
            "No current convergence sequence is attached to this single-run result.",
        ),
        {
            let evidence_ids = ledger
                .validation_evidence
                .iter()
                .filter(|evidence| {
                    matches!(
                        evidence.kind,
                        FemValidationEvidenceKind::QualifiedReference
                            | FemValidationEvidenceKind::PhysicalTest
                    )
                })
                .map(|evidence| evidence.validation_id.clone())
                .collect::<Vec<_>>();
            FemVerificationLayerDto {
                layer: "physicalReference".into(),
                status: if evidence_ids.is_empty() {
                    "missing".into()
                } else {
                    "passed".into()
                },
                detail: if evidence_ids.is_empty() {
                    "No physical or qualified-reference validation is recorded.".into()
                } else {
                    "Physical or qualified-reference validation is recorded.".into()
                },
                evidence_ids,
            }
        },
    ]
}

fn evidence_subject_name(value: FemEvidenceSubject) -> &'static str {
    match value {
        FemEvidenceSubject::Material => "material",
        FemEvidenceSubject::Load => "load",
        FemEvidenceSubject::Support => "support",
        FemEvidenceSubject::Connection => "connection",
        FemEvidenceSubject::Geometry => "geometry",
        FemEvidenceSubject::AcceptanceCriterion => "acceptanceCriterion",
    }
}

fn evidence_authority_name(value: FemEvidenceAuthority) -> &'static str {
    match value {
        FemEvidenceAuthority::Unknown => "unknown",
        FemEvidenceAuthority::Proposed => "proposed",
        FemEvidenceAuthority::UserAccepted => "userAccepted",
        FemEvidenceAuthority::RecordedSource => "recordedSource",
    }
}

fn assumption_category_name(value: FemStudyAssumptionCategory) -> &'static str {
    match value {
        FemStudyAssumptionCategory::Geometry => "geometry",
        FemStudyAssumptionCategory::Physics => "physics",
        FemStudyAssumptionCategory::Material => "material",
        FemStudyAssumptionCategory::Load => "load",
        FemStudyAssumptionCategory::Support => "support",
        FemStudyAssumptionCategory::Connection => "connection",
    }
}

fn assumption_status_name(value: FemStudyAssumptionStatus) -> &'static str {
    match value {
        FemStudyAssumptionStatus::Unknown => "unknown",
        FemStudyAssumptionStatus::Proposed => "proposed",
        FemStudyAssumptionStatus::Accepted => "accepted",
        FemStudyAssumptionStatus::Rejected => "rejected",
    }
}

fn applicability_kind_name(value: FemApplicabilityCheckKind) -> &'static str {
    match value {
        FemApplicabilityCheckKind::OneSolidScope => "oneSolidScope",
        FemApplicabilityCheckKind::UnsupportedInterfaces => "unsupportedInterfaces",
        FemApplicabilityCheckKind::ThinSlenderTet4Risk => "thinSlenderTet4Risk",
        FemApplicabilityCheckKind::NearIncompressibleLocking => "nearIncompressibleLocking",
        FemApplicabilityCheckKind::ConstraintRealism => "constraintRealism",
        FemApplicabilityCheckKind::ConcentratedLoadSingularity => "concentratedLoadSingularity",
        FemApplicabilityCheckKind::DisplacementRatio => "displacementRatio",
        FemApplicabilityCheckKind::ElasticRange => "elasticRange",
        FemApplicabilityCheckKind::HotspotStability => "hotspotStability",
        FemApplicabilityCheckKind::BoundaryConditionSingularity => "boundaryConditionSingularity",
    }
}

fn applicability_status_name(value: FemApplicabilityStatus) -> &'static str {
    match value {
        FemApplicabilityStatus::Pass => "pass",
        FemApplicabilityStatus::Warning => "warning",
        FemApplicabilityStatus::Blocked => "blocked",
        FemApplicabilityStatus::NotEvaluated => "notEvaluated",
    }
}

fn validation_kind_name(value: FemValidationEvidenceKind) -> &'static str {
    match value {
        FemValidationEvidenceKind::Analytical => "analytical",
        FemValidationEvidenceKind::DifferentialSolver => "differentialSolver",
        FemValidationEvidenceKind::QualifiedReference => "qualifiedReference",
        FemValidationEvidenceKind::PhysicalTest => "physicalTest",
    }
}

fn extremum_response(extremum: &FemResultExtremum) -> FemExtremumDto {
    FemExtremumDto {
        field_kind: match extremum.field_kind {
            FemResultFieldKind::DisplacementMagnitude => "displacementMagnitude",
            FemResultFieldKind::VonMisesStress => "vonMisesStress",
            FemResultFieldKind::PrincipalStressMaximum => "principalStressMaximum",
        }
        .to_string(),
        value: extremum.value,
        unit: extremum.unit.clone(),
        node_id: extremum.node_id,
        element_id: extremum.element_id,
        coordinate_mm: [
            extremum.coordinate_mm.x_mm,
            extremum.coordinate_mm.y_mm,
            extremum.coordinate_mm.z_mm,
        ],
        mesh_content_digest: extremum.mesh_content_digest.clone(),
        source_boundary_digest: extremum.source_boundary_digest.clone(),
    }
}

fn mesh_asset_response(request: &FemStudyRequest, asset: FemMeshAsset) -> FemMeshPreviewResponse {
    let asset_dir = asset
        .manifest_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let node_count = asset
        .arrays
        .iter()
        .find(|array| array.name == "nodesMm")
        .and_then(|array| array.shape.first())
        .copied()
        .unwrap_or(0);
    let tet4_cell_count = asset
        .arrays
        .iter()
        .find(|array| array.name == "tet4Cells")
        .and_then(|array| array.shape.first())
        .copied()
        .unwrap_or(0);
    let boundary_triangle_count = asset
        .arrays
        .iter()
        .find(|array| array.name == "boundaryTriangles")
        .and_then(|array| array.shape.first())
        .copied()
        .unwrap_or(0);
    FemMeshPreviewResponse {
        job_id: request.job_id.clone(),
        model_id: request.model_id.clone(),
        analysis_name: request.analysis_name.clone(),
        analysis_identity_digest: asset.analysis_identity_digest,
        mesh_content_digest: asset.mesh_content_digest,
        source_boundary_digest: asset.source_boundary_digest,
        manifest_path: asset.manifest_path.to_string_lossy().into_owned(),
        arrays: asset
            .arrays
            .into_iter()
            .map(|array| FemResultArrayDto {
                name: array.name,
                path: asset_dir.join(array.path).to_string_lossy().into_owned(),
                scalar_type: match array.scalar_type {
                    FemScalarType::Float64Le => "float64Le".to_string(),
                    FemScalarType::Uint32Le => "uint32Le".to_string(),
                },
                shape: array.shape,
                byte_length: array.byte_length,
                sha256: array.sha256,
            })
            .collect(),
        node_count,
        tet4_cell_count,
        boundary_triangle_count,
        face_group_count: asset.face_group_count as u64,
        minimum_scaled_jacobian: asset.mesh_quality.minimum_scaled_jacobian,
        minimum_radius_ratio: asset.mesh_quality.minimum_radius_ratio,
        connected_component_count: asset.mesh_quality.connected_component_count as u64,
        boundary_area_mm2_by_group: asset.mesh_quality.boundary_area_mm2_by_group,
    }
}

fn validate_job_id(job_id: &str) -> AppResult<()> {
    if job_id.is_empty()
        || job_id.len() > 128
        || !job_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err(AppError::validation(
            "FEM jobId must contain 1-128 ASCII letters, digits, '-' or '_'.",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.join("config")
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.join("data")
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    #[test]
    fn run_intent_owns_job_identity_and_compute_policy() {
        let first_job_id = next_fem_run_job_id();
        let second_job_id = next_fem_run_job_id();
        assert_ne!(first_job_id, second_job_id);
        validate_job_id(&first_job_id).expect("Rust-owned job id");

        let input = crate::contracts::FemRunIntentInput {
            model_id: "model-current".into(),
            source: "(model (analysis bracket-static))".into(),
            analysis_name: "bracket-static".into(),
        };
        let compute = FemComputeConfig {
            quality: crate::contracts::FemComputeQuality::Draft,
            maximum_wall_time_minutes: 7,
            maximum_memory_mib: 2_048,
            thread_count: 3,
        };

        let request = fem_study_request_from_intent(input, &compute, "fem-run-owned-id".into());

        assert_eq!(request.job_id, "fem-run-owned-id");
        assert_eq!(request.model_id, "model-current");
        assert_eq!(request.analysis_name, "bracket-static");
        assert_eq!(request.budgets.tet4_cells, compute.maximum_fem_elements());
        assert_eq!(request.budgets.nodes, compute.maximum_fem_nodes());
        assert_eq!(request.budgets.dofs, compute.maximum_fem_dofs());
        assert_eq!(request.control.maximum_runtime_ms, 7 * 60_000);
        assert_eq!(request.control.thread_count, 3);
    }

    #[test]
    fn convergence_intent_owns_job_identity_tolerances_and_compute_policy() {
        let compute = FemComputeConfig {
            quality: crate::contracts::FemComputeQuality::Draft,
            maximum_wall_time_minutes: 4,
            maximum_memory_mib: 1_024,
            thread_count: 2,
        };
        let request = fem_convergence_request_from_intent(
            crate::contracts::FemConvergenceIntentInput {
                model_id: "model-current".into(),
                source: "(model (analysis bracket-static))".into(),
                analysis_name: "bracket-static".into(),
                mesh_sizes_mm: vec![4.0, 2.0, 1.0],
            },
            &compute,
            next_fem_job_id("convergence"),
        );

        validate_job_id(&request.study.job_id).expect("Rust-owned convergence job id");
        assert!(request.study.job_id.starts_with("fem-convergence-"));
        assert_eq!(request.study.control.maximum_runtime_ms, 4 * 60_000);
        assert_eq!(request.study.control.thread_count, 2);
        assert_eq!(
            request.study.budgets.tet4_cells,
            compute.maximum_fem_elements()
        );
        assert_eq!(request.displacement_relative_tolerance, 0.03);
        assert_eq!(request.stress_relative_tolerance, 0.05);
    }

    #[test]
    fn indexed_region_selection_matches_exhaustive_triangle_distance() {
        let triangles = vec![
            [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
            [[4.0, 4.0, 1.0], [7.0, 4.0, 1.0], [4.0, 7.0, 1.0]],
            [[-3.0, -2.0, -1.0], [-1.0, -2.0, 2.0], [-2.0, 1.0, 0.0]],
        ];
        let points = (-8..=16)
            .flat_map(|x| {
                (-8..=16).flat_map(move |y| {
                    (-4..=8).map(move |z| [x as f64 * 0.5, y as f64 * 0.5, z as f64 * 0.5])
                })
            })
            .collect::<Vec<_>>();
        for depth in [0.25, 0.75, 1.5] {
            let expected = points
                .iter()
                .enumerate()
                .filter(|(_, point)| {
                    triangles.iter().any(|triangle| {
                        point_triangle_distance_squared(**point, *triangle) <= depth * depth
                    })
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            assert_eq!(
                indexed_points_near_triangles(&points, &triangles, depth),
                expected
            );
        }
    }

    #[test]
    #[ignore = "explicit product-scale passive-region latency profile"]
    fn profile_product_scale_indexed_region_selection() {
        let points = (0..50_000)
            .map(|index| {
                let x = index % 50;
                let y = (index / 50) % 50;
                let z = index / 2_500;
                [x as f64 * 2.4, y as f64 * 2.4, z as f64 * 2.4]
            })
            .collect::<Vec<_>>();
        let triangles = (0..300)
            .map(|index| {
                let x = (index % 20) as f64 * 6.0;
                let y = (index / 20) as f64 * 6.0;
                [[x, y, 0.0], [x + 5.0, y, 0.0], [x, y + 5.0, 0.0]]
            })
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let selected = indexed_points_near_triangles(&points, &triangles, 2.5);
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        eprintln!(
            "{{:workload \"passive-region-50k\" :points {} :triangles {} :selected {} :elapsed-ms {elapsed_ms}}}",
            points.len(),
            triangles.len(),
            selected.len()
        );
        assert!(!selected.is_empty());
        assert!(elapsed_ms <= 30_000.0);
    }

    #[test]
    fn topology_face_groups_translate_to_exact_resultant_and_fixed_dofs() {
        let mesh = crate::services::fem_artifacts::FemTopologyMeshData {
            mesh: ecky_fem::FemIndexedTet4Mesh {
                schema_version: FEM_SCHEMA_VERSION,
                nodes: vec![
                    ecky_fem::FemPoint3::new(0.0, 0.0, 0.0),
                    ecky_fem::FemPoint3::new(1.0, 0.0, 0.0),
                    ecky_fem::FemPoint3::new(0.0, 1.0, 0.0),
                    ecky_fem::FemPoint3::new(0.0, 0.0, 1.0),
                ],
                cells: vec![[0, 1, 2, 3]],
            },
            boundary_triangles: vec![[1, 3, 2], [0, 2, 3], [0, 3, 1], [0, 1, 2]],
            boundary_face_group_indices: vec![0, 1, 2, 3],
            face_group_count: 4,
        };
        let loads = topology_surface_loads(
            &mesh,
            &[crate::contracts::FemTopologySurfaceLoadDto {
                id: "bottle-lateral".into(),
                weight: 1.0,
                face_group_indices: vec![0],
                total_force_n: [3.0, -4.0, 5.0],
            }],
        )
        .unwrap();
        let resultant = (0..3)
            .map(|axis| loads[0].rhs_n.iter().skip(axis).step_by(3).sum::<f64>())
            .collect::<Vec<_>>();
        assert_eq!(resultant, vec![3.0, -4.0, 5.0]);

        let fixed = topology_fixed_constraints(&mesh, &[3]).unwrap();
        assert_eq!(fixed.len(), 9);
        assert!(fixed.iter().all(|constraint| constraint.dof_index < 9));
    }

    #[test]
    fn shared_fem_job_stops_only_after_final_subscriber_cancels() {
        let job = Arc::new(FemSharedJob::new());
        let first_cancelled = Arc::new(AtomicBool::new(false));
        let second_cancelled = Arc::new(AtomicBool::new(false));
        let first = job.subscribe(first_cancelled.clone()).unwrap();
        let second = job.subscribe(second_cancelled.clone()).unwrap();

        first_cancelled.store(true, Ordering::Release);
        job.refresh_cancellation().unwrap();
        assert!(!job.execution_cancelled.load(Ordering::Acquire));

        second_cancelled.store(true, Ordering::Release);
        job.refresh_cancellation().unwrap();
        assert!(job.execution_cancelled.load(Ordering::Acquire));

        drop(first);
        drop(second);
        let next = job.subscribe(Arc::new(AtomicBool::new(false))).unwrap();
        assert!(!job.execution_cancelled.load(Ordering::Acquire));
        drop(next);
    }

    #[test]
    fn subscriber_after_final_cancellation_gets_a_fresh_job_generation() {
        let digest = format!(
            "subscriber-rollover-{}",
            FEM_REQUEST_CACHE_NONCE.fetch_add(1, Ordering::Relaxed)
        );
        let cancelled_job = fem_singleflight_job(&digest).unwrap();
        let cancelled = Arc::new(AtomicBool::new(true));
        let old_subscription = cancelled_job.subscribe(cancelled).unwrap();
        cancelled_job.refresh_cancellation().unwrap();
        assert!(cancelled_job.execution_cancelled.load(Ordering::Acquire));

        let fresh_job = fem_singleflight_job(&digest).unwrap();

        assert!(!Arc::ptr_eq(&cancelled_job, &fresh_job));
        assert!(!fresh_job.execution_cancelled.load(Ordering::Acquire));
        drop(old_subscription);
    }

    #[test]
    fn request_cache_prunes_to_bounded_immutable_edn_entries() {
        let root = std::env::temp_dir().join(format!(
            "ecky-fem-cache-bound-{}-{}",
            std::process::id(),
            FEM_REQUEST_CACHE_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        for index in 0..5 {
            fs::write(root.join(format!("{index}.edn")), vec![b'x'; 32]).unwrap();
        }
        fs::write(root.join("active.tmp"), b"private publication").unwrap();

        prune_fem_cache_directory(&root, 3, 80).unwrap();

        let edn_entries = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("edn")
            })
            .collect::<Vec<_>>();
        assert_eq!(edn_entries.len(), 2);
        assert!(root.join("active.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn request_cache_identity_changes_for_every_physics_and_provenance_input() {
        use crate::gmsh_mesher::{ExactBrepMesherRuntime, GmshRuntimeIdentity};

        let request = convergence_request().study;
        let selectors = serde_json::json!({
            "mount": {"kind":"face", "durableTargetIds":["body:face:mount"]},
            "load": {"kind":"face", "durableTargetIds":["body:face:load"]}
        });
        let runtime = ExactBrepMesherRuntime {
            gmsh: GmshRuntimeIdentity {
                version: "4.15.2".into(),
                platform: "test".into(),
                architecture: "test".into(),
                executable_path: PathBuf::from("/runtime/gmsh"),
                executable_sha256: "sha256:executable-a".into(),
            },
            netgen: None,
        };
        let digest = |request: &FemStudyRequest,
                      boundary: &str,
                      geometry: &str,
                      selectors: &serde_json::Value,
                      runtime: &ExactBrepMesherRuntime,
                      mesh_size: Option<f64>| {
            fem_request_cache_digest_components(
                request,
                boundary,
                geometry,
                "sha256:step-a",
                selectors,
                runtime,
                mesh_size,
            )
            .expect("cache identity")
        };
        let baseline = digest(
            &request,
            "sha256:boundary-a",
            "sha256:geometry-a",
            &selectors,
            &runtime,
            None,
        );
        let mut runtime_with_netgen = runtime.clone();
        runtime_with_netgen.netgen = Some(crate::netgen_mesher::NetgenRuntimeIdentity {
            python_path: PathBuf::from("/runtime/python"),
            python_sha256: "sha256:python-a".into(),
            module_path: PathBuf::from("/runtime/libngpy.so"),
            module_sha256: "sha256:libngpy-a".into(),
            runtime_digest: "sha256:netgen-runtime-a".into(),
            version: "6.2.2606".into(),
            platform: "test".into(),
            architecture: "test".into(),
        });
        assert_ne!(
            baseline,
            digest(
                &request,
                "sha256:boundary-a",
                "sha256:geometry-a",
                &selectors,
                &runtime_with_netgen,
                None,
            ),
            "Netgen fallback runtime mutation must invalidate cache"
        );

        for (label, source) in [
            ("source", "(model source-b)"),
            ("material", "(model (material steel))"),
            ("load", "(model (load service-load-b))"),
            ("constraint", "(model (constraint mount-b))"),
        ] {
            let mut changed = request.clone();
            changed.source = source.into();
            assert_ne!(
                baseline,
                digest(
                    &changed,
                    "sha256:boundary-a",
                    "sha256:geometry-a",
                    &selectors,
                    &runtime,
                    None
                ),
                "{label} mutation must invalidate cache"
            );
        }

        let mut changed_params = request.clone();
        changed_params.model_id = "model-parameter-snapshot-b".into();
        assert_ne!(
            baseline,
            fem_request_cache_digest_components(
                &request,
                "sha256:boundary-a",
                "sha256:geometry-a",
                "sha256:step-b",
                &selectors,
                &runtime,
                None,
            )
            .expect("changed STEP identity"),
            "exact STEP mutation must invalidate cache"
        );
        assert_ne!(
            baseline,
            digest(
                &changed_params,
                "sha256:boundary-a",
                "sha256:geometry-a",
                &selectors,
                &runtime,
                None
            ),
            "parameter snapshot mutation must invalidate cache"
        );
        assert_ne!(
            baseline,
            digest(
                &request,
                "sha256:boundary-b",
                "sha256:geometry-a",
                &selectors,
                &runtime,
                None
            ),
            "boundary mesh mutation must invalidate cache"
        );
        assert_ne!(
            baseline,
            digest(
                &request,
                "sha256:boundary-a",
                "sha256:geometry-b",
                &selectors,
                &runtime,
                None
            ),
            "exact geometry mutation must invalidate cache"
        );
        let mut changed_selectors = selectors.clone();
        changed_selectors["mount"]["durableTargetIds"][0] = serde_json::json!("body:face:mount-b");
        assert_ne!(
            baseline,
            digest(
                &request,
                "sha256:boundary-a",
                "sha256:geometry-a",
                &changed_selectors,
                &runtime,
                None
            ),
            "selector mutation must invalidate cache"
        );
        let mut changed_tolerance = request.clone();
        changed_tolerance.control.relative_solver_tolerance = 1.0e-9;
        assert_ne!(
            baseline,
            digest(
                &changed_tolerance,
                "sha256:boundary-a",
                "sha256:geometry-a",
                &selectors,
                &runtime,
                None
            ),
            "solver tolerance mutation must invalidate cache"
        );
        assert_ne!(
            baseline,
            digest(
                &request,
                "sha256:boundary-a",
                "sha256:geometry-a",
                &selectors,
                &runtime,
                Some(0.5)
            ),
            "volume-mesh control mutation must invalidate cache"
        );
        let mut changed_runtime = runtime.clone();
        changed_runtime.gmsh.executable_sha256 = "sha256:executable-b".into();
        assert_ne!(
            baseline,
            digest(
                &request,
                "sha256:boundary-a",
                "sha256:geometry-a",
                &selectors,
                &changed_runtime,
                None
            ),
            "runtime binary mutation must invalidate cache"
        );
    }

    #[test]
    fn convergence_status_never_averages_away_red_levels() {
        assert_eq!(
            convergence_status([0.01, 0.02].into_iter(), 0.03, false),
            "converged"
        );
        assert_eq!(
            convergence_status([0.01, 0.08].into_iter(), 0.03, false),
            "unconverged"
        );
        assert_eq!(
            convergence_status([0.04, 0.08].into_iter(), 0.03, true),
            "suspectedSingularity"
        );
    }

    #[test]
    fn convergence_requires_three_strictly_decreasing_bounded_sizes() {
        let base = convergence_request();
        assert!(validate_convergence_request(&base).is_ok());
        let mut invalid = base.clone();
        invalid.mesh_sizes_mm = vec![4.0, 4.0, 1.0];
        assert!(validate_convergence_request(&invalid).is_err());
    }

    #[test]
    fn convergence_cache_identity_ignores_ephemeral_job_id_but_tracks_source() {
        let request = convergence_request();
        let baseline = fem_convergence_cache_digest(&request).expect("cache identity");

        let mut next_job = request.clone();
        next_job.study.job_id = "next-window-open".to_string();
        assert_eq!(
            baseline,
            fem_convergence_cache_digest(&next_job).expect("same cache identity")
        );

        let mut changed_source = request;
        changed_source.study.source = "(model (part changed (box 1 1 1)))".to_string();
        assert_ne!(
            baseline,
            fem_convergence_cache_digest(&changed_source).expect("changed cache identity")
        );
    }

    fn convergence_request() -> FemConvergenceRequest {
        FemConvergenceRequest {
            study: FemStudyRequest {
                job_id: "convergence".to_string(),
                model_id: "model".to_string(),
                source: "(model)".to_string(),
                analysis_name: "study".to_string(),
                budgets: crate::contracts::FemBudgetLimitsDto {
                    boundary_triangles: 10,
                    tet4_cells: 10,
                    nodes: 10,
                    dofs: 30,
                    sparse_nonzeros: 100,
                    result_bytes: 1024,
                    convergence_levels: 3,
                },
                control: crate::contracts::FemPipelineControlDto {
                    envelope_mm: 0.1,
                    minimum_scaled_jacobian: 1.0e-6,
                    maximum_runtime_ms: 1000,
                    relative_solver_tolerance: 1.0e-8,
                    thread_count: 1,
                },
            },
            mesh_sizes_mm: vec![4.0, 2.0, 1.0],
            displacement_relative_tolerance: 0.03,
            stress_relative_tolerance: 0.05,
        }
    }

    fn convergence_run_response(job_id: String, value: f64) -> FemRunResponse {
        FemRunResponse {
            job_id,
            model_id: "model".to_string(),
            analysis_name: "study".to_string(),
            source_digest: "sha256:source".to_string(),
            analysis_identity_digest: format!("sha256:analysis-{value}"),
            solution_digest: format!("sha256:solution-{value}"),
            result_digest: format!("sha256:result-{value}"),
            mesh_content_digest: format!("sha256:mesh-{value}"),
            source_boundary_digest: "sha256:boundary".to_string(),
            decision_ready: true,
            decision_readiness_error: None,
            manifest_path: "/tmp/fem-result.json".to_string(),
            arrays: vec![],
            summary: FemResultSummaryDto {
                maximum_displacement_mm: value,
                maximum_von_mises_mpa: value * 100.0,
                maximum_principal_stress_mpa: value * 110.0,
                volume_mm3: 1.0,
                mass_kg: 1.0,
                minimum_yield_safety_factor: Some(2.0),
                equilibrium_relative_imbalance: 1.0e-12,
                solver_relative_residual: 1.0e-13,
                minimum_scaled_jacobian: 0.2,
                node_count: 10,
                tet4_cell_count: 20,
                extrema: vec![],
            },
            support_reactions: vec![],
            engineering_evidence: serde_json::from_value(serde_json::json!({
                "question": {"questionId":"q", "statement":"question", "decision":"decide", "acceptanceMetricIds":["metric"]},
                "idealization": {"artifactDigest":"sha256:idealization", "kind":"exactSolid", "sourceGeometryDigest":"sha256:geometry", "analysisGeometryDigest":"sha256:geometry", "manufacturingGeometryDigest":"sha256:geometry", "affectedTopologyIds":[], "justification":"exact solid", "expectedInfluencePercent":0.0, "acceptedByUser":true},
                "inputs": [], "assumptions": [], "applicability": [], "sensitivity": null,
                "validationEvidence": [], "verificationLayers": []
            })).expect("test engineering evidence DTO"),
            acceptance_evaluations: vec![],
        }
    }

    #[test]
    fn convergence_preserves_completed_levels_when_intermediate_solve_fails() {
        let request = convergence_request();
        let mut invocation = 0;
        let response = run_fem_convergence_sequence(
            request,
            &AtomicBool::new(false),
            |study, _mesh_size_mm| {
                invocation += 1;
                if invocation == 2 {
                    Err(AppError::internal(
                        "Faer factorization failed at refinement level 2",
                    ))
                } else {
                    Ok(convergence_run_response(study.job_id, invocation as f64))
                }
            },
        )
        .expect("partial convergence evidence");
        assert_eq!(response.sequence_status, "failed");
        assert_eq!(response.displacement_status, "failed");
        assert_eq!(response.stress_status, "failed");
        assert_eq!(response.levels.len(), 2);
        assert_eq!(response.levels[0].status, "completed");
        assert!(response.levels[0].result_digest.is_some());
        assert!(response.levels[0].mesh_content_digest.is_some());
        assert_eq!(response.levels[0].solver_relative_residual, Some(1.0e-13));
        assert_eq!(response.levels[1].status, "failed");
        assert!(response.levels[1]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("Faer factorization failed"));
        assert!(response.levels[1].result_digest.is_none());
    }

    #[test]
    fn three_level_convergence_keeps_identity_quality_residual_extrema_and_separate_metric_status()
    {
        let mut level = 0usize;
        let displacement = [1.0, 1.01, 1.015];
        let stress = [100.0, 110.0, 125.0];
        let response = run_fem_convergence_sequence(
            convergence_request(),
            &AtomicBool::new(false),
            |study, _mesh_size_mm| {
                let mut response = convergence_run_response(study.job_id, displacement[level]);
                response.summary.maximum_von_mises_mpa = stress[level];
                response.summary.node_count = 10 + level as u64;
                response.summary.tet4_cell_count = 20 + level as u64 * 2;
                response.summary.minimum_scaled_jacobian = 0.2 - level as f64 * 0.01;
                level += 1;
                Ok(response)
            },
        )
        .expect("three-level convergence evidence");

        assert_eq!(response.sequence_status, "completed");
        assert_eq!(response.levels.len(), 3);
        assert_eq!(response.displacement_status, "converged");
        assert_eq!(response.stress_status, "suspectedSingularity");
        assert!(response.levels[0].displacement_relative_delta.is_none());
        assert!(response.levels[1..].iter().all(|level| {
            level.analysis_identity_digest.is_some()
                && level.solution_digest.is_some()
                && level.result_digest.is_some()
                && level.mesh_content_digest.is_some()
                && level.node_count.is_some()
                && level.tet4_cell_count.is_some()
                && level.minimum_scaled_jacobian.is_some()
                && level.solver_relative_residual.is_some()
                && level.maximum_displacement_mm.is_some()
                && level.maximum_von_mises_mpa.is_some()
                && level.displacement_relative_delta.is_some()
                && level.stress_relative_delta.is_some()
        }));
    }

    #[test]
    fn convergence_quality_failure_preserves_completed_level_and_raw_gate_detail() {
        let mut level = 0usize;
        let response = run_fem_convergence_sequence(
            convergence_request(),
            &AtomicBool::new(false),
            |study, _mesh_size_mm| {
                level += 1;
                if level == 2 {
                    return Err(AppError::validation(
                        "minimum scaled Jacobian -0.04 is below allowed 0.01",
                    ));
                }
                Ok(convergence_run_response(study.job_id, 1.0))
            },
        )
        .expect("quality failure evidence");

        assert_eq!(response.sequence_status, "failed");
        assert_eq!(response.levels[0].status, "completed");
        assert_eq!(response.levels[1].status, "failed");
        assert!(response.levels[1]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("scaled Jacobian -0.04"));
    }

    #[test]
    fn convergence_cancellation_returns_explicit_level_without_running_solver() {
        let mut invoked = false;
        let cancellation = AtomicBool::new(true);
        let response = run_fem_convergence_sequence(
            convergence_request(),
            &cancellation,
            |_study, _mesh_size_mm| {
                invoked = true;
                unreachable!("cancelled convergence must not start next level")
            },
        )
        .expect("cancelled sequence evidence");
        assert!(!invoked);
        assert_eq!(response.sequence_status, "cancelled");
        assert_eq!(response.levels.len(), 1);
        assert_eq!(response.levels[0].status, "cancelled");
        assert!(response.levels[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("cancelled"));
    }

    #[test]
    fn required_acceptance_only_passes_with_matching_metric_convergence() {
        let evaluation = FemAcceptanceEvaluationDto {
            study_name: "bracket-static".to_string(),
            metric_id: "tip-limit".to_string(),
            field: "maximum-displacement".to_string(),
            status: "pending".to_string(),
            observed: Some(0.4),
            unit: "mm".to_string(),
            threshold: 0.5,
            comparison: "lessThanOrEqual".to_string(),
            mesh_size_mm: 1.0,
            node_id: Some(7),
            element_id: None,
            coordinate_mm: Some([1.0, 2.0, 3.0]),
            analysis_identity_digest: "sha256:analysis".to_string(),
            mesh_content_digest: "sha256:mesh".to_string(),
            result_digest: "sha256:result".to_string(),
            convergence_status: None,
            evidence_chain: FemAcceptanceEvidenceChainDto {
                source_geometry_digest: "sha256:geometry".into(),
                analysis_geometry_digest: "sha256:geometry".into(),
                idealization_accepted: true,
                input_evidence_ids: vec![],
                applicability_check_ids: vec![],
                convergence_status: None,
                sensitivity_result_digests: vec![],
                validation_evidence_ids: vec![],
                gaps: vec!["convergence pending".into()],
            },
            detail: "single run pending".to_string(),
        };
        let green = complete_converged_acceptance_evaluation(
            evaluation.clone(),
            "completed",
            "converged",
            "unconverged",
        );
        assert_eq!(green.status, "passed");
        assert_eq!(green.convergence_status.as_deref(), Some("converged"));
        assert_eq!(
            green.evidence_chain.convergence_status.as_deref(),
            Some("converged")
        );

        let red = complete_converged_acceptance_evaluation(
            evaluation,
            "completed",
            "unconverged",
            "converged",
        );
        assert_eq!(red.status, "pending");
        assert!(red.detail.contains("cannot pass"));
    }

    #[test]
    fn parameter_sweep_preserves_current_selectors_and_rejects_removed_topology_without_coordinate_rebinding(
    ) {
        use crate::contracts::{DesignParams, ParamValue};
        use crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle;
        use crate::ecky_cad_host::direct_occt_sdk::{
            bundled_occt_runtime_root_from_repo, inspect_occt_runtime,
        };

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root")
            .to_path_buf();
        let layout = inspect_occt_runtime(bundled_occt_runtime_root_from_repo(&repo_root));
        if !layout.runtime_complete() {
            eprintln!("native FEM topology-transition sweep requires complete OCCT runtime");
            return;
        }
        let source = include_str!("../../tests/fixtures/fem/parameter-topology-sweep.ecky");
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("sweep fixture");
        let root = std::env::temp_dir().join(format!(
            "ecky-fem-parameter-sweep-{}-{}",
            std::process::id(),
            FEM_REQUEST_CACHE_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        let resolver = TestResolver { root: root.clone() };
        let render_case = |width: f64, cut_radius: f64| {
            let params = DesignParams::from([
                ("width".to_string(), ParamValue::Number(width)),
                ("cut-radius".to_string(), ParamValue::Number(cut_radius)),
            ]);
            let (bundle, manifest) =
                render_core_program_runtime_bundle(&program, source, &params, &layout, &resolver)
                    .expect("parameter sweep exact OCCT render");
            let bundle_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &bundle.model_id)
                .expect("sweep bundle directory");
            let boundary = load_direct_occt_analysis_boundary_surface(&bundle_dir, "bracket")
                .expect("sweep analysis boundary");
            let resolved = resolve_fem_face_tags(&manifest.tagged_anchors, &boundary)
                .expect("current exact selectors survive");
            assert_eq!(resolved["mounting"].len(), 1);
            assert_eq!(resolved["load-pad"].len(), 1);
            assert_eq!(
                boundary.triangle_face_group_indices.len(),
                boundary.triangles.len()
            );
            assert!(boundary
                .face_groups
                .iter()
                .all(|group| group.triangle_count > 0));
            assert!(boundary
                .triangle_face_group_indices
                .iter()
                .all(|index| (*index as usize) < boundary.face_groups.len()));
            (manifest, boundary)
        };

        let _minimum = render_case(8.0, 0.0);
        let (nominal_manifest, nominal_boundary) = render_case(10.0, 0.0);
        let _maximum = render_case(12.0, 0.0);
        let (_transition_manifest, transition_boundary) = render_case(10.0, 2.0);

        let transition_ids = transition_boundary
            .face_groups
            .iter()
            .map(|group| group.canonical_target_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let removed = nominal_boundary
            .face_groups
            .iter()
            .find(|group| !transition_ids.contains(group.canonical_target_id.as_str()))
            .expect("topology transition removes at least one nominal face identity");
        let mut stale_anchors = nominal_manifest.tagged_anchors;
        stale_anchors.insert(
            "removed-transition-face".into(),
            crate::contracts::TaggedAnchorBinding {
                kind: crate::contracts::TaggedAnchorKind::Face,
                authored_selector: "exact removed nominal face".into(),
                target: "bracket".into(),
                target_ids: vec![removed.target_id.clone()],
                durable_target_ids: vec![removed
                    .durable_target_id
                    .clone()
                    .expect("nominal durable face")],
                canonical_target_ids: vec![removed.canonical_target_id.clone()],
                alias_ids: vec![],
            },
        );
        let error = resolve_fem_face_tags(&stale_anchors, &transition_boundary)
            .expect_err("removed topology must fail instead of coordinate rebinding");
        assert!(error.message.contains("resolved to 0"), "{error:?}");

        fs::remove_dir_all(root).expect("cleanup parameter sweep fixture");
    }

    #[test]
    fn exact_warm_run_uses_singleflight_cache_without_meshing_or_solving_again() {
        use crate::contracts::DesignParams;
        use crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle;
        use crate::ecky_cad_host::direct_occt_sdk::{
            bundled_occt_runtime_root_from_repo, inspect_occt_runtime,
        };

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root")
            .to_path_buf();
        let occt_root = bundled_occt_runtime_root_from_repo(&repo_root);
        if !occt_root.is_dir() || probe_system_exact_brep_mesher_runtime().is_err() {
            eprintln!("native FEM cache proof is platform-gated");
            return;
        }
        let layout = inspect_occt_runtime(&occt_root);
        if !layout.runtime_complete() {
            eprintln!("native FEM cache proof requires complete OCCT runtime");
            return;
        }
        let source = r#"(model
          (tag-face mounting :faces "bottom" bracket)
          (tag-face load-pad :faces "top" bracket)
          (part bracket (box 10 10 10))
          (analysis bracket-static
            (linear-static :part bracket)
            (question bracket-strength :statement "Does the part remain elastic?" :decision "accept or revise" :acceptance-metrics [stress-limit displacement-limit])
            (acceptance-criterion stress-limit :field von-mises-stress :comparison less-than-or-equal :limit "200" :unit MPa :requires-convergence false)
            (acceptance-criterion displacement-limit :field maximum-displacement :comparison less-than-or-equal :limit "1" :unit mm :requires-convergence true)
            (idealization exact-solid :justification "Exact connected solid." :accepted true)
            (evidence material-source :subject material :source "qualified material record" :authority recorded-source :uncertainty-percent 0 :decision-critical true)
            (evidence load-source :subject load :source "accepted load case" :authority user-accepted :uncertainty-percent 0 :decision-critical true)
            (evidence support-source :subject support :source "accepted fixture" :authority user-accepted :uncertainty-percent 0 :decision-critical true)
            (input-evidence aluminum :evidence material-source)
            (input-evidence applied-load :evidence load-source)
            (input-evidence mounting :evidence support-source)
            (assumption small-strain :category physics :statement "Small displacement linear elasticity." :status accepted :evidence [material-source load-source support-source])
            (validation-evidence fixture :kind physical-test :source "versioned fixture" :result-digest "sha256:fixture")
            (material aluminum :young-modulus 68900MPa :poisson-ratio 0.33 :density 2700kg-per-m3 :yield-strength 276MPa)
            (volume-mesh :element tet4 :size 5mm)
            (fixed :faces (tag mounting))
            (surface-force :faces (tag load-pad) :total [0N 0N -10N])
            (solve :method sparse-direct)))"#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let root =
            std::env::temp_dir().join(format!("ecky-fem-command-cache-{}", uuid::Uuid::new_v4()));
        let resolver = TestResolver { root: root.clone() };
        let (bundle, mut manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("render exact OCCT bundle");
        manifest.source_digest = Some(crate::services::render_snapshot::canonical_source_digest(
            source,
        ));
        crate::model_runtime::write_runtime_bundle(&resolver, &bundle.model_id, &bundle, &manifest)
            .expect("bind source digest to runtime bundle");
        let make_request = |job_id: &str| FemStudyRequest {
            job_id: job_id.to_string(),
            model_id: bundle.model_id.clone(),
            source: source.to_string(),
            analysis_name: "bracket-static".to_string(),
            budgets: crate::contracts::FemBudgetLimitsDto {
                boundary_triangles: 100_000,
                tet4_cells: 500_000,
                nodes: 150_000,
                dofs: 450_000,
                sparse_nonzeros: 30_000_000,
                result_bytes: 128 * 1024 * 1024,
                convergence_levels: 3,
            },
            control: crate::contracts::FemPipelineControlDto {
                envelope_mm: 0.1,
                minimum_scaled_jacobian: 1.0e-6,
                maximum_runtime_ms: 120_000,
                relative_solver_tolerance: 1.0e-8,
                thread_count: 1,
            },
        };
        let mut cold_mesh_progress = Vec::new();
        let cold_mesh = preview_fem_mesh_with_resolver(
            make_request("mesh-cache-cold"),
            &resolver,
            &AtomicBool::new(false),
            |event| cold_mesh_progress.push(event),
        )
        .expect("cold FEM mesh preview");
        assert!(cold_mesh_progress
            .iter()
            .any(|event| event.stage == FemPipelineStage::VolumeMesh));
        assert!(!cold_mesh_progress
            .iter()
            .any(|event| event.stage == FemPipelineStage::Solve));
        let mut warm_mesh_progress = Vec::new();
        let warm_mesh = preview_fem_mesh_with_resolver(
            make_request("mesh-cache-warm"),
            &resolver,
            &AtomicBool::new(false),
            |event| warm_mesh_progress.push(event),
        )
        .expect("warm FEM mesh preview");
        assert_eq!(warm_mesh.mesh_content_digest, cold_mesh.mesh_content_digest);
        assert_eq!(warm_mesh_progress.len(), 1);
        assert!(warm_mesh_progress[0].detail.contains("Gmsh HXT skipped"));

        let mut cold_progress = Vec::new();
        let cold = run_fem_study_with_resolver(
            make_request("cache-cold"),
            &resolver,
            &AtomicBool::new(false),
            |event| cold_progress.push(event),
        )
        .expect("cold FEM run");
        assert_eq!(cold.acceptance_evaluations.len(), 2);
        assert_eq!(cold.acceptance_evaluations[0].metric_id, "stress-limit");
        assert_eq!(cold.acceptance_evaluations[0].status, "passed");
        assert!(cold.acceptance_evaluations[0].element_id.is_some());
        assert_eq!(
            cold.acceptance_evaluations[0].analysis_identity_digest,
            cold.analysis_identity_digest
        );
        assert_eq!(
            cold.acceptance_evaluations[0].result_digest,
            cold.result_digest
        );
        assert_eq!(
            cold.acceptance_evaluations[1].metric_id,
            "displacement-limit"
        );
        assert_eq!(cold.acceptance_evaluations[1].status, "pending");
        assert!(cold.acceptance_evaluations[1]
            .detail
            .contains("requires current convergence evidence"));
        assert!(!cold.decision_ready);
        assert!(cold_progress
            .iter()
            .any(|event| event.stage == FemPipelineStage::VolumeMesh));
        assert!(cold_progress
            .iter()
            .any(|event| event.stage == FemPipelineStage::Solve));

        let mut warm_progress = Vec::new();
        let warm = run_fem_study_with_resolver(
            make_request("cache-warm"),
            &resolver,
            &AtomicBool::new(false),
            |event| warm_progress.push(event),
        )
        .expect("warm FEM run");
        assert_eq!(warm.analysis_identity_digest, cold.analysis_identity_digest);
        assert_eq!(warm.solution_digest, cold.solution_digest);
        assert_eq!(warm_progress.len(), 1);
        assert_eq!(warm_progress[0].stage, FemPipelineStage::Publish);
        assert!(warm_progress[0].detail.contains("mesh and solve skipped"));

        let mut concurrent_a = make_request("singleflight-a");
        concurrent_a.control.relative_solver_tolerance = 5.0e-9;
        let mut concurrent_b = make_request("singleflight-b");
        concurrent_b.control.relative_solver_tolerance = 5.0e-9;
        let resolver_a = resolver.clone();
        let resolver_b = resolver.clone();
        let first = std::thread::spawn(move || {
            let mut progress = Vec::new();
            let result = run_fem_study_with_resolver(
                concurrent_a,
                &resolver_a,
                &AtomicBool::new(false),
                |event| progress.push(event),
            );
            (result, progress)
        });
        let second = std::thread::spawn(move || {
            let mut progress = Vec::new();
            let result = run_fem_study_with_resolver(
                concurrent_b,
                &resolver_b,
                &AtomicBool::new(false),
                |event| progress.push(event),
            );
            (result, progress)
        });
        let (first_result, first_progress) = first.join().expect("first singleflight thread");
        let (second_result, second_progress) = second.join().expect("second singleflight thread");
        let first_result = first_result.expect("first singleflight result");
        let second_result = second_result.expect("second singleflight result");
        assert_eq!(first_result.solution_digest, second_result.solution_digest);
        let combined = first_progress
            .iter()
            .chain(&second_progress)
            .collect::<Vec<_>>();
        assert_eq!(
            combined
                .iter()
                .filter(|event| event.stage == FemPipelineStage::VolumeMesh)
                .count(),
            1
        );
        assert_eq!(
            combined
                .iter()
                .filter(|event| event.stage == FemPipelineStage::Solve)
                .count(),
            1
        );
        assert_eq!(
            combined
                .iter()
                .filter(|event| event.detail.contains("mesh and solve skipped"))
                .count(),
            1
        );
        fs::remove_dir_all(root).expect("cleanup cache fixture");
    }

    #[test]
    fn authored_topology_run_builds_mesh_internally_and_resumes_deterministically() {
        use crate::contracts::DesignParams;
        use crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle;
        use crate::ecky_cad_host::direct_occt_sdk::{
            bundled_occt_runtime_root_from_repo, inspect_occt_runtime,
        };

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root")
            .to_path_buf();
        let occt_root = bundled_occt_runtime_root_from_repo(&repo_root);
        if !occt_root.is_dir() || probe_system_exact_brep_mesher_runtime().is_err() {
            eprintln!("native topology orchestration proof is platform-gated");
            return;
        }
        let layout = inspect_occt_runtime(&occt_root);
        if !layout.runtime_complete() {
            eprintln!("native topology orchestration proof requires complete OCCT runtime");
            return;
        }
        let source = r#"(model
          (tag-face mounting :faces "bottom" bracket)
          (tag-face load-pad :faces "top" bracket)
          (part bracket (box 10 10 10))
          (analysis bracket-topology
            (linear-static :part bracket)
            (question stiffness :statement "Is the load path stiff?" :decision "optimize" :acceptance-metrics [displacement-limit])
            (acceptance-criterion displacement-limit :field maximum-displacement :comparison less-than-or-equal :limit "1" :unit mm :requires-convergence false)
            (idealization exact-solid :justification "Exact connected design domain." :accepted true)
            (evidence material-source :subject material :source "screening datasheet" :authority recorded-source :uncertainty-percent 0 :decision-critical true)
            (evidence load-source :subject load :source "accepted topology load" :authority user-accepted :uncertainty-percent 0 :decision-critical true)
            (evidence support-source :subject support :source "accepted mount" :authority user-accepted :uncertainty-percent 0 :decision-critical true)
            (input-evidence petg-cf :evidence material-source)
            (input-evidence applied-load :evidence load-source)
            (input-evidence mounting :evidence support-source)
            (assumption small-strain :category physics :statement "Small displacement linear elasticity." :status accepted :evidence [material-source load-source support-source])
            (material petg-cf :young-modulus 4000MPa :poisson-ratio 0.35 :density 1250kg-per-m3 :yield-strength 45MPa)
            (volume-mesh :element tet4 :size 5mm)
            (topology-controls :volume-fraction 0.5 :penalty 3 :minimum-density 0.001
              :filter-radius 2mm :move-limit 0.1 :convergence-tolerance 0.000000000000001)
            (passive-solid :faces (tag mounting) :depth 3mm)
            (passive-void :faces (tag load-pad) :depth 3mm)
            (fixed :faces (tag mounting))
            (surface-force :faces (tag load-pad) :total [0N 0N -10N])
            (solve :method sparse-direct)))"#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let topology_analysis = program
            .analyses
            .iter()
            .find(|analysis| analysis.name == "bracket-topology")
            .expect("topology analysis");
        assert!(topology_analysis.clauses.iter().any(|clause| matches!(
            clause.kind,
            ecky_render::core_ir::CoreAnalysisClauseKind::PassiveSolid { .. }
        )));
        assert!(topology_analysis.clauses.iter().any(|clause| matches!(
            clause.kind,
            ecky_render::core_ir::CoreAnalysisClauseKind::PassiveVoid { .. }
        )));
        let root =
            std::env::temp_dir().join(format!("ecky-authored-topology-{}", uuid::Uuid::new_v4()));
        let resolver = TestResolver { root: root.clone() };
        let (bundle, mut manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("render exact OCCT bundle");
        manifest.source_digest = Some(crate::services::render_snapshot::canonical_source_digest(
            source,
        ));
        crate::model_runtime::write_runtime_bundle(&resolver, &bundle.model_id, &bundle, &manifest)
            .expect("bind source digest to runtime bundle");

        let study = |job_id: &str| FemStudyRequest {
            job_id: job_id.into(),
            model_id: bundle.model_id.clone(),
            source: source.into(),
            analysis_name: "bracket-topology".into(),
            budgets: crate::contracts::FemBudgetLimitsDto {
                boundary_triangles: 100_000,
                tet4_cells: 100_000,
                nodes: 100_000,
                dofs: 300_000,
                sparse_nonzeros: 10_000_000,
                result_bytes: 64 * 1024 * 1024,
                convergence_levels: 3,
            },
            control: crate::contracts::FemPipelineControlDto {
                envelope_mm: 0.1,
                minimum_scaled_jacobian: 1.0e-6,
                maximum_runtime_ms: 120_000,
                relative_solver_tolerance: 1.0e-8,
                thread_count: 1,
            },
        };
        let runtime_policy = |maximum_new_iterations| FemTopologyRuntimePolicy {
            maximum_iterations: 2,
            maximum_new_iterations,
            maximum_dimension: 300_000,
            maximum_elements: 100_000,
            maximum_working_memory_bytes: 512 * 1024 * 1024,
            maximum_result_bytes: 64 * 1024 * 1024,
            maximum_wall_time_ms: 120_000,
        };

        assert!(!resolver.app_data_dir().join("fem-meshes-v1").exists());
        let stepped = run_fem_topology_with_resolver(
            FemTopologyRunRequest {
                study: study("topology-step"),
                resume_state_digest: None,
            },
            &runtime_policy(1),
            &resolver,
            &AtomicBool::new(false),
        )
        .expect("one authored topology operation builds mesh and advances");
        assert_eq!(stepped.termination, "paused");
        assert_eq!(stepped.iteration_count, 1);
        assert!(resolver.app_data_dir().join("fem-meshes-v1").is_dir());
        assert!(PathBuf::from(&stepped.checkpoint_path).is_file());
        assert!(PathBuf::from(stepped.density_path.as_ref().unwrap()).is_file());
        assert!(PathBuf::from(stepped.preview_vtu_path.as_ref().unwrap()).is_file());
        assert!(stepped.passive_solid_volume_fraction.unwrap() > 0.0);
        assert!(stepped.passive_void_volume_fraction.unwrap() > 0.0);

        let cancelled_study = study("topology-cancelled");
        let resolved = resolve_request(&cancelled_study, &resolver).unwrap();
        let resolved_faces =
            resolve_fem_face_tags(&resolved.manifest.tagged_anchors, &resolved.boundary).unwrap();
        let authored = authored_study_from_core(
            &resolved.program,
            &cancelled_study.analysis_name,
            &resolved_faces,
            resolved.budgets.clone(),
        )
        .unwrap();
        let (
            material,
            load_cases,
            fixed_face_group_indices,
            passive_solid_regions,
            passive_void_regions,
        ) = topology_inputs_from_authored_study(&authored, &resolved.boundary).unwrap();
        let cancelled = run_fem_topology_artifact_with_resolver(
            FemTopologyArtifactRunRequest {
                job_id: cancelled_study.job_id,
                analysis_identity_digest: stepped.analysis_identity_digest.clone(),
                mesh_content_digest: stepped.mesh_content_digest.clone(),
                material,
                load_cases,
                fixed_face_group_indices,
                passive_solid_regions,
                passive_void_regions,
                relative_solver_tolerance: cancelled_study.control.relative_solver_tolerance,
                controls: topology_controls_from_authored(
                    authored.topology_controls.as_ref().unwrap(),
                    &runtime_policy(1),
                ),
                resume_state_digest: None,
            },
            &resolver,
            &AtomicBool::new(true),
        )
        .expect("cancelled optimizer publishes state-only checkpoint");
        assert_eq!(cancelled.termination, "cancelled");
        assert!(cancelled.result_digest.is_none());
        assert!(cancelled.final_compliance.is_none());
        assert!(cancelled.density_path.is_none());
        assert!(cancelled.preview_vtu_path.is_none());
        assert!(PathBuf::from(&cancelled.checkpoint_path).is_file());
        let resumed_after_cancel = run_fem_topology_with_resolver(
            FemTopologyRunRequest {
                study: study("topology-resume-cancelled"),
                resume_state_digest: Some(cancelled.state_digest),
            },
            &runtime_policy(1),
            &resolver,
            &AtomicBool::new(false),
        )
        .expect("state-only checkpoint resumes through authored pipeline");
        let stepped_artifact = crate::services::fem_topology_artifacts::load_fem_topology_artifact(
            &resolver,
            &stepped.input_digest,
            &stepped.state_digest,
            64 * 1024 * 1024,
        )
        .expect("load stepped topology artifact");
        let resumed_after_cancel_artifact =
            crate::services::fem_topology_artifacts::load_fem_topology_artifact(
                &resolver,
                &resumed_after_cancel.input_digest,
                &resumed_after_cancel.state_digest,
                64 * 1024 * 1024,
            )
            .expect("load topology artifact resumed after cancellation");
        assert_eq!(
            resumed_after_cancel_artifact.state.design_densities,
            stepped_artifact.state.design_densities
        );
        assert_eq!(
            resumed_after_cancel_artifact.state.mma87,
            stepped_artifact.state.mma87
        );
        assert_eq!(
            resumed_after_cancel_artifact.state.iterations,
            stepped_artifact.state.iterations
        );
        assert_eq!(resumed_after_cancel.state_digest, stepped.state_digest);
        assert_eq!(resumed_after_cancel.result_digest, stepped.result_digest);

        let resumed = run_fem_topology_with_resolver(
            FemTopologyRunRequest {
                study: study("topology-resume"),
                resume_state_digest: Some(stepped.state_digest.clone()),
            },
            &runtime_policy(1),
            &resolver,
            &AtomicBool::new(false),
        )
        .expect("resume from internal checkpoint");
        let uninterrupted = run_fem_topology_with_resolver(
            FemTopologyRunRequest {
                study: study("topology-uninterrupted"),
                resume_state_digest: None,
            },
            &runtime_policy(2),
            &resolver,
            &AtomicBool::new(false),
        )
        .expect("uninterrupted authored topology operation");
        assert_eq!(resumed.termination, "maximumIterations");
        assert_eq!(resumed.iteration_count, 2);
        assert_eq!(
            resumed.analysis_identity_digest,
            uninterrupted.analysis_identity_digest
        );
        assert_eq!(
            resumed.mesh_content_digest,
            uninterrupted.mesh_content_digest
        );
        let topology_input_dir = PathBuf::from(&uninterrupted.checkpoint_path)
            .parent()
            .and_then(Path::parent)
            .expect("topology input artifact directory")
            .to_path_buf();
        let durable_state_count = fs::read_dir(&topology_input_dir)
            .expect("read topology state directories")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .count();
        assert!(
            durable_state_count >= 2,
            "every completed outer iteration must leave a resumable state artifact"
        );
        let resumed_artifact = crate::services::fem_topology_artifacts::load_fem_topology_artifact(
            &resolver,
            &resumed.input_digest,
            &resumed.state_digest,
            64 * 1024 * 1024,
        )
        .expect("load resumed topology artifact");
        let uninterrupted_artifact =
            crate::services::fem_topology_artifacts::load_fem_topology_artifact(
                &resolver,
                &uninterrupted.input_digest,
                &uninterrupted.state_digest,
                64 * 1024 * 1024,
            )
            .expect("load uninterrupted topology artifact");
        assert_eq!(
            resumed_artifact.state.design_densities,
            uninterrupted_artifact.state.design_densities
        );
        assert_eq!(
            resumed_artifact.state.mma87,
            uninterrupted_artifact.state.mma87
        );
        assert_eq!(
            resumed_artifact.state.iterations,
            uninterrupted_artifact.state.iterations
        );
        assert_eq!(resumed.state_digest, uninterrupted.state_digest);
        assert_eq!(resumed.result_digest, uninterrupted.result_digest);
        assert_eq!(resumed.final_compliance, uninterrupted.final_compliance);
        assert_eq!(
            resumed.final_volume_fraction,
            uninterrupted.final_volume_fraction
        );
        assert!(!resumed.exact_brep);
        assert!(!resumed.production_step);
        assert!(!resumed.engineering_accepted);
        fs::remove_dir_all(root).expect("cleanup topology orchestration fixture");
    }
}
