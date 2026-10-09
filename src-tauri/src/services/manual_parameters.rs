use crate::contracts::{
    AppError, AppResult, ArtifactBundle, DesignOutput, DesignParams, InteractionMode,
    MessageStatus, ModelManifest, ModelSourceKind,
};
use crate::models::{AppState, PathResolver};
use crate::services::design::{AddManualVersionRequest, add_manual_version};
use crate::services::render_snapshot::{RenderSnapshotInput, build_render_snapshot};
use crate::services::session::{build_saved_version_snapshot, write_last_snapshot};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManualParameterApplyRequest {
    pub thread_id: String,
    pub target_message_id: String,
    pub parameters: DesignParams,
    #[serde(default)]
    pub persist: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub version_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManualParameterApplyResponse {
    pub thread_id: String,
    pub base_message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub status: MessageStatus,
    pub design_output: DesignOutput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_bundle: Option<ArtifactBundle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_manifest: Option<ModelManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}

fn requested_design(
    base: &DesignOutput,
    parameters: DesignParams,
    title: Option<&str>,
    version_name: Option<&str>,
) -> DesignOutput {
    let mut design = base.clone();
    if let Some(title) = title.map(str::trim).filter(|value| !value.is_empty()) {
        design.title = title.to_string();
    }
    if let Some(version_name) = version_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        design.version_name = version_name.to_string();
    }
    design.response = "Parameter version appended.".to_string();
    design.interaction_mode = InteractionMode::Tune;
    design.initial_params = parameters;
    design
}

fn version_request(
    thread_id: &str,
    design: &DesignOutput,
    artifact_bundle: Option<ArtifactBundle>,
    model_manifest: Option<ModelManifest>,
    status: MessageStatus,
    error_message: Option<String>,
) -> AddManualVersionRequest {
    AddManualVersionRequest {
        thread_id: thread_id.to_string(),
        title: design.title.clone(),
        version_name: design.version_name.clone(),
        macro_code: design.macro_code.clone(),
        source_language: Some(design.source_language),
        geometry_backend: Some(design.geometry_backend),
        parameters: design.initial_params.clone(),
        ui_spec: design.ui_spec.clone(),
        post_processing: design.post_processing.clone(),
        artifact_bundle,
        model_manifest,
        response_text: Some(design.response.clone()),
        agent_origin: None,
        status: Some(status),
        error_message,
    }
}

async fn stored_design(state: &AppState, message_id: &str) -> AppResult<DesignOutput> {
    let conn = state.db.lock().await;
    crate::db::get_message_output_and_thread(&conn, message_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .map(|(design, _)| design)
        .ok_or_else(|| {
            AppError::persistence(format!(
                "Manual parameter version {} disappeared after persistence.",
                message_id
            ))
        })
}

async fn persist_failure(
    request: &ManualParameterApplyRequest,
    message_id: Option<String>,
    attempted_design: DesignOutput,
    error: AppError,
    state: &AppState,
    _app: &dyn PathResolver,
) -> AppResult<ManualParameterApplyResponse> {
    if let Some(message_id) = message_id.as_deref() {
        let error_text = error.to_string();
        let conn = state.db.lock().await;
        crate::db::update_message_status_and_output(
            &conn,
            message_id,
            crate::db::MessageStatusUpdate {
                status: &MessageStatus::Error,
                output: Some(&attempted_design),
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                visual_kind: None,
                content: Some(&error_text),
            },
        )
        .map_err(|db_error| AppError::persistence(db_error.to_string()))?;
        drop(conn);
        state.emit_history_changed(
            Some(request.thread_id.clone()),
            Some(message_id.to_string()),
            "parameterVersionFailed",
        );
    }
    let design_output = if let Some(message_id) = message_id.as_deref() {
        stored_design(state, message_id).await?
    } else {
        attempted_design
    };
    Ok(ManualParameterApplyResponse {
        thread_id: request.thread_id.clone(),
        base_message_id: request.target_message_id.clone(),
        message_id,
        status: MessageStatus::Error,
        design_output,
        artifact_bundle: None,
        model_manifest: None,
        snapshot_id: None,
        error: Some(error),
    })
}

async fn persist_success(
    request: &ManualParameterApplyRequest,
    message_id: &str,
    design: &DesignOutput,
    artifact_bundle: &ArtifactBundle,
    model_manifest: &ModelManifest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<()> {
    let configured_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    crate::db::update_message_status_and_output(
        &conn,
        message_id,
        crate::db::MessageStatusUpdate {
            status: &MessageStatus::Success,
            output: Some(design),
            usage: None,
            artifact_bundle: Some(artifact_bundle),
            model_manifest: Some(model_manifest),
            structural_verification: None,
            visual_kind: None,
            content: Some("Parameter version appended."),
        },
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    crate::thread_source_binding::refresh_on_version_append(
        app,
        &conn,
        configured_root.as_deref(),
        &request.thread_id,
        &design.title,
        &design.macro_code,
        message_id,
        Some(&artifact_bundle.model_id),
        Some(message_id),
    )?;
    if let Err(error) = crate::component_package_runtime::capture_latest_successful_components(
        app,
        &conn,
        &request.thread_id,
        message_id,
        &design.macro_code,
    ) {
        state.push_log(format!(
            "Component capture failed for version {message_id}: {error}"
        ));
    }
    drop(conn);
    state.emit_history_changed(
        Some(request.thread_id.clone()),
        Some(message_id.to_string()),
        "parameterVersionRendered",
    );
    Ok(())
}

async fn render_parameters(
    design: &DesignOutput,
    previous_bundle: Option<&ArtifactBundle>,
    previous_manifest: Option<&ModelManifest>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<(ArtifactBundle, ModelManifest)> {
    let (artifact_bundle, model_manifest) = if design.macro_code.trim().is_empty() {
        let bundle = previous_bundle.ok_or_else(|| {
            AppError::validation("Imported parameter apply requires target artifact runtime.")
        })?;
        let manifest = previous_manifest.ok_or_else(|| {
            AppError::validation("Imported parameter apply requires target model manifest.")
        })?;
        if !matches!(
            bundle.source_kind,
            ModelSourceKind::ImportedFcstd
                | ModelSourceKind::ImportedStep
                | ModelSourceKind::ImportedMesh
        ) {
            return Err(AppError::validation(
                "Target has no executable source or imported component runtime.",
            ));
        }
        let _guard = state.acquire_geometry_render().await;
        crate::freecad::apply_imported_model(
            bundle,
            manifest,
            &design.initial_params,
            crate::services::render::configured_freecad_cmd(state).as_deref(),
            app,
        )?
    } else {
        crate::contracts::validate_design_params(&design.initial_params, &design.ui_spec)?;
        let config_default_backend = state.config.lock().unwrap().default_geometry_backend;
        let context = crate::mcp::handlers::resolve_macro_authoring_context(
            design.source_language,
            design.geometry_backend,
            &design.macro_dialect,
            None,
            config_default_backend,
        )?;
        let artifact_bundle = crate::services::render::render_model_with_previous_manifest(
            &design.macro_code,
            &design.initial_params,
            Some(design.macro_dialect.clone()),
            Some(context.geometry_backend),
            design.post_processing.as_ref(),
            previous_manifest,
            state,
            app,
        )
        .await?;
        let manifest = crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
        (artifact_bundle, manifest)
    };

    let model_manifest = crate::mcp::handlers::carry_forward_semantic_manifest(
        previous_manifest,
        model_manifest,
        &artifact_bundle,
    );
    let model_manifest = crate::model_runtime::write_model_manifest(
        app,
        &artifact_bundle.model_id,
        &model_manifest,
    )?;
    let artifact_bundle =
        crate::model_runtime::read_artifact_bundle(app, &artifact_bundle.model_id)?;
    crate::contracts::validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;
    Ok((artifact_bundle, model_manifest))
}

pub struct PendingManualParameterApply {
    request: ManualParameterApplyRequest,
    attempted_design: DesignOutput,
    previous_bundle: Option<ArtifactBundle>,
    previous_manifest: Option<ModelManifest>,
    message_id: Option<String>,
}

pub async fn begin_manual_parameter_apply(
    request: ManualParameterApplyRequest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<(ManualParameterApplyResponse, PendingManualParameterApply)> {
    let target = {
        let conn = state.db.lock().await;
        crate::services::target::resolve_target(
            &conn,
            app,
            Some(request.thread_id.clone()),
            Some(request.target_message_id.clone()),
        )?
    };
    let base_design = target
        .design
        .ok_or_else(|| AppError::validation("Target has no design output."))?;
    let attempted_design = requested_design(
        &base_design,
        request.parameters.clone(),
        request.title.as_deref(),
        request.version_name.as_deref(),
    );

    let message_id = if request.persist {
        Some(
            add_manual_version(
                version_request(
                    &request.thread_id,
                    &attempted_design,
                    None,
                    None,
                    MessageStatus::Working,
                    Some("Parameter version pending render.".to_string()),
                ),
                state,
                app,
            )
            .await?,
        )
    } else {
        None
    };

    if let Some(message_id) = message_id.as_deref() {
        let snapshot = build_saved_version_snapshot(
            Some(attempted_design.clone()),
            request.thread_id.clone(),
            message_id.to_string(),
            None,
            None,
            None,
        );
        *state.last_snapshot.lock().unwrap() = Some(snapshot.clone());
        write_last_snapshot(app, Some(&snapshot));
        state.emit_history_changed(
            Some(request.thread_id.clone()),
            Some(message_id.to_string()),
            "parameterVersionAppended",
        );
    }

    let response = ManualParameterApplyResponse {
        thread_id: request.thread_id.clone(),
        base_message_id: request.target_message_id.clone(),
        message_id: message_id.clone(),
        status: MessageStatus::Working,
        design_output: attempted_design.clone(),
        artifact_bundle: None,
        model_manifest: None,
        snapshot_id: None,
        error: None,
    };
    let pending = PendingManualParameterApply {
        request,
        attempted_design,
        previous_bundle: target.artifact_bundle,
        previous_manifest: target.model_manifest,
        message_id,
    };
    Ok((response, pending))
}

pub async fn finish_manual_parameter_apply(
    pending: PendingManualParameterApply,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ManualParameterApplyResponse> {
    let PendingManualParameterApply {
        request,
        attempted_design,
        previous_bundle,
        previous_manifest,
        message_id,
    } = pending;

    let (artifact_bundle, model_manifest) = match render_parameters(
        &attempted_design,
        previous_bundle.as_ref(),
        previous_manifest.as_ref(),
        state,
        app,
    )
    .await
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return persist_failure(&request, message_id, attempted_design, error, state, app)
                .await;
        }
    };
    let snapshot_id = match build_render_snapshot(RenderSnapshotInput {
        design: &attempted_design,
        effective_params: &attempted_design.initial_params,
        artifact_bundle: &artifact_bundle,
        model_manifest: &model_manifest,
    }) {
        Ok(snapshot) => snapshot.snapshot_id,
        Err(error) => {
            return persist_failure(&request, message_id, attempted_design, error, state, app)
                .await;
        }
    };
    if let Some(message_id) = message_id.as_deref() {
        persist_success(
            &request,
            message_id,
            &attempted_design,
            &artifact_bundle,
            &model_manifest,
            state,
            app,
        )
        .await?;
    }
    let design_output = if let Some(message_id) = message_id.as_deref() {
        stored_design(state, message_id).await?
    } else {
        attempted_design
    };
    let snapshot = build_saved_version_snapshot(
        Some(design_output.clone()),
        request.thread_id.clone(),
        message_id
            .clone()
            .unwrap_or_else(|| request.target_message_id.clone()),
        Some(artifact_bundle.clone()),
        Some(model_manifest.clone()),
        None,
    );
    *state.last_snapshot.lock().unwrap() = Some(snapshot.clone());
    write_last_snapshot(app, Some(&snapshot));

    Ok(ManualParameterApplyResponse {
        thread_id: request.thread_id,
        base_message_id: request.target_message_id,
        message_id,
        status: MessageStatus::Success,
        design_output,
        artifact_bundle: Some(artifact_bundle),
        model_manifest: Some(model_manifest),
        snapshot_id: Some(snapshot_id),
        error: None,
    })
}

pub async fn apply_manual_parameters(
    request: ManualParameterApplyRequest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ManualParameterApplyResponse> {
    let (_, pending) = begin_manual_parameter_apply(request, state, app).await?;
    finish_manual_parameter_apply(pending, state, app).await
}
