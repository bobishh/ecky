use crate::db;
use crate::freecad::resolve_resource_path;
use crate::mcp::contracts::*;
use crate::mcp::runtime;
use crate::models::{
    AgentDraft, AgentSession, AppError, AppErrorCode, AppResult, AppState, ArtifactBundle,
    ControlPrimitive, ControlView, ControlViewSource, DesignOutput, DesignParams, InteractionMode,
    MacroDialect, MeasurementAnnotation, MeasurementAnnotationSource, ModelManifest,
    ModelSourceKind, ParamValue, PathResolver, SourceLanguage, UiSpec, WorkspaceSceneLens,
    WorkspaceSceneRepresentation, WorkspaceSceneRepresentationKind,
    WorkspaceSceneRepresentationStatus, WorkspaceSceneTopology,
};
use crate::services::agent_versions::{
    save_or_update_agent_version_for_session, SaveOrUpdateAgentVersionRequest,
};
use crate::services::design::{auto_heal_legacy_params, is_param_schema_mismatch};
use crate::services::{agent_dialogue, history, render};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::sync::oneshot;
use uuid::Uuid;

const THREAD_MESSAGE_CONTENT_MAX_CHARS: usize = 240;

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

fn now_secs() -> u64 {
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

fn push_mcp_profile(
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

fn compact_message_content(content: &str) -> String {
    crate::context::compact_text(content, THREAD_MESSAGE_CONTENT_MAX_CHARS)
}

fn configured_prompt_timeout_secs(state: &AppState, override_timeout_secs: Option<u64>) -> u64 {
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

fn push_unique_strings(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.iter().any(|existing| existing == value) {
            target.push(value.clone());
        }
    }
}

fn selection_target_match_ids(target: &crate::models::SelectionTarget) -> Vec<String> {
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

fn carry_forward_semantic_manifest(
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

async fn claim_owner_for_thread(state: &AppState, thread_id: &str) -> Option<AgentSession> {
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

async fn ensure_thread_claim(
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

fn dialogue_identity(ctx: &AgentContext) -> agent_dialogue::AgentDialogueIdentity {
    agent_dialogue::AgentDialogueIdentity {
        session_id: ctx.session_id.clone(),
        client_kind: ctx.client_kind.clone(),
        host_label: ctx.host_label.clone(),
        agent_label: ctx.agent_label.clone(),
        llm_model_id: ctx.llm_model_id.clone(),
        llm_model_label: ctx.llm_model_label.clone(),
    }
}

struct TraceEvent<'a> {
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
    phase: &'a str,
    kind: &'a str,
    summary: String,
    details: Option<String>,
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

fn push_trace_event(state: &AppState, ctx: &AgentContext, event: TraceEvent<'_>) {
    log_trace_event(state, ctx, event);
}

fn push_trace_event_with_conn(
    state: &AppState,
    _conn: &rusqlite::Connection,
    ctx: &AgentContext,
    event: TraceEvent<'_>,
) {
    log_trace_event(state, ctx, event);
}

fn has_managed_runtime_session(state: &AppState, session_id: &str) -> bool {
    runtime::runtime_snapshot_by_session_id(state, session_id).is_some()
}

async fn resolve_prompt_thread_context(
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

async fn resolve_explicit_session_target(
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

async fn resolve_request_user_prompt_target(
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

fn emit_prompt_closed(
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
struct ManagedPendingTarget {
    thread_id: String,
    message_id: Option<String>,
    model_id: Option<String>,
}

fn managed_pending_target(state: &AppState, session_id: &str) -> Option<ManagedPendingTarget> {
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

async fn mutate_live_session<F>(state: &AppState, ctx: &AgentContext, mutate: F)
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

async fn drop_live_session(state: &AppState, session_id: &str) {
    state
        .close_prompts_for_session(session_id, "session_disconnected")
        .await;
    state.mcp_sessions.lock().await.remove(session_id);
}

fn session_target_ref(
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

async fn mark_live_session_waiting(
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

async fn mark_live_session_busy(
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

async fn mark_live_session_idle(
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

async fn settle_live_render_phase<T>(
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

async fn current_turn_working_user_message_ids_for_thread(
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

async fn remember_turn_working_user_messages(
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

async fn clear_turn_working_state(state: &AppState, session_id: &str, thread_id: &str) {
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
async fn ensure_turn_working_version_message(
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

async fn resolve_turn_working_target(
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

async fn resolve_turn_working_semantic_target(
    state: &AppState,
    app: &dyn PathResolver,
    ctx: &AgentContext,
    target: SemanticTargetState,
    create_summary: String,
) -> AppResult<SemanticTargetState> {
    let working_message_id = ensure_turn_working_version_message(
        state,
        app,
        ctx,
        &target.thread_id,
        &target.message_id,
        Some(target.artifact_bundle.model_id.clone()),
        &target.design_output,
        Some(target.artifact_bundle.clone()),
        Some(target.model_manifest.clone()),
        create_summary,
    )
    .await?;
    let conn = state.db.lock().await;
    resolve_semantic_target(&conn, app, Some(target.thread_id), Some(working_message_id))
}

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
    let runtime_capabilities = crate::runtime_capabilities::collect_runtime_capabilities(
        render::configured_freecad_cmd(state).as_deref(),
        app,
    );
    let freecad_configured = runtime_capabilities.freecad.available;
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
        runtime_capabilities,
    })
}

pub async fn handle_ui_dispatch(
    app: &tauri::AppHandle,
    params: UiDispatchRequest,
) -> AppResult<UiDispatchResponse> {
    app.emit(
        "mcp://ui-dispatch",
        AgentUiDispatchEvent {
            action: params.action,
            target: params.target,
            value: params.value,
        },
    )
    .map_err(|e| AppError::internal(format!("Failed to emit UI dispatch event: {}", e)))?;

    Ok(UiDispatchResponse { success: true })
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

pub async fn handle_thread_get(
    state: &AppState,
    req: ThreadGetRequest,
) -> AppResult<ThreadGetResponse> {
    let conn = state.db.lock().await;
    let thread = history::get_thread(&conn, &req.thread_id)?;
    drop(conn);
    Ok(ThreadGetResponse {
        thread,
        claim_owner: claim_owner_for_thread(state, &req.thread_id).await,
    })
}

pub async fn handle_thread_messages_get(
    state: &AppState,
    req: ThreadMessagesRequest,
) -> AppResult<ThreadMessagesResponse> {
    let conn = state.db.lock().await;
    let thread = history::get_thread(&conn, &req.thread_id)?;
    drop(conn);

    let mut messages = thread.messages;

    // Filter by before ID
    if let Some(before_id) = &req.before {
        if let Some(pos) = messages.iter().position(|m| &m.id == before_id) {
            messages.truncate(pos);
        }
    }

    // Filter by roles
    if let Some(roles) = &req.roles {
        messages.retain(|m| {
            let role_str = serde_json::to_value(&m.role)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            roles.contains(&role_str)
        });
    }

    // Limit
    if let Some(limit) = req.limit {
        let len = messages.len();
        if len > limit {
            messages = messages.split_off(len - limit);
        }
    }

    let compact_messages = messages
        .into_iter()
        .map(|m| ThreadMessageEntry {
            id: m.id,
            role: serde_json::to_value(&m.role)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            status: serde_json::to_value(&m.status)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            timestamp: m.timestamp,
            content: compact_message_content(&m.content),
            has_output: m.output.is_some(),
            has_artifacts: m.artifact_bundle.is_some(),
            has_manifest: m.model_manifest.is_some(),
        })
        .collect();

    Ok(ThreadMessagesResponse {
        thread_id: req.thread_id,
        messages: compact_messages,
    })
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

        let artifact_digest = target.artifact_bundle.as_ref().map(artifact_bundle_digest);

        Ok(TargetGetResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            title: design.title,
            version_name: design.version_name,
            macro_code: design.macro_code,
            ui_spec: design.ui_spec,
            initial_params: design.initial_params,
            artifact_bundle: target.artifact_bundle,
            artifact_digest,
            model_manifest: target.model_manifest,
            latest_draft: None,
        })
    })();

    if let Err(err) = &result {
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

fn map_target_resolved_from(
    source: crate::services::target::EditableTargetSource,
) -> TargetResolvedFrom {
    match source {
        crate::services::target::EditableTargetSource::Base => TargetResolvedFrom::Base,
    }
}

fn build_agent_scene_packet(
    design_output: &DesignOutput,
    artifact_bundle: Option<&ArtifactBundle>,
    model_manifest: Option<&ModelManifest>,
    has_draft: bool,
) -> crate::models::AgentScenePacket {
    let has_source = !design_output.macro_code.trim().is_empty();
    let exact_committed = artifact_bundle.is_some() && model_manifest.is_some();
    let sketch_status = if has_source {
        WorkspaceSceneRepresentationStatus::Rebuildable
    } else {
        WorkspaceSceneRepresentationStatus::Pending
    };
    let draft_status = if has_draft {
        WorkspaceSceneRepresentationStatus::Fresh
    } else if has_source {
        WorkspaceSceneRepresentationStatus::Stale
    } else {
        WorkspaceSceneRepresentationStatus::Pending
    };
    let exact_status = if exact_committed {
        WorkspaceSceneRepresentationStatus::Committed
    } else if has_source {
        WorkspaceSceneRepresentationStatus::Rebuildable
    } else {
        WorkspaceSceneRepresentationStatus::Pending
    };
    let active_lens = if exact_status == WorkspaceSceneRepresentationStatus::Committed {
        WorkspaceSceneLens::Exact
    } else if draft_status == WorkspaceSceneRepresentationStatus::Fresh {
        WorkspaceSceneLens::Draft
    } else {
        WorkspaceSceneLens::Sketch
    };
    let mut allowed_patch_targets = Vec::new();
    if has_source {
        allowed_patch_targets.push("macroBufferReplaceAndPreview".to_string());
    }
    if design_output.source_language == SourceLanguage::EckyIrV0 {
        allowed_patch_targets.push("eckyAstReplaceAndRender".to_string());
    }
    if model_manifest.is_some() {
        allowed_patch_targets.push("semanticManifestPatch".to_string());
    }
    if has_draft {
        allowed_patch_targets.push("commitPreviewVersion".to_string());
    }

    crate::models::AgentScenePacket {
        schema_version: 1,
        active_lens,
        representations: vec![
            WorkspaceSceneRepresentation {
                kind: WorkspaceSceneRepresentationKind::SketchIntent,
                status: sketch_status,
            },
            WorkspaceSceneRepresentation {
                kind: WorkspaceSceneRepresentationKind::MeshDraft,
                status: draft_status,
            },
            WorkspaceSceneRepresentation {
                kind: WorkspaceSceneRepresentationKind::ExactModel,
                status: exact_status,
            },
        ],
        topology: WorkspaceSceneTopology {
            edge_target_count: artifact_bundle
                .map(|bundle| bundle.edge_targets.len())
                .unwrap_or(0),
            face_target_count: artifact_bundle
                .map(|bundle| bundle.face_targets.len())
                .unwrap_or(0),
            selection_target_count: model_manifest
                .map(|manifest| manifest.selection_targets.len())
                .unwrap_or(0),
            control_primitive_count: model_manifest
                .map(|manifest| manifest.control_primitives.len())
                .unwrap_or(0),
            control_relation_count: model_manifest
                .map(|manifest| manifest.control_relations.len())
                .unwrap_or(0),
            control_view_count: model_manifest
                .map(|manifest| manifest.control_views.len())
                .unwrap_or(0),
        },
        allowed_patch_targets,
    }
}

#[allow(dead_code)]
fn build_target_meta_response(
    target: &crate::services::target::EditableTarget,
) -> TargetMetaResponse {
    let (range_count, number_count, select_count, checkbox_count) = target
        .design_output
        .ui_spec
        .fields
        .iter()
        .fold((0, 0, 0, 0), |acc, field| match field {
            crate::models::UiField::Range { .. } => (acc.0 + 1, acc.1, acc.2, acc.3),
            crate::models::UiField::Number { .. } => (acc.0, acc.1 + 1, acc.2, acc.3),
            crate::models::UiField::Select { .. } => (acc.0, acc.1, acc.2 + 1, acc.3),
            crate::models::UiField::Checkbox { .. } => (acc.0, acc.1, acc.2, acc.3 + 1),
            crate::models::UiField::Image { .. } => acc,
        });

    let export_formats = target
        .artifact_bundle
        .as_ref()
        .map(|bundle| {
            bundle
                .export_artifacts
                .iter()
                .map(|artifact| artifact.format.as_str().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let has_step_export = export_formats
        .iter()
        .any(|format| format.eq_ignore_ascii_case("step"));
    let step_export_path = target.artifact_bundle.as_ref().and_then(|bundle| {
        bundle
            .export_artifacts
            .iter()
            .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
            .map(|artifact| artifact.path.clone())
    });
    let edge_target_count = target
        .artifact_bundle
        .as_ref()
        .map(|bundle| bundle.edge_targets.len())
        .unwrap_or(0);
    let face_target_count = target
        .artifact_bundle
        .as_ref()
        .map(|bundle| bundle.face_targets.len())
        .unwrap_or(0);

    TargetMetaResponse {
        thread_id: target.thread_id.clone(),
        message_id: target.message_id.clone(),
        title: target.design_output.title.clone(),
        version_name: target.design_output.version_name.clone(),
        model_id: target.model_id(),
        source_language: target.design_output.source_language.as_str().to_string(),
        macro_dialect: crate::mcp::authoring::macro_dialect_label(
            &target.design_output.macro_dialect,
        )
        .to_string(),
        geometry_backend: target.design_output.geometry_backend.as_str().to_string(),
        has_draft: false,
        resolved_from: map_target_resolved_from(target.resolved_from),
        has_artifact_bundle: target.artifact_bundle.is_some(),
        has_runtime_manifest: target.artifact_bundle.is_some() && target.model_manifest.is_some(),
        export_formats,
        has_step_export,
        step_export_path,
        edge_target_count,
        face_target_count,
        ui_field_count: target.design_output.ui_spec.fields.len(),
        range_count,
        number_count,
        select_count,
        checkbox_count,
        parameter_count: target.design_output.initial_params.len(),
        has_semantic_manifest: target.model_manifest.is_some(),
        control_primitive_count: target
            .model_manifest
            .as_ref()
            .map(|manifest| manifest.control_primitives.len())
            .unwrap_or(0),
        control_relation_count: target
            .model_manifest
            .as_ref()
            .map(|manifest| manifest.control_relations.len())
            .unwrap_or(0),
        control_view_count: target
            .model_manifest
            .as_ref()
            .map(|manifest| manifest.control_views.len())
            .unwrap_or(0),
        scene_packet: build_agent_scene_packet(
            &target.design_output,
            target.artifact_bundle.as_ref(),
            target.model_manifest.as_ref(),
            false,
        ),
    }
}

pub async fn handle_target_meta_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: TargetMetaRequest,
    ctx: &AgentContext,
) -> AppResult<TargetMetaResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<TargetMetaResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading target metadata.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let has_draft = preview.is_some();
        let (target_thread_id, target_message_id, design_output, artifact_bundle, model_manifest) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
                    Some(preview.model_manifest),
                )
            } else {
                let target = crate::services::target::resolve_editable_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                (
                    target.thread_id,
                    target.message_id,
                    target.design_output,
                    target.artifact_bundle,
                    target.model_manifest,
                )
            };

        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());
        tracked_model_id = artifact_bundle
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

        let (range_count, number_count, select_count, checkbox_count) = design_output
            .ui_spec
            .fields
            .iter()
            .fold((0, 0, 0, 0), |acc, field| match field {
                crate::models::UiField::Range { .. } => (acc.0 + 1, acc.1, acc.2, acc.3),
                crate::models::UiField::Number { .. } => (acc.0, acc.1 + 1, acc.2, acc.3),
                crate::models::UiField::Select { .. } => (acc.0, acc.1, acc.2 + 1, acc.3),
                crate::models::UiField::Checkbox { .. } => (acc.0, acc.1, acc.2, acc.3 + 1),
                crate::models::UiField::Image { .. } => acc,
            });
        let export_formats = artifact_bundle
            .as_ref()
            .map(|bundle| {
                bundle
                    .export_artifacts
                    .iter()
                    .map(|artifact| artifact.format.as_str().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let has_step_export = export_formats
            .iter()
            .any(|format| format.eq_ignore_ascii_case("step"));
        let step_export_path = artifact_bundle.as_ref().and_then(|bundle| {
            bundle
                .export_artifacts
                .iter()
                .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
                .map(|artifact| artifact.path.clone())
        });
        let edge_target_count = artifact_bundle
            .as_ref()
            .map(|bundle| bundle.edge_targets.len())
            .unwrap_or(0);
        let face_target_count = artifact_bundle
            .as_ref()
            .map(|bundle| bundle.face_targets.len())
            .unwrap_or(0);
        let scene_packet = build_agent_scene_packet(
            &design_output,
            artifact_bundle.as_ref(),
            model_manifest.as_ref(),
            has_draft,
        );

        Ok(TargetMetaResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            model_id: artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.clone()),
            source_language: design_output.source_language.as_str().to_string(),
            macro_dialect: crate::mcp::authoring::macro_dialect_label(&design_output.macro_dialect)
                .to_string(),
            geometry_backend: design_output.geometry_backend.as_str().to_string(),
            has_draft,
            resolved_from: TargetResolvedFrom::Base,
            has_artifact_bundle: artifact_bundle.is_some(),
            has_runtime_manifest: artifact_bundle.is_some() && model_manifest.is_some(),
            export_formats,
            has_step_export,
            step_export_path,
            edge_target_count,
            face_target_count,
            ui_field_count: design_output.ui_spec.fields.len(),
            range_count,
            number_count,
            select_count,
            checkbox_count,
            parameter_count: design_output.initial_params.len(),
            has_semantic_manifest: model_manifest.is_some(),
            control_primitive_count: model_manifest
                .as_ref()
                .map(|manifest| manifest.control_primitives.len())
                .unwrap_or(0),
            control_relation_count: model_manifest
                .as_ref()
                .map(|manifest| manifest.control_relations.len())
                .unwrap_or(0),
            control_view_count: model_manifest
                .as_ref()
                .map(|manifest| manifest.control_views.len())
                .unwrap_or(0),
            scene_packet,
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_target_macro_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: TargetMacroRequest,
    ctx: &AgentContext,
) -> AppResult<TargetMacroResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<TargetMacroResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading target macro.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, design_output, artifact_bundle, _model_manifest) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
                    Some(preview.model_manifest),
                )
            } else {
                let target = crate::services::target::resolve_editable_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                (
                    target.thread_id,
                    target.message_id,
                    target.design_output,
                    target.artifact_bundle,
                    target.model_manifest,
                )
            };

        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());
        tracked_model_id = artifact_bundle
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

        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);
        let macro_code = design_output.macro_code;
        let lines = macro_buffer_lines(&macro_code);
        let line_count = lines.len();
        let digest = macro_buffer_digest(&macro_code);
        let (window_start_line, window_end_line, truncated, window_lines) =
            macro_buffer_line_window(&lines, req.start_line, req.end_line)?;

        Ok(TargetMacroResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            digest,
            line_count,
            window_start_line,
            window_end_line,
            truncated,
            lines: window_lines,
            macro_dialect: design_output.macro_dialect,
            post_processing: design_output.post_processing,
            authoring_context,
            artifact_digest,
        })
    })();

    if let Err(err) = &result {
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

#[derive(Debug, Clone)]
struct SessionMacroBuffer {
    thread_id: String,
    message_id: String,
    macro_code: String,
    macro_dialect: MacroDialect,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: crate::models::GeometryBackend,
}

static MACRO_BUFFERS: OnceLock<StdMutex<HashMap<String, SessionMacroBuffer>>> = OnceLock::new();

fn macro_buffers() -> &'static StdMutex<HashMap<String, SessionMacroBuffer>> {
    MACRO_BUFFERS.get_or_init(|| StdMutex::new(HashMap::new()))
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

fn agent_draft_from_session_render_preview(preview: SessionRenderPreview) -> AgentDraft {
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

fn draft_feedback_from_structural_verification(
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

async fn clear_session_render_preview_durable(state: &AppState, session_id: &str) -> AppResult<()> {
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

fn macro_buffer_digest(macro_code: &str) -> String {
    crate::mcp::macro_buffer::source_digest(macro_code)
}

fn ecky_ast_authoring_enabled(state: &AppState) -> bool {
    state.config.lock().unwrap().mcp.ecky_ast_authoring
}

const DEFAULT_ECKY_AST_DEPTH: usize = 3;
const DEFAULT_ECKY_AST_MAX_NODES: usize = 120;
const MAX_ECKY_AST_DEPTH: usize = 12;
const MAX_ECKY_AST_NODES: usize = 500;
const ECKY_AST_SOURCE_MAX_BYTES: usize = 4096;
const DEFAULT_MACRO_BUFFER_WINDOW_LINES: usize = 200;

fn macro_buffer_lines(macro_code: &str) -> Vec<MacroBufferLine> {
    macro_code
        .lines()
        .enumerate()
        .map(|(idx, text)| MacroBufferLine {
            line_number: idx + 1,
            text: text.to_string(),
        })
        .collect()
}

fn macro_buffer_line_window(
    lines: &[MacroBufferLine],
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> AppResult<(usize, usize, bool, Vec<MacroBufferLine>)> {
    let line_count = lines.len();
    if line_count == 0 {
        return Ok((0, 0, false, Vec::new()));
    }

    let start = start_line.unwrap_or(1);
    if start == 0 || start > line_count {
        return Err(AppError::validation(format!(
            "Macro buffer startLine {} is outside buffer line count {}.",
            start, line_count
        )));
    }

    let requested_end = end_line.unwrap_or_else(|| {
        std::cmp::min(
            line_count,
            start.saturating_add(DEFAULT_MACRO_BUFFER_WINDOW_LINES - 1),
        )
    });
    if requested_end < start {
        return Err(AppError::validation(format!(
            "Macro buffer endLine {} is before startLine {}.",
            requested_end, start
        )));
    }

    let end = std::cmp::min(requested_end, line_count);
    let window = lines[(start - 1)..end].to_vec();
    Ok((start, end, start > 1 || end < line_count, window))
}

fn path_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn core_node_kind_label(kind: &crate::ecky_core_ir::CoreNodeKind) -> &'static str {
    match kind {
        crate::ecky_core_ir::CoreNodeKind::Literal(_) => "Literal",
        crate::ecky_core_ir::CoreNodeKind::Reference(_) => "Reference",
        crate::ecky_core_ir::CoreNodeKind::Build { .. } => "Build",
        crate::ecky_core_ir::CoreNodeKind::Let { .. } => "Let",
        crate::ecky_core_ir::CoreNodeKind::If { .. } => "If",
        crate::ecky_core_ir::CoreNodeKind::Call { .. } => "Call",
        crate::ecky_core_ir::CoreNodeKind::Range { .. } => "Range",
        crate::ecky_core_ir::CoreNodeKind::Map { .. } => "Map",
        crate::ecky_core_ir::CoreNodeKind::Apply { .. } => "Apply",
        crate::ecky_core_ir::CoreNodeKind::List(_) => "List",
        crate::ecky_core_ir::CoreNodeKind::Group(_) => "Group",
    }
}

fn core_node_child_paths<'a>(
    node: &'a crate::ecky_core_ir::CoreNode,
    path: &str,
) -> Vec<(String, &'a crate::ecky_core_ir::CoreNode)> {
    match &node.kind {
        crate::ecky_core_ir::CoreNodeKind::Literal(_)
        | crate::ecky_core_ir::CoreNodeKind::Reference(_) => Vec::new(),
        crate::ecky_core_ir::CoreNodeKind::Build { bindings, result } => bindings
            .iter()
            .map(|binding| {
                (
                    format!("{}/build/bindings/{}", path, path_segment(&binding.name)),
                    &binding.value,
                )
            })
            .chain(std::iter::once((
                format!("{path}/build/result"),
                result.as_ref(),
            )))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::Let { bindings, body } => bindings
            .iter()
            .map(|binding| {
                (
                    format!("{}/let/bindings/{}", path, path_segment(&binding.name)),
                    &binding.value,
                )
            })
            .chain(std::iter::once((format!("{path}/let/body"), body.as_ref())))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => vec![
            (format!("{path}/if/condition"), condition.as_ref()),
            (format!("{path}/if/then"), then_branch.as_ref()),
            (format!("{path}/if/else"), else_branch.as_ref()),
        ],
        crate::ecky_core_ir::CoreNodeKind::Call { args, keywords, .. } => args
            .iter()
            .enumerate()
            .map(|(idx, arg)| (format!("{path}/call/args/{idx}"), arg))
            .chain(keywords.iter().map(|keyword| {
                (
                    format!("{}/call/keywords/{}", path, path_segment(&keyword.name)),
                    keyword.source_node(),
                )
            }))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::Range { start, end } => vec![
            (format!("{path}/range/start"), start.as_ref()),
            (format!("{path}/range/end"), end.as_ref()),
        ],
        crate::ecky_core_ir::CoreNodeKind::Map { sources, body, .. } => sources
            .iter()
            .enumerate()
            .map(|(idx, source)| (format!("{path}/map/sources/{idx}"), source))
            .chain(std::iter::once((format!("{path}/map/body"), body.as_ref())))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::Apply { args, list, .. } => args
            .iter()
            .enumerate()
            .map(|(idx, arg)| (format!("{path}/apply/args/{idx}"), arg))
            .chain(std::iter::once((
                format!("{path}/apply/list"),
                list.as_ref(),
            )))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::List(items) => items
            .iter()
            .enumerate()
            .map(|(idx, item)| (format!("{path}/list/{idx}"), item))
            .collect(),
        crate::ecky_core_ir::CoreNodeKind::Group(items) => items
            .iter()
            .enumerate()
            .map(|(idx, item)| (format!("{path}/group/{idx}"), item))
            .collect(),
    }
}

fn core_node_op_label(node: &crate::ecky_core_ir::CoreNode) -> Option<String> {
    match &node.kind {
        crate::ecky_core_ir::CoreNodeKind::Call { op, .. }
        | crate::ecky_core_ir::CoreNodeKind::Apply { op, .. } => Some(format!("{op:?}")),
        _ => None,
    }
}

fn core_node_digest(node: &crate::ecky_core_ir::CoreNode) -> String {
    let mut parts = vec![
        core_node_kind_label(&node.kind).to_string(),
        format!("{:?}", node.value_kind),
    ];
    match &node.kind {
        crate::ecky_core_ir::CoreNodeKind::Literal(value) => parts.push(format!("{value:?}")),
        crate::ecky_core_ir::CoreNodeKind::Reference(value) => parts.push(format!("{value:?}")),
        crate::ecky_core_ir::CoreNodeKind::Call { op, .. }
        | crate::ecky_core_ir::CoreNodeKind::Apply { op, .. } => parts.push(format!("{op:?}")),
        crate::ecky_core_ir::CoreNodeKind::Map { params, .. } => parts.push(format!("{params:?}")),
        crate::ecky_core_ir::CoreNodeKind::Build { bindings, .. } => parts.push(format!(
            "bindings:{:?}",
            bindings
                .iter()
                .map(|binding| &binding.name)
                .collect::<Vec<_>>()
        )),
        crate::ecky_core_ir::CoreNodeKind::Let { bindings, .. } => parts.push(format!(
            "bindings:{:?}",
            bindings
                .iter()
                .map(|binding| &binding.name)
                .collect::<Vec<_>>()
        )),
        crate::ecky_core_ir::CoreNodeKind::If { .. }
        | crate::ecky_core_ir::CoreNodeKind::Range { .. }
        | crate::ecky_core_ir::CoreNodeKind::List(_)
        | crate::ecky_core_ir::CoreNodeKind::Group(_) => {}
    }
    if let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } = &node.kind {
        parts.push(format!(
            "keywords:{:?}",
            keywords
                .iter()
                .map(|keyword| (&keyword.name, keyword.selector_payload()))
                .collect::<Vec<_>>()
        ));
    }
    for (_, child) in core_node_child_paths(node, "") {
        parts.push(core_node_digest(child));
    }
    crate::mcp::macro_buffer::source_digest(&parts.join("|"))
}

#[derive(Debug, Clone)]
struct EckyAstNodeAddressability {
    stable_node_key: String,
    source_addressable: bool,
    editable_ops: Vec<EckyAstEditOperation>,
    non_editable_reason: Option<String>,
}

fn binding_label_for_ast_path(path: &str) -> Option<String> {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>();
    if segments.len() == 2 && matches!(segments[0].as_str(), "params" | "parts") {
        return Some(segments[1].clone());
    }
    segments.windows(2).find_map(|window| {
        if matches!(window[0].as_str(), "bindings" | "keywords") {
            Some(window[1].clone())
        } else {
            None
        }
    })
}

fn source_slice_digest(source: &str, span: Option<(usize, usize)>) -> Option<String> {
    let (start, end) = span?;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    Some(crate::mcp::macro_buffer::source_digest(&source[start..end]))
}

fn bounded_ecky_ast_source_slice(source: &str, span: (usize, usize)) -> Option<EckyAstSourceSlice> {
    let (start, end) = span;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }

    let byte_len = end - start;
    let mut text_end = end.min(start + ECKY_AST_SOURCE_MAX_BYTES);
    while text_end > start && !source.is_char_boundary(text_end) {
        text_end -= 1;
    }
    if text_end == start {
        return None;
    }

    Some(EckyAstSourceSlice {
        span: EckyAstSpan {
            start: start as u32,
            end: end as u32,
        },
        text: source[start..text_end].to_string(),
        truncated: text_end < end,
        max_bytes: ECKY_AST_SOURCE_MAX_BYTES,
        byte_len,
    })
}

fn attach_ecky_ast_source_slices(source: &str, nodes: &mut [EckyAstNode]) {
    for node in nodes.iter_mut() {
        if !node.source_addressable {
            continue;
        }
        let Ok(span) = source_span_for_ecky_path(source, &node.path) else {
            continue;
        };
        node.source = bounded_ecky_ast_source_slice(source, span);
    }
}

fn stable_ast_node_key(
    source: &str,
    path: &str,
    kind: &str,
    value_kind: &str,
    op: Option<&str>,
    span: Option<(usize, usize)>,
) -> String {
    let mut parts = vec![
        format!("path={path}"),
        format!("kind={kind}"),
        format!("valueKind={value_kind}"),
    ];
    if let Some(op) = op {
        parts.push(format!("op={op}"));
    }
    if let Some(binding) = binding_label_for_ast_path(path) {
        parts.push(format!("binding={binding}"));
    }
    if let Some(digest) = source_slice_digest(source, span) {
        parts.push(format!("source={digest}"));
    }
    crate::mcp::macro_buffer::source_digest(&parts.join("|"))
}

fn editable_ops_for_source_target_kind(kind: &SourcePathTargetKind) -> Vec<EckyAstEditOperation> {
    match kind {
        SourcePathTargetKind::Root
        | SourcePathTargetKind::BuildResult
        | SourcePathTargetKind::LetBody => vec![EckyAstEditOperation::Replace],
        SourcePathTargetKind::PositionalArg | SourcePathTargetKind::KeywordValue { .. } => vec![
            EckyAstEditOperation::Replace,
            EckyAstEditOperation::InsertBefore,
            EckyAstEditOperation::InsertAfter,
            EckyAstEditOperation::Delete,
        ],
        SourcePathTargetKind::PartClause { .. }
        | SourcePathTargetKind::ParamDecl { .. }
        | SourcePathTargetKind::BuildBinding { .. }
        | SourcePathTargetKind::LetBinding { .. } => vec![
            EckyAstEditOperation::Replace,
            EckyAstEditOperation::InsertBefore,
            EckyAstEditOperation::InsertAfter,
            EckyAstEditOperation::Delete,
            EckyAstEditOperation::Rename,
        ],
    }
}

fn ecky_ast_node_addressability(
    source: &str,
    path: &str,
    kind: &str,
    value_kind: &str,
    op: Option<&str>,
    fallback_span: Option<(usize, usize)>,
) -> EckyAstNodeAddressability {
    let source_target = SourceExprParser::new(source).parse_all().and_then(|exprs| {
        let target = source_target_for_ecky_path(&exprs, source, path)?;
        Ok((
            (target.expr.start, target.expr.end),
            editable_ops_for_source_target_kind(&target.kind),
        ))
    });

    match source_target {
        Ok((source_span, editable_ops)) => EckyAstNodeAddressability {
            stable_node_key: stable_ast_node_key(
                source,
                path,
                kind,
                value_kind,
                op,
                Some(source_span),
            ),
            source_addressable: true,
            editable_ops,
            non_editable_reason: None,
        },
        Err(err) => EckyAstNodeAddressability {
            stable_node_key: stable_ast_node_key(source, path, kind, value_kind, op, fallback_span),
            source_addressable: false,
            editable_ops: Vec::new(),
            non_editable_reason: Some(err.message),
        },
    }
}

fn core_param_digest(param: &crate::ecky_core_ir::CoreParameter) -> String {
    crate::mcp::macro_buffer::source_digest(&format!(
        "param|{}|{}|{:?}|{:?}|{}|{:?}",
        param.key, param.label, param.kind, param.default_value, param.frozen, param.constraints
    ))
}

fn core_part_digest(part: &crate::ecky_core_ir::CorePart) -> String {
    crate::mcp::macro_buffer::source_digest(&format!(
        "part|{}|{}|{}",
        part.key,
        part.label,
        core_node_digest(&part.root)
    ))
}

fn collect_core_part_clause_ast_nodes(
    program: &crate::ecky_core_ir::CoreProgram,
    source: &str,
    requested_path: Option<&str>,
    max_nodes: usize,
    nodes: &mut Vec<EckyAstNode>,
) -> AppResult<bool> {
    let mut truncated = false;
    for part in &program.parts {
        if nodes.len() >= max_nodes {
            return Ok(true);
        }
        let path = format!("/parts/{}", path_segment(&part.key));
        if let Some(requested_path) = requested_path {
            if requested_path != "/" && requested_path != path {
                continue;
            }
        }
        let span = source_span_for_ecky_path(source, &path)
            .ok()
            .map(|(start, end)| EckyAstSpan {
                start: start as u32,
                end: end as u32,
            });
        let addressability = ecky_ast_node_addressability(
            source,
            &path,
            "Part",
            "Part",
            None,
            span.as_ref()
                .map(|span| (span.start as usize, span.end as usize)),
        );
        nodes.push(EckyAstNode {
            path,
            stable_node_key: addressability.stable_node_key,
            digest: core_part_digest(part),
            node_id: 0,
            kind: "Part".to_string(),
            value_kind: "Part".to_string(),
            op: None,
            part_key: Some(part.key.clone()),
            span,
            source_addressable: addressability.source_addressable,
            editable_ops: addressability.editable_ops,
            non_editable_reason: addressability.non_editable_reason,
            source: None,
            child_paths: vec![format!("/parts/{}/root", path_segment(&part.key))],
        });
    }
    if nodes.len() >= max_nodes {
        truncated = true;
    }
    Ok(truncated)
}

fn collect_core_param_ast_nodes(
    program: &crate::ecky_core_ir::CoreProgram,
    source: &str,
    requested_path: Option<&str>,
    max_nodes: usize,
    nodes: &mut Vec<EckyAstNode>,
) -> AppResult<bool> {
    let mut truncated = false;
    for param in &program.parameters {
        if nodes.len() >= max_nodes {
            return Ok(true);
        }
        let path = format!("/params/{}", path_segment(&param.key));
        if let Some(requested_path) = requested_path {
            if requested_path != "/" && requested_path != path {
                continue;
            }
        }
        let span = source_span_for_ecky_path(source, &path)
            .ok()
            .map(|(start, end)| EckyAstSpan {
                start: start as u32,
                end: end as u32,
            });
        let value_kind = format!("{:?}", param.kind);
        let addressability = ecky_ast_node_addressability(
            source,
            &path,
            "Param",
            &value_kind,
            None,
            span.as_ref()
                .map(|span| (span.start as usize, span.end as usize)),
        );
        nodes.push(EckyAstNode {
            path,
            stable_node_key: addressability.stable_node_key,
            digest: core_param_digest(param),
            node_id: 0,
            kind: "Param".to_string(),
            value_kind,
            op: None,
            part_key: None,
            span,
            source_addressable: addressability.source_addressable,
            editable_ops: addressability.editable_ops,
            non_editable_reason: addressability.non_editable_reason,
            source: None,
            child_paths: Vec::new(),
        });
    }
    if nodes.len() >= max_nodes {
        truncated = true;
    }
    Ok(truncated)
}

fn collect_core_ast_nodes(
    source: &str,
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    part_key: Option<&str>,
    depth: usize,
    max_nodes: usize,
    nodes: &mut Vec<EckyAstNode>,
) -> bool {
    if nodes.len() >= max_nodes {
        return true;
    }
    let children = core_node_child_paths(node, path);
    let child_paths = children
        .iter()
        .map(|(child_path, _)| child_path.clone())
        .collect::<Vec<_>>();
    let kind = core_node_kind_label(&node.kind).to_string();
    let value_kind = format!("{:?}", node.value_kind);
    let op = core_node_op_label(node);
    let span = node.span.map(|span| EckyAstSpan {
        start: span.start,
        end: span.end,
    });
    let addressability = ecky_ast_node_addressability(
        source,
        path,
        &kind,
        &value_kind,
        op.as_deref(),
        span.as_ref()
            .map(|span| (span.start as usize, span.end as usize)),
    );
    nodes.push(EckyAstNode {
        path: path.to_string(),
        stable_node_key: addressability.stable_node_key,
        digest: core_node_digest(node),
        node_id: node.id.raw(),
        kind,
        value_kind,
        op,
        part_key: part_key.map(str::to_string),
        span,
        source_addressable: addressability.source_addressable,
        editable_ops: addressability.editable_ops,
        non_editable_reason: addressability.non_editable_reason,
        source: None,
        child_paths,
    });
    if depth == 0 {
        return false;
    }
    for (child_path, child) in children {
        if collect_core_ast_nodes(
            source,
            child,
            &child_path,
            part_key,
            depth - 1,
            max_nodes,
            nodes,
        ) {
            return true;
        }
    }
    false
}

fn collect_matching_core_ast_nodes(
    source: &str,
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    part_key: Option<&str>,
    requested_path: &str,
    depth: usize,
    max_nodes: usize,
    nodes: &mut Vec<EckyAstNode>,
) -> bool {
    if path == requested_path {
        return collect_core_ast_nodes(source, node, path, part_key, depth, max_nodes, nodes);
    }
    for (child_path, child) in core_node_child_paths(node, path) {
        if requested_path.starts_with(&child_path)
            && collect_matching_core_ast_nodes(
                source,
                child,
                &child_path,
                part_key,
                requested_path,
                depth,
                max_nodes,
                nodes,
            )
        {
            return true;
        }
    }
    false
}

fn find_core_ast_node<'a>(
    node: &'a crate::ecky_core_ir::CoreNode,
    path: &str,
    requested_path: &str,
) -> Option<&'a crate::ecky_core_ir::CoreNode> {
    if path == requested_path {
        return Some(node);
    }
    for (child_path, child) in core_node_child_paths(node, path) {
        if requested_path.starts_with(&child_path) {
            if let Some(found) = find_core_ast_node(child, &child_path, requested_path) {
                return Some(found);
            }
        }
    }
    None
}

fn find_core_ast_node_in_program<'a>(
    program: &'a crate::ecky_core_ir::CoreProgram,
    requested_path: &str,
) -> Option<&'a crate::ecky_core_ir::CoreNode> {
    for part in &program.parts {
        let root_path = format!("/parts/{}/root", path_segment(&part.key));
        if requested_path.starts_with(&root_path) {
            if let Some(found) = find_core_ast_node(&part.root, &root_path, requested_path) {
                return Some(found);
            }
        }
    }
    None
}

fn ast_path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>()
}

fn ecky_ast_operation_name(operation: &EckyAstEditOperation) -> &'static str {
    match operation {
        EckyAstEditOperation::Replace => "replace",
        EckyAstEditOperation::InsertBefore => "insertBefore",
        EckyAstEditOperation::InsertAfter => "insertAfter",
        EckyAstEditOperation::Delete => "delete",
        EckyAstEditOperation::Rename => "rename",
    }
}

fn source_line_for_offset(source: &str, offset: usize) -> Option<usize> {
    if offset > source.len() {
        return None;
    }
    Some(
        source.as_bytes()[..offset]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1,
    )
}

fn parse_byte_offset_from_message(message: &str) -> Option<usize> {
    let marker = "byte ";
    let idx = message.find(marker)?;
    let digits = message[idx + marker.len()..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<usize>().ok())
        .flatten()
}

fn source_line_range_for_span(
    source: &str,
    span: crate::ecky_core_ir::SourceSpan,
) -> Option<(usize, usize)> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    let start_line = source_line_for_offset(source, start)?;
    let inclusive_end = end.saturating_sub(1);
    let end_line = source_line_for_offset(source, inclusive_end)?;
    Some((start_line, end_line.max(start_line)))
}

fn compile_error_with_diagnostics(
    message: String,
    source: &str,
    compile_error: crate::ecky_core_ir::CompilerError,
    operation: Option<&str>,
    stable_node_key: Option<&str>,
) -> AppError {
    let mut error = AppError::validation(message);
    if let Some(operation) = operation {
        error = error.with_operation(operation.to_string());
    }
    if let Some(stable_node_key) = stable_node_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        error = error.with_stable_node_key(stable_node_key.to_string());
    }
    if let Some(span) = compile_error.primary_span {
        if let Some((start_line, end_line)) = source_line_range_for_span(source, span) {
            error = error.with_line_range(start_line, end_line);
        }
    } else if let Some(byte_offset) = parse_byte_offset_from_message(&compile_error.message) {
        if let Some(line) = source_line_for_offset(source, byte_offset.min(source.len())) {
            error = error.with_line_range(line, line);
        }
    }
    error
}

fn stable_node_key_for_program_path(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    path: &str,
) -> Option<String> {
    let segments = ast_path_segments(path);
    if segments.len() == 2 && segments[0] == "params" {
        let param = program
            .parameters
            .iter()
            .find(|item| item.key == segments[1])?;
        let span = source_span_for_ecky_path(source, path).ok();
        return Some(stable_ast_node_key(
            source,
            path,
            "Param",
            &format!("{:?}", param.kind),
            None,
            span,
        ));
    }
    if segments.len() == 2 && segments[0] == "parts" {
        let _part = program.parts.iter().find(|item| item.key == segments[1])?;
        let span = source_span_for_ecky_path(source, path).ok();
        return Some(stable_ast_node_key(
            source, path, "Part", "Part", None, span,
        ));
    }
    let node = find_core_ast_node_in_program(program, path)?;
    let fallback_span = node
        .span
        .map(|span| (span.start as usize, span.end as usize));
    let span = source_span_for_ecky_path(source, path)
        .ok()
        .or(fallback_span);
    Some(stable_ast_node_key(
        source,
        path,
        core_node_kind_label(&node.kind),
        &format!("{:?}", node.value_kind),
        core_node_op_label(node).as_deref(),
        span,
    ))
}

fn collect_program_node_paths(
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    paths: &mut Vec<String>,
) {
    paths.push(path.to_string());
    for (child_path, child) in core_node_child_paths(node, path) {
        collect_program_node_paths(child, &child_path, paths);
    }
}

fn all_program_ast_paths(program: &crate::ecky_core_ir::CoreProgram) -> Vec<String> {
    let mut paths = Vec::new();
    for param in &program.parameters {
        paths.push(format!("/params/{}", path_segment(&param.key)));
    }
    for part in &program.parts {
        let part_path = format!("/parts/{}", path_segment(&part.key));
        paths.push(part_path.clone());
        let root_path = format!("{part_path}/root");
        collect_program_node_paths(&part.root, &root_path, &mut paths);
    }
    paths
}

fn resolve_path_from_stable_node_key(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    stable_node_key: &str,
    tool_name: &str,
) -> AppResult<String> {
    let trimmed_key = stable_node_key.trim();
    if trimmed_key.is_empty() {
        return Err(AppError::validation(format!(
            "{tool_name} stableNodeKey must not be empty."
        )));
    }
    for path in all_program_ast_paths(program) {
        let Some(candidate_key) = stable_node_key_for_program_path(source, program, &path) else {
            continue;
        };
        if candidate_key == trimmed_key {
            return Ok(path);
        }
    }
    Err(AppError::validation(format!(
        "{tool_name} stableNodeKey not found in AST: {trimmed_key}."
    )))
}

fn resolve_ecky_ast_patch_path(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    path: Option<&str>,
    stable_node_key: Option<&str>,
    tool_name: &str,
) -> AppResult<String> {
    let explicit_path = path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let stable_node_key = stable_node_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let resolved_from_key = stable_node_key
        .as_deref()
        .map(|key| resolve_path_from_stable_node_key(source, program, key, tool_name))
        .transpose()?;

    match (explicit_path, resolved_from_key) {
        (None, None) => Err(AppError::validation(format!(
            "{tool_name} requires stableNodeKey or path."
        ))),
        (Some(path), None) => Ok(path),
        (None, Some(path)) => Ok(path),
        (Some(path), Some(resolved)) => {
            if path == resolved {
                Ok(path)
            } else {
                Err(AppError::validation(format!(
                    "{tool_name} stableNodeKey/path mismatch: stableNodeKey resolves to {resolved}, path is {path}."
                )))
            }
        }
    }
}

fn affected_node_keys_for_patch(
    old_source: &str,
    old_program: &crate::ecky_core_ir::CoreProgram,
    old_path: &str,
    new_source: &str,
    new_program: &crate::ecky_core_ir::CoreProgram,
    new_path: &str,
) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(key) = stable_node_key_for_program_path(old_source, old_program, old_path) {
        keys.push(key);
    }
    if !new_path.trim().is_empty() {
        if let Some(key) = stable_node_key_for_program_path(new_source, new_program, new_path) {
            if !keys.iter().any(|existing| existing == &key) {
                keys.push(key);
            }
        }
    }
    keys
}

fn dependency_impact_for_patch(
    program: &crate::ecky_core_ir::CoreProgram,
    edited_path: &str,
    affected_paths: &[String],
) -> EckyAstPatchDependencyImpactSummary {
    let summary_path = edited_path.trim();
    let segments = ast_path_segments(summary_path);
    if segments.len() == 2 && segments[0] == "params" {
        if let Ok(param_id) = param_id_for_dependency_key(program, &segments[1]) {
            let dependent_source_paths = dependent_source_paths_for_param(program, param_id);
            let reference_count = dependent_source_paths.len();
            let impacted_part_ids = impacted_part_ids_for_dependency_paths(&dependent_source_paths);
            let impact_labels = impact_labels_for_dependency(&impacted_part_ids, reference_count);
            return EckyAstPatchDependencyImpactSummary {
                path: format!("/params/{}", path_segment(&segments[1])),
                dependency_kind: "parameterReference".to_string(),
                dependent_source_paths,
                impacted_part_ids,
                impact_labels,
                reference_count,
            };
        }
    }

    let mut dependent_source_paths = Vec::new();
    for path in affected_paths {
        if path.trim().is_empty() {
            continue;
        }
        if !dependent_source_paths
            .iter()
            .any(|existing| existing == path)
        {
            dependent_source_paths.push(path.clone());
        }
    }
    let reference_count = dependent_source_paths.len();
    let impacted_part_ids = impacted_part_ids_for_dependency_paths(&dependent_source_paths);
    let impact_labels = impact_labels_for_dependency(&impacted_part_ids, reference_count);
    EckyAstPatchDependencyImpactSummary {
        path: summary_path.to_string(),
        dependency_kind: "pathLocal".to_string(),
        dependent_source_paths,
        impacted_part_ids,
        impact_labels,
        reference_count,
    }
}

enum EckyDependencyQuery {
    ParameterKey(String),
    SelectionTargetId(String),
}

fn parse_ecky_dependency_path(path: &str) -> AppResult<EckyDependencyQuery> {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>();
    if segments.len() != 2 || segments[1].is_empty() {
        return Err(AppError::validation(format!(
            "ecky_dependency_get supports /params/{{key}} and /targets/{{targetId}} paths. Unsupported path: {path}."
        )));
    }

    match segments[0].as_str() {
        "params" => Ok(EckyDependencyQuery::ParameterKey(segments[1].clone())),
        "targets" => Ok(EckyDependencyQuery::SelectionTargetId(segments[1].clone())),
        _ => Err(AppError::validation(format!(
            "ecky_dependency_get supports /params/{{key}} and /targets/{{targetId}} paths. Unsupported path: {path}."
        ))),
    }
}

fn param_id_for_dependency_key(
    program: &crate::ecky_core_ir::CoreProgram,
    key: &str,
) -> AppResult<crate::ecky_core_ir::ParamId> {
    program
        .parameters
        .iter()
        .find(|param| param.key == key)
        .map(|param| param.id)
        .ok_or_else(|| {
            AppError::validation(format!(
                "Ecky dependency source path not found: /params/{}.",
                key
            ))
        })
}

fn selection_targets_by_id<'a>(
    manifest: &'a ModelManifest,
    requested_id: &str,
) -> Vec<&'a crate::models::SelectionTarget> {
    manifest
        .selection_targets
        .iter()
        .filter(|target| {
            selection_target_match_ids(target)
                .iter()
                .any(|id| id == requested_id)
        })
        .collect()
}

fn selection_target_by_id<'a>(
    manifest: &'a ModelManifest,
    requested_id: &str,
) -> Option<&'a crate::models::SelectionTarget> {
    selection_targets_by_id(manifest, requested_id)
        .into_iter()
        .next()
}

fn feature_bindings_for_target_ids(
    manifest: &ModelManifest,
    target_ids: &[String],
) -> (Vec<String>, Vec<String>) {
    let Some(graph) = manifest.feature_graph.as_ref() else {
        return (Vec::new(), Vec::new());
    };

    let mut feature_ids = Vec::new();
    let mut source_paths = Vec::new();
    for node in &graph.nodes {
        let output_match = node.output_refs.iter().any(|output| {
            output
                .target_ids
                .iter()
                .any(|target_id| target_ids.iter().any(|requested| requested == target_id))
        });
        let port_match = node.ports.iter().any(|port| {
            port.target_ids
                .iter()
                .any(|target_id| target_ids.iter().any(|requested| requested == target_id))
        });
        if !output_match && !port_match {
            continue;
        }

        if !feature_ids
            .iter()
            .any(|existing| existing == &node.feature_id)
        {
            feature_ids.push(node.feature_id.clone());
        }
        if let Some(path) = node
            .source_ref
            .as_ref()
            .and_then(|source_ref| source_ref.path.clone())
        {
            if !path.trim().is_empty() && !source_paths.iter().any(|existing| existing == &path) {
                source_paths.push(path);
            }
        }
        for port in &node.ports {
            let port_hit = port
                .target_ids
                .iter()
                .any(|target_id| target_ids.iter().any(|requested| requested == target_id));
            if !port_hit {
                continue;
            }
            if let Some(path) = port
                .source_ref
                .as_ref()
                .and_then(|source_ref| source_ref.path.clone())
            {
                if !path.trim().is_empty() && !source_paths.iter().any(|existing| existing == &path)
                {
                    source_paths.push(path);
                }
            }
        }
    }

    (feature_ids, source_paths)
}

fn selection_target_kind_role(kind: &crate::models::SelectionTargetKind) -> String {
    match kind {
        crate::models::SelectionTargetKind::Part => "part".to_string(),
        crate::models::SelectionTargetKind::Object => "object".to_string(),
        crate::models::SelectionTargetKind::Group => "group".to_string(),
        crate::models::SelectionTargetKind::Edge => "edge".to_string(),
        crate::models::SelectionTargetKind::Face => "face".to_string(),
    }
}

fn collect_selector_provenance_candidates(
    manifest: &ModelManifest,
    selected_targets: &[&crate::models::SelectionTarget],
    source: Option<&str>,
) -> EckySelectorResolveProvenanceCandidates {
    let mut source_paths = Vec::new();
    let mut operation_kinds = Vec::new();
    let mut primitive_ids = Vec::new();
    let mut feature_roles = Vec::new();

    for target in selected_targets {
        push_unique_strings(&mut primitive_ids, &target.primitive_ids);

        let feature_role = selection_target_kind_role(&target.kind);
        if !feature_roles
            .iter()
            .any(|existing| existing == &feature_role)
        {
            feature_roles.push(feature_role);
        }

        let target_ids = selection_target_match_ids(target);
        let Some(graph) = manifest.feature_graph.as_ref() else {
            continue;
        };
        for node in &graph.nodes {
            let output_match = node.output_refs.iter().any(|output| {
                output
                    .target_ids
                    .iter()
                    .any(|target_id| target_ids.iter().any(|requested| requested == target_id))
            });
            let port_match = node.ports.iter().any(|port| {
                port.target_ids
                    .iter()
                    .any(|target_id| target_ids.iter().any(|requested| requested == target_id))
            });
            if !output_match && !port_match {
                continue;
            }

            if !node.kind.trim().is_empty()
                && !operation_kinds
                    .iter()
                    .any(|existing| existing == &node.kind)
            {
                operation_kinds.push(node.kind.clone());
            }

            if let Some(path) = node
                .source_ref
                .as_ref()
                .and_then(|source_ref| source_ref.path.clone())
                .filter(|path| !path.trim().is_empty())
            {
                if !source_paths.iter().any(|existing| existing == &path) {
                    source_paths.push(path);
                }
            }

            for port in &node.ports {
                let port_hit = port
                    .target_ids
                    .iter()
                    .any(|target_id| target_ids.iter().any(|requested| requested == target_id));
                if !port_hit {
                    continue;
                }
                if let Some(path) = port
                    .source_ref
                    .as_ref()
                    .and_then(|source_ref| source_ref.path.clone())
                    .filter(|path| !path.trim().is_empty())
                {
                    if !source_paths.iter().any(|existing| existing == &path) {
                        source_paths.push(path);
                    }
                }
            }
        }
    }

    let mut source_stable_node_keys = Vec::new();
    if let Some(source_text) = source {
        if let Ok(program) = crate::ecky_scheme::compile_to_core_program(source_text) {
            let mut seen = HashSet::new();
            for path in source_paths {
                if let Some(stable_key) =
                    stable_node_key_for_program_path(source_text, &program, &path)
                {
                    let trimmed = stable_key.trim();
                    if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                        source_stable_node_keys.push(trimmed.to_string());
                    }
                }
            }
        }
    }

    EckySelectorResolveProvenanceCandidates {
        feature_role: if feature_roles.len() == 1 {
            feature_roles.into_iter().next()
        } else {
            None
        },
        source_stable_node_keys,
        operation_kinds,
        primitive_ids,
    }
}

fn collect_param_reference_paths(
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    param_id: crate::ecky_core_ir::ParamId,
    paths: &mut Vec<String>,
) {
    if matches!(
        &node.kind,
        crate::ecky_core_ir::CoreNodeKind::Reference(
            crate::ecky_core_ir::CoreReference::Parameter(id)
        ) if *id == param_id
    ) {
        paths.push(path.to_string());
    }
    for (child_path, child) in core_node_child_paths(node, path) {
        collect_param_reference_paths(child, &child_path, param_id, paths);
    }
}

fn dependent_source_paths_for_param(
    program: &crate::ecky_core_ir::CoreProgram,
    param_id: crate::ecky_core_ir::ParamId,
) -> Vec<String> {
    let mut paths = Vec::new();
    for part in &program.parts {
        let root_path = format!("/parts/{}/root", path_segment(&part.key));
        collect_param_reference_paths(&part.root, &root_path, param_id, &mut paths);
    }
    paths
}

fn impacted_part_ids_for_dependency_paths(paths: &[String]) -> Vec<String> {
    let mut ids = Vec::new();
    for path in paths {
        let segments = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.len() >= 2 && segments[0] == "parts" {
            let part_id = path_segment_decode(segments[1]);
            if !ids.iter().any(|existing| existing == &part_id) {
                ids.push(part_id);
            }
        }
    }
    ids
}

fn impact_labels_for_dependency(
    impacted_part_ids: &[String],
    reference_count: usize,
) -> Vec<String> {
    if reference_count == 0 {
        return vec!["local".to_string()];
    }
    if impacted_part_ids.is_empty() {
        return vec!["local".to_string()];
    }
    if impacted_part_ids.len() == 1 {
        return vec!["part-local".to_string(), "export-affecting".to_string()];
    }
    vec!["assembly-wide".to_string(), "export-affecting".to_string()]
}

const SHAPE_GRAPH_SECTION_MAX_ITEMS: usize = 64;

fn shape_graph_section_enabled(
    filters: &[ShapeGraphFilterSection],
    section: ShapeGraphFilterSection,
) -> bool {
    filters.is_empty() || filters.iter().any(|candidate| *candidate == section)
}

fn shape_graph_payload<T>(items: Vec<T>) -> ShapeGraphSectionPayload<T> {
    let truncated = items.len() > SHAPE_GRAPH_SECTION_MAX_ITEMS;
    ShapeGraphSectionPayload {
        truncated,
        items: items
            .into_iter()
            .take(SHAPE_GRAPH_SECTION_MAX_ITEMS)
            .collect(),
    }
}

fn relation_operand_label(
    program: &crate::ecky_core_ir::CoreProgram,
    operand: &crate::ecky_core_ir::CoreRelationOperand,
) -> String {
    match operand {
        crate::ecky_core_ir::CoreRelationOperand::Number(value) => {
            if value.fract() == 0.0 {
                format!("{}", *value as i64)
            } else {
                value.to_string()
            }
        }
        crate::ecky_core_ir::CoreRelationOperand::Parameter(param_id) => program
            .parameters
            .iter()
            .find(|param| param.id == *param_id)
            .map(|param| param.key.clone())
            .unwrap_or_else(|| format!("param#{}", param_id.raw())),
    }
}

fn collect_relation_param_keys(
    program: &crate::ecky_core_ir::CoreProgram,
    relation: &crate::ecky_core_ir::CoreRelationConstraint,
) -> Vec<String> {
    let mut keys = Vec::new();
    for operand in [&relation.left, &relation.right] {
        if let crate::ecky_core_ir::CoreRelationOperand::Parameter(param_id) = operand {
            if let Some(param_key) = program
                .parameters
                .iter()
                .find(|param| param.id == *param_id)
                .map(|param| param.key.clone())
            {
                if !keys.iter().any(|existing| existing == &param_key) {
                    keys.push(param_key);
                }
            }
        }
    }
    keys
}

fn build_shape_graph_packet(
    design_output: &DesignOutput,
    model_manifest: Option<&ModelManifest>,
    artifact_bundle: Option<&ArtifactBundle>,
    filters: &[ShapeGraphFilterSection],
) -> AppResult<ShapeGraphPacket> {
    let source = design_output.macro_code.as_str();
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let program = if design_output.source_language == crate::models::SourceLanguage::EckyIrV0 {
        Some(
            crate::ecky_scheme::compile_to_core_program(source).map_err(|err| {
                compile_error_with_diagnostics(
                    format!("Failed to compile Ecky source for shapeGraph: {err}"),
                    source,
                    err,
                    None,
                    None,
                )
            })?,
        )
    } else {
        None
    };

    let mut core_fingerprint = Vec::new();
    let mut editable_stable_node_keys = Vec::new();
    if let Some(program) = program.as_ref() {
        for param in &program.parameters {
            core_fingerprint.push(format!("param:{}", param.key));
            let path = format!("/params/{}", path_segment(&param.key));
            if let Some(stable_key) = stable_node_key_for_program_path(source, program, &path) {
                if !stable_key.trim().is_empty()
                    && !editable_stable_node_keys
                        .iter()
                        .any(|existing| existing == &stable_key)
                {
                    editable_stable_node_keys.push(stable_key);
                }
            }
        }
        for part in &program.parts {
            core_fingerprint.push(format!("part:{}", part.key));
            let path = format!("/parts/{}", path_segment(&part.key));
            if let Some(stable_key) = stable_node_key_for_program_path(source, program, &path) {
                if !stable_key.trim().is_empty()
                    && !editable_stable_node_keys
                        .iter()
                        .any(|existing| existing == &stable_key)
                {
                    editable_stable_node_keys.push(stable_key);
                }
            }
        }
        for relation in &program.constraints.relations {
            core_fingerprint.push(format!(
                "relation:{}:{}:{}",
                relation_operand_label(program, &relation.left),
                relation.operator.as_str(),
                relation_operand_label(program, &relation.right)
            ));
        }
    }
    let core_digest = crate::mcp::macro_buffer::source_digest(&core_fingerprint.join("|"));

    let parts = shape_graph_section_enabled(filters, ShapeGraphFilterSection::Parts).then(|| {
        let mut section_items = Vec::new();
        if let Some(manifest) = model_manifest {
            for part in &manifest.parts {
                let stable_node_key = program.as_ref().and_then(|program| {
                    let path = format!("/parts/{}", path_segment(&part.part_id));
                    stable_node_key_for_program_path(source, program, &path)
                });
                section_items.push(ShapeGraphPart {
                    part_id: part.part_id.clone(),
                    label: part.label.clone(),
                    kind: part.kind.clone(),
                    editable: part.editable,
                    stable_node_key,
                });
            }
        } else if let Some(program) = program.as_ref() {
            for part in &program.parts {
                let path = format!("/parts/{}", path_segment(&part.key));
                section_items.push(ShapeGraphPart {
                    part_id: part.key.clone(),
                    label: part.label.clone(),
                    kind: "solid".to_string(),
                    editable: true,
                    stable_node_key: stable_node_key_for_program_path(source, program, &path),
                });
            }
        }
        shape_graph_payload(section_items)
    });

    let instances =
        shape_graph_section_enabled(filters, ShapeGraphFilterSection::Instances).then(|| {
            let mut section_items = Vec::new();
            if let Some(graph) = model_manifest.and_then(|manifest| manifest.feature_graph.as_ref())
            {
                for node in &graph.nodes {
                    let node_kind = node.kind.to_ascii_lowercase();
                    if !(node_kind.contains("repeat") || node_kind.contains("instance")) {
                        continue;
                    }
                    let target_ids = node
                        .output_refs
                        .iter()
                        .flat_map(|output| output.target_ids.iter().cloned())
                        .collect::<Vec<_>>();
                    section_items.push(ShapeGraphInstance {
                        instance_id: node.feature_id.clone(),
                        prototype_feature_id: node.dependency_ids.first().cloned(),
                        dependency_ids: node.dependency_ids.clone(),
                        target_ids,
                    });
                }
            }
            shape_graph_payload(section_items)
        });

    let constraints = shape_graph_section_enabled(filters, ShapeGraphFilterSection::Constraints)
        .then(|| {
            let mut section_items = Vec::new();
            if let Some(program) = program.as_ref() {
                for (index, relation) in program.constraints.relations.iter().enumerate() {
                    let path = format!("/params/:relations/{index}");
                    let depends_on_param_keys = collect_relation_param_keys(program, relation);
                    let mut affects_stable_node_keys = Vec::new();
                    for param_key in &depends_on_param_keys {
                        let Some(param_id) = program
                            .parameters
                            .iter()
                            .find(|param| param.key == *param_key)
                            .map(|param| param.id)
                        else {
                            continue;
                        };
                        for source_path in dependent_source_paths_for_param(program, param_id) {
                            let Some(stable_key) =
                                stable_node_key_for_program_path(source, program, &source_path)
                            else {
                                continue;
                            };
                            if stable_key.trim().is_empty()
                                || affects_stable_node_keys
                                    .iter()
                                    .any(|existing| existing == &stable_key)
                            {
                                continue;
                            }
                            affects_stable_node_keys.push(stable_key);
                        }
                    }
                    section_items.push(ShapeGraphConstraint {
                        constraint_id: format!("relation:{index}"),
                        label: format!(
                            "{} {} {}",
                            relation_operand_label(program, &relation.left),
                            relation.operator.as_str(),
                            relation_operand_label(program, &relation.right)
                        ),
                        kind: "relation".to_string(),
                        depends_on_param_keys,
                        affects_stable_node_keys,
                        source_stable_node_key: stable_node_key_for_program_path(
                            source, program, &path,
                        ),
                    });
                }
            }
            shape_graph_payload(section_items)
        });

    let debug_overlays = shape_graph_section_enabled(filters, ShapeGraphFilterSection::Debug)
        .then(|| shape_graph_payload(Vec::<ShapeGraphDebugOverlay>::new()));

    let dependencies = shape_graph_section_enabled(filters, ShapeGraphFilterSection::Dependencies)
        .then(|| {
            let mut section_items = Vec::new();
            if let Some(program) = program.as_ref() {
                for param in &program.parameters {
                    let dependent_source_paths =
                        dependent_source_paths_for_param(program, param.id);
                    section_items.push(ShapeGraphDependency {
                        parameter_key: param.key.clone(),
                        impacted_part_ids: impacted_part_ids_for_dependency_paths(
                            &dependent_source_paths,
                        ),
                        dependent_source_paths,
                    });
                }
            }
            shape_graph_payload(section_items)
        });

    let topology_target_counts = if let Some(bundle) = artifact_bundle {
        ShapeGraphTopologyCounts {
            edge_target_count: bundle.edge_targets.len(),
            face_target_count: bundle.face_targets.len(),
        }
    } else {
        ShapeGraphTopologyCounts {
            edge_target_count: 0,
            face_target_count: 0,
        }
    };

    Ok(ShapeGraphPacket {
        source_digest,
        core_digest,
        artifact_digest: artifact_bundle.map(artifact_bundle_digest),
        editable_stable_node_keys,
        topology_target_counts,
        parts,
        instances,
        constraints,
        debug_overlays,
        dependencies,
    })
}

fn param_value_from_core(value: &crate::ecky_core_ir::CoreParameterValue) -> ParamValue {
    match value {
        crate::ecky_core_ir::CoreParameterValue::Number(value) => ParamValue::Number(*value),
        crate::ecky_core_ir::CoreParameterValue::Boolean(value) => ParamValue::Boolean(*value),
        crate::ecky_core_ir::CoreParameterValue::Text(value)
        | crate::ecky_core_ir::CoreParameterValue::Choice(value)
        | crate::ecky_core_ir::CoreParameterValue::Image(value) => {
            ParamValue::String(value.clone())
        }
    }
}

fn param_value_matches_core_choice(
    value: &ParamValue,
    choice: &crate::ecky_core_ir::CoreParameterValue,
) -> bool {
    match (value, choice) {
        (ParamValue::Number(left), crate::ecky_core_ir::CoreParameterValue::Number(right)) => {
            left == right
        }
        (ParamValue::String(left), crate::ecky_core_ir::CoreParameterValue::Choice(right))
        | (ParamValue::String(left), crate::ecky_core_ir::CoreParameterValue::Text(right))
        | (ParamValue::String(left), crate::ecky_core_ir::CoreParameterValue::Image(right)) => {
            left == right
        }
        _ => false,
    }
}

fn effective_ecky_constraint_params(
    program: &crate::ecky_core_ir::CoreProgram,
    design_params: &DesignParams,
    provided_params: Option<DesignParams>,
) -> (DesignParams, String) {
    let mut params = DesignParams::new();
    for param in &program.parameters {
        params.insert(
            param.key.clone(),
            param_value_from_core(&param.default_value),
        );
    }

    match provided_params {
        Some(provided) => {
            for (key, value) in provided {
                params.insert(key, value);
            }
            (params, "provided".to_string())
        }
        None => {
            for (key, value) in design_params {
                params.insert(key.clone(), value.clone());
            }
            (params, "initialOrDefault".to_string())
        }
    }
}

fn validate_ecky_constraint_row(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    param: &crate::ecky_core_ir::CoreParameter,
    value: &ParamValue,
) -> EckyConstraintValidationRow {
    let mut failures = Vec::new();
    let number_value = match (&param.kind, value) {
        (crate::ecky_core_ir::CoreParameterKind::Number, ParamValue::Number(value)) => Some(*value),
        (crate::ecky_core_ir::CoreParameterKind::Number, other) => {
            failures.push(format!("Expected number, got {}.", other.kind()));
            None
        }
        (crate::ecky_core_ir::CoreParameterKind::Boolean, ParamValue::Boolean(_)) => None,
        (crate::ecky_core_ir::CoreParameterKind::Boolean, other) => {
            failures.push(format!("Expected boolean, got {}.", other.kind()));
            None
        }
        (crate::ecky_core_ir::CoreParameterKind::Choice, ParamValue::String(_))
        | (crate::ecky_core_ir::CoreParameterKind::Choice, ParamValue::Number(_))
        | (crate::ecky_core_ir::CoreParameterKind::Text, ParamValue::String(_))
        | (crate::ecky_core_ir::CoreParameterKind::Image, ParamValue::String(_)) => None,
        (
            crate::ecky_core_ir::CoreParameterKind::Choice,
            other @ (ParamValue::Boolean(_) | ParamValue::Null),
        ) => {
            failures.push(format!("Expected choice value, got {}.", other.kind()));
            None
        }
        (
            crate::ecky_core_ir::CoreParameterKind::Text
            | crate::ecky_core_ir::CoreParameterKind::Image,
            other,
        ) => {
            failures.push(format!("Expected string, got {}.", other.kind()));
            None
        }
    };

    if let Some(value) = number_value {
        if let Some(min) = param.constraints.min {
            if value < min {
                failures.push(format!("Value {value} is below min {min}."));
            }
        }
        if let Some(max) = param.constraints.max {
            if value > max {
                failures.push(format!("Value {value} is above max {max}."));
            }
        }
        if let Some(step) = param.constraints.step {
            if !step.is_finite() || step <= 0.0 {
                failures.push(format!("Step constraint {step} is not positive."));
            } else {
                let base = param.constraints.min.unwrap_or(0.0);
                let units = (value - base) / step;
                let nearest = units.round();
                let tolerance = 1e-9_f64.max(units.abs() * 1e-9);
                if (units - nearest).abs() > tolerance {
                    failures.push(format!(
                        "Value {value} does not align to step {step} from base {base}."
                    ));
                }
            }
        }
    }

    if !param.constraints.choices.is_empty()
        && !param
            .constraints
            .choices
            .iter()
            .any(|choice| param_value_matches_core_choice(value, &choice.value))
    {
        let choices = param
            .constraints
            .choices
            .iter()
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        failures.push(format!("Value is not one of allowed choices: {choices}."));
    }

    let path = format!("/params/{}", path_segment(&param.key));
    let involved_param_keys = vec![param.key.clone()];
    let source_stable_node_keys = stable_node_key_for_program_path(source, program, &path)
        .into_iter()
        .collect::<Vec<_>>();
    let source_stable_node_key = source_stable_node_keys.first().cloned();
    let mut affects_stable_node_keys = source_stable_node_keys.clone();
    for dependency_path in dependent_source_paths_for_param(program, param.id) {
        if let Some(stable_key) =
            stable_node_key_for_program_path(source, program, &dependency_path)
        {
            if !affects_stable_node_keys
                .iter()
                .any(|existing| existing == &stable_key)
            {
                affects_stable_node_keys.push(stable_key);
            }
        }
    }
    let raw_value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    if failures.is_empty() {
        EckyConstraintValidationRow {
            path,
            status: "pass".to_string(),
            severity: "info".to_string(),
            raw_value,
            message: "OK.".to_string(),
            constraint_id: Some(format!("param_{}", param.key)),
            label: Some(format!("Parameter `{}` constraints", param.label)),
            kind: Some("parameter".to_string()),
            source_stable_node_key,
            depends_on_param_keys: involved_param_keys.clone(),
            affects_stable_node_keys,
            involved_param_keys,
            source_stable_node_keys,
        }
    } else {
        EckyConstraintValidationRow {
            path,
            status: "fail".to_string(),
            severity: "error".to_string(),
            raw_value,
            message: failures.join(" "),
            constraint_id: Some(format!("param_{}", param.key)),
            label: Some(format!("Parameter `{}` constraints", param.label)),
            kind: Some("parameter".to_string()),
            source_stable_node_key,
            depends_on_param_keys: involved_param_keys.clone(),
            affects_stable_node_keys,
            involved_param_keys,
            source_stable_node_keys,
        }
    }
}

fn validate_ecky_constraints(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    params: &DesignParams,
) -> Vec<EckyConstraintValidationRow> {
    let mut rows = program
        .parameters
        .iter()
        .map(|param| {
            let value = params
                .get(&param.key)
                .cloned()
                .unwrap_or_else(|| param_value_from_core(&param.default_value));
            validate_ecky_constraint_row(source, program, param, &value)
        })
        .collect::<Vec<_>>();
    rows.extend(
        program
            .constraints
            .relations
            .iter()
            .enumerate()
            .map(|(index, relation)| {
                validate_ecky_relation_constraint_row(source, program, params, relation, index)
            }),
    );
    rows
}

fn evaluate_relation_operand(
    program: &crate::ecky_core_ir::CoreProgram,
    params: &DesignParams,
    operand: &crate::ecky_core_ir::CoreRelationOperand,
) -> Result<(f64, Option<String>), String> {
    match operand {
        crate::ecky_core_ir::CoreRelationOperand::Number(value) => Ok((*value, None)),
        crate::ecky_core_ir::CoreRelationOperand::Parameter(param_id) => {
            let param = program
                .parameters
                .iter()
                .find(|candidate| candidate.id == *param_id)
                .ok_or_else(|| {
                    format!(
                        "Relation references unknown parameter id {}.",
                        param_id.raw()
                    )
                })?;
            let value = params
                .get(&param.key)
                .cloned()
                .unwrap_or_else(|| param_value_from_core(&param.default_value));
            match value {
                ParamValue::Number(number) => Ok((number, Some(param.key.clone()))),
                other => Err(format!(
                    "Relation operand `{}` expected number, got {}.",
                    param.key,
                    other.kind()
                )),
            }
        }
    }
}

fn validate_ecky_relation_constraint_row(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    params: &DesignParams,
    relation: &crate::ecky_core_ir::CoreRelationConstraint,
    index: usize,
) -> EckyConstraintValidationRow {
    let mut failures = Vec::new();
    let mut involved_param_keys = Vec::new();
    let mut depends_on_param_keys = Vec::new();
    for operand in [&relation.left, &relation.right] {
        let crate::ecky_core_ir::CoreRelationOperand::Parameter(param_id) = operand else {
            continue;
        };
        let Some(param_key) = program
            .parameters
            .iter()
            .find(|param| param.id == *param_id)
            .map(|param| param.key.clone())
        else {
            continue;
        };
        if !depends_on_param_keys
            .iter()
            .any(|candidate| candidate == &param_key)
        {
            depends_on_param_keys.push(param_key);
        }
    }

    let left = evaluate_relation_operand(program, params, &relation.left).map_err(|err| {
        failures.push(err);
    });
    let right = evaluate_relation_operand(program, params, &relation.right).map_err(|err| {
        failures.push(err);
    });

    let left_value = left.ok().map(|(value, key)| {
        if let Some(key) = key {
            if !involved_param_keys
                .iter()
                .any(|candidate| candidate == &key)
            {
                involved_param_keys.push(key);
            }
        }
        value
    });
    let right_value = right.ok().map(|(value, key)| {
        if let Some(key) = key {
            if !involved_param_keys
                .iter()
                .any(|candidate| candidate == &key)
            {
                involved_param_keys.push(key);
            }
        }
        value
    });

    if let (Some(left), Some(right)) = (left_value, right_value) {
        let relation_ok = match relation.operator {
            crate::ecky_core_ir::CoreRelationOperator::LessThan => left < right,
            crate::ecky_core_ir::CoreRelationOperator::LessThanOrEqual => left <= right,
            crate::ecky_core_ir::CoreRelationOperator::GreaterThan => left > right,
            crate::ecky_core_ir::CoreRelationOperator::GreaterThanOrEqual => left >= right,
        };
        if !relation_ok {
            failures.push(format!(
                "Relation {} failed: {} !{} {}.",
                relation.operator.as_str(),
                left,
                relation.operator.as_str(),
                right
            ));
        }
    }

    let path = format!("/params/:relations/{index}");
    let mut source_stable_node_keys = stable_node_key_for_program_path(source, program, &path)
        .into_iter()
        .collect::<Vec<_>>();
    let source_stable_node_key = source_stable_node_keys.first().cloned().or_else(|| {
        Some(stable_ast_node_key(
            source,
            &path,
            "RelationConstraint",
            "RelationConstraint",
            Some(relation.operator.as_str()),
            None,
        ))
    });
    if let Some(stable_key) = source_stable_node_key.clone() {
        if !source_stable_node_keys
            .iter()
            .any(|existing| existing == &stable_key)
        {
            source_stable_node_keys.push(stable_key);
        }
    }
    let mut affects_stable_node_keys = source_stable_node_keys.clone();
    for key in &depends_on_param_keys {
        let param_path = format!("/params/{}", path_segment(key));
        if let Some(stable_key) = stable_node_key_for_program_path(source, program, &param_path) {
            if !affects_stable_node_keys
                .iter()
                .any(|existing| existing == &stable_key)
            {
                affects_stable_node_keys.push(stable_key);
            }
        }
    }
    let raw_value = serde_json::json!({
        "operator": relation.operator.as_str(),
        "left": left_value,
        "right": right_value,
    });

    if failures.is_empty() {
        EckyConstraintValidationRow {
            path,
            status: "pass".to_string(),
            severity: "info".to_string(),
            raw_value,
            message: "OK.".to_string(),
            constraint_id: Some(format!("relation_{index}")),
            label: Some(format!("Relation #{}", index + 1)),
            kind: Some("relation".to_string()),
            source_stable_node_key,
            depends_on_param_keys: depends_on_param_keys.clone(),
            affects_stable_node_keys: affects_stable_node_keys.clone(),
            involved_param_keys,
            source_stable_node_keys,
        }
    } else {
        EckyConstraintValidationRow {
            path,
            status: "fail".to_string(),
            severity: "error".to_string(),
            raw_value,
            message: failures.join(" "),
            constraint_id: Some(format!("relation_{index}")),
            label: Some(format!("Relation #{}", index + 1)),
            kind: Some("relation".to_string()),
            source_stable_node_key,
            depends_on_param_keys,
            affects_stable_node_keys,
            involved_param_keys,
            source_stable_node_keys,
        }
    }
}

#[derive(Debug, Clone)]
struct AnonymousDeltaUse {
    part_key: String,
    param_key: String,
    delta: f64,
    path: String,
}

fn collect_anonymous_delta_uses(
    node: &crate::ecky_core_ir::CoreNode,
    path: &str,
    part_key: &str,
    program: &crate::ecky_core_ir::CoreProgram,
    out: &mut Vec<AnonymousDeltaUse>,
) {
    let mut maybe_record_use = |param_id: crate::ecky_core_ir::ParamId, delta: f64| {
        let Some(param_key) = program
            .parameters
            .iter()
            .find(|param| param.id == param_id)
            .map(|param| param.key.clone())
        else {
            return;
        };
        out.push(AnonymousDeltaUse {
            part_key: part_key.to_string(),
            param_key,
            delta,
            path: path.to_string(),
        });
    };

    if let crate::ecky_core_ir::CoreNodeKind::Call { op, args, .. } = &node.kind {
        if let crate::ecky_core_ir::CoreOperation::Custom(op_name) = op {
            let param_id_from_node =
                |candidate: &crate::ecky_core_ir::CoreNode| match &candidate.kind {
                    crate::ecky_core_ir::CoreNodeKind::Reference(
                        crate::ecky_core_ir::CoreReference::Parameter(param_id),
                    ) => Some(*param_id),
                    _ => None,
                };
            let number_from_node = |candidate: &crate::ecky_core_ir::CoreNode| match &candidate.kind
            {
                crate::ecky_core_ir::CoreNodeKind::Literal(
                    crate::ecky_core_ir::CoreLiteral::Number(value),
                ) => Some(*value),
                _ => None,
            };

            if op_name == "+" && args.len() == 2 {
                match (
                    param_id_from_node(&args[0]),
                    number_from_node(&args[1]),
                    number_from_node(&args[0]),
                    param_id_from_node(&args[1]),
                ) {
                    (Some(param_id), Some(delta), _, _) => maybe_record_use(param_id, delta),
                    (_, _, Some(delta), Some(param_id)) => maybe_record_use(param_id, delta),
                    _ => {}
                }
            }
            if op_name == "-" && args.len() == 2 {
                if let (Some(param_id), Some(delta)) =
                    (param_id_from_node(&args[0]), number_from_node(&args[1]))
                {
                    maybe_record_use(param_id, -delta);
                }
            }
        }
    }

    for (child_path, child) in core_node_child_paths(node, path) {
        collect_anonymous_delta_uses(child, &child_path, part_key, program, out);
    }
}

fn anonymous_delta_suffix_for_param_key(param_key: &str) -> Option<(&'static str, String)> {
    let trimmed = param_key.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(stem) = trimmed.strip_suffix("_w") {
        return Some(("_margin_x", stem.trim_end_matches('_').to_string()));
    }
    if let Some(stem) = trimmed.strip_suffix("_h") {
        return Some(("_margin_y", stem.trim_end_matches('_').to_string()));
    }
    if let Some(stem) = trimmed.strip_suffix("width") {
        return Some(("_margin_x", stem.trim_end_matches('_').to_string()));
    }
    if let Some(stem) = trimmed.strip_suffix("height") {
        return Some(("_margin_y", stem.trim_end_matches('_').to_string()));
    }
    None
}

fn anonymous_delta_numeric_token(value: f64) -> String {
    if value.is_finite() && (value.fract().abs() < 1e-9) {
        if value < 0.0 {
            return format!("neg_{:.0}", value.abs());
        }
        return format!("{:.0}", value);
    }
    let base = value.to_string().replace('.', "_");
    if value < 0.0 {
        format!("neg_{}", base.trim_start_matches('-'))
    } else {
        base
    }
}

fn anonymous_delta_suggested_param_key(param_key: &str, delta: f64) -> String {
    if let Some((suffix, stem)) = anonymous_delta_suffix_for_param_key(param_key) {
        if stem.is_empty() {
            return format!("{param_key}{suffix}");
        }
        return format!("{stem}{suffix}");
    }
    let token = anonymous_delta_numeric_token(delta);
    format!("delta_{param_key}_{token}")
}

fn collect_ecky_constraint_authoring_lints(
    source: &str,
    program: &crate::ecky_core_ir::CoreProgram,
) -> Vec<EckyConstraintAuthoringLint> {
    let mut uses = Vec::new();
    for part in &program.parts {
        let root_path = format!("/parts/{}/root", path_segment(&part.key));
        collect_anonymous_delta_uses(&part.root, &root_path, &part.key, program, &mut uses);
    }

    let mut grouped: HashMap<(String, String, u64), Vec<&AnonymousDeltaUse>> = HashMap::new();
    for usage in &uses {
        grouped
            .entry((
                usage.part_key.clone(),
                usage.param_key.clone(),
                usage.delta.to_bits(),
            ))
            .or_default()
            .push(usage);
    }

    let mut lints = Vec::new();
    for ((part_key, param_key, delta_bits), group_uses) in grouped {
        if group_uses.len() < 2 {
            continue;
        }
        let delta = f64::from_bits(delta_bits);
        let occurrence_count = group_uses.len();
        let suggested_param_key = anonymous_delta_suggested_param_key(&param_key, delta);
        let mut source_stable_node_keys = Vec::new();
        for usage in group_uses {
            if let Some(stable_key) = stable_node_key_for_program_path(source, program, &usage.path)
            {
                if !source_stable_node_keys
                    .iter()
                    .any(|existing| existing == &stable_key)
                {
                    source_stable_node_keys.push(stable_key);
                }
            }
        }
        lints.push(EckyConstraintAuthoringLint {
            kind: "anonymousDelta".to_string(),
            part_key: part_key.clone(),
            param_key: param_key.clone(),
            delta,
            occurrence_count,
            suggested_param_key: suggested_param_key.clone(),
            message: format!(
                "Repeated anonymous delta on `{param_key}` in part `{part_key}`. Extract `{suggested_param_key}` parameter and reuse."
            ),
            source_stable_node_keys,
        });
    }

    lints.sort_by(|left, right| {
        left.part_key
            .cmp(&right.part_key)
            .then(left.param_key.cmp(&right.param_key))
            .then(left.suggested_param_key.cmp(&right.suggested_param_key))
    });
    lints
}

fn source_addressable_digest_for_path(
    program: &crate::ecky_core_ir::CoreProgram,
    requested_path: &str,
) -> Option<String> {
    let segments = requested_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>();
    if segments.len() == 2 && segments[0] == "params" {
        return program
            .parameters
            .iter()
            .find(|param| param.key == segments[1])
            .map(core_param_digest);
    }
    if segments.len() == 2 && segments[0] == "parts" {
        return program
            .parts
            .iter()
            .find(|part| part.key == segments[1])
            .map(core_part_digest);
    }
    find_core_ast_node_in_program(program, requested_path).map(core_node_digest)
}

fn raw_source_target_digest_for_path(source: &str, requested_path: &str) -> AppResult<String> {
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, requested_path)?;
    Ok(crate::mcp::macro_buffer::source_digest(&format!(
        "{:?}|{}",
        target.kind,
        &source[target.expr.start..target.expr.end]
    )))
}

fn edit_digest_for_ecky_path(
    program: &crate::ecky_core_ir::CoreProgram,
    source: &str,
    requested_path: &str,
) -> AppResult<String> {
    Ok(source_addressable_digest_for_path(program, requested_path)
        .unwrap_or(raw_source_target_digest_for_path(source, requested_path)?))
}

#[derive(Debug, Clone)]
struct SourceExprSpan {
    start: usize,
    end: usize,
    children: Vec<SourceExprSpan>,
}

impl SourceExprSpan {
    fn atom_text<'a>(&self, source: &'a str) -> Option<&'a str> {
        if self.children.is_empty() {
            Some(&source[self.start..self.end])
        } else {
            None
        }
    }
}

struct SourceExprParser<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> SourceExprParser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    fn parse_all(mut self) -> AppResult<Vec<SourceExprSpan>> {
        let mut exprs = Vec::new();
        while self.skip_ws_and_comments() < self.source.len() {
            exprs.push(self.parse_expr()?);
        }
        Ok(exprs)
    }

    fn skip_ws_and_comments(&mut self) -> usize {
        while self.cursor < self.source.len() {
            let rest = &self.source[self.cursor..];
            if rest.starts_with(';') {
                while self.cursor < self.source.len()
                    && !self.source[self.cursor..].starts_with('\n')
                {
                    self.cursor += 1;
                }
            } else if let Some(ch) = rest.chars().next() {
                if ch.is_whitespace() {
                    self.cursor += ch.len_utf8();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        self.cursor
    }

    fn parse_expr(&mut self) -> AppResult<SourceExprSpan> {
        self.skip_ws_and_comments();
        if self.cursor >= self.source.len() {
            return Err(AppError::validation("Unexpected end of Ecky source."));
        }
        if self.source[self.cursor..].starts_with('(') {
            return self.parse_list();
        }
        if self.source[self.cursor..].starts_with('"') {
            return self.parse_string();
        }
        Ok(self.parse_atom())
    }

    fn parse_list(&mut self) -> AppResult<SourceExprSpan> {
        let start = self.cursor;
        self.cursor += 1;
        let mut children = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.cursor >= self.source.len() {
                return Err(AppError::validation("Unclosed list in Ecky source."));
            }
            if self.source[self.cursor..].starts_with(')') {
                self.cursor += 1;
                return Ok(SourceExprSpan {
                    start,
                    end: self.cursor,
                    children,
                });
            }
            children.push(self.parse_expr()?);
        }
    }

    fn parse_string(&mut self) -> AppResult<SourceExprSpan> {
        let start = self.cursor;
        self.cursor += 1;
        let mut escaped = false;
        while self.cursor < self.source.len() {
            let ch = self.source[self.cursor..]
                .chars()
                .next()
                .ok_or_else(|| AppError::validation("Unclosed string in Ecky source."))?;
            self.cursor += ch.len_utf8();
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                return Ok(SourceExprSpan {
                    start,
                    end: self.cursor,
                    children: Vec::new(),
                });
            }
        }
        Err(AppError::validation("Unclosed string in Ecky source."))
    }

    fn parse_atom(&mut self) -> SourceExprSpan {
        let start = self.cursor;
        while self.cursor < self.source.len() {
            let ch = self.source[self.cursor..].chars().next().unwrap();
            if ch.is_whitespace() || ch == '(' || ch == ')' || ch == ';' {
                break;
            }
            self.cursor += ch.len_utf8();
        }
        SourceExprSpan {
            start,
            end: self.cursor,
            children: Vec::new(),
        }
    }
}

fn path_segment_decode(value: &str) -> String {
    value.replace("~1", "/").replace("~0", "~")
}

fn list_head<'a>(expr: &'a SourceExprSpan, source: &'a str) -> Option<&'a str> {
    expr.children
        .first()
        .and_then(|head| head.atom_text(source))
}

fn source_positional_arg<'a>(
    expr: &'a SourceExprSpan,
    source: &str,
    index: usize,
) -> Option<&'a SourceExprSpan> {
    let mut positional = 0usize;
    let mut idx = 1usize;
    while idx < expr.children.len() {
        let child = &expr.children[idx];
        if child
            .atom_text(source)
            .is_some_and(|text| text.starts_with(':'))
        {
            idx += 2;
            continue;
        }
        if positional == index {
            return Some(child);
        }
        positional += 1;
        idx += 1;
    }
    None
}

fn source_keyword_value<'a>(
    expr: &'a SourceExprSpan,
    source: &str,
    name: &str,
) -> Option<&'a SourceExprSpan> {
    let expected = format!(":{name}");
    expr.children.windows(2).find_map(|pair| {
        if pair[0].atom_text(source) == Some(expected.as_str()) {
            Some(&pair[1])
        } else {
            None
        }
    })
}

fn source_keyword_pair_span(
    expr: &SourceExprSpan,
    source: &str,
    name: &str,
) -> Option<(usize, usize)> {
    let expected = format!(":{name}");
    expr.children.windows(2).find_map(|pair| {
        if pair[0].atom_text(source) == Some(expected.as_str()) {
            Some((pair[0].start, pair[1].end))
        } else {
            None
        }
    })
}

fn model_form<'a>(exprs: &'a [SourceExprSpan], source: &str) -> AppResult<&'a SourceExprSpan> {
    exprs
        .iter()
        .find(|expr| list_head(expr, source) == Some("model"))
        .ok_or_else(|| AppError::validation("Ecky source has no model form."))
}

fn model_part_clause<'a>(
    model: &'a SourceExprSpan,
    source: &str,
    part_key: &str,
) -> Option<&'a SourceExprSpan> {
    model.children.iter().find(|expr| {
        list_head(expr, source) == Some("part")
            && expr.children.get(1).and_then(|item| item.atom_text(source)) == Some(part_key)
    })
}

fn model_params_form<'a>(model: &'a SourceExprSpan, source: &str) -> Option<&'a SourceExprSpan> {
    model
        .children
        .iter()
        .find(|expr| list_head(expr, source) == Some("params"))
}

fn model_param_decl<'a>(
    params: &'a SourceExprSpan,
    source: &str,
    key: &str,
) -> Option<&'a SourceExprSpan> {
    params
        .children
        .iter()
        .skip(1)
        .find(|expr| expr.children.get(1).and_then(|item| item.atom_text(source)) == Some(key))
}

fn build_shape_clause<'a>(
    build: &'a SourceExprSpan,
    source: &str,
    name: &str,
) -> Option<&'a SourceExprSpan> {
    build.children.iter().skip(1).find(|expr| {
        list_head(expr, source) == Some("shape")
            && expr.children.get(1).and_then(|item| item.atom_text(source)) == Some(name)
    })
}

fn build_result_clause<'a>(build: &'a SourceExprSpan, source: &str) -> Option<&'a SourceExprSpan> {
    build
        .children
        .iter()
        .skip(1)
        .find(|expr| list_head(expr, source) == Some("result"))
}

fn let_binding_pair<'a>(
    let_expr: &'a SourceExprSpan,
    source: &str,
    name: &str,
) -> Option<&'a SourceExprSpan> {
    let_expr.children.get(1).and_then(|bindings| {
        bindings.children.iter().find(|pair| {
            let Some(raw_name) = pair
                .children
                .first()
                .and_then(|item| item.atom_text(source))
            else {
                return false;
            };
            raw_name == name || name.contains(raw_name)
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SourcePathTargetKind {
    Root,
    PositionalArg,
    KeywordValue { name: String },
    PartClause { name: String },
    ParamDecl { name: String },
    BuildBinding { name: String },
    BuildResult,
    LetBinding { name: String },
    LetBody,
}

struct SourcePathTarget<'a> {
    expr: &'a SourceExprSpan,
    parent: Option<&'a SourceExprSpan>,
    scope: Option<&'a SourceExprSpan>,
    kind: SourcePathTargetKind,
}

fn source_target_for_ecky_path<'a>(
    exprs: &'a [SourceExprSpan],
    source: &str,
    path: &str,
) -> AppResult<SourcePathTarget<'a>> {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(path_segment_decode)
        .collect::<Vec<_>>();
    let model = model_form(exprs, source)?;
    if segments.len() == 2 && segments[0] == "params" {
        let params = model_params_form(model, source)
            .ok_or_else(|| AppError::validation("Ecky source has no params form."))?;
        let param = model_param_decl(params, source, &segments[1]).ok_or_else(|| {
            AppError::validation(format!("Ecky source has no param {}.", segments[1]))
        })?;
        return Ok(SourcePathTarget {
            expr: param,
            parent: Some(params),
            scope: Some(model),
            kind: SourcePathTargetKind::ParamDecl {
                name: segments[1].clone(),
            },
        });
    }
    if segments.len() == 2 && segments[0] == "parts" {
        let part = model_part_clause(model, source, &segments[1]).ok_or_else(|| {
            AppError::validation(format!("Ecky source has no part {}.", segments[1]))
        })?;
        return Ok(SourcePathTarget {
            expr: part,
            parent: Some(model),
            scope: Some(model),
            kind: SourcePathTargetKind::PartClause {
                name: segments[1].clone(),
            },
        });
    }
    if segments.len() < 3 || segments[0] != "parts" || segments[2] != "root" {
        return Err(AppError::validation(format!(
            "Ecky AST path is not source-span addressable in v1: {path}."
        )));
    }
    let part_key = &segments[1];
    let part = model_part_clause(model, source, part_key)
        .ok_or_else(|| AppError::validation(format!("Ecky source has no part {part_key}.")))?;
    let mut current = part
        .children
        .get(2)
        .ok_or_else(|| AppError::validation(format!("Ecky part {part_key} has no root node.")))?;
    let mut parent = None;
    let mut scope = Some(part);
    let mut kind = SourcePathTargetKind::Root;
    let mut idx = 3usize;
    while idx < segments.len() {
        match segments.get(idx).map(String::as_str) {
            Some("build") if list_head(current, source) == Some("build") => {
                match segments.get(idx + 1).map(String::as_str) {
                    Some("bindings") => {
                        let name = segments.get(idx + 2).ok_or_else(|| {
                            AppError::validation(format!("Invalid Ecky AST path: {path}."))
                        })?;
                        let shape = build_shape_clause(current, source, name).ok_or_else(|| {
                            AppError::validation(format!(
                                "Ecky AST source build binding not found: {path}."
                            ))
                        })?;
                        parent = Some(shape);
                        scope = Some(current);
                        current = shape.children.get(2).ok_or_else(|| {
                            AppError::validation(format!("Ecky build binding {name} has no value."))
                        })?;
                        kind = SourcePathTargetKind::BuildBinding {
                            name: name.to_string(),
                        };
                        idx += 3;
                    }
                    Some("result") => {
                        let result = build_result_clause(current, source).ok_or_else(|| {
                            AppError::validation(format!(
                                "Ecky AST source build result not found: {path}."
                            ))
                        })?;
                        parent = Some(result);
                        scope = Some(current);
                        current = result.children.get(1).ok_or_else(|| {
                            AppError::validation("Ecky build result has no value.")
                        })?;
                        kind = SourcePathTargetKind::BuildResult;
                        idx += 2;
                    }
                    _ => {
                        return Err(AppError::validation(format!(
                            "Ecky AST path is not source-span addressable in v1: {path}."
                        )));
                    }
                }
            }
            Some("let")
                if list_head(current, source) == Some("let")
                    || list_head(current, source) == Some("let*") =>
            {
                match segments.get(idx + 1).map(String::as_str) {
                    Some("bindings") => {
                        let name = segments.get(idx + 2).ok_or_else(|| {
                            AppError::validation(format!("Invalid Ecky AST path: {path}."))
                        })?;
                        let binding = let_binding_pair(current, source, name).ok_or_else(|| {
                            AppError::validation(format!(
                                "Ecky AST source let binding not found: {path}."
                            ))
                        })?;
                        parent = Some(binding);
                        scope = Some(current);
                        current = binding.children.get(1).ok_or_else(|| {
                            AppError::validation(format!("Ecky let binding {name} has no value."))
                        })?;
                        let raw_name = binding
                            .children
                            .first()
                            .and_then(|item| item.atom_text(source))
                            .unwrap_or(name);
                        kind = SourcePathTargetKind::LetBinding {
                            name: raw_name.to_string(),
                        };
                        idx += 3;
                    }
                    Some("body") => {
                        parent = Some(current);
                        scope = Some(current);
                        current = current
                            .children
                            .get(2)
                            .ok_or_else(|| AppError::validation("Ecky let form has no body."))?;
                        kind = SourcePathTargetKind::LetBody;
                        idx += 2;
                    }
                    _ => {
                        return Err(AppError::validation(format!(
                            "Ecky AST path is not source-span addressable in v1: {path}."
                        )));
                    }
                }
            }
            Some("call") if segments.get(idx + 1).map(String::as_str) == Some("args") => {
                let arg_index = segments
                    .get(idx + 2)
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| {
                        AppError::validation(format!("Invalid Ecky AST path: {path}."))
                    })?;
                parent = Some(current);
                scope = Some(current);
                current = source_positional_arg(current, source, arg_index).ok_or_else(|| {
                    AppError::validation(format!("Ecky AST source arg path not found: {path}."))
                })?;
                kind = SourcePathTargetKind::PositionalArg;
                idx += 3;
            }
            Some("call") if segments.get(idx + 1).map(String::as_str) == Some("keywords") => {
                let keyword = segments.get(idx + 2).ok_or_else(|| {
                    AppError::validation(format!("Invalid Ecky AST path: {path}."))
                })?;
                parent = Some(current);
                scope = Some(current);
                current = source_keyword_value(current, source, keyword).ok_or_else(|| {
                    AppError::validation(format!("Ecky AST source keyword path not found: {path}."))
                })?;
                kind = SourcePathTargetKind::KeywordValue {
                    name: keyword.to_string(),
                };
                idx += 3;
            }
            _ => {
                return Err(AppError::validation(format!(
                    "Ecky AST path is not source-span addressable in v1: {path}."
                )));
            }
        }
    }
    Ok(SourcePathTarget {
        expr: current,
        parent,
        scope,
        kind,
    })
}

fn source_span_for_ecky_path(source: &str, path: &str) -> AppResult<(usize, usize)> {
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, path)?;
    Ok((target.expr.start, target.expr.end))
}

fn source_anchor_span_for_edit(source: &str, path: &str) -> AppResult<(usize, usize)> {
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, path)?;
    match (&target.kind, target.parent) {
        (SourcePathTargetKind::KeywordValue { name }, Some(parent)) => {
            source_keyword_pair_span(parent, source, name).ok_or_else(|| {
                AppError::validation(format!("Ecky AST source keyword pair not found: {path}."))
            })
        }
        (
            SourcePathTargetKind::BuildBinding { .. }
            | SourcePathTargetKind::BuildResult
            | SourcePathTargetKind::LetBinding { .. },
            Some(parent),
        ) => Ok((parent.start, parent.end)),
        _ => Ok((target.expr.start, target.expr.end)),
    }
}

fn expand_delete_span(source: &str, start: usize, end: usize) -> (usize, usize) {
    if end < source.len() {
        if let Some(ch) = source[end..].chars().next() {
            if ch.is_whitespace() {
                return (start, end + ch.len_utf8());
            }
        }
    }
    if start > 0 {
        if let Some((prev_start, ch)) = source[..start].char_indices().last() {
            if ch.is_whitespace() {
                return (prev_start, end);
            }
        }
    }
    (start, end)
}

fn validate_ecky_identifier(name: &str) -> AppResult<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.starts_with(':')
        || trimmed
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' || ch == ';')
    {
        return Err(AppError::validation(format!(
            "Invalid Ecky identifier for rename: {name}."
        )));
    }
    Ok(trimmed)
}

fn collect_identifier_spans(
    expr: &SourceExprSpan,
    source: &str,
    name: &str,
    spans: &mut Vec<(usize, usize)>,
) {
    if let Some(text) = expr.atom_text(source) {
        if text == name {
            spans.push((expr.start, expr.end));
        }
        return;
    }
    for child in &expr.children {
        collect_identifier_spans(child, source, name, spans);
    }
}

fn rewrite_ranges(source: &str, ranges: &[(usize, usize)], replacement: &str) -> AppResult<String> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|(start, _)| *start);
    for pair in sorted.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(AppError::validation("Overlapping Ecky AST rename ranges."));
        }
    }
    let mut next = source.to_string();
    for (start, end) in sorted.into_iter().rev() {
        if start >= end
            || end > next.len()
            || !next.is_char_boundary(start)
            || !next.is_char_boundary(end)
        {
            return Err(AppError::validation(format!(
                "Invalid Ecky AST rename range {start}..{end}."
            )));
        }
        next.replace_range(start..end, replacement);
    }
    Ok(next)
}

fn collect_identifier_spans_excluding_shadowed_lets(
    expr: &SourceExprSpan,
    source: &str,
    name: &str,
    spans: &mut Vec<(usize, usize)>,
) {
    if let Some(text) = expr.atom_text(source) {
        if text == name {
            spans.push((expr.start, expr.end));
        }
        return;
    }

    let head = list_head(expr, source);
    if matches!(head, Some("let") | Some("let*")) {
        let mut shadows_name = false;
        if let Some(bindings) = expr.children.get(1) {
            for binding in &bindings.children {
                if binding
                    .children
                    .first()
                    .and_then(|item| item.atom_text(source))
                    == Some(name)
                {
                    shadows_name = true;
                }
                for child in binding.children.iter().skip(1) {
                    collect_identifier_spans_excluding_shadowed_lets(child, source, name, spans);
                }
            }
        }
        if !shadows_name {
            for child in expr.children.iter().skip(2) {
                collect_identifier_spans_excluding_shadowed_lets(child, source, name, spans);
            }
        }
        return;
    }

    for child in &expr.children {
        collect_identifier_spans_excluding_shadowed_lets(child, source, name, spans);
    }
}

fn rename_ecky_source_target(source: &str, path: &str, new_name: &str) -> AppResult<String> {
    let new_name = validate_ecky_identifier(new_name)?;
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, path)?;
    let mut ranges = Vec::new();
    match (&target.kind, target.parent, target.scope) {
        (SourcePathTargetKind::BuildBinding { name }, Some(shape), Some(build)) => {
            if build_shape_clause(build, source, new_name).is_some() {
                return Err(AppError::validation(format!(
                    "Ecky build binding {new_name} already exists."
                )));
            }
            let name_atom = shape.children.get(1).ok_or_else(|| {
                AppError::validation(format!("Ecky build binding {name} has no name atom."))
            })?;
            ranges.push((name_atom.start, name_atom.end));
            let shape_index = build
                .children
                .iter()
                .position(|child| child.start == shape.start && child.end == shape.end)
                .ok_or_else(|| AppError::validation("Ecky build binding parent not found."))?;
            for child in build.children.iter().skip(shape_index + 1) {
                collect_identifier_spans(child, source, name, &mut ranges);
            }
        }
        (SourcePathTargetKind::LetBinding { name }, Some(binding), Some(let_expr)) => {
            let duplicate = let_expr
                .children
                .get(1)
                .map(|bindings| {
                    bindings.children.iter().any(|pair| {
                        pair.children
                            .first()
                            .and_then(|item| item.atom_text(source))
                            == Some(new_name)
                    })
                })
                .unwrap_or(false);
            if duplicate {
                return Err(AppError::validation(format!(
                    "Ecky let binding {new_name} already exists."
                )));
            }
            let name_atom = binding.children.first().ok_or_else(|| {
                AppError::validation(format!("Ecky let binding {name} has no name atom."))
            })?;
            ranges.push((name_atom.start, name_atom.end));
            let body = let_expr
                .children
                .get(2)
                .ok_or_else(|| AppError::validation("Ecky let form has no body."))?;
            collect_identifier_spans(body, source, name, &mut ranges);
        }
        (SourcePathTargetKind::PartClause { name }, Some(_), Some(model)) => {
            if model_part_clause(model, source, new_name).is_some() {
                return Err(AppError::validation(format!(
                    "Ecky part {new_name} already exists."
                )));
            }
            let part = target.expr;
            let name_atom = part.children.get(1).ok_or_else(|| {
                AppError::validation(format!("Ecky part {name} has no name atom."))
            })?;
            ranges.push((name_atom.start, name_atom.end));
        }
        (SourcePathTargetKind::ParamDecl { name }, Some(_), Some(model)) => {
            let params = model_params_form(model, source)
                .ok_or_else(|| AppError::validation("Ecky source has no params form."))?;
            if model_param_decl(params, source, new_name).is_some() {
                return Err(AppError::validation(format!(
                    "Ecky param {new_name} already exists."
                )));
            }
            let param = target.expr;
            let name_atom = param.children.get(1).ok_or_else(|| {
                AppError::validation(format!("Ecky param {name} has no name atom."))
            })?;
            ranges.push((name_atom.start, name_atom.end));
            for child in &model.children {
                if child.start == params.start && child.end == params.end {
                    continue;
                }
                collect_identifier_spans_excluding_shadowed_lets(child, source, name, &mut ranges);
            }
        }
        _ => {
            return Err(AppError::validation(format!(
                "Ecky AST rename is not supported for path: {path}."
            )));
        }
    }
    let next_source = rewrite_ranges(source, &ranges, new_name)?;
    crate::ecky_scheme::compile_to_core_program(&next_source).map_err(|err| {
        compile_error_with_diagnostics(
            format!("Rename produced invalid Ecky source at {path}: {err}"),
            &next_source,
            err,
            Some("rename"),
            None,
        )
    })?;
    Ok(next_source)
}

fn replace_ecky_ast_source(
    source: &str,
    expected_source_digest: &str,
    path: &str,
    expected_node_digest: &str,
    operation: &EckyAstEditOperation,
    replacement_source: Option<&str>,
    new_name: Option<&str>,
) -> AppResult<String> {
    crate::mcp::macro_buffer::assert_expected_digest(source, expected_source_digest)?;
    let operation_name = ecky_ast_operation_name(operation);
    let program = crate::ecky_scheme::compile_to_core_program(source).map_err(|err| {
        compile_error_with_diagnostics(
            format!("Failed to compile Ecky source: {err}"),
            source,
            err,
            Some(operation_name),
            None,
        )
    })?;
    let exprs = SourceExprParser::new(source).parse_all()?;
    let target = source_target_for_ecky_path(&exprs, source, path)?;
    let target_kind = target.kind.clone();
    let node = find_core_ast_node_in_program(&program, path);
    let diagnostic_stable_node_key = stable_node_key_for_program_path(source, &program, path);
    let actual_node_digest = edit_digest_for_ecky_path(&program, source, path)?;
    if actual_node_digest != expected_node_digest {
        return Err(AppError::validation(format!(
            "Ecky AST node digest mismatch at {path}: expected {expected_node_digest}, actual {actual_node_digest}."
        )));
    }
    if matches!(operation, EckyAstEditOperation::Rename) {
        let new_name = new_name
            .or(replacement_source)
            .ok_or_else(|| AppError::validation("newName is required for Ecky AST rename."))?;
        return rename_ecky_source_target(source, path, new_name);
    }

    let replacement = match operation {
        EckyAstEditOperation::Replace
        | EckyAstEditOperation::InsertBefore
        | EckyAstEditOperation::InsertAfter => replacement_source
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::validation("replacementSource is required for Ecky AST replace/insert.")
            })?,
        EckyAstEditOperation::Delete => "",
        EckyAstEditOperation::Rename => unreachable!("rename returned above"),
    };

    let (start, end) = match operation {
        EckyAstEditOperation::Replace => {
            let core_span = if matches!(
                target_kind,
                SourcePathTargetKind::Root
                    | SourcePathTargetKind::PositionalArg
                    | SourcePathTargetKind::KeywordValue { .. }
            ) {
                node.and_then(|node| node.span)
                    .map(|span| (span.start as usize, span.end as usize))
            } else {
                None
            };
            match core_span {
                Some((start, end))
                    if start < end
                        && end <= source.len()
                        && source.is_char_boundary(start)
                        && source.is_char_boundary(end) =>
                {
                    (start, end)
                }
                _ => source_span_for_ecky_path(source, path)?,
            }
        }
        EckyAstEditOperation::InsertBefore
        | EckyAstEditOperation::InsertAfter
        | EckyAstEditOperation::Delete => source_anchor_span_for_edit(source, path)?,
        EckyAstEditOperation::Rename => unreachable!("rename returned above"),
    };
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return Err(AppError::validation(format!(
            "Ecky AST node at {path} has invalid source span {start}..{end}."
        )));
    }

    let next_source = match operation {
        EckyAstEditOperation::Replace => {
            let mut next_source = String::with_capacity(
                source
                    .len()
                    .saturating_sub(end - start)
                    .saturating_add(replacement.len()),
            );
            next_source.push_str(&source[..start]);
            next_source.push_str(replacement);
            next_source.push_str(&source[end..]);
            next_source
        }
        EckyAstEditOperation::InsertBefore => {
            let inserted = format!("{replacement} ");
            let mut next_source = String::with_capacity(source.len() + inserted.len());
            next_source.push_str(&source[..start]);
            next_source.push_str(&inserted);
            next_source.push_str(&source[start..]);
            next_source
        }
        EckyAstEditOperation::InsertAfter => {
            let inserted = format!(" {replacement}");
            let mut next_source = String::with_capacity(source.len() + inserted.len());
            next_source.push_str(&source[..end]);
            next_source.push_str(&inserted);
            next_source.push_str(&source[end..]);
            next_source
        }
        EckyAstEditOperation::Delete => {
            let exprs = SourceExprParser::new(source).parse_all()?;
            let target = source_target_for_ecky_path(&exprs, source, path)?;
            if matches!(target.kind, SourcePathTargetKind::Root) {
                return Err(AppError::validation(
                    "Deleting a part root is not supported by Ecky AST v1.",
                ));
            }
            let (delete_start, delete_end) = expand_delete_span(source, start, end);
            let mut next_source =
                String::with_capacity(source.len().saturating_sub(delete_end - delete_start));
            next_source.push_str(&source[..delete_start]);
            next_source.push_str(&source[delete_end..]);
            next_source
        }
        EckyAstEditOperation::Rename => unreachable!("rename returned above"),
    };
    crate::ecky_scheme::compile_to_core_program(&next_source).map_err(|err| {
        compile_error_with_diagnostics(
            format!("Replacement produced invalid Ecky source at {path}: {err}"),
            &next_source,
            err,
            Some(operation_name),
            diagnostic_stable_node_key.as_deref(),
        )
    })?;
    Ok(next_source)
}

fn text_line_len(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count().max(1)
    }
}

fn ecky_ast_patch_diff_side(source: &str, start: usize, end: usize) -> EckyAstPatchTextDiffSide {
    EckyAstPatchTextDiffSide {
        digest: crate::mcp::macro_buffer::source_digest(&source[start..end]),
        byte_len: end - start,
        line_len: text_line_len(&source[start..end]),
        span: EckyAstSpan {
            start: start as u32,
            end: end as u32,
        },
    }
}

fn patch_diff_spans(before: &str, after: &str) -> ((usize, usize), (usize, usize)) {
    let before_bytes = before.as_bytes();
    let after_bytes = after.as_bytes();

    let mut prefix = 0usize;
    let min_len = before_bytes.len().min(after_bytes.len());
    while prefix < min_len && before_bytes[prefix] == after_bytes[prefix] {
        prefix += 1;
    }

    let mut before_suffix = before_bytes.len();
    let mut after_suffix = after_bytes.len();
    while before_suffix > prefix
        && after_suffix > prefix
        && before_bytes[before_suffix - 1] == after_bytes[after_suffix - 1]
    {
        before_suffix -= 1;
        after_suffix -= 1;
    }

    ((prefix, before_suffix), (prefix, after_suffix))
}

fn renamed_path(path: &str, new_name: &str) -> String {
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(last) = segments.last_mut() {
        *last = path_segment(new_name);
    }
    format!("/{}", segments.join("/"))
}

fn validate_ecky_ast_patch(
    source: &str,
    expected_source_digest: &str,
    path: &str,
    expected_node_digest: &str,
    operation: &EckyAstEditOperation,
    replacement_source: Option<&str>,
    new_name: Option<&str>,
) -> AppResult<(String, String, String, EckyAstPatchDiff)> {
    let next_source = replace_ecky_ast_source(
        source,
        expected_source_digest,
        path,
        expected_node_digest,
        operation,
        replacement_source,
        new_name,
    )?;
    let next_program =
        crate::ecky_scheme::compile_to_core_program(&next_source).map_err(|err| {
            compile_error_with_diagnostics(
                format!("Failed to compile Ecky source: {err}"),
                &next_source,
                err,
                Some(ecky_ast_operation_name(operation)),
                None,
            )
        })?;
    let new_path = match operation {
        EckyAstEditOperation::Rename => new_name.map(|name| renamed_path(path, name)),
        EckyAstEditOperation::Delete => None,
        _ => Some(path.to_string()),
    };
    let new_node_digest = new_path
        .as_deref()
        .and_then(|next_path| {
            edit_digest_for_ecky_path(&next_program, &next_source, next_path).ok()
        })
        .unwrap_or_else(|| "deleted".to_string());
    let ((old_start, old_end), (new_start, new_end)) = patch_diff_spans(source, &next_source);
    let diff = EckyAstPatchDiff {
        old: ecky_ast_patch_diff_side(source, old_start, old_end),
        new: ecky_ast_patch_diff_side(&next_source, new_start, new_end),
    };
    Ok((
        next_source,
        new_node_digest,
        new_path.unwrap_or_default(),
        diff,
    ))
}

fn apply_macro_buffer_replacements(
    macro_code: &str,
    expected_digest: &str,
    replacements: &[MacroBufferReplacement],
) -> AppResult<String> {
    crate::mcp::macro_buffer::assert_expected_digest(macro_code, expected_digest).map_err(
        |err| {
            AppError::validation(format!(
                "Macro {} Read macro_buffer_get again before patching.",
                err.message.replacen("Buffer", "buffer", 1)
            ))
        },
    )?;

    if replacements.is_empty() {
        return Err(AppError::validation(
            "macro_buffer_replace_and_preview requires at least one replacement.",
        ));
    }

    let had_trailing_newline = macro_code.ends_with('\n');
    let mut lines: Vec<String> = macro_code.lines().map(str::to_string).collect();
    let line_count = lines.len();
    let mut sorted = replacements.to_vec();
    sorted.sort_by_key(|replacement| replacement.start_line);

    let mut previous_end = 0usize;
    for replacement in &sorted {
        if replacement.start_line == 0 {
            return Err(AppError::validation(
                "Macro buffer replacement startLine is 1-based and must be >= 1.",
            ));
        }
        if replacement.end_line < replacement.start_line {
            return Err(AppError::validation(format!(
                "Macro buffer replacement has endLine {} before startLine {}.",
                replacement.end_line, replacement.start_line
            )));
        }
        if replacement.end_line > line_count {
            return Err(AppError::validation(format!(
                "Macro buffer replacement line range {}..{} exceeds line count {}.",
                replacement.start_line, replacement.end_line, line_count
            )));
        }
        if replacement.start_line <= previous_end {
            return Err(AppError::validation(
                "Macro buffer replacements must not overlap.",
            ));
        }
        previous_end = replacement.end_line;
    }

    for replacement in sorted.iter().rev() {
        let start_idx = replacement.start_line - 1;
        let end_idx = replacement.end_line;
        let replacement_lines: Vec<String> =
            replacement.new_text.lines().map(str::to_string).collect();
        lines.splice(start_idx..end_idx, replacement_lines);
    }

    let mut patched = lines.join("\n");
    if had_trailing_newline {
        patched.push('\n');
    }
    Ok(patched)
}

fn get_session_macro_buffer(ctx: &AgentContext) -> AppResult<SessionMacroBuffer> {
    macro_buffers()
        .lock()
        .unwrap()
        .get(&ctx.session_id)
        .cloned()
        .ok_or_else(|| {
            AppError::validation(
                "No macro buffer for this session. Call macro_buffer_get before editing.",
            )
        })
}

fn set_session_macro_buffer(ctx: &AgentContext, buffer: SessionMacroBuffer) {
    macro_buffers()
        .lock()
        .unwrap()
        .insert(ctx.session_id.clone(), buffer);
}

pub async fn handle_macro_buffer_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroBufferGetRequest,
    ctx: &AgentContext,
) -> AppResult<MacroBufferGetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<MacroBufferGetResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading macro buffer.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, design_output, artifact_bundle, _model_manifest) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
                    Some(preview.model_manifest),
                )
            } else {
                let target = crate::services::target::resolve_editable_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                (
                    target.thread_id,
                    target.message_id,
                    target.design_output,
                    target.artifact_bundle,
                    target.model_manifest,
                )
            };

        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());
        tracked_model_id = artifact_bundle
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

        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);
        let macro_code = design_output.macro_code.clone();
        let lines = macro_buffer_lines(&macro_code);
        let line_count = lines.len();
        let digest = macro_buffer_digest(&macro_code);
        let (window_start_line, window_end_line, truncated, window_lines) =
            macro_buffer_line_window(&lines, req.start_line, req.end_line)?;
        set_session_macro_buffer(
            ctx,
            SessionMacroBuffer {
                thread_id: target_thread_id.clone(),
                message_id: target_message_id.clone(),
                macro_code: macro_code.clone(),
                macro_dialect: design_output.macro_dialect.clone(),
                post_processing: design_output.post_processing.clone(),
                geometry_backend: design_output.geometry_backend,
            },
        );

        Ok(MacroBufferGetResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            digest,
            line_count,
            window_start_line,
            window_end_line,
            truncated,
            lines: window_lines,
            source_language: design_output.source_language.as_str().to_string(),
            macro_dialect: design_output.macro_dialect,
            geometry_backend: design_output.geometry_backend.as_str().to_string(),
            post_processing: design_output.post_processing,
            authoring_context,
            artifact_digest,
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_ecky_ast_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyAstGetRequest,
    ctx: &AgentContext,
) -> AppResult<EckyAstGetResponse> {
    if !ecky_ast_authoring_enabled(state) {
        return Err(AppError::validation(
            "Ecky AST authoring is disabled. Set mcp.eckyAstAuthoring=true to expose ecky_ast_get.",
        ));
    }

    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<EckyAstGetResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading Ecky AST.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, design_output, artifact_bundle, _model_manifest) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
                    Some(preview.model_manifest),
                )
            } else {
                let target = crate::services::target::resolve_editable_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                (
                    target.thread_id,
                    target.message_id,
                    target.design_output,
                    target.artifact_bundle,
                    target.model_manifest,
                )
            };

        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());
        tracked_model_id = artifact_bundle
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

        if design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
            return Err(AppError::validation(format!(
                "ecky_ast_get only supports sourceLanguage=ecky. Target sourceLanguage={}.",
                design_output.source_language.as_str()
            )));
        }

        let source = design_output.macro_code.clone();
        let program = crate::ecky_scheme::compile_to_core_program(&source)
            .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
        let depth = req
            .depth
            .unwrap_or(DEFAULT_ECKY_AST_DEPTH)
            .min(MAX_ECKY_AST_DEPTH);
        let max_nodes = req
            .max_nodes
            .unwrap_or(DEFAULT_ECKY_AST_MAX_NODES)
            .clamp(1, MAX_ECKY_AST_NODES);
        let requested_path = req.path.filter(|path| !path.trim().is_empty());
        let root_paths = program
            .parameters
            .iter()
            .map(|param| format!("/params/{}", path_segment(&param.key)))
            .chain(
                program
                    .parts
                    .iter()
                    .map(|part| format!("/parts/{}/root", path_segment(&part.key))),
            )
            .chain(
                program
                    .parts
                    .iter()
                    .map(|part| format!("/parts/{}", path_segment(&part.key))),
            )
            .collect::<Vec<_>>();
        let mut nodes = Vec::new();
        let mut truncated = false;
        truncated |= collect_core_param_ast_nodes(
            &program,
            &source,
            requested_path.as_deref(),
            max_nodes,
            &mut nodes,
        )?;
        truncated |= collect_core_part_clause_ast_nodes(
            &program,
            &source,
            requested_path.as_deref(),
            max_nodes,
            &mut nodes,
        )?;

        for part in &program.parts {
            if nodes.len() >= max_nodes {
                truncated = true;
                break;
            }
            let root_path = format!("/parts/{}/root", path_segment(&part.key));
            if let Some(requested_path) = requested_path.as_deref() {
                if requested_path == "/" {
                    truncated |= collect_core_ast_nodes(
                        &source,
                        &part.root,
                        &root_path,
                        Some(&part.key),
                        depth,
                        max_nodes,
                        &mut nodes,
                    );
                } else if requested_path.starts_with(&root_path) {
                    truncated |= collect_matching_core_ast_nodes(
                        &source,
                        &part.root,
                        &root_path,
                        Some(&part.key),
                        requested_path,
                        depth,
                        max_nodes,
                        &mut nodes,
                    );
                }
            } else {
                truncated |= collect_core_ast_nodes(
                    &source,
                    &part.root,
                    &root_path,
                    Some(&part.key),
                    depth,
                    max_nodes,
                    &mut nodes,
                );
            }
            if nodes.len() >= max_nodes {
                truncated = true;
                break;
            }
        }

        if requested_path.as_deref().is_some_and(|path| path != "/") && nodes.is_empty() {
            return Err(AppError::validation(format!(
                "Ecky AST path not found: {}.",
                requested_path.as_deref().unwrap_or_default()
            )));
        }

        if req.include_source.unwrap_or(false) {
            attach_ecky_ast_source_slices(&source, &mut nodes);
        }

        let core_digest = crate::mcp::macro_buffer::source_digest(
            &nodes
                .iter()
                .map(|node| format!("{}={}", node.path, node.digest))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);

        Ok(EckyAstGetResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            source_digest: crate::mcp::macro_buffer::source_digest(&source),
            core_digest,
            root_paths,
            requested_path,
            depth,
            max_nodes,
            truncated,
            nodes,
            authoring_context,
            artifact_digest,
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_ecky_dependency_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyDependencyGetRequest,
    ctx: &AgentContext,
) -> AppResult<EckyDependencyGetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<EckyDependencyGetResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Reading Ecky dependency graph.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, design_output, artifact_bundle, model_manifest) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
                    Some(preview.model_manifest),
                )
            } else {
                let target = crate::services::target::resolve_editable_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                (
                    target.thread_id,
                    target.message_id,
                    target.design_output,
                    target.artifact_bundle,
                    target.model_manifest,
                )
            };

        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());
        tracked_model_id = artifact_bundle
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

        if design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
            return Err(AppError::validation(format!(
                "ecky_dependency_get only supports sourceLanguage=ecky. Target sourceLanguage={}.",
                design_output.source_language.as_str()
            )));
        }

        let path = req.path.trim();
        if path.is_empty() {
            return Err(AppError::validation(
                "ecky_dependency_get requires path. Supported path shapes: /params/{key}, /targets/{targetId}.",
            ));
        }

        let source = design_output.macro_code.clone();
        let program = crate::ecky_scheme::compile_to_core_program(&source)
            .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
        let query = parse_ecky_dependency_path(path)?;

        let (
            dependency_kind,
            dependent_source_paths,
            impacted_part_ids,
            impact_labels,
            feature_ids,
            parameter_keys,
            target_ids,
        ) = match query {
            EckyDependencyQuery::ParameterKey(param_key) => {
                let param_id = param_id_for_dependency_key(&program, &param_key)?;
                let dependent_source_paths = dependent_source_paths_for_param(&program, param_id);
                let reference_count = dependent_source_paths.len();
                let impacted_part_ids =
                    impacted_part_ids_for_dependency_paths(&dependent_source_paths);
                let impact_labels =
                    impact_labels_for_dependency(&impacted_part_ids, reference_count);
                (
                    "parameterReference".to_string(),
                    dependent_source_paths,
                    impacted_part_ids,
                    impact_labels,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            }
            EckyDependencyQuery::SelectionTargetId(target_id) => {
                let manifest = model_manifest.as_ref().ok_or_else(|| {
                    AppError::validation(
                        "ecky_dependency_get /targets/{targetId} requires a target modelManifest.",
                    )
                })?;
                let target = selection_target_by_id(manifest, &target_id).ok_or_else(|| {
                    AppError::validation(format!(
                        "Ecky dependency source path not found: /targets/{}.",
                        target_id
                    ))
                })?;

                let target_ids = selection_target_match_ids(target);
                let parameter_keys = target.parameter_keys.clone();
                let impacted_part_ids = vec![target.part_id.clone()];
                let (feature_ids, dependent_source_paths) =
                    feature_bindings_for_target_ids(manifest, &target_ids);
                let impact_labels =
                    impact_labels_for_dependency(&impacted_part_ids, dependent_source_paths.len());
                (
                    "selectionTargetReference".to_string(),
                    dependent_source_paths,
                    impacted_part_ids,
                    impact_labels,
                    feature_ids,
                    parameter_keys,
                    target_ids,
                )
            }
        };
        let reference_count = dependent_source_paths.len();
        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);

        Ok(EckyDependencyGetResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            source_digest: crate::mcp::macro_buffer::source_digest(&source),
            path: path.to_string(),
            dependency_kind,
            dependent_source_paths,
            impacted_part_ids,
            impact_labels,
            feature_ids,
            parameter_keys,
            target_ids,
            reference_count,
            authoring_context,
            artifact_digest,
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_ecky_selector_resolve(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckySelectorResolveRequest,
    ctx: &AgentContext,
) -> AppResult<EckySelectorResolveResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<EckySelectorResolveResponse> {
        let requested_target_id = req.target_id.trim();
        if requested_target_id.is_empty() {
            return Err(AppError::validation(
                "ecky_selector_resolve requires targetId.",
            ));
        }

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Resolving selector target.",
        )?;

        let target = crate::services::target::resolve_editable_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = target.model_id();

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "reading",
            "",
        )?;

        let Some(manifest) = target.model_manifest.as_ref() else {
            return Ok(EckySelectorResolveResponse {
                target_id: requested_target_id.to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                feature_ids: Vec::new(),
                parameter_keys: Vec::new(),
                provenance_candidates: EckySelectorResolveProvenanceCandidates {
                    feature_role: None,
                    source_stable_node_keys: Vec::new(),
                    operation_kinds: Vec::new(),
                    primitive_ids: Vec::new(),
                },
                confidence: EckySelectorResolveConfidence::None,
                reason: "No model manifest available for active target.".to_string(),
            });
        };

        let matched_targets = selection_targets_by_id(manifest, requested_target_id);
        if matched_targets.is_empty() {
            return Ok(EckySelectorResolveResponse {
                target_id: requested_target_id.to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                feature_ids: Vec::new(),
                parameter_keys: Vec::new(),
                provenance_candidates: EckySelectorResolveProvenanceCandidates {
                    feature_role: None,
                    source_stable_node_keys: Vec::new(),
                    operation_kinds: Vec::new(),
                    primitive_ids: Vec::new(),
                },
                confidence: EckySelectorResolveConfidence::None,
                reason: format!(
                    "No selection target matched targetId `{}`.",
                    requested_target_id
                ),
            });
        }

        if matched_targets.len() > 1 {
            let mut feature_ids = Vec::new();
            let mut parameter_keys = Vec::new();
            for matched in &matched_targets {
                push_unique_strings(&mut parameter_keys, &matched.parameter_keys);
                let (target_feature_ids, _) =
                    feature_bindings_for_target_ids(manifest, &selection_target_match_ids(matched));
                push_unique_strings(&mut feature_ids, &target_feature_ids);
            }
            let provenance_candidates = collect_selector_provenance_candidates(
                manifest,
                &matched_targets,
                Some(&target.design_output.macro_code),
            );
            return Ok(EckySelectorResolveResponse {
                target_id: requested_target_id.to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                feature_ids,
                parameter_keys,
                provenance_candidates,
                confidence: EckySelectorResolveConfidence::Ambiguous,
                reason: format!(
                    "Alias collision: {} selection targets matched targetId `{}`.",
                    matched_targets.len(),
                    requested_target_id
                ),
            });
        }

        let selected = matched_targets[0];
        let resolved_target_id = selected
            .target_id
            .clone()
            .unwrap_or_else(|| requested_target_id.to_string());
        let parameter_keys = selected.parameter_keys.clone();
        let (feature_ids, _) =
            feature_bindings_for_target_ids(manifest, &selection_target_match_ids(selected));
        let provenance_candidates = collect_selector_provenance_candidates(
            manifest,
            &[selected],
            Some(&target.design_output.macro_code),
        );

        let (confidence, reason) = if feature_ids.len() > 1 {
            (
                EckySelectorResolveConfidence::Ambiguous,
                format!(
                    "Multiple feature matches ({}) found for targetId `{}`.",
                    feature_ids.len(),
                    requested_target_id
                ),
            )
        } else if !parameter_keys.is_empty() {
            (
                EckySelectorResolveConfidence::Exact,
                "Resolved single selection target with <=1 feature match and non-empty parameter keys."
                    .to_string(),
            )
        } else {
            (
                EckySelectorResolveConfidence::Inferred,
                "Resolved target, but feature/parameter binding is partial.".to_string(),
            )
        };

        Ok(EckySelectorResolveResponse {
            target_id: resolved_target_id,
            durable_target_id: selected.durable_target_id.clone(),
            canonical_target_id: selected.canonical_target_id.clone(),
            feature_ids,
            parameter_keys,
            provenance_candidates,
            confidence,
            reason,
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_ecky_constraints_validate(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyConstraintsValidateRequest,
    ctx: &AgentContext,
) -> AppResult<EckyConstraintsValidateResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<EckyConstraintsValidateResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            "Validating Ecky parameter constraints.",
        )?;

        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, design_output, artifact_bundle) =
            if let Some(preview) = preview {
                (
                    preview.thread_id,
                    preview.preview_id,
                    preview.design_output,
                    Some(preview.artifact_bundle),
                )
            } else {
                let target = crate::services::target::resolve_editable_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                (
                    target.thread_id,
                    target.message_id,
                    target.design_output,
                    target.artifact_bundle,
                )
            };

        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());
        tracked_model_id = artifact_bundle
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

        if design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
            return Err(AppError::validation(format!(
                "ecky_constraints_validate only supports sourceLanguage=ecky. Target sourceLanguage={}.",
                design_output.source_language.as_str()
            )));
        }

        let source = design_output.macro_code.clone();
        let program = crate::ecky_scheme::compile_to_core_program(&source)
            .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
        let (params, parameter_source) = effective_ecky_constraint_params(
            &program,
            &design_output.initial_params,
            req.parameters,
        );
        let rows = validate_ecky_constraints(&source, &program, &params);
        let authoring_lints = collect_ecky_constraint_authoring_lints(&source, &program);
        let pass_count = rows.iter().filter(|row| row.status == "pass").count();
        let fail_count = rows.len().saturating_sub(pass_count);
        let authoring_context = crate::mcp::authoring::target_authoring_context(&design_output);
        let artifact_digest = artifact_bundle.as_ref().map(artifact_bundle_digest);

        Ok(EckyConstraintsValidateResponse {
            thread_id: target_thread_id,
            message_id: target_message_id,
            title: design_output.title,
            version_name: design_output.version_name,
            resolved_from: TargetResolvedFrom::Base,
            source_digest: crate::mcp::macro_buffer::source_digest(&source),
            parameter_source,
            pass_count,
            fail_count,
            rows,
            authoring_lints,
            authoring_context,
            artifact_digest,
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_ecky_ast_replace_and_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyAstReplaceAndRenderRequest,
    ctx: &AgentContext,
) -> AppResult<MacroReplaceResponse> {
    if !ecky_ast_authoring_enabled(state) {
        return Err(AppError::validation(
            "Ecky AST authoring is disabled. Set mcp.eckyAstAuthoring=true to expose ecky_ast_replace_and_render.",
        ));
    }

    let ctx = ctx.with_override(&req.identity);
    let preview = session_render_preview_for_request(
        &ctx,
        req.thread_id.as_deref(),
        req.message_id.as_deref(),
    );
    let (thread_id, message_id, design_output) = if let Some(preview) = preview {
        (preview.thread_id, preview.preview_id, preview.design_output)
    } else {
        let conn = state.db.lock().await;
        let target = crate::services::target::resolve_editable_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;
        drop(conn);
        (target.thread_id, target.message_id, target.design_output)
    };

    if design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
        return Err(AppError::validation(format!(
            "ecky_ast_replace_and_render only supports sourceLanguage=ecky. Target sourceLanguage={}.",
            design_output.source_language.as_str()
        )));
    }

    let source = design_output.macro_code.clone();
    let program = crate::ecky_scheme::compile_to_core_program(&source)
        .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
    let resolved_path = resolve_ecky_ast_patch_path(
        &source,
        &program,
        req.path.as_deref(),
        req.stable_node_key.as_deref(),
        "ecky_ast_replace_and_render",
    )?;

    let next_source = replace_ecky_ast_source(
        &source,
        &req.source_digest,
        &resolved_path,
        &req.expected_node_digest,
        &req.operation,
        req.replacement_source.as_deref(),
        req.new_name.as_deref(),
    )?;

    handle_macro_preview_render(
        state,
        app,
        MacroReplaceRequest {
            identity: req.identity,
            thread_id: Some(thread_id),
            message_id: Some(message_id),
            macro_code: next_source,
            macro_dialect: Some(MacroDialect::EckyIrV0),
            ui_spec: None,
            parameters: req.parameters,
            post_processing: req.post_processing,
            geometry_backend: req.geometry_backend,
        },
        &ctx,
    )
    .await
}

pub async fn handle_ecky_ast_patch_validate(
    state: &AppState,
    app: &dyn PathResolver,
    req: EckyAstPatchValidateRequest,
    _ctx: &AgentContext,
) -> AppResult<EckyAstPatchValidateResponse> {
    if !ecky_ast_authoring_enabled(state) {
        return Err(AppError::validation(
            "Ecky AST authoring is disabled. Set mcp.eckyAstAuthoring=true to expose ecky_ast_patch_validate.",
        ));
    }

    let conn = state.db.lock().await;
    let target = crate::services::target::resolve_editable_target(
        &conn,
        app,
        req.thread_id.clone(),
        req.message_id.clone(),
    )?;
    drop(conn);

    if target.design_output.source_language != crate::models::SourceLanguage::EckyIrV0 {
        return Err(AppError::validation(format!(
            "ecky_ast_patch_validate only supports sourceLanguage=ecky. Target sourceLanguage={}.",
            target.design_output.source_language.as_str()
        )));
    }

    let source = target.design_output.macro_code.clone();
    let source_program = crate::ecky_scheme::compile_to_core_program(&source)
        .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
    let resolved_path = resolve_ecky_ast_patch_path(
        &source,
        &source_program,
        req.path.as_deref(),
        req.stable_node_key.as_deref(),
        "ecky_ast_patch_validate",
    )?;
    let (next_source, new_node_digest, new_path, diff) = validate_ecky_ast_patch(
        &source,
        &req.source_digest,
        &resolved_path,
        &req.expected_node_digest,
        &req.operation,
        req.replacement_source.as_deref(),
        req.new_name.as_deref(),
    )?;
    let next_program = crate::ecky_scheme::compile_to_core_program(&next_source)
        .map_err(|err| AppError::validation(format!("Failed to compile Ecky source: {err}")))?;
    let operation = match req.operation {
        EckyAstEditOperation::Replace => "replace",
        EckyAstEditOperation::InsertBefore => "insertBefore",
        EckyAstEditOperation::InsertAfter => "insertAfter",
        EckyAstEditOperation::Delete => "delete",
        EckyAstEditOperation::Rename => "rename",
    };
    let affected_path_details = vec![EckyAstPatchAffectedPath {
        change: operation.to_string(),
        old_path: resolved_path.clone(),
        new_path: new_path.clone(),
        old_digest: req.expected_node_digest.clone(),
        new_digest: new_node_digest.clone(),
    }];

    let authoring_context = crate::mcp::authoring::target_authoring_context(&target.design_output);
    let mut affected_paths = vec![resolved_path.clone()];
    if !new_path.is_empty() && new_path != resolved_path {
        affected_paths.push(new_path.clone());
    }
    let edited_path_for_summary = if new_path.is_empty() {
        resolved_path.clone()
    } else {
        new_path.clone()
    };
    let affected_node_keys = affected_node_keys_for_patch(
        &source,
        &source_program,
        &resolved_path,
        &next_source,
        &next_program,
        &new_path,
    );
    let dependency_impact = Some(dependency_impact_for_patch(
        &next_program,
        &edited_path_for_summary,
        &affected_paths,
    ));

    Ok(EckyAstPatchValidateResponse {
        thread_id: target.thread_id,
        message_id: target.message_id,
        title: target.design_output.title,
        version_name: target.design_output.version_name,
        resolved_from: map_target_resolved_from(target.resolved_from),
        operation: operation.to_string(),
        edited_path: edited_path_for_summary,
        status: "valid".to_string(),
        source_digest: req.source_digest,
        new_source_digest: crate::mcp::macro_buffer::source_digest(&next_source),
        old_node_digest: req.expected_node_digest,
        new_node_digest,
        affected_paths,
        affected_path_details,
        affected_node_keys,
        dependency_impact,
        diff,
        authoring_context,
    })
}

pub async fn handle_target_detail_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: TargetDetailRequest,
    ctx: &AgentContext,
) -> AppResult<TargetDetailResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;

    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<TargetDetailResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            format!("Reading target detail section {:?}.", req.section),
        )?;

        let target = crate::services::target::resolve_editable_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = target.model_id();

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "reading",
            "",
        )?;

        let (
            ui_spec,
            initial_params,
            artifact_bundle,
            artifact_paths,
            viewer_assets,
            export_artifacts,
            latest_draft,
            shape_graph,
        ) = match req.section {
            TargetDetailSection::UiSpec => (
                Some(target.design_output.ui_spec.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            TargetDetailSection::InitialParams => (
                None,
                Some(target.design_output.initial_params.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            TargetDetailSection::ArtifactBundle => {
                let digest = target.artifact_bundle.as_ref().map(artifact_bundle_digest);
                (None, None, Some(digest), None, None, None, None, None)
            }
            TargetDetailSection::ArtifactPaths => {
                let paths = target.artifact_bundle.as_ref().map(|b| {
                    let mut p: Vec<String> = vec![b.fcstd_path.clone()];
                    if let Some(mp) = &b.macro_path {
                        p.insert(0, mp.clone());
                    }
                    if !b.preview_stl_path.is_empty() {
                        p.push(b.preview_stl_path.clone());
                    }
                    p
                });
                (None, None, None, paths, None, None, None, None)
            }
            TargetDetailSection::ViewerAssets => (
                None,
                None,
                None,
                None,
                target
                    .artifact_bundle
                    .as_ref()
                    .map(|b| b.viewer_assets.clone()),
                None,
                None,
                None,
            ),
            TargetDetailSection::ExportArtifacts => (
                None,
                None,
                None,
                None,
                None,
                target
                    .artifact_bundle
                    .as_ref()
                    .map(|b| b.export_artifacts.clone()),
                None,
                None,
            ),
            TargetDetailSection::LatestDraft => {
                let latest_draft = if let Some(preview) = session_render_preview_for_request(
                    ctx,
                    Some(target.thread_id.as_str()),
                    Some(target.message_id.as_str()),
                ) {
                    Some(Some(agent_draft_from_session_render_preview(preview)))
                } else {
                    let draft = db::get_agent_draft_for_session(&conn, &ctx.session_id)
                        .map_err(|e| AppError::persistence(e.to_string()))?
                        .filter(|draft| {
                            draft.thread_id == target.thread_id
                                && (draft.preview_id == target.message_id
                                    || draft.base_message_id.as_deref()
                                        == Some(target.message_id.as_str()))
                        });
                    Some(draft)
                };
                (None, None, None, None, None, None, latest_draft, None)
            }
            TargetDetailSection::ShapeGraph => (
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(build_shape_graph_packet(
                    &target.design_output,
                    target.model_manifest.as_ref(),
                    target.artifact_bundle.as_ref(),
                    req.shape_graph_filters.as_deref().unwrap_or(&[]),
                )?),
            ),
        };

        let authoring_context =
            crate::mcp::authoring::target_authoring_context(&target.design_output);

        Ok(TargetDetailResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            title: target.design_output.title,
            version_name: target.design_output.version_name,
            resolved_from: map_target_resolved_from(target.resolved_from),
            section: req.section,
            authoring_context,
            ui_spec,
            initial_params,
            artifact_bundle,
            artifact_paths,
            viewer_assets,
            export_artifacts,
            latest_draft,
            shape_graph,
        })
    })();

    if let Err(err) = &result {
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

fn artifact_bundle_digest(bundle: &ArtifactBundle) -> ArtifactBundleDigest {
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

pub async fn handle_artifact_manifest_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: ArtifactManifestRequest,
    ctx: &AgentContext,
) -> AppResult<ArtifactManifestResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;
    let target = crate::services::target::resolve_editable_target(
        &conn,
        app,
        req.thread_id.clone(),
        req.message_id.clone(),
    )?;
    drop(conn);

    let requested_model_id = req
        .model_id
        .clone()
        .or_else(|| target.model_id())
        .ok_or_else(|| AppError::validation("Target has no artifact modelId."))?;

    {
        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            ctx,
            Some(target.thread_id.clone()),
            Some(target.message_id.clone()),
            Some(requested_model_id.clone()),
            "reading",
            "Reading runtime artifact manifest.",
        )?;
    }

    let (artifact_bundle, model_manifest) = match (
        target.artifact_bundle.clone(),
        target.model_manifest.clone(),
    ) {
        (Some(bundle), Some(manifest)) if bundle.model_id == requested_model_id => {
            (bundle, manifest)
        }
        _ => {
            let bundle = crate::model_runtime::read_artifact_bundle(app, &requested_model_id)?;
            let manifest = crate::model_runtime::read_model_manifest(app, &requested_model_id)?;
            (bundle, manifest)
        }
    };

    crate::models::validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;
    let digest = artifact_bundle_digest(&artifact_bundle);

    Ok(ArtifactManifestResponse {
        thread_id: target.thread_id,
        message_id: target.message_id,
        model_id: requested_model_id,
        digest,
        artifact_bundle,
        model_manifest,
        runtime_manifest_valid: true,
    })
}

pub async fn handle_artifact_feature_graph_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: ArtifactFeatureGraphGetRequest,
    ctx: &AgentContext,
) -> AppResult<ArtifactFeatureGraphGetResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;
    let target = crate::services::target::resolve_editable_target(
        &conn,
        app,
        req.thread_id.clone(),
        req.message_id.clone(),
    )?;
    drop(conn);

    let requested_model_id = req
        .model_id
        .clone()
        .or_else(|| target.model_id())
        .ok_or_else(|| AppError::validation("Target has no artifact modelId."))?;

    {
        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            ctx,
            Some(target.thread_id.clone()),
            Some(target.message_id.clone()),
            Some(requested_model_id.clone()),
            "reading",
            "Reading artifact feature graph.",
        )?;
    }

    let (artifact_bundle, model_manifest) =
        crate::model_runtime::read_runtime_bundle(app, &requested_model_id).map_err(|err| {
            if err.message.contains("Failed to read model manifest") {
                AppError::validation(format!(
                    "No model manifest found for modelId '{}'. artifact_feature_graph_get requires a runtime manifest.",
                    requested_model_id
                ))
            } else {
                err
            }
        })?;
    if artifact_bundle.model_id != requested_model_id
        || model_manifest.model_id != requested_model_id
    {
        return Err(AppError::validation(format!(
            "Runtime manifest modelId does not match requested modelId '{}'.",
            requested_model_id
        )));
    }
    crate::models::validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;

    Ok(ArtifactFeatureGraphGetResponse {
        thread_id: target.thread_id,
        message_id: target.message_id,
        model_id: requested_model_id,
        artifact_digest: artifact_bundle_digest(&artifact_bundle),
        feature_graph: model_manifest.feature_graph,
        correspondence_graph: model_manifest.correspondence_graph,
    })
}

pub async fn handle_params_preview_render(
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
        let preview = session_render_preview_for_request(
            ctx,
            req.thread_id.as_deref(),
            req.message_id.as_deref(),
        );
        let (target_thread_id, target_message_id, base_design, base_model_manifest) =
            if let Some(preview) = preview.clone() {
                (
                    preview.thread_id.clone(),
                    preview
                        .base_message_id
                        .clone()
                        .unwrap_or_else(|| preview.preview_id.clone()),
                    preview.design_output.clone(),
                    Some(preview.model_manifest.clone()),
                )
            } else {
                let target = crate::services::target::resolve_target(
                    &conn,
                    app,
                    req.thread_id.clone(),
                    req.message_id.clone(),
                )?;
                let base_design = target
                    .design
                    .ok_or_else(|| AppError::validation("Target has no design output."))?;
                tracked_model_id = target
                    .artifact_bundle
                    .as_ref()
                    .map(|bundle| bundle.model_id.clone());
                (
                    target.thread_id,
                    target.message_id,
                    base_design,
                    target.model_manifest,
                )
            };

        if let Some(preview) = preview.as_ref() {
            tracked_model_id = Some(preview.artifact_bundle.model_id.clone());
        }
        tracked_thread_id = Some(target_thread_id.clone());
        tracked_message_id = Some(target_message_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "patching_params",
            "",
        )?;
        mark_live_session_busy(
            state,
            ctx,
            Some(session_target_ref(
                target_thread_id.clone(),
                target_message_id.clone(),
                tracked_model_id.clone(),
            )),
            "patching_params",
            Some("Patching parameters for the active target.".to_string()),
            None,
            false,
        )
        .await;
        push_trace_event_with_conn(
            state,
            &conn,
            ctx,
            TraceEvent {
                thread_id: tracked_thread_id.clone(),
                message_id: tracked_message_id.clone(),
                model_id: tracked_model_id.clone(),
                phase: "patching_params",
                kind: "tool_start",
                summary: "Patching parameters for the active target.".to_string(),
                details: None,
            },
        );

        let mut merged_params = base_design.initial_params.clone();
        for (key, value) in req.parameter_patch.clone() {
            merged_params.insert(key, value);
        }

        let mut healed_ui_spec = base_design.ui_spec.clone();
        let mut healed_params = merged_params.clone();
        if let Err(err) = crate::models::validate_design_params(&healed_params, &healed_ui_spec) {
            if base_design.macro_dialect == MacroDialect::Legacy && is_param_schema_mismatch(&err) {
                if let Some((next_ui_spec, next_params, heal_report)) = auto_heal_legacy_params(
                    &base_design.macro_code,
                    &healed_ui_spec,
                    &healed_params,
                    Some(&base_design.initial_params),
                )? {
                    push_trace_event_with_conn(
                        state,
                        &conn,
                        ctx,
                        TraceEvent {
                            thread_id: tracked_thread_id.clone(),
                            message_id: tracked_message_id.clone(),
                            model_id: tracked_model_id.clone(),
                            phase: "patching_params",
                            kind: "auto_heal_applied",
                            summary: "Reconciled legacy uiSpec and initialParams from parsed macro params."
                                .to_string(),
                            details: Some(format!(
                                "added={:?}; dropped={:?}; carried={:?}",
                                heal_report.added_keys, heal_report.dropped_keys, heal_report.carried_keys
                            )),
                        },
                    );
                    healed_ui_spec = next_ui_spec;
                    healed_params = next_params;
                } else {
                    return Err(AppError::with_details(
                        crate::contracts::AppErrorCode::Validation,
                        err.message,
                        format!(
                            "Legacy param auto-heal could not parse dynamic params for session {} on thread {:?}.",
                            ctx.session_id, tracked_thread_id
                        ),
                    ));
                }
            } else {
                return Err(err);
            }
        }

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "rendering",
            "",
        )?;
        mark_live_session_busy(
            state,
            ctx,
            Some(session_target_ref(
                target_thread_id.clone(),
                target_message_id.clone(),
                tracked_model_id.clone(),
            )),
            "rendering",
            Some("Rendering the updated version.".to_string()),
            None,
            false,
        )
        .await;

        let next_post_processing = req
            .post_processing
            .clone()
            .or_else(|| base_design.post_processing.clone());
        let authoring_context = resolve_macro_authoring_context(
            base_design.source_language,
            base_design.geometry_backend,
            &base_design.macro_dialect,
            req.geometry_backend,
        )?;
        let render_geometry_backend = authoring_context.geometry_backend;
        let base_context = MacroAuthoringContext {
            source_language: base_design.source_language,
            geometry_backend: base_design.geometry_backend,
        };
        log_macro_backend_resolution(
            state,
            &conn,
            ctx,
            "params_preview_render",
            &base_context,
            &base_design.macro_dialect,
            req.geometry_backend,
            &authoring_context,
            Some(&target_thread_id),
            Some(&target_message_id),
            tracked_model_id.as_deref(),
        );

        drop(conn);

        let artifact_bundle = render::render_model(
            &base_design.macro_code,
            &healed_params,
            Some(base_design.macro_dialect.clone()),
            Some(render_geometry_backend),
            next_post_processing.as_ref(),
            state,
            app,
        )
        .await?;
        let model_manifest =
            crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
        let model_manifest = carry_forward_semantic_manifest(
            base_model_manifest.as_ref(),
            model_manifest,
            &artifact_bundle,
        );
        let model_manifest = crate::model_runtime::write_model_manifest(
            app,
            &artifact_bundle.model_id,
            &model_manifest,
        )?;
        tracked_model_id = Some(artifact_bundle.model_id.clone());

        let mut design_output = base_design.clone();
        design_output.ui_spec = healed_ui_spec;
        design_output.initial_params = healed_params.clone();
        design_output.post_processing = next_post_processing;
        design_output.source_language = authoring_context.source_language;
        design_output.geometry_backend = render_geometry_backend;
        design_output.version_name.clear();
        design_output.interaction_mode = InteractionMode::Tune;

        let sv = crate::services::structural_verification::verify_structure(
            &artifact_bundle,
            &model_manifest,
        );
        let preview = store_session_render_preview(
            state,
            app,
            ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: target_thread_id.clone(),
                base_message_id: Some(target_message_id.clone()),
                design_output: design_output.clone(),
                artifact_bundle: artifact_bundle.clone(),
                model_manifest: model_manifest.clone(),
                draft_feedback: Some(draft_feedback_from_structural_verification(&sv)),
            },
        )
        .await?;
        tracked_message_id = Some(preview.preview_id.clone());
        Ok(ParamsPatchResponse {
            thread_id: target_thread_id,
            message_id: preview.preview_id,
            merged_params: healed_params,
            artifact_digest: artifact_bundle_digest(&artifact_bundle),
            artifact_bundle,
            model_manifest,
            design_output,
            structural_verification: Some(sv),
        })
    }
    .await;

    settle_live_render_phase(
        state,
        ctx,
        tracked_thread_id.as_deref(),
        tracked_message_id.as_deref(),
        tracked_model_id.clone(),
        &result,
    )
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

pub async fn handle_macro_buffer_replace_and_preview(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroBufferReplaceAndRenderRequest,
    ctx: &AgentContext,
) -> AppResult<MacroBufferReplaceAndRenderResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut buffer = get_session_macro_buffer(ctx)?;
    if let Some(thread_id) = &req.thread_id {
        if thread_id != &buffer.thread_id {
            return Err(AppError::validation(
                "macro_buffer_replace_and_preview threadId does not match session buffer.",
            ));
        }
    }
    if let Some(message_id) = &req.message_id {
        if message_id != &buffer.message_id {
            return Err(AppError::validation(
                "macro_buffer_replace_and_preview messageId does not match session buffer.",
            ));
        }
    }
    let patched_macro_code = apply_macro_buffer_replacements(
        &buffer.macro_code,
        &req.expected_digest,
        &req.replacements,
    )?;
    buffer.macro_code = patched_macro_code.clone();
    set_session_macro_buffer(ctx, buffer.clone());

    let render_response = handle_macro_preview_render(
        state,
        app,
        MacroReplaceRequest {
            identity: req.identity,
            thread_id: Some(buffer.thread_id.clone()),
            message_id: Some(buffer.message_id.clone()),
            macro_code: patched_macro_code,
            macro_dialect: Some(buffer.macro_dialect.clone()),
            ui_spec: req.ui_spec,
            parameters: req.parameters,
            post_processing: req.post_processing.or(buffer.post_processing.clone()),
            geometry_backend: Some(buffer.geometry_backend),
        },
        ctx,
    )
    .await?;

    let digest = macro_buffer_digest(&render_response.macro_code);
    let line_count = macro_buffer_lines(&render_response.macro_code).len();
    buffer.thread_id = render_response.thread_id.clone();
    buffer.message_id = render_response.message_id.clone();
    buffer.macro_code = render_response.macro_code.clone();
    set_session_macro_buffer(ctx, buffer);
    Ok(MacroBufferReplaceAndRenderResponse {
        thread_id: render_response.thread_id,
        message_id: render_response.message_id,
        digest,
        line_count,
        macro_code: render_response.macro_code,
        ui_spec: render_response.ui_spec,
        initial_params: render_response.initial_params,
        artifact_bundle: render_response.artifact_bundle,
        model_manifest: render_response.model_manifest,
        structural_verification: render_response.structural_verification,
        artifact_digest: render_response.artifact_digest,
    })
}

pub async fn handle_macro_buffer_replace_range(
    req: MacroBufferReplaceAndRenderRequest,
    ctx: &AgentContext,
) -> AppResult<MacroBufferEditResponse> {
    let ctx = ctx.with_override(&req.identity);
    let mut buffer = get_session_macro_buffer(&ctx)?;
    let window_start = req
        .replacements
        .iter()
        .map(|replacement| replacement.start_line)
        .min();
    buffer.macro_code = apply_macro_buffer_replacements(
        &buffer.macro_code,
        &req.expected_digest,
        &req.replacements,
    )?;
    let lines = macro_buffer_lines(&buffer.macro_code);
    let (window_start_line, window_end_line, truncated, window_lines) =
        macro_buffer_line_window(&lines, window_start, None)?;
    let response = MacroBufferEditResponse {
        digest: macro_buffer_digest(&buffer.macro_code),
        line_count: lines.len(),
        window_start_line,
        window_end_line,
        truncated,
        lines: window_lines,
    };
    set_session_macro_buffer(&ctx, buffer);
    Ok(response)
}

pub async fn handle_macro_buffer_apply_patch(
    req: MacroBufferApplyPatchRequest,
    ctx: &AgentContext,
) -> AppResult<MacroBufferEditResponse> {
    let ctx = ctx.with_override(&req.identity);
    let mut buffer = get_session_macro_buffer(&ctx)?;
    crate::mcp::macro_buffer::assert_expected_digest(&buffer.macro_code, &req.expected_digest)?;
    buffer.macro_code =
        crate::mcp::macro_buffer::apply_unified_patch(&buffer.macro_code, &req.patch)?;
    let lines = macro_buffer_lines(&buffer.macro_code);
    let (window_start_line, window_end_line, truncated, window_lines) =
        macro_buffer_line_window(&lines, None, None)?;
    let response = MacroBufferEditResponse {
        digest: macro_buffer_digest(&buffer.macro_code),
        line_count: lines.len(),
        window_start_line,
        window_end_line,
        truncated,
        lines: window_lines,
    };
    set_session_macro_buffer(&ctx, buffer);
    Ok(response)
}

pub async fn handle_macro_buffer_preview_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroBufferRenderRequest,
    ctx: &AgentContext,
) -> AppResult<MacroReplaceResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut buffer = get_session_macro_buffer(ctx)?;
    crate::mcp::macro_buffer::assert_expected_digest(&buffer.macro_code, &req.expected_digest)?;
    let response = handle_macro_preview_render(
        state,
        app,
        MacroReplaceRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(buffer.thread_id.clone()),
            message_id: Some(buffer.message_id.clone()),
            macro_code: buffer.macro_code.clone(),
            macro_dialect: Some(buffer.macro_dialect.clone()),
            ui_spec: req.ui_spec,
            parameters: req.parameters,
            post_processing: req.post_processing.or(buffer.post_processing.clone()),
            geometry_backend: Some(buffer.geometry_backend),
        },
        ctx,
    )
    .await?;
    buffer.thread_id = response.thread_id.clone();
    buffer.message_id = response.message_id.clone();
    buffer.macro_code = response.macro_code.clone();
    set_session_macro_buffer(ctx, buffer);
    Ok(response)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MacroAuthoringContext {
    source_language: crate::models::SourceLanguage,
    geometry_backend: crate::models::GeometryBackend,
}

fn infer_macro_source_language(dialect: &MacroDialect) -> crate::models::SourceLanguage {
    match dialect {
        MacroDialect::EckyIrV0 => crate::models::SourceLanguage::EckyIrV0,
        MacroDialect::Build123d => crate::models::SourceLanguage::Build123d,
        MacroDialect::Legacy | MacroDialect::CadFrameworkV1 => {
            crate::models::SourceLanguage::LegacyPython
        }
    }
}

fn configured_authoring_context(state: &AppState) -> MacroAuthoringContext {
    let config = state.config.lock().unwrap();
    MacroAuthoringContext {
        source_language: config.default_source_language,
        geometry_backend: config.default_geometry_backend,
    }
}

fn log_macro_backend_resolution(
    state: &AppState,
    conn: &rusqlite::Connection,
    ctx: &AgentContext,
    phase: &'static str,
    base_context: &MacroAuthoringContext,
    macro_dialect: &MacroDialect,
    requested_geometry_backend: Option<crate::models::GeometryBackend>,
    resolved_context: &MacroAuthoringContext,
    thread_id: Option<&str>,
    message_id: Option<&str>,
    model_id: Option<&str>,
) {
    let configured_context = configured_authoring_context(state);
    let requested = requested_geometry_backend
        .map(|backend| backend.as_str())
        .unwrap_or("none");
    let summary = format!(
        "Resolved macro render backend: sourceLanguage={} geometryBackend={}.",
        resolved_context.source_language.as_str(),
        resolved_context.geometry_backend.as_str()
    );
    let details = format!(
        "baseSourceLanguage={}; baseGeometryBackend={}; requestedGeometryBackend={}; configSourceLanguage={}; configGeometryBackend={}; macroDialect={:?}",
        base_context.source_language.as_str(),
        base_context.geometry_backend.as_str(),
        requested,
        configured_context.source_language.as_str(),
        configured_context.geometry_backend.as_str(),
        macro_dialect,
    );
    eprintln!(
        "[MCP] {} backend_resolved session={} agent={} thread={} message={} model={} {} {}",
        phase,
        ctx.session_id,
        ctx.agent_label,
        thread_id.unwrap_or("-"),
        message_id.unwrap_or("-"),
        model_id.unwrap_or("-"),
        summary,
        details,
    );
    push_trace_event_with_conn(
        state,
        conn,
        ctx,
        TraceEvent {
            thread_id: thread_id.map(str::to_string),
            message_id: message_id.map(str::to_string),
            model_id: model_id.map(str::to_string),
            phase,
            kind: "backend_resolved",
            summary,
            details: Some(details),
        },
    );
}

fn resolve_macro_authoring_context(
    base_source_language: crate::models::SourceLanguage,
    base_geometry_backend: crate::models::GeometryBackend,
    macro_dialect: &MacroDialect,
    requested_geometry_backend: Option<crate::models::GeometryBackend>,
) -> AppResult<MacroAuthoringContext> {
    let macro_source_language = infer_macro_source_language(macro_dialect);
    if macro_source_language != base_source_language {
        return Err(AppError::validation(format!(
            "Macro source language mismatch: target model is {}, macro is {}. Fork or create a new version before migrating source language.",
            base_source_language.as_str(),
            macro_source_language.as_str()
        )));
    }

    if let Some(requested) = requested_geometry_backend {
        if base_source_language != crate::models::SourceLanguage::EckyIrV0
            && requested != base_geometry_backend
        {
            return Err(AppError::validation(format!(
                "Geometry backend override is only valid for Ecky source. Target model is {} on {}; requested backend is {}.",
                base_source_language.as_str(),
                base_geometry_backend.as_str(),
                requested.as_str()
            )));
        }
    }

    let geometry_backend = if base_source_language == crate::models::SourceLanguage::EckyIrV0 {
        requested_geometry_backend.unwrap_or(base_geometry_backend)
    } else {
        base_geometry_backend
    };

    Ok(MacroAuthoringContext {
        source_language: base_source_language,
        geometry_backend,
    })
}

fn first_version_authoring_context(
    state: &AppState,
    macro_dialect: &MacroDialect,
    requested_geometry_backend: Option<crate::models::GeometryBackend>,
) -> MacroAuthoringContext {
    match infer_macro_source_language(macro_dialect) {
        crate::models::SourceLanguage::LegacyPython => MacroAuthoringContext {
            source_language: crate::models::SourceLanguage::EckyIrV0,
            geometry_backend: requested_geometry_backend.unwrap_or_else(|| {
                let fallback = configured_authoring_context(state);
                fallback.geometry_backend
            }),
        },
        crate::models::SourceLanguage::Build123d => MacroAuthoringContext {
            source_language: crate::models::SourceLanguage::Build123d,
            geometry_backend: crate::models::GeometryBackend::Build123d,
        },
        crate::models::SourceLanguage::EckyIrV0 => {
            let fallback = configured_authoring_context(state);
            MacroAuthoringContext {
                source_language: crate::models::SourceLanguage::EckyIrV0,
                geometry_backend: requested_geometry_backend.unwrap_or(fallback.geometry_backend),
            }
        }
    }
}

pub async fn handle_macro_preview_render(
    state: &AppState,
    app: &dyn PathResolver,
    req: MacroReplaceRequest,
    ctx: &AgentContext,
) -> AppResult<MacroReplaceResponse> {
    let total_started = Instant::now();
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = async {
        let (working_thread_id, base_design, base_model_manifest) = if let Some(preview) =
            session_render_preview_for_request(
                ctx,
                req.thread_id.as_deref(),
                req.message_id.as_deref(),
            )
        {
            tracked_thread_id = Some(preview.thread_id.clone());
            tracked_message_id = preview
                .base_message_id
                .clone()
                .or_else(|| Some(preview.preview_id.clone()));
            tracked_model_id = Some(preview.artifact_bundle.model_id.clone());
            (
                preview.thread_id,
                preview.design_output,
                Some(preview.model_manifest),
            )
        } else if req.message_id.is_some() {
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

            let base_design = target
                .design
                .ok_or_else(|| AppError::validation("Target has no design output."))?;
            (target.thread_id, base_design, target.model_manifest)
        } else {
            // Bootstrap: thread has no versions yet — use an empty stub as the base.
            let thread_id = req.thread_id.clone().ok_or_else(|| {
                AppError::validation("thread_id is required to create the first version.")
            })?;
            tracked_thread_id = Some(thread_id.clone());
            let stub = DesignOutput {
                title: String::new(),
                version_name: String::new(),
                response: String::new(),
                interaction_mode: InteractionMode::Design,
                macro_code: String::new(),
                macro_dialect: MacroDialect::Legacy,
                engine_kind: crate::models::EngineKind::default(),
                source_language: crate::models::SourceLanguage::default(),
                geometry_backend: crate::models::GeometryBackend::default(),
                ui_spec: UiSpec { fields: vec![] },
                initial_params: std::collections::BTreeMap::new(),
                post_processing: None,
            };
            (thread_id, stub, None)
        };

        let conn = state.db.lock().await;

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "patching_macro",
            "",
        )?;
        mark_live_session_busy(
            state,
            ctx,
            tracked_thread_id
                .as_ref()
                .zip(tracked_message_id.as_ref())
                .map(|(t, m)| {
                    session_target_ref(t.clone(), m.clone(), tracked_model_id.clone())
                }),
            "patching_macro",
            Some("Replacing macro code for the active target.".to_string()),
            None,
            false,
        )
        .await;
        push_trace_event_with_conn(
            state,
            &conn,
            ctx,
            TraceEvent {
                thread_id: tracked_thread_id.clone(),
                message_id: tracked_message_id.clone(),
                model_id: tracked_model_id.clone(),
                phase: "patching_macro",
                kind: "tool_start",
                summary: "Replacing macro code for the active target.".to_string(),
                details: None,
            },
        );

        let requested_macro_dialect = req
            .macro_dialect
            .clone()
            .unwrap_or_else(|| crate::contracts::infer_macro_dialect_from_code(&req.macro_code));
        let is_ir = requested_macro_dialect == MacroDialect::EckyIrV0;
        let framework_parsed = if requested_macro_dialect == MacroDialect::CadFrameworkV1 {
            crate::commands::design::derive_framework_controls(&req.macro_code)?
        } else if requested_macro_dialect == MacroDialect::Legacy {
            crate::commands::design::derive_framework_controls(&req.macro_code)?
        } else {
            None
        };
        let parsed_legacy = if framework_parsed.is_none()
            && requested_macro_dialect != MacroDialect::Build123d
        {
            Some(crate::commands::design::parse_macro_params(req.macro_code.clone()))
        } else {
            None
        };
        let macro_edit_parameter_source = if base_design.macro_code.trim().is_empty() {
            req.parameters
                .clone()
                .unwrap_or_else(|| base_design.initial_params.clone())
        } else {
            base_design.initial_params.clone()
        };
        let (mut ui_spec, mut initial_params, macro_dialect) = if let Some(parsed) = framework_parsed {
            (
                UiSpec {
                    fields: parsed.fields.clone(),
                },
                crate::commands::design::reconcile_framework_params(
                    &parsed.fields,
                    &macro_edit_parameter_source,
                    &parsed.params,
                ),
                MacroDialect::CadFrameworkV1,
            )
        } else if is_ir {
            let parsed = parsed_legacy
                .clone()
                .expect("parse_macro_params should exist for IR path");
            let params = crate::commands::design::reconcile_framework_params(
                &parsed.fields,
                &macro_edit_parameter_source,
                &parsed.params,
            );
            (
                req.ui_spec.clone().unwrap_or(UiSpec {
                    fields: parsed.fields,
                }),
                params,
                MacroDialect::EckyIrV0,
            )
        } else if requested_macro_dialect == MacroDialect::Build123d {
            (
                req.ui_spec
                    .clone()
                    .unwrap_or_else(|| base_design.ui_spec.clone()),
                macro_edit_parameter_source.clone(),
                MacroDialect::Build123d,
            )
        } else {
            let parsed_legacy = parsed_legacy
                .clone()
                .expect("legacy parser should exist when framework parse is absent");
            let mut reconciled_params = parsed_legacy.params.clone();
            for (key, value) in &macro_edit_parameter_source {
                if reconciled_params.contains_key(key.as_str()) {
                    reconciled_params.insert(key.clone(), value.clone());
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
        if let Err(err) = crate::models::validate_design_params(&initial_params, &ui_spec) {
            if macro_dialect == MacroDialect::Legacy && is_param_schema_mismatch(&err) {
                if let Some((next_ui_spec, next_params, heal_report)) = auto_heal_legacy_params(
                    &req.macro_code,
                    &ui_spec,
                    &initial_params,
                    Some(&base_design.initial_params),
                )? {
                    push_trace_event_with_conn(
                        state,
                        &conn,
                        ctx,
                        TraceEvent {
                            thread_id: tracked_thread_id.clone(),
                            message_id: tracked_message_id.clone(),
                            model_id: tracked_model_id.clone(),
                            phase: "patching_macro",
                            kind: "auto_heal_applied",
                            summary: "Reconciled legacy uiSpec and initialParams from parsed macro params."
                                .to_string(),
                            details: Some(format!(
                                "added={:?}; dropped={:?}; carried={:?}",
                                heal_report.added_keys, heal_report.dropped_keys, heal_report.carried_keys
                            )),
                        },
                    );
                    ui_spec = next_ui_spec;
                    initial_params = next_params;
                } else {
                    let parsed_keys = parsed_legacy
                        .as_ref()
                        .map(|parsed| {
                            parsed
                                .params
                                .keys()
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    return Err(AppError::with_details(
                        crate::contracts::AppErrorCode::Validation,
                        err.message,
                        format!(
                            "Legacy param auto-heal failed for session {} on thread {:?}. parsedKeys={:?}",
                            ctx.session_id, tracked_thread_id, parsed_keys
                        ),
                    ));
                }
            } else {
                return Err(err);
            }
        }

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "rendering",
            "",
        )?;
        mark_live_session_busy(
            state,
            ctx,
            tracked_thread_id
                .as_ref()
                .zip(tracked_message_id.as_ref())
                .map(|(t, m)| {
                    session_target_ref(t.clone(), m.clone(), tracked_model_id.clone())
                }),
            "rendering",
            Some("Rendering the updated version.".to_string()),
            None,
            false,
        )
        .await;

        let base_context = if base_design.macro_code.trim().is_empty() {
            first_version_authoring_context(state, &macro_dialect, req.geometry_backend)
        } else {
            MacroAuthoringContext {
                source_language: base_design.source_language,
                geometry_backend: base_design.geometry_backend,
            }
        };
        let authoring_context = resolve_macro_authoring_context(
            base_context.source_language,
            base_context.geometry_backend,
            &macro_dialect,
            req.geometry_backend,
        )?;
        let render_geometry_backend = authoring_context.geometry_backend;
        log_macro_backend_resolution(
            state,
            &conn,
            ctx,
            "macro_preview_render",
            &base_context,
            &macro_dialect,
            req.geometry_backend,
            &authoring_context,
            Some(&working_thread_id),
            tracked_message_id.as_deref(),
            tracked_model_id.as_deref(),
        );

        drop(conn);

        let next_post_processing = req
            .post_processing
            .clone()
            .or_else(|| base_design.post_processing.clone());

        let render_started = Instant::now();
        let artifact_bundle = render::render_model(
            &req.macro_code,
            &initial_params,
            Some(macro_dialect.clone()),
            Some(render_geometry_backend),
            next_post_processing.as_ref(),
            state,
            app,
        )
        .await?;
        push_mcp_profile(
            state,
            ctx,
            "macro_preview_render",
            "render_model",
            render_started,
            Some(&working_thread_id),
            tracked_message_id.as_deref(),
            Some(&artifact_bundle.model_id),
        );
        let manifest_started = Instant::now();
        let model_manifest =
            crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
        let model_manifest = carry_forward_semantic_manifest(
            base_model_manifest.as_ref(),
            model_manifest,
            &artifact_bundle,
        );
        let model_manifest = crate::model_runtime::write_model_manifest(
            app,
            &artifact_bundle.model_id,
            &model_manifest,
        )?;
        tracked_model_id = Some(artifact_bundle.model_id.clone());
        push_mcp_profile(
            state,
            ctx,
            "macro_preview_render",
            "manifest_read_carry_write",
            manifest_started,
            Some(&working_thread_id),
            tracked_message_id.as_deref(),
            tracked_model_id.as_deref(),
        );

        let engine_kind = authoring_context.source_language.to_engine_kind();
        let design_output = DesignOutput {
            title: base_design.title.clone(),
            version_name: String::new(),
            response: "Draft update via macro replacement.".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: req.macro_code.clone(),
            macro_dialect,
            engine_kind,
            source_language: authoring_context.source_language,
            geometry_backend: render_geometry_backend,
            ui_spec: ui_spec.clone(),
            initial_params: initial_params.clone(),
            post_processing: next_post_processing,
        };

        let sv = crate::services::structural_verification::verify_structure(
            &artifact_bundle,
            &model_manifest,
        );
        let store_started = Instant::now();
        let preview = store_session_render_preview(
            state,
            app,
            ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: working_thread_id.clone(),
                base_message_id: tracked_message_id.clone(),
                design_output: design_output.clone(),
                artifact_bundle: artifact_bundle.clone(),
                model_manifest: model_manifest.clone(),
                draft_feedback: Some(draft_feedback_from_structural_verification(&sv)),
            },
        )
        .await?;
        push_mcp_profile(
            state,
            ctx,
            "macro_preview_render",
            "store_preview",
            store_started,
            Some(&working_thread_id),
            Some(&preview.preview_id),
            Some(&artifact_bundle.model_id),
        );
        tracked_message_id = Some(preview.preview_id.clone());
        Ok(MacroReplaceResponse {
            thread_id: working_thread_id,
            message_id: preview.preview_id,
            macro_code: req.macro_code.clone(),
            ui_spec,
            initial_params,
            artifact_digest: artifact_bundle_digest(&artifact_bundle),
            artifact_bundle,
            model_manifest,
            structural_verification: Some(sv),
        })
    }
    .await;

    settle_live_render_phase(
        state,
        ctx,
        tracked_thread_id.as_deref(),
        tracked_message_id.as_deref(),
        tracked_model_id.clone(),
        &result,
    )
    .await;

    push_mcp_profile(
        state,
        ctx,
        "macro_preview_render",
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
                response_text_updated: format!("{} updated the MCP preview commit.", ctx.agent_label),
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

#[derive(Debug, Clone)]
struct SemanticTargetState {
    thread_id: String,
    message_id: String,
    design_output: DesignOutput,
    artifact_bundle: ArtifactBundle,
    model_manifest: ModelManifest,
}

fn resolve_semantic_target(
    conn: &rusqlite::Connection,
    app: &dyn PathResolver,
    thread_id: Option<String>,
    message_id: Option<String>,
) -> AppResult<SemanticTargetState> {
    let target =
        crate::services::target::resolve_editable_target(conn, app, thread_id, message_id)?;
    let design_output = target.design_output;
    let artifact_bundle = target
        .artifact_bundle
        .ok_or_else(|| AppError::validation("Target has no artifact bundle."))?;
    let model_manifest = target
        .model_manifest
        .ok_or_else(|| AppError::validation("Target has no model manifest."))?;

    if model_manifest.source_kind != ModelSourceKind::Generated {
        return Err(AppError::validation(
            "Semantic knob/view MCP tools currently support generated models only.",
        ));
    }

    crate::models::validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;

    Ok(SemanticTargetState {
        thread_id: target.thread_id,
        message_id: target.message_id,
        design_output,
        artifact_bundle,
        model_manifest,
    })
}

#[allow(clippy::too_many_arguments)]
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
    crate::models::validate_model_runtime_bundle(&next_manifest, &target.artifact_bundle)?;

    let mut design_output = target.design_output.clone();
    if let Some(next_title) = title.clone() {
        design_output.title = next_title;
    }
    if let Some(next_version_name) = version_name.clone() {
        design_output.version_name = next_version_name;
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
            model_id: Some(target.artifact_bundle.model_id.clone()),
            design_output: design_output.clone(),
            artifact_bundle: Some(target.artifact_bundle.clone()),
            model_manifest: Some(next_manifest.clone()),
            updated_at: now_secs(),
            response_text_created: response_text.clone(),
            response_text_updated: response_text,
            preserve_existing_title: title.is_none(),
            preserve_existing_version_name: version_name.is_none(),
            force_create_new_message: false,
            announce_created_working_version: false,
        },
    )
    .await?;
    let agent_origin = save_result.agent_origin.clone();
    let artifact_digest = artifact_bundle_digest(&target.artifact_bundle);

    Ok(SemanticManifestMutationResponse {
        thread_id: target.thread_id,
        message_id: save_result.message_id,
        model_id: target.artifact_bundle.model_id.clone(),
        title: design_output.title,
        version_name: save_result.version_name,
        artifact_digest,
        control_primitive_count: next_manifest.control_primitives.len(),
        relation_count: next_manifest.control_relations.len(),
        view_count: next_manifest.control_views.len(),
        advisory_count: next_manifest.advisories.len(),
        measurement_annotation_count: next_manifest.measurement_annotations.len(),
        part_count: next_manifest.parts.len(),
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

fn normalize_llm_measurement_annotation(
    annotation: MeasurementAnnotation,
) -> AppResult<MeasurementAnnotation> {
    let annotation_id = annotation.annotation_id.trim();
    if annotation_id.is_empty() {
        return Err(AppError::validation(
            "Measurement annotation id cannot be empty.",
        ));
    }

    let label = annotation.label.trim();
    if label.is_empty() {
        return Err(AppError::validation(
            "Measurement annotation label cannot be empty.",
        ));
    }

    Ok(MeasurementAnnotation {
        annotation_id: annotation_id.to_string(),
        label: label.to_string(),
        basis: annotation.basis,
        axis: annotation.axis,
        parameter_keys: annotation.parameter_keys,
        primitive_ids: annotation.primitive_ids,
        target_ids: annotation.target_ids,
        guide_id: annotation.guide_id.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        }),
        explanation: annotation.explanation.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        }),
        formula_hint: annotation.formula_hint.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        }),
        source: MeasurementAnnotationSource::Llm,
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
            "Reading semantic manifest summary.",
        )?;

        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
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
            message_id: target.message_id,
            title: Some(target.design_output.title),
            version_name: Some(target.design_output.version_name),
            control_primitive_count: target.model_manifest.control_primitives.len(),
            relation_count: target.model_manifest.control_relations.len(),
            view_count: target.model_manifest.control_views.len(),
            advisory_count: target.model_manifest.advisories.len(),
            measurement_annotation_count: target.model_manifest.measurement_annotations.len(),
            part_count: target.model_manifest.parts.len(),
        })
    })();

    if let Err(err) = &result {
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

pub async fn handle_semantic_manifest_detail_get(
    state: &AppState,
    app: &dyn PathResolver,
    req: SemanticManifestDetailRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticManifestDetailResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let conn = state.db.lock().await;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = None;

    let result = (|| -> AppResult<SemanticManifestDetailResponse> {
        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            None,
            "reading",
            format!(
                "Reading semantic manifest detail section {:?}.",
                req.section
            ),
        )?;

        let target =
            resolve_semantic_target(&conn, app, req.thread_id.clone(), req.message_id.clone())?;

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
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

        let (
            control_primitives,
            control_relations,
            control_views,
            advisories,
            measurement_annotations,
            parts,
        ) = match req.section {
            SemanticManifestSection::ControlPrimitives => (
                Some(target.model_manifest.control_primitives),
                None,
                None,
                None,
                None,
                None,
            ),
            SemanticManifestSection::ControlRelations => (
                None,
                Some(target.model_manifest.control_relations),
                None,
                None,
                None,
                None,
            ),
            SemanticManifestSection::ControlViews => (
                None,
                None,
                Some(target.model_manifest.control_views),
                None,
                None,
                None,
            ),
            SemanticManifestSection::Advisories => (
                None,
                None,
                None,
                Some(target.model_manifest.advisories),
                None,
                None,
            ),
            SemanticManifestSection::MeasurementAnnotations => (
                None,
                None,
                None,
                None,
                Some(target.model_manifest.measurement_annotations),
                None,
            ),
            SemanticManifestSection::Parts => (
                None,
                None,
                None,
                None,
                None,
                Some(target.model_manifest.parts),
            ),
        };

        Ok(SemanticManifestDetailResponse {
            thread_id: target.thread_id,
            message_id: target.message_id,
            section: req.section,
            control_primitives,
            control_relations,
            control_views,
            advisories,
            measurement_annotations,
            parts,
        })
    })();

    if let Err(err) = &result {
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
        drop(conn);
        let target = resolve_turn_working_semantic_target(
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
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
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
        drop(conn);
        let target = resolve_turn_working_semantic_target(
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
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
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
        drop(conn);
        let target = resolve_turn_working_semantic_target(
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
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
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
        drop(conn);
        let target = resolve_turn_working_semantic_target(
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
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
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

pub async fn handle_measurement_annotation_save(
    state: &AppState,
    app: &dyn PathResolver,
    req: MeasurementAnnotationSaveRequest,
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
        drop(conn);
        let target = resolve_turn_working_semantic_target(
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
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Saving measurement annotation.",
        )?;

        let next_annotation = normalize_llm_measurement_annotation(req.annotation)?;
        let next_annotation_id = next_annotation.annotation_id.clone();
        let mut next_manifest = target.model_manifest.clone();
        if let Some(existing) = next_manifest
            .measurement_annotations
            .iter_mut()
            .find(|entry| entry.annotation_id == next_annotation_id)
        {
            *existing = next_annotation;
        } else {
            next_manifest.measurement_annotations.push(next_annotation);
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
            format!(
                "{} updated a measurement annotation via MCP.",
                ctx.agent_label
            ),
        )
        .await?;
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
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

pub async fn handle_measurement_annotation_delete(
    state: &AppState,
    app: &dyn PathResolver,
    req: MeasurementAnnotationDeleteRequest,
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
        tracked_message_id = Some(target.message_id.clone());
        tracked_model_id = Some(target.artifact_bundle.model_id.clone());

        persist_agent_session(
            &conn,
            ctx,
            tracked_thread_id.clone(),
            tracked_message_id.clone(),
            tracked_model_id.clone(),
            "saving_version",
            "Deleting measurement annotation.",
        )?;

        let mut next_manifest = target.model_manifest.clone();
        let annotation_id = req.annotation_id;
        next_manifest
            .measurement_annotations
            .retain(|entry| entry.annotation_id != annotation_id);

        drop(conn);

        let response = save_semantic_manifest_version(
            state,
            app,
            ctx,
            target,
            next_manifest,
            req.title,
            req.version_name,
            format!(
                "{} deleted a measurement annotation via MCP.",
                ctx.agent_label
            ),
        )
        .await?;
        tracked_message_id = Some(response.message_id.clone());
        tracked_model_id = Some(response.model_id.clone());

        Ok(response)
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

// ── Structural verification MCP handlers ────────────────────────────────────

pub fn handle_verify_generated_model(
    _state: &AppState,
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    model_id: &str,
    _original_prompt: &str,
) -> AppResult<VerifyGeneratedModelResponse> {
    let bundle = crate::model_runtime::read_artifact_bundle(app, model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(app, model_id)?;
    let artifact_digest = artifact_bundle_digest(&bundle);
    let result =
        crate::services::author_verification_foundation::verify_structure_with_author_verification(
            &bundle, &manifest,
        );
    Ok(VerifyGeneratedModelResponse {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        model_id: model_id.to_string(),
        artifact_digest,
        result,
    })
}

pub fn handle_structural_verification_summary(
    _state: &AppState,
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    model_id: &str,
) -> AppResult<StructuralVerificationSummaryResponse> {
    let bundle = crate::model_runtime::read_artifact_bundle(app, model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(app, model_id)?;
    let artifact_digest = artifact_bundle_digest(&bundle);
    let result =
        crate::services::author_verification_foundation::verify_structure_with_author_verification(
            &bundle, &manifest,
        );
    Ok(StructuralVerificationSummaryResponse {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        model_id: model_id.to_string(),
        artifact_digest,
        passed: result.passed,
        summary: result.summary,
        issue_count: result.issues.len(),
        verifier_status: result.verifier_status,
        verifier_source: result.verifier_source,
    })
}

fn printability_manifest_source_anchor(manifest: &ModelManifest) -> Option<String> {
    let graph = manifest.feature_graph.as_ref()?;
    let anchors = graph
        .nodes
        .iter()
        .filter_map(printability_feature_node_anchor)
        .collect::<Vec<_>>();

    match anchors.as_slice() {
        [anchor] => Some(anchor.clone()),
        _ => None,
    }
}

fn printability_manifest_risk_anchor(
    manifest: &ModelManifest,
) -> Option<crate::services::printability::PrintabilityRiskAnchor> {
    let graph = manifest.feature_graph.as_ref()?;
    let mut anchors = graph
        .nodes
        .iter()
        .filter_map(printability_feature_node_risk_anchor)
        .collect::<Vec<_>>();
    let mut anchor = match anchors.len() {
        1 => anchors.swap_remove(0),
        _ => return None,
    };
    let has_feature_id = anchor
        .feature_id
        .as_ref()
        .is_some_and(|feature_id| !feature_id.trim().is_empty());
    if !has_feature_id {
        return None;
    }
    if anchor.target_ids.is_empty() {
        return Some(anchor);
    }
    anchor.target_ids.dedup();
    anchor.stable_node_keys.dedup();
    Some(anchor)
}

fn printability_feature_node_risk_anchor(
    node: &crate::models::FeatureNode,
) -> Option<crate::services::printability::PrintabilityRiskAnchor> {
    let feature_id = node.feature_id.trim();
    if feature_id.is_empty() {
        return None;
    }

    let mut target_ids = Vec::new();
    for output_ref in &node.output_refs {
        for target_id in &output_ref.target_ids {
            let trimmed = target_id.trim();
            if !trimmed.is_empty() {
                target_ids.push(trimmed.to_string());
            }
        }
    }
    for port in &node.ports {
        for target_id in &port.target_ids {
            let trimmed = target_id.trim();
            if !trimmed.is_empty() {
                target_ids.push(trimmed.to_string());
            }
        }
    }
    target_ids.dedup();

    let mut stable_node_keys = target_ids
        .iter()
        .filter_map(|target_id| printability_stable_node_key_from_target_id(target_id))
        .collect::<Vec<_>>();
    stable_node_keys.dedup();

    Some(crate::services::printability::PrintabilityRiskAnchor {
        feature_id: Some(feature_id.to_string()),
        target_ids,
        stable_node_keys,
    })
}

fn printability_stable_node_key_from_target_id(target_id: &str) -> Option<String> {
    let (_, remainder) = target_id.split_once(":stable-node-key:")?;
    let (stable_node_key, _) = remainder
        .split_once(":edge:")
        .or_else(|| remainder.split_once(":face:"))?;
    let stable_node_key = stable_node_key.trim();
    (!stable_node_key.is_empty()).then(|| stable_node_key.to_string())
}

fn printability_feature_node_anchor(node: &crate::models::FeatureNode) -> Option<String> {
    let feature_id = node.feature_id.trim();
    if feature_id.is_empty() {
        return None;
    }

    if let Some(source_anchor) = node
        .source_ref
        .as_ref()
        .and_then(printability_source_ref_anchor)
    {
        return Some(format!("feature:{feature_id}@{source_anchor}"));
    }

    Some(format!("feature:{feature_id}"))
}

fn printability_source_ref_anchor(source_ref: &crate::models::SourceRef) -> Option<String> {
    let source_id = source_ref
        .source_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let path = source_ref
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let span = match (source_ref.start_byte, source_ref.end_byte) {
        (Some(start), Some(end)) => Some(format!("{start}-{end}")),
        (Some(start), None) => Some(start.to_string()),
        (None, Some(end)) => Some(format!("0-{end}")),
        (None, None) => None,
    };

    let mut parts = Vec::new();
    if let Some(source_id) = source_id {
        parts.push(source_id.to_string());
    }
    if let Some(path) = path {
        parts.push(path.to_string());
    }
    if let Some(span) = span {
        parts.push(span);
    }

    (!parts.is_empty()).then(|| format!("source:{}", parts.join(":")))
}

pub fn handle_printability_analyze(
    _state: &AppState,
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    model_id: &str,
) -> AppResult<PrintabilityAnalyzeResponse> {
    let bundle = crate::model_runtime::read_artifact_bundle(app, model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(app, model_id)?;
    let artifact_digest = artifact_bundle_digest(&bundle);
    if bundle.preview_stl_path.trim().is_empty() {
        return Err(AppError::validation(
            "Artifact bundle has no preview STL path.",
        ));
    }
    let mut analysis = crate::services::printability::analyze_stl_path(std::path::Path::new(
        &bundle.preview_stl_path,
    ))
    .map_err(|err| AppError::parse(err.to_string()))?;
    crate::services::printability::enrich_transform_suggestions_with_source_anchor(
        &mut analysis,
        printability_manifest_source_anchor(&manifest),
    );
    crate::services::printability::enrich_transform_suggestions_with_risk_anchor(
        &mut analysis,
        printability_manifest_risk_anchor(&manifest),
    );

    Ok(PrintabilityAnalyzeResponse {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        model_id: model_id.to_string(),
        artifact_digest,
        preview_stl_path: bundle.preview_stl_path,
        analysis,
    })
}

pub fn handle_printability_transform_recipes_get(
    _state: &AppState,
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    model_id: &str,
) -> AppResult<PrintabilityTransformRecipesGetResponse> {
    let bundle = crate::model_runtime::read_artifact_bundle(app, model_id)?;
    let manifest = crate::model_runtime::read_model_manifest(app, model_id)?;
    let artifact_digest = artifact_bundle_digest(&bundle);
    if bundle.preview_stl_path.trim().is_empty() {
        return Err(AppError::validation(
            "Artifact bundle has no preview STL path.",
        ));
    }
    let mut analysis = crate::services::printability::analyze_stl_path(std::path::Path::new(
        &bundle.preview_stl_path,
    ))
    .map_err(|err| AppError::parse(err.to_string()))?;
    crate::services::printability::enrich_transform_suggestions_with_source_anchor(
        &mut analysis,
        printability_manifest_source_anchor(&manifest),
    );
    crate::services::printability::enrich_transform_suggestions_with_risk_anchor(
        &mut analysis,
        printability_manifest_risk_anchor(&manifest),
    );
    let recipes = crate::services::printability::supportless_fdm_transform_recipes(&analysis);

    Ok(PrintabilityTransformRecipesGetResponse {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        model_id: model_id.to_string(),
        artifact_digest,
        preview_stl_path: bundle.preview_stl_path,
        recipes,
    })
}

pub async fn handle_semantic_transform_preview(
    state: &AppState,
    app: &dyn PathResolver,
    req: SemanticTransformPreviewRequest,
    ctx: &AgentContext,
) -> AppResult<SemanticTransformPreviewResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let mut tracked_thread_id = req.thread_id.clone();
    let mut tracked_message_id = req.message_id.clone();
    let mut tracked_model_id = req.model_id.clone();

    let result = async {
        let conn = state.db.lock().await;
        let target = crate::services::target::resolve_editable_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?;
        drop(conn);

        tracked_thread_id = Some(target.thread_id.clone());
        tracked_message_id = Some(target.message_id.clone());
        let requested_model_id = req
            .model_id
            .clone()
            .or_else(|| target.model_id())
            .ok_or_else(|| AppError::validation("Target has no artifact modelId."))?;
        tracked_model_id = Some(requested_model_id.clone());

        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                ctx,
                tracked_thread_id.clone(),
                tracked_message_id.clone(),
                tracked_model_id.clone(),
                "patching_macro",
                "Preparing semantic transform preview.",
            )?;
        }

        let design_output = target.design_output;
        let (bundle, manifest) =
            crate::model_runtime::read_runtime_bundle(app, &requested_model_id)?;
        crate::models::validate_model_runtime_bundle(&manifest, &bundle)?;
        validate_semantic_transform_artifact_guard(&req.expected_artifact, &bundle)?;
        validate_semantic_transform_ecky_source(&design_output, &bundle, &manifest)?;

        match req.action_kind {
            crate::services::printability::SupportlessFdmRecipeActionKind::Reorient => {}
            crate::services::printability::SupportlessFdmRecipeActionKind::Chamfer => {
                return Err(AppError::validation(
                    "semantic_transform_preview actionKind=chamfer is unsupported.",
                ));
            }
            crate::services::printability::SupportlessFdmRecipeActionKind::Split => {
                return Err(AppError::validation(
                    "semantic_transform_preview actionKind=split is unsupported.",
                ));
            }
            crate::services::printability::SupportlessFdmRecipeActionKind::Relief => {
                return Err(AppError::validation(
                    "semantic_transform_preview actionKind=relief is unsupported.",
                ));
            }
            crate::services::printability::SupportlessFdmRecipeActionKind::Clearance => {
                return Err(AppError::validation(
                    "semantic_transform_preview actionKind=clearance is unsupported.",
                ));
            }
        }

        if bundle.preview_stl_path.trim().is_empty() {
            return Err(AppError::validation(
                "Artifact bundle has no preview STL path.",
            ));
        }
        let mut analysis = crate::services::printability::analyze_stl_path(std::path::Path::new(
            &bundle.preview_stl_path,
        ))
        .map_err(|err| AppError::parse(err.to_string()))?;
        crate::services::printability::enrich_transform_suggestions_with_source_anchor(
            &mut analysis,
            printability_manifest_source_anchor(&manifest),
        );
        crate::services::printability::enrich_transform_suggestions_with_risk_anchor(
            &mut analysis,
            printability_manifest_risk_anchor(&manifest),
        );
        let recipes = crate::services::printability::supportless_fdm_transform_recipes(&analysis);
        let recipe = recipes
            .iter()
            .find(|recipe| {
                recipe.recipe_id == req.recipe_id && recipe.action_kind == req.action_kind
            })
            .ok_or_else(|| {
                AppError::validation(format!(
                    "No supportless-FDM recipe matched recipeId={} actionKind={}.",
                    req.recipe_id,
                    semantic_transform_action_kind_label(req.action_kind)
                ))
            })?;
        let rotation_degrees = recipe
            .rotation_degrees
            .ok_or_else(|| AppError::validation("Reorient recipe is missing rotationDegrees."))?;

        let source_digest = crate::mcp::macro_buffer::source_digest(&design_output.macro_code);
        let next_source = crate::services::printability::reorient_ecky_source(
            &design_output.macro_code,
            rotation_degrees,
        )
        .map_err(AppError::validation)?;
        crate::ecky_scheme::compile_to_core_program(&next_source)
            .map_err(|err| AppError::validation(err.to_string()))?;
        let new_source_digest = crate::mcp::macro_buffer::source_digest(&next_source);

        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                ctx,
                tracked_thread_id.clone(),
                tracked_message_id.clone(),
                tracked_model_id.clone(),
                "rendering",
                "Rendering semantic transform preview.",
            )?;
        }

        let artifact_bundle = render::render_model(
            &next_source,
            &design_output.initial_params,
            Some(MacroDialect::EckyIrV0),
            Some(design_output.geometry_backend),
            design_output.post_processing.as_ref(),
            state,
            app,
        )
        .await?;
        let model_manifest =
            crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
        let model_manifest =
            carry_forward_semantic_manifest(Some(&manifest), model_manifest, &artifact_bundle);
        let model_manifest = crate::model_runtime::write_model_manifest(
            app,
            &artifact_bundle.model_id,
            &model_manifest,
        )?;
        tracked_model_id = Some(artifact_bundle.model_id.clone());

        let mut preview_design = design_output.clone();
        preview_design.version_name.clear();
        preview_design.response = format!(
            "Draft semantic transform preview for supportless-FDM recipe {}.",
            req.recipe_id
        );
        preview_design.macro_code = next_source;
        preview_design.macro_dialect = MacroDialect::EckyIrV0;
        preview_design.engine_kind = crate::models::EngineKind::EckyIrV0;
        preview_design.source_language = crate::models::SourceLanguage::EckyIrV0;
        preview_design.geometry_backend = artifact_bundle.geometry_backend;

        let sv = crate::services::structural_verification::verify_structure(
            &artifact_bundle,
            &model_manifest,
        );
        let preview = store_session_render_preview(
            state,
            app,
            ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: target.thread_id.clone(),
                base_message_id: Some(target.message_id.clone()),
                design_output: preview_design,
                artifact_bundle: artifact_bundle.clone(),
                model_manifest,
                draft_feedback: Some(draft_feedback_from_structural_verification(&sv)),
            },
        )
        .await?;
        tracked_message_id = Some(preview.preview_id.clone());

        Ok(SemanticTransformPreviewResponse {
            thread_id: target.thread_id,
            base_message_id: target.message_id,
            preview_id: preview.preview_id,
            model_id: artifact_bundle.model_id.clone(),
            recipe_id: recipe.recipe_id.clone(),
            action_kind: recipe.action_kind,
            source_digest,
            new_source_digest,
            preview_support_status: recipe.preview_support_status,
            apply_support_status: recipe.apply_support_status,
            artifact_digest: artifact_bundle_digest(&artifact_bundle),
        })
    }
    .await;

    settle_live_render_phase(
        state,
        ctx,
        tracked_thread_id.as_deref(),
        tracked_message_id.as_deref(),
        tracked_model_id.clone(),
        &result,
    )
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

fn validate_semantic_transform_artifact_guard(
    expected: &SemanticTransformArtifactGuard,
    bundle: &ArtifactBundle,
) -> AppResult<()> {
    if expected.model_id != bundle.model_id
        || expected.preview_stl_path != bundle.preview_stl_path
        || expected.content_hash != bundle.content_hash
    {
        return Err(AppError::validation(
            "semantic_transform_preview artifact guard mismatch: expected modelId, previewStlPath, and contentHash must match current runtime bundle.",
        ));
    }
    Ok(())
}

fn validate_semantic_transform_ecky_source(
    design_output: &DesignOutput,
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> AppResult<()> {
    if design_output.source_language != crate::models::SourceLanguage::EckyIrV0
        || bundle.source_language != crate::models::SourceLanguage::EckyIrV0
        || manifest.source_language != crate::models::SourceLanguage::EckyIrV0
    {
        return Err(AppError::validation(
            "semantic_transform_preview supports sourceLanguage=ecky .ecky source only.",
        ));
    }

    let has_ecky_source_path = bundle
        .macro_path
        .as_deref()
        .and_then(|path| std::path::Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("ecky"))
        .unwrap_or(false);
    if !has_ecky_source_path {
        return Err(AppError::validation(
            "semantic_transform_preview supports sourceLanguage=ecky .ecky source only.",
        ));
    }
    Ok(())
}

fn semantic_transform_action_kind_label(
    action_kind: crate::services::printability::SupportlessFdmRecipeActionKind,
) -> &'static str {
    match action_kind {
        crate::services::printability::SupportlessFdmRecipeActionKind::Reorient => "reorient",
        crate::services::printability::SupportlessFdmRecipeActionKind::Chamfer => "chamfer",
        crate::services::printability::SupportlessFdmRecipeActionKind::Split => "split",
        crate::services::printability::SupportlessFdmRecipeActionKind::Relief => "relief",
        crate::services::printability::SupportlessFdmRecipeActionKind::Clearance => "clearance",
    }
}

pub async fn handle_compare_models(
    app: &dyn PathResolver,
    req: CompareModelsRequest,
) -> AppResult<CompareModelsResponse> {
    let script_path = resolve_resource_path(
        app,
        "server/compare_metric.py",
        &["../server/compare_metric.py", "server/compare_metric.py"],
    )?;

    let output = std::process::Command::new("python3")
        .arg(script_path)
        .arg(&req.ref_path)
        .arg(&req.gen_path)
        .output()
        .map_err(|e| {
            AppError::new(
                AppErrorCode::Internal,
                format!("Failed to execute comparison script: {}", e),
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Err(AppError::new(
            AppErrorCode::Internal,
            format!("Comparison script failed: {}\n{}", stdout, stderr),
        ));
    }

    // Parse output lines
    let mut ref_vol = 0.0;
    let mut gen_vol = 0.0;
    let mut vol_diff = 0.0;
    let mut bb_err = 0.0;
    let mut status = "UNKNOWN".to_string();

    for line in stdout.lines() {
        if line.starts_with("Reference Volume:") {
            ref_vol = parse_metric(line);
        } else if line.starts_with("Generated Volume:") {
            gen_vol = parse_metric(line);
        } else if line.starts_with("Volume Difference:") {
            vol_diff = parse_metric(line);
        } else if line.starts_with("Bounding Box Match Error:") {
            bb_err = parse_metric(line);
        } else if line.starts_with("Status:") {
            status = line
                .strip_prefix("Status: ")
                .unwrap_or(line)
                .trim()
                .to_string();
        }
    }

    Ok(CompareModelsResponse {
        reference_volume: ref_vol,
        generated_volume: gen_vol,
        volume_difference_percent: vol_diff,
        bounding_box_match_error: bb_err,
        status,
        details: stdout.into_owned(),
    })
}

fn parse_metric(line: &str) -> f64 {
    line.split(':')
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        AppErrorCode, Config, ControlPrimitiveKind, ControlRelationMode, ControlViewScope,
        DesignParams, DocumentMetadata, EnrichmentStatus, McpConfig, MeasurementAnnotation,
        MeasurementAnnotationSource, MeasurementAxis, MeasurementBasis, Message, MessageRole,
        MessageStatus, MessageVisualKind, ParamValue, UiField,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    struct TestPathResolver {
        root: PathBuf,
    }

    impl PathResolver for TestPathResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, path: &str) -> Option<PathBuf> {
            Some(self.root.join(path))
        }
    }

    fn test_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-mcp-{}-{}", name, Uuid::new_v4()))
    }

    fn write_executable(path: &std::path::Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
    }

    fn write_closed_tetra_binary_stl(path: &std::path::Path) {
        let triangles = [
            [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0f32, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
            [[0.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [[1.0f32, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
        ];

        write_binary_stl(path, &triangles);
    }

    fn write_binary_stl(path: &std::path::Path, triangles: &[[[f32; 3]; 3]]) {
        let mut bytes = vec![0u8; 80];
        bytes.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
        for triangle in triangles.iter().copied() {
            for normal_component in [0.0f32, 0.0, 0.0] {
                bytes.extend_from_slice(&normal_component.to_le_bytes());
            }
            for vertex in triangle {
                for component in vertex {
                    bytes.extend_from_slice(&component.to_le_bytes());
                }
            }
            bytes.extend_from_slice(&0u16.to_le_bytes());
        }
        fs::write(path, bytes).unwrap();
    }

    fn test_config() -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            default_engine_kind: crate::models::EngineKind::Freecad,
            default_geometry_backend: crate::models::GeometryBackend::Freecad,
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
        }
    }

    fn test_session_id() -> String {
        // Globals like MACRO_BUFFERS and SESSION_RENDER_PREVIEWS are keyed by
        // session id; a per-test-thread nonce keeps tests from contaminating
        // each other through them while staying stable within one test.
        thread_local! {
            static NONCE: String = uuid::Uuid::new_v4().simple().to_string();
        }
        NONCE.with(|nonce| format!("session-1-{nonce}"))
    }

    fn test_session_id_other() -> String {
        thread_local! {
            static NONCE: String = uuid::Uuid::new_v4().simple().to_string();
        }
        NONCE.with(|nonce| format!("session-2-{nonce}"))
    }

    fn test_ctx() -> AgentContext {
        AgentContext {
            session_id: test_session_id(),
            client_kind: "http".to_string(),
            host_label: "Claude Code".to_string(),
            agent_label: "claude".to_string(),
            llm_model_id: None,
            llm_model_label: Some("Claude Sonnet".to_string()),
        }
    }

    fn test_ctx_other() -> AgentContext {
        AgentContext {
            session_id: test_session_id_other(),
            client_kind: "http".to_string(),
            host_label: "Codex".to_string(),
            agent_label: "codex".to_string(),
            llm_model_id: None,
            llm_model_label: Some("GPT-5.4".to_string()),
        }
    }

    #[test]
    fn infer_macro_source_language_maps_dialect_to_authoring_language() {
        assert_eq!(
            infer_macro_source_language(&MacroDialect::Legacy),
            crate::models::SourceLanguage::LegacyPython
        );
        assert_eq!(
            infer_macro_source_language(&MacroDialect::CadFrameworkV1),
            crate::models::SourceLanguage::LegacyPython
        );
        assert_eq!(
            infer_macro_source_language(&MacroDialect::EckyIrV0),
            crate::models::SourceLanguage::EckyIrV0
        );
        assert_eq!(
            infer_macro_source_language(&MacroDialect::Build123d),
            crate::models::SourceLanguage::Build123d
        );
    }

    #[test]
    fn macro_replacement_authoring_context_rejects_source_language_change() {
        let err = resolve_macro_authoring_context(
            crate::models::SourceLanguage::LegacyPython,
            crate::models::GeometryBackend::Freecad,
            &MacroDialect::EckyIrV0,
            None,
        )
        .expect_err("ecky macro should not replace legacy python model source");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("source language"));
    }

    #[test]
    fn first_version_authoring_context_rejects_raw_freecad_by_policy() {
        let conn = crate::db::init_db(&test_db_path("mcp-first-version-policy")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let base = first_version_authoring_context(&state, &MacroDialect::Legacy, None);

        assert_eq!(
            base.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert_eq!(
            base.geometry_backend,
            crate::models::GeometryBackend::Freecad
        );

        let err = resolve_macro_authoring_context(
            base.source_language,
            base.geometry_backend,
            &MacroDialect::Legacy,
            None,
        )
        .expect_err("raw FreeCAD macro must not bootstrap a new MCP version");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("source language"));
    }

    #[test]
    fn macro_replacement_authoring_context_rejects_non_ecky_backend_override() {
        let err = resolve_macro_authoring_context(
            crate::models::SourceLanguage::Build123d,
            crate::models::GeometryBackend::Build123d,
            &MacroDialect::Build123d,
            Some(crate::models::GeometryBackend::Freecad),
        )
        .expect_err("non-ecky model must follow version backend setting");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("Geometry backend override"));
    }

    #[test]
    fn macro_replacement_authoring_context_allows_ecky_backend_override() {
        let context = resolve_macro_authoring_context(
            crate::models::SourceLanguage::EckyIrV0,
            crate::models::GeometryBackend::EckyRust,
            &MacroDialect::EckyIrV0,
            Some(crate::models::GeometryBackend::Build123d),
        )
        .expect("ecky source should allow geometry backend override");

        assert_eq!(
            context.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert_eq!(
            context.geometry_backend,
            crate::models::GeometryBackend::Build123d
        );
    }

    fn sample_ui_spec() -> UiSpec {
        UiSpec {
            fields: vec![
                UiField::Range {
                    key: "diameter".to_string(),
                    label: "Diameter".to_string(),
                    min: Some(10.0),
                    max: Some(200.0),
                    step: Some(1.0),
                    min_from: None,
                    max_from: None,
                    frozen: false,
                },
                UiField::Select {
                    key: "mount".to_string(),
                    label: "Mount".to_string(),
                    options: vec![crate::contracts::SelectOption {
                        label: "Inner".to_string(),
                        value: crate::contracts::SelectValue::String("inner".to_string()),
                    }],
                    frozen: false,
                },
                UiField::Checkbox {
                    key: "lip".to_string(),
                    label: "Lip".to_string(),
                    frozen: false,
                },
            ],
        }
    }

    fn sample_params() -> DesignParams {
        BTreeMap::from([
            ("diameter".to_string(), ParamValue::Number(130.0)),
            ("mount".to_string(), ParamValue::String("inner".to_string())),
            ("lip".to_string(), ParamValue::Boolean(true)),
        ])
    }

    fn sample_design(title: &str, version_name: &str, macro_code: &str) -> DesignOutput {
        DesignOutput {
            title: title.to_string(),
            version_name: version_name.to_string(),
            response: "ok".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: macro_code.to_string(),
            macro_dialect: MacroDialect::Legacy,
            engine_kind: crate::models::EngineKind::Freecad,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            ui_spec: sample_ui_spec(),
            initial_params: sample_params(),
            post_processing: Some(crate::contracts::PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![],
            }),
        }
    }

    fn sample_bundle(model_id: &str, preview_name: &str) -> ArtifactBundle {
        ArtifactBundle {
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::Freecad,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            content_hash: format!("hash-{}", model_id),
            artifact_version: 1,
            fcstd_path: format!("/tmp/{}.FCStd", model_id),
            manifest_path: format!("/tmp/{}.json", model_id),
            macro_path: Some(format!("/tmp/{}.py", model_id)),
            preview_stl_path: format!("/tmp/{}", preview_name),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        }
    }

    fn sample_manifest(model_id: &str) -> ModelManifest {
        ModelManifest {
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: crate::models::EngineKind::Freecad,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            document: DocumentMetadata {
                document_name: "Doc".to_string(),
                document_label: "Doc".to_string(),
                source_path: None,
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: vec![crate::models::PartBinding {
                part_id: "body".to_string(),
                freecad_object_name: "Body".to_string(),
                label: "Body".to_string(),
                kind: "solid".to_string(),
                semantic_role: None,
                viewer_asset_path: None,
                viewer_node_ids: vec!["body".to_string()],
                parameter_keys: Vec::new(),
                editable: true,
                bounds: None,
                volume: None,
                area: None,
            }],
            parameter_groups: Vec::new(),
            control_primitives: vec![
                ControlPrimitive {
                    primitive_id: "diameter".to_string(),
                    label: "Diameter".to_string(),
                    kind: ControlPrimitiveKind::Number,
                    source: ControlViewSource::Llm,
                    part_ids: Vec::new(),
                    bindings: vec![crate::contracts::PrimitiveBinding {
                        parameter_key: "diameter".to_string(),
                        scale: 1.0,
                        offset: 0.0,
                        min: None,
                        max: None,
                    }],
                    editable: true,
                    order: 1,
                },
                ControlPrimitive {
                    primitive_id: "lip".to_string(),
                    label: "Lip".to_string(),
                    kind: ControlPrimitiveKind::Toggle,
                    source: ControlViewSource::Llm,
                    part_ids: Vec::new(),
                    bindings: vec![crate::contracts::PrimitiveBinding {
                        parameter_key: "lip".to_string(),
                        scale: 1.0,
                        offset: 0.0,
                        min: None,
                        max: None,
                    }],
                    editable: true,
                    order: 2,
                },
            ],
            control_relations: vec![crate::contracts::ControlRelation {
                relation_id: "rel-1".to_string(),
                source_primitive_id: "diameter".to_string(),
                target_primitive_id: "lip".to_string(),
                mode: ControlRelationMode::Mirror,
                scale: 1.0,
                offset: 0.0,
                enabled: true,
            }],
            control_views: vec![ControlView {
                view_id: "main".to_string(),
                label: "Main".to_string(),
                scope: ControlViewScope::Global,
                part_ids: Vec::new(),
                primitive_ids: vec!["diameter".to_string(), "lip".to_string()],
                sections: Vec::new(),
                is_default: true,
                source: ControlViewSource::Llm,
                status: EnrichmentStatus::Accepted,
                order: 1,
            }],
            advisories: Vec::new(),
            selection_targets: vec![
                crate::models::SelectionTarget {
                    target_id: Some("body:edge:0:0-0-0_10-0-0".to_string()),
                    durable_target_id: None,
                    canonical_target_id: None,
                    alias_ids: Vec::new(),
                    part_id: "body".to_string(),
                    viewer_node_id: "body".to_string(),
                    label: "Body.Edge1".to_string(),
                    kind: crate::models::SelectionTargetKind::Edge,
                    editable: true,
                    parameter_keys: Vec::new(),
                    primitive_ids: Vec::new(),
                    view_ids: Vec::new(),
                },
                crate::models::SelectionTarget {
                    target_id: Some("body:face:0:5-5-5:100".to_string()),
                    durable_target_id: None,
                    canonical_target_id: None,
                    alias_ids: Vec::new(),
                    part_id: "body".to_string(),
                    viewer_node_id: "body".to_string(),
                    label: "Body.Face1".to_string(),
                    kind: crate::models::SelectionTargetKind::Face,
                    editable: true,
                    parameter_keys: Vec::new(),
                    primitive_ids: Vec::new(),
                    view_ids: Vec::new(),
                },
            ],
            measurement_annotations: Vec::new(),
            feature_graph: None,
            correspondence_graph: None,
            warnings: Vec::new(),
            enrichment_state: crate::contracts::ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    async fn seed_target() -> (AppState, TestPathResolver) {
        seed_target_with_macro("Base Pot", "V-base", "base_macro()").await
    }

    async fn seed_ecky_verify_target(
        source: &str,
        model_id: &str,
        preview_name: &str,
        include_step_export: bool,
    ) -> (AppState, TestPathResolver) {
        let (state, resolver) = seed_target_with_macro("Verify Target", "V-verify", source).await;
        let preview_stl_path = resolver.root.join(preview_name);
        write_closed_tetra_binary_stl(&preview_stl_path);
        let source_path = resolver.root.join(format!("{model_id}.ecky"));
        fs::write(&source_path, source).expect("write ecky source");

        let mut design = sample_design("Verify Target", "V-verify", source);
        design.macro_dialect = MacroDialect::EckyIrV0;
        design.engine_kind = crate::models::EngineKind::EckyIrV0;
        design.geometry_backend = crate::models::GeometryBackend::EckyRust;
        design.source_language = crate::models::SourceLanguage::EckyIrV0;
        design.post_processing = None;

        let mut bundle = sample_bundle(model_id, preview_name);
        bundle.engine_kind = crate::models::EngineKind::EckyIrV0;
        bundle.geometry_backend = crate::models::GeometryBackend::EckyRust;
        bundle.source_language = crate::models::SourceLanguage::EckyIrV0;
        bundle.content_hash = format!("verify-{model_id}");
        bundle.macro_path = Some(source_path.display().to_string());
        bundle.preview_stl_path = preview_stl_path.display().to_string();
        if include_step_export {
            bundle.export_artifacts.push(crate::models::ExportArtifact {
                label: "STEP".to_string(),
                format: "step".to_string(),
                path: format!("/tmp/{model_id}.step"),
                role: "cad-exchange".to_string(),
            });
        }

        let mut manifest = sample_manifest(model_id);
        manifest.engine_kind = crate::models::EngineKind::EckyIrV0;
        manifest.geometry_backend = crate::models::GeometryBackend::EckyRust;
        manifest.source_language = crate::models::SourceLanguage::EckyIrV0;
        manifest.source_digest = Some(crate::mcp::macro_buffer::source_digest(source));

        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");
        {
            let conn = state.db.lock().await;
            conn.execute(
                "UPDATE messages SET output = ?1, artifact_bundle = ?2, model_manifest = ?3 WHERE id = 'msg-1'",
                rusqlite::params![
                    serde_json::to_string(&design).expect("design json"),
                    serde_json::to_string(&bundle).expect("bundle json"),
                    serde_json::to_string(&manifest).expect("manifest json"),
                ],
            )
            .expect("update verify target");
        }

        (state, resolver)
    }

    #[test]
    fn carry_forward_semantic_manifest_keeps_controls_and_face_bindings() {
        let mut previous = sample_manifest("model-base");
        previous.selection_targets[1].parameter_keys = vec!["diameter".to_string()];
        previous.selection_targets[1].primitive_ids = vec!["diameter".to_string()];
        previous.selection_targets[1].view_ids = vec!["main".to_string()];

        let mut next = sample_manifest("model-next");
        next.control_primitives.clear();
        next.control_relations.clear();
        next.control_views.clear();
        next.selection_targets[1].parameter_keys.clear();
        next.selection_targets[1].primitive_ids.clear();
        next.selection_targets[1].view_ids.clear();
        let mut bundle = sample_bundle("model-next", "next.stl");
        bundle.edge_targets.push(crate::models::ViewerEdgeTarget {
            target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Edge1".to_string(),
            editable: true,
            start: crate::models::ViewerEdgePoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            end: crate::models::ViewerEdgePoint {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            },
        });
        bundle.face_targets.push(crate::models::ViewerFaceTarget {
            target_id: "body:face:0:5-5-5:100".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Face1".to_string(),
            editable: true,
            center: crate::models::ViewerEdgePoint {
                x: 5.0,
                y: 5.0,
                z: 5.0,
            },
            normal: Some([0.0, 0.0, 1.0]),
            area: Some(100.0),
        });

        let merged = carry_forward_semantic_manifest(Some(&previous), next, &bundle);

        assert_eq!(merged.control_primitives.len(), 2);
        assert_eq!(merged.control_views.len(), 1);
        assert_eq!(
            merged.selection_targets[1].parameter_keys,
            vec!["diameter".to_string()]
        );
        assert_eq!(
            merged.selection_targets[1].primitive_ids,
            vec!["diameter".to_string()]
        );
        assert_eq!(
            merged.selection_targets[1].view_ids,
            vec!["main".to_string()]
        );
        assert!(merged.warnings.is_empty());
    }

    #[test]
    fn carry_forward_semantic_manifest_ignores_broad_target_bindings() {
        let mut previous = sample_manifest("model-base");
        previous.selection_targets[1].parameter_keys = vec![
            "diameter".to_string(),
            "height".to_string(),
            "clearance".to_string(),
        ];

        let mut next = sample_manifest("model-next");
        next.selection_targets[1].parameter_keys.clear();
        let mut bundle = sample_bundle("model-next", "next.stl");
        bundle.edge_targets.push(crate::models::ViewerEdgeTarget {
            target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Edge1".to_string(),
            editable: true,
            start: crate::models::ViewerEdgePoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            end: crate::models::ViewerEdgePoint {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            },
        });
        bundle.face_targets.push(crate::models::ViewerFaceTarget {
            target_id: "body:face:0:5-5-5:100".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Face1".to_string(),
            editable: true,
            center: crate::models::ViewerEdgePoint {
                x: 5.0,
                y: 5.0,
                z: 5.0,
            },
            normal: Some([0.0, 0.0, 1.0]),
            area: Some(100.0),
        });

        let merged = carry_forward_semantic_manifest(Some(&previous), next, &bundle);

        assert_eq!(merged.control_primitives.len(), 2);
        assert!(merged.selection_targets[1].parameter_keys.is_empty());
    }

    async fn seed_target_with_macro(
        title: &str,
        version_name: &str,
        macro_code: &str,
    ) -> (AppState, TestPathResolver) {
        let root = std::env::temp_dir().join(format!("ecky-mcp-root-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::init_db(&test_db_path("target-read")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let resolver = TestPathResolver { root };
        let now = now_secs();

        let mut base_bundle = sample_bundle("model-base", "base.stl");
        base_bundle
            .export_artifacts
            .push(crate::models::ExportArtifact {
                label: "STEP".to_string(),
                format: "step".to_string(),
                path: "/tmp/model-base.step".to_string(),
                role: "cad-exchange".to_string(),
            });
        base_bundle
            .edge_targets
            .push(crate::models::ViewerEdgeTarget {
                target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "body".to_string(),
                label: "Body.Edge1".to_string(),
                editable: true,
                start: crate::models::ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                end: crate::models::ViewerEdgePoint {
                    x: 10.0,
                    y: 0.0,
                    z: 0.0,
                },
            });
        base_bundle
            .face_targets
            .push(crate::models::ViewerFaceTarget {
                target_id: "body:face:0:5-5-5:100".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "body".to_string(),
                label: "Body.Face1".to_string(),
                editable: true,
                center: crate::models::ViewerEdgePoint {
                    x: 5.0,
                    y: 5.0,
                    z: 5.0,
                },
                normal: Some([0.0, 0.0, 1.0]),
                area: Some(100.0),
            });
        let base_manifest = sample_manifest("model-base");
        let mut base_design = sample_design(title, version_name, macro_code);
        if macro_code.trim_start().starts_with("(model") {
            base_design.macro_dialect = MacroDialect::EckyIrV0;
            base_design.engine_kind = crate::models::EngineKind::EckyIrV0;
            base_design.geometry_backend = crate::models::GeometryBackend::EckyRust;
            base_design.source_language = crate::models::SourceLanguage::EckyIrV0;
        }

        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-1", "Thread", now, None).unwrap();
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "msg-1".to_string(),
                    role: MessageRole::Assistant,
                    content: "Base version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(base_design),
                    usage: None,
                    artifact_bundle: Some(base_bundle),
                    model_manifest: Some(base_manifest),
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
        }

        (state, resolver)
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn health_check_includes_runtime_capabilities() {
        let _guard = crate::build123d_test_env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!("ecky-mcp-health-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let python = root.join("bin").join("python3");
        write_executable(&python, "#!/bin/sh\nprintf '%s\\n' \"$0\"\nexit 0\n");
        std::env::set_var("BUILD123D_PYTHON", &python);

        let conn = crate::db::init_db(&test_db_path("health-check")).expect("db");
        let mut config = test_config();
        config.freecad_cmd = "/missing/freecadcmd".to_string();
        let state = AppState::new(config, None, conn);
        let resolver = TestPathResolver { root };

        let response = handle_health_check(&state, &resolver)
            .await
            .expect("health check");

        std::env::remove_var("BUILD123D_PYTHON");

        assert!(response.db_ready);
        assert!(!response.freecad_configured);
        assert!(!response.runtime_capabilities.freecad.available);
        assert!(response.runtime_capabilities.build123d.available);
        assert_eq!(
            response
                .runtime_capabilities
                .recommended_authoring_context
                .geometry_backend,
            crate::models::GeometryBackend::Build123d
        );
    }

    async fn seed_live_session(state: &AppState) {
        state.mcp_sessions.lock().await.insert(
            test_session_id(),
            crate::models::McpSessionState {
                client_kind: "mcp-http".to_string(),
                host_label: "Claude Code".to_string(),
                agent_label: "claude".to_string(),
                llm_model_id: None,
                llm_model_label: Some("Claude Sonnet".to_string()),
                bound_thread_id: None,
                last_target: Some(session_target_ref(
                    "thread-1".to_string(),
                    "msg-1".to_string(),
                    Some("model-base".to_string()),
                )),
                phase: Some("idle".to_string()),
                status_text: Some("Agent joined the workspace.".to_string()),
                busy: false,
                activity_label: None,
                activity_started_at: None,
                attention_kind: None,
                waiting_on_prompt: false,
                current_turn_id: None,
                current_turn_thread_id: None,
                current_turn_working_message_ids: Vec::new(),
                current_turn_working_version_message_id: None,
                updated_at: now_secs(),
            },
        );
    }

    #[tokio::test]
    async fn thread_create_creates_blank_thread_and_binds_session() {
        let conn = crate::db::init_db(&test_db_path("thread-create")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        seed_live_session(&state).await;

        let response = handle_thread_create(
            &state,
            ThreadCreateRequest {
                identity: AgentIdentityOverride::default(),
                title: Some("Seven Petal Badge".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("thread create");

        assert_eq!(response.title, "Seven Petal Badge");

        let conn = state.db.lock().await;
        let thread = history::get_thread(&conn, &response.thread_id).expect("created thread");
        assert_eq!(thread.title, "Seven Petal Badge");
        assert_eq!(thread.version_count, 0);
        let stored_session = db::get_sessions_by_ids(&conn, &[test_session_id()])
            .expect("stored session")
            .into_iter()
            .next()
            .expect("session row");
        assert_eq!(
            stored_session.thread_id.as_deref(),
            Some(response.thread_id.as_str())
        );
        assert!(stored_session.message_id.is_none());
        drop(conn);

        let live_session = state
            .mcp_sessions
            .lock()
            .await
            .get(&test_session_id())
            .cloned()
            .expect("live session");
        assert_eq!(
            live_session.bound_thread_id.as_deref(),
            Some(response.thread_id.as_str())
        );
        assert!(live_session.last_target.is_none());
        assert!(!live_session.busy);
    }

    #[tokio::test]
    async fn thread_borrow_switches_current_session_target_without_logout() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;
        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-2", "Thread Two", now_secs(), None).unwrap();
        }

        let response = handle_thread_borrow(
            &state,
            ThreadBorrowRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-2".to_string()),
                message_id: None,
                model_id: None,
                steal_thread: false,
            },
            &test_ctx(),
        )
        .await
        .expect("borrow thread");

        assert_eq!(response.thread_id, "thread-2");
        assert_eq!(response.title, "Thread Two");
        assert_eq!(response.message_id, None);

        let live_session = state
            .mcp_sessions
            .lock()
            .await
            .get(&test_session_id())
            .cloned()
            .expect("live session");
        assert_eq!(live_session.bound_thread_id.as_deref(), Some("thread-2"));
        assert!(live_session.last_target.is_none());

        let conn = state.db.lock().await;
        let stored_session = db::get_sessions_by_ids(&conn, &[test_session_id()])
            .expect("stored session")
            .into_iter()
            .next()
            .expect("session row");
        assert_eq!(stored_session.thread_id.as_deref(), Some("thread-2"));
        assert!(stored_session.message_id.is_none());
    }

    #[tokio::test]
    async fn thread_borrow_message_target_sets_last_target() {
        let (state, _resolver) = seed_target().await;
        state.mcp_sessions.lock().await.insert(
            test_session_id(),
            crate::models::McpSessionState::new("mcp-http".to_string(), "Claude Code".to_string()),
        );

        let response = handle_thread_borrow(
            &state,
            ThreadBorrowRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: None,
                message_id: Some("msg-1".to_string()),
                model_id: Some("model-base".to_string()),
                steal_thread: false,
            },
            &test_ctx(),
        )
        .await
        .expect("borrow message target");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.message_id.as_deref(), Some("msg-1"));
        assert_eq!(response.model_id.as_deref(), Some("model-base"));

        let live_session = state
            .mcp_sessions
            .lock()
            .await
            .get(&test_session_id())
            .cloned()
            .expect("live session");
        assert_eq!(live_session.bound_thread_id.as_deref(), Some("thread-1"));
        let last_target = live_session.last_target.expect("last target");
        assert_eq!(last_target.thread_id, "thread-1");
        assert_eq!(last_target.message_id, "msg-1");
        assert_eq!(last_target.model_id.as_deref(), Some("model-base"));
    }

    #[tokio::test]
    async fn resolve_prompt_thread_context_returns_bound_thread_identity() {
        let (state, _resolver) = seed_target().await;

        let (thread_id, thread_title) = resolve_prompt_thread_context(
            &state,
            Some(&agent_dialogue::SessionThreadTarget {
                thread_id: "thread-1".to_string(),
                message_id: Some("msg-1".to_string()),
                model_id: Some("model-base".to_string()),
            }),
        )
        .await
        .expect("prompt thread context");

        assert_eq!(thread_id.as_deref(), Some("thread-1"));
        assert_eq!(thread_title.as_deref(), Some("Thread"));
    }

    #[tokio::test]
    async fn request_user_prompt_target_does_not_fall_back_to_current_snapshot() {
        let (state, _resolver) = seed_target().await;
        {
            let mut snapshot = state.last_snapshot.lock().unwrap();
            *snapshot = Some(crate::models::LastDesignSnapshot {
                design: None,
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                artifact_bundle: Some(sample_bundle("model-base", "base.stl")),
                model_manifest: None,
                selected_part_id: None,
            });
        }

        let target = resolve_request_user_prompt_target(
            &state,
            &test_session_id(),
            &UserPromptRequest {
                request_id: None,
                message: Some("Hello".to_string()),
                timeout_secs: Some(30),
                thread_id: None,
                message_id: None,
                model_id: None,
            },
        )
        .await
        .expect("request target");

        assert_eq!(target, None);
    }

    #[test]
    fn configured_prompt_timeout_prefers_request_override_and_config_default() {
        let conn = crate::db::init_db(&test_db_path("prompt-timeout-config")).expect("db");
        let mut config = test_config();
        config.mcp.prompt_timeout_secs = 1444;
        let state = AppState::new(config, None, conn);

        assert_eq!(configured_prompt_timeout_secs(&state, None), 1444);
        assert_eq!(configured_prompt_timeout_secs(&state, Some(45)), 45);
        assert_eq!(configured_prompt_timeout_secs(&state, Some(0)), 10);
        assert_eq!(configured_prompt_timeout_secs(&state, Some(999_999)), 1800);
    }

    #[tokio::test]
    async fn request_user_prompt_allows_explicit_cross_thread_target() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;
        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-2", "Thread 2", now_secs(), None).unwrap();
        }

        let target = resolve_request_user_prompt_target(
            &state,
            &test_session_id(),
            &UserPromptRequest {
                request_id: None,
                message: Some("Hello".to_string()),
                timeout_secs: Some(30),
                thread_id: Some("thread-2".to_string()),
                message_id: None,
                model_id: None,
            },
        )
        .await
        .expect("cross-thread prompt override should resolve");

        let target = target.expect("explicit target");
        assert_eq!(target.thread_id, "thread-2");
        assert_eq!(target.message_id, None);
    }

    #[tokio::test]
    async fn target_meta_get_returns_lightweight_summary_without_heavy_fields() {
        let (state, resolver) = seed_target().await;
        let response = handle_target_meta_get(
            &state,
            &resolver,
            TargetMetaRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("target meta");

        assert_eq!(response.resolved_from, TargetResolvedFrom::Base);
        assert_eq!(response.model_id.as_deref(), Some("model-base"));
        assert_eq!(response.source_language, "legacyPython");
        assert_eq!(response.macro_dialect, "legacy");
        assert_eq!(response.geometry_backend, "freecad");
        assert!(!response.has_draft);
        assert!(response.has_artifact_bundle);
        assert!(response.has_runtime_manifest);
        assert_eq!(response.export_formats, vec!["step".to_string()]);
        assert!(response.has_step_export);
        assert_eq!(
            response.step_export_path.as_deref(),
            Some("/tmp/model-base.step")
        );
        assert_eq!(response.edge_target_count, 1);
        assert_eq!(response.face_target_count, 1);
        assert_eq!(response.ui_field_count, 3);
        assert_eq!(response.range_count, 1);
        assert_eq!(response.select_count, 1);
        assert_eq!(response.checkbox_count, 1);
        assert_eq!(response.parameter_count, 3);
        assert!(response.has_semantic_manifest);
        assert_eq!(response.control_primitive_count, 2);
        assert_eq!(response.control_relation_count, 1);
        assert_eq!(response.control_view_count, 1);
        assert_eq!(response.scene_packet.schema_version, 1);
        assert_eq!(response.scene_packet.active_lens.as_str(), "exact");
        assert_eq!(
            response
                .scene_packet
                .representations
                .iter()
                .map(|entry| (entry.kind.as_str(), entry.status.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("sketchIntent", "rebuildable"),
                ("meshDraft", "stale"),
                ("exactModel", "committed")
            ]
        );
        assert!(response
            .scene_packet
            .allowed_patch_targets
            .contains(&"macroBufferReplaceAndPreview".to_string()));
        assert_eq!(response.scene_packet.topology.edge_target_count, 1);
        assert_eq!(response.scene_packet.topology.face_target_count, 1);

        let value = serde_json::to_value(&response).unwrap();
        assert!(value.get("scenePacket").is_some());
        assert!(value.get("macroCode").is_none());
        assert!(value.get("artifactBundle").is_none());
        assert!(value.get("modelManifest").is_none());
        assert!(value.get("latestDraft").is_none());
        assert!(value.get("cadSdkSnippet").is_none());
    }

    #[tokio::test]
    async fn target_meta_get_marks_ecky_source_as_ast_patchable() {
        let (state, resolver) =
            seed_target_with_macro("Ecky block", "V-ecky", "(model\n  (box :size 10))").await;
        let response = handle_target_meta_get(
            &state,
            &resolver,
            TargetMetaRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("target meta");

        assert_eq!(response.source_language, "ecky");
        assert!(response
            .scene_packet
            .allowed_patch_targets
            .contains(&"eckyAstReplaceAndRender".to_string()));
        assert_eq!(response.scene_packet.active_lens.as_str(), "exact");
    }

    #[tokio::test]
    async fn managed_session_log_in_allows_no_bound_target() {
        let conn = crate::db::init_db(&test_db_path("managed-session-login-target")).expect("db");
        let mut config = test_config();
        config.connection_type = Some("mcp".to_string());
        config.mcp.mode = crate::contracts::McpMode::Active;
        config.mcp.primary_agent_id = Some("agent-1".to_string());
        config.mcp.auto_agents = vec![crate::contracts::AutoAgent {
            id: "agent-1".to_string(),
            label: "claude".to_string(),
            cmd: "claude".to_string(),
            model: None,
            args: Vec::new(),
            enabled: true,
            start_on_demand: true,
        }];
        let state = AppState::new(config, None, conn);
        crate::mcp::runtime::initialize_auto_agent_supervisors(state.clone());
        crate::mcp::runtime::bind_managed_http_session(
            &state,
            "agent-1",
            &test_session_id(),
            Some("Connected to Ecky.".to_string()),
        );
        state.mcp_sessions.lock().await.insert(
            test_session_id(),
            crate::models::McpSessionState::new("mcp-http".to_string(), "Claude Code".to_string()),
        );

        let response = handle_session_log_in(
            &state,
            SessionLoginRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: None,
                message_id: None,
                model_id: None,
                steal_thread: false,
            },
            &test_ctx(),
        )
        .await
        .expect("managed session should log in without a bound target");

        assert_eq!(response.thread_id, None);
        assert_eq!(response.message_id, None);
        assert_eq!(response.model_id, None);

        let conn = state.db.lock().await;
        let stored_session = db::get_sessions_by_ids(&conn, &[test_session_id()])
            .expect("stored session")
            .into_iter()
            .next()
            .expect("session row");
        assert_eq!(stored_session.thread_id, None);
        assert_eq!(stored_session.message_id, None);
        drop(conn);

        let live_session = state
            .mcp_sessions
            .lock()
            .await
            .get(&test_session_id())
            .cloned()
            .expect("live session");
        assert_eq!(live_session.bound_thread_id, None);
        assert!(live_session.last_target.is_none());
        assert!(!live_session.busy);
    }

    #[tokio::test]
    async fn passive_session_log_in_allows_no_thread_target_without_snapshot_fallback() {
        let (state, _resolver) = seed_target().await;
        {
            let mut snapshot = state.last_snapshot.lock().unwrap();
            *snapshot = Some(crate::models::LastDesignSnapshot {
                design: None,
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                artifact_bundle: Some(sample_bundle("model-base", "base.stl")),
                model_manifest: None,
                selected_part_id: None,
            });
        }

        let response = handle_session_log_in(
            &state,
            SessionLoginRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: None,
                message_id: None,
                model_id: None,
                steal_thread: false,
            },
            &test_ctx(),
        )
        .await
        .expect("passive session log in should allow no thread target");

        assert_eq!(response.thread_id, None);
        assert_eq!(response.message_id, None);
        assert_eq!(response.model_id, None);

        let conn = state.db.lock().await;
        let stored_session = db::get_sessions_by_ids(&conn, &[test_session_id()])
            .expect("stored session")
            .into_iter()
            .next()
            .expect("session row");
        assert_eq!(stored_session.thread_id, None);
        assert_eq!(stored_session.message_id, None);
    }

    #[tokio::test]
    async fn managed_session_log_in_keeps_runtime_thread_without_snapshot_message_fallback() {
        let (state, _resolver) = seed_target().await;
        let mut config = state.config.lock().unwrap().clone();
        config.connection_type = Some("mcp".to_string());
        config.mcp.mode = crate::contracts::McpMode::Active;
        config.mcp.primary_agent_id = Some("agent-1".to_string());
        config.mcp.auto_agents = vec![crate::contracts::AutoAgent {
            id: "agent-1".to_string(),
            label: "claude".to_string(),
            cmd: "claude".to_string(),
            model: None,
            args: Vec::new(),
            enabled: true,
            start_on_demand: true,
        }];
        {
            *state.config.lock().unwrap() = config;
        }
        crate::mcp::runtime::initialize_auto_agent_supervisors(state.clone());
        crate::mcp::runtime::bind_managed_http_session(
            &state,
            "agent-1",
            &test_session_id(),
            Some("Connected to Ecky.".to_string()),
        );
        crate::mcp::runtime::wake_auto_agent_by_label(
            &state,
            "claude",
            Some("thread-1".to_string()),
        )
        .await
        .expect("wake should capture the thread-only target");
        {
            let mut snapshot = state.last_snapshot.lock().unwrap();
            *snapshot = Some(crate::models::LastDesignSnapshot {
                design: None,
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                artifact_bundle: Some(sample_bundle("model-base", "base.stl")),
                model_manifest: None,
                selected_part_id: None,
            });
        }

        let response = handle_session_log_in(
            &state,
            SessionLoginRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: None,
                message_id: None,
                model_id: None,
                steal_thread: false,
            },
            &test_ctx(),
        )
        .await
        .expect("managed session log in should bind from runtime thread");

        assert_eq!(response.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(response.message_id, None);
        assert_eq!(response.model_id, None);
    }

    #[tokio::test]
    async fn session_log_in_blocks_claimed_thread_without_steal() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;
        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                &test_ctx(),
                Some("thread-1".to_string()),
                Some("msg-1".to_string()),
                Some("model-base".to_string()),
                "idle",
                "Agent joined the workspace.",
            )
            .unwrap();
        }

        let err = handle_session_log_in(
            &state,
            SessionLoginRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: Some("model-base".to_string()),
                steal_thread: false,
            },
            &test_ctx_other(),
        )
        .await
        .expect_err("claimed thread should require explicit steal");

        assert_eq!(err.code, AppErrorCode::Conflict);
        assert!(err.message.contains("stealThread"));
        assert!(err.message.contains("claude"));
    }

    #[tokio::test]
    async fn session_log_in_with_steal_transfers_thread_claim() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;
        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                &test_ctx(),
                Some("thread-1".to_string()),
                Some("msg-1".to_string()),
                Some("model-base".to_string()),
                "idle",
                "Agent joined the workspace.",
            )
            .unwrap();
        }

        state.mcp_sessions.lock().await.insert(
            test_session_id_other(),
            crate::models::McpSessionState::new("http".to_string(), "Codex".to_string()),
        );

        let response = handle_session_log_in(
            &state,
            SessionLoginRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: Some("model-base".to_string()),
                steal_thread: true,
            },
            &test_ctx_other(),
        )
        .await
        .expect("steal should transfer thread claim");

        assert_eq!(response.thread_id.as_deref(), Some("thread-1"));
        let sessions = state.mcp_sessions.lock().await;
        let prior_owner = sessions.get(&test_session_id()).expect("prior owner");
        assert_eq!(prior_owner.bound_thread_id, None);
        assert!(prior_owner.last_target.is_none());
        let new_owner = sessions.get(&test_session_id_other()).expect("new owner");
        assert_eq!(new_owner.bound_thread_id.as_deref(), Some("thread-1"));
        drop(sessions);

        let conn = state.db.lock().await;
        let stored = db::get_sessions_by_ids(&conn, &[test_session_id(), test_session_id_other()])
            .expect("stored sessions");
        let old_row = stored
            .iter()
            .find(|session| session.session_id == test_session_id())
            .expect("old row");
        let new_row = stored
            .iter()
            .find(|session| session.session_id == test_session_id_other())
            .expect("new row");
        assert_eq!(old_row.thread_id, None);
        assert_eq!(new_row.thread_id.as_deref(), Some("thread-1"));
    }

    #[tokio::test]
    async fn session_resume_blocks_claimed_thread_without_explicit_steal_path() {
        let (state, _resolver) = seed_target().await;
        state.mcp_sessions.lock().await.insert(
            test_session_id_other(),
            crate::models::McpSessionState {
                client_kind: "mcp-http".to_string(),
                host_label: "Codex".to_string(),
                agent_label: "codex".to_string(),
                llm_model_id: None,
                llm_model_label: Some("GPT-5".to_string()),
                bound_thread_id: None,
                last_target: Some(session_target_ref(
                    "thread-1".to_string(),
                    "msg-1".to_string(),
                    Some("model-base".to_string()),
                )),
                phase: Some("idle".to_string()),
                status_text: Some("Agent joined the workspace.".to_string()),
                busy: false,
                activity_label: None,
                activity_started_at: None,
                attention_kind: None,
                waiting_on_prompt: false,
                current_turn_id: None,
                current_turn_thread_id: None,
                current_turn_working_message_ids: Vec::new(),
                current_turn_working_version_message_id: None,
                updated_at: now_secs(),
            },
        );
        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                &test_ctx(),
                Some("thread-1".to_string()),
                Some("msg-1".to_string()),
                Some("model-base".to_string()),
                "disconnected",
                "Agent left the workspace.",
            )
            .unwrap();
            persist_agent_session(
                &conn,
                &test_ctx_other(),
                Some("thread-1".to_string()),
                Some("msg-1".to_string()),
                Some("model-base".to_string()),
                "idle",
                "Agent joined the workspace.",
            )
            .unwrap();
        }

        let err = handle_session_resume(
            &state,
            SessionResumeRequest {
                identity: AgentIdentityOverride::default(),
            },
            &test_ctx(),
        )
        .await
        .expect_err("resume should not steal another live thread claim");

        assert_eq!(err.code, AppErrorCode::Conflict);
        assert!(err.message.contains("stealThread"));
        assert!(err.message.contains("codex"));
    }

    #[tokio::test]
    async fn thread_list_and_get_surface_live_claim_owner() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;

        let list = handle_thread_list(&state).await.expect("thread list");
        assert_eq!(list.threads.len(), 1);

        let thread = handle_thread_get(
            &state,
            ThreadGetRequest {
                thread_id: "thread-1".to_string(),
            },
        )
        .await
        .expect("thread get");
        assert_eq!(
            thread
                .claim_owner
                .as_ref()
                .map(|owner| owner.agent_label.as_str()),
            Some("claude")
        );
    }

    #[tokio::test]
    async fn concept_preview_save_stores_agent_image_without_selected_engine() {
        let conn = crate::db::init_db(&test_db_path("concept-preview-save")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        seed_live_session(&state).await;
        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-1", "Thread", now_secs(), None).unwrap();
        }

        let response = handle_concept_preview_save(
            &state,
            ConceptPreviewSaveRequest {
                image_data: "data:image/svg+xml;base64,PHN2Zy8+".to_string(),
                caption: "Agent sketch.".to_string(),
                thread_id: Some("thread-1".to_string()),
                message_id: None,
                identity: AgentIdentityOverride::default(),
            },
            &test_ctx(),
        )
        .await
        .expect("concept preview save");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.caption, "Agent sketch.");
        let messages = {
            let conn = state.db.lock().await;
            db::get_thread_messages(&conn, "thread-1").expect("messages")
        };
        let saved = messages
            .iter()
            .find(|message| message.id == response.message_id)
            .expect("saved concept preview");
        assert_eq!(saved.content, "Agent sketch.");
        assert_eq!(saved.role, MessageRole::Assistant);
        assert_eq!(
            saved.image_data.as_deref(),
            Some(response.image_data.as_str())
        );
        assert_eq!(saved.visual_kind, Some(MessageVisualKind::ConceptPreview));
        assert_eq!(saved.usage, None);
    }

    #[tokio::test]
    async fn thread_list_and_meta_surface_pending_inbox_anchor() {
        let (state, _resolver) = seed_target().await;
        let now = now_secs();

        {
            let conn = state.db.lock().await;
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "assistant-pending-1".to_string(),
                    role: MessageRole::Assistant,
                    content: "Working on it".to_string(),
                    status: MessageStatus::Pending,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "user-pending-1".to_string(),
                    role: MessageRole::User,
                    content: "first".to_string(),
                    status: MessageStatus::Pending,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "user-pending-2".to_string(),
                    role: MessageRole::User,
                    content: "second".to_string(),
                    status: MessageStatus::Pending,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
            db::set_thread_pending_confirm(&conn, "thread-1", Some("needs-review")).unwrap();
        }

        let list = handle_thread_list(&state).await.expect("thread list");
        assert_eq!(list.threads.len(), 1);
        let entry = &list.threads[0];
        assert_eq!(entry.pending_count, 1);
        assert_eq!(entry.queued_count, 2);
        assert_eq!(entry.pending_confirm.as_deref(), Some("needs-review"));
        assert_eq!(
            entry.latest_pending_message_id.as_deref(),
            Some("user-pending-2")
        );

        let meta = handle_thread_meta_get(
            &state,
            ThreadMetaRequest {
                thread_id: "thread-1".to_string(),
            },
        )
        .await
        .expect("thread meta");
        assert_eq!(meta.pending_count, 1);
        assert_eq!(meta.queued_count, 2);
        assert_eq!(meta.pending_confirm.as_deref(), Some("needs-review"));
        assert_eq!(
            meta.latest_pending_message_id.as_deref(),
            Some("user-pending-2")
        );
    }

    #[tokio::test]
    async fn thread_get_rejects_deleted_thread_as_normal_mcp_thread() {
        let (state, _resolver) = seed_target().await;
        {
            let conn = state.db.lock().await;
            db::delete_thread(&conn, "thread-1").unwrap();
        }

        let list = handle_thread_list(&state).await.expect("thread list");
        assert!(list.threads.is_empty());

        let err = handle_thread_get(
            &state,
            ThreadGetRequest {
                thread_id: "thread-1".to_string(),
            },
        )
        .await
        .expect_err("deleted thread should not load through normal MCP thread_get");

        assert_eq!(err.code, AppErrorCode::NotFound);
    }

    #[tokio::test]
    async fn session_log_in_rejects_deleted_thread_target() {
        let (state, _resolver) = seed_target().await;
        {
            let conn = state.db.lock().await;
            db::delete_thread(&conn, "thread-1").unwrap();
        }

        let err = handle_session_log_in(
            &state,
            SessionLoginRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: None,
                model_id: None,
                steal_thread: false,
            },
            &test_ctx(),
        )
        .await
        .expect_err("deleted thread should not accept normal MCP session claim");

        assert_eq!(err.code, AppErrorCode::NotFound);
    }

    #[tokio::test]
    async fn thread_messages_get_compacts_content_and_keeps_payload_flags() {
        let (state, _resolver) = seed_target().await;
        let long_content = "connector ".repeat(40);
        {
            let conn = state.db.lock().await;
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "msg-2".to_string(),
                    role: MessageRole::Assistant,
                    content: long_content.clone(),
                    status: MessageStatus::Success,
                    output: None,
                    usage: None,
                    artifact_bundle: Some(sample_bundle("model-2", "preview.stl")),
                    model_manifest: Some(sample_manifest("model-2")),
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now_secs() + 1,
                },
            )
            .unwrap();
        }

        let response = handle_thread_messages_get(
            &state,
            ThreadMessagesRequest {
                thread_id: "thread-1".to_string(),
                limit: Some(1),
                before: None,
                roles: None,
            },
        )
        .await
        .expect("thread messages");

        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0].id, "msg-2");
        assert!(response.messages[0].content.len() < long_content.len());
        assert!(response.messages[0].content.ends_with('…'));
        assert!(response.messages[0].has_artifacts);
        assert!(response.messages[0].has_manifest);
    }

    #[tokio::test]
    async fn target_macro_get_returns_active_macro_payload() {
        let (state, resolver) = seed_target().await;
        let response = handle_target_macro_get(
            &state,
            &resolver,
            TargetMacroRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                start_line: None,
                end_line: None,
            },
            &test_ctx(),
        )
        .await
        .expect("target macro");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.message_id, "msg-1");
        assert_eq!(response.title, "Base Pot");
        assert_eq!(response.version_name, "V-base");
        assert_eq!(response.resolved_from, TargetResolvedFrom::Base);
        assert_eq!(response.line_count, 1);
        assert_eq!(response.window_start_line, 1);
        assert_eq!(response.window_end_line, 1);
        assert!(!response.truncated);
        assert_eq!(response.lines[0].text, "base_macro()");
        assert_eq!(response.macro_dialect, MacroDialect::Legacy);
        let value = serde_json::to_value(&response).expect("target macro json");
        assert!(value.get("macroCode").is_none());
        let artifact_digest = response.artifact_digest.as_ref().expect("artifact digest");
        assert_eq!(artifact_digest.model_id, "model-base");
        assert!(artifact_digest.has_step_export);
        assert_eq!(
            artifact_digest.step_export_path.as_deref(),
            Some("/tmp/model-base.step")
        );
        assert!(response.post_processing.is_none());
        assert_eq!(response.authoring_context.source_language, "legacyPython");
        assert_eq!(response.authoring_context.macro_dialect, "legacy");
        assert_eq!(response.authoring_context.geometry_backend, "freecad");
        assert!(response
            .authoring_context
            .authoring_card
            .contains("Ecky authoring card"));
        assert!(response
            .authoring_context
            .guide_uris
            .iter()
            .any(|uri| uri == "ecky://guides/authoring-card"));
    }

    #[tokio::test]
    async fn target_get_returns_artifact_digest_for_export_truth() {
        let (state, resolver) = seed_target().await;
        let response = handle_target_get(
            &state,
            &resolver,
            TargetGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("target get");

        let artifact_digest = response.artifact_digest.expect("artifact digest");
        assert_eq!(artifact_digest.model_id, "model-base");
        assert_eq!(artifact_digest.export_formats, vec!["step"]);
        assert!(artifact_digest.has_step_export);
        assert_eq!(
            artifact_digest.step_export_path.as_deref(),
            Some("/tmp/model-base.step")
        );
    }

    #[test]
    fn artifact_bundle_digest_reports_topology_target_counts() {
        let mut bundle = sample_bundle("model-topology", "topology.stl");
        bundle.edge_targets.push(crate::models::ViewerEdgeTarget {
            target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Edge1".to_string(),
            editable: true,
            start: crate::models::ViewerEdgePoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            end: crate::models::ViewerEdgePoint {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            },
        });
        bundle.face_targets.push(crate::models::ViewerFaceTarget {
            target_id: "body:face:0:5-5-5:100".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Face1".to_string(),
            editable: true,
            center: crate::models::ViewerEdgePoint {
                x: 5.0,
                y: 5.0,
                z: 5.0,
            },
            normal: Some([0.0, 0.0, 1.0]),
            area: Some(100.0),
        });

        let digest = artifact_bundle_digest(&bundle);

        assert_eq!(digest.edge_target_count, 1);
        assert_eq!(digest.face_target_count, 1);
    }

    #[test]
    fn render_mutation_responses_include_artifact_digest_for_export_truth() {
        let mut bundle = sample_bundle("model-render", "render.stl");
        bundle.export_artifacts.push(crate::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: "/tmp/model-render.step".to_string(),
            role: "cad-exchange".to_string(),
        });
        let digest = artifact_bundle_digest(&bundle);
        let manifest = sample_manifest("model-render");
        let design = sample_design("Render", "V-render", "render_macro()");
        let sv = crate::services::structural_verification::verify_structure(&bundle, &manifest);

        let macro_response = MacroReplaceResponse {
            thread_id: "thread-1".to_string(),
            message_id: "msg-render".to_string(),
            macro_code: design.macro_code.clone(),
            ui_spec: design.ui_spec.clone(),
            initial_params: design.initial_params.clone(),
            artifact_bundle: bundle.clone(),
            model_manifest: manifest.clone(),
            structural_verification: Some(sv.clone()),
            artifact_digest: digest.clone(),
        };
        let params_response = ParamsPatchResponse {
            thread_id: "thread-1".to_string(),
            message_id: "msg-render".to_string(),
            merged_params: design.initial_params.clone(),
            artifact_bundle: bundle.clone(),
            model_manifest: manifest.clone(),
            design_output: design.clone(),
            structural_verification: Some(sv.clone()),
            artifact_digest: digest.clone(),
        };
        let buffer_response = MacroBufferReplaceAndRenderResponse {
            thread_id: "thread-1".to_string(),
            message_id: "msg-render".to_string(),
            digest: "source-digest".to_string(),
            line_count: 1,
            macro_code: design.macro_code.clone(),
            ui_spec: design.ui_spec.clone(),
            initial_params: design.initial_params.clone(),
            artifact_bundle: bundle,
            model_manifest: manifest,
            structural_verification: Some(sv),
            artifact_digest: digest,
        };

        for value in [
            serde_json::to_value(macro_response).expect("macro response json"),
            serde_json::to_value(params_response).expect("params response json"),
            serde_json::to_value(buffer_response).expect("buffer response json"),
        ] {
            assert_eq!(value["artifactDigest"]["modelId"], "model-render");
            assert_eq!(value["artifactDigest"]["hasStepExport"], true);
            assert_eq!(
                value["artifactDigest"]["stepExportPath"],
                "/tmp/model-render.step"
            );
        }
    }

    #[tokio::test]
    async fn macro_buffer_get_returns_artifact_digest_for_export_truth() {
        let (state, resolver) = seed_target().await;
        let response = handle_macro_buffer_get(
            &state,
            &resolver,
            MacroBufferGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                start_line: None,
                end_line: None,
            },
            &test_ctx(),
        )
        .await
        .expect("macro buffer");

        let artifact_digest = response.artifact_digest.as_ref().expect("artifact digest");
        assert_eq!(artifact_digest.model_id, "model-base");
        assert!(artifact_digest.has_step_export);
        assert_eq!(
            artifact_digest.step_export_path.as_deref(),
            Some("/tmp/model-base.step")
        );

        let value = serde_json::to_value(&response).expect("macro buffer json");
        assert!(value.get("macroCode").is_none());
        assert_eq!(value["lineCount"], 1);
        assert_eq!(value["windowStartLine"], 1);
        assert_eq!(value["windowEndLine"], 1);
        assert_eq!(value["truncated"], false);
        assert_eq!(value["lines"][0]["text"], "base_macro()");
    }

    #[tokio::test]
    async fn macro_buffer_get_returns_requested_window_without_full_source() {
        let (state, resolver) = seed_target_with_macro(
            "window",
            "V-window",
            &(1..=205)
                .map(|line| format!("line_{line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .await;

        let response = handle_macro_buffer_get(
            &state,
            &resolver,
            MacroBufferGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                start_line: Some(201),
                end_line: Some(205),
            },
            &test_ctx(),
        )
        .await
        .expect("macro buffer");

        let value = serde_json::to_value(&response).expect("macro buffer json");
        assert!(value.get("macroCode").is_none());
        assert_eq!(value["lineCount"], 205);
        assert_eq!(value["windowStartLine"], 201);
        assert_eq!(value["windowEndLine"], 205);
        assert_eq!(value["truncated"], true);
        assert_eq!(value["lines"].as_array().expect("lines").len(), 5);
        assert_eq!(value["lines"][0]["text"], "line_201");
    }

    #[tokio::test]
    async fn ecky_ast_get_requires_feature_toggle() {
        let (state, resolver) =
            seed_target_with_macro("Box", "V-ast", "(model (part body (box 1 2 3)))").await;

        let err = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: None,
                depth: None,
                max_nodes: None,
                include_source: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("feature toggle should gate AST tool");

        assert!(err.message.contains("mcp.eckyAstAuthoring=true"));
    }

    #[tokio::test]
    async fn ecky_ast_get_returns_bounded_core_nodes_when_enabled() {
        let (state, resolver) =
            seed_target_with_macro("Box", "V-ast", "(model (part body (box 1 2 3)))").await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let response = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: None,
                depth: Some(1),
                max_nodes: Some(4),
                include_source: None,
            },
            &test_ctx(),
        )
        .await
        .expect("ast response");

        let value = serde_json::to_value(&response).expect("ast json");
        assert_eq!(
            value["sourceDigest"]
                .as_str()
                .unwrap()
                .starts_with("sha256:"),
            true
        );
        assert_eq!(value["rootPaths"][0], "/parts/body/root");
        let nodes = value["nodes"].as_array().expect("nodes");
        assert!(nodes.len() >= 1);
        let root_node = nodes
            .iter()
            .find(|node| node["path"] == "/parts/body/root")
            .expect("root node");
        assert!(root_node["digest"].as_str().unwrap().starts_with("sha256:"));
        assert!(root_node["stableNodeKey"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert_eq!(root_node["sourceAddressable"], true);
        assert_eq!(root_node["editableOps"], serde_json::json!(["replace"]));
        assert!(root_node.get("nonEditableReason").is_none());
        assert!(root_node.get("source").is_none());
        assert!(value.get("macroCode").is_none());
    }

    async fn stable_key_for_path(path: &str, source: &str) -> String {
        let (state, resolver) = seed_target_with_macro("StableKey", "V-stable-key", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let response = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: Some(path.to_string()),
                depth: Some(0),
                max_nodes: Some(8),
                include_source: Some(false),
            },
            &test_ctx(),
        )
        .await
        .expect("ast response");
        response
            .nodes
            .first()
            .map(|node| node.stable_node_key.clone())
            .expect("stable node key")
    }

    #[tokio::test]
    async fn given_unrelated_param_insert_when_ast_reloaded_then_unchanged_node_keeps_stable_key() {
        let path = "/params/width";
        let source_before =
            "(model (params (number width 12) (number height 8)) (part body (box width 2 3)))";
        let source_after = "(model (params (number depth 4) (number width 12) (number height 8)) (part body (box width 2 3)))";

        let key_before = stable_key_for_path(path, source_before).await;
        let key_after = stable_key_for_path(path, source_after).await;

        assert_eq!(key_before, key_after);
    }

    #[tokio::test]
    async fn given_unrelated_param_reorder_when_ast_reloaded_then_unchanged_node_keeps_stable_key()
    {
        let path = "/params/width";
        let source_before =
            "(model (params (number width 12) (number height 8) (number depth 4)) (part body (box width 2 3)))";
        let source_after =
            "(model (params (number depth 4) (number height 8) (number width 12)) (part body (box width 2 3)))";

        let key_before = stable_key_for_path(path, source_before).await;
        let key_after = stable_key_for_path(path, source_after).await;

        assert_eq!(key_before, key_after);
    }

    #[tokio::test]
    async fn given_numeric_change_elsewhere_when_ast_reloaded_then_unchanged_node_keeps_stable_key()
    {
        let path = "/params/width";
        let source_before =
            "(model (params (number width 12) (number height 8)) (part body (box width 2 3)))";
        let source_after =
            "(model (params (number width 12) (number height 9)) (part body (box width 2 3)))";

        let key_before = stable_key_for_path(path, source_before).await;
        let key_after = stable_key_for_path(path, source_after).await;

        assert_eq!(key_before, key_after);
    }

    #[tokio::test]
    async fn given_whitespace_only_change_when_ast_reloaded_then_unchanged_node_keeps_stable_key() {
        let path = "/params/width";
        let source_before =
            "(model (params (number width 12) (number height 8)) (part body (box width 2 3)))";
        let source_after =
            "(model\n  (params   (number width 12)\n           (number height 8))\n  (part body (box width 2 3)))";

        let key_before = stable_key_for_path(path, source_before).await;
        let key_after = stable_key_for_path(path, source_after).await;

        assert_eq!(key_before, key_after);
    }

    #[tokio::test]
    async fn given_ast_get_include_source_false_when_serialized_then_nodes_omit_source() {
        let (state, resolver) = seed_target_with_macro(
            "Params",
            "V-ast-source-off",
            "(model (params (number width 12)) (part body (box width 2 3)))",
        )
        .await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let response = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: Some("/params/width".to_string()),
                depth: Some(0),
                max_nodes: Some(4),
                include_source: Some(false),
            },
            &test_ctx(),
        )
        .await
        .expect("ast response");

        let value = serde_json::to_value(&response).expect("ast json");
        assert!(value["nodes"][0].get("source").is_none());
    }

    #[tokio::test]
    async fn given_ast_get_include_source_true_when_param_path_then_exact_bounded_source_returns() {
        let (state, resolver) = seed_target_with_macro(
            "Params",
            "V-ast-source-on",
            "(model (params (number width 12)) (part body (box width 2 3)))",
        )
        .await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let response = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: Some("/params/width".to_string()),
                depth: Some(0),
                max_nodes: Some(4),
                include_source: Some(true),
            },
            &test_ctx(),
        )
        .await
        .expect("ast response");

        let value = serde_json::to_value(&response).expect("ast json");
        let source = &value["nodes"][0]["source"];
        assert_eq!(source["text"], "(number width 12)");
        assert_eq!(source["span"], value["nodes"][0]["span"]);
        assert_eq!(source["truncated"], false);
        assert_eq!(source["maxBytes"], 4096);
        assert_eq!(source["byteLen"], "(number width 12)".len());
    }

    #[test]
    fn given_source_slice_exceeds_limit_when_bounded_then_text_truncates_with_metadata() {
        let source = format!("({})", "a".repeat(ECKY_AST_SOURCE_MAX_BYTES + 100));
        let slice =
            bounded_ecky_ast_source_slice(&source, (0, source.len())).expect("source slice");

        assert_eq!(slice.text.len(), ECKY_AST_SOURCE_MAX_BYTES);
        assert_eq!(slice.byte_len, source.len());
        assert_eq!(slice.max_bytes, ECKY_AST_SOURCE_MAX_BYTES);
        assert!(slice.truncated);
        assert_eq!(slice.span.start, 0);
        assert_eq!(slice.span.end, source.len() as u32);
    }

    #[tokio::test]
    async fn given_lowered_if_child_path_when_ast_get_then_node_reports_not_source_addressable() {
        let (state, resolver) = seed_target_with_macro(
            "Conditional",
            "V-ast-if",
            "(model (params (toggle raised true)) (part body (if raised (sphere 10) (cylinder 10 20))))",
        )
        .await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let response = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: Some("/parts/body/root/if/condition".to_string()),
                depth: Some(0),
                max_nodes: Some(4),
                include_source: Some(true),
            },
            &test_ctx(),
        )
        .await
        .expect("ast response");

        let value = serde_json::to_value(&response).expect("ast json");
        let node = &value["nodes"][0];
        assert_eq!(node["path"], "/parts/body/root/if/condition");
        assert_eq!(node["sourceAddressable"], false);
        assert_eq!(node["editableOps"], serde_json::json!([]));
        assert!(node["stableNodeKey"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert!(node["nonEditableReason"]
            .as_str()
            .unwrap()
            .contains("not source-span addressable"));
        assert!(node.get("source").is_none());
    }

    #[tokio::test]
    async fn given_ecky_params_when_ast_get_then_param_paths_are_visible() {
        let (state, resolver) = seed_target_with_macro(
            "Params",
            "V-ast-params",
            "(model (params (number width 12)) (part body (box width 2 3)))",
        )
        .await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let response = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: Some("/params/width".to_string()),
                depth: Some(0),
                max_nodes: Some(4),
                include_source: None,
            },
            &test_ctx(),
        )
        .await
        .expect("ast response");

        let value = serde_json::to_value(&response).expect("ast json");
        assert_eq!(value["rootPaths"][0], "/params/width");
        assert_eq!(value["nodes"][0]["path"], "/params/width");
        assert_eq!(value["nodes"][0]["kind"], "Param");
        assert!(value["nodes"][0]["digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
    }

    #[tokio::test]
    async fn given_ecky_param_path_when_ecky_dependency_get_then_core_reference_paths_return() {
        let (state, resolver) = seed_target_with_macro(
            "Params",
            "V-deps",
            "(model (params (number width 12) (number height 6)) (part body (box width height 3)))",
        )
        .await;

        let response = handle_ecky_dependency_get(
            &state,
            &resolver,
            EckyDependencyGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: "/params/width".to_string(),
            },
            &test_ctx(),
        )
        .await
        .expect("dependency response");

        assert_eq!(response.path, "/params/width");
        assert_eq!(response.dependency_kind, "parameterReference");
        assert_eq!(response.reference_count, 1);
        assert_eq!(response.impacted_part_ids, vec!["body".to_string()]);
        assert_eq!(
            response.impact_labels,
            vec!["part-local".to_string(), "export-affecting".to_string()]
        );
        assert_eq!(
            response.dependent_source_paths,
            vec!["/parts/body/root/call/args/0".to_string()]
        );
    }

    #[tokio::test]
    async fn given_unsupported_path_when_ecky_dependency_get_then_validation_names_supported_shape()
    {
        let (state, resolver) = seed_target_with_macro(
            "Params",
            "V-deps",
            "(model (params (number width 12)) (part body (box width 2 3)))",
        )
        .await;

        let err = handle_ecky_dependency_get(
            &state,
            &resolver,
            EckyDependencyGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: "/parts/body/root".to_string(),
            },
            &test_ctx(),
        )
        .await
        .expect_err("unsupported path should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("/params/{key}"));
        assert!(err.message.contains("/targets/{targetId}"));
        assert!(err.message.contains("/parts/body/root"));
    }

    #[tokio::test]
    async fn given_target_path_when_ecky_dependency_get_then_returns_feature_and_parameter_bindings(
    ) {
        let (state, resolver) = seed_target_with_macro(
            "Params",
            "V-deps-target",
            "(model (params (number lens_bore_d 42)) (part body (box lens_bore_d 2 3)))",
        )
        .await;

        let mut manifest = sample_manifest("model-base");
        manifest.source_language = crate::models::SourceLanguage::EckyIrV0;
        manifest.geometry_backend = crate::models::GeometryBackend::EckyRust;
        manifest.selection_targets[1].parameter_keys = vec!["lens_bore_d".to_string()];
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "lens_bore".to_string(),
                kind: "bore".to_string(),
                label: "Lens Bore".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-main".to_string()),
                    path: Some("/parts/body/root".to_string()),
                    start_byte: None,
                    end_byte: None,
                }),
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "lens_bore".to_string(),
                    output_id: "carrier-bore".to_string(),
                    target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                }],
                ports: Vec::new(),
            }],
        });

        {
            let conn = state.db.lock().await;
            conn.execute(
                "UPDATE messages SET model_manifest = ?1 WHERE id = 'msg-1'",
                rusqlite::params![serde_json::to_string(&manifest).expect("manifest json")],
            )
            .expect("update manifest");
        }

        let response = handle_ecky_dependency_get(
            &state,
            &resolver,
            EckyDependencyGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: "/targets/body:face:0:5-5-5:100".to_string(),
            },
            &test_ctx(),
        )
        .await
        .expect("dependency response");

        assert_eq!(response.path, "/targets/body:face:0:5-5-5:100");
        assert_eq!(response.dependency_kind, "selectionTargetReference");
        assert_eq!(response.impacted_part_ids, vec!["body".to_string()]);
        assert_eq!(response.parameter_keys, vec!["lens_bore_d".to_string()]);
        assert_eq!(response.feature_ids, vec!["lens_bore".to_string()]);
        assert_eq!(
            response.target_ids,
            vec!["body:face:0:5-5-5:100".to_string()]
        );
        assert_eq!(
            response.dependent_source_paths,
            vec!["/parts/body/root".to_string()]
        );
    }

    #[tokio::test]
    async fn given_single_target_with_one_feature_and_params_when_selector_resolve_then_exact() {
        let (state, resolver) = seed_target_with_macro(
            "Selector",
            "V-selector-exact",
            "(model (params (number lens_bore_d 42)) (part body (box lens_bore_d 2 3)))",
        )
        .await;

        let mut manifest = sample_manifest("model-base");
        manifest.selection_targets[1].parameter_keys = vec!["lens_bore_d".to_string()];
        manifest.selection_targets[1].primitive_ids = vec!["primitive-face-1".to_string()];
        manifest.selection_targets[1].durable_target_id = Some("durable-face-1".to_string());
        manifest.selection_targets[1].canonical_target_id = Some("canonical-face-1".to_string());
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "lens_bore".to_string(),
                kind: "bore".to_string(),
                label: "Lens Bore".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-main".to_string()),
                    path: Some("/parts/body/root".to_string()),
                    start_byte: None,
                    end_byte: None,
                }),
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "lens_bore".to_string(),
                    output_id: "carrier-bore".to_string(),
                    target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                }],
                ports: Vec::new(),
            }],
        });

        {
            let conn = state.db.lock().await;
            conn.execute(
                "UPDATE messages SET model_manifest = ?1 WHERE id = 'msg-1'",
                rusqlite::params![serde_json::to_string(&manifest).expect("manifest json")],
            )
            .expect("update manifest");
        }

        let response = handle_ecky_selector_resolve(
            &state,
            &resolver,
            EckySelectorResolveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                target_id: "body:face:0:5-5-5:100".to_string(),
            },
            &test_ctx(),
        )
        .await
        .expect("selector response");

        assert_eq!(response.target_id, "body:face:0:5-5-5:100");
        assert_eq!(
            response.durable_target_id.as_deref(),
            Some("durable-face-1")
        );
        assert_eq!(
            response.canonical_target_id.as_deref(),
            Some("canonical-face-1")
        );
        assert_eq!(response.feature_ids, vec!["lens_bore".to_string()]);
        assert_eq!(response.parameter_keys, vec!["lens_bore_d".to_string()]);
        assert_eq!(
            response.provenance_candidates.feature_role.as_deref(),
            Some("face")
        );
        assert_eq!(
            response.provenance_candidates.operation_kinds,
            vec!["bore".to_string()]
        );
        assert_eq!(
            response.provenance_candidates.primitive_ids,
            vec!["primitive-face-1".to_string()]
        );
        assert_eq!(
            response.provenance_candidates.source_stable_node_keys.len(),
            1
        );
        assert!(!response.provenance_candidates.source_stable_node_keys[0]
            .trim()
            .is_empty());
        assert_eq!(response.confidence, EckySelectorResolveConfidence::Exact);
    }

    #[tokio::test]
    async fn given_alias_collision_when_selector_resolve_then_ambiguous() {
        let (state, resolver) = seed_target_with_macro(
            "Selector",
            "V-selector-ambiguous",
            "(model (params (number width 12)) (part body (box width 2 3)))",
        )
        .await;

        let mut manifest = sample_manifest("model-base");
        manifest.selection_targets[0].alias_ids = vec!["shared-face".to_string()];
        manifest.selection_targets[1].alias_ids = vec!["shared-face".to_string()];
        manifest.selection_targets[0].parameter_keys = vec!["edge_param".to_string()];
        manifest.selection_targets[1].parameter_keys = vec!["face_param".to_string()];

        {
            let conn = state.db.lock().await;
            conn.execute(
                "UPDATE messages SET model_manifest = ?1 WHERE id = 'msg-1'",
                rusqlite::params![serde_json::to_string(&manifest).expect("manifest json")],
            )
            .expect("update manifest");
        }

        let response = handle_ecky_selector_resolve(
            &state,
            &resolver,
            EckySelectorResolveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                target_id: "shared-face".to_string(),
            },
            &test_ctx(),
        )
        .await
        .expect("selector response");

        assert_eq!(
            response.confidence,
            EckySelectorResolveConfidence::Ambiguous
        );
        assert_eq!(response.target_id, "shared-face");
        assert!(response.reason.contains("Alias collision"));
        assert_eq!(response.durable_target_id, None);
        assert_eq!(response.canonical_target_id, None);
        assert_eq!(
            response.parameter_keys,
            vec!["edge_param".to_string(), "face_param".to_string()]
        );
    }

    #[tokio::test]
    async fn given_missing_target_when_selector_resolve_then_none() {
        let (state, resolver) = seed_target_with_macro(
            "Selector",
            "V-selector-none",
            "(model (params (number width 12)) (part body (box width 2 3)))",
        )
        .await;

        let response = handle_ecky_selector_resolve(
            &state,
            &resolver,
            EckySelectorResolveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                target_id: "missing-target".to_string(),
            },
            &test_ctx(),
        )
        .await
        .expect("selector response");

        assert_eq!(response.confidence, EckySelectorResolveConfidence::None);
        assert_eq!(response.target_id, "missing-target");
        assert!(response.reason.contains("No selection target matched"));
        assert!(response.feature_ids.is_empty());
        assert!(response.parameter_keys.is_empty());
        assert!(response.provenance_candidates.feature_role.is_none());
        assert!(response
            .provenance_candidates
            .source_stable_node_keys
            .is_empty());
        assert!(response.provenance_candidates.operation_kinds.is_empty());
        assert!(response.provenance_candidates.primitive_ids.is_empty());
    }

    #[tokio::test]
    async fn given_provided_params_when_ecky_constraints_validate_then_reports_pass_fail_rows() {
        let (state, resolver) = seed_target_with_macro(
            "Constrained",
            "V-constraints",
            "(model (params (number width 12 :min 10 :max 20 :step 2) (select mount inner :options ((Inner inner) (Outer outer)))) (part body (box width 2 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: Some(BTreeMap::from([
                    ("width".to_string(), ParamValue::Number(13.0)),
                    ("mount".to_string(), ParamValue::String("outer".to_string())),
                ])),
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        assert_eq!(response.parameter_source, "provided");
        assert_eq!(response.pass_count, 1);
        assert_eq!(response.fail_count, 1);
        let width = response
            .rows
            .iter()
            .find(|row| row.path == "/params/width")
            .expect("width row");
        assert_eq!(width.status, "fail");
        assert_eq!(width.severity, "error");
        assert_eq!(width.raw_value, serde_json::json!(13.0));
        assert!(width.message.contains("step"));
        assert_eq!(width.involved_param_keys, vec!["width".to_string()]);
        assert_eq!(width.source_stable_node_keys.len(), 1);
        assert!(!width.source_stable_node_keys[0].trim().is_empty());
        let mount = response
            .rows
            .iter()
            .find(|row| row.path == "/params/mount")
            .expect("mount row");
        assert_eq!(mount.status, "pass");
        assert_eq!(mount.severity, "info");
        assert_eq!(mount.involved_param_keys, vec!["mount".to_string()]);
        assert_eq!(mount.source_stable_node_keys.len(), 1);
        assert!(!mount.source_stable_node_keys[0].trim().is_empty());
    }

    #[tokio::test]
    async fn given_missing_params_when_ecky_constraints_validate_then_uses_core_defaults() {
        let (state, resolver) = seed_target_with_macro(
            "Defaults",
            "V-constraints-default",
            "(model (params (number width 12 :min 10 :max 20 :step 2)) (part body (box width 2 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        assert_eq!(response.parameter_source, "initialOrDefault");
        assert_eq!(response.pass_count, 1);
        assert_eq!(response.fail_count, 0);
        assert_eq!(response.rows[0].path, "/params/width");
        assert_eq!(response.rows[0].severity, "info");
        assert_eq!(response.rows[0].raw_value, serde_json::json!(12.0));
        assert_eq!(
            response.rows[0].involved_param_keys,
            vec!["width".to_string()]
        );
        assert_eq!(response.rows[0].source_stable_node_keys.len(), 1);
        assert!(!response.rows[0].source_stable_node_keys[0]
            .trim()
            .is_empty());
    }

    #[tokio::test]
    async fn given_passing_relation_when_ecky_constraints_validate_then_relation_row_passes() {
        let (state, resolver) = seed_target_with_macro(
            "Relation pass",
            "V-relation-pass",
            "(model (params (number lens_bore_d 8) (number tunnel_aperture_h 10) :relations ((< lens_bore_d tunnel_aperture_h))) (part body (box lens_bore_d 2 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: Some(BTreeMap::from([
                    ("lens_bore_d".to_string(), ParamValue::Number(8.0)),
                    ("tunnel_aperture_h".to_string(), ParamValue::Number(10.0)),
                ])),
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        assert_eq!(response.pass_count, 3);
        assert_eq!(response.fail_count, 0);
        let relation = response
            .rows
            .iter()
            .find(|row| row.path == "/params/:relations/0")
            .expect("relation row");
        assert_eq!(relation.status, "pass");
        assert_eq!(relation.severity, "info");
        assert_eq!(
            relation.involved_param_keys,
            vec!["lens_bore_d".to_string(), "tunnel_aperture_h".to_string()]
        );
    }

    #[tokio::test]
    async fn given_failing_relation_when_ecky_constraints_validate_then_relation_row_fails() {
        let (state, resolver) = seed_target_with_macro(
            "Relation fail",
            "V-relation-fail",
            "(model (params (number lens_bore_d 8) (number tunnel_aperture_h 10) :relations ((< lens_bore_d tunnel_aperture_h))) (part body (box lens_bore_d 2 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: Some(BTreeMap::from([
                    ("lens_bore_d".to_string(), ParamValue::Number(12.0)),
                    ("tunnel_aperture_h".to_string(), ParamValue::Number(10.0)),
                ])),
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        assert_eq!(response.pass_count, 2);
        assert_eq!(response.fail_count, 1);
        let relation = response
            .rows
            .iter()
            .find(|row| row.path == "/params/:relations/0")
            .expect("relation row");
        assert_eq!(relation.status, "fail");
        assert_eq!(relation.severity, "error");
        assert!(
            relation.message.contains("Relation < failed"),
            "{}",
            relation.message
        );
        assert_eq!(
            relation.involved_param_keys,
            vec!["lens_bore_d".to_string(), "tunnel_aperture_h".to_string()]
        );
    }

    #[tokio::test]
    async fn given_relation_row_when_ecky_constraints_validate_then_row_includes_relation_metadata()
    {
        let (state, resolver) = seed_target_with_macro(
            "Relation metadata",
            "V-relation-metadata",
            "(model (params (number lens_bore_d 8) (number tunnel_aperture_h 10) :relations ((< lens_bore_d tunnel_aperture_h))) (part body (box lens_bore_d 2 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let relation = response
            .rows
            .iter()
            .find(|row| row.path == "/params/:relations/0")
            .expect("relation row");
        let value = serde_json::to_value(relation).expect("row json");

        assert_eq!(value["constraintId"], "relation_0");
        assert_eq!(value["label"], "Relation #1");
        assert_eq!(value["kind"], "relation");
        assert!(value["sourceStableNodeKey"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty()));
        assert_eq!(
            value["dependsOnParamKeys"],
            serde_json::json!(["lens_bore_d", "tunnel_aperture_h"])
        );
        assert!(value["affectsStableNodeKeys"]
            .as_array()
            .is_some_and(|arr| !arr.is_empty()));
    }

    #[tokio::test]
    async fn given_repeated_anonymous_delta_when_ecky_constraints_validate_then_authoring_lint_suggests_holder_margin_x(
    ) {
        let (state, resolver) = seed_target_with_macro(
            "Anonymous delta lint",
            "V-anonymous-delta-lint",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) (+ holder_w 12) 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let value = serde_json::to_value(response).expect("response json");
        let lints = value["authoringLints"]
            .as_array()
            .expect("authoring lints array");
        assert!(
            !lints.is_empty(),
            "expected at least one authoring lint for repeated anonymous delta"
        );
        assert!(lints.iter().any(|lint| {
            lint["kind"] == "anonymousDelta"
                && lint["paramKey"] == "holder_w"
                && lint["delta"] == 12.0
                && lint["suggestedParamKey"] == "holder_margin_x"
        }));
    }

    #[tokio::test]
    async fn given_single_anonymous_delta_when_ecky_constraints_validate_then_no_authoring_lint() {
        let (state, resolver) = seed_target_with_macro(
            "Anonymous delta no lint",
            "V-anonymous-delta-no-lint",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) 8 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let value = serde_json::to_value(response).expect("response json");
        assert_eq!(value["authoringLints"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn given_repeated_anonymous_delta_when_preview_stored_then_draft_feedback_payload_includes_authoring_lints(
    ) {
        let (state, resolver) = seed_target_with_macro(
            "Anonymous delta draft feedback lint",
            "V-anonymous-delta-feedback-lint",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) (+ holder_w 12) 3)))",
        )
        .await;
        let ctx = test_ctx();

        let mut design_output = sample_design(
            "Anonymous delta draft feedback lint",
            "",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) (+ holder_w 12) 3)))",
        );
        design_output.macro_dialect = MacroDialect::EckyIrV0;
        design_output.engine_kind = crate::models::EngineKind::EckyIrV0;
        design_output.geometry_backend = crate::models::GeometryBackend::EckyRust;
        design_output.source_language = crate::models::SourceLanguage::EckyIrV0;

        let preview = store_session_render_preview(
            &state,
            &resolver,
            &ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: "thread-1".to_string(),
                base_message_id: Some("msg-1".to_string()),
                design_output: design_output.clone(),
                artifact_bundle: sample_bundle("model-feedback-lint", "feedback-lint.stl"),
                model_manifest: sample_manifest("model-feedback-lint"),
                draft_feedback: Some(DraftFeedbackSeed {
                    status: crate::models::AgentDraftFeedbackStatus::Warning,
                    summary: "Draft warnings.".to_string(),
                    items: Vec::new(),
                    authoring_lints: Vec::new(),
                    source: crate::models::AgentDraftFeedbackSource::StructuralVerification,
                }),
            },
        )
        .await
        .expect("store preview");

        let event = crate::contracts::AgentDraftPreviewUpdatedEvent {
            session_id: preview.session_id.clone(),
            thread_id: preview.thread_id.clone(),
            preview_id: preview.preview_id.clone(),
            base_message_id: preview.base_message_id.clone(),
            model_id: Some(preview.artifact_bundle.model_id.clone()),
            design: preview.design_output.clone(),
            artifact_bundle: preview.artifact_bundle.clone(),
            model_manifest: preview.model_manifest.clone(),
            feedback: preview.draft_feedback.clone(),
        };
        let value = serde_json::to_value(&event).expect("event json");
        let lints = value["feedback"]["authoringLints"]
            .as_array()
            .expect("authoring lints array");

        assert!(lints.iter().any(|lint| {
            lint["kind"] == "anonymousDelta"
                && lint["paramKey"] == "holder_w"
                && lint["delta"] == 12.0
                && lint["suggestedParamKey"] == "holder_margin_x"
        }));
    }

    #[tokio::test]
    async fn given_no_repeated_anonymous_delta_when_preview_stored_then_draft_feedback_payload_has_empty_authoring_lints(
    ) {
        let (state, resolver) = seed_target_with_macro(
            "Anonymous delta draft feedback no lint",
            "V-anonymous-delta-feedback-no-lint",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) 8 3)))",
        )
        .await;
        let ctx = test_ctx();

        let mut design_output = sample_design(
            "Anonymous delta draft feedback no lint",
            "",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) 8 3)))",
        );
        design_output.macro_dialect = MacroDialect::EckyIrV0;
        design_output.engine_kind = crate::models::EngineKind::EckyIrV0;
        design_output.geometry_backend = crate::models::GeometryBackend::EckyRust;
        design_output.source_language = crate::models::SourceLanguage::EckyIrV0;

        let preview = store_session_render_preview(
            &state,
            &resolver,
            &ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: "thread-1".to_string(),
                base_message_id: Some("msg-1".to_string()),
                design_output: design_output.clone(),
                artifact_bundle: sample_bundle("model-feedback-no-lint", "feedback-no-lint.stl"),
                model_manifest: sample_manifest("model-feedback-no-lint"),
                draft_feedback: Some(DraftFeedbackSeed {
                    status: crate::models::AgentDraftFeedbackStatus::Passed,
                    summary: "Draft passed.".to_string(),
                    items: Vec::new(),
                    authoring_lints: Vec::new(),
                    source: crate::models::AgentDraftFeedbackSource::StructuralVerification,
                }),
            },
        )
        .await
        .expect("store preview");

        let event = crate::contracts::AgentDraftPreviewUpdatedEvent {
            session_id: preview.session_id.clone(),
            thread_id: preview.thread_id.clone(),
            preview_id: preview.preview_id.clone(),
            base_message_id: preview.base_message_id.clone(),
            model_id: Some(preview.artifact_bundle.model_id.clone()),
            design: preview.design_output.clone(),
            artifact_bundle: preview.artifact_bundle.clone(),
            model_manifest: preview.model_manifest.clone(),
            feedback: preview.draft_feedback.clone(),
        };
        let value = serde_json::to_value(&event).expect("event json");
        assert_eq!(value["feedback"]["authoringLints"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn given_physical_decision_calibration_defaults_when_ecky_constraints_validate_then_relation_rows_pass(
    ) {
        let source =
            include_str!("../../../model-runtime/examples/physical-decision-calibration.ecky");
        let (state, resolver) = seed_target_with_macro(
            "Physical Decision Calibration",
            "V-physical-decision-defaults",
            source,
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let relation_rows = response
            .rows
            .iter()
            .filter(|row| row.path.starts_with("/params/:relations/"))
            .collect::<Vec<_>>();

        assert_eq!(response.parameter_source, "initialOrDefault");
        assert_eq!(response.fail_count, 0);
        assert_eq!(relation_rows.len(), 13);
        assert!(relation_rows.iter().all(|row| row.status == "pass"));
    }

    #[tokio::test]
    async fn given_physical_decision_calibration_overrides_when_ecky_constraints_validate_then_relation_rows_fail_with_expected_involved_keys(
    ) {
        let source =
            include_str!("../../../model-runtime/examples/physical-decision-calibration.ecky");
        let (state, resolver) = seed_target_with_macro(
            "Physical Decision Calibration",
            "V-physical-decision-overrides",
            source,
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: Some(BTreeMap::from([
                    ("thread_clearance".to_string(), ParamValue::Number(0.10)),
                    ("lens_bore_d".to_string(), ParamValue::Number(58.70)),
                ])),
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let failing_relation_rows = response
            .rows
            .iter()
            .filter(|row| row.path.starts_with("/params/:relations/") && row.status == "fail")
            .collect::<Vec<_>>();

        assert_eq!(response.parameter_source, "provided");
        assert!(response.fail_count >= 2);

        let thread_clearance_row = failing_relation_rows
            .iter()
            .find(|row| {
                row.involved_param_keys
                    .iter()
                    .any(|key| key == "thread_clearance")
                    && row
                        .involved_param_keys
                        .iter()
                        .any(|key| key == "thread_clearance_min")
            })
            .expect("thread clearance relation fail row");
        assert!(thread_clearance_row.message.contains("Relation >="));

        let lens_row = failing_relation_rows
            .iter()
            .find(|row| {
                row.involved_param_keys
                    .iter()
                    .any(|key| key == "lens_bore_d")
                    && row
                        .involved_param_keys
                        .iter()
                        .any(|key| key == "lens_fit_floor")
            })
            .expect("lens relation fail row");
        assert!(lens_row.message.contains("Relation >="));
    }

    #[tokio::test]
    async fn given_physical_decision_calibration_failure_when_ecky_constraints_validate_then_failing_relation_includes_source_handles(
    ) {
        let source =
            include_str!("../../../model-runtime/examples/physical-decision-calibration.ecky");
        let (state, resolver) = seed_target_with_macro(
            "Physical Decision Calibration",
            "V-physical-decision-traceability",
            source,
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: Some(BTreeMap::from([(
                    "thread_clearance".to_string(),
                    ParamValue::Number(0.10),
                )])),
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let failing_row = response
            .rows
            .iter()
            .find(|row| {
                row.path.starts_with("/params/:relations/")
                    && row.status == "fail"
                    && row
                        .involved_param_keys
                        .iter()
                        .any(|key| key == "thread_clearance")
                    && row
                        .involved_param_keys
                        .iter()
                        .any(|key| key == "thread_clearance_min")
            })
            .expect("thread clearance failing relation row");

        for key in &failing_row.involved_param_keys {
            let param_row = response
                .rows
                .iter()
                .find(|row| row.path == format!("/params/{key}"))
                .expect("param row for failing key");
            assert!(!param_row.source_stable_node_keys.is_empty());
            assert!(param_row
                .source_stable_node_keys
                .iter()
                .all(|stable_key| !stable_key.trim().is_empty()));
        }
    }

    fn load_physical_decision_calibration_fail_fixture() -> String {
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../model-runtime/examples/physical-decision-calibration-fail.ecky");
        fs::read_to_string(&fixture_path).unwrap_or_else(|_| {
            "(model
                (params
                    (number lens_bore_d 58.70)
                    (number lens_fit_floor 58.80)
                    (number thread_clearance 0.10)
                    (number thread_clearance_min 0.25)
                    :relations
                    (
                        (>= lens_bore_d lens_fit_floor)
                        (>= thread_clearance thread_clearance_min)
                    )
                )
                (part calibration (box 1 1 1))
            )"
            .to_string()
        })
    }

    #[tokio::test]
    async fn given_physical_decision_fail_fixture_when_ecky_constraints_validate_then_multiple_relation_rows_fail(
    ) {
        let source = load_physical_decision_calibration_fail_fixture();
        let (state, resolver) = seed_target_with_macro(
            "Physical Decision Calibration Fail",
            "V-physical-decision-fail-relations",
            &source,
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let failing_relation_rows = response
            .rows
            .iter()
            .filter(|row| row.path.starts_with("/params/:relations/") && row.status == "fail")
            .collect::<Vec<_>>();

        assert!(
            failing_relation_rows.len() >= 2,
            "expected >=2 failing relation rows, got {}",
            failing_relation_rows.len()
        );
        assert!(response.fail_count >= 2);
    }

    #[tokio::test]
    async fn given_physical_decision_fail_fixture_when_ecky_constraints_validate_then_failing_rows_include_keys_and_source_traceability(
    ) {
        let source = load_physical_decision_calibration_fail_fixture();
        let (state, resolver) = seed_target_with_macro(
            "Physical Decision Calibration Fail",
            "V-physical-decision-fail-traceability",
            &source,
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let failing_relation_rows = response
            .rows
            .iter()
            .filter(|row| row.path.starts_with("/params/:relations/") && row.status == "fail")
            .collect::<Vec<_>>();

        assert!(
            !failing_relation_rows.is_empty(),
            "expected failing relation rows"
        );

        for relation_row in failing_relation_rows {
            assert!(
                !relation_row.involved_param_keys.is_empty(),
                "missing involvedParamKeys for {}",
                relation_row.path
            );
            for key in &relation_row.involved_param_keys {
                let param_row = response
                    .rows
                    .iter()
                    .find(|row| row.path == format!("/params/{key}"))
                    .expect("param row for involved key");
                assert!(
                    !param_row.source_stable_node_keys.is_empty(),
                    "missing source_stable_node_keys for {}",
                    param_row.path
                );
                assert!(param_row
                    .source_stable_node_keys
                    .iter()
                    .all(|stable_key| !stable_key.trim().is_empty()));
            }
        }
    }

    #[test]
    fn ecky_ast_replace_source_rewrites_spanned_node_with_digest_guards() {
        let source = "(model (part body (box 1 2 3)))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = "/parts/body/root";
        let node = find_core_ast_node_in_program(&program, &path).expect("node");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = core_node_digest(node);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Replace,
            Some("(box 4 5 6)"),
            None,
        )
        .expect("replace");

        assert_eq!(next, "(model (part body (box 4 5 6)))");
    }

    fn source_edit_digest(source: &str, path: &str) -> String {
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        edit_digest_for_ecky_path(&program, source, path).expect("path digest")
    }

    #[test]
    fn given_param_path_when_replace_then_source_rewrites_param_decl() {
        let source = "(model (params (number width 12)) (part body (box width 2 3)))";
        let path = "/params/width";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Replace,
            Some("(number width 24)"),
            None,
        )
        .expect("replace param");

        assert_eq!(
            next,
            "(model (params (number width 24)) (part body (box width 2 3)))"
        );
    }

    #[test]
    fn given_param_path_when_rename_then_decl_and_refs_update() {
        let source = "(model (params (number width 12)) (part body (box width 2 3)))";
        let path = "/params/width";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Rename,
            None,
            Some("height"),
        )
        .expect("rename param");

        assert_eq!(
            next,
            "(model (params (number height 12)) (part body (box height 2 3)))"
        );
    }

    #[test]
    fn given_part_path_when_rename_then_part_name_updates() {
        let source = "(model (part body (box 1 2 3)) (part cap (sphere 2)))";
        let path = "/parts/cap";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Rename,
            None,
            Some("panel"),
        )
        .expect("rename part");

        assert_eq!(
            next,
            "(model (part body (box 1 2 3)) (part panel (sphere 2)))"
        );
    }

    #[test]
    fn given_ast_arg_path_when_insert_after_then_source_adds_sibling() {
        let source = "(model (part body (union (box 1 2 3) (sphere 4))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = "/parts/body/root/call/args/1";
        let node = find_core_ast_node_in_program(&program, &path).expect("node");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = core_node_digest(node);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::InsertAfter,
            Some("(cylinder 2 8)"),
            None,
        )
        .expect("insert");

        assert_eq!(
            next,
            "(model (part body (union (box 1 2 3) (sphere 4) (cylinder 2 8))))"
        );
    }

    #[test]
    fn given_ast_arg_path_when_delete_then_source_removes_sibling() {
        let source = "(model (part body (union (box 1 2 3) (sphere 4))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = "/parts/body/root/call/args/1";
        let node = find_core_ast_node_in_program(&program, &path).expect("node");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = core_node_digest(node);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Delete,
            None,
            None,
        )
        .expect("delete");

        assert_eq!(next, "(model (part body (union (box 1 2 3))))");
    }

    #[test]
    fn given_ast_keyword_path_when_delete_then_source_removes_keyword_pair() {
        let source = "(model (part body (fillet 2 :edges \"top\" (box 1 2 3))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = "/parts/body/root/call/keywords/edges";
        let node = find_core_ast_node_in_program(&program, &path).expect("node");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = core_node_digest(node);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Delete,
            None,
            None,
        )
        .expect("delete keyword");

        assert_eq!(next, "(model (part body (fillet 2 (box 1 2 3))))");
    }

    #[test]
    fn given_param_path_when_insert_after_then_source_adds_param_sibling() {
        let source = "(model (params (number width 12)) (part body (box width height 3)))";
        let path = "/params/width";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::InsertAfter,
            Some("(number height 6)"),
            None,
        )
        .expect("insert param");

        assert_eq!(
            next,
            "(model (params (number width 12) (number height 6)) (part body (box width height 3)))"
        );
    }

    #[test]
    fn given_part_path_when_insert_after_then_source_adds_part_sibling() {
        let source = "(model (part body (box 1 2 3)))";
        let path = "/parts/body";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::InsertAfter,
            Some("(part lid (sphere 2))"),
            None,
        )
        .expect("insert part");

        assert_eq!(
            next,
            "(model (part body (box 1 2 3)) (part lid (sphere 2)))"
        );
    }

    #[test]
    fn given_part_path_when_delete_then_source_removes_part_clause() {
        let source = "(model (part body (box 1 2 3)) (part lid (sphere 2)))";
        let path = "/parts/lid";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Delete,
            None,
            None,
        )
        .expect("delete part");

        assert_eq!(next, "(model (part body (box 1 2 3)))");
    }

    #[test]
    fn given_build_binding_path_when_replace_then_source_rewrites_shape_value() {
        let source = "(model (part body (build (shape rail (box 1 2 3)) (result rail))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = "/parts/body/root/build/bindings/rail";
        let node = find_core_ast_node_in_program(&program, path).expect("node");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = core_node_digest(node);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Replace,
            Some("(cylinder 2 8)"),
            None,
        )
        .expect("replace build binding");

        assert_eq!(
            next,
            "(model (part body (build (shape rail (cylinder 2 8)) (result rail))))"
        );
    }

    #[test]
    fn given_build_binding_path_when_insert_after_then_source_adds_shape_sibling() {
        let source = "(model (part body (build (shape rail (box 1 2 3)) (result rail))))";
        let path = "/parts/body/root/build/bindings/rail";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::InsertAfter,
            Some("(shape cap (translate 0 0 1 rail))"),
            None,
        )
        .expect("insert build shape");

        assert_eq!(
            next,
            "(model (part body (build (shape rail (box 1 2 3)) (shape cap (translate 0 0 1 rail)) (result rail))))"
        );
    }

    #[test]
    fn given_build_binding_path_when_delete_then_source_removes_shape_clause() {
        let source = "(model (part body (build (shape rail (box 1 2 3)) (shape cap (sphere 2)) (result cap))))";
        let path = "/parts/body/root/build/bindings/rail";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Delete,
            None,
            None,
        )
        .expect("delete build shape");

        assert_eq!(
            next,
            "(model (part body (build (shape cap (sphere 2)) (result cap))))"
        );
    }

    #[test]
    fn given_let_binding_path_when_replace_then_source_rewrites_binding_value() {
        let source = "(model (part body (let ((lift 3)) (translate 0 0 lift (box 1 2 3)))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = core_node_child_paths(&program.parts[0].root, "/parts/body/root")
            .into_iter()
            .find_map(|(path, _)| path.contains("/let/bindings/").then_some(path))
            .expect("let binding path");
        let node = find_core_ast_node_in_program(&program, path.as_str()).expect("node");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = core_node_digest(node);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            &path,
            &node_digest,
            &EckyAstEditOperation::Replace,
            Some("6"),
            None,
        )
        .expect("replace let binding");

        assert_eq!(
            next,
            "(model (part body (let ((lift 6)) (translate 0 0 lift (box 1 2 3)))))"
        );
    }

    #[test]
    fn given_let_binding_path_when_insert_after_then_source_adds_binding_sibling() {
        let source = "(model (part body (let ((lift 3)) (translate 0 0 lift (box 1 2 3)))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = core_node_child_paths(&program.parts[0].root, "/parts/body/root")
            .into_iter()
            .find_map(|(path, _)| path.contains("/let/bindings/").then_some(path))
            .expect("let binding path");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, &path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            &path,
            &node_digest,
            &EckyAstEditOperation::InsertAfter,
            Some("(drop 4)"),
            None,
        )
        .expect("insert let binding");

        assert_eq!(
            next,
            "(model (part body (let ((lift 3) (drop 4)) (translate 0 0 lift (box 1 2 3)))))"
        );
    }

    #[test]
    fn given_let_binding_path_when_delete_then_source_removes_binding_pair() {
        let source =
            "(model (part body (let ((lift 3) (drop 4)) (translate 0 0 drop (box 1 2 3)))))";
        let path = "/parts/body/root/let/bindings/lift";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Delete,
            None,
            None,
        )
        .expect("delete let binding");

        assert_eq!(
            next,
            "(model (part body (let ((drop 4)) (translate 0 0 drop (box 1 2 3)))))"
        );
    }

    #[test]
    fn given_build_binding_path_when_rename_then_refs_update_in_scope() {
        let source = "(model (part body (build (shape rail (box 1 2 3)) (shape cap (translate 0 0 1 rail)) (result cap))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = "/parts/body/root/build/bindings/rail";
        let node = find_core_ast_node_in_program(&program, path).expect("node");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = core_node_digest(node);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            path,
            &node_digest,
            &EckyAstEditOperation::Rename,
            None,
            Some("spine"),
        )
        .expect("rename build binding");

        assert_eq!(
            next,
            "(model (part body (build (shape spine (box 1 2 3)) (shape cap (translate 0 0 1 spine)) (result cap))))"
        );
    }

    #[test]
    fn given_let_binding_path_when_rename_then_body_refs_update_not_binding_value() {
        let source = "(model (part body (let ((lift height)) (translate 0 0 lift (box 1 2 3)))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
        let path = core_node_child_paths(&program.parts[0].root, "/parts/body/root")
            .into_iter()
            .find_map(|(path, _)| path.contains("/let/bindings/").then_some(path))
            .expect("let binding path");
        let node = find_core_ast_node_in_program(&program, path.as_str()).expect("node");
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = core_node_digest(node);

        let next = replace_ecky_ast_source(
            source,
            &source_digest,
            &path,
            &node_digest,
            &EckyAstEditOperation::Rename,
            None,
            Some("zlift"),
        )
        .expect("rename let binding");

        assert_eq!(
            next,
            "(model (part body (let ((zlift height)) (translate 0 0 zlift (box 1 2 3)))))"
        );
    }

    #[test]
    fn ecky_ast_replace_source_rejects_stale_node_digest() {
        let source = "(model (part body (box 1 2 3)))";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);

        let err = replace_ecky_ast_source(
            source,
            &source_digest,
            "/parts/body/root",
            "sha256:not-current",
            &EckyAstEditOperation::Replace,
            Some("(box 4 5 6)"),
            None,
        )
        .expect_err("stale node digest should fail");

        assert!(err.message.contains("node digest mismatch"));
    }

    #[tokio::test]
    async fn given_valid_replace_when_ecky_ast_patch_validate_then_structured_diff_returns_without_render_payload(
    ) {
        let source = "(model (part body (box 1 2 3)))";
        let (state, resolver) = seed_target_with_macro("Box", "V-validate", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/parts/body/root";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let node_digest = source_edit_digest(source, path);

        let response = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest,
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: node_digest.clone(),
                replacement_source: Some("(box 4 5 6)".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("patch validate");

        let value = serde_json::to_value(&response).expect("validate json");
        assert_eq!(value["operation"], "replace");
        assert_eq!(value["editedPath"], path);
        assert_eq!(value["status"], "valid");
        assert_eq!(value["oldNodeDigest"], node_digest);
        assert!(value["newNodeDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert_ne!(value["sourceDigest"], value["newSourceDigest"]);
        assert_eq!(value["affectedPaths"], serde_json::json!([path]));
        assert_eq!(
            value["affectedPathDetails"],
            serde_json::json!([{
                "change": "replace",
                "oldPath": path,
                "newPath": path,
                "oldDigest": value["oldNodeDigest"].clone(),
                "newDigest": value["newNodeDigest"].clone(),
            }])
        );
        assert_eq!(
            value["affectedNodeKeys"].as_array().map(|v| v.len()),
            Some(2)
        );
        assert!(value["affectedNodeKeys"][0]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert!(value["affectedNodeKeys"][1]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert_eq!(
            value["dependencyImpact"]["dependencyKind"],
            serde_json::json!("pathLocal")
        );
        assert_eq!(
            value["dependencyImpact"]["impactedPartIds"],
            serde_json::json!(["body"])
        );
        assert_eq!(
            value["dependencyImpact"]["impactLabels"],
            serde_json::json!(["part-local", "export-affecting"])
        );
        assert!(value["diff"]["old"]["byteLen"].as_u64().unwrap_or(0) > 0);
        assert!(value["diff"]["new"]["byteLen"].as_u64().unwrap_or(0) > 0);
        assert!(value["diff"]["old"]["digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert!(value["diff"]["new"]["digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert!(value.get("macroCode").is_none());
        assert!(value.get("artifactBundle").is_none());
        assert!(value.get("modelManifest").is_none());
        assert!(value.get("artifactDigest").is_none());
        assert!(value.get("draft").is_none());
    }

    #[tokio::test]
    async fn given_stable_node_key_when_ecky_ast_patch_validate_then_patch_resolves_path() {
        let source = "(model (part body (box 1 2 3)))";
        let (state, resolver) = seed_target_with_macro("Box", "V-validate-key", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/parts/body/root";
        let node_digest = source_edit_digest(source, path);

        let ast = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: Some(path.to_string()),
                depth: Some(0),
                max_nodes: Some(8),
                include_source: Some(false),
            },
            &test_ctx(),
        )
        .await
        .expect("ast response");
        let stable_node_key = ast
            .nodes
            .first()
            .map(|node| node.stable_node_key.clone())
            .expect("stable node key");

        let response = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: Some(stable_node_key),
                path: None,
                expected_node_digest: node_digest,
                replacement_source: Some("(box 9 9 9)".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("patch validate");

        assert_eq!(response.edited_path, path);
    }

    #[tokio::test]
    async fn given_bogus_stable_node_key_when_ecky_ast_patch_validate_then_rejects_cleanly() {
        let source = "(model (part body (box 1 2 3)))";
        let (state, resolver) = seed_target_with_macro("Box", "V-validate-bogus-key", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let err = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: Some("sha256:not-a-real-node".to_string()),
                path: None,
                expected_node_digest: source_edit_digest(source, "/parts/body/root"),
                replacement_source: Some("(box 4 5 6)".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("bogus stable node key");

        assert!(err.message.contains("stableNodeKey not found in AST"));
    }

    #[tokio::test]
    async fn given_mismatched_stable_node_key_and_path_when_ecky_ast_patch_validate_then_rejects() {
        let source = "(model (part body (box 1 2 3)))";
        let (state, resolver) =
            seed_target_with_macro("Box", "V-validate-key-mismatch", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let key_path = "/parts/body/root";

        let ast = handle_ecky_ast_get(
            &state,
            &resolver,
            EckyAstGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: Some(key_path.to_string()),
                depth: Some(0),
                max_nodes: Some(8),
                include_source: Some(false),
            },
            &test_ctx(),
        )
        .await
        .expect("ast response");
        let stable_node_key = ast
            .nodes
            .first()
            .map(|node| node.stable_node_key.clone())
            .expect("stable node key");

        let err = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: Some(stable_node_key),
                path: Some("/parts/body/root/call/args/0".to_string()),
                expected_node_digest: source_edit_digest(source, key_path),
                replacement_source: Some("9".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("key/path mismatch");

        assert!(err.message.contains("stableNodeKey/path mismatch"));
    }

    #[tokio::test]
    async fn given_param_patch_when_ecky_ast_patch_validate_then_dependency_impact_uses_param_helpers(
    ) {
        let source =
            "(model (params (number width 12) (number height 6)) (part body (box width height 3)))";
        let (state, resolver) =
            seed_target_with_macro("Params", "V-validate-param-impact", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/params/width";

        let response = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: source_edit_digest(source, path),
                replacement_source: Some("(number width 24)".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("param patch validate");

        let value = serde_json::to_value(&response).expect("param patch json");
        assert_eq!(
            value["dependencyImpact"]["dependencyKind"],
            serde_json::json!("parameterReference")
        );
        assert_eq!(
            value["dependencyImpact"]["path"],
            serde_json::json!("/params/width")
        );
        assert_eq!(
            value["dependencyImpact"]["impactedPartIds"],
            serde_json::json!(["body"])
        );
        assert_eq!(
            value["dependencyImpact"]["dependentSourcePaths"],
            serde_json::json!(["/parts/body/root/call/args/0"])
        );
        assert_eq!(
            value["dependencyImpact"]["referenceCount"],
            serde_json::json!(1)
        );
    }

    #[tokio::test]
    async fn given_film_coupon_fixture_when_film_gap_patch_validate_then_patch_preview_renders() {
        let source =
            include_str!("../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
        let (state, resolver) =
            seed_target_with_macro("Film Coupon", "V-film-coupon", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/params/film_gap";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let expected_node_digest = source_edit_digest(source, path);
        let replacement_source =
            "(number film_gap 0.45 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01)".to_string();

        let validate = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: source_digest.clone(),
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: expected_node_digest.clone(),
                replacement_source: Some(replacement_source.clone()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("film gap patch validate");

        assert_eq!(validate.status, "valid");
        assert_eq!(validate.edited_path, path);
        assert_ne!(validate.source_digest, validate.new_source_digest);
        assert_eq!(
            validate
                .dependency_impact
                .as_ref()
                .map(|impact| impact.dependency_kind.as_str()),
            Some("parameterReference")
        );

        let preview = handle_ecky_ast_replace_and_render(
            &state,
            &resolver,
            EckyAstReplaceAndRenderRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest,
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest,
                replacement_source: Some(replacement_source),
                new_name: None,
                parameters: None,
                post_processing: None,
                geometry_backend: None,
            },
            &test_ctx(),
        )
        .await
        .expect("film gap patch preview");

        assert_eq!(preview.thread_id, "thread-1");
        assert_ne!(preview.message_id, "msg-1");
        assert!(preview.macro_code.contains("(number film_gap 0.45"));
        assert_eq!(
            preview.artifact_bundle.source_language,
            crate::models::SourceLanguage::EckyIrV0
        );
        assert!(!preview.artifact_bundle.preview_stl_path.trim().is_empty());
    }

    #[tokio::test]
    async fn given_film_coupon_fixture_when_film_gap_patch_preview_then_commit_returns_model_id_and_digest(
    ) {
        let source =
            include_str!("../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
        let (state, resolver) =
            seed_target_with_macro("Film Coupon", "V-film-coupon-commit", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/params/film_gap";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let expected_node_digest = source_edit_digest(source, path);
        let replacement_source =
            "(number film_gap 0.53 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01)".to_string();

        let preview = handle_ecky_ast_replace_and_render(
            &state,
            &resolver,
            EckyAstReplaceAndRenderRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest,
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest,
                replacement_source: Some(replacement_source),
                new_name: None,
                parameters: None,
                post_processing: None,
                geometry_backend: None,
            },
            &test_ctx(),
        )
        .await
        .expect("film gap preview for commit");

        assert_eq!(preview.thread_id, "thread-1");
        assert_eq!(
            preview.artifact_digest.model_id,
            preview.artifact_bundle.model_id
        );
        assert_eq!(
            preview.artifact_digest.content_hash,
            preview.artifact_bundle.content_hash
        );

        let commit = handle_commit_preview_version(
            &state,
            &resolver,
            VersionSaveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some(preview.thread_id.clone()),
                message_id: Some(preview.message_id.clone()),
                title: Some("Film Coupon Committed".to_string()),
                version_name: Some("V-film-gap-commit".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("commit film gap preview");

        assert_eq!(commit.thread_id, "thread-1");
        assert_eq!(commit.model_id, preview.artifact_bundle.model_id);

        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").expect("thread messages");
        let committed = messages
            .iter()
            .find(|message| message.id == commit.message_id)
            .expect("committed message");
        assert!(committed
            .output
            .as_ref()
            .expect("committed output")
            .macro_code
            .contains("(number film_gap 0.53"));
    }

    #[tokio::test]
    async fn given_wrapper_param_path_when_ecky_ast_replace_and_render_then_only_numeric_token_changes_and_preview_renders(
    ) {
        let source = "(model\n  (params\n    ; keep formatting + comment\n    (number film_gap 0.35 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01))\n  (part body (box film_gap 2 3)))";
        let (state, resolver) =
            seed_target_with_macro("Wrapper Path", "V-wrapper-number", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/params/film_gap";
        let source_digest = crate::mcp::macro_buffer::source_digest(source);
        let expected_node_digest = source_edit_digest(source, path);

        let preview = handle_ecky_ast_replace_and_render(
            &state,
            &resolver,
            EckyAstReplaceAndRenderRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest,
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest,
                replacement_source: Some(
                    "(number film_gap 0.45 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01)"
                        .to_string(),
                ),
                new_name: None,
                parameters: None,
                post_processing: None,
                geometry_backend: None,
            },
            &test_ctx(),
        )
        .await
        .expect("wrapper-path numeric preview");

        assert!(preview.macro_code.contains("; keep formatting + comment"));
        assert!(preview
            .macro_code
            .contains("(number film_gap 0.45 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01)"));
        assert!(!preview.macro_code.contains("(number film_gap 0.35 "));
        assert!(!preview.artifact_bundle.preview_stl_path.trim().is_empty());
    }

    #[tokio::test]
    async fn given_ecky_ast_shape_patch_when_params_omitted_then_preview_preserves_current_values()
    {
        let source =
            "(model (params (number width 10) (number height 3)) (part body (box width height 2)))";
        let (state, resolver) =
            seed_target_with_macro("Preserve Params", "V-preserve-params", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        {
            let conn = state.db.lock().await;
            let mut messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
            let mut message = messages.pop().expect("seed message");
            let mut output = message.output.take().expect("output");
            output
                .initial_params
                .insert("width".to_string(), ParamValue::Number(42.0));
            output
                .initial_params
                .insert("height".to_string(), ParamValue::Number(9.0));
            db::update_message_status_and_output(
                &conn,
                "msg-1",
                db::MessageStatusUpdate {
                    status: &MessageStatus::Success,
                    output: Some(&output),
                    usage: None,
                    artifact_bundle: message.artifact_bundle.as_ref(),
                    model_manifest: message.model_manifest.as_ref(),
                    visual_kind: None,
                    content: Some("Base version"),
                },
            )
            .expect("update params");
        }

        let preview = handle_ecky_ast_replace_and_render(
            &state,
            &resolver,
            EckyAstReplaceAndRenderRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some("/parts/body".to_string()),
                expected_node_digest: source_edit_digest(source, "/parts/body"),
                replacement_source: Some("(part body (box width height 4))".to_string()),
                new_name: None,
                parameters: None,
                post_processing: None,
                geometry_backend: None,
            },
            &test_ctx(),
        )
        .await
        .expect("shape patch preview");

        assert_eq!(
            preview.initial_params.get("width"),
            Some(&ParamValue::Number(42.0))
        );
        assert_eq!(
            preview.initial_params.get("height"),
            Some(&ParamValue::Number(9.0))
        );
    }

    #[tokio::test]
    async fn given_macro_preview_on_existing_target_when_agent_passes_parameters_then_current_values_win(
    ) {
        let source =
            "(model (params (number width 10) (number height 3)) (part body (box width height 2)))";
        let (state, resolver) =
            seed_target_with_macro("Macro Params", "V-macro-params", source).await;
        {
            let conn = state.db.lock().await;
            let mut messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
            let mut message = messages.pop().expect("seed message");
            let mut output = message.output.take().expect("output");
            output
                .initial_params
                .insert("width".to_string(), ParamValue::Number(42.0));
            output
                .initial_params
                .insert("height".to_string(), ParamValue::Number(9.0));
            db::update_message_status_and_output(
                &conn,
                "msg-1",
                db::MessageStatusUpdate {
                    status: &MessageStatus::Success,
                    output: Some(&output),
                    usage: None,
                    artifact_bundle: message.artifact_bundle.as_ref(),
                    model_manifest: message.model_manifest.as_ref(),
                    visual_kind: None,
                    content: Some("Base version"),
                },
            )
            .expect("update params");
        }

        let preview = handle_macro_preview_render(
            &state,
            &resolver,
            MacroReplaceRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                macro_code:
                    "(model (params (number width 10) (number height 3)) (part body (box width height 4)))"
                        .to_string(),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                ui_spec: None,
                parameters: Some(BTreeMap::from([
                    ("width".to_string(), ParamValue::Number(999.0)),
                    ("height".to_string(), ParamValue::Number(888.0)),
                ])),
                post_processing: None,
                geometry_backend: None,
            },
            &test_ctx(),
        )
        .await
        .expect("macro preview");

        assert_eq!(
            preview.initial_params.get("width"),
            Some(&ParamValue::Number(42.0))
        );
        assert_eq!(
            preview.initial_params.get("height"),
            Some(&ParamValue::Number(9.0))
        );
    }

    #[tokio::test]
    async fn given_macro_preview_adds_first_params_to_existing_target_then_new_macro_defaults_win()
    {
        let source = "(model (part body (box 1 2 3)))";
        let (state, resolver) =
            seed_target_with_macro("First Params", "V-first-params", source).await;
        {
            let conn = state.db.lock().await;
            let mut messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
            let mut message = messages.pop().expect("seed message");
            let mut output = message.output.take().expect("output");
            output.initial_params.clear();
            db::update_message_status_and_output(
                &conn,
                "msg-1",
                db::MessageStatusUpdate {
                    status: &MessageStatus::Success,
                    output: Some(&output),
                    usage: None,
                    artifact_bundle: message.artifact_bundle.as_ref(),
                    model_manifest: message.model_manifest.as_ref(),
                    visual_kind: None,
                    content: Some("Base version"),
                },
            )
            .expect("clear params");
        }

        let preview = handle_macro_preview_render(
            &state,
            &resolver,
            MacroReplaceRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                macro_code:
                    "(model (params (number width 100) (number height 7)) (part body (box width height 4)))"
                        .to_string(),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                ui_spec: None,
                parameters: Some(BTreeMap::from([
                    ("width".to_string(), ParamValue::Number(999.0)),
                    ("height".to_string(), ParamValue::Number(888.0)),
                ])),
                post_processing: None,
                geometry_backend: None,
            },
            &test_ctx(),
        )
        .await
        .expect("macro preview");

        assert_eq!(
            preview.initial_params.get("width"),
            Some(&ParamValue::Number(100.0))
        );
        assert_eq!(
            preview.initial_params.get("height"),
            Some(&ParamValue::Number(7.0))
        );
    }

    #[tokio::test]
    async fn given_lens_bore_patch_when_ecky_ast_patch_validate_then_dependency_scope_stays_on_bore_controls(
    ) {
        let source = "(model (params (number lens_bore_d 42) (number wall_t 3)) (part bore_carrier (cylinder lens_bore_d 6)) (part wall (box wall_t wall_t 4)))";
        let (state, resolver) =
            seed_target_with_macro("Lens Bore Scope", "V-lens-bore-scope", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/params/lens_bore_d";

        let response = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: source_edit_digest(source, path),
                replacement_source: Some("(number lens_bore_d 44)".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("lens bore patch validate");

        assert_eq!(response.status, "valid");
        assert_eq!(response.edited_path, path);
        assert_eq!(response.affected_paths, vec![path.to_string()]);
        let impact = response
            .dependency_impact
            .as_ref()
            .expect("dependency impact");
        assert_eq!(impact.path, path);
        assert_eq!(impact.dependency_kind, "parameterReference");
        assert_eq!(impact.impacted_part_ids, vec!["bore_carrier".to_string()]);
        assert_eq!(
            impact.dependent_source_paths,
            vec!["/parts/bore_carrier/root/call/args/0".to_string()]
        );
        assert_eq!(impact.reference_count, 1);
        assert!(!impact.impacted_part_ids.iter().any(|id| id == "wall"));
        assert!(!impact
            .dependent_source_paths
            .iter()
            .any(|path| path.contains("/parts/wall/")));
    }

    #[tokio::test]
    async fn given_lens_bore_dependency_fixture_when_ecky_dependency_get_then_downstream_roles_return_together(
    ) {
        let source = "(model
  (params (number lens_bore_d 42))
  (part carrier (cylinder lens_bore_d 6))
  (part socket (cylinder lens_bore_d 5))
  (part thread (cylinder lens_bore_d 4))
  (part stop_lip (cylinder lens_bore_d 3)))";
        let (state, resolver) =
            seed_target_with_macro("Lens Bore Dependency", "V-lens-bore-deps", source).await;

        let response = handle_ecky_dependency_get(
            &state,
            &resolver,
            EckyDependencyGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                path: "/params/lens_bore_d".to_string(),
            },
            &test_ctx(),
        )
        .await
        .expect("lens bore dependency");

        assert_eq!(response.path, "/params/lens_bore_d");
        assert_eq!(response.dependency_kind, "parameterReference");
        let mut impacted = response.impacted_part_ids.clone();
        impacted.sort();
        assert_eq!(
            impacted,
            vec![
                "carrier".to_string(),
                "socket".to_string(),
                "stop_lip".to_string(),
                "thread".to_string(),
            ]
        );
        assert_eq!(response.reference_count, 4);
        assert_eq!(
            response.dependent_source_paths,
            vec![
                "/parts/carrier/root/call/args/0".to_string(),
                "/parts/socket/root/call/args/0".to_string(),
                "/parts/thread/root/call/args/0".to_string(),
                "/parts/stop_lip/root/call/args/0".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn given_stale_source_digest_when_ecky_ast_patch_validate_then_rejects() {
        let source = "(model (part body (box 1 2 3)))";
        let (state, resolver) =
            seed_target_with_macro("Box", "V-validate-stale-source", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/parts/body/root";

        let err = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: "sha256:stale".to_string(),
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: source_edit_digest(source, path),
                replacement_source: Some("(box 4 5 6)".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("stale source digest");

        assert!(err.message.contains("digest mismatch"));
    }

    #[tokio::test]
    async fn given_stale_node_digest_when_ecky_ast_patch_validate_then_rejects() {
        let source = "(model (part body (box 1 2 3)))";
        let (state, resolver) =
            seed_target_with_macro("Box", "V-validate-stale-node", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let err = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some("/parts/body/root".to_string()),
                expected_node_digest: "sha256:not-current".to_string(),
                replacement_source: Some("(box 4 5 6)".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("stale node digest");

        assert!(err.message.contains("node digest mismatch"));
    }

    #[tokio::test]
    async fn given_invalid_replacement_when_ecky_ast_patch_validate_then_rejects_before_render() {
        let source = "(model (part body (box 1 2 3)))";
        let (state, resolver) = seed_target_with_macro("Box", "V-validate-invalid", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/parts/body/root";

        let err = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: source_edit_digest(source, path),
                replacement_source: Some("(box 4 5 6))".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("invalid replacement");

        assert!(err
            .message
            .contains("Replacement produced invalid Ecky source"));
        assert_eq!(err.operation.as_deref(), Some("replace"));
        assert!(err
            .stable_node_key
            .as_deref()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert!(err.start_line.is_some());
        assert!(err.end_line.is_some());
        assert!(err.start_line.unwrap() <= err.end_line.unwrap());
    }

    #[tokio::test]
    async fn given_helical_ridge_parse_failure_when_patch_validate_then_error_keeps_stable_key_and_span_lines(
    ) {
        let source = "(model
  (part body
    (helical-ridge
      (cylinder 12 8)
      :pitch 2
      :height 6)))";
        let (state, resolver) =
            seed_target_with_macro("Helical Ridge", "V-helical-ridge-err", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/parts/body/root";

        let err = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: source_edit_digest(source, path),
                replacement_source: Some(
                    "(helical-ridge (cylinder 12 8) :pitch 2 :height 6))".to_string(),
                ),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("helical ridge parse failure");

        assert!(err
            .message
            .contains("Replacement produced invalid Ecky source"));
        assert_eq!(err.operation.as_deref(), Some("replace"));
        assert!(err
            .stable_node_key
            .as_deref()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert!(err.start_line.is_some());
        assert!(err.end_line.is_some());
        assert!(err.start_line.unwrap() <= err.end_line.unwrap());
        assert!(err.message.contains("/parts/body/root"), "{err:?}");
    }

    #[tokio::test]
    async fn given_render_lowering_failures_when_macro_preview_render_then_mcp_error_keeps_diagnostics_and_raw_details(
    ) {
        let lowering_source = r#"(model
  (part body
    (banana-boolean
      (box 10 10 10)
      (sphere 4))))"#;
        let (state, resolver) =
            seed_target_with_macro("Lowering Fail", "V-lowering-fail", lowering_source).await;

        let lowering_err = handle_macro_preview_render(
            &state,
            &resolver,
            MacroReplaceRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                macro_code: lowering_source.to_string(),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                ui_spec: None,
                parameters: None,
                post_processing: None,
                geometry_backend: Some(crate::models::GeometryBackend::Build123d),
            },
            &test_ctx(),
        )
        .await
        .expect_err("build123d lowering should fail for unsupported operation");

        assert_eq!(lowering_err.operation.as_deref(), Some("lower:build123d"));
        assert!(
            lowering_err
                .details
                .as_deref()
                .is_some_and(|details| details.contains("banana-boolean")),
            "{lowering_err:?}"
        );

        let malformed_source = "(model\n  (part body (box 1 2 3))\n$)";
        let (state, resolver) =
            seed_target_with_macro("Malformed", "V-malformed", malformed_source).await;
        let malformed_err = handle_macro_preview_render(
            &state,
            &resolver,
            MacroReplaceRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                macro_code: malformed_source.to_string(),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                ui_spec: None,
                parameters: None,
                post_processing: None,
                geometry_backend: Some(crate::models::GeometryBackend::Build123d),
            },
            &test_ctx(),
        )
        .await
        .expect_err("invalid Ecky source should fail in lowering path");

        assert_eq!(malformed_err.operation.as_deref(), Some("lower:build123d"));
        assert!(malformed_err
            .message
            .contains("Expected a proper list for model form."));
        assert!(
            malformed_err
                .start_line
                .zip(malformed_err.end_line)
                .is_none_or(|(start, end)| start <= end),
            "{malformed_err:?}"
        );
        if let Some(stable_node_key) = malformed_err.stable_node_key.as_deref() {
            assert!(stable_node_key.starts_with("sha256:"), "{malformed_err:?}");
        }
    }

    #[tokio::test]
    async fn given_delete_operation_when_ecky_ast_patch_validate_then_returns_valid_delete_diff() {
        let source = "(model (part body (build (shape rail (box 1 2 3)) (shape cap (sphere 2)) (result cap))))";
        let (state, resolver) = seed_target_with_macro("Delete", "V-validate-delete", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/parts/body/root/build/bindings/rail";

        let response = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Delete,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: source_edit_digest(source, path),
                replacement_source: None,
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("delete validate");

        let value = serde_json::to_value(&response).expect("delete json");
        assert_eq!(value["operation"], "delete");
        assert_eq!(value["status"], "valid");
        assert_eq!(value["newNodeDigest"], "deleted");
    }

    #[tokio::test]
    async fn given_rename_operation_when_ecky_ast_patch_validate_then_returns_new_path() {
        let source = "(model (part body (build (shape rail (box 1 2 3)) (shape cap (translate 0 0 1 rail)) (result cap))))";
        let (state, resolver) = seed_target_with_macro("Rename", "V-validate-rename", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
        let path = "/parts/body/root/build/bindings/rail";

        let response = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Rename,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some(path.to_string()),
                expected_node_digest: source_edit_digest(source, path),
                replacement_source: None,
                new_name: Some("spine".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("rename validate");

        let value = serde_json::to_value(&response).expect("rename json");
        assert_eq!(value["operation"], "rename");
        assert_eq!(
            value["affectedPathDetails"][0]["newPath"],
            "/parts/body/root/build/bindings/spine"
        );
        assert_ne!(value["newNodeDigest"], "deleted");
    }

    #[tokio::test]
    async fn given_non_source_addressable_path_when_ecky_ast_patch_validate_then_rejects() {
        let source = "(model (params (toggle raised true)) (part body (if raised (sphere 10) (cylinder 10 20))))";
        let (state, resolver) =
            seed_target_with_macro("Conditional", "V-validate-path", source).await;
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let err = handle_ecky_ast_patch_validate(
            &state,
            &resolver,
            EckyAstPatchValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                operation: EckyAstEditOperation::Replace,
                source_digest: crate::mcp::macro_buffer::source_digest(source),
                stable_node_key: None,
                path: Some("/parts/body/root/if/condition".to_string()),
                expected_node_digest: "sha256:not-used".to_string(),
                replacement_source: Some("raised".to_string()),
                new_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("non source-addressable path");

        assert!(err.message.contains("not source-span addressable"));
    }

    #[tokio::test]
    async fn version_restore_returns_artifact_digest_for_export_truth() {
        let (state, _resolver) = seed_target().await;
        let response = handle_version_restore(
            &state,
            VersionRestoreRequest {
                identity: AgentIdentityOverride::default(),
                message_id: "msg-1".to_string(),
            },
            &test_ctx(),
        )
        .await
        .expect("version restore");

        let artifact_digest = response.artifact_digest.expect("artifact digest");
        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.message_id, "msg-1");
        assert_eq!(artifact_digest.model_id, "model-base");
        assert!(artifact_digest.has_step_export);
        assert_eq!(
            artifact_digest.step_export_path.as_deref(),
            Some("/tmp/model-base.step")
        );
    }

    #[tokio::test]
    async fn target_detail_get_returns_requested_ui_spec_only() {
        let (state, resolver) = seed_target().await;
        let response = handle_target_detail_get(
            &state,
            &resolver,
            TargetDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                section: TargetDetailSection::UiSpec,
                shape_graph_filters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("target uiSpec detail");

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["section"], "uiSpec");
        assert_eq!(value["authoringContext"]["sourceLanguage"], "legacyPython");
        assert_eq!(value["authoringContext"]["geometryBackend"], "freecad");
        assert!(value["authoringContext"]["authoringCard"]
            .as_str()
            .unwrap()
            .contains("Ecky authoring card"));
        assert!(value.get("uiSpec").is_some());
        assert!(value.get("initialParams").is_none());
        assert!(value.get("artifactBundle").is_none());
        assert!(value.get("latestDraft").is_none());
    }

    #[tokio::test]
    async fn target_detail_get_returns_requested_initial_params_only() {
        let (state, resolver) = seed_target().await;
        let response = handle_target_detail_get(
            &state,
            &resolver,
            TargetDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                section: TargetDetailSection::InitialParams,
                shape_graph_filters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("target params detail");

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["section"], "initialParams");
        assert_eq!(value["initialParams"]["diameter"], 130.0);
        assert!(value.get("uiSpec").is_none());
        assert!(value.get("artifactBundle").is_none());
        assert!(value.get("latestDraft").is_none());
    }

    #[tokio::test]
    async fn target_detail_get_returns_active_artifact_bundle_only() {
        let (state, resolver) = seed_target().await;
        let response = handle_target_detail_get(
            &state,
            &resolver,
            TargetDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                section: TargetDetailSection::ArtifactBundle,
                shape_graph_filters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("target artifact detail");

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["section"], "artifactBundle");
        assert_eq!(value["artifactBundle"]["modelId"], "model-base");
        assert_eq!(value["artifactBundle"]["sourceLanguage"], "legacyPython");
        assert_eq!(value["artifactBundle"]["geometryBackend"], "freecad");
        assert_eq!(value["artifactBundle"]["hasPreviewStl"], true);
        assert_eq!(
            value["artifactBundle"]["exportFormats"],
            serde_json::json!(["step"])
        );
        assert_eq!(value["artifactBundle"]["hasStepExport"], true);
        assert_eq!(
            value["artifactBundle"]["stepExportPath"],
            "/tmp/model-base.step"
        );
        assert!(value.get("uiSpec").is_none());
        assert!(value.get("initialParams").is_none());
        assert!(value.get("latestDraft").is_none());
    }

    #[tokio::test]
    async fn target_detail_get_shape_graph_returns_compact_packet_without_source_text() {
        let source = "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part body (box holder_w holder_h 3)))";
        let (state, resolver) = seed_target_with_macro("ShapeGraph", "V-shape-graph", source).await;
        let response = handle_target_detail_get(
            &state,
            &resolver,
            TargetDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                section: TargetDetailSection::ShapeGraph,
                shape_graph_filters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("target shape graph detail");

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["section"], "shapeGraph");
        assert!(value.get("macroCode").is_none());
        assert!(value.get("uiSpec").is_none());
        assert_eq!(value["shapeGraph"]["parts"]["items"][0]["partId"], "body");
        assert_eq!(
            value["shapeGraph"]["constraints"]["items"][0]["kind"],
            "relation"
        );
        assert_eq!(
            value["shapeGraph"]["dependencies"]["items"][0]["parameterKey"],
            "holder_w"
        );
        assert!(value["shapeGraph"]["sourceDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert!(value["shapeGraph"]["coreDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
    }

    #[tokio::test]
    async fn target_detail_get_shape_graph_honors_section_filters() {
        let source = "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part body (box holder_w holder_h 3)))";
        let (state, resolver) =
            seed_target_with_macro("ShapeGraphFilter", "V-shape-graph-filter", source).await;
        let response = handle_target_detail_get(
            &state,
            &resolver,
            TargetDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                section: TargetDetailSection::ShapeGraph,
                shape_graph_filters: Some(vec![ShapeGraphFilterSection::Constraints]),
            },
            &test_ctx(),
        )
        .await
        .expect("target filtered shape graph detail");

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["section"], "shapeGraph");
        assert!(value["shapeGraph"].get("constraints").is_some());
        assert!(value["shapeGraph"].get("parts").is_none());
        assert!(value["shapeGraph"].get("instances").is_none());
        assert!(value["shapeGraph"].get("dependencies").is_none());
    }

    #[tokio::test]
    async fn given_agent_needs_intent_without_full_source_when_target_detail_get_section_shape_graph_then_returns_parts_constraints_dependencies_without_macro_or_source_payload(
    ) {
        let source = "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part body (box holder_w holder_h 3)))";
        let (state, resolver) =
            seed_target_with_macro("ShapeGraphIntent", "V-shape-graph-intent", source).await;
        let response = handle_target_detail_get(
            &state,
            &resolver,
            TargetDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                section: TargetDetailSection::ShapeGraph,
                shape_graph_filters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("target shape graph detail");

        let value = serde_json::to_value(&response).expect("response json");
        assert_eq!(value["section"], "shapeGraph");
        assert!(value["shapeGraph"].get("parts").is_some());
        assert!(value["shapeGraph"].get("constraints").is_some());
        assert!(value["shapeGraph"].get("dependencies").is_some());
        assert!(value.get("macroCode").is_none());
        assert!(value.get("macro_code").is_none());
        assert!(value["shapeGraph"]["parts"]["items"][0]
            .get("source")
            .is_none());
        assert!(value["shapeGraph"]["constraints"]["items"][0]
            .get("source")
            .is_none());
        assert!(value["shapeGraph"]["dependencies"]["items"][0]
            .get("source")
            .is_none());
    }

    #[tokio::test]
    async fn given_agent_validates_physical_edits_when_ecky_constraints_validate_with_repeated_anonymous_offsets_then_returns_actionable_authoring_lint_and_relation_metadata(
    ) {
        let (state, resolver) = seed_target_with_macro(
            "Anonymous delta relation metadata",
            "V-anonymous-delta-relation-metadata",
            "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part holder (box (+ holder_w 12) (+ holder_w 12) 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        let relation = response
            .rows
            .iter()
            .find(|row| row.path == "/params/:relations/0")
            .expect("relation row");
        assert_eq!(relation.constraint_id.as_deref(), Some("relation_0"));
        assert_eq!(relation.kind.as_deref(), Some("relation"));
        assert_eq!(
            relation.depends_on_param_keys,
            vec!["holder_h".to_string(), "holder_w".to_string()]
        );
        assert!(relation
            .source_stable_node_key
            .as_ref()
            .is_some_and(|key| !key.trim().is_empty()));
        assert!(!relation.affects_stable_node_keys.is_empty());

        let lint = response
            .authoring_lints
            .iter()
            .find(|lint| {
                lint.kind == "anonymousDelta" && lint.param_key == "holder_w" && lint.delta == 12.0
            })
            .expect("anonymous delta lint");
        assert_eq!(lint.suggested_param_key, "holder_margin_x");
        assert!(lint.message.contains("holder_margin_x"));
        assert!(!lint.source_stable_node_keys.is_empty());
    }

    #[tokio::test]
    async fn given_no_repeated_offsets_when_ecky_constraints_validate_then_returns_no_authoring_lint_noise(
    ) {
        let (state, resolver) = seed_target_with_macro(
            "No repeated anonymous delta",
            "V-no-repeated-anonymous-delta",
            "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part holder (box (+ holder_w 12) holder_h 3)))",
        )
        .await;

        let response = handle_ecky_constraints_validate(
            &state,
            &resolver,
            EckyConstraintsValidateRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                parameters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("constraint validation");

        assert!(response.authoring_lints.is_empty());
    }

    #[tokio::test]
    async fn artifact_manifest_get_returns_full_valid_runtime_manifest() {
        let (state, resolver) = seed_target().await;
        let response = handle_artifact_manifest_get(
            &state,
            &resolver,
            ArtifactManifestRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: None,
            },
            &test_ctx(),
        )
        .await
        .expect("artifact manifest");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.message_id, "msg-1");
        assert_eq!(response.model_id, "model-base");
        assert!(response.runtime_manifest_valid);
        assert_eq!(response.digest.model_id, "model-base");
        assert_eq!(response.digest.geometry_backend, "freecad");
        assert!(response.digest.has_step_export);
        assert_eq!(
            response.digest.step_export_path.as_deref(),
            Some("/tmp/model-base.step")
        );
        assert_eq!(response.artifact_bundle.model_id, "model-base");
        assert_eq!(response.model_manifest.model_id, "model-base");

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["runtimeManifestValid"], true);
        assert_eq!(
            value["artifactBundle"]["exportArtifacts"][0]["format"],
            "step"
        );
        assert_eq!(
            value["modelManifest"]["controlPrimitives"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn artifact_manifest_get_rejects_bundle_manifest_mismatch() {
        let (state, resolver) = seed_target().await;
        let mut bad_bundle = sample_bundle("model-bad", "bad.stl");
        bad_bundle
            .export_artifacts
            .push(crate::models::ExportArtifact {
                label: "STEP".to_string(),
                format: "step".to_string(),
                path: "/tmp/model-bad.step".to_string(),
                role: "cad-exchange".to_string(),
            });
        let bad_manifest = sample_manifest("model-other");
        {
            let conn = state.db.lock().await;
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "msg-bad".to_string(),
                    role: MessageRole::Assistant,
                    content: "Bad version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("Bad", "V-bad", "bad_macro()")),
                    usage: None,
                    artifact_bundle: Some(bad_bundle),
                    model_manifest: Some(bad_manifest),
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now_secs() + 1,
                },
            )
            .unwrap();
        }

        let err = handle_artifact_manifest_get(
            &state,
            &resolver,
            ArtifactManifestRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-bad".to_string()),
                model_id: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("mismatched runtime manifest should be rejected");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("model id"), "{}", err.message);
    }

    #[tokio::test]
    async fn artifact_feature_graph_get_reads_runtime_manifest_and_returns_backfilled_graphs() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-feature-graph";
        let mut bundle = sample_bundle(model_id, "feature-graph.stl");
        bundle.export_artifacts.push(crate::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: "/tmp/generated-feature-graph.step".to_string(),
            role: "cad-exchange".to_string(),
        });
        let mut runtime_manifest = sample_manifest(model_id);
        runtime_manifest.correspondence_graph = Some(crate::models::CorrespondenceGraph {
            edges: vec![crate::models::CorrespondenceEdge {
                edge_id: "edge-1".to_string(),
                source: crate::models::FeatureOutputRef {
                    feature_id: "part:body".to_string(),
                    output_id: "selectionTargets".to_string(),
                    target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                },
                target: crate::models::FeatureOutputRef {
                    feature_id: "part:body".to_string(),
                    output_id: "selectionTargets".to_string(),
                    target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                },
                relation: "sameTopology".to_string(),
                source_ref: None,
            }],
        });
        let (stored_bundle, _stored_manifest) = crate::model_runtime::write_runtime_bundle(
            &resolver,
            model_id,
            &bundle,
            &runtime_manifest,
        )
        .expect("runtime bundle");
        let stale_message_manifest = sample_manifest(model_id);
        assert!(stale_message_manifest.feature_graph.is_none());
        assert!(stale_message_manifest.correspondence_graph.is_none());
        {
            let conn = state.db.lock().await;
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "msg-feature-graph".to_string(),
                    role: MessageRole::Assistant,
                    content: "Feature graph version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("Graph", "V-graph", "graph_macro()")),
                    usage: None,
                    artifact_bundle: Some(stored_bundle),
                    model_manifest: Some(stale_message_manifest),
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now_secs() + 1,
                },
            )
            .unwrap();
        }

        let response = handle_artifact_feature_graph_get(
            &state,
            &resolver,
            ArtifactFeatureGraphGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-feature-graph".to_string()),
                model_id: None,
            },
            &test_ctx(),
        )
        .await
        .expect("feature graph");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.message_id, "msg-feature-graph");
        assert_eq!(response.model_id, model_id);
        assert_eq!(response.artifact_digest.model_id, model_id);
        assert!(response.artifact_digest.has_step_export);
        let feature_graph = response.feature_graph.as_ref().expect("feature graph");
        assert_eq!(feature_graph.nodes.len(), 1);
        assert_eq!(feature_graph.nodes[0].feature_id, "part:body");
        assert_eq!(
            feature_graph.nodes[0].output_refs[0].target_ids,
            vec![
                "body:edge:0:0-0-0_10-0-0".to_string(),
                "body:face:0:5-5-5:100".to_string()
            ]
        );
        assert_eq!(
            response
                .correspondence_graph
                .as_ref()
                .expect("correspondence graph")
                .edges[0]
                .edge_id,
            "edge-1"
        );

        let value = serde_json::to_value(&response).expect("feature graph json");
        assert_eq!(value["modelId"], model_id);
        assert!(value["artifactDigest"]["hasStepExport"].as_bool().unwrap());
        assert_eq!(value["featureGraph"]["nodes"][0]["featureId"], "part:body");
        assert_eq!(
            value["correspondenceGraph"]["edges"][0]["relation"],
            "sameTopology"
        );
    }

    #[tokio::test]
    async fn artifact_feature_graph_get_preserves_feature_ports() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-feature-ports";
        let bundle = sample_bundle(model_id, "feature-ports.stl");
        let mut runtime_manifest = sample_manifest(model_id);
        runtime_manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "part:body".to_string(),
                kind: "part".to_string(),
                label: "Body".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-main".to_string()),
                    path: Some("/parts/body/root".to_string()),
                    start_byte: Some(0),
                    end_byte: Some(42),
                }),
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "part:body".to_string(),
                    output_id: "selectionTargets".to_string(),
                    target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                }],
                ports: vec![crate::models::FeaturePort {
                    port_id: "mount-face".to_string(),
                    type_id: "mechanical.mount".to_string(),
                    target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                    frame: Some(crate::models::PortFrame::identity()),
                    interfaces: vec!["m3-clearance".to_string()],
                    params: std::collections::BTreeMap::from([(
                        "clearanceMm".to_string(),
                        crate::models::ComponentInterfaceValue::Number(0.3),
                    )]),
                    source_ref: None,
                    confidence: Some(0.85),
                    target_role: Some("mountingFace".to_string()),
                }],
            }],
        });
        let (stored_bundle, _stored_manifest) = crate::model_runtime::write_runtime_bundle(
            &resolver,
            model_id,
            &bundle,
            &runtime_manifest,
        )
        .expect("runtime bundle");
        {
            let conn = state.db.lock().await;
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "msg-feature-ports".to_string(),
                    role: MessageRole::Assistant,
                    content: "Feature ports version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("Ports", "V-ports", "ports_macro()")),
                    usage: None,
                    artifact_bundle: Some(stored_bundle),
                    model_manifest: Some(sample_manifest(model_id)),
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now_secs() + 1,
                },
            )
            .unwrap();
        }

        let response = handle_artifact_feature_graph_get(
            &state,
            &resolver,
            ArtifactFeatureGraphGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-feature-ports".to_string()),
                model_id: None,
            },
            &test_ctx(),
        )
        .await
        .expect("feature graph");

        let port = &response
            .feature_graph
            .as_ref()
            .expect("feature graph")
            .nodes[0]
            .ports[0];
        assert_eq!(port.port_id, "mount-face");
        assert_eq!(port.target_ids, vec!["body:face:0:5-5-5:100"]);
        assert_eq!(port.interfaces, vec!["m3-clearance"]);
        assert_eq!(port.confidence, Some(0.85));
        assert_eq!(port.target_role.as_deref(), Some("mountingFace"));

        let value = serde_json::to_value(&response).expect("feature ports json");
        assert_eq!(
            value["featureGraph"]["nodes"][0]["ports"][0]["portId"],
            "mount-face"
        );
        assert_eq!(
            value["featureGraph"]["nodes"][0]["ports"][0]["params"]["clearanceMm"],
            0.3
        );
    }

    #[tokio::test]
    async fn artifact_feature_graph_get_film_adapter_fixture_exposes_expected_kinds_source_keys_and_targets(
    ) {
        let source =
            include_str!("../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
        let model_id = "generated-film-adapter-feature-graph";
        let (state, resolver, _) =
            seed_ecky_printability_target(source, model_id, "film-adapter-feature-graph.stl").await;
        let bundle = crate::model_runtime::read_artifact_bundle(&resolver, model_id)
            .expect("runtime bundle");
        let mut manifest = crate::model_runtime::read_model_manifest(&resolver, model_id)
            .expect("runtime manifest");
        let rendered_target_ids = manifest
            .selection_targets
            .iter()
            .filter_map(|target| target.target_id.clone())
            .collect::<Vec<_>>();
        assert!(
            rendered_target_ids.len() >= 2,
            "expected seeded manifest selection targets"
        );
        let edge_target_id = rendered_target_ids[0].clone();
        let face_target_id = rendered_target_ids[1].clone();
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![
                crate::models::FeatureNode {
                    feature_id: "film_path".to_string(),
                    kind: "film_path".to_string(),
                    label: "Film Path".to_string(),
                    source_ref: Some(crate::models::SourceRef {
                        source_id: Some("source-film-path".to_string()),
                        path: Some("/parts/body/film_path".to_string()),
                        start_byte: Some(10),
                        end_byte: Some(40),
                    }),
                    dependency_ids: Vec::new(),
                    output_refs: vec![crate::models::FeatureOutputRef {
                        feature_id: "film_path".to_string(),
                        output_id: "film-path-solid".to_string(),
                        target_ids: vec![edge_target_id.clone()],
                    }],
                    ports: Vec::new(),
                },
                crate::models::FeatureNode {
                    feature_id: "insert_clamp".to_string(),
                    kind: "insert_clamp".to_string(),
                    label: "Insert Clamp".to_string(),
                    source_ref: Some(crate::models::SourceRef {
                        source_id: Some("source-insert-clamp".to_string()),
                        path: Some("/parts/body/insert_clamp".to_string()),
                        start_byte: Some(50),
                        end_byte: Some(80),
                    }),
                    dependency_ids: vec!["film_path".to_string()],
                    output_refs: vec![crate::models::FeatureOutputRef {
                        feature_id: "insert_clamp".to_string(),
                        output_id: "insert-clamp-solid".to_string(),
                        target_ids: vec![face_target_id.clone()],
                    }],
                    ports: Vec::new(),
                },
                crate::models::FeatureNode {
                    feature_id: "helicoid_thread".to_string(),
                    kind: "helicoid_thread".to_string(),
                    label: "Helicoid Thread".to_string(),
                    source_ref: Some(crate::models::SourceRef {
                        source_id: Some("source-helicoid-thread".to_string()),
                        path: Some("/parts/body/helicoid_thread".to_string()),
                        start_byte: Some(90),
                        end_byte: Some(130),
                    }),
                    dependency_ids: vec!["insert_clamp".to_string()],
                    output_refs: vec![crate::models::FeatureOutputRef {
                        feature_id: "helicoid_thread".to_string(),
                        output_id: "helicoid-thread-solid".to_string(),
                        target_ids: vec![face_target_id.clone()],
                    }],
                    ports: Vec::new(),
                },
                crate::models::FeatureNode {
                    feature_id: "lens_bore".to_string(),
                    kind: "lens_bore".to_string(),
                    label: "Lens Bore".to_string(),
                    source_ref: Some(crate::models::SourceRef {
                        source_id: Some("source-lens-bore".to_string()),
                        path: Some("/parts/body/lens_bore".to_string()),
                        start_byte: Some(140),
                        end_byte: Some(170),
                    }),
                    dependency_ids: vec!["helicoid_thread".to_string()],
                    output_refs: vec![crate::models::FeatureOutputRef {
                        feature_id: "lens_bore".to_string(),
                        output_id: "lens-bore-solid".to_string(),
                        target_ids: vec![edge_target_id.clone()],
                    }],
                    ports: Vec::new(),
                },
            ],
        });
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle with explicit film adapter graph");

        let response = handle_artifact_feature_graph_get(
            &state,
            &resolver,
            ArtifactFeatureGraphGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: Some(model_id.to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("feature graph from film adapter fixture");

        let graph = response.feature_graph.expect("feature graph");
        assert!(!graph.nodes.is_empty());
        assert_eq!(graph.nodes.len(), 4);
        let expected = [
            (
                "film_path",
                "film_path",
                "source-film-path",
                "/parts/body/film_path",
                vec![edge_target_id.clone()],
            ),
            (
                "insert_clamp",
                "insert_clamp",
                "source-insert-clamp",
                "/parts/body/insert_clamp",
                vec![face_target_id.clone()],
            ),
            (
                "helicoid_thread",
                "helicoid_thread",
                "source-helicoid-thread",
                "/parts/body/helicoid_thread",
                vec![face_target_id.clone()],
            ),
            (
                "lens_bore",
                "lens_bore",
                "source-lens-bore",
                "/parts/body/lens_bore",
                vec![edge_target_id.clone()],
            ),
        ];
        for (feature_id, kind, source_id, path, target_ids) in expected {
            let node = graph
                .nodes
                .iter()
                .find(|node| node.feature_id == feature_id)
                .expect("expected feature node");
            assert_eq!(node.kind, kind);
            let source_ref = node.source_ref.as_ref().expect("feature source ref");
            assert_eq!(source_ref.source_id.as_deref(), Some(source_id));
            assert_eq!(source_ref.path.as_deref(), Some(path));
            assert_eq!(node.output_refs.len(), 1);
            assert_eq!(node.output_refs[0].target_ids, target_ids);
            assert!(
                node.output_refs[0]
                    .target_ids
                    .iter()
                    .all(|target_id| rendered_target_ids.contains(target_id)),
                "feature {feature_id} must anchor only rendered target ids"
            );
        }
    }

    #[tokio::test]
    async fn artifact_feature_graph_get_reports_validation_when_runtime_manifest_missing() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-no-feature-manifest";
        let bundle = sample_bundle(model_id, "no-feature-manifest.stl");
        let stored_bundle =
            crate::model_runtime::write_artifact_bundle(&resolver, model_id, &bundle)
                .expect("artifact bundle");
        {
            let conn = state.db.lock().await;
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "msg-no-feature-manifest".to_string(),
                    role: MessageRole::Assistant,
                    content: "No manifest version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("No Manifest", "V-none", "none_macro()")),
                    usage: None,
                    artifact_bundle: Some(stored_bundle),
                    model_manifest: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now_secs() + 1,
                },
            )
            .unwrap();
        }

        let err = handle_artifact_feature_graph_get(
            &state,
            &resolver,
            ArtifactFeatureGraphGetRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-no-feature-manifest".to_string()),
                model_id: None,
            },
            &test_ctx(),
        )
        .await
        .expect_err("missing manifest should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("No model manifest found"));
        assert!(err.message.contains(model_id));
        assert!(err.message.contains("artifact_feature_graph_get"));
    }

    #[tokio::test]
    async fn structural_verification_summary_includes_artifact_digest() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-verify";
        let mut bundle = sample_bundle(model_id, "verify.stl");
        bundle.export_artifacts.push(crate::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: "/tmp/generated-verify.step".to_string(),
            role: "cad-exchange".to_string(),
        });
        let manifest = sample_manifest(model_id);
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");

        let response = handle_structural_verification_summary(
            &state, &resolver, "thread-1", "msg-1", model_id,
        )
        .expect("verification summary");

        assert_eq!(response.artifact_digest.model_id, model_id);
        assert!(response.artifact_digest.has_step_export);
        assert_eq!(
            response.artifact_digest.step_export_path.as_deref(),
            Some("/tmp/generated-verify.step")
        );
    }

    #[tokio::test]
    async fn verify_generated_model_merges_authored_verify_failure_into_structural_result() {
        let source = r#"
            (model
              (verify
                (tag body_shell)
                (metric check (manifest has-step))
                (expect check (= false)))
              (part body (box 10 10 10)))
        "#;
        let model_id = "generated-authored-verify-fail";
        let (state, resolver) =
            seed_ecky_verify_target(source, model_id, "authored-verify-fail.stl", true).await;

        let response =
            handle_verify_generated_model(&state, &resolver, "thread-1", "msg-1", model_id, "")
                .expect("verification response");

        assert!(!response.result.passed);
        assert!(response
            .result
            .issues
            .iter()
            .any(|issue| issue.code == "AUTHORED_VERIFY_FAILED"));
        assert!(response.result.summary.contains("AUTHORED_VERIFY_FAILED"));
    }

    #[tokio::test]
    async fn verify_generated_model_surfaces_authored_verify_errors() {
        let source = r#"
            (model
              (verify
                (tag body_shell)
                (metric check (bogus has-step))
                (expect check (= true)))
              (part body (box 10 10 10)))
        "#;
        let model_id = "generated-authored-verify-error";
        let (state, resolver) =
            seed_ecky_verify_target(source, model_id, "authored-verify-error.stl", true).await;

        let response =
            handle_verify_generated_model(&state, &resolver, "thread-1", "msg-1", model_id, "")
                .expect("verification response");

        assert!(!response.result.passed);
        assert!(response
            .result
            .issues
            .iter()
            .any(|issue| issue.code == "AUTHORED_VERIFY_ERROR"));
    }

    #[tokio::test]
    async fn structural_verification_summary_reflects_authored_verify_failures() {
        let source = r#"
            (model
              (verify
                (tag body_shell)
                (metric check (manifest has-step))
                (expect check (= false)))
              (part body (box 10 10 10)))
        "#;
        let model_id = "generated-authored-verify-summary";
        let (state, resolver) =
            seed_ecky_verify_target(source, model_id, "authored-verify-summary.stl", true).await;

        let response = handle_structural_verification_summary(
            &state, &resolver, "thread-1", "msg-1", model_id,
        )
        .expect("verification summary");

        assert!(!response.passed);
        assert_eq!(response.issue_count, 1);
        assert!(response.summary.contains("AUTHORED_VERIFY_FAILED"));
    }

    #[tokio::test]
    async fn printability_analyze_reads_preview_stl_and_includes_artifact_digest() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-printability";
        let preview_stl_path = resolver.root.join("printability-preview.stl");
        write_closed_tetra_binary_stl(&preview_stl_path);
        let mut bundle = sample_bundle(model_id, "printability-preview.stl");
        bundle.preview_stl_path = preview_stl_path.display().to_string();
        bundle.export_artifacts.push(crate::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: "/tmp/generated-printability.step".to_string(),
            role: "cad-exchange".to_string(),
        });
        let manifest = sample_manifest(model_id);
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");

        let response =
            handle_printability_analyze(&state, &resolver, "thread-1", "msg-1", model_id)
                .expect("printability analysis");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.message_id, "msg-1");
        assert_eq!(response.model_id, model_id);
        assert_eq!(response.artifact_digest.model_id, model_id);
        assert!(response.artifact_digest.has_step_export);
        assert_eq!(
            response.preview_stl_path,
            preview_stl_path.display().to_string()
        );
        assert_eq!(response.analysis.triangle_count, 4);
        assert_eq!(response.analysis.topology.component_count, Some(1));
        assert_eq!(response.analysis.risk_metrics.bridge_span_mm, Some(1.0));
        assert_eq!(response.analysis.risk_metrics.thin_wall_mm, Some(1.0));

        let value = serde_json::to_value(&response).expect("printability json");
        assert_eq!(value["artifactDigest"]["modelId"], model_id);
        assert_eq!(
            value["previewStlPath"],
            preview_stl_path.display().to_string()
        );
        assert_eq!(value["analysis"]["triangleCount"], 4);
        assert_eq!(value["analysis"]["topology"]["componentCount"], 1);
        assert_eq!(value["analysis"]["riskMetrics"]["bridgeSpanMm"], 1.0);
        assert_eq!(value["analysis"]["riskMetrics"]["thinWallMm"], 1.0);
    }

    #[tokio::test]
    async fn printability_analyze_anchors_suggestions_when_feature_graph_has_one_clear_target() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-printability-anchor";
        let preview_stl_path = resolver.root.join("printability-anchor-preview.stl");
        write_binary_stl(
            &preview_stl_path,
            &[
                [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
            ],
        );
        let mut bundle = sample_bundle(model_id, "printability-anchor-preview.stl");
        bundle.preview_stl_path = preview_stl_path.display().to_string();
        let mut manifest = sample_manifest(model_id);
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "feature-ledge".to_string(),
                kind: "extrude".to_string(),
                label: "Ledge".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-main".to_string()),
                    path: Some("/parts/body/ledge".to_string()),
                    start_byte: Some(12),
                    end_byte: Some(42),
                }),
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "feature-ledge".to_string(),
                    output_id: "solid".to_string(),
                    target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                }],
                ports: Vec::new(),
            }],
        });
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");

        let response =
            handle_printability_analyze(&state, &resolver, "thread-1", "msg-1", model_id)
                .expect("printability analysis");

        let suggestions = &response.analysis.transform_suggestions;
        assert!(
            !suggestions.is_empty(),
            "expected transform suggestions for overhang mesh"
        );
        assert_eq!(
            response.analysis.risk_metrics.unsupported_island_count,
            Some(1)
        );
        let split_suggestion = suggestions
            .iter()
            .find(|suggestion| {
                suggestion.kind
                    == crate::services::printability::PrintabilityTransformSuggestionKind::Split
            })
            .expect("split suggestion");
        assert_eq!(split_suggestion.unsupported_island_count, Some(1));
        assert!(suggestions.iter().all(|suggestion| {
            suggestion.source_anchor.as_deref()
                == Some("feature:feature-ledge@source:source-main:/parts/body/ledge:12-42")
        }));
        assert!(suggestions
            .iter()
            .all(|suggestion| suggestion.risk_anchor.as_ref().is_some_and(
                |risk_anchor| risk_anchor.feature_id.as_deref() == Some("feature-ledge")
                    && risk_anchor.target_ids == vec!["body:face:0:5-5-5:100".to_string()]
                    && risk_anchor.stable_node_keys.is_empty()
            )));

        let value = serde_json::to_value(&response).expect("printability json");
        assert_eq!(
            value["analysis"]["transformSuggestions"][0]["riskAnchor"]["featureId"],
            "feature-ledge"
        );
        assert_eq!(
            value["analysis"]["transformSuggestions"][0]["riskAnchor"]["targetIds"][0],
            "body:face:0:5-5-5:100"
        );
        assert!(
            value["analysis"]["transformSuggestions"][0]["riskAnchor"]["stableNodeKeys"].is_null()
        );
        assert_eq!(
            value["analysis"]["riskMetrics"]["unsupportedIslandCount"],
            1
        );
        let split_suggestion_json = value["analysis"]["transformSuggestions"]
            .as_array()
            .expect("transform suggestions array")
            .iter()
            .find(|suggestion| suggestion["kind"] == "split")
            .expect("split suggestion json");
        assert_eq!(split_suggestion_json["unsupportedIslandCount"], 1);
    }

    #[tokio::test]
    async fn printability_helicoid_fixture_analysis_and_recipes_include_risk_suggestions_and_anchors(
    ) {
        let source =
            include_str!("../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
        let model_id = "generated-printability-helicoid-fixture";
        let (state, resolver, _) = seed_ecky_printability_target(
            source,
            model_id,
            "printability-helicoid-fixture-preview.stl",
        )
        .await;

        let mut bundle = crate::model_runtime::read_artifact_bundle(&resolver, model_id)
            .expect("runtime bundle");
        bundle.face_targets.push(crate::models::ViewerFaceTarget {
            target_id: "body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.HelicoidFace".to_string(),
            editable: true,
            center: crate::models::ViewerEdgePoint {
                x: 5.0,
                y: 5.0,
                z: 5.0,
            },
            normal: Some([0.0, 0.0, 1.0]),
            area: Some(100.0),
        });
        let mut manifest = crate::model_runtime::read_model_manifest(&resolver, model_id)
            .expect("runtime manifest");
        manifest
            .selection_targets
            .push(crate::models::SelectionTarget {
                target_id: Some("body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string()),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "body".to_string(),
                label: "Body.HelicoidFace".to_string(),
                kind: crate::models::SelectionTargetKind::Face,
                editable: true,
                parameter_keys: Vec::new(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            });
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "feature-helicoid-thread".to_string(),
                kind: "helical-ridge".to_string(),
                label: "Helicoid Thread".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-main".to_string()),
                    path: Some("/parts/body/helicoid".to_string()),
                    start_byte: Some(320),
                    end_byte: Some(420),
                }),
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "feature-helicoid-thread".to_string(),
                    output_id: "solid".to_string(),
                    target_ids: vec![
                        "body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string()
                    ],
                }],
                ports: Vec::new(),
            }],
        });
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle with helicoid feature graph");

        let analyze_response =
            handle_printability_analyze(&state, &resolver, "thread-1", "msg-1", model_id)
                .expect("printability analysis");
        assert!(analyze_response
            .analysis
            .transform_suggestions
            .iter()
            .any(|suggestion| {
                suggestion.kind
                    == crate::services::printability::PrintabilityTransformSuggestionKind::Split
                    || suggestion.kind
                        == crate::services::printability::PrintabilityTransformSuggestionKind::OrientationHint
            }));
        assert!(analyze_response
            .analysis
            .transform_suggestions
            .iter()
            .all(|suggestion| suggestion.risk_anchor.as_ref().is_some_and(
                |risk_anchor| risk_anchor.feature_id.as_deref() == Some("feature-helicoid-thread")
                    && risk_anchor.target_ids
                        == vec!["body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string()]
                    && risk_anchor.stable_node_keys == vec!["body.helicoid".to_string()]
            )));

        let analyze_json = serde_json::to_value(&analyze_response).expect("analysis json");
        assert_eq!(
            analyze_json["analysis"]["transformSuggestions"][0]["riskAnchor"]["featureId"],
            "feature-helicoid-thread"
        );
        assert_eq!(
            analyze_json["analysis"]["transformSuggestions"][0]["riskAnchor"]["stableNodeKeys"][0],
            "body.helicoid"
        );

        let recipes_response = handle_printability_transform_recipes_get(
            &state, &resolver, "thread-1", "msg-1", model_id,
        )
        .expect("transform recipes");
        assert!(recipes_response.recipes.iter().any(|recipe| {
            recipe.action_kind
                == crate::services::printability::SupportlessFdmRecipeActionKind::Clearance
                || recipe.action_kind
                    == crate::services::printability::SupportlessFdmRecipeActionKind::Reorient
        }));
        assert!(recipes_response.recipes.iter().all(|recipe| {
            recipe.risk_anchor.as_ref().is_some_and(|risk_anchor| {
                risk_anchor.feature_id.as_deref() == Some("feature-helicoid-thread")
                    && risk_anchor.target_ids
                        == vec!["body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string()]
                    && risk_anchor.stable_node_keys == vec!["body.helicoid".to_string()]
            })
        }));
    }

    #[tokio::test]
    async fn printability_analyze_preserves_empty_anchor_when_feature_graph_is_ambiguous() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-printability-ambiguous-anchor";
        let preview_stl_path = resolver
            .root
            .join("printability-ambiguous-anchor-preview.stl");
        write_binary_stl(
            &preview_stl_path,
            &[
                [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
            ],
        );
        let mut bundle = sample_bundle(model_id, "printability-ambiguous-anchor-preview.stl");
        bundle.preview_stl_path = preview_stl_path.display().to_string();
        let mut manifest = sample_manifest(model_id);
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![
                crate::models::FeatureNode {
                    feature_id: "feature-left".to_string(),
                    kind: "part".to_string(),
                    label: "Left".to_string(),
                    source_ref: None,
                    dependency_ids: Vec::new(),
                    output_refs: Vec::new(),
                    ports: Vec::new(),
                },
                crate::models::FeatureNode {
                    feature_id: "feature-right".to_string(),
                    kind: "part".to_string(),
                    label: "Right".to_string(),
                    source_ref: None,
                    dependency_ids: Vec::new(),
                    output_refs: Vec::new(),
                    ports: Vec::new(),
                },
            ],
        });
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");

        let response =
            handle_printability_analyze(&state, &resolver, "thread-1", "msg-1", model_id)
                .expect("printability analysis");

        let suggestions = response.analysis.transform_suggestions;
        assert!(
            !suggestions.is_empty(),
            "expected transform suggestions for overhang mesh"
        );
        assert!(suggestions
            .iter()
            .all(|suggestion| suggestion.source_anchor.is_none()));
    }

    #[tokio::test]
    async fn printability_transform_recipes_get_returns_digest_guarded_overhang_recipes() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-printability-recipes";
        let preview_stl_path = resolver.root.join("printability-recipes-preview.stl");
        write_binary_stl(
            &preview_stl_path,
            &[
                [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
            ],
        );
        let mut bundle = sample_bundle(model_id, "printability-recipes-preview.stl");
        bundle.preview_stl_path = preview_stl_path.display().to_string();
        let mut manifest = sample_manifest(model_id);
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "feature-ledge".to_string(),
                kind: "extrude".to_string(),
                label: "Ledge".to_string(),
                source_ref: None,
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "feature-ledge".to_string(),
                    output_id: "solid".to_string(),
                    target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                }],
                ports: Vec::new(),
            }],
        });
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");

        let response = handle_printability_transform_recipes_get(
            &state, &resolver, "thread-1", "msg-1", model_id,
        )
        .expect("transform recipes");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.message_id, "msg-1");
        assert_eq!(response.model_id, model_id);
        assert_eq!(response.artifact_digest.model_id, model_id);
        assert_eq!(
            response.preview_stl_path,
            preview_stl_path.display().to_string()
        );
        let recipe = response
            .recipes
            .iter()
            .find(|recipe| {
                recipe.action_kind
                    == crate::services::printability::SupportlessFdmRecipeActionKind::Reorient
            })
            .expect("reorient recipe");
        assert_eq!(
            recipe.source_anchor.as_deref(),
            Some("feature:feature-ledge")
        );
        assert_eq!(recipe.target.as_deref(), Some("rotateX270"));
        assert_eq!(
            recipe.preview_support_status,
            crate::services::printability::TransformRecipeSupportStatus::Pending
        );
        assert_eq!(
            recipe.apply_support_status,
            crate::services::printability::TransformRecipeSupportStatus::Unsupported
        );
        assert!(recipe
            .risk_anchor
            .as_ref()
            .is_some_and(
                |risk_anchor| risk_anchor.feature_id.as_deref() == Some("feature-ledge")
                    && risk_anchor.target_ids == vec!["body:face:0:5-5-5:100".to_string()]
                    && risk_anchor.stable_node_keys.is_empty()
            ));
        assert!(response.recipes.iter().any(|recipe| {
            recipe.action_kind
                == crate::services::printability::SupportlessFdmRecipeActionKind::Relief
        }));
        let clearance = response
            .recipes
            .iter()
            .find(|recipe| {
                recipe.action_kind
                    == crate::services::printability::SupportlessFdmRecipeActionKind::Clearance
            })
            .expect("clearance recipe");
        assert_eq!(clearance.bridge_span_mm, Some(1.0));
        assert_eq!(clearance.thin_wall_mm, Some(1.0));
        assert_eq!(clearance.unsupported_island_count, Some(1));

        let value = serde_json::to_value(&response).expect("recipes json");
        assert_eq!(value["artifactDigest"]["modelId"], model_id);
        assert_eq!(
            value["artifactDigest"]["contentHash"],
            format!("hash-{model_id}")
        );
        assert_eq!(value["recipes"][0]["previewSupportStatus"], "pending");
        assert_eq!(value["recipes"][0]["applySupportStatus"], "unsupported");
        assert_eq!(
            value["recipes"][0]["riskAnchor"]["featureId"],
            "feature-ledge"
        );
        let clearance_json = value["recipes"]
            .as_array()
            .expect("recipes array")
            .iter()
            .find(|recipe| recipe["actionKind"] == "clearance")
            .expect("clearance recipe json");
        assert_eq!(clearance_json["bridgeSpanMm"], 1.0);
        assert_eq!(clearance_json["thinWallMm"], 1.0);
        assert_eq!(clearance_json["unsupportedIslandCount"], 1);
    }

    #[tokio::test]
    async fn printability_transform_recipes_get_returns_empty_for_no_risk_stl() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-printability-no-risk-recipes";
        let preview_stl_path = resolver.root.join("printability-no-risk-preview.stl");
        // A unit tetra reads as a 1.00 mm thin wall (below the 1.20 mm
        // advisory); scale it up so the mesh is genuinely risk-free.
        let triangles = [
            [[0.0f32, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            [[0.0f32, 0.0, 0.0], [0.0, 0.0, 10.0], [10.0, 0.0, 0.0]],
            [[0.0f32, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
            [[10.0f32, 0.0, 0.0], [0.0, 0.0, 10.0], [0.0, 10.0, 0.0]],
        ];
        write_binary_stl(&preview_stl_path, &triangles);
        let mut bundle = sample_bundle(model_id, "printability-no-risk-preview.stl");
        bundle.preview_stl_path = preview_stl_path.display().to_string();
        let manifest = sample_manifest(model_id);
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");

        let response = handle_printability_transform_recipes_get(
            &state, &resolver, "thread-1", "msg-1", model_id,
        )
        .expect("transform recipes");

        assert!(response.recipes.is_empty(), "{:?}", response.recipes);
    }

    #[tokio::test]
    async fn printability_transform_recipes_get_reports_missing_preview_stl() {
        let (state, resolver) = seed_target().await;
        let model_id = "generated-printability-missing-preview";
        let mut bundle = sample_bundle(model_id, "missing-preview.stl");
        bundle.preview_stl_path.clear();
        let manifest = sample_manifest(model_id);
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");

        let err = handle_printability_transform_recipes_get(
            &state, &resolver, "thread-1", "msg-1", model_id,
        )
        .expect_err("missing preview STL should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert_eq!(err.message, "Artifact bundle has no preview STL path.");
    }

    async fn seed_ecky_printability_target(
        source: &str,
        model_id: &str,
        preview_name: &str,
    ) -> (AppState, TestPathResolver, SemanticTransformArtifactGuard) {
        let (state, resolver) = seed_target_with_macro("Ecky Pot", "V-ecky", source).await;
        let preview_stl_path = resolver.root.join(preview_name);
        write_binary_stl(
            &preview_stl_path,
            &[
                [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
            ],
        );
        let source_path = resolver.root.join(format!("{model_id}.ecky"));
        fs::write(&source_path, source).expect("write ecky source");

        let mut design = sample_design("Ecky Pot", "V-ecky", source);
        design.macro_dialect = MacroDialect::EckyIrV0;
        design.engine_kind = crate::models::EngineKind::EckyIrV0;
        design.geometry_backend = crate::models::GeometryBackend::EckyRust;
        design.source_language = crate::models::SourceLanguage::EckyIrV0;
        design.post_processing = None;

        let mut bundle = sample_bundle(model_id, preview_name);
        bundle.engine_kind = crate::models::EngineKind::EckyIrV0;
        bundle.geometry_backend = crate::models::GeometryBackend::EckyRust;
        bundle.source_language = crate::models::SourceLanguage::EckyIrV0;
        bundle.content_hash = format!("content-{model_id}");
        bundle.macro_path = Some(source_path.display().to_string());
        bundle.preview_stl_path = preview_stl_path.display().to_string();

        let mut manifest = sample_manifest(model_id);
        manifest.engine_kind = crate::models::EngineKind::EckyIrV0;
        manifest.geometry_backend = crate::models::GeometryBackend::EckyRust;
        manifest.source_language = crate::models::SourceLanguage::EckyIrV0;
        manifest.source_digest = Some(crate::mcp::macro_buffer::source_digest(source));

        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");
        {
            let conn = state.db.lock().await;
            conn.execute(
                "UPDATE messages SET output = ?1, artifact_bundle = ?2, model_manifest = ?3 WHERE id = 'msg-1'",
                rusqlite::params![
                    serde_json::to_string(&design).expect("design json"),
                    serde_json::to_string(&bundle).expect("bundle json"),
                    serde_json::to_string(&manifest).expect("manifest json"),
                ],
            )
            .expect("update ecky target");
        }

        let guard = SemanticTransformArtifactGuard {
            model_id: model_id.to_string(),
            preview_stl_path: bundle.preview_stl_path.clone(),
            content_hash: bundle.content_hash.clone(),
        };
        (state, resolver, guard)
    }

    #[tokio::test]
    async fn semantic_transform_preview_reorient_recipe_creates_preview_draft_without_committed_message(
    ) {
        let source = "(model (part body (box 10 20 30)))";
        let (state, resolver, expected_artifact) = seed_ecky_printability_target(
            source,
            "generated-semantic-reorient",
            "semantic-reorient.stl",
        )
        .await;

        let response = handle_semantic_transform_preview(
            &state,
            &resolver,
            SemanticTransformPreviewRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: Some("generated-semantic-reorient".to_string()),
                recipe_id: "supportless-fdm-orientation-best".to_string(),
                action_kind:
                    crate::services::printability::SupportlessFdmRecipeActionKind::Reorient,
                expected_artifact,
            },
            &test_ctx(),
        )
        .await
        .expect("semantic reorient preview");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.base_message_id, "msg-1");
        assert_eq!(response.model_id, response.artifact_digest.model_id);
        let rendered_bundle =
            crate::model_runtime::read_artifact_bundle(&resolver, &response.model_id)
                .expect("rendered runtime bundle");
        assert_eq!(
            response.artifact_digest.content_hash,
            rendered_bundle.content_hash
        );
        assert_eq!(response.recipe_id, "supportless-fdm-orientation-best");
        assert_eq!(
            response.action_kind,
            crate::services::printability::SupportlessFdmRecipeActionKind::Reorient
        );
        assert_eq!(
            response.preview_support_status,
            crate::services::printability::TransformRecipeSupportStatus::Pending
        );
        assert_eq!(
            response.apply_support_status,
            crate::services::printability::TransformRecipeSupportStatus::Unsupported
        );
        assert_eq!(
            response.source_digest,
            crate::mcp::macro_buffer::source_digest(source)
        );
        assert_ne!(response.source_digest, response.new_source_digest);

        let draft = {
            let conn = state.db.lock().await;
            let committed_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE id = ?1",
                    [&response.preview_id],
                    |row| row.get(0),
                )
                .expect("message count");
            assert_eq!(committed_count, 0);
            db::get_agent_draft_for_session(&conn, &test_ctx().session_id)
                .expect("draft query")
                .expect("draft")
        };
        assert_eq!(draft.preview_id, response.preview_id);
        assert!(draft.design_output.macro_code.contains("(rotate 270 0 0"));
        assert!(draft.design_output.macro_code.contains("(part body"));
    }

    #[tokio::test]
    async fn semantic_transform_preview_stale_model_id_or_preview_stl_guard_rejects_because_digest_lacks_preview_path(
    ) {
        let (state, resolver, mut expected_artifact) = seed_ecky_printability_target(
            "(model (part body (box 10 20 30)))",
            "generated-semantic-stale",
            "semantic-stale.stl",
        )
        .await;
        expected_artifact.preview_stl_path = "/tmp/stale-preview.stl".to_string();

        let err = handle_semantic_transform_preview(
            &state,
            &resolver,
            SemanticTransformPreviewRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: Some("generated-semantic-stale".to_string()),
                recipe_id: "supportless-fdm-orientation-best".to_string(),
                action_kind:
                    crate::services::printability::SupportlessFdmRecipeActionKind::Reorient,
                expected_artifact,
            },
            &test_ctx(),
        )
        .await
        .expect_err("stale artifact guard should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("artifact guard mismatch"));
    }

    #[tokio::test]
    async fn semantic_transform_preview_missing_content_hash_guard_rejects_at_request_boundary() {
        let req = serde_json::json!({
            "threadId": "thread-1",
            "messageId": "msg-1",
            "modelId": "generated-semantic-missing-hash",
            "recipeId": "supportless-fdm-orientation-best",
            "actionKind": "reorient",
            "expectedArtifact": {
                "modelId": "generated-semantic-missing-hash",
                "previewStlPath": "/tmp/semantic-missing-hash.stl"
            }
        });

        let err = serde_json::from_value::<SemanticTransformPreviewRequest>(req)
            .expect_err("missing contentHash should fail deserialization");

        assert!(err.to_string().contains("contentHash"));
    }

    #[tokio::test]
    async fn semantic_transform_preview_stale_content_hash_guard_rejects() {
        let (state, resolver, mut expected_artifact) = seed_ecky_printability_target(
            "(model (part body (box 10 20 30)))",
            "generated-semantic-stale-hash",
            "semantic-stale-hash.stl",
        )
        .await;
        expected_artifact.content_hash = "stale-content-hash".to_string();

        let err = handle_semantic_transform_preview(
            &state,
            &resolver,
            SemanticTransformPreviewRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: Some("generated-semantic-stale-hash".to_string()),
                recipe_id: "supportless-fdm-orientation-best".to_string(),
                action_kind:
                    crate::services::printability::SupportlessFdmRecipeActionKind::Reorient,
                expected_artifact,
            },
            &test_ctx(),
        )
        .await
        .expect_err("stale contentHash should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("artifact guard mismatch"));
        assert!(err.message.contains("contentHash"));
    }

    #[tokio::test]
    async fn semantic_transform_preview_unsupported_actions_return_explicit_validation_errors() {
        let (state, resolver, expected_artifact) = seed_ecky_printability_target(
            "(model (part body (box 10 20 30)))",
            "generated-semantic-unsupported",
            "semantic-unsupported.stl",
        )
        .await;

        for action_kind in [
            crate::services::printability::SupportlessFdmRecipeActionKind::Chamfer,
            crate::services::printability::SupportlessFdmRecipeActionKind::Split,
            crate::services::printability::SupportlessFdmRecipeActionKind::Relief,
            crate::services::printability::SupportlessFdmRecipeActionKind::Clearance,
        ] {
            let err = handle_semantic_transform_preview(
                &state,
                &resolver,
                SemanticTransformPreviewRequest {
                    identity: AgentIdentityOverride::default(),
                    thread_id: Some("thread-1".to_string()),
                    message_id: Some("msg-1".to_string()),
                    model_id: Some("generated-semantic-unsupported".to_string()),
                    recipe_id: "supportless-fdm-unsupported".to_string(),
                    action_kind,
                    expected_artifact: expected_artifact.clone(),
                },
                &test_ctx(),
            )
            .await
            .expect_err("unsupported transform should fail");

            assert_eq!(err.code, AppErrorCode::Validation);
            assert!(err.message.contains("unsupported"));
            assert!(err.message.contains(match action_kind {
                crate::services::printability::SupportlessFdmRecipeActionKind::Chamfer => "chamfer",
                crate::services::printability::SupportlessFdmRecipeActionKind::Split => "split",
                crate::services::printability::SupportlessFdmRecipeActionKind::Relief => "relief",
                crate::services::printability::SupportlessFdmRecipeActionKind::Clearance =>
                    "clearance",
                crate::services::printability::SupportlessFdmRecipeActionKind::Reorient =>
                    unreachable!(),
            }));
        }
    }

    #[tokio::test]
    async fn semantic_transform_preview_non_ecky_source_is_unsupported() {
        let (state, resolver) = seed_target().await;
        let preview_stl_path = resolver.root.join("semantic-legacy.stl");
        write_binary_stl(
            &preview_stl_path,
            &[
                [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
            ],
        );
        let model_id = "generated-semantic-legacy";
        let mut bundle = sample_bundle(model_id, "semantic-legacy.stl");
        bundle.preview_stl_path = preview_stl_path.display().to_string();
        let manifest = sample_manifest(model_id);
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
            .expect("runtime bundle");

        let err = handle_semantic_transform_preview(
            &state,
            &resolver,
            SemanticTransformPreviewRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: Some(model_id.to_string()),
                recipe_id: "supportless-fdm-orientation-best".to_string(),
                action_kind:
                    crate::services::printability::SupportlessFdmRecipeActionKind::Reorient,
                expected_artifact: SemanticTransformArtifactGuard {
                    model_id: model_id.to_string(),
                    preview_stl_path: bundle.preview_stl_path.clone(),
                    content_hash: bundle.content_hash.clone(),
                },
            },
            &test_ctx(),
        )
        .await
        .expect_err("non-Ecky source should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("sourceLanguage=ecky"));
    }

    #[tokio::test]
    async fn given_durable_preview_feedback_when_latest_draft_requested_then_response_restores_feedback(
    ) {
        let (state, resolver) = seed_target().await;
        let ctx = test_ctx();
        let preview_stl_path = resolver.root.join("preview-pass.stl");
        write_closed_tetra_binary_stl(&preview_stl_path);

        let mut preview_bundle = sample_bundle("model-preview-pass", "preview-pass.stl");
        preview_bundle.preview_stl_path = preview_stl_path.display().to_string();
        let preview = store_session_render_preview(
            &state,
            &resolver,
            &ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: "thread-1".to_string(),
                base_message_id: Some("msg-1".to_string()),
                design_output: sample_design("Preview Pass", "", "preview_pass_macro()"),
                artifact_bundle: preview_bundle,
                model_manifest: sample_manifest("model-preview-pass"),
                draft_feedback: None,
            },
        )
        .await
        .expect("store preview");
        assert_eq!(
            preview
                .draft_feedback
                .as_ref()
                .expect("preview feedback")
                .status,
            crate::models::AgentDraftFeedbackStatus::Passed
        );

        clear_session_render_preview(&ctx.session_id);

        let response = handle_target_detail_get(
            &state,
            &resolver,
            TargetDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                section: TargetDetailSection::LatestDraft,
                shape_graph_filters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("target draft detail");

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["section"], "latestDraft");
        assert!(value.get("latestDraft").is_some());
        assert_eq!(value["latestDraft"]["previewId"], preview.preview_id);
        assert_eq!(value["latestDraft"]["draftFeedback"]["status"], "passed");
        assert_eq!(
            value["latestDraft"]["draftFeedback"]["source"],
            "structuralVerification"
        );
        assert!(value.get("uiSpec").is_none());
        assert!(value.get("initialParams").is_none());
        assert!(value.get("artifactBundle").is_none());
    }

    #[tokio::test]
    async fn target_detail_get_returns_latest_draft_null_when_absent() {
        let (state, resolver) = seed_target().await;
        let response = handle_target_detail_get(
            &state,
            &resolver,
            TargetDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                section: TargetDetailSection::LatestDraft,
                shape_graph_filters: None,
            },
            &test_ctx(),
        )
        .await
        .expect("target null draft detail");

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["section"], "latestDraft");
        assert!(value.get("latestDraft").is_some());
        assert!(value["latestDraft"].is_null());
    }

    #[tokio::test]
    async fn given_preview_render_when_commit_runs_then_history_gets_one_version() {
        let (state, resolver) = seed_target().await;
        let ctx = test_ctx();
        let initial_count = {
            let conn = state.db.lock().await;
            db::get_thread_messages(&conn, "thread-1").unwrap().len()
        };
        let preview_design = sample_design("Preview Pot", "", "preview_macro()");
        let preview_bundle = sample_bundle("model-preview", "preview.stl");
        let preview_manifest = sample_manifest("model-preview");

        let preview = store_session_render_preview(
            &state,
            &resolver,
            &ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: "thread-1".to_string(),
                base_message_id: Some("msg-1".to_string()),
                design_output: preview_design.clone(),
                artifact_bundle: preview_bundle.clone(),
                model_manifest: preview_manifest.clone(),
                draft_feedback: None,
            },
        )
        .await
        .expect("store preview");

        {
            let conn = state.db.lock().await;
            assert_eq!(
                db::get_thread_messages(&conn, "thread-1").unwrap().len(),
                initial_count
            );
        }
        assert_eq!(
            session_render_preview_for_request(
                &ctx,
                Some("thread-1"),
                Some(preview.preview_id.as_str())
            )
            .expect("session preview")
            .design_output
            .macro_code,
            "preview_macro()"
        );

        let response = handle_commit_preview_version(
            &state,
            &resolver,
            VersionSaveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some(preview.preview_id.clone()),
                title: Some("Committed Pot".to_string()),
                version_name: Some("V-preview".to_string()),
            },
            &ctx,
        )
        .await
        .expect("commit preview");

        {
            let conn = state.db.lock().await;
            let messages = db::get_thread_messages(&conn, "thread-1").unwrap();
            assert_eq!(messages.len(), initial_count + 1);
            let committed = messages
                .iter()
                .find(|message| message.id == response.message_id)
                .expect("committed message");
            assert_eq!(
                committed.output.as_ref().unwrap().macro_code,
                "preview_macro()"
            );
            assert_eq!(committed.output.as_ref().unwrap().version_name, "V-preview");
        }
        assert!(session_render_preview_for_request(
            &ctx,
            Some("thread-1"),
            Some(preview.preview_id.as_str())
        )
        .is_none());
    }

    #[tokio::test]
    async fn given_preview_render_when_session_memory_clears_then_commit_by_preview_id_uses_durable_draft(
    ) {
        let (state, resolver) = seed_target().await;
        let ctx = test_ctx();
        let initial_count = {
            let conn = state.db.lock().await;
            db::get_thread_messages(&conn, "thread-1").unwrap().len()
        };

        let preview = store_session_render_preview(
            &state,
            &resolver,
            &ctx,
            StoreSessionRenderPreviewRequest {
                thread_id: "thread-1".to_string(),
                base_message_id: Some("msg-1".to_string()),
                design_output: sample_design("Durable Pot", "", "durable_preview_macro()"),
                artifact_bundle: sample_bundle("model-durable-preview", "durable-preview.stl"),
                model_manifest: sample_manifest("model-durable-preview"),
                draft_feedback: Some(DraftFeedbackSeed {
                    status: crate::models::AgentDraftFeedbackStatus::Failed,
                    summary: "Draft failed structural verification.".to_string(),
                    items: vec![crate::models::AgentDraftFeedbackItem {
                        code: "non_manifold".to_string(),
                        message: "Mesh contains a non-manifold edge.".to_string(),
                    }],
                    authoring_lints: Vec::new(),
                    source: crate::models::AgentDraftFeedbackSource::StructuralVerification,
                }),
            },
        )
        .await
        .expect("store preview");
        assert_eq!(
            preview
                .draft_feedback
                .as_ref()
                .expect("draft feedback")
                .summary,
            "Draft failed structural verification."
        );

        clear_session_render_preview(&ctx.session_id);
        assert!(session_render_preview_for_request(
            &ctx,
            Some("thread-1"),
            Some(preview.preview_id.as_str())
        )
        .is_none());
        let restored = resolve_session_render_preview_for_request(
            &state,
            &ctx,
            Some("thread-1"),
            Some(preview.preview_id.as_str()),
        )
        .await
        .expect("resolve durable preview")
        .expect("durable preview restored");
        assert_eq!(
            restored
                .draft_feedback
                .as_ref()
                .expect("restored draft feedback")
                .summary,
            "Draft failed structural verification."
        );

        let response = handle_commit_preview_version(
            &state,
            &resolver,
            VersionSaveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some(preview.preview_id.clone()),
                title: None,
                version_name: Some("V-durable".to_string()),
            },
            &ctx,
        )
        .await
        .expect("commit durable preview");

        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").unwrap();
        assert_eq!(messages.len(), initial_count + 1);
        let committed = messages
            .iter()
            .find(|message| message.id == response.message_id)
            .expect("committed message");
        assert_eq!(
            committed.output.as_ref().unwrap().macro_code,
            "durable_preview_macro()"
        );
        assert_eq!(committed.output.as_ref().unwrap().version_name, "V-durable");
        assert!(db::get_agent_draft_for_session(&conn, &ctx.session_id)
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn measurement_annotation_save_persists_semantic_annotation_in_new_version() {
        let (state, resolver) = seed_target().await;
        let response = handle_measurement_annotation_save(
            &state,
            &resolver,
            MeasurementAnnotationSaveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                annotation: MeasurementAnnotation {
                    annotation_id: "measurement-outer-diameter".to_string(),
                    label: "Outer Diameter".to_string(),
                    basis: MeasurementBasis::Outer,
                    axis: MeasurementAxis::X,
                    parameter_keys: vec!["diameter".to_string()],
                    primitive_ids: vec!["diameter".to_string()],
                    target_ids: Vec::new(),
                    guide_id: None,
                    explanation: Some("Measures the outside width.".to_string()),
                    formula_hint: None,
                    source: MeasurementAnnotationSource::Manual,
                },
                title: None,
                version_name: Some("V-mcp-measurement".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("measurement annotation save");

        assert_eq!(response.version_name, "V-mcp-measurement");
        assert_eq!(response.measurement_annotation_count, 1);
        assert_eq!(response.artifact_digest.model_id, "model-base");
        let value = serde_json::to_value(&response).expect("semantic mutation json");
        assert!(value.get("artifactBundle").is_none());
        assert!(value.get("modelManifest").is_none());
        let detail = handle_semantic_manifest_detail_get(
            &state,
            &resolver,
            SemanticManifestDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some(response.thread_id.clone()),
                message_id: Some(response.message_id.clone()),
                section: SemanticManifestSection::MeasurementAnnotations,
            },
            &test_ctx(),
        )
        .await
        .expect("measurement detail");
        let annotations = detail
            .measurement_annotations
            .expect("measurement annotations");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].source, MeasurementAnnotationSource::Llm);
        assert_eq!(annotations[0].annotation_id, "measurement-outer-diameter");
    }

    #[tokio::test]
    async fn semantic_manifest_get_includes_measurement_annotations() {
        let (state, resolver) = seed_target().await;

        let created = handle_measurement_annotation_save(
            &state,
            &resolver,
            MeasurementAnnotationSaveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                annotation: MeasurementAnnotation {
                    annotation_id: "measurement-inner-width".to_string(),
                    label: "Inner Width".to_string(),
                    basis: MeasurementBasis::Inner,
                    axis: MeasurementAxis::X,
                    parameter_keys: vec!["diameter".to_string()],
                    primitive_ids: vec!["diameter".to_string()],
                    target_ids: Vec::new(),
                    guide_id: None,
                    explanation: Some("Measures the inner cavity span.".to_string()),
                    formula_hint: None,
                    source: MeasurementAnnotationSource::Manual,
                },
                title: None,
                version_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("seed measurement annotation");

        let response = handle_semantic_manifest_get(
            &state,
            &resolver,
            SemanticManifestRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some(created.thread_id.clone()),
                message_id: Some(created.message_id.clone()),
            },
            &test_ctx(),
        )
        .await
        .expect("semantic manifest with measurements");

        assert_eq!(response.measurement_annotation_count, 1);
    }

    #[tokio::test]
    async fn measurement_annotation_delete_removes_existing_annotation() {
        let (state, resolver) = seed_target().await;

        let created = handle_measurement_annotation_save(
            &state,
            &resolver,
            MeasurementAnnotationSaveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                annotation: MeasurementAnnotation {
                    annotation_id: "measurement-wall".to_string(),
                    label: "Wall Thickness".to_string(),
                    basis: MeasurementBasis::Wall,
                    axis: MeasurementAxis::Normal,
                    parameter_keys: vec!["diameter".to_string()],
                    primitive_ids: vec!["diameter".to_string()],
                    target_ids: Vec::new(),
                    guide_id: None,
                    explanation: None,
                    formula_hint: None,
                    source: MeasurementAnnotationSource::Manual,
                },
                title: None,
                version_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("seed annotation");

        let deleted = handle_measurement_annotation_delete(
            &state,
            &resolver,
            MeasurementAnnotationDeleteRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some(created.thread_id.clone()),
                message_id: Some(created.message_id.clone()),
                annotation_id: "measurement-wall".to_string(),
                title: None,
                version_name: None,
            },
            &test_ctx(),
        )
        .await
        .expect("delete annotation");

        assert_eq!(deleted.measurement_annotation_count, 0);
        let detail = handle_semantic_manifest_detail_get(
            &state,
            &resolver,
            SemanticManifestDetailRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some(deleted.thread_id.clone()),
                message_id: Some(deleted.message_id.clone()),
                section: SemanticManifestSection::MeasurementAnnotations,
            },
            &test_ctx(),
        )
        .await
        .expect("measurement detail after delete");
        assert!(detail
            .measurement_annotations
            .expect("measurement annotations")
            .is_empty());
    }

    #[tokio::test]
    async fn session_reply_save_persists_final_reply_to_thread_history_and_logs() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;

        {
            let conn = state.db.lock().await;
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "user-working-1".to_string(),
                    role: MessageRole::User,
                    content: "Please adjust the frame.".to_string(),
                    status: MessageStatus::Working,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now_secs(),
                },
            )
            .unwrap();
        }
        {
            let mut sessions = state.mcp_sessions.lock().await;
            let session = sessions.get_mut(&test_session_id()).expect("live session");
            session.current_turn_id = Some("turn-1".to_string());
            session.current_turn_thread_id = Some("thread-1".to_string());
            session.current_turn_working_message_ids = vec!["user-working-1".to_string()];
        }

        let response = handle_session_reply_save(
            &state,
            SessionReplySaveRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                body: "Saved in the current pot frame thread.".to_string(),
                fatal: false,
            },
            &test_ctx(),
        )
        .await
        .expect("session reply save");

        assert_eq!(response.thread_id, "thread-1");

        let messages = {
            let conn = state.db.lock().await;
            db::get_thread_messages(&conn, "thread-1").expect("messages")
        };
        let saved = messages
            .iter()
            .find(|message| message.id == response.message_id)
            .expect("saved reply");
        assert_eq!(saved.content, "Saved in the current pot frame thread.");
        assert_eq!(saved.role, MessageRole::Assistant);
        assert_eq!(
            saved
                .agent_origin
                .as_ref()
                .map(|origin| origin.session_id.as_str()),
            Some(test_session_id().as_str())
        );

        let working_message = messages
            .iter()
            .find(|message| message.id == "user-working-1")
            .expect("working user message");
        assert_eq!(working_message.status, MessageStatus::Success);
        let live_session = state
            .mcp_sessions
            .lock()
            .await
            .get(&test_session_id())
            .cloned()
            .expect("live session");
        assert!(live_session.current_turn_working_message_ids.is_empty());

        let logs = state.app_logs.lock().unwrap();
        let last = logs.back().expect("log entry");
        assert!(last.message.contains("kind=final_reply_save"));
        assert!(last
            .message
            .contains("Saved in the current pot frame thread."));
    }

    #[tokio::test]
    async fn long_action_notice_updates_live_session_and_logs() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;

        let response = handle_long_action_notice(
            &state,
            LongActionNoticeRequest {
                identity: AgentIdentityOverride::default(),
                message: "Developing the next iteration".to_string(),
                phase: Some("working".to_string()),
                details: Some("connector placement pass".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("long action notice");

        assert_eq!(response.phase, "working");
        assert!(response.busy);
        assert_eq!(response.activity_label, "Developing the next iteration");

        let live_session = state
            .mcp_sessions
            .lock()
            .await
            .get(&test_session_id())
            .cloned()
            .expect("live session");
        assert!(live_session.busy);
        assert_eq!(
            live_session.activity_label.as_deref(),
            Some("Developing the next iteration")
        );
        assert_eq!(live_session.phase.as_deref(), Some("working"));

        let logs = state.app_logs.lock().unwrap();
        let last = logs.back().expect("log entry");
        assert!(last.message.contains("kind=session_activity_set"));
        assert!(last.message.contains("connector placement pass"));
    }

    #[tokio::test]
    async fn long_action_clear_resets_live_session_busy_state() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;
        handle_long_action_notice(
            &state,
            LongActionNoticeRequest {
                identity: AgentIdentityOverride::default(),
                message: "Developing the next iteration".to_string(),
                phase: Some("working".to_string()),
                details: None,
            },
            &test_ctx(),
        )
        .await
        .expect("seed long action");

        let response = handle_long_action_clear(
            &state,
            LongActionClearRequest {
                identity: AgentIdentityOverride::default(),
                phase: Some("idle".to_string()),
                status_text: Some("Ready for the next queued message.".to_string()),
            },
            &test_ctx(),
        )
        .await
        .expect("long action clear");

        assert_eq!(response.phase, "idle");
        assert!(!response.busy);

        let live_session = state
            .mcp_sessions
            .lock()
            .await
            .get(&test_session_id())
            .cloned()
            .expect("live session");
        assert!(!live_session.busy);
        assert_eq!(live_session.activity_label, None);
        assert_eq!(live_session.phase.as_deref(), Some("idle"));
        assert_eq!(
            live_session.status_text.as_deref(),
            Some("Ready for the next queued message.")
        );
    }

    #[tokio::test]
    async fn mark_as_read_drains_pending_thread_batch_and_sets_session_working() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;
        let now = now_secs();

        {
            let conn = state.db.lock().await;
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "user-pending-1".to_string(),
                    role: MessageRole::User,
                    content: "Please thin the lip.".to_string(),
                    status: MessageStatus::Pending,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "user-pending-2".to_string(),
                    role: MessageRole::User,
                    content: "Also widen the top opening.".to_string(),
                    status: MessageStatus::Pending,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
            persist_agent_session(
                &conn,
                &test_ctx(),
                Some("thread-1".to_string()),
                Some("msg-1".to_string()),
                Some("model-base".to_string()),
                "idle",
                "Agent joined the workspace.",
            )
            .unwrap();
        }

        let response = handle_mark_as_read(
            &state,
            MarkAsReadRequest {
                message_id: "user-pending-1".to_string(),
                thread_id: Some("thread-1".to_string()),
                identity: AgentIdentityOverride::default(),
            },
            &test_ctx(),
        )
        .await
        .expect("mark_as_read");

        assert_eq!(response.thread_id, "thread-1");
        assert_eq!(response.message_id, "user-pending-1");
        assert_eq!(
            response.message_ids,
            vec!["user-pending-1".to_string(), "user-pending-2".to_string()]
        );
        assert_eq!(response.status, "working");

        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
        let statuses = messages
            .into_iter()
            .filter(|message| message.role == MessageRole::User)
            .map(|message| (message.id, message.status))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            statuses.get("user-pending-1"),
            Some(&MessageStatus::Working)
        );
        assert_eq!(
            statuses.get("user-pending-2"),
            Some(&MessageStatus::Working)
        );
        let sessions = db::get_sessions_by_ids(&conn, &[test_session_id()]).expect("sessions");
        assert_eq!(sessions[0].phase, "working");
        assert_eq!(sessions[0].message_id.as_deref(), Some("user-pending-1"));
    }

    #[tokio::test]
    async fn session_log_out_removes_live_session_and_hides_it_from_active_sessions() {
        let (state, _resolver) = seed_target().await;
        seed_live_session(&state).await;

        {
            let conn = state.db.lock().await;
            persist_agent_session(
                &conn,
                &test_ctx(),
                Some("thread-1".to_string()),
                Some("msg-1".to_string()),
                Some("model-base".to_string()),
                "idle",
                "Agent joined the workspace.",
            )
            .unwrap();
        }

        handle_session_log_out(
            &state,
            SessionLogoutRequest {
                identity: AgentIdentityOverride::default(),
            },
            &test_ctx(),
        )
        .await
        .expect("session_log_out");

        assert!(state
            .mcp_sessions
            .lock()
            .await
            .get(&test_session_id())
            .is_none());

        let conn = state.db.lock().await;
        let stored = db::get_sessions_by_ids(&conn, &[test_session_id()]).expect("stored");
        assert_eq!(stored[0].phase, "disconnected");
        let active = db::get_active_agent_sessions(&conn, 600).expect("active sessions");
        assert!(active
            .into_iter()
            .all(|session| session.session_id != test_session_id()));
    }

    #[test]
    fn macro_buffer_replaces_line_range_with_digest_guard() {
        let source = "(model\n  (part body (box 1 1 1))\n)\n";
        let digest = macro_buffer_digest(source);
        let patched = apply_macro_buffer_replacements(
            source,
            &digest,
            &[MacroBufferReplacement {
                start_line: 2,
                end_line: 2,
                new_text: "  (part body (box 2 2 2))".to_string(),
            }],
        )
        .expect("patched macro");

        assert_eq!(patched, "(model\n  (part body (box 2 2 2))\n)\n");
    }

    #[test]
    fn macro_buffer_edit_response_omits_full_macro_code() {
        let response = MacroBufferEditResponse {
            digest: "digest".to_string(),
            line_count: 2,
            window_start_line: 1,
            window_end_line: 2,
            truncated: false,
            lines: vec![
                MacroBufferLine {
                    line_number: 1,
                    text: "(model".to_string(),
                },
                MacroBufferLine {
                    line_number: 2,
                    text: ")".to_string(),
                },
            ],
        };

        let value = serde_json::to_value(response).expect("edit response json");
        assert!(value.get("macroCode").is_none());
        assert_eq!(value["windowStartLine"], 1);
        assert_eq!(value["windowEndLine"], 2);
        assert_eq!(value["truncated"], false);
        assert_eq!(value["lines"].as_array().expect("lines").len(), 2);
    }

    #[test]
    fn macro_buffer_rejects_stale_digest() {
        let err = apply_macro_buffer_replacements(
            "(model)\n",
            "stale",
            &[MacroBufferReplacement {
                start_line: 1,
                end_line: 1,
                new_text: "(model\n)".to_string(),
            }],
        )
        .expect_err("stale digest should fail");

        assert!(err.message.contains("Macro buffer digest mismatch"));
    }
}

// --- Component library tools (component-unification T5) ---

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentExtractToolRequest {
    /// Full `.ecky` model source containing the part to lift.
    pub source: String,
    pub part_key: String,
    /// Component name; defaults to the part key.
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    /// Save the extracted component into the component library.
    #[serde(default)]
    pub save: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentExtractToolResponse {
    pub name: String,
    /// Copy-inline `define-component` source, pasteable into any model.
    pub component_source: String,
    pub header: crate::component_extract::ComponentHeader,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSearchToolRequest {
    pub query: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSearchToolResponse {
    pub results: Vec<crate::component_package_runtime::ExtractedComponentSearchResult>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentGetToolRequest {
    pub name: String,
}

pub fn handle_component_extract(
    app: &dyn PathResolver,
    req: ComponentExtractToolRequest,
) -> AppResult<ComponentExtractToolResponse> {
    let extracted = crate::component_extract::extract_component(
        &crate::component_extract::ComponentExtractRequest {
            source: req.source,
            part_key: req.part_key,
            component_name: req.name,
            description: req.description,
            tags: req.tags,
            thread_id: req.thread_id,
            message_id: req.message_id,
        },
    )?;
    let saved_path = if req.save {
        let dir = crate::component_package_runtime::save_extracted_component(app, &extracted)?;
        Some(dir.to_string_lossy().to_string())
    } else {
        None
    };
    Ok(ComponentExtractToolResponse {
        name: extracted.name.clone(),
        component_source: extracted.component_source.clone(),
        header: extracted.header.clone(),
        saved_path,
    })
}

pub fn handle_component_search(
    app: &dyn PathResolver,
    req: ComponentSearchToolRequest,
) -> AppResult<ComponentSearchToolResponse> {
    let limit = req.limit.unwrap_or(20).clamp(1, 100);
    let results = crate::component_package_runtime::search_extracted_components(
        app,
        req.query.as_deref().unwrap_or(""),
        limit,
    )?;
    Ok(ComponentSearchToolResponse { results })
}

pub fn handle_component_get(
    app: &dyn PathResolver,
    req: ComponentGetToolRequest,
) -> AppResult<crate::component_package_runtime::ExtractedComponentRecord> {
    crate::component_package_runtime::read_extracted_component(app, &req.name)
}
