use crate::db;
use crate::mcp::contracts::*;
use crate::mcp::runtime;
use crate::models::{
    AgentDraft, AgentSession, AppError, AppResult, AppState, ArtifactBundle, DesignOutput,
    ModelManifest, PathResolver, SourceLanguage,
};
use crate::services::agent_dialogue;
use crate::services::agent_versions::{
    save_or_update_agent_version_for_session, SaveOrUpdateAgentVersionRequest,
};
use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use uuid::Uuid;

#[cfg(test)]
use crate::models::{ControlPrimitive, ControlView, ControlViewSource, ModelSourceKind};
#[cfg(test)]
use crate::models::{InteractionMode, MacroDialect, UiSpec};
#[cfg(test)]
use crate::services::history;

mod artifact_read;
mod compare;
mod component;
pub(super) mod ecky_ast;
pub(super) mod macro_buffer;
mod printability;
mod project_folder;
mod render_preview;
mod semantic;
mod session;
mod system;
mod target_detail;
mod target_read;
mod thread_read;
mod verify;
mod version_write;

pub use artifact_read::{handle_artifact_feature_graph_get, handle_artifact_manifest_get};
pub use compare::handle_compare_models;
pub use component::{
    handle_component_extract, handle_component_get, handle_component_search,
    ComponentExtractToolRequest, ComponentExtractToolResponse, ComponentGetToolRequest,
    ComponentSearchToolRequest, ComponentSearchToolResponse,
};
#[cfg(test)]
use ecky_ast::{
    bounded_ecky_ast_source_slice, core_node_child_paths, core_node_digest,
    edit_digest_for_ecky_path, find_core_ast_node_in_program, replace_ecky_ast_source,
    ECKY_AST_SOURCE_MAX_BYTES,
};
use ecky_ast::{build_shape_graph_packet, collect_ecky_constraint_authoring_lints};
pub use ecky_ast::{
    handle_ecky_ast_get, handle_ecky_ast_patch_validate, handle_ecky_ast_replace_and_render,
    handle_ecky_constraints_validate, handle_ecky_dependency_get, handle_ecky_selector_resolve,
};
#[cfg(test)]
use macro_buffer::apply_macro_buffer_replacements;
pub use macro_buffer::{
    handle_macro_buffer_apply_patch, handle_macro_buffer_get, handle_macro_buffer_preview_render,
    handle_macro_buffer_replace_and_preview, handle_macro_buffer_replace_range,
};
use macro_buffer::{macro_buffer_digest, macro_buffer_line_window, macro_buffer_lines};
pub use printability::{
    handle_printability_analyze, handle_printability_transform_recipes_get,
    handle_semantic_transform_preview,
};
pub use project_folder::{
    handle_project_folder_apply, handle_project_folder_export, handle_project_folder_status,
    project_folder_watcher_context, ProjectFolderApplyRequest, ProjectFolderExportRequest,
    ProjectFolderStatusRequest, ProjectFolderWatchEvent, ProjectFolderWatcher,
};
#[cfg(test)]
use render_preview::{
    first_version_authoring_context, infer_macro_source_language, resolve_macro_authoring_context,
};
pub use render_preview::{handle_macro_preview_render, handle_params_preview_render};
pub use semantic::{
    handle_control_primitive_delete, handle_control_primitive_save, handle_control_view_delete,
    handle_control_view_save, handle_measurement_annotation_delete,
    handle_measurement_annotation_save, handle_semantic_manifest_detail_get,
    handle_semantic_manifest_get,
};
pub use session::{
    handle_concept_preview_save, handle_delete_thread, handle_finalize_thread,
    handle_long_action_clear, handle_long_action_notice, handle_mark_as_read,
    handle_request_user_prompt, handle_session_activity_clear, handle_session_activity_set,
    handle_session_log_in, handle_session_log_out, handle_session_reply_save,
    handle_session_resume, handle_thread_borrow, handle_thread_create, handle_thread_list,
    handle_thread_meta_get, handle_user_confirm_request,
};
pub use system::{handle_health_check, handle_ui_dispatch};
pub use target_detail::handle_target_detail_get;
pub use target_read::{handle_target_get, handle_target_macro_get, handle_target_meta_get};
pub use thread_read::{handle_agent_identity_set, handle_thread_get, handle_thread_messages_get};
pub use verify::{handle_structural_verification_summary, handle_verify_generated_model};
pub use version_write::{
    handle_commit_preview_version, handle_saved_target_version, handle_thread_fork_from_target,
    handle_version_restore,
};

pub(super) const THREAD_MESSAGE_CONTENT_MAX_CHARS: usize = 240;

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

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn mcp_profile_target(
    thread_id: Option<&str>,
    message_id: Option<&str>,
    model_id: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(thread_id) = thread_id {
        parts.push(format!("thread={}", thread_id));
    }
    if let Some(message_id) = message_id {
        parts.push(format!("message={}", message_id));
    }
    if let Some(model_id) = model_id {
        parts.push(format!("model={}", model_id));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" {}", parts.join(" "))
    }
}

pub(super) fn push_mcp_profile(
    state: &AppState,
    ctx: &AgentContext,
    op: &str,
    stage: &str,
    started: Instant,
    thread_id: Option<&str>,
    message_id: Option<&str>,
    model_id: Option<&str>,
) {
    state.push_log(format!(
        "[MCP_PROFILE] session={} agent={} op={} stage={} ms={}{}",
        ctx.session_id,
        ctx.agent_label,
        op,
        stage,
        started.elapsed().as_millis(),
        mcp_profile_target(thread_id, message_id, model_id),
    ));
}

pub(super) fn configured_prompt_timeout_secs(
    state: &AppState,
    override_timeout_secs: Option<u64>,
) -> u64 {
    let config_timeout_secs = state.config.lock().unwrap().mcp.prompt_timeout_secs;
    override_timeout_secs
        .unwrap_or(config_timeout_secs)
        .clamp(10, 1800)
}

fn live_thread_claim_target(
    session: &crate::models::McpSessionState,
) -> Option<(String, Option<String>, Option<String>)> {
    session
        .last_target
        .as_ref()
        .map(|target| {
            (
                target.thread_id.clone(),
                Some(target.message_id.clone()),
                target.model_id.clone(),
            )
        })
        .or_else(|| {
            session
                .bound_thread_id
                .clone()
                .map(|thread_id| (thread_id, None, None))
        })
}

pub(super) fn push_unique_strings(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.iter().any(|existing| existing == value) {
            target.push(value.clone());
        }
    }
}

pub(super) fn selection_target_match_ids(target: &crate::models::SelectionTarget) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(target_id) = target.target_id.as_deref() {
        ids.push(target_id.to_string());
    }
    if let Some(durable_target_id) = target.durable_target_id.as_deref() {
        ids.push(durable_target_id.to_string());
    }
    if let Some(canonical_target_id) = target.canonical_target_id.as_deref() {
        ids.push(canonical_target_id.to_string());
    }
    ids.extend(target.alias_ids.iter().cloned());
    ids
}

fn is_specific_selection_binding(target: &crate::models::SelectionTarget) -> bool {
    !target.primitive_ids.is_empty()
        || !target.view_ids.is_empty()
        || target.parameter_keys.len() <= 2
}

pub(super) fn carry_forward_semantic_manifest(
    previous: Option<&ModelManifest>,
    next: ModelManifest,
    artifact_bundle: &ArtifactBundle,
) -> ModelManifest {
    let Some(previous) = previous else {
        return next;
    };

    let mut merged = next.clone();
    if !previous.control_primitives.is_empty() {
        merged.control_primitives = previous.control_primitives.clone();
        merged.control_relations = previous.control_relations.clone();
        merged.control_views = previous.control_views.clone();
    }
    if !previous.advisories.is_empty() {
        merged.advisories = previous.advisories.clone();
    }
    if !previous.measurement_annotations.is_empty() {
        merged.measurement_annotations = previous.measurement_annotations.clone();
    }
    let previous_feature_graph = previous.feature_graph.clone();
    let previous_correspondence_graph = previous.correspondence_graph.clone();
    merged.feature_graph = previous_feature_graph;
    merged.correspondence_graph = previous_correspondence_graph;
    if previous.enrichment_state.status != crate::models::EnrichmentStatus::None
        || !previous.enrichment_state.proposals.is_empty()
    {
        merged.enrichment_state = previous.enrichment_state.clone();
    }

    let mut previous_targets: HashMap<String, &crate::models::SelectionTarget> = HashMap::new();
    for target in &previous.selection_targets {
        for id in selection_target_match_ids(target) {
            previous_targets.entry(id).or_insert(target);
        }
    }

    for target in &mut merged.selection_targets {
        let match_ids = selection_target_match_ids(target);
        for id in match_ids {
            if let Some(previous_target) = previous_targets.get(&id) {
                if !is_specific_selection_binding(previous_target) {
                    continue;
                }
                push_unique_strings(&mut target.parameter_keys, &previous_target.parameter_keys);
                push_unique_strings(&mut target.primitive_ids, &previous_target.primitive_ids);
                push_unique_strings(&mut target.view_ids, &previous_target.view_ids);
            }
        }
    }

    if crate::models::validate_model_runtime_bundle(&merged, artifact_bundle).is_ok() {
        return merged;
    }

    merged.feature_graph = None;
    merged.correspondence_graph = None;
    if crate::models::validate_model_runtime_bundle(&merged, artifact_bundle).is_ok() {
        merged.warnings.push(
            "Feature graph was not carried forward because rendered topology no longer validates old feature bindings."
                .to_string(),
        );
        return merged;
    }

    merged.measurement_annotations.clear();
    if crate::models::validate_model_runtime_bundle(&merged, artifact_bundle).is_ok() {
        merged.warnings.push(
            "Measurement annotations were not carried forward because rendered topology no longer validates old measurement bindings."
                .to_string(),
        );
        return merged;
    }

    let mut fallback = next;
    fallback.warnings.push(
        "Semantic manifest was not carried forward because rendered topology no longer validates old semantic bindings."
            .to_string(),
    );
    fallback
}

fn live_claim_session(
    session_id: &str,
    session: &crate::models::McpSessionState,
) -> Option<AgentSession> {
    let (thread_id, message_id, model_id) = live_thread_claim_target(session)?;
    Some(AgentSession {
        session_id: session_id.to_string(),
        client_kind: session.client_kind.clone(),
        host_label: session.host_label.clone(),
        agent_label: session.agent_label.clone(),
        llm_model_id: session.llm_model_id.clone(),
        llm_model_label: session.llm_model_label.clone(),
        thread_id: Some(thread_id),
        message_id,
        model_id,
        phase: session.phase.clone().unwrap_or_else(|| "idle".to_string()),
        status_text: session.status_text.clone().unwrap_or_default(),
        updated_at: session.updated_at,
    })
}

pub async fn claim_owners_by_thread(state: &AppState) -> HashMap<String, AgentSession> {
    let sessions = state.mcp_sessions.lock().await;
    let mut claims = HashMap::<String, AgentSession>::new();
    for (session_id, session) in sessions.iter() {
        let Some(claim) = live_claim_session(session_id, session) else {
            continue;
        };
        let thread_id = claim.thread_id.clone().unwrap_or_default();
        let should_replace = claims
            .get(&thread_id)
            .map(|existing| existing.updated_at <= claim.updated_at)
            .unwrap_or(true);
        if should_replace {
            claims.insert(thread_id, claim);
        }
    }
    claims
}

pub(super) async fn claim_owner_for_thread(
    state: &AppState,
    thread_id: &str,
) -> Option<AgentSession> {
    claim_owners_by_thread(state).await.remove(thread_id)
}

fn claim_owner_summary(owner: &AgentSession) -> String {
    let agent = owner.agent_label.trim();
    let host = owner.host_label.trim();
    let model = owner.llm_model_label.as_deref().unwrap_or("").trim();
    let identity = if !agent.is_empty() && !host.is_empty() && agent != host {
        format!("{} ({})", agent, host)
    } else if !agent.is_empty() {
        agent.to_string()
    } else if !host.is_empty() {
        host.to_string()
    } else {
        owner.session_id.clone()
    };
    if model.is_empty() {
        identity
    } else {
        format!("{} · {}", identity, model)
    }
}

async fn release_thread_claim(
    state: &AppState,
    owner: &AgentSession,
    released_by: &AgentContext,
) -> AppResult<()> {
    let released_session = {
        let mut sessions = state.mcp_sessions.lock().await;
        let Some(session) = sessions.get_mut(&owner.session_id) else {
            return Ok(());
        };
        session.bound_thread_id = None;
        session.last_target = None;
        session.waiting_on_prompt = false;
        session.current_turn_id = None;
        session.current_turn_thread_id = None;
        session.current_turn_working_message_ids.clear();
        session.current_turn_working_version_message_id = None;
        session.phase = Some("idle".to_string());
        session.busy = false;
        session.activity_label = None;
        session.activity_started_at = None;
        session.attention_kind = None;
        session.status_text = Some(format!(
            "Thread claim transferred to {}.",
            claim_owner_summary(&AgentSession {
                session_id: released_by.session_id.clone(),
                client_kind: released_by.client_kind.clone(),
                host_label: released_by.host_label.clone(),
                agent_label: released_by.agent_label.clone(),
                llm_model_id: released_by.llm_model_id.clone(),
                llm_model_label: released_by.llm_model_label.clone(),
                thread_id: None,
                message_id: None,
                model_id: None,
                phase: "idle".to_string(),
                status_text: String::new(),
                updated_at: now_secs(),
            })
        ));
        session.updated_at = now_secs();
        session.clone()
    };

    let conn = state.db.lock().await;
    db::upsert_agent_session(
        &conn,
        &AgentSession {
            session_id: owner.session_id.clone(),
            client_kind: released_session.client_kind,
            host_label: released_session.host_label,
            agent_label: released_session.agent_label,
            llm_model_id: released_session.llm_model_id,
            llm_model_label: released_session.llm_model_label,
            thread_id: None,
            message_id: None,
            model_id: None,
            phase: released_session.phase.unwrap_or_else(|| "idle".to_string()),
            status_text: released_session.status_text.unwrap_or_default(),
            updated_at: released_session.updated_at,
        },
    )
    .map_err(|err| AppError::persistence(err.to_string()))
}

pub(super) async fn ensure_thread_claim(
    state: &AppState,
    ctx: &AgentContext,
    thread_id: &str,
    steal_thread: bool,
) -> AppResult<()> {
    let Some(owner) = claim_owner_for_thread(state, thread_id).await else {
        return Ok(());
    };
    if owner.session_id == ctx.session_id {
        return Ok(());
    }
    if !steal_thread {
        return Err(AppError::conflict(format!(
            "Thread {} is currently claimed by {}. Pass stealThread: true to take over explicitly.",
            thread_id,
            claim_owner_summary(&owner)
        )));
    }
    release_thread_claim(state, &owner, ctx).await
}

pub(super) fn persist_agent_session(
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

pub(super) fn try_record_agent_error(
    state: &AppState,
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
        thread_id.clone(),
        message_id.clone(),
        model_id.clone(),
        "error",
        err.to_string(),
    );
    push_trace_event_with_conn(
        state,
        conn,
        ctx,
        TraceEvent {
            thread_id: thread_id.clone(),
            message_id: message_id.clone(),
            model_id,
            phase: "error",
            kind: "tool_error",
            summary: err.message.clone(),
            details: err.details.clone(),
        },
    );
}

pub(super) fn dialogue_identity(ctx: &AgentContext) -> agent_dialogue::AgentDialogueIdentity {
    agent_dialogue::AgentDialogueIdentity {
        session_id: ctx.session_id.clone(),
        client_kind: ctx.client_kind.clone(),
        host_label: ctx.host_label.clone(),
        agent_label: ctx.agent_label.clone(),
        llm_model_id: ctx.llm_model_id.clone(),
        llm_model_label: ctx.llm_model_label.clone(),
    }
}

pub(super) struct TraceEvent<'a> {
    pub(super) thread_id: Option<String>,
    pub(super) message_id: Option<String>,
    pub(super) model_id: Option<String>,
    pub(super) phase: &'a str,
    pub(super) kind: &'a str,
    pub(super) summary: String,
    pub(super) details: Option<String>,
}

fn log_trace_event(state: &AppState, ctx: &AgentContext, event: TraceEvent<'_>) {
    let target = match (event.thread_id.as_deref(), event.message_id.as_deref()) {
        (Some(thread_id), Some(message_id)) => {
            format!(" thread={} message={}", thread_id, message_id)
        }
        (Some(thread_id), None) => format!(" thread={}", thread_id),
        _ => String::new(),
    };
    let model = event
        .model_id
        .as_deref()
        .map(|model_id| format!(" model={}", model_id))
        .unwrap_or_default();
    let details = event
        .details
        .as_deref()
        .map(|value| format!("\n{}", value))
        .unwrap_or_default();
    state.push_log(format!(
        "[MCP] session={} agent={} phase={} kind={}{}{}: {}{}",
        ctx.session_id,
        ctx.agent_label,
        event.phase,
        event.kind,
        target,
        model,
        event.summary,
        details,
    ));
}

pub(super) fn push_trace_event(state: &AppState, ctx: &AgentContext, event: TraceEvent<'_>) {
    log_trace_event(state, ctx, event);
}

pub(super) fn push_trace_event_with_conn(
    state: &AppState,
    _conn: &rusqlite::Connection,
    ctx: &AgentContext,
    event: TraceEvent<'_>,
) {
    log_trace_event(state, ctx, event);
}

pub(super) fn has_managed_runtime_session(state: &AppState, session_id: &str) -> bool {
    runtime::runtime_snapshot_by_session_id(state, session_id).is_some()
}

pub(super) async fn resolve_prompt_thread_context(
    state: &AppState,
    prompt_target: Option<&agent_dialogue::SessionThreadTarget>,
) -> AppResult<(Option<String>, Option<String>)> {
    let Some(target) = prompt_target else {
        return Ok((None, None));
    };

    let thread_id = target.thread_id.clone();
    let conn = state.db.lock().await;
    let thread_title = db::get_visible_thread_title(&conn, &thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .and_then(|title| {
            let trimmed = title.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
    Ok((Some(thread_id), thread_title))
}

pub(super) async fn resolve_explicit_session_target(
    state: &AppState,
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
) -> AppResult<Option<agent_dialogue::SessionThreadTarget>> {
    match (thread_id, message_id) {
        (None, None) => Ok(None),
        (Some(thread_id), None) => {
            let conn = state.db.lock().await;
            db::get_visible_thread_title(&conn, &thread_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .ok_or_else(|| AppError::not_found(format!("Thread {} not found.", thread_id)))?;
            Ok(Some(agent_dialogue::SessionThreadTarget {
                thread_id,
                message_id: None,
                model_id,
            }))
        }
        (expected_thread_id, Some(message_id)) => {
            let conn = state.db.lock().await;
            let actual_thread_id = db::get_visible_message_thread_id(&conn, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .ok_or_else(|| AppError::not_found(format!("Message {} not found.", message_id)))?;
            drop(conn);
            if expected_thread_id
                .as_deref()
                .is_some_and(|expected| expected != actual_thread_id)
            {
                return Err(AppError::validation(format!(
                    "Message {} does not belong to thread {}.",
                    message_id,
                    expected_thread_id.unwrap_or_default()
                )));
            }
            Ok(Some(agent_dialogue::SessionThreadTarget {
                thread_id: actual_thread_id,
                message_id: Some(message_id),
                model_id,
            }))
        }
    }
}

pub(super) async fn resolve_request_user_prompt_target(
    state: &AppState,
    session_id: &str,
    req: &UserPromptRequest,
) -> AppResult<Option<agent_dialogue::SessionThreadTarget>> {
    let explicit_target = resolve_explicit_session_target(
        state,
        req.thread_id.clone(),
        req.message_id.clone(),
        req.model_id.clone(),
    )
    .await?;
    let bound_target = agent_dialogue::resolve_session_thread_target(state, session_id).await?;

    if let Some(explicit_target) = explicit_target {
        return Ok(Some(explicit_target));
    }

    Ok(bound_target)
}

pub(super) fn emit_prompt_closed(
    handle: &tauri::AppHandle,
    request_id: &str,
    session_id: &str,
    thread_id: Option<String>,
    reason: &str,
) {
    let _ = handle.emit(
        "agent-prompt-closed",
        AgentPromptClosedEvent {
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            thread_id,
            reason: reason.to_string(),
        },
    );
}

#[derive(Debug, Clone)]
pub(super) struct ManagedPendingTarget {
    pub(super) thread_id: String,
    pub(super) message_id: Option<String>,
    pub(super) model_id: Option<String>,
}

pub(super) fn managed_pending_target(
    state: &AppState,
    session_id: &str,
) -> Option<ManagedPendingTarget> {
    runtime::runtime_snapshot_by_session_id(state, session_id).and_then(|snapshot| {
        snapshot
            .pending_thread_id
            .map(|thread_id| ManagedPendingTarget {
                thread_id,
                message_id: snapshot.pending_message_id,
                model_id: snapshot.pending_model_id,
            })
    })
}

pub(super) async fn mutate_live_session<F>(state: &AppState, ctx: &AgentContext, mutate: F)
where
    F: FnOnce(&mut crate::models::McpSessionState),
{
    let mut sessions = state.mcp_sessions.lock().await;
    if let Some(session) = sessions.get_mut(&ctx.session_id) {
        session.agent_label = ctx.agent_label.clone();
        session.llm_model_id = ctx.llm_model_id.clone();
        session.llm_model_label = ctx.llm_model_label.clone();
        mutate(session);
        session.updated_at = now_secs();
    }
}

pub(super) async fn drop_live_session(state: &AppState, session_id: &str) {
    state
        .close_prompts_for_session(session_id, "session_disconnected")
        .await;
    state.mcp_sessions.lock().await.remove(session_id);
}

pub(super) fn session_target_ref(
    thread_id: impl Into<String>,
    message_id: impl Into<String>,
    model_id: Option<String>,
) -> crate::models::McpTargetRef {
    crate::models::McpTargetRef {
        thread_id: thread_id.into(),
        message_id: message_id.into(),
        model_id,
    }
}

pub(super) async fn mark_live_session_waiting(
    state: &AppState,
    ctx: &AgentContext,
    target: Option<crate::models::McpTargetRef>,
    status_text: String,
) {
    mutate_live_session(state, ctx, move |session| {
        if let Some(target) = target.clone() {
            session.bound_thread_id = Some(target.thread_id.clone());
            session.last_target = Some(target);
        }
        session.phase = Some("waiting_for_user".to_string());
        session.status_text = Some(status_text.clone());
        session.busy = false;
        session.activity_label = None;
        session.activity_started_at = None;
        session.attention_kind = None;
        session.waiting_on_prompt = true;
    })
    .await;
}

pub(super) async fn mark_live_session_busy(
    state: &AppState,
    ctx: &AgentContext,
    target: Option<crate::models::McpTargetRef>,
    phase: impl Into<String>,
    status_text: Option<String>,
    activity_label: Option<String>,
    reset_activity_started_at: bool,
) {
    let phase = phase.into();
    mutate_live_session(state, ctx, move |session| {
        if let Some(target) = target.clone() {
            session.bound_thread_id = Some(target.thread_id.clone());
            session.last_target = Some(target);
        }
        session.phase = Some(phase.clone());
        session.status_text = status_text.clone();
        session.busy = true;
        session.waiting_on_prompt = false;
        if reset_activity_started_at || session.activity_started_at.is_none() {
            session.activity_started_at = Some(now_secs());
        }
        session.activity_label = activity_label.clone();
        session.attention_kind = None;
    })
    .await;
}

pub(super) async fn mark_live_session_idle(
    state: &AppState,
    ctx: &AgentContext,
    target: Option<crate::models::McpTargetRef>,
    phase: impl Into<String>,
    status_text: Option<String>,
) {
    let phase = phase.into();
    mutate_live_session(state, ctx, move |session| {
        if let Some(target) = target.clone() {
            session.bound_thread_id = Some(target.thread_id.clone());
            session.last_target = Some(target);
        }
        session.phase = Some(phase.clone());
        session.status_text = status_text.clone();
        session.busy = false;
        session.activity_label = None;
        session.activity_started_at = None;
        session.attention_kind = None;
        session.waiting_on_prompt = false;
    })
    .await;
}

pub(super) async fn settle_live_render_phase<T>(
    state: &AppState,
    ctx: &AgentContext,
    thread_id: Option<&str>,
    message_id: Option<&str>,
    model_id: Option<String>,
    result: &AppResult<T>,
) {
    let (Some(thread_id), Some(message_id)) = (thread_id, message_id) else {
        return;
    };
    let (phase, status_text) = match result {
        Ok(_) => ("idle", Some("Ready.".to_string())),
        Err(err) => ("error", Some(err.message.clone())),
    };
    mark_live_session_idle(
        state,
        ctx,
        Some(session_target_ref(
            thread_id.to_string(),
            message_id.to_string(),
            model_id,
        )),
        phase,
        status_text,
    )
    .await;
}

pub(super) async fn current_turn_working_user_message_ids_for_thread(
    state: &AppState,
    session_id: &str,
    thread_id: &str,
) -> Vec<String> {
    state
        .mcp_sessions
        .lock()
        .await
        .get(session_id)
        .and_then(|session| {
            (session.current_turn_thread_id.as_deref() == Some(thread_id))
                .then(|| session.current_turn_working_message_ids.clone())
        })
        .unwrap_or_default()
}

async fn current_turn_working_version_message_id_for_thread(
    state: &AppState,
    session_id: &str,
    thread_id: &str,
) -> Option<String> {
    state
        .mcp_sessions
        .lock()
        .await
        .get(session_id)
        .and_then(|session| {
            (session.current_turn_thread_id.as_deref() == Some(thread_id))
                .then(|| session.current_turn_working_version_message_id.clone())
                .flatten()
        })
}

pub(super) async fn remember_turn_working_user_messages(
    state: &AppState,
    session_id: &str,
    thread_id: &str,
    message_ids: &[String],
) {
    let mut sessions = state.mcp_sessions.lock().await;
    if let Some(session) = sessions.get_mut(session_id) {
        if session.current_turn_id.is_none() {
            session.current_turn_id = Some(Uuid::new_v4().to_string());
        }
        session.current_turn_thread_id = Some(thread_id.to_string());
        let mut next_ids = session.current_turn_working_message_ids.clone();
        for message_id in message_ids {
            if !next_ids.contains(message_id) {
                next_ids.push(message_id.clone());
            }
        }
        session.current_turn_working_message_ids = next_ids;
        session.updated_at = now_secs();
    }
}

async fn remember_turn_working_version_message(
    state: &AppState,
    session_id: &str,
    thread_id: &str,
    message_id: &str,
) {
    let mut sessions = state.mcp_sessions.lock().await;
    if let Some(session) = sessions.get_mut(session_id) {
        if session.current_turn_id.is_none() {
            session.current_turn_id = Some(Uuid::new_v4().to_string());
        }
        session.current_turn_thread_id = Some(thread_id.to_string());
        session.current_turn_working_version_message_id = Some(message_id.to_string());
        session.updated_at = now_secs();
    }
}

pub(super) async fn clear_turn_working_state(state: &AppState, session_id: &str, thread_id: &str) {
    let mut sessions = state.mcp_sessions.lock().await;
    if let Some(session) = sessions.get_mut(session_id) {
        if session.current_turn_thread_id.as_deref() == Some(thread_id) {
            session.current_turn_working_message_ids.clear();
            session.current_turn_working_version_message_id = None;
            session.current_turn_id = None;
            session.current_turn_thread_id = None;
        }
        session.updated_at = now_secs();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn ensure_turn_working_version_message(
    state: &AppState,
    app: &dyn PathResolver,
    ctx: &AgentContext,
    thread_id: &str,
    base_message_id: &str,
    model_id: Option<String>,
    design_output: &DesignOutput,
    artifact_bundle: Option<ArtifactBundle>,
    model_manifest: Option<ModelManifest>,
    create_summary: String,
) -> AppResult<String> {
    if let Some(message_id) =
        current_turn_working_version_message_id_for_thread(state, &ctx.session_id, thread_id).await
    {
        let conn = state.db.lock().await;
        if crate::services::target::resolve_target(
            &conn,
            app,
            Some(thread_id.to_string()),
            Some(message_id.clone()),
        )
        .is_ok()
        {
            return Ok(message_id);
        }
        drop(conn);
    }

    let mut working_design = design_output.clone();
    working_design.version_name.clear();

    let save_result = save_or_update_agent_version_for_session(
        state,
        app,
        SaveOrUpdateAgentVersionRequest {
            session_id: ctx.session_id.clone(),
            thread_id: thread_id.to_string(),
            base_message_id: base_message_id.to_string(),
            model_id,
            design_output: working_design,
            artifact_bundle,
            model_manifest,
            updated_at: now_secs(),
            response_text_created: create_summary,
            response_text_updated: String::new(),
            preserve_existing_title: true,
            preserve_existing_version_name: false,
            force_create_new_message: true,
            announce_created_working_version: true,
        },
    )
    .await?;
    remember_turn_working_version_message(
        state,
        &ctx.session_id,
        &save_result.thread_id,
        &save_result.message_id,
    )
    .await;
    Ok(save_result.message_id)
}

pub(super) async fn resolve_turn_working_target(
    state: &AppState,
    app: &dyn PathResolver,
    ctx: &AgentContext,
    target: crate::services::target::ResolvedTarget,
    create_summary: String,
) -> AppResult<crate::services::target::ResolvedTarget> {
    let working_message_id = ensure_turn_working_version_message(
        state,
        app,
        ctx,
        &target.thread_id,
        &target.message_id,
        target
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone()),
        target
            .design
            .as_ref()
            .ok_or_else(|| AppError::validation("Target has no design output."))?,
        target.artifact_bundle.clone(),
        target.model_manifest.clone(),
        create_summary,
    )
    .await?;
    let conn = state.db.lock().await;
    crate::services::target::resolve_target(
        &conn,
        app,
        Some(target.thread_id),
        Some(working_message_id),
    )
}

pub(super) fn map_target_resolved_from(
    source: crate::services::target::EditableTargetSource,
) -> TargetResolvedFrom {
    match source {
        crate::services::target::EditableTargetSource::Base => TargetResolvedFrom::Base,
    }
}

#[derive(Debug, Clone)]
pub struct StoreSessionRenderPreviewRequest {
    pub thread_id: String,
    pub base_message_id: Option<String>,
    pub design_output: DesignOutput,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
    pub draft_feedback: Option<DraftFeedbackSeed>,
}

#[derive(Debug, Clone)]
pub struct SessionRenderPreview {
    pub session_id: String,
    pub preview_id: String,
    pub thread_id: String,
    pub base_message_id: Option<String>,
    pub design_output: DesignOutput,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
    pub draft_feedback: Option<crate::models::AgentDraftFeedback>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DraftFeedbackSeed {
    pub status: crate::models::AgentDraftFeedbackStatus,
    pub summary: String,
    pub items: Vec<crate::models::AgentDraftFeedbackItem>,
    pub authoring_lints: Vec<crate::models::AgentDraftFeedbackAuthoringLint>,
    pub source: crate::models::AgentDraftFeedbackSource,
}

static SESSION_RENDER_PREVIEWS: OnceLock<StdMutex<HashMap<String, SessionRenderPreview>>> =
    OnceLock::new();

fn session_render_previews() -> &'static StdMutex<HashMap<String, SessionRenderPreview>> {
    SESSION_RENDER_PREVIEWS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn preview_matches_request(
    preview: &SessionRenderPreview,
    thread_id: Option<&str>,
    message_id: Option<&str>,
) -> bool {
    if let Some(thread_id) = thread_id {
        if preview.thread_id != thread_id {
            return false;
        }
    }
    if let Some(message_id) = message_id {
        message_id == preview.preview_id || preview.base_message_id.as_deref() == Some(message_id)
    } else {
        true
    }
}

pub fn session_render_preview_for_request(
    ctx: &AgentContext,
    thread_id: Option<&str>,
    message_id: Option<&str>,
) -> Option<SessionRenderPreview> {
    session_render_previews()
        .lock()
        .unwrap()
        .get(&ctx.session_id)
        .filter(|preview| preview_matches_request(preview, thread_id, message_id))
        .cloned()
}

fn session_render_preview_from_draft(draft: AgentDraft) -> SessionRenderPreview {
    SessionRenderPreview {
        session_id: draft.session_id,
        preview_id: draft.preview_id,
        thread_id: draft.thread_id,
        base_message_id: draft.base_message_id,
        design_output: draft.design_output,
        artifact_bundle: draft.artifact_bundle,
        model_manifest: draft.model_manifest,
        draft_feedback: draft.draft_feedback,
        updated_at: draft.updated_at,
    }
}

pub(super) fn agent_draft_from_session_render_preview(preview: SessionRenderPreview) -> AgentDraft {
    AgentDraft {
        preview_id: preview.preview_id,
        session_id: preview.session_id,
        thread_id: preview.thread_id,
        base_message_id: preview.base_message_id,
        design_output: preview.design_output,
        artifact_bundle: preview.artifact_bundle,
        model_manifest: preview.model_manifest,
        draft_feedback: preview.draft_feedback,
        updated_at: preview.updated_at,
    }
}

pub(super) fn draft_feedback_from_structural_verification(
    result: &crate::contracts::StructuralVerificationResult,
) -> DraftFeedbackSeed {
    let status = if result.passed {
        crate::models::AgentDraftFeedbackStatus::Passed
    } else if matches!(
        result.verifier_status,
        crate::contracts::VerifierStatus::SkippedUnavailable
            | crate::contracts::VerifierStatus::SkippedBackendUnavailable
    ) {
        crate::models::AgentDraftFeedbackStatus::Warning
    } else {
        crate::models::AgentDraftFeedbackStatus::Failed
    };
    let summary = if result.passed {
        result.summary.trim().to_string()
    } else if let Some(first_issue) = result.issues.first() {
        if result.issues.len() > 1 {
            format!(
                "{} (+{} more)",
                first_issue.message,
                result.issues.len() - 1
            )
        } else {
            first_issue.message.clone()
        }
    } else {
        result.summary.trim().to_string()
    };
    let items = result
        .issues
        .iter()
        .take(3)
        .map(|issue| crate::models::AgentDraftFeedbackItem {
            code: issue.code.clone(),
            message: issue.message.clone(),
        })
        .collect();
    DraftFeedbackSeed {
        status,
        summary,
        items,
        authoring_lints: Vec::new(),
        source: crate::models::AgentDraftFeedbackSource::StructuralVerification,
    }
}

fn hydrate_draft_feedback(
    preview: &SessionRenderPreview,
    seed: Option<DraftFeedbackSeed>,
) -> Option<crate::models::AgentDraftFeedback> {
    seed.map(|seed| crate::models::AgentDraftFeedback {
        session_id: preview.session_id.clone(),
        thread_id: preview.thread_id.clone(),
        preview_id: preview.preview_id.clone(),
        status: seed.status,
        summary: seed.summary,
        items: seed.items,
        authoring_lints: seed.authoring_lints,
        source: seed.source,
    })
}

fn draft_feedback_authoring_lints_for_design_output(
    design_output: &DesignOutput,
) -> Vec<crate::models::AgentDraftFeedbackAuthoringLint> {
    if design_output.source_language != SourceLanguage::EckyIrV0 {
        return Vec::new();
    }
    let Ok(program) = crate::ecky_scheme::compile_to_core_program(&design_output.macro_code) else {
        return Vec::new();
    };
    collect_ecky_constraint_authoring_lints(&design_output.macro_code, &program)
        .into_iter()
        .map(|lint| crate::models::AgentDraftFeedbackAuthoringLint {
            kind: lint.kind,
            part_key: lint.part_key,
            param_key: lint.param_key,
            delta: lint.delta,
            occurrence_count: lint.occurrence_count,
            suggested_param_key: lint.suggested_param_key,
            message: lint.message,
            source_stable_node_keys: lint.source_stable_node_keys,
        })
        .collect()
}

pub async fn resolve_session_render_preview_for_request(
    state: &AppState,
    ctx: &AgentContext,
    thread_id: Option<&str>,
    message_id: Option<&str>,
) -> AppResult<Option<SessionRenderPreview>> {
    if let Some(preview) = session_render_preview_for_request(ctx, thread_id, message_id) {
        return Ok(Some(preview));
    }

    let draft = {
        let conn = state.db.lock().await;
        db::get_agent_draft_for_session(&conn, &ctx.session_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
    };
    let Some(draft) = draft else {
        return Ok(None);
    };
    let preview = session_render_preview_from_draft(draft);
    if !preview_matches_request(&preview, thread_id, message_id) {
        return Ok(None);
    }
    session_render_previews()
        .lock()
        .unwrap()
        .insert(ctx.session_id.clone(), preview.clone());
    Ok(Some(preview))
}

fn clear_session_render_preview(session_id: &str) {
    session_render_previews().lock().unwrap().remove(session_id);
}

pub(super) async fn clear_session_render_preview_durable(
    state: &AppState,
    session_id: &str,
) -> AppResult<()> {
    clear_session_render_preview(session_id);
    let conn = state.db.lock().await;
    db::delete_agent_draft_for_session(&conn, session_id)
        .map_err(|e| AppError::persistence(e.to_string()))?;
    Ok(())
}

pub async fn store_session_render_preview(
    state: &AppState,
    app: &dyn PathResolver,
    ctx: &AgentContext,
    req: StoreSessionRenderPreviewRequest,
) -> AppResult<SessionRenderPreview> {
    let total_started = Instant::now();
    let validate_started = Instant::now();
    let thread_id_for_log = req.thread_id.clone();
    let base_message_id_for_log = req.base_message_id.clone();
    let model_id_for_log = req.artifact_bundle.model_id.clone();
    crate::models::validate_design_output(&req.design_output)?;
    crate::models::validate_model_runtime_bundle(&req.model_manifest, &req.artifact_bundle)?;
    push_mcp_profile(
        state,
        ctx,
        "preview_store",
        "validate_contracts",
        validate_started,
        Some(&thread_id_for_log),
        base_message_id_for_log.as_deref(),
        Some(&model_id_for_log),
    );
    let feedback_started = Instant::now();
    let draft_feedback = req.draft_feedback.clone().or_else(|| {
        Some(draft_feedback_from_structural_verification(
            &crate::services::structural_verification::verify_structure(
                &req.artifact_bundle,
                &req.model_manifest,
            ),
        ))
    });
    let authoring_lints = draft_feedback_authoring_lints_for_design_output(&req.design_output);
    let draft_feedback = draft_feedback.map(|mut feedback| {
        if feedback.authoring_lints.is_empty() {
            feedback.authoring_lints = authoring_lints;
        }
        feedback
    });
    push_mcp_profile(
        state,
        ctx,
        "preview_store",
        "feedback_and_lints",
        feedback_started,
        Some(&thread_id_for_log),
        base_message_id_for_log.as_deref(),
        Some(&model_id_for_log),
    );

    let preview = SessionRenderPreview {
        session_id: ctx.session_id.clone(),
        preview_id: Uuid::new_v4().to_string(),
        thread_id: req.thread_id,
        base_message_id: req.base_message_id,
        design_output: req.design_output,
        artifact_bundle: req.artifact_bundle,
        model_manifest: req.model_manifest,
        draft_feedback: None,
        updated_at: now_secs(),
    };
    let preview = SessionRenderPreview {
        draft_feedback: hydrate_draft_feedback(&preview, draft_feedback),
        ..preview
    };

    session_render_previews()
        .lock()
        .unwrap()
        .insert(ctx.session_id.clone(), preview.clone());

    {
        let db_started = Instant::now();
        let conn = state.db.lock().await;
        db::upsert_agent_draft(
            &conn,
            &AgentDraft {
                preview_id: preview.preview_id.clone(),
                session_id: preview.session_id.clone(),
                thread_id: preview.thread_id.clone(),
                base_message_id: preview.base_message_id.clone(),
                design_output: preview.design_output.clone(),
                artifact_bundle: preview.artifact_bundle.clone(),
                model_manifest: preview.model_manifest.clone(),
                draft_feedback: preview.draft_feedback.clone(),
                updated_at: preview.updated_at,
            },
        )
        .map_err(|e| AppError::persistence(e.to_string()))?;
        push_mcp_profile(
            state,
            ctx,
            "preview_store",
            "db_upsert_draft",
            db_started,
            Some(&preview.thread_id),
            preview.base_message_id.as_deref(),
            Some(&preview.artifact_bundle.model_id),
        );
    }

    let snapshot_started = Instant::now();
    let snapshot = crate::services::session::build_runtime_snapshot(
        Some(preview.design_output.clone()),
        Some(preview.thread_id.clone()),
        Some(preview.preview_id.clone()),
        Some(preview.artifact_bundle.clone()),
        Some(preview.model_manifest.clone()),
        None,
    );
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
    }
    crate::services::session::write_last_snapshot(app, Some(&snapshot));
    push_mcp_profile(
        state,
        ctx,
        "preview_store",
        "snapshot_write",
        snapshot_started,
        Some(&preview.thread_id),
        Some(&preview.preview_id),
        Some(&preview.artifact_bundle.model_id),
    );

    let emit_started = Instant::now();
    state.emit_agent_draft_preview_updated(&crate::contracts::AgentDraftPreviewUpdatedEvent {
        session_id: preview.session_id.clone(),
        thread_id: preview.thread_id.clone(),
        preview_id: preview.preview_id.clone(),
        base_message_id: preview.base_message_id.clone(),
        model_id: Some(preview.artifact_bundle.model_id.clone()),
        design: preview.design_output.clone(),
        artifact_bundle: preview.artifact_bundle.clone(),
        model_manifest: preview.model_manifest.clone(),
        feedback: preview.draft_feedback.clone(),
    });
    push_mcp_profile(
        state,
        ctx,
        "preview_store",
        "emit_draft_event",
        emit_started,
        Some(&preview.thread_id),
        Some(&preview.preview_id),
        Some(&preview.artifact_bundle.model_id),
    );
    push_mcp_profile(
        state,
        ctx,
        "preview_store",
        "total",
        total_started,
        Some(&preview.thread_id),
        Some(&preview.preview_id),
        Some(&preview.artifact_bundle.model_id),
    );

    Ok(preview)
}

pub(super) fn artifact_bundle_digest(bundle: &ArtifactBundle) -> ArtifactBundleDigest {
    let export_formats: Vec<String> = bundle
        .export_artifacts
        .iter()
        .map(|artifact| artifact.format.as_str().to_string())
        .collect();
    let step_export_path = bundle
        .export_artifacts
        .iter()
        .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
        .map(|artifact| artifact.path.clone());
    ArtifactBundleDigest {
        model_id: bundle.model_id.clone(),
        content_hash: bundle.content_hash.clone(),
        source_language: bundle.source_language.as_str().to_string(),
        geometry_backend: bundle.geometry_backend.as_str().to_string(),
        has_preview_stl: !bundle.preview_stl_path.is_empty(),
        viewer_asset_count: bundle.viewer_assets.len(),
        edge_target_count: bundle.edge_targets.len(),
        face_target_count: bundle.face_targets.len(),
        export_format_count: bundle.export_artifacts.len(),
        export_formats,
        has_step_export: step_export_path.is_some(),
        step_export_path,
        multipart: bundle.viewer_assets.len() > 1,
    }
}

#[cfg(test)]
mod tests;
