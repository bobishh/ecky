use crate::db;
use crate::mcp::contracts::*;
use crate::models::{
    AgentDraft, AgentOrigin, AgentSession, AppError, AppResult, AppState, ArtifactBundle,
    ControlPrimitive, ControlView, ControlViewSource, DesignOutput, InteractionMode, MacroDialect,
    ModelManifest, ModelSourceKind, PathResolver, UiSpec,
};
use crate::services::{history, render};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AgentContext {
    pub session_id: String,
    pub client_kind: String,
    pub host_label: String,
    pub agent_label: String,
    pub llm_model_id: Option<String>,
    pub llm_model_label: Option<String>,
}

impl AgentContext {
    pub fn as_identity_response(&self) -> AgentIdentityResponse {
        AgentIdentityResponse {
            session_id: self.session_id.clone(),
            client_kind: self.client_kind.clone(),
            host_label: self.host_label.clone(),
            agent_label: self.agent_label.clone(),
            llm_model_id: self.llm_model_id.clone(),
            llm_model_label: self.llm_model_label.clone(),
        }
    }

    fn origin(&self) -> AgentOrigin {
        AgentOrigin {
            host_label: self.host_label.clone(),
            client_kind: self.client_kind.clone(),
            agent_label: self.agent_label.clone(),
            llm_model_id: self.llm_model_id.clone(),
            llm_model_label: self.llm_model_label.clone(),
            session_id: self.session_id.clone(),
            created_at: now_secs(),
        }
    }

    pub fn with_override(&self, override_identity: &AgentIdentityOverride) -> Self {
        let agent_label = override_identity
            .agent_label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.agent_label.clone());
        let llm_model_id = override_identity
            .llm_model_id
            .as_ref()
            .and_then(|value| {
                let trimmed = value.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            })
            .or_else(|| self.llm_model_id.clone());
        let llm_model_label = override_identity
            .llm_model_label
            .as_ref()
            .and_then(|value| {
                let trimmed = value.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            })
            .or_else(|| self.llm_model_label.clone());

        Self {
            session_id: self.session_id.clone(),
            client_kind: self.client_kind.clone(),
            host_label: self.host_label.clone(),
            agent_label,
            llm_model_id,
            llm_model_label,
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn persist_agent_session(
    conn: &rusqlite::Connection,
    ctx: &AgentContext,
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
    phase: &str,
    status_text: impl Into<String>,
) -> AppResult<()> {
    db::upsert_agent_session(
        conn,
        &AgentSession {
            session_id: ctx.session_id.clone(),
            client_kind: ctx.client_kind.clone(),
            host_label: ctx.host_label.clone(),
            agent_label: ctx.agent_label.clone(),
            llm_model_id: ctx.llm_model_id.clone(),
            llm_model_label: ctx.llm_model_label.clone(),
            thread_id,
            message_id,
            model_id,
            phase: phase.to_string(),
            status_text: status_text.into(),
            updated_at: now_secs(),
        },
    )
    .map_err(|e| AppError::persistence(e.to_string()))
}

fn try_record_agent_error(
    conn: &rusqlite::Connection,
    ctx: &AgentContext,
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
    err: &AppError,
) {
    let _ = persist_agent_session(
        conn,
        ctx,
        thread_id,
        message_id,
        model_id,
        "error",
        err.to_string(),
    );
}

pub async fn handle_user_confirm_request(
    state: &AppState,
    handle: &tauri::AppHandle,
    req: UserConfirmRequest,
    ctx: &AgentContext,
) -> AppResult<UserConfirmResponse> {
    let request_id = req
        .request_id
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let buttons = req
        .buttons
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| vec!["Yes".to_string(), "No".to_string()]);
    let timeout_secs = req.timeout_secs.unwrap_or(120).clamp(5, 600);

    let (tx, rx) = oneshot::channel::<String>();

    {
        let mut channels = state.confirm_channels.lock().await;
        channels.insert(request_id.clone(), tx);
    }

    handle
        .emit(
            "agent-confirm-request",
            AgentConfirmEvent {
                request_id: request_id.clone(),
                message: req.message,
                buttons,
                agent_label: ctx.agent_label.clone(),
            },
        )
        .map_err(|e| AppError::internal(format!("Failed to emit confirmation event: {}", e)))?;

    let choice = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx)
        .await
        .map_err(|_| {
            // Clean up stale channel on timeout
            let state_clone = state.confirm_channels.clone();
            let id_clone = request_id.clone();
            tokio::spawn(async move {
                state_clone.lock().await.remove(&id_clone);
            });
            AppError::internal(format!(
                "User confirmation timed out after {} seconds.",
                timeout_secs
            ))
        })?
        .map_err(|_| AppError::internal("Confirmation channel closed unexpectedly.".to_string()))?;

    Ok(UserConfirmResponse { request_id, choice })
}

pub async fn handle_request_user_prompt(
    state: &AppState,
    handle: &tauri::AppHandle,
    req: UserPromptRequest,
    ctx: &AgentContext,
) -> AppResult<UserPromptResponse> {
    let request_id = req
        .request_id
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let timeout_secs = req.timeout_secs.unwrap_or(300).clamp(10, 1800);

    let (tx, rx) = oneshot::channel::<String>();

    {
        let mut channels = state.prompt_channels.lock().await;
        channels.insert(request_id.clone(), tx);
    }

    handle
        .emit(
            "agent-prompt-request",
            AgentPromptEvent {
                request_id: request_id.clone(),
                message: req.message,
                agent_label: ctx.agent_label.clone(),
                session_id: ctx.session_id.clone(),
            },
        )
        .map_err(|e| AppError::internal(format!("Failed to emit prompt event: {}", e)))?;

    let prompt_text = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx)
        .await
        .map_err(|_| {
            let state_clone = state.prompt_channels.clone();
            let id_clone = request_id.clone();
            tokio::spawn(async move {
                state_clone.lock().await.remove(&id_clone);
            });
            AppError::internal(format!(
                "User prompt timed out after {} seconds.",
                timeout_secs
            ))
        })?
        .map_err(|_| AppError::internal("Prompt channel closed unexpectedly.".to_string()))?;

    Ok(UserPromptResponse {
        request_id,
        prompt_text,
    })
}

pub async fn handle_health_check(
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<HealthCheckResponse> {
    let db_ready = state
        .db
        .lock()
        .await
        .query_row("SELECT 1", [], |_row| Ok(()))
        .is_ok();
    let freecad_configured = render::is_freecad_available(state);
    let config_dir = app.app_config_dir();
    let db_path = config_dir
        .join("history.sqlite")
        .to_string_lossy()
        .to_string();

    Ok(HealthCheckResponse {
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        db_path,
        freecad_configured,
        db_ready,
    })
}

pub async fn handle_thread_list(state: &AppState) -> AppResult<ThreadListResponse> {
    let conn = state.db.lock().await;
    let threads = history::get_history(&conn)?;
    let entries = threads
        .into_iter()
        .map(|t| ThreadListEntry {
            thread_id: t.id,
            title: t.title,
            updated_at: t.updated_at,
            version_count: t.version_count,
            pending_count: t.pending_count,
            error_count: t.error_count,
            status: t.status,
            finalized_at: t.finalized_at,
        })
        .collect();

    Ok(ThreadListResponse { threads: entries })
}

pub async fn handle_finalize_thread(
    state: &AppState,
    req: FinalizeThreadRequest,
) -> AppResult<FinalizeThreadResponse> {
    let conn = state.db.lock().await;
    history::finalize_thread(&conn, &req.thread_id)?;
    let finalized_at = now_secs();
    Ok(FinalizeThreadResponse {
        thread_id: req.thread_id,
        finalized_at,
    })
}

pub async fn handle_session_log_in(
    state: &AppState,
    req: SessionLoginRequest,
    ctx: &AgentContext,
) -> AppResult<SessionLoginResponse> {
    let ctx = ctx.with_override(&req.identity);
    let conn = state.db.lock().await;

    persist_agent_session(
        &conn,
        &ctx,
        req.thread_id.clone(),
        req.message_id.clone(),
        None,
        "idle",
        "Agent joined the workspace.",
    )?;

    Ok(SessionLoginResponse {
        session_id: ctx.session_id.clone(),
        thread_id: req.thread_id,
        message_id: req.message_id,
    })
}

pub async fn handle_session_log_out(
    state: &AppState,
    req: SessionLogoutRequest,
    ctx: &AgentContext,
) -> AppResult<SessionLogoutResponse> {
    let ctx = ctx.with_override(&req.identity);
    let conn = state.db.lock().await;

    // Transition to 'idle' with a status message about leaving.
    // Stale timeout will eventually clear the session UI,
    // but the DB record remains for resume.
    persist_agent_session(
        &conn,
        &ctx,
        None,
        None,
        None,
        "idle",
        "Agent left the workspace (graceful log-out).",
    )?;

    Ok(SessionLogoutResponse { success: true })
}

pub async fn handle_session_resume(
    state: &AppState,
    req: SessionResumeRequest,
    ctx: &AgentContext,
) -> AppResult<SessionResumeResponse> {
    let ctx = ctx.with_override(&req.identity);
    let conn = state.db.lock().await;

    let last_session = db::get_thread_last_agent_session_for_agent(&conn, &ctx.agent_label)
        .map_err(|e| AppError::persistence(e.to_string()))?;

    match last_session {
        Some(s) => {
            // Re-establish as active
            persist_agent_session(
                &conn,
                &ctx,
                s.thread_id.clone(),
                s.message_id.clone(),
                s.model_id.clone(),
                "idle",
                "Agent resumed previous session.",
            )?;

            Ok(SessionResumeResponse {
                thread_id: s.thread_id,
                message_id: s.message_id,
                model_id: s.model_id,
                last_interaction_at: s.updated_at,
            })
        }
        None => Err(AppError::validation(
            "No previous session found for this agent.",
        )),
    }
}

pub async fn handle_thread_get(
    state: &AppState,
    req: ThreadGetRequest,
) -> AppResult<ThreadGetResponse> {
    let conn = state.db.lock().await;
    let thread = history::get_thread(&conn, &req.thread_id)?;
    Ok(ThreadGetResponse { thread })
}

pub fn handle_agent_identity_set(
    ctx: &AgentContext,
    req: AgentIdentitySetRequest,
) -> AgentIdentityResponse {
    ctx.with_override(&AgentIdentityOverride {
        agent_label: req.agent_label,
        llm_model_id: req.llm_model_id,
        llm_model_label: req.llm_model_label,
    })
    .as_identity_response()
}

pub async fn handle_target_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: TargetGetRequest,
    ctx: &AgentContext,
) -> AppResult<TargetGetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<TargetGetResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "",
        )?;

        let target = crate::services::target::resolve_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = target
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "reading",
            "",
        )?;

        let design = target
            .design
            .ok_or_else(|| AppError::validation("Target has no design output."))?;

        Ok(TargetGetResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            title: design.title,
            version_name: design.version_name,
            macro_code: design.macro_code,
            ui_spec: design.ui_spec,
            initial_params: design.initial_params,
            artifact_bundle: target.artifact_bundle,
            model_manifest: target.model_manifest,
            latest_draft: target.latest_draft,
        })
    })();

    if let Err(err) = &result {
        try_record_agent_error(
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

pub async fn handle_params_patch_and_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: ParamsPatchRequest,
    ctx: &AgentContext,
) -> AppResult<ParamsPatchResponse> {
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
        tracked_model_id = target
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "patching_params",
            "",
        )?;

        let (base_design, _base_bundle, _base_manifest) = if let Some(draft) = &target.latest_draft
        {
            (
                draft.design_output.clone(),
                draft.artifact_bundle.clone(),
                draft.model_manifest.clone(),
            )
        } else {
            (
                target
                    .design
                    .ok_or_else(|| AppError::validation("Target has no design output."))?,
                target.artifact_bundle.clone(),
                target.model_manifest.clone(),
            )
        };

        let mut merged_params = base_design.initial_params.clone();
        for (key, value) in req.parameter_patch.clone() {
            merged_params.insert(key, value);
        }

        crate::models::validate_design_params(&merged_params, &base_design.ui_spec)?;

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "rendering",
            "",
        )?;

        drop(conn);

        let next_post_processing = req
            .post_processing
            .clone()
            .or_else(|| base_design.post_processing.clone());

        let artifact_bundle = render::render_model(
            &base_design.macro_code,
            &merged_params,
            next_post_processing.as_ref(),
            state,
            app,
        )
        .await?;
        let model_manifest = crate::freecad::get_model_manifest(app, &artifact_bundle.model_id)?;
        tracked_model_id = Some(artifact_bundle.model_id.clone());

        let mut design_output = base_design.clone();
        design_output.initial_params = merged_params.clone();
        design_output.post_processing = next_post_processing;

        let conn = state.db.lock().await;
        let draft = AgentDraft {
            session_id: ctx.session_id.clone(),
            thread_id: target.thread_id.clone(),
            base_message_id: target.message_id.clone(),
            model_id: Some(artifact_bundle.model_id.clone()),
            design_output: design_output.clone(),
            artifact_bundle: Some(artifact_bundle.clone()),
            model_manifest: Some(model_manifest.clone()),
            updated_at: now_secs(),
        };
        db::upsert_agent_draft(&conn, &draft).map_err(|e| AppError::persistence(e.to_string()))?;

        persist_agent_session(
            &conn,
            ctx,
            Some(target.thread_id.clone()),
            Some(target.message_id.clone()),
            Some(artifact_bundle.model_id.clone()),
            "idle",
            "",
        )?;

        Ok(ParamsPatchResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            merged_params,
            artifact_bundle,
            model_manifest,
            design_output,
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
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

pub async fn handle_macro_replace_and_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroReplaceRequest,
    ctx: &AgentContext,
) -> AppResult<MacroReplaceResponse> {
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
        tracked_model_id = target
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "patching_macro",
            "",
        )?;

        let (base_design, _base_bundle, _base_manifest) = if let Some(draft) = &target.latest_draft
        {
            (
                draft.design_output.clone(),
                draft.artifact_bundle.clone(),
                draft.model_manifest.clone(),
            )
        } else {
            (
                target
                    .design
                    .ok_or_else(|| AppError::validation("Target has no design output."))?,
                target.artifact_bundle.clone(),
                target.model_manifest.clone(),
            )
        };

        let framework_parsed = crate::commands::design::derive_framework_controls(&req.macro_code)?;
        let (ui_spec, initial_params, macro_dialect) = if let Some(parsed) = framework_parsed {
            let current_params = req
                .parameters
                .clone()
                .unwrap_or_else(|| base_design.initial_params.clone());
            (
                UiSpec {
                    fields: parsed.fields.clone(),
                },
                crate::commands::design::reconcile_framework_params(
                    &parsed.fields,
                    &current_params,
                    &parsed.params,
                ),
                MacroDialect::CadFrameworkV1,
            )
        } else {
            let parsed_legacy = crate::commands::design::parse_macro_params(req.macro_code.clone());
            let mut reconciled_params = parsed_legacy.params.clone();
            if let Some(provided) = req.parameters.clone() {
                for (key, value) in provided {
                    if reconciled_params.contains_key(&key) {
                        reconciled_params.insert(key, value);
                    }
                }
            } else {
                for (key, value) in base_design.initial_params {
                    if reconciled_params.contains_key(&key) {
                        reconciled_params.insert(key, value);
                    }
                }
            }
            (
                req.ui_spec.clone().unwrap_or_else(|| {
                    if parsed_legacy.fields.is_empty() {
                        base_design.ui_spec.clone()
                    } else {
                        UiSpec {
                            fields: parsed_legacy.fields,
                        }
                    }
                }),
                reconciled_params,
                MacroDialect::Legacy,
            )
        };
        crate::models::validate_design_params(&initial_params, &ui_spec)?;

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "rendering",
            "",
        )?;

        drop(conn);

        let next_post_processing = req
            .post_processing
            .clone()
            .or_else(|| base_design.post_processing.clone());

        let artifact_bundle = render::render_model(
            &req.macro_code,
            &initial_params,
            next_post_processing.as_ref(),
            state,
            app,
        )
        .await?;
        let model_manifest = crate::freecad::get_model_manifest(app, &artifact_bundle.model_id)?;
        tracked_model_id = Some(artifact_bundle.model_id.clone());

        let design_output = DesignOutput {
            title: base_design.title.clone(),
            version_name: base_design.version_name.clone(),
            response: "Draft update via macro replacement.".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: req.macro_code.clone(),
            macro_dialect,
            ui_spec: ui_spec.clone(),
            initial_params: initial_params.clone(),
            post_processing: next_post_processing,
        };

        let conn = state.db.lock().await;
        let draft = AgentDraft {
            session_id: ctx.session_id.clone(),
            thread_id: target.thread_id.clone(),
            base_message_id: target.message_id.clone(),
            model_id: Some(artifact_bundle.model_id.clone()),
            design_output: design_output.clone(),
            artifact_bundle: Some(artifact_bundle.clone()),
            model_manifest: Some(model_manifest.clone()),
            updated_at: now_secs(),
        };
        db::upsert_agent_draft(&conn, &draft).map_err(|e| AppError::persistence(e.to_string()))?;

        persist_agent_session(
            &conn,
            ctx,
            Some(target.thread_id.clone()),
            Some(target.message_id.clone()),
            Some(artifact_bundle.model_id.clone()),
            "idle",
            "",
        )?;

        Ok(MacroReplaceResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            macro_code: req.macro_code.clone(),
            ui_spec,
            initial_params,
            artifact_bundle,
            model_manifest,
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
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

pub async fn handle_version_save(
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

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());

        let draft = target
            .latest_draft
            .ok_or_else(|| AppError::validation("No successful draft available to save."))?;
        tracked_model_id = draft.model_id.clone();

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

        let title = req
            .title
            .clone()
            .unwrap_or(draft.design_output.title.clone());
        let version_name = req.version_name.clone().unwrap_or_else(|| {
            let now = chrono::Local::now();
            format!("V-mcp-{}", now.format("%Y%m%d-%H%M%S"))
        });

        let new_message_id = crate::services::design::add_manual_version(
            target.thread_id.clone(),
            title,
            version_name,
            draft.design_output.macro_code.clone(),
            draft.design_output.initial_params.clone(),
            draft.design_output.ui_spec.clone(),
            draft.artifact_bundle.clone(),
            draft.model_manifest.clone(),
            Some(format!(
                "{} committed a new version via MCP.",
                ctx.agent_label
            )),
            Some(ctx.origin()),
            state,
            app,
        )
        .await?;

        let conn = state.db.lock().await;
        db::delete_agent_draft(&conn, &target.thread_id, &target.message_id)
            .map_err(|e| AppError::persistence(e.to_string()))?;

        persist_agent_session(
            &conn,
            ctx,
            Some(target.thread_id.clone()),
            Some(new_message_id.clone()),
            draft.model_id.clone(),
            "idle",
            "",
        )?;

        Ok(VersionSaveResponse {
            thread_id: target.thread_id,
            message_id: new_message_id,
            model_id: draft.model_id.unwrap_or_default(),
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
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
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(&conn, ctx, tracked_thread_id, tracked_message_id, None, err);
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

        let (design_output, artifact_bundle, model_manifest, model_id) =
            if let Some(draft) = target.latest_draft.clone() {
                (
                    draft.design_output,
                    draft.artifact_bundle,
                    draft.model_manifest,
                    draft.model_id,
                )
            } else {
                let design = target
                    .design
                    .clone()
                    .ok_or_else(|| AppError::validation("Target has no design output."))?;
                let model_id = target
                    .artifact_bundle
                    .as_ref()
                    .map(|bundle| bundle.model_id.clone());
                (
                    design,
                    target.artifact_bundle.clone(),
                    target.model_manifest.clone(),
                    model_id,
                )
            };
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

        let title = req
            .title
            .clone()
            .unwrap_or_else(|| design_output.title.clone());
        let version_name = req.version_name.clone().unwrap_or_else(|| {
            let now = chrono::Local::now();
            format!("V-mcp-{}", now.format("%Y%m%d-%H%M%S"))
        });
        let new_thread_id = Uuid::new_v4().to_string();

        let new_message_id = crate::services::design::add_manual_version(
            new_thread_id.clone(),
            title,
            version_name,
            design_output.macro_code.clone(),
            design_output.initial_params.clone(),
            design_output.ui_spec.clone(),
            artifact_bundle,
            model_manifest,
            Some(format!("{} forked this version via MCP.", ctx.agent_label)),
            Some(ctx.origin()),
            state,
            app,
        )
        .await?;

        let conn = state.db.lock().await;
        db::delete_agent_draft(&conn, &target.thread_id, &target.message_id)
            .map_err(|e| AppError::persistence(e.to_string()))?;

        persist_agent_session(
            &conn,
            ctx,
            Some(new_thread_id.clone()),
            Some(new_message_id.clone()),
            tracked_model_id.clone(),
            "idle",
            "",
        )?;

        Ok(ThreadForkResponse {
            thread_id: new_thread_id,
            message_id: new_message_id,
            model_id: tracked_model_id.clone().unwrap_or_default(),
        })
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
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

#[derive(Debug, Clone)]
struct SemanticTargetState {
    thread_id: String,
    base_message_id: String,
    design_output: DesignOutput,
    artifact_bundle: ArtifactBundle,
    model_manifest: ModelManifest,
    latest_draft: Option<AgentDraft>,
}

fn default_version_name() -> String {
    let now = chrono::Local::now();
    format!("V-mcp-{}", now.format("%Y%m%d-%H%M%S"))
}

fn resolve_semantic_target(
    conn: &rusqlite::Connection,
    app: &dyn PathResolver,
    thread_id: Option<String>,
    message_id: Option<String>,
) -> AppResult<SemanticTargetState> {
    let target = crate::services::target::resolve_target(conn, app, thread_id, message_id)?;
    let latest_draft = target.latest_draft.clone();
    let (design_output, artifact_bundle, model_manifest) = if let Some(draft) = latest_draft.clone()
    {
        (
            draft.design_output,
            draft
                .artifact_bundle
                .ok_or_else(|| AppError::validation("Draft has no artifact bundle."))?,
            draft
                .model_manifest
                .ok_or_else(|| AppError::validation("Draft has no model manifest."))?,
        )
    } else {
        (
            target
                .design
                .ok_or_else(|| AppError::validation("Target has no design output."))?,
            target
                .artifact_bundle
                .ok_or_else(|| AppError::validation("Target has no artifact bundle."))?,
            target
                .model_manifest
                .ok_or_else(|| AppError::validation("Target has no model manifest."))?,
        )
    };

    if model_manifest.source_kind != ModelSourceKind::Generated {
        return Err(AppError::validation(
            "Semantic knob/view MCP tools currently support generated models only.",
        ));
    }

    if artifact_bundle.model_id != model_manifest.model_id {
        return Err(AppError::validation(
            "Model manifest does not match artifact bundle model id.",
        ));
    }

    Ok(SemanticTargetState {
        thread_id: target.thread_id,
        base_message_id: target.message_id,
        design_output,
        artifact_bundle,
        model_manifest,
        latest_draft,
    })
}

async fn save_semantic_manifest_version(
    state: &AppState,
    app: &dyn PathResolver,
    ctx: &AgentContext,
    target: SemanticTargetState,
    next_manifest: ModelManifest,
    title: Option<String>,
    version_name: Option<String>,
    response_text: String,
) -> AppResult<SemanticManifestMutationResponse> {
    crate::models::validate_model_manifest(&next_manifest)?;
    if next_manifest.model_id != target.artifact_bundle.model_id {
        return Err(AppError::validation(
            "Updated model manifest does not match artifact bundle model id.",
        ));
    }

    let title = title.unwrap_or_else(|| target.design_output.title.clone());
    let version_name = version_name.unwrap_or_else(default_version_name);
    let artifact_bundle = target.artifact_bundle.clone();
    let agent_origin = ctx.origin();

    let new_message_id = crate::services::design::add_manual_version(
        target.thread_id.clone(),
        title.clone(),
        version_name.clone(),
        target.design_output.macro_code.clone(),
        target.design_output.initial_params.clone(),
        target.design_output.ui_spec.clone(),
        Some(artifact_bundle.clone()),
        Some(next_manifest.clone()),
        Some(response_text),
        Some(agent_origin.clone()),
        state,
        app,
    )
    .await?;

    let conn = state.db.lock().await;
    db::delete_agent_draft(&conn, &target.thread_id, &target.base_message_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    Ok(SemanticManifestMutationResponse {
        thread_id: target.thread_id,
        message_id: new_message_id,
        model_id: artifact_bundle.model_id.clone(),
        title,
        version_name,
        artifact_bundle,
        model_manifest: next_manifest,
        agent_origin,
    })
}

fn normalize_llm_primitive(
    primitive: ControlPrimitive,
    existing: Option<&ControlPrimitive>,
    manifest: &ModelManifest,
) -> AppResult<ControlPrimitive> {
    let primitive_id = primitive.primitive_id.trim();
    if primitive_id.is_empty() {
        return Err(AppError::validation("Primitive id cannot be empty."));
    }

    let order = if primitive.order == 0 {
        existing.map(|value| value.order).unwrap_or_else(|| {
            manifest
                .control_primitives
                .iter()
                .map(|entry| entry.order)
                .max()
                .unwrap_or(0)
                + 1
        })
    } else {
        primitive.order
    };

    Ok(ControlPrimitive {
        primitive_id: primitive_id.to_string(),
        label: primitive.label.trim().to_string(),
        kind: primitive.kind,
        source: ControlViewSource::Llm,
        part_ids: primitive.part_ids,
        bindings: primitive.bindings,
        editable: primitive.editable,
        order,
    })
}

fn normalize_llm_view(
    view: ControlView,
    existing: Option<&ControlView>,
    manifest: &ModelManifest,
) -> AppResult<ControlView> {
    let view_id = view.view_id.trim();
    if view_id.is_empty() {
        return Err(AppError::validation("View id cannot be empty."));
    }

    let order = if view.order == 0 {
        existing.map(|value| value.order).unwrap_or_else(|| {
            manifest
                .control_views
                .iter()
                .map(|entry| entry.order)
                .max()
                .unwrap_or(0)
                + 1
        })
    } else {
        view.order
    };

    Ok(ControlView {
        view_id: view_id.to_string(),
        label: view.label.trim().to_string(),
        scope: view.scope,
        part_ids: view.part_ids,
        primitive_ids: view.primitive_ids,
        sections: view.sections,
        is_default: view.is_default,
        source: ControlViewSource::Llm,
        status: view.status,
        order,
    })
}

pub async fn handle_semantic_manifest_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: SemanticManifestRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<SemanticManifestResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading semantic manifest.",
        )?;

        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.base_message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "idle",
            "",
        )?;

        Ok(SemanticManifestResponse {
            thread_id: target.thread_id,
            message_id: target.base_message_id,
            title: Some(target.design_output.title),
            version_name: Some(target.design_output.version_name),
            artifact_bundle: target.artifact_bundle,
            model_manifest: target.model_manifest,
            latest_draft: target.latest_draft,
        })
    })();

    if let Err(err) = &result {
        try_record_agent_error(
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

pub async fn handle_control_primitive_save(
    state: &AppState,
    app: &dyn PathResolver,
    req: ControlPrimitiveSaveRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.base_message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Saving semantic knob.",
        )?;

        let existing = target
            .model_manifest
            .control_primitives
            .iter()
            .find(|entry| entry.primitive_id == req.primitive.primitive_id);
        let next_primitive =
            normalize_llm_primitive(req.primitive, existing, &target.model_manifest)?;
        let next_primitive_id = next_primitive.primitive_id.clone();
        let mut next_manifest = target.model_manifest.clone();
        next_manifest.control_primitives = next_manifest
            .control_primitives
            .into_iter()
            .filter(|entry| entry.primitive_id != next_primitive_id)
            .chain(std::iter::once(next_primitive))
            .collect();
        next_manifest.control_primitives.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.label.cmp(&right.label))
        });

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!("{} updated a semantic knob via MCP.", ctx.agent_label),
        )
        .await?;

        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            ctx,
            Some(response.thread_id.clone()),
            Some(response.message_id.clone()),
            Some(response.model_id.clone()),
            "idle",
            "",
        )?;

        Ok(response)
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
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

pub async fn handle_control_primitive_delete(
    state: &AppState,
    app: &dyn PathResolver,
    req: ControlPrimitiveDeleteRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.base_message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Deleting semantic knob.",
        )?;

        let mut next_manifest = target.model_manifest.clone();
        let primitive_id = req.primitive_id;
        next_manifest
            .control_primitives
            .retain(|entry| entry.primitive_id != primitive_id);
        next_manifest.control_relations.retain(|relation| {
            relation.source_primitive_id != primitive_id
                && relation.target_primitive_id != primitive_id
        });
        for view in &mut next_manifest.control_views {
            view.primitive_ids.retain(|entry| entry != &primitive_id);
            for section in &mut view.sections {
                section.primitive_ids.retain(|entry| entry != &primitive_id);
            }
        }
        for advisory in &mut next_manifest.advisories {
            advisory
                .primitive_ids
                .retain(|entry| entry != &primitive_id);
        }

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!("{} deleted a semantic knob via MCP.", ctx.agent_label),
        )
        .await?;

        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            ctx,
            Some(response.thread_id.clone()),
            Some(response.message_id.clone()),
            Some(response.model_id.clone()),
            "idle",
            "",
        )?;

        Ok(response)
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
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

pub async fn handle_control_view_save(
    state: &AppState,
    app: &dyn PathResolver,
    req: ControlViewSaveRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.base_message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Saving semantic view.",
        )?;

        let existing = target
            .model_manifest
            .control_views
            .iter()
            .find(|entry| entry.view_id == req.view.view_id);
        let next_view = normalize_llm_view(req.view, existing, &target.model_manifest)?;
        let next_view_id = next_view.view_id.clone();
        let mut next_manifest = target.model_manifest.clone();
        next_manifest.control_views = next_manifest
            .control_views
            .into_iter()
            .filter(|entry| entry.view_id != next_view_id)
            .chain(std::iter::once(next_view))
            .collect();
        next_manifest.control_views.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.label.cmp(&right.label))
        });

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!("{} updated a semantic view via MCP.", ctx.agent_label),
        )
        .await?;

        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            ctx,
            Some(response.thread_id.clone()),
            Some(response.message_id.clone()),
            Some(response.model_id.clone()),
            "idle",
            "",
        )?;

        Ok(response)
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
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

pub async fn handle_control_view_delete(
    state: &AppState,
    app: &dyn PathResolver,
    req: ControlViewDeleteRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestMutationResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let conn = state.db.lock().await;
        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;
        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.base_message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Deleting semantic view.",
        )?;

        let mut next_manifest = target.model_manifest.clone();
        let view_id = req.view_id;
        next_manifest
            .control_views
            .retain(|entry| entry.view_id != view_id);
        for advisory in &mut next_manifest.advisories {
            advisory.view_ids.retain(|entry| entry != &view_id);
        }

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!("{} deleted a semantic view via MCP.", ctx.agent_label),
        )
        .await?;

        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            ctx,
            Some(response.thread_id.clone()),
            Some(response.message_id.clone()),
            Some(response.model_id.clone()),
            "idle",
            "",
        )?;

        Ok(response)
    }
    .await;

    if let Err(err) = &result {
        let conn = state.db.lock().await;
        try_record_agent_error(
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
