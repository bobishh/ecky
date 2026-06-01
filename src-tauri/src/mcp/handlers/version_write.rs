use super::{
    artifact_bundle_digest, clear_session_render_preview_durable, now_secs, persist_agent_session,
    push_mcp_profile, resolve_session_render_preview_for_request, resolve_turn_working_target,
    try_record_agent_error, AgentContext,
};
use crate::db;
use crate::mcp::contracts::{
    ThreadForkRequest, ThreadForkResponse, VersionRestoreRequest, VersionRestoreResponse,
    VersionSaveRequest, VersionSaveResponse,
};
use crate::models::{AppError, AppResult, AppState, PathResolver};
use crate::services::agent_versions::{
    save_or_update_agent_version_for_session, SaveOrUpdateAgentVersionRequest,
};
use crate::services::history;
use std::time::Instant;
use uuid::Uuid;

pub async fn handle_commit_preview_version(
    state: &AppState,
    app: &dyn PathResolver,
    req: VersionSaveRequest,
    ctx: &AgentContext,
) -> AppResult<VersionSaveResponse> {
    let total_started = Instant::now();
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let resolve_started = Instant::now();
        let preview = resolve_session_render_preview_for_request(
            state,
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        )
        .await?
        .ok_or_else(|| {
            AppError::validation(
                "No preview draft is available for this MCP session. Render a preview before commit_preview_version.",
            )
        })?;
        push_mcp_profile(
            state,
            ctx,
            "commit_preview_version",
            "resolve_preview",
            resolve_started,
            Some(&preview.thread_id),
            Some(&preview.preview_id),
            Some(&preview.artifact_bundle.model_id),
        );

        tracked_thread_id = Some(preview.thread_id.clone());
        tracked_message_id = Some(preview.preview_id.clone());
        tracked_model_id = Some(preview.artifact_bundle.model_id.clone());

        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                ctx,
                tracked_thread_id.clone(),
                tracked_message_id.clone(),
                tracked_model_id.clone(),
                "saving_version",
                "Committing preview draft.",
            )?;
        }

        let mut design_output = preview.design_output.clone();
        if let Some(title) = req.title.clone() {
            design_output.title = title;
        }
        if let Some(version_name) = req.version_name.clone() {
            design_output.version_name = version_name;
        } else if design_output.version_name.trim().is_empty() {
            design_output.version_name.clear();
        }

        let save_started = Instant::now();
        let save_result = save_or_update_agent_version_for_session(
            state,
            app,
            SaveOrUpdateAgentVersionRequest {
                session_id: ctx.session_id.clone(),
                thread_id: preview.thread_id.clone(),
                base_message_id: preview.base_message_id.clone().unwrap_or_default(),
                model_id: Some(preview.artifact_bundle.model_id.clone()),
                design_output,
                artifact_bundle: Some(preview.artifact_bundle.clone()),
                model_manifest: Some(preview.model_manifest.clone()),
                updated_at: now_secs(),
                response_text_created: format!("{} committed the MCP preview.", ctx.agent_label),
                response_text_updated: format!(
                    "{} updated the MCP preview commit.",
                    ctx.agent_label
                ),
                preserve_existing_title: req.title.is_none(),
                preserve_existing_version_name: req.version_name.is_none(),
                force_create_new_message: true,
                announce_created_working_version: false,
            },
        )
        .await?;
        push_mcp_profile(
            state,
            ctx,
            "commit_preview_version",
            "save_or_update_version",
            save_started,
            Some(&preview.thread_id),
            Some(&save_result.message_id),
            save_result.model_id.as_deref(),
        );

        let clear_started = Instant::now();
        clear_session_render_preview_durable(state, &ctx.session_id).await?;
        push_mcp_profile(
            state,
            ctx,
            "commit_preview_version",
            "clear_preview_draft",
            clear_started,
            Some(&preview.thread_id),
            Some(&save_result.message_id),
            save_result.model_id.as_deref(),
        );
        tracked_message_id = Some(save_result.message_id.clone());
        tracked_model_id = save_result.model_id.clone();

        Ok(VersionSaveResponse {
            thread_id: preview.thread_id,
            message_id: save_result.message_id,
            model_id: save_result.model_id.unwrap_or_default(),
        })
    }
    .await;

    push_mcp_profile(
        state,
        ctx,
        "commit_preview_version",
        if result.is_ok() {
            "total_ok"
        } else {
            "total_err"
        },
        total_started,
        tracked_thread_id.as_deref(),
        tracked_message_id.as_deref(),
        tracked_model_id.as_deref(),
    );

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
        state.emit_history_updated();

        let tid = db::get_message_thread_id(&conn, &req.message_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| AppError::not_found("Restored message not found."))?;
        tracked_thread_id = Some(tid.clone());
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
