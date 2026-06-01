use super::{
    claim_owner_for_thread, clear_turn_working_state, configured_prompt_timeout_secs,
    current_turn_working_user_message_ids_for_thread, dialogue_identity, drop_live_session,
    emit_prompt_closed, ensure_thread_claim, has_managed_runtime_session, managed_pending_target,
    mark_live_session_busy, mark_live_session_idle, mark_live_session_waiting, mutate_live_session,
    now_secs, persist_agent_session, push_trace_event, remember_turn_working_user_messages,
    resolve_explicit_session_target, resolve_prompt_thread_context,
    resolve_request_user_prompt_target, session_target_ref, AgentContext, TraceEvent,
};
use crate::db;
use crate::mcp::contracts::*;
use crate::mcp::runtime;
use crate::models::{AppError, AppResult, AppState};
use crate::services::{agent_dialogue, history};
use tauri::Emitter;
use tokio::sync::oneshot;
use uuid::Uuid;

fn summarize_user_facing_text(content: &str) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return "Saved an empty agent reply.".to_string();
    }
    if trimmed.len() <= 120 {
        return trimmed.to_string();
    }
    let end = trimmed.floor_char_boundary(119);
    format!("{}…", &trimmed[..end])
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
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let timeout_secs = configured_prompt_timeout_secs(state, req.timeout_secs);
    let prompt_message = req.message.clone();
    let prompt_content = agent_dialogue::normalize_prompt_request_message(
        prompt_message.as_deref(),
        &ctx.agent_label,
    );
    let prompt_target = resolve_request_user_prompt_target(state, &ctx.session_id, &req).await?;
    let (response_thread_id, response_thread_title) =
        resolve_prompt_thread_context(state, prompt_target.as_ref()).await?;
    if prompt_target.is_none() {
        let details = if has_managed_runtime_session(state, &ctx.session_id) {
            "Use a wake target, call thread_borrow/thread_create, or pass threadId/messageId before request_user_prompt."
        } else {
            "Call thread_list/thread_get plus thread_borrow, or call thread_create, before requesting user input."
        };
        push_trace_event(
            state,
            ctx,
            TraceEvent {
                thread_id: None,
                message_id: None,
                model_id: None,
                phase: "error",
                kind: "session_bind_failed",
                summary: "Agent session tried to prompt the user without a bound thread target."
                    .to_string(),
                details: Some(details.to_string()),
            },
        );
        return Err(AppError::validation(
            "request_user_prompt requires a thread target. Call thread_borrow/thread_create first, or pass threadId/messageId explicitly.",
        ));
    }
    if let Some(target) = prompt_target.as_ref() {
        ensure_thread_claim(state, ctx, &target.thread_id, false).await?;
    }
    push_trace_event(
        state,
        ctx,
        TraceEvent {
            thread_id: prompt_target
                .as_ref()
                .map(|target| target.thread_id.clone()),
            message_id: prompt_target
                .as_ref()
                .and_then(|target| target.message_id.clone()),
            model_id: prompt_target
                .as_ref()
                .and_then(|target| target.model_id.clone()),
            phase: "waiting_for_user",
            kind: "request_user_prompt",
            summary: prompt_content.clone(),
            details: None,
        },
    );

    // Supersede any existing live prompt for this session before registering the new one.
    state
        .close_prompts_for_session(&ctx.session_id, "superseded")
        .await;

    let (tx, rx) = oneshot::channel::<Result<crate::contracts::ResolveAgentPromptInput, String>>();

    {
        let mut channels = state.prompt_channels.lock().await;
        channels.insert(request_id.clone(), tx);
    }

    if let Some(target) = prompt_target.as_ref() {
        let timestamp = now_secs();
        agent_dialogue::add_dialogue_message(
            state,
            &target.thread_id,
            &crate::models::Message {
                id: Uuid::new_v4().to_string(),
                role: crate::models::MessageRole::Assistant,
                content: prompt_content.clone(),
                status: crate::models::MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: Some(agent_dialogue::build_agent_origin(
                    &dialogue_identity(ctx),
                    timestamp,
                )),
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp,
            },
        )
        .await?;
        state.emit_history_updated();
    }

    let prompt_target_ref = prompt_target.as_ref().and_then(|target| {
        target.message_id.clone().map(|message_id| {
            session_target_ref(
                target.thread_id.clone(),
                message_id,
                target.model_id.clone(),
            )
        })
    });
    mark_live_session_waiting(
        state,
        ctx,
        prompt_target_ref.clone(),
        prompt_content.clone(),
    )
    .await;

    handle
        .emit(
            "agent-prompt-request",
            AgentPromptEvent {
                request_id: request_id.clone(),
                message: prompt_message.clone(),
                agent_label: ctx.agent_label.clone(),
                session_id: ctx.session_id.clone(),
                thread_id: response_thread_id.clone(),
                message_id: prompt_target
                    .as_ref()
                    .and_then(|target| target.message_id.clone()),
                model_id: prompt_target
                    .as_ref()
                    .and_then(|target| target.model_id.clone()),
            },
        )
        .map_err(|e| AppError::internal(format!("Failed to emit prompt event: {}", e)))?;

    // For active-mode auto-agents: freeze the process group while waiting.
    // The supervisor registered the pgid; we stash it so resolve can SIGCONT.
    #[cfg(unix)]
    let pgid = {
        let pgid = if has_managed_runtime_session(state, &ctx.session_id) {
            runtime::runtime_snapshot_by_session_id(state, &ctx.session_id)
                .and_then(|snapshot| snapshot.pid)
        } else {
            None
        };
        if let Some(pgid) = pgid {
            unsafe {
                libc::kill(-pgid, libc::SIGSTOP);
            }
            eprintln!("[MCP] SIGSTOP pgid {} (agent: {})", pgid, ctx.agent_label);
        }
        pgid
    };
    #[cfg(not(unix))]
    let pgid = None;
    state.prompt_waits.lock().unwrap().insert(
        request_id.clone(),
        crate::models::PromptResumeState {
            pgid,
            agent_label: ctx.agent_label.clone(),
            session_id: ctx.session_id.clone(),
            thread_id: prompt_target.map(|target| target.thread_id),
        },
    );
    if has_managed_runtime_session(state, &ctx.session_id) {
        runtime::mark_managed_session_waiting(
            state,
            &ctx.session_id,
            ctx.llm_model_label.clone(),
            prompt_message
                .clone()
                .or_else(|| Some("Waiting for your next queued message.".to_string())),
        );
    }

    let prompt_input = match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx)
        .await
    {
        Ok(Ok(Ok(prompt_input))) => prompt_input,
        Ok(Ok(Err(reason))) => {
            // close_single_prompt already released prompt_wait, emitted agent-prompt-closed,
            // and cleared waiting_on_prompt. Just clean up managed session state.
            mark_live_session_idle(
                state,
                ctx,
                prompt_target_ref.clone(),
                "idle",
                Some(format!("Prompt closed ({reason}).")),
            )
            .await;
            if has_managed_runtime_session(state, &ctx.session_id) {
                runtime::mark_managed_session_active(
                    state,
                    &ctx.session_id,
                    response_thread_id.clone(),
                    ctx.llm_model_label.clone(),
                    Some(format!("Prompt closed ({reason}).")),
                );
            }
            return Err(AppError::validation(format!(
                "Prompt closed ({reason}). If you still need input, call request_user_prompt again."
            )));
        }
        Ok(Err(_)) => {
            // Sender dropped without sending — unexpected; treat like closed.
            runtime::release_prompt_wait(state, &request_id);
            emit_prompt_closed(
                handle,
                &request_id,
                &ctx.session_id,
                response_thread_id.clone(),
                "closed",
            );
            mark_live_session_idle(
                state,
                ctx,
                prompt_target_ref.clone(),
                "idle",
                Some("Prompt request closed before the user replied.".to_string()),
            )
            .await;
            if has_managed_runtime_session(state, &ctx.session_id) {
                runtime::mark_managed_session_active(
                    state,
                    &ctx.session_id,
                    response_thread_id.clone(),
                    ctx.llm_model_label.clone(),
                    Some(
                        "No pending prompt request. The previous request_user_prompt closed."
                            .to_string(),
                    ),
                );
            }
            return Err(AppError::validation(
                "Prompt channel closed unexpectedly. If you still need input, call request_user_prompt again."
                    .to_string(),
            ));
        }
        Err(_) => {
            state.prompt_channels.lock().await.remove(&request_id);
            runtime::release_prompt_wait(state, &request_id);
            emit_prompt_closed(
                handle,
                &request_id,
                &ctx.session_id,
                response_thread_id.clone(),
                "timed_out",
            );
            let timeout_text = format!(
                "No pending prompt request. request_user_prompt timed out after {} seconds.",
                timeout_secs
            );
            mark_live_session_idle(
                state,
                ctx,
                prompt_target_ref.clone(),
                "idle",
                Some(timeout_text.clone()),
            )
            .await;
            if has_managed_runtime_session(state, &ctx.session_id) {
                runtime::mark_managed_session_active(
                    state,
                    &ctx.session_id,
                    response_thread_id.clone(),
                    ctx.llm_model_label.clone(),
                    Some(timeout_text.clone()),
                );
            }
            return Err(AppError::validation(format!(
                "User prompt timed out after {} seconds. This is normal when the user does not answer right away. Poll again later or call session_log_out if you are leaving the workspace.",
                timeout_secs
            )));
        }
    };

    Ok(UserPromptResponse {
        request_id,
        prompt_text: prompt_input.prompt_text,
        thread_id: response_thread_id,
        thread_title: response_thread_title,
        attachments: prompt_input.attachments,
    })
}

pub async fn handle_mark_as_read(
    state: &AppState,
    req: MarkAsReadRequest,
    ctx: &AgentContext,
) -> AppResult<MarkAsReadResponse> {
    let ctx = ctx.with_override(&req.identity);
    let conn = state.db.lock().await;
    let thread_id = db::get_visible_message_thread_id(&conn, &req.message_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found(format!("Message {} not found.", req.message_id)))?;
    if req
        .thread_id
        .as_deref()
        .is_some_and(|expected| expected != thread_id)
    {
        return Err(AppError::validation(format!(
            "Message {} does not belong to thread {}.",
            req.message_id,
            req.thread_id.unwrap_or_default()
        )));
    }
    ensure_thread_claim(state, &ctx, &thread_id, false).await?;
    let message = db::get_thread_messages(&conn, &thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .into_iter()
        .find(|message| message.id == req.message_id)
        .ok_or_else(|| AppError::not_found(format!("Message {} not found.", req.message_id)))?;
    if message.role != crate::models::MessageRole::User {
        return Err(AppError::validation(format!(
            "Only user thread messages can be marked as read. {} is {:?}.",
            req.message_id, message.role
        )));
    }
    let claimed_message_ids = {
        let pending_ids = db::get_thread_messages(&conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .into_iter()
            .filter(|candidate| {
                candidate.role == crate::models::MessageRole::User
                    && candidate.status == crate::models::MessageStatus::Pending
            })
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        if pending_ids.is_empty() {
            vec![req.message_id.clone()]
        } else {
            pending_ids
        }
    };
    for message_id in &claimed_message_ids {
        db::update_message_status_and_output(
            &conn,
            message_id,
            db::MessageStatusUpdate {
                status: &crate::models::MessageStatus::Working,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                visual_kind: None,
                content: None,
            },
        )
        .map_err(|err| AppError::persistence(err.to_string()))?;
    }
    let primary_message_id = claimed_message_ids
        .first()
        .cloned()
        .unwrap_or_else(|| req.message_id.clone());
    persist_agent_session(
        &conn,
        &ctx,
        Some(thread_id.clone()),
        Some(primary_message_id.clone()),
        None,
        "working",
        "Agent picked up the queued thread batch.",
    )?;
    drop(conn);
    remember_turn_working_user_messages(state, &ctx.session_id, &thread_id, &claimed_message_ids)
        .await;
    let target = session_target_ref(thread_id.clone(), primary_message_id.clone(), None);
    mark_live_session_busy(
        state,
        &ctx,
        Some(target),
        "working",
        Some("Working through the queued thread batch.".to_string()),
        Some("Working through the queued thread batch.".to_string()),
        true,
    )
    .await;
    if has_managed_runtime_session(state, &ctx.session_id) {
        runtime::mark_managed_session_turn_busy(
            state,
            &ctx.session_id,
            Some(thread_id.clone()),
            ctx.llm_model_label.clone(),
            Some("Working through the queued thread batch.".to_string()),
        );
    }
    state.emit_history_updated();
    push_trace_event(
        state,
        &ctx,
        TraceEvent {
            thread_id: Some(thread_id.clone()),
            message_id: Some(primary_message_id.clone()),
            model_id: None,
            phase: "working",
            kind: "mark_as_read",
            summary: "Agent picked up the queued thread batch.".to_string(),
            details: Some(format!(
                "claimed {} pending user message(s)",
                claimed_message_ids.len()
            )),
        },
    );
    Ok(MarkAsReadResponse {
        thread_id,
        message_id: req.message_id,
        message_ids: claimed_message_ids,
        status: "working".to_string(),
    })
}

pub async fn handle_session_reply_save(
    state: &AppState,
    req: SessionReplySaveRequest,
    ctx: &AgentContext,
) -> AppResult<SessionReplySaveResponse> {
    let ctx = ctx.with_override(&req.identity);
    let body = req.body.trim();
    if body.is_empty() {
        return Err(AppError::validation(
            "session_reply_save requires a non-empty body.",
        ));
    }

    let target = if let Some(thread_id) = req.thread_id.clone() {
        agent_dialogue::SessionThreadTarget {
            thread_id,
            message_id: req.message_id.clone(),
            model_id: None,
        }
    } else {
        agent_dialogue::resolve_session_thread_target(state, &ctx.session_id)
            .await?
            .ok_or_else(|| {
                AppError::validation(
                    "No active session target is available for session_reply_save.",
                )
            })?
    };
    ensure_thread_claim(state, &ctx, &target.thread_id, false).await?;

    let timestamp = now_secs();
    let message_id = Uuid::new_v4().to_string();
    agent_dialogue::add_dialogue_message(
        state,
        &target.thread_id,
        &crate::models::Message {
            id: message_id.clone(),
            role: crate::models::MessageRole::Assistant,
            content: body.to_string(),
            status: if req.fatal {
                crate::models::MessageStatus::Error
            } else {
                crate::models::MessageStatus::Success
            },
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: Some(agent_dialogue::build_agent_origin(
                &dialogue_identity(&ctx),
                timestamp,
            )),
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
            timestamp,
        },
    )
    .await?;

    let working_message_ids =
        current_turn_working_user_message_ids_for_thread(state, &ctx.session_id, &target.thread_id)
            .await;
    if !working_message_ids.is_empty() {
        let conn = state.db.lock().await;
        for working_message_id in &working_message_ids {
            db::update_message_status_and_output(
                &conn,
                working_message_id,
                db::MessageStatusUpdate {
                    status: &crate::models::MessageStatus::Success,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    structural_verification: None,
                    visual_kind: None,
                    content: None,
                },
            )
            .map_err(|err| AppError::persistence(err.to_string()))?;
        }
        drop(conn);
        clear_turn_working_state(state, &ctx.session_id, &target.thread_id).await;
    }

    state.emit_history_updated();

    let conn = state.db.lock().await;
    persist_agent_session(
        &conn,
        &ctx,
        Some(target.thread_id.clone()),
        Some(
            target
                .message_id
                .clone()
                .unwrap_or_else(|| message_id.clone()),
        ),
        target.model_id.clone(),
        if req.fatal { "error" } else { "idle" },
        summarize_user_facing_text(body),
    )?;
    drop(conn);

    push_trace_event(
        state,
        &ctx,
        TraceEvent {
            thread_id: Some(target.thread_id.clone()),
            message_id: target
                .message_id
                .clone()
                .or_else(|| Some(message_id.clone())),
            model_id: target.model_id.clone(),
            phase: if req.fatal { "error" } else { "idle" },
            kind: "final_reply_save",
            summary: summarize_user_facing_text(body),
            details: (!req.fatal)
                .then_some(body.to_string())
                .filter(|text| text.len() > 140),
        },
    );

    if req.fatal {
        mark_live_session_idle(
            state,
            &ctx,
            Some(session_target_ref(
                target.thread_id.clone(),
                target
                    .message_id
                    .clone()
                    .unwrap_or_else(|| message_id.clone()),
                target.model_id.clone(),
            )),
            "error",
            Some(summarize_user_facing_text(body)),
        )
        .await;
        if has_managed_runtime_session(state, &ctx.session_id) {
            runtime::mark_managed_session_error(
                state,
                &ctx.session_id,
                Some(target.thread_id.clone()),
                summarize_user_facing_text(body),
            );
        }
    } else {
        mark_live_session_idle(
            state,
            &ctx,
            Some(session_target_ref(
                target.thread_id.clone(),
                target
                    .message_id
                    .clone()
                    .unwrap_or_else(|| message_id.clone()),
                target.model_id.clone(),
            )),
            "idle",
            Some(summarize_user_facing_text(body)),
        )
        .await;
        if has_managed_runtime_session(state, &ctx.session_id) {
            runtime::mark_managed_session_active(
                state,
                &ctx.session_id,
                Some(target.thread_id.clone()),
                ctx.llm_model_label.clone(),
                Some("Saved final reply.".to_string()),
            );
        }
    }

    Ok(SessionReplySaveResponse {
        thread_id: target.thread_id,
        message_id,
        fatal: req.fatal,
    })
}

pub async fn handle_concept_preview_save(
    state: &AppState,
    req: ConceptPreviewSaveRequest,
    ctx: &AgentContext,
) -> AppResult<ConceptPreviewSaveResponse> {
    let ctx = ctx.with_override(&req.identity);
    let image_data = req.image_data.trim();
    if image_data.is_empty() {
        return Err(AppError::validation(
            "concept_preview_save requires non-empty imageData.",
        ));
    }
    if !image_data.starts_with("data:image/") {
        return Err(AppError::validation(
            "concept_preview_save imageData must be a data:image URL produced by the MCP agent.",
        ));
    }
    let caption = req.caption.trim().to_string();

    let target = if let Some(explicit_target) =
        resolve_explicit_session_target(state, req.thread_id.clone(), req.message_id.clone(), None)
            .await?
    {
        explicit_target
    } else {
        agent_dialogue::resolve_session_thread_target(state, &ctx.session_id)
            .await?
            .ok_or_else(|| {
                AppError::validation(
                    "No active session target is available for concept_preview_save.",
                )
            })?
    };

    ensure_thread_claim(state, &ctx, &target.thread_id, false).await?;

    let timestamp = now_secs();
    let message_id = Uuid::new_v4().to_string();
    agent_dialogue::add_dialogue_message(
        state,
        &target.thread_id,
        &crate::models::Message {
            id: message_id.clone(),
            role: crate::models::MessageRole::Assistant,
            content: caption.clone(),
            status: crate::models::MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: Some(agent_dialogue::build_agent_origin(
                &dialogue_identity(&ctx),
                timestamp,
            )),
            image_data: Some(image_data.to_string()),
            visual_kind: Some(crate::models::MessageVisualKind::ConceptPreview),
            attachment_images: Vec::new(),
            timestamp,
        },
    )
    .await?;
    state.emit_history_updated();

    Ok(ConceptPreviewSaveResponse {
        thread_id: target.thread_id,
        message_id,
        image_data: image_data.to_string(),
        caption,
    })
}

pub async fn handle_long_action_notice(
    state: &AppState,
    req: LongActionNoticeRequest,
    ctx: &AgentContext,
) -> AppResult<LongActionNoticeResponse> {
    let response = handle_session_activity_set(
        state,
        SessionActivitySetRequest {
            identity: req.identity,
            phase: req.phase.unwrap_or_else(|| "working".to_string()),
            label: Some(req.message),
            detail: req.details,
            attention_kind: None,
        },
        ctx,
    )
    .await?;

    Ok(LongActionNoticeResponse {
        session_id: response.session_id,
        phase: response.phase,
        busy: response.busy,
        activity_label: response.activity_label.unwrap_or_default(),
        activity_started_at: response.activity_started_at,
    })
}

pub async fn handle_session_activity_set(
    state: &AppState,
    req: SessionActivitySetRequest,
    ctx: &AgentContext,
) -> AppResult<SessionActivitySetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let phase = req.phase.trim().to_string();
    if phase.is_empty() {
        return Err(AppError::validation(
            "session_activity_set requires a non-empty phase.",
        ));
    }
    let label = req
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let target = agent_dialogue::resolve_session_thread_target(state, &ctx.session_id).await?;
    let target_ref = target.as_ref().and_then(|target| {
        target.message_id.clone().map(|message_id| {
            session_target_ref(
                target.thread_id.clone(),
                message_id,
                target.model_id.clone(),
            )
        })
    });
    let activity_started_at = now_secs();
    let attention_kind = req
        .attention_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    mark_live_session_busy(
        state,
        &ctx,
        target_ref,
        phase.clone(),
        label.clone(),
        label.clone(),
        true,
    )
    .await;
    if attention_kind.is_some() {
        mutate_live_session(state, &ctx, move |session| {
            session.attention_kind = attention_kind.clone();
        })
        .await;
    }

    let conn = state.db.lock().await;
    persist_agent_session(
        &conn,
        &ctx,
        target.as_ref().map(|target| target.thread_id.clone()),
        target.as_ref().and_then(|target| target.message_id.clone()),
        target.as_ref().and_then(|target| target.model_id.clone()),
        &phase,
        label
            .clone()
            .unwrap_or_else(|| format!("Session activity set to {}.", phase)),
    )?;
    drop(conn);

    push_trace_event(
        state,
        &ctx,
        TraceEvent {
            thread_id: target.as_ref().map(|target| target.thread_id.clone()),
            message_id: target.as_ref().and_then(|target| target.message_id.clone()),
            model_id: target.as_ref().and_then(|target| target.model_id.clone()),
            phase: &phase,
            kind: "session_activity_set",
            summary: label
                .clone()
                .unwrap_or_else(|| format!("Session activity set to {}.", phase)),
            details: req
                .detail
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        },
    );

    Ok(SessionActivitySetResponse {
        session_id: ctx.session_id,
        phase,
        busy: true,
        activity_label: label,
        activity_started_at,
    })
}

pub async fn handle_long_action_clear(
    state: &AppState,
    req: LongActionClearRequest,
    ctx: &AgentContext,
) -> AppResult<LongActionClearResponse> {
    let response = handle_session_activity_clear(
        state,
        SessionActivityClearRequest {
            identity: req.identity,
            phase: req.phase,
            status_text: req.status_text,
        },
        ctx,
    )
    .await?;

    Ok(LongActionClearResponse {
        session_id: response.session_id,
        phase: response.phase,
        busy: response.busy,
        status_text: response.status_text,
    })
}

pub async fn handle_session_activity_clear(
    state: &AppState,
    req: SessionActivityClearRequest,
    ctx: &AgentContext,
) -> AppResult<SessionActivityClearResponse> {
    let ctx = ctx.with_override(&req.identity);
    let phase = req
        .phase
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("idle")
        .to_string();
    let status_text = req
        .status_text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let target = agent_dialogue::resolve_session_thread_target(state, &ctx.session_id).await?;
    let target_ref = target.as_ref().and_then(|target| {
        target.message_id.clone().map(|message_id| {
            session_target_ref(
                target.thread_id.clone(),
                message_id,
                target.model_id.clone(),
            )
        })
    });

    mark_live_session_idle(state, &ctx, target_ref, phase.clone(), status_text.clone()).await;

    let conn = state.db.lock().await;
    persist_agent_session(
        &conn,
        &ctx,
        target.as_ref().map(|target| target.thread_id.clone()),
        target.as_ref().and_then(|target| target.message_id.clone()),
        target.as_ref().and_then(|target| target.model_id.clone()),
        &phase,
        status_text
            .clone()
            .unwrap_or_else(|| "Cleared long action state.".to_string()),
    )?;
    drop(conn);

    push_trace_event(
        state,
        &ctx,
        TraceEvent {
            thread_id: target.as_ref().map(|target| target.thread_id.clone()),
            message_id: target.as_ref().and_then(|target| target.message_id.clone()),
            model_id: target.as_ref().and_then(|target| target.model_id.clone()),
            phase: &phase,
            kind: "session_activity_clear",
            summary: status_text
                .clone()
                .unwrap_or_else(|| "Cleared long action state.".to_string()),
            details: None,
        },
    );

    Ok(SessionActivityClearResponse {
        session_id: ctx.session_id,
        phase,
        busy: false,
        status_text,
    })
}

fn build_thread_list_entry(
    conn: &rusqlite::Connection,
    thread: crate::models::Thread,
) -> AppResult<ThreadListEntry> {
    let latest_pending_message_id = db::get_latest_pending_user_message_id(conn, &thread.id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(ThreadListEntry {
        thread_id: thread.id,
        title: thread.title,
        updated_at: thread.updated_at,
        version_count: thread.version_count,
        pending_count: thread.pending_count,
        queued_count: thread.queued_count,
        error_count: thread.error_count,
        status: thread.status,
        finalized_at: thread.finalized_at,
        pending_confirm: thread.pending_confirm,
        latest_pending_message_id,
    })
}

pub async fn handle_thread_list(state: &AppState) -> AppResult<ThreadListResponse> {
    let conn = state.db.lock().await;
    let threads = history::get_history(&conn)?;
    let entries = threads
        .into_iter()
        .map(|thread| build_thread_list_entry(&conn, thread))
        .collect::<AppResult<Vec<_>>>()?;
    drop(conn);

    Ok(ThreadListResponse { threads: entries })
}

fn normalize_created_thread_title(title: Option<String>) -> String {
    title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("New Thread")
        .to_string()
}

async fn mark_live_session_thread_idle(
    state: &AppState,
    ctx: &AgentContext,
    thread_id: Option<String>,
    status_text: String,
) {
    mutate_live_session(state, ctx, move |session| {
        session.bound_thread_id = thread_id.clone();
        session.last_target = None;
        session.phase = Some("idle".to_string());
        session.status_text = Some(status_text.clone());
        session.busy = false;
        session.activity_label = None;
        session.activity_started_at = None;
        session.attention_kind = None;
        session.waiting_on_prompt = false;
    })
    .await;
}

pub async fn handle_thread_create(
    state: &AppState,
    req: ThreadCreateRequest,
    ctx: &AgentContext,
) -> AppResult<ThreadCreateResponse> {
    let ctx = ctx.with_override(&req.identity);
    let title = normalize_created_thread_title(req.title);
    let thread_id = Uuid::new_v4().to_string();
    let now = now_secs();

    {
        let conn = state.db.lock().await;
        db::create_or_update_thread(&conn, &thread_id, &title, now, None)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        persist_agent_session(
            &conn,
            &ctx,
            Some(thread_id.clone()),
            None,
            None,
            "idle",
            format!("Created new thread '{}'.", title),
        )?;
    }

    mark_live_session_thread_idle(
        state,
        &ctx,
        Some(thread_id.clone()),
        format!("Created new thread '{}'.", title),
    )
    .await;
    state.emit_history_updated();

    Ok(ThreadCreateResponse { thread_id, title })
}

pub async fn handle_thread_borrow(
    state: &AppState,
    req: ThreadBorrowRequest,
    ctx: &AgentContext,
) -> AppResult<ThreadBorrowResponse> {
    let ctx = ctx.with_override(&req.identity);
    let target = resolve_explicit_session_target(
        state,
        req.thread_id.clone(),
        req.message_id.clone(),
        req.model_id.clone(),
    )
    .await?
    .ok_or_else(|| AppError::validation("thread_borrow requires threadId or messageId."))?;

    ensure_thread_claim(state, &ctx, &target.thread_id, req.steal_thread).await?;

    let conn = state.db.lock().await;
    let title = db::get_visible_thread_title(&conn, &target.thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .unwrap_or_else(|| target.thread_id.clone());
    persist_agent_session(
        &conn,
        &ctx,
        Some(target.thread_id.clone()),
        target.message_id.clone(),
        target.model_id.clone(),
        "idle",
        format!("Borrowed thread '{}'.", title),
    )?;
    drop(conn);

    if let Some(message_id) = target.message_id.clone() {
        mark_live_session_idle(
            state,
            &ctx,
            Some(session_target_ref(
                target.thread_id.clone(),
                message_id,
                target.model_id.clone(),
            )),
            "idle",
            Some(format!("Borrowed thread '{}'.", title)),
        )
        .await;
    } else {
        mark_live_session_thread_idle(
            state,
            &ctx,
            Some(target.thread_id.clone()),
            format!("Borrowed thread '{}'.", title),
        )
        .await;
    }

    push_trace_event(
        state,
        &ctx,
        TraceEvent {
            thread_id: Some(target.thread_id.clone()),
            message_id: target.message_id.clone(),
            model_id: target.model_id.clone(),
            phase: "idle",
            kind: "thread_borrowed",
            summary: format!("Borrowed thread '{}'.", title),
            details: None,
        },
    );

    Ok(ThreadBorrowResponse {
        session_id: ctx.session_id,
        thread_id: target.thread_id,
        title,
        message_id: target.message_id,
        model_id: target.model_id,
    })
}

pub async fn handle_thread_meta_get(
    state: &AppState,
    req: ThreadMetaRequest,
) -> AppResult<ThreadMetaResponse> {
    let conn = state.db.lock().await;
    let t = history::get_thread(&conn, &req.thread_id)?;
    let latest_pending_message_id = db::get_latest_pending_user_message_id(&conn, &req.thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    drop(conn);
    let claim_owner = claim_owner_for_thread(state, &req.thread_id).await;
    Ok(ThreadMetaResponse {
        thread_id: t.id,
        title: t.title,
        updated_at: t.updated_at,
        version_count: t.version_count,
        pending_count: t.pending_count,
        queued_count: t.queued_count,
        error_count: t.error_count,
        status: t.status,
        finalized_at: t.finalized_at,
        pending_confirm: t.pending_confirm,
        latest_pending_message_id,
        claim_owner,
    })
}

pub async fn handle_finalize_thread(
    state: &AppState,
    req: FinalizeThreadRequest,
) -> AppResult<FinalizeThreadResponse> {
    let conn = state.db.lock().await;
    history::finalize_thread(&conn, &req.thread_id, req.message_id.as_deref())?;
    let finalized_at = now_secs();
    Ok(FinalizeThreadResponse {
        thread_id: req.thread_id,
        finalized_at,
    })
}

pub async fn handle_delete_thread(
    state: &AppState,
    req: DeleteThreadRequest,
) -> AppResult<DeleteThreadResponse> {
    let conn = state.db.lock().await;
    let changed = crate::db::delete_thread(&conn, &req.thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    if !changed {
        return Err(AppError::not_found("Thread not found."));
    }

    Ok(DeleteThreadResponse {
        thread_id: req.thread_id,
        deleted: true,
    })
}

pub async fn handle_session_log_in(
    state: &AppState,
    req: SessionLoginRequest,
    ctx: &AgentContext,
) -> AppResult<SessionLoginResponse> {
    let ctx = ctx.with_override(&req.identity);
    let runtime_target = managed_pending_target(state, &ctx.session_id);
    let explicit_target = resolve_explicit_session_target(
        state,
        req.thread_id.clone(),
        req.message_id.clone(),
        req.model_id.clone(),
    )
    .await?;
    let runtime_matches_explicit_thread = explicit_target.as_ref().is_none_or(|target| {
        runtime_target
            .as_ref()
            .map(|runtime| runtime.thread_id.as_str())
            == Some(target.thread_id.as_str())
    });
    let resolved_thread_id = explicit_target
        .as_ref()
        .map(|target| target.thread_id.clone())
        .or_else(|| {
            runtime_target
                .as_ref()
                .map(|target| target.thread_id.clone())
        });
    let resolved_message_id = explicit_target
        .as_ref()
        .and_then(|target| target.message_id.clone())
        .or_else(|| {
            runtime_matches_explicit_thread
                .then(|| {
                    runtime_target
                        .as_ref()
                        .and_then(|target| target.message_id.clone())
                })
                .flatten()
        });
    let resolved_model_id = explicit_target
        .as_ref()
        .and_then(|target| target.model_id.clone())
        .or_else(|| {
            runtime_matches_explicit_thread
                .then(|| {
                    runtime_target
                        .as_ref()
                        .and_then(|target| target.model_id.clone())
                })
                .flatten()
        });

    if let Some(thread_id) = resolved_thread_id.as_deref() {
        ensure_thread_claim(state, &ctx, thread_id, req.steal_thread).await?;
    }

    let conn = state.db.lock().await;
    persist_agent_session(
        &conn,
        &ctx,
        resolved_thread_id.clone(),
        resolved_message_id.clone(),
        resolved_model_id.clone(),
        "idle",
        "Agent joined the workspace.",
    )?;
    drop(conn);

    if runtime_matches_explicit_thread {
        if let Some(runtime_target) = runtime_target.as_ref() {
            push_trace_event(
                state,
                &ctx,
                TraceEvent {
                    thread_id: Some(runtime_target.thread_id.clone()),
                    message_id: runtime_target.message_id.clone(),
                    model_id: runtime_target.model_id.clone(),
                    phase: "idle",
                    kind: "pending_target_captured",
                    summary: match runtime_target.message_id.as_deref() {
                        Some(message_id) => format!(
                            "Pending target captured for {} / {}.",
                            runtime_target.thread_id, message_id
                        ),
                        None => format!(
                            "Pending target captured for thread {}.",
                            runtime_target.thread_id
                        ),
                    },
                    details: None,
                },
            );
        }
    }

    if let Some(message_id) = resolved_message_id.clone() {
        let target = session_target_ref(
            resolved_thread_id
                .clone()
                .expect("message target implies thread target"),
            message_id,
            resolved_model_id.clone(),
        );
        mark_live_session_idle(
            state,
            &ctx,
            Some(target),
            "idle",
            Some("Agent joined the workspace.".to_string()),
        )
        .await;
    } else {
        mark_live_session_thread_idle(
            state,
            &ctx,
            resolved_thread_id.clone(),
            "Agent joined the workspace.".to_string(),
        )
        .await;
    }
    if has_managed_runtime_session(state, &ctx.session_id) {
        runtime::mark_managed_session_active(
            state,
            &ctx.session_id,
            resolved_thread_id.clone(),
            ctx.llm_model_label.clone(),
            Some("Connected to Ecky.".to_string()),
        );
    }
    push_trace_event(
        state,
        &ctx,
        TraceEvent {
            thread_id: resolved_thread_id.clone(),
            message_id: resolved_message_id.clone(),
            model_id: resolved_model_id.clone(),
            phase: "idle",
            kind: "session_bound",
            summary: if let Some(thread_id) = resolved_thread_id.as_deref() {
                if let Some(message_id) = resolved_message_id.as_deref() {
                    format!("Bound session to {} / {}.", thread_id, message_id)
                } else {
                    format!("Bound session to thread {}.", thread_id)
                }
            } else {
                "Agent logged in without an active thread target.".to_string()
            },
            details: None,
        },
    );

    Ok(SessionLoginResponse {
        session_id: ctx.session_id.clone(),
        thread_id: resolved_thread_id,
        message_id: resolved_message_id,
        model_id: resolved_model_id,
    })
}

pub async fn handle_session_log_out(
    state: &AppState,
    req: SessionLogoutRequest,
    ctx: &AgentContext,
) -> AppResult<SessionLogoutResponse> {
    let ctx = ctx.with_override(&req.identity);
    let conn = state.db.lock().await;

    persist_agent_session(
        &conn,
        &ctx,
        None,
        None,
        None,
        "disconnected",
        "Agent left the workspace (graceful log-out).",
    )?;
    drop(conn);
    drop_live_session(state, &ctx.session_id).await;
    if has_managed_runtime_session(state, &ctx.session_id) {
        runtime::mark_agent_disconnected_for_session(
            state,
            &ctx.session_id,
            Some("Agent left the workspace.".to_string()),
        );
    }

    Ok(SessionLogoutResponse { success: true })
}

pub async fn handle_session_resume(
    state: &AppState,
    req: SessionResumeRequest,
    ctx: &AgentContext,
) -> AppResult<SessionResumeResponse> {
    let ctx = ctx.with_override(&req.identity);
    let conn = state.db.lock().await;
    let stored_session = db::get_sessions_by_ids(&conn, std::slice::from_ref(&ctx.session_id))
        .map_err(|e| AppError::persistence(e.to_string()))?
        .into_iter()
        .next()
        .ok_or_else(|| {
            AppError::validation(
                "No previous session found for this session id. Passive MCP resume no longer falls back by agent label.",
            )
        })?;
    if let Some(thread_id) = stored_session.thread_id.as_deref() {
        ensure_thread_claim(state, &ctx, thread_id, false).await?;
    }

    persist_agent_session(
        &conn,
        &ctx,
        stored_session.thread_id.clone(),
        stored_session.message_id.clone(),
        stored_session.model_id.clone(),
        "idle",
        "Agent resumed previous session.",
    )?;
    drop(conn);

    let target = match (
        stored_session.thread_id.clone(),
        stored_session.message_id.clone(),
    ) {
        (Some(thread_id), Some(message_id)) => Some(session_target_ref(
            thread_id,
            message_id,
            stored_session.model_id.clone(),
        )),
        _ => None,
    };
    if target.is_none() {
        mutate_live_session(state, &ctx, |session| {
            session.bound_thread_id = stored_session.thread_id.clone();
        })
        .await;
    }
    mark_live_session_idle(
        state,
        &ctx,
        target,
        "idle",
        Some("Agent resumed the previous session.".to_string()),
    )
    .await;
    if has_managed_runtime_session(state, &ctx.session_id) {
        runtime::mark_managed_session_active(
            state,
            &ctx.session_id,
            stored_session.thread_id.clone(),
            ctx.llm_model_label.clone(),
            Some("Agent resumed the previous session.".to_string()),
        );
    }

    Ok(SessionResumeResponse {
        thread_id: stored_session.thread_id,
        message_id: stored_session.message_id,
        model_id: stored_session.model_id,
        last_interaction_at: stored_session.updated_at,
    })
}
