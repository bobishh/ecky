use super::{
    artifact_bundle_digest, clear_session_thread_render_preview_durable, now_secs,
    persist_agent_session, resolve_session_render_preview_for_request, resolve_turn_working_target,
    try_record_agent_error, AgentContext,
};
use crate::contracts::{AppError, AppErrorCode, AppResult, RenderSnapshot};
use crate::db;
use crate::mcp::contracts::{
    FemVerifiedPublishRequest, ThreadForkRequest, ThreadForkResponse, VersionDeleteRequest,
    VersionDeleteResponse, VersionRestoreRequest, VersionRestoreResponse, VersionSaveRequest,
    VersionSaveResponse,
};
use crate::models::{AppState, PathResolver};
use crate::services::agent_versions::{
    save_or_update_agent_version_for_session, SaveOrUpdateAgentVersionRequest,
};
use crate::services::history;
use uuid::Uuid;

pub async fn handle_publish_fem_verified_result(
    state: &AppState,
    app: &dyn PathResolver,
    req: FemVerifiedPublishRequest,
    ctx: &AgentContext,
) -> AppResult<VersionSaveResponse> {
    let ctx = ctx.with_override(&req.identity);
    let preview = resolve_session_render_preview_for_request(
        state,
        &ctx,
        req.thread_id.as_deref(),
        req.message_id.as_deref(),
    )
    .await?
    .ok_or_else(|| {
        AppError::validation(
            "No preview draft is available for FEM evidence publication. Render a preview first.",
        )
    })?;
    let result = crate::commands::fem::read_fem_result_with_resolver(
        crate::contracts::FemResultReadRequest {
            analysis_identity_digest: req.analysis_identity_digest,
            solution_digest: req.solution_digest,
            maximum_result_bytes: 128 * 1024 * 1024,
        },
        app,
    )?;
    let current = crate::commands::fem::validate_fem_study_with_resolver(
        crate::contracts::FemStudyRequest {
            job_id: format!("fem-publish-{}", Uuid::new_v4()),
            model_id: preview.artifact_bundle.model_id.clone(),
            source: preview.design_output.macro_code.clone(),
            analysis_name: req.analysis_name,
            budgets: fem_publish_validation_budgets(),
            control: fem_publish_validation_control(),
        },
        app,
    )?;
    validate_fem_result_for_preview(
        &result,
        &current.source_digest,
        &current.boundary_digest,
        &req.result_digest,
    )?;
    let guided_request_id =
        require_green_verification_for_preview(state, &preview, req.capture_guided_result.as_ref())
            .await?;
    let message_id = preview
        .base_message_id
        .clone()
        .ok_or_else(|| AppError::persistence("FEM preview has no durable version."))?;
    if let Some(request_id) = guided_request_id.as_deref() {
        let conn = state.db.lock().await;
        crate::capture_runs::complete_guided_reconstruction(
            &conn,
            request_id,
            &message_id,
            Some(&preview.artifact_bundle.model_id),
        )?;
    }
    clear_session_thread_render_preview_durable(state, &ctx.session_id, &preview.thread_id).await?;
    Ok(VersionSaveResponse {
        thread_id: preview.thread_id,
        message_id,
        model_id: preview.artifact_bundle.model_id,
    })
}

fn fem_publish_validation_budgets() -> crate::contracts::FemBudgetLimitsDto {
    crate::contracts::FemBudgetLimitsDto {
        boundary_triangles: 250_000,
        tet4_cells: 500_000,
        nodes: 150_000,
        dofs: 450_000,
        sparse_nonzeros: 30_000_000,
        result_bytes: 128 * 1024 * 1024,
        convergence_levels: 3,
    }
}

fn fem_publish_validation_control() -> crate::contracts::FemPipelineControlDto {
    crate::contracts::FemPipelineControlDto {
        envelope_mm: 0.1,
        minimum_scaled_jacobian: 1.0e-6,
        maximum_runtime_ms: 10 * 60 * 1000,
        relative_solver_tolerance: 1.0e-8,
        thread_count: 0,
    }
}

async fn require_green_verification_for_preview(
    state: &AppState,
    preview: &super::SessionRenderPreview,
    guided_result: Option<&crate::contracts::CaptureGuidedCommitResult>,
) -> AppResult<Option<String>> {
    let snapshot = preview_render_snapshot(preview)?;
    let pending_guide = {
        let conn = state.db.lock().await;
        crate::capture_runs::pending_guided_reconstruction_for_thread(&conn, &preview.thread_id)?
    };
    if let Some(pending) = pending_guide.as_ref() {
        let source_stl_path_result = {
            let conn = state.db.lock().await;
            crate::capture_runs::selected_source_path(&conn, &pending.run_id).map(|value| value.0)
        };
        let source_stl_path = match source_stl_path_result {
            Ok(path) => path,
            Err(error) => {
                let conn = state.db.lock().await;
                crate::capture_runs::record_guided_reconstruction_validation_error(
                    &conn,
                    &pending.request_id,
                    &error.message,
                )?;
                return Err(error);
            }
        };
        let topology_path = match std::path::Path::new(&snapshot.artifact_bundle.manifest_path)
            .parent()
        {
            Some(directory) => crate::ecky_cad_host::topology_artifact::topology_path(directory),
            None => {
                let error = AppError::validation(
                    "Guided reconstruction preview manifest has no artifact directory.",
                );
                let conn = state.db.lock().await;
                crate::capture_runs::record_guided_reconstruction_validation_error(
                    &conn,
                    &pending.request_id,
                    &error.message,
                )?;
                return Err(error);
            }
        };
        let validation = (|| {
            let inferred_regions = validate_capture_guided_commit_result(pending, guided_result)?;
            let mut provenance =
                crate::capture_brep_validation::validate_capture_direct_occt_snapshot(
                    &pending.guide,
                    &snapshot,
                    &topology_path,
                )?;
            provenance.inferred_regions = inferred_regions;
            let part_ids = provenance
                .correspondences
                .iter()
                .map(|correspondence| correspondence.part_id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            if part_ids.is_empty() {
                return Err(AppError::validation(
                    "Guided reconstruction has no exact BRep parts for observed deviation.",
                ));
            }
            let bundle_dir = topology_path.parent().ok_or_else(|| {
                AppError::validation("Guided reconstruction topology has no artifact directory.")
            })?;
            let boundaries = part_ids
                .iter()
                .map(|part_id| {
                    crate::ecky_cad_host::analysis_boundary::load_direct_occt_analysis_boundary_surface(
                        bundle_dir,
                        part_id,
                    )
                })
                .collect::<AppResult<Vec<_>>>()?;
            let deviation = crate::capture_deviation::compute_observed_mesh_to_brep_deviation_across_boundaries(
                &source_stl_path,
                &pending.guide,
                &boundaries,
                &provenance.geometry_digest,
                100_000,
                capture_deviation_outlier_threshold(&pending.guide)?,
            )?;
            Ok::<_, AppError>((provenance, deviation))
        })();
        let (provenance, deviation) = match validation {
            Ok(result) => result,
            Err(error) => {
                let conn = state.db.lock().await;
                crate::capture_runs::record_guided_reconstruction_validation_error(
                    &conn,
                    &pending.request_id,
                    &error.message,
                )?;
                return Err(error);
            }
        };
        let conn = state.db.lock().await;
        crate::capture_runs::record_guided_reconstruction_validation_success(
            &conn,
            &pending.request_id,
            &provenance,
            &deviation,
        )?;
    } else if guided_result.is_some() {
        return Err(AppError::validation(
            "captureGuidedResult was supplied without a pending guided reconstruction in this thread.",
        ));
    }
    let record = {
        let conn = state.db.lock().await;
        db::get_verification_record(&conn, &snapshot.snapshot_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    };
    let is_current_green_record = record.as_ref().is_some_and(|record| {
        record.passed
            && record.snapshot_id == snapshot.snapshot_id
            && record.artifact_digest == snapshot.artifact_digest
    });
    if is_current_green_record {
        return Ok(pending_guide.map(|pending| pending.request_id));
    }

    let verification_evidence = record.as_ref().map_or_else(
        || "verificationRecord=missing".to_string(),
        |record| {
            format!(
                "verificationSnapshotId={} verificationArtifactDigest={} verificationPassed={}",
                record.snapshot_id, record.artifact_digest, record.passed
            )
        },
    );
    Err(AppError::with_details(
        AppErrorCode::Conflict,
        "FEM evidence requires an explicit green verification for the same preview.",
        format!(
            "previewId={} snapshotId={} artifactDigest={} {}",
            preview.preview_id,
            snapshot.snapshot_id,
            snapshot.artifact_digest,
            verification_evidence
        ),
    )
    .with_operation("fem_publish_verified_result"))
}

fn capture_deviation_outlier_threshold(
    guide: &crate::contracts::CaptureReconstructionGuide,
) -> AppResult<f64> {
    guide
        .feature_expectations
        .iter()
        .filter(|expectation| expectation.required_for_acceptance)
        .flat_map(|expectation| {
            [
                expectation.position_tolerance_mm,
                expectation.radial_tolerance_mm,
            ]
            .into_iter()
            .flatten()
        })
        .fold(None::<f64>, |maximum, value| {
            Some(maximum.map_or(value, |current| current.max(value)))
        })
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| {
            AppError::validation(
                "Guided reconstruction requires an explicit acceptance tolerance for observed deviation.",
            )
        })
}

fn validate_capture_guided_commit_result(
    pending: &crate::capture_runs::PendingGuidedReconstruction,
    result: Option<&crate::contracts::CaptureGuidedCommitResult>,
) -> AppResult<Vec<String>> {
    let result = result.ok_or_else(|| {
        AppError::validation(format!(
            "Guided reconstruction request '{}' requires captureGuidedResult before commit.",
            pending.request_id
        ))
    })?;
    result.validate().map_err(AppError::validation)?;
    if result.request_id != pending.request_id {
        return Err(AppError::conflict(format!(
            "Guided commit result request '{}' differs from pending request '{}'.",
            result.request_id, pending.request_id
        )));
    }
    if result.guide_canonical_digest != pending.guide.canonical_digest {
        return Err(AppError::conflict(
            "Guided commit result guide digest differs from pending guide identity.",
        ));
    }
    if !result.unresolved_assumptions.is_empty() {
        return Err(AppError::validation(format!(
            "Guided reconstruction remains pending: unresolved assumptions require targeted user confirmation: {}",
            result.unresolved_assumptions.join("; ")
        )));
    }
    Ok(result.inferred_regions.clone())
}

fn validate_fem_result_for_preview(
    result: &crate::contracts::FemResultReadResponse,
    preview_source_digest: &str,
    preview_boundary_digest: &str,
    expected_result_digest: &str,
) -> AppResult<()> {
    if !result.decision_ready {
        return Err(AppError::validation(format!(
            "FEM result is not decision-ready: {}",
            result
                .decision_readiness_error
                .as_deref()
                .unwrap_or("engineering evidence is red")
        )));
    }
    let result_source_digest = result.source_digest.trim();
    if result_source_digest.is_empty() {
        return Err(AppError::conflict(
            "FEM result has no exact source identity; rerun study against current preview.",
        ));
    }
    if result_source_digest != preview_source_digest {
        return Err(AppError::conflict(format!(
            "FEM result source is stale: result '{result_source_digest}', preview '{preview_source_digest}'."
        )));
    }
    if result.source_boundary_digest != preview_boundary_digest {
        return Err(AppError::conflict(format!(
            "FEM result boundary is stale: result '{}', preview '{}'.",
            result.source_boundary_digest, preview_boundary_digest
        )));
    }
    if result.result_digest != expected_result_digest {
        return Err(AppError::conflict(format!(
            "FEM result digest differs from requested immutable evidence: loaded '{}', requested '{expected_result_digest}'.",
            result.result_digest
        )));
    }
    Ok(())
}

fn preview_render_snapshot(preview: &super::SessionRenderPreview) -> AppResult<RenderSnapshot> {
    crate::services::render_snapshot::build_render_snapshot(
        crate::services::render_snapshot::RenderSnapshotInput {
            design: &preview.design_output,
            effective_params: &preview.design_output.initial_params,
            artifact_bundle: &preview.artifact_bundle,
            model_manifest: &preview.model_manifest,
        },
    )
}

pub async fn handle_saved_target_version(
    state: &AppState,
    app: &dyn PathResolver,
    req: VersionSaveRequest,
    ctx: &AgentContext,
) -> AppResult<VersionSaveResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target = crate::services::target::resolve_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;
        drop(conn);
        let target = resolve_turn_working_target(
            state,
            app,
            ctx,
            target,
            format!(
                "{} created a working version for this turn.",
                ctx.agent_label
            ),
        )
        .await?;
        let conn = state.db.lock().await;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        let mut design_output = target
            .design
            .clone()
            .ok_or_else(|| AppError::validation("Target has no design output."))?;
        let model_id = target
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());
        tracked_model_id = model_id.clone();

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "",
        )?;

        drop(conn);
        if let Some(title) = req.title.clone() {
            design_output.title = title;
        }
        if let Some(version_name) = req.version_name.clone() {
            design_output.version_name = version_name;
        } else {
            design_output.version_name.clear();
        }

        let save_result = save_or_update_agent_version_for_session(
            state,
            app,
            SaveOrUpdateAgentVersionRequest {
                session_id: ctx.session_id.clone(),
                thread_id: target.thread_id.clone(),
                base_message_id: target.message_id.clone(),
                model_id: model_id.clone(),
                design_output,
                artifact_bundle: target.artifact_bundle.clone(),
                model_manifest: target.model_manifest.clone(),
                updated_at: now_secs(),
                response_text_created: String::new(),
                response_text_updated: String::new(),
                preserve_existing_title: req.title.is_none(),
                preserve_existing_version_name: req.version_name.is_none(),
                force_create_new_message: false,
                announce_created_working_version: false,
            },
        )
        .await?;
        tracked_message_id = Some(save_result.message_id.clone());
        tracked_model_id = save_result.model_id.clone();

        Ok(VersionSaveResponse {
            thread_id: target.thread_id,
            message_id: save_result.message_id,
            model_id: save_result.model_id.unwrap_or_default(),
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
            state,
            &conn,
            ctx,
            tracked_thread_id,
            tracked_message_id,
            tracked_model_id,
            err,
        );
    }

    result
}

pub async fn handle_version_restore(
    state: &AppState,
    req: VersionRestoreRequest,
    ctx: &AgentContext,
) -> AppResult<VersionRestoreResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = None;
    let tracked_message_id = Some(req.message_id.clone());

    let result = async {
        let conn = state.db.lock().await;

        persist_agent_session(
            &conn,
            ctx,
            None,
            tracked_message_id.clone(),
            None,
            "restoring_version",
            "",
        )?;

        history::restore_version(&conn, &req.message_id)?;
        let tid = db::get_message_thread_id(&conn, &req.message_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| AppError::not_found("Restored message not found."))?;
        tracked_thread_id = Some(tid.clone());
        state.emit_history_changed(
            Some(tid.clone()),
            Some(req.message_id.clone()),
            "versionRestored",
        );
        let artifact_digest = db::get_message_runtime_and_thread(&conn, &req.message_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .and_then(|(artifact_bundle, _, _)| {
                artifact_bundle.as_ref().map(artifact_bundle_digest)
            });

        persist_agent_session(
            &conn,
            ctx,
            Some(tid.clone()),
            tracked_message_id.clone(),
            None,
            "idle",
            "",
        )?;

        Ok(VersionRestoreResponse {
            thread_id: tid,
            message_id: req.message_id.clone(),
            artifact_digest,
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
            state,
            &conn,
            ctx,
            tracked_thread_id,
            tracked_message_id,
            None,
            err,
        );
    }

    result
}

pub async fn handle_version_delete(
    state: &AppState,
    req: VersionDeleteRequest,
    ctx: &AgentContext,
) -> AppResult<VersionDeleteResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = None;
    let tracked_message_id = Some(req.message_id.clone());

    let result = async {
        let conn = state.db.lock().await;
        let thread_id = db::get_message_thread_id(&conn, &req.message_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .ok_or_else(|| AppError::not_found("Version message not found."))?;
        tracked_thread_id = Some(thread_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            Some(thread_id.clone()),
            tracked_message_id.clone(),
            None,
            "deleting_version",
            "",
        )?;

        history::delete_version(&conn, &req.message_id)?;
        state.emit_history_changed(
            Some(thread_id.clone()),
            Some(req.message_id.clone()),
            "versionDeleted",
        );

        persist_agent_session(&conn, ctx, Some(thread_id.clone()), None, None, "idle", "")?;

        Ok(VersionDeleteResponse {
            thread_id,
            message_id: req.message_id.clone(),
            deleted: true,
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
            state,
            &conn,
            ctx,
            tracked_thread_id,
            tracked_message_id,
            None,
            err,
        );
    }

    result
}

pub async fn handle_thread_fork_from_target(
    state: &AppState,
    app: &dyn PathResolver,
    req: ThreadForkRequest,
    ctx: &AgentContext,
) -> AppResult<ThreadForkResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target = crate::services::target::resolve_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());

        let mut design_output = target
            .design
            .clone()
            .ok_or_else(|| AppError::validation("Target has no design output."))?;
        let model_id = target
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());
        tracked_model_id = model_id.clone();

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Forking target into a new thread.",
        )?;

        drop(conn);

        let new_thread_id = Uuid::new_v4().to_string();
        if let Some(title) = req.title.clone() {
            design_output.title = title;
        }
        if let Some(version_name) = req.version_name.clone() {
            design_output.version_name = version_name;
        } else {
            design_output.version_name.clear();
        }

        let save_result = save_or_update_agent_version_for_session(
            state,
            app,
            SaveOrUpdateAgentVersionRequest {
                session_id: ctx.session_id.clone(),
                thread_id: new_thread_id.clone(),
                base_message_id: target.message_id.clone(),
                model_id: model_id.clone(),
                design_output,
                artifact_bundle: target.artifact_bundle.clone(),
                model_manifest: target.model_manifest.clone(),
                updated_at: now_secs(),
                response_text_created: format!("{} forked this version via MCP.", ctx.agent_label),
                response_text_updated: format!(
                    "{} updated the forked MCP version.",
                    ctx.agent_label
                ),
                preserve_existing_title: false,
                preserve_existing_version_name: false,
                force_create_new_message: true,
                announce_created_working_version: false,
            },
        )
        .await?;
        tracked_message_id = Some(save_result.message_id.clone());
        tracked_model_id = save_result.model_id.clone();

        Ok(ThreadForkResponse {
            thread_id: new_thread_id,
            message_id: save_result.message_id,
            model_id: save_result.model_id.unwrap_or_default(),
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
            state,
            &conn,
            ctx,
            tracked_thread_id,
            tracked_message_id,
            tracked_model_id,
            err,
        );
    }

    result
}

#[cfg(test)]
mod guided_commit_tests {
    use super::*;

    fn pending() -> crate::capture_runs::PendingGuidedReconstruction {
        let mut guide = crate::contracts::CaptureReconstructionGuide::test_fixture();
        guide.canonical_digest = guide.compute_canonical_digest().unwrap();
        crate::capture_runs::PendingGuidedReconstruction {
            run_id: "run-1".into(),
            request_id: "capture-guide:sha256:req".into(),
            guide,
        }
    }

    fn result(
        pending: &crate::capture_runs::PendingGuidedReconstruction,
    ) -> crate::contracts::CaptureGuidedCommitResult {
        crate::contracts::CaptureGuidedCommitResult {
            schema_version: 1,
            request_id: pending.request_id.clone(),
            guide_canonical_digest: pending.guide.canonical_digest.clone(),
            unresolved_assumptions: Vec::new(),
            inferred_regions: vec!["quarter completed by declared symmetry".into()],
        }
    }

    #[test]
    fn guided_commit_requires_exact_identity_and_no_unresolved_assumptions() {
        let pending = pending();
        assert_eq!(
            capture_deviation_outlier_threshold(&pending.guide).unwrap(),
            0.1
        );
        let error = validate_capture_guided_commit_result(&pending, None)
            .expect_err("missing guided result")
            .message;
        assert!(error.contains("requires captureGuidedResult"), "{error}");

        let mut unresolved = result(&pending);
        unresolved.unresolved_assumptions = vec!["hidden bore diameter unknown".into()];
        let error = validate_capture_guided_commit_result(&pending, Some(&unresolved))
            .expect_err("unresolved assumption")
            .message;
        assert!(error.contains("remains pending"), "{error}");
        assert!(error.contains("targeted user confirmation"), "{error}");

        let mut stale = result(&pending);
        stale.guide_canonical_digest = "sha256:stale".into();
        let error = validate_capture_guided_commit_result(&pending, Some(&stale))
            .expect_err("stale guide result")
            .message;
        assert!(
            error.contains("differs from pending guide identity"),
            "{error}"
        );

        assert_eq!(
            validate_capture_guided_commit_result(&pending, Some(&result(&pending)))
                .expect("resolved guided result"),
            vec!["quarter completed by declared symmetry"]
        );

        let mut no_threshold = pending.guide.clone();
        no_threshold.feature_expectations[0].position_tolerance_mm = None;
        assert!(capture_deviation_outlier_threshold(&no_threshold)
            .expect_err("explicit threshold")
            .message
            .contains("explicit acceptance tolerance"));
    }
}

#[cfg(test)]
mod fem_commit_tests {
    use super::*;

    fn result(
        source_digest: &str,
        boundary_digest: &str,
        decision_ready: bool,
    ) -> crate::contracts::FemResultReadResponse {
        crate::contracts::FemResultReadResponse {
            source_digest: source_digest.to_string(),
            analysis_identity_digest: "sha256:analysis".into(),
            solution_digest: "sha256:solution".into(),
            result_digest: "sha256:result".into(),
            mesh_content_digest: "sha256:mesh".into(),
            source_boundary_digest: boundary_digest.into(),
            decision_ready,
            decision_readiness_error: (!decision_ready).then(|| "missing load evidence".into()),
            manifest_path: "/immutable/fem/manifest.json".into(),
            arrays: Vec::new(),
            summary: crate::contracts::FemResultSummaryDto {
                maximum_displacement_mm: 0.1,
                maximum_von_mises_mpa: 1.0,
                maximum_principal_stress_mpa: 1.0,
                volume_mm3: 1.0,
                mass_kg: 0.001,
                minimum_yield_safety_factor: Some(2.0),
                equilibrium_relative_imbalance: 1e-12,
                solver_relative_residual: 1e-12,
                minimum_scaled_jacobian: 0.5,
                node_count: 4,
                tet4_cell_count: 1,
                extrema: Vec::new(),
            },
            support_reactions: Vec::new(),
            engineering_evidence: crate::contracts::FemEngineeringEvidenceDto {
                question: crate::contracts::FemEngineeringQuestionDto {
                    question_id: "q".into(),
                    statement: "safe?".into(),
                    decision: "accept".into(),
                    acceptance_metric_ids: vec!["stress".into()],
                },
                idealization: crate::contracts::FemIdealizationDto {
                    artifact_digest: "sha256:idealization".into(),
                    kind: "exactSolid".into(),
                    source_geometry_digest: "sha256:geometry".into(),
                    analysis_geometry_digest: "sha256:geometry".into(),
                    manufacturing_geometry_digest: "sha256:geometry".into(),
                    affected_topology_ids: Vec::new(),
                    justification: "exact".into(),
                    expected_influence_percent: 0.0,
                    accepted_by_user: true,
                },
                inputs: Vec::new(),
                assumptions: Vec::new(),
                applicability: Vec::new(),
                sensitivity: None,
                validation_evidence: Vec::new(),
                verification_layers: Vec::new(),
            },
            acceptance_evaluations: Vec::new(),
        }
    }

    #[test]
    fn fem_verified_commit_rejects_red_stale_or_unbound_result() {
        let red = validate_fem_result_for_preview(
            &result("sha256:source", "sha256:boundary", false),
            "sha256:source",
            "sha256:boundary",
            "sha256:result",
        )
        .expect_err("red result");
        assert!(red.message.contains("missing load evidence"), "{red:?}");

        let stale_source = validate_fem_result_for_preview(
            &result("sha256:old", "sha256:boundary", true),
            "sha256:source",
            "sha256:boundary",
            "sha256:result",
        )
        .expect_err("stale source");
        assert!(stale_source.message.contains("source"), "{stale_source:?}");

        let stale_boundary = validate_fem_result_for_preview(
            &result("sha256:source", "sha256:old-boundary", true),
            "sha256:source",
            "sha256:boundary",
            "sha256:result",
        )
        .expect_err("stale boundary");
        assert!(
            stale_boundary.message.contains("boundary"),
            "{stale_boundary:?}"
        );

        let missing_source = validate_fem_result_for_preview(
            &result("", "sha256:boundary", true),
            "sha256:source",
            "sha256:boundary",
            "sha256:result",
        )
        .expect_err("legacy artifact cannot bind exact source");
        assert!(
            missing_source.message.contains("source identity"),
            "{missing_source:?}"
        );

        validate_fem_result_for_preview(
            &result("sha256:source", "sha256:boundary", true),
            "sha256:source",
            "sha256:boundary",
            "sha256:result",
        )
        .expect("current green result");
    }
}
