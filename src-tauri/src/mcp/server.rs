use crate::db;
use crate::mcp::contracts::*;
use crate::mcp::handlers;
use crate::models::{
    AppError, AppErrorCode, AppResult, AppState, McpSessionState, McpTargetRef, PathResolver,
    TargetLeaseInfo,
};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::net::TcpListener;
use uuid::Uuid;

const SESSION_HEADER: &str = "Mcp-Session-Id";
const LEASE_TTL_SECS: u64 = 45;
const MCP_PROTOCOL_LATEST: &str = "2025-06-18";
const MCP_PROTOCOL_LEGACY: &str = "2024-11-05";

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    pub arguments: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ReadResourceParams {
    uri: String,
}

#[derive(Debug, Deserialize)]
struct GetPromptParams {
    name: String,
    #[serde(default)]
    arguments: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    #[serde(default)]
    protocol_version: Option<String>,
    #[serde(default)]
    client_info: Option<ClientInfo>,
}

#[derive(Debug, Default, Deserialize)]
struct ClientInfo {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedTargetRef {
    thread_id: String,
    message_id: String,
    model_id: Option<String>,
    title: String,
    version_name: String,
    has_draft: bool,
    ui_field_count: usize,
    range_count: usize,
    number_count: usize,
    select_count: usize,
    checkbox_count: usize,
    parameter_count: usize,
    has_semantic_manifest: bool,
    control_primitive_count: usize,
    control_relation_count: usize,
    control_view_count: usize,
}

#[derive(Clone)]
struct HttpServerState {
    state: AppState,
    app: Arc<dyn PathResolver + Send + Sync>,
    handle: tauri::AppHandle,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn json_rpc_result(id: Option<Value>, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(result),
        error: None,
        id,
    }
}

fn json_rpc_error(id: Option<Value>, code: i32, message: impl Into<String>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.into(),
            data: None,
        }),
        id,
    }
}

fn mcp_tool_success(id: Option<Value>, value: &Value) -> JsonRpcResponse {
    json_rpc_result(
        id,
        json!({
            "content": [
                {
                    "type": "text",
                    "text": serde_json::to_string_pretty(value).unwrap()
                }
            ]
        }),
    )
}

fn mcp_tool_error(id: Option<Value>, error: &AppError) -> JsonRpcResponse {
    json_rpc_result(
        id,
        json!({
            "content": [
                {
                    "type": "text",
                    "text": serde_json::to_string_pretty(error).unwrap_or_else(|_| error.to_string())
                }
            ],
            "isError": true
        }),
    )
}

fn json_http_response(
    status: StatusCode,
    payload: &JsonRpcResponse,
    session_id: Option<&str>,
) -> Response {
    let mut response = (status, Json(payload)).into_response();
    if let Some(session_id) = session_id {
        if let Ok(header) = HeaderValue::from_str(session_id) {
            response.headers_mut().insert(SESSION_HEADER, header);
        }
    }
    response
}

fn plain_text_response(status: StatusCode, message: &str) -> Response {
    (status, message.to_string()).into_response()
}

fn empty_response(status: StatusCode) -> Response {
    status.into_response()
}

fn negotiated_protocol_version(requested: Option<&str>) -> &'static str {
    match requested.map(str::trim) {
        Some(MCP_PROTOCOL_LATEST) => MCP_PROTOCOL_LATEST,
        Some(MCP_PROTOCOL_LEGACY) => MCP_PROTOCOL_LEGACY,
        _ => MCP_PROTOCOL_LATEST,
    }
}

fn allowed_origin(origin: &str) -> bool {
    let normalized = origin.trim().to_ascii_lowercase();
    if normalized == "tauri://localhost" {
        return true;
    }
    for prefix in ["http://127.0.0.1:", "http://localhost:"] {
        if let Some(port) = normalized.strip_prefix(prefix) {
            return !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit());
        }
    }
    false
}

fn validate_origin(headers: &HeaderMap) -> Option<Response> {
    let origin = headers.get("origin")?;
    let origin = match origin.to_str() {
        Ok(value) => value,
        Err(_) => {
            return Some(plain_text_response(
                StatusCode::FORBIDDEN,
                "Origin not allowed.",
            ))
        }
    };
    if allowed_origin(origin) {
        None
    } else {
        Some(plain_text_response(
            StatusCode::FORBIDDEN,
            "Origin not allowed.",
        ))
    }
}

fn session_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn create_session(state: &AppState, host_label: String) -> String {
    let session_id = format!("mcp-http-{}", Uuid::new_v4());
    let mut sessions = state.mcp_sessions.lock().await;
    sessions.insert(
        session_id.clone(),
        McpSessionState::new("mcp-http".to_string(), host_label),
    );
    session_id
}

async fn get_session(state: &AppState, session_id: &str) -> Option<McpSessionState> {
    state.mcp_sessions.lock().await.get(session_id).cloned()
}

async fn update_session_state<F>(state: &AppState, session_id: &str, f: F) -> AppResult<()>
where
    F: FnOnce(&mut McpSessionState),
{
    let mut sessions = state.mcp_sessions.lock().await;
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| AppError::not_found("MCP session not found."))?;
    f(session);
    Ok(())
}

async fn set_session_target(state: &AppState, session_id: &str, target: Option<McpTargetRef>) {
    let mut sessions = state.mcp_sessions.lock().await;
    if let Some(session) = sessions.get_mut(session_id) {
        session.last_target = target;
    }
}

async fn remove_session(state: &AppState, session_id: &str) -> AppResult<()> {
    state.mcp_sessions.lock().await.remove(session_id);
    let conn = state.db.lock().await;
    db::delete_target_leases_for_session(&conn, session_id)
        .map_err(|e| AppError::persistence(e.to_string()))?;
    db::delete_agent_session(&conn, session_id)
        .map_err(|e| AppError::persistence(e.to_string()))?;
    Ok(())
}

/// Emit the current live session list to the frontend so it can update without polling.
/// Uses the in-memory mcp_sessions map as authoritative source of live connections,
/// then fetches full DB records for those IDs.
async fn emit_sessions_changed(state: &AppState, handle: &tauri::AppHandle) {
    use tauri::Emitter;
    let live_ids: Vec<String> = state.mcp_sessions.lock().await.keys().cloned().collect();
    let conn = state.db.lock().await;
    let sessions = db::get_sessions_by_ids(&conn, &live_ids).unwrap_or_default();
    drop(conn);
    let _ = handle.emit("agent-sessions-changed", sessions);
}

fn current_context(session_id: &str, session: &McpSessionState) -> handlers::AgentContext {
    handlers::AgentContext {
        session_id: session_id.to_string(),
        client_kind: session.client_kind.clone(),
        host_label: session.host_label.clone(),
        agent_label: session.agent_label.clone(),
        llm_model_id: session.llm_model_id.clone(),
        llm_model_label: session.llm_model_label.clone(),
    }
}

async fn resolve_target_for_session(
    state: &AppState,
    app: &dyn PathResolver,
    session_id: &str,
    explicit_thread_id: Option<String>,
    explicit_message_id: Option<String>,
) -> AppResult<ResolvedTargetRef> {
    let cached_target = {
        state
            .mcp_sessions
            .lock()
            .await
            .get(session_id)
            .and_then(|session| session.last_target.clone())
    };

    let conn = state.db.lock().await;

    let target = if let Some(message_id) = explicit_message_id {
        crate::services::target::resolve_target(&conn, app, explicit_thread_id, Some(message_id))?
    } else if let Some(thread_id) = explicit_thread_id {
        let message_id = db::get_latest_successful_message_id_in_thread(&conn, &thread_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| {
                AppError::validation(format!("Thread {} has no successful versions.", thread_id))
            })?;
        crate::services::target::resolve_target(&conn, app, Some(thread_id), Some(message_id))?
    } else if let Some(cached_target) = cached_target {
        let still_exists = db::get_message_thread_id(&conn, &cached_target.message_id)
            .map_err(|e| AppError::persistence(e.to_string()))?;
        if still_exists.as_deref() == Some(cached_target.thread_id.as_str()) {
            crate::services::target::resolve_target(
                &conn,
                app,
                Some(cached_target.thread_id),
                Some(cached_target.message_id),
            )?
        } else {
            let recent = db::get_latest_successful_target_in_most_recent_thread(&conn)
                .map_err(|e| AppError::persistence(e.to_string()))?
                .ok_or_else(|| AppError::validation("No active target available."))?;
            crate::services::target::resolve_target(
                &conn,
                app,
                Some(recent.thread_id),
                Some(recent.message_id),
            )?
        }
    } else {
        let recent = db::get_latest_successful_target_in_most_recent_thread(&conn)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| AppError::validation("No active target available."))?;
        crate::services::target::resolve_target(
            &conn,
            app,
            Some(recent.thread_id),
            Some(recent.message_id),
        )?
    };

    let design = target
        .latest_draft
        .as_ref()
        .map(|draft| draft.design_output.clone())
        .or(target.design.clone())
        .ok_or_else(|| AppError::validation("Target has no design output."))?;
    let (range_count, number_count, select_count, checkbox_count) = design
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
    let model_id = target
        .latest_draft
        .as_ref()
        .and_then(|draft| draft.model_id.clone())
        .or_else(|| {
            target
                .artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.clone())
        });

    Ok(ResolvedTargetRef {
        thread_id: target.thread_id,
        message_id: target.message_id,
        model_id,
        title: design.title,
        version_name: design.version_name,
        has_draft: target.latest_draft.is_some(),
        ui_field_count: design.ui_spec.fields.len(),
        range_count,
        number_count,
        select_count,
        checkbox_count,
        parameter_count: design.initial_params.len(),
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
    })
}

async fn acquire_lease(
    state: &AppState,
    ctx: &handlers::AgentContext,
    target: &McpTargetRef,
) -> AppResult<()> {
    let conn = state.db.lock().await;
    if let Some(active) = db::get_active_target_lease(
        &conn,
        &target.thread_id,
        &target.message_id,
        target.model_id.as_deref(),
    )
    .map_err(|e| AppError::persistence(e.to_string()))?
    {
        if active.session_id != ctx.session_id {
            let details = serde_json::to_string_pretty(&active).unwrap_or_default();
            return Err(AppError::with_details(
                AppErrorCode::Conflict,
                "Target is currently leased by another agent.",
                details,
            ));
        }
    }

    let now = now_secs();
    db::upsert_target_lease(
        &conn,
        &TargetLeaseInfo {
            session_id: ctx.session_id.clone(),
            thread_id: target.thread_id.clone(),
            message_id: target.message_id.clone(),
            model_id: target.model_id.clone(),
            host_label: ctx.host_label.clone(),
            agent_label: ctx.agent_label.clone(),
            acquired_at: now,
            expires_at: now + LEASE_TTL_SECS,
        },
    )
    .map_err(|e| AppError::persistence(e.to_string()))
}

async fn move_or_refresh_lease(
    state: &AppState,
    ctx: &handlers::AgentContext,
    previous: &McpTargetRef,
    next: &McpTargetRef,
) -> AppResult<()> {
    let conn = state.db.lock().await;
    db::delete_target_lease(
        &conn,
        &ctx.session_id,
        &previous.thread_id,
        &previous.message_id,
        previous.model_id.as_deref(),
    )
    .map_err(|e| AppError::persistence(e.to_string()))?;

    let now = now_secs();
    db::upsert_target_lease(
        &conn,
        &TargetLeaseInfo {
            session_id: ctx.session_id.clone(),
            thread_id: next.thread_id.clone(),
            message_id: next.message_id.clone(),
            model_id: next.model_id.clone(),
            host_label: ctx.host_label.clone(),
            agent_label: ctx.agent_label.clone(),
            acquired_at: now,
            expires_at: now + LEASE_TTL_SECS,
        },
    )
    .map_err(|e| AppError::persistence(e.to_string()))
}

async fn release_lease(state: &AppState, session_id: &str, target: &McpTargetRef) -> AppResult<()> {
    let conn = state.db.lock().await;
    db::delete_target_lease(
        &conn,
        session_id,
        &target.thread_id,
        &target.message_id,
        target.model_id.as_deref(),
    )
    .map_err(|e| AppError::persistence(e.to_string()))
}

fn target_ref_from_value(value: &Value) -> Option<McpTargetRef> {
    let thread_id = value.get("threadId")?.as_str()?.to_string();
    let message_id = value.get("messageId")?.as_str()?.to_string();
    let model_id = value
        .get("modelId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get("artifactBundle")
                .and_then(|bundle| bundle.get("modelId"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    Some(McpTargetRef {
        thread_id,
        message_id,
        model_id,
    })
}

fn thread_list_entry(thread: crate::models::Thread) -> ThreadListEntry {
    ThreadListEntry {
        thread_id: thread.id,
        title: thread.title,
        updated_at: thread.updated_at,
        version_count: thread.version_count,
        pending_count: thread.pending_count,
        error_count: thread.error_count,
        status: thread.status,
        finalized_at: thread.finalized_at,
    }
}

fn identity_props() -> Value {
    json!({
        "agentLabel": { "type": "string" },
        "llmModelId": { "type": "string" },
        "llmModelLabel": { "type": "string" }
    })
}

fn with_identity(extra: &[(&str, Value)], required: &[&str]) -> Value {
    let mut properties = identity_props()
        .as_object()
        .cloned()
        .unwrap_or_else(serde_json::Map::new);
    for (key, value) in extra {
        properties.insert((*key).to_string(), value.clone());
    }
    let mut schema = json!({
        "type": "object",
        "properties": properties
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn selected_engine_prompt(state: &AppState) -> String {
    let config = state.config.lock().unwrap();
    let engine = config
        .engines
        .iter()
        .find(|engine| engine.id == config.selected_engine_id)
        .or_else(|| config.engines.first());
    engine
        .map(|engine| engine.system_prompt.trim())
        .filter(|prompt| !prompt.is_empty())
        .unwrap_or(crate::DEFAULT_PROMPT)
        .to_string()
}

fn selected_engine_label(state: &AppState) -> String {
    let config = state.config.lock().unwrap();
    let engine = config
        .engines
        .iter()
        .find(|engine| engine.id == config.selected_engine_id)
        .or_else(|| config.engines.first());
    match engine {
        Some(engine) => {
            let provider = engine.provider.trim();
            let model = engine.model.trim();
            if provider.is_empty() && model.is_empty() {
                engine.name.clone()
            } else if provider.is_empty() {
                model.to_string()
            } else if model.is_empty() {
                provider.to_string()
            } else {
                format!("{} / {}", provider, model)
            }
        }
        None => "default engine".to_string(),
    }
}

fn workflow_guide_text(state: &AppState) -> String {
    format!(
        concat!(
            "Ecky MCP guide\n\n",
            "Purpose:\n",
            "- Ecky edits CAD models, especially generated FreeCAD macro-based models.\n",
            "- Use the current selected engine prompt as the design-policy baseline.\n\n",
            "Current engine:\n",
            "- {}\n\n",
            "Modeling rules:\n",
            "- Units are millimeters.\n",
            "- Prefer manifold printable solids with practical wall thickness and clearances.\n",
            "- For generated models, keep macroCode, uiSpec, and initialParams aligned.\n",
            "- Remove stale parameters that are no longer used.\n",
            "- Preserve the current thread/version intent unless explicitly asked to fork or restore.\n",
            "- For semantic controls, use knobs/views that reflect the actual model structure.\n\n",
            "Recommended startup sequence:\n",
            "1. Read ecky://guides/system-prompt and ecky://guides/modeling-guidelines.\n",
            "2. Call workspace_overview.\n",
            "3. If needed, call target_get or thread_get.\n",
            "4. Then mutate with params_patch_and_render, macro_replace_and_render, or semantic tools.\n"
        ),
        selected_engine_label(state)
    )
}

fn workspace_overview_brief(state: &AppState) -> WorkspaceOverviewBrief {
    WorkspaceOverviewBrief {
        engine_label: selected_engine_label(state),
        summary: "Use millimeters, keep macro/uiSpec/initialParams aligned, and prefer printable manifold solids. Read the canonical Ecky resources before making broad edits.".to_string(),
        rules: vec![
            "Units are millimeters.".to_string(),
            "Keep macroCode, uiSpec, and initialParams aligned.".to_string(),
            "Remove stale parameters that are no longer used.".to_string(),
            "Prefer printable manifold solids with practical clearances.".to_string(),
            "Preserve current thread/version intent unless explicitly asked to fork or restore."
                .to_string(),
        ],
        resources: vec![
            "ecky://guides/system-prompt".to_string(),
            "ecky://guides/technical-system-prompt".to_string(),
            "ecky://guides/modeling-guidelines".to_string(),
        ],
        next_steps: vec![
            "Read ecky://guides/system-prompt if you have not loaded Ecky guidance yet."
                .to_string(),
            "Call target_get for full editable target details.".to_string(),
            "Then use params_patch_and_render, macro_replace_and_render, or semantic tools."
                .to_string(),
        ],
    }
}

fn workspace_control_surface(target: &ResolvedTargetRef) -> WorkspaceControlSurface {
    let mut hints = vec![];
    if target.ui_field_count > 0 {
        hints.push(format!(
            "This target exposes {} uiSpec fields. Use target_get to inspect exact control keys, defaults, and option values.",
            target.ui_field_count
        ));
    } else {
        hints.push(
            "This target does not currently expose uiSpec fields. Macro edits may be needed before parameter-only changes are possible."
                .to_string(),
        );
    }
    if target.select_count > 0 || target.checkbox_count > 0 {
        hints.push(format!(
            "There are {} select controls and {} checkbox toggles. These are the most likely feature switches inside the macro.",
            target.select_count, target.checkbox_count
        ));
    }
    if target.has_semantic_manifest {
        hints.push(format!(
            "Semantic manifest is present with {} control primitives, {} relations, and {} views. Use semantic_manifest_get to inspect bindings and control_view/control_primitive tools to edit them.",
            target.control_primitive_count, target.control_relation_count, target.control_view_count
        ));
    } else {
        hints.push(
            "No semantic manifest is attached to this target yet. Control relationships may exist only implicitly in macroCode/uiSpec."
                .to_string(),
        );
    }

    WorkspaceControlSurface {
        ui_field_count: target.ui_field_count,
        range_count: target.range_count,
        number_count: target.number_count,
        select_count: target.select_count,
        checkbox_count: target.checkbox_count,
        parameter_count: target.parameter_count,
        has_semantic_manifest: target.has_semantic_manifest,
        control_primitive_count: target.control_primitive_count,
        control_relation_count: target.control_relation_count,
        control_view_count: target.control_view_count,
        hints,
    }
}

fn resource_definitions(state: &AppState) -> Vec<Value> {
    vec![
        json!({
            "uri": "ecky://guides/system-prompt",
            "name": "Selected Engine System Prompt",
            "description": format!("The active Ecky system prompt for {}.", selected_engine_label(state)),
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/technical-system-prompt",
            "name": "Technical System Prompt",
            "description": "The stricter CAD-technical response contract used by Ecky flows.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/modeling-guidelines",
            "name": "Modeling Guidelines",
            "description": "Core modeling, printability, and workflow guidance for external agents using Ecky MCP.",
            "mimeType": "text/plain"
        }),
    ]
}

fn read_resource_text(state: &AppState, uri: &str) -> Option<String> {
    match uri {
        "ecky://guides/system-prompt" => Some(selected_engine_prompt(state)),
        "ecky://guides/technical-system-prompt" => Some(crate::TECHNICAL_SYSTEM_PROMPT.to_string()),
        "ecky://guides/modeling-guidelines" => Some(workflow_guide_text(state)),
        _ => None,
    }
}

fn prompt_definitions() -> Vec<Value> {
    vec![json!({
        "name": "bootstrap_ecky",
        "description": "Read Ecky's modeling/system guidance and establish the default target before editing.",
        "arguments": []
    })]
}

fn prompt_payload(state: &AppState, name: &str) -> Option<Value> {
    match name {
        "bootstrap_ecky" => {
            let system_prompt = selected_engine_prompt(state);
            let workflow = workflow_guide_text(state);
            Some(json!({
                "description": "Bootstrap prompt for external agents connecting to Ecky MCP.",
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "{}\n\nSelected system prompt:\n\n{}\n\nAfter reading this, call `workspace_overview` before editing anything.",
                                workflow,
                                system_prompt
                            )
                        }
                    }
                ]
            }))
        }
        _ => None,
    }
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "health_check",
            "description": "Confirm server is alive and can reach storage/runtime.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "workspace_overview",
            "description": "Fast entrypoint: resolve the default editable target, list recent threads, and report any conflicting lease.",
            "inputSchema": with_identity(&[], &[])
        }),
        json!({
            "name": "session_log_in",
            "description": "Notify the workspace that an agent has joined.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agentLabel": { "type": "string" },
                    "threadId": { "type": "string" },
                    "messageId": { "type": "string" }
                },
                "required": ["agentLabel"]
            }
        }),
        json!({
            "name": "session_log_out",
            "description": "Notify the workspace that an agent is leaving.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agentLabel": { "type": "string" }
                },
                "required": ["agentLabel"]
            }
        }),
        json!({
            "name": "resume_session",
            "description": "Resume a previous agent session by retrieving the last known context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agentLabel": { "type": "string" }
                },
                "required": ["agentLabel"]
            }
        }),
        json!({
            "name": "thread_list",
            "description": "Lightweight browsing of available work targets.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "thread_get",
            "description": "Fetch a full thread with versions and runtime metadata.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "threadId": { "type": "string" }
                },
                "required": ["threadId"]
            }
        }),
        json!({
            "name": "agent_identity_set",
            "description": "Set sticky agent/model labels for this MCP session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agentLabel": { "type": "string" },
                    "llmModelId": { "type": "string" },
                    "llmModelLabel": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "target_get",
            "description": "Fetch the current resolved editable target. If no target is provided, the server uses the session cache or the most recent successful thread.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "params_patch_and_render",
            "description": "Patch a subset of parameters and rerender a draft. Works without prior browsing by resolving the default target automatically.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("parameterPatch", json!({ "type": "object" }))
                ],
                &["parameterPatch"],
            )
        }),
        json!({
            "name": "macro_replace_and_render",
            "description": concat!(
                "Replace macro code and rerender a draft. ",
                "uiSpec.fields is an array of control descriptors — each field MUST have: key (string), label (string), type (one of: range|number|select|checkbox|image). ",
                "range/number: min, max, step (numbers). ",
                "select: options array of {label, value} objects — MUST have at least one option. ",
                "checkbox: no extra fields. ",
                "image: use for file-picker inputs (e.g. a reference photo) — no extra fields, value is an absolute file path string. ",
                "parameters is a flat key→value map matching uiSpec field keys. ",
                "For image fields, omit the key from parameters (user picks the file in the UI)."
            ),
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("macroCode", json!({ "type": "string" })),
                    ("uiSpec", json!({
                        "type": "object",
                        "properties": {
                            "fields": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["key", "label", "type"],
                                    "properties": {
                                        "key": { "type": "string" },
                                        "label": { "type": "string" },
                                        "type": { "type": "string", "enum": ["range", "number", "select", "checkbox", "image"] },
                                        "min": { "type": "number" },
                                        "max": { "type": "number" },
                                        "step": { "type": "number" },
                                        "options": { "type": "array", "items": { "type": "object", "properties": { "label": { "type": "string" }, "value": {} } } }
                                    }
                                }
                            }
                        }
                    })),
                    ("parameters", json!({ "type": "object" }))
                ],
                &["macroCode"],
            )
        }),
        json!({
            "name": "semantic_manifest_get",
            "description": "Fetch the semantic manifest for the current generated-model target.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "control_primitive_save",
            "description": "Create or update one semantic knob and save a new version.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("primitive", json!({ "type": "object" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &["primitive"],
            )
        }),
        json!({
            "name": "control_primitive_delete",
            "description": "Delete one semantic knob and save a new version.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("primitiveId", json!({ "type": "string" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &["primitiveId"],
            )
        }),
        json!({
            "name": "control_view_save",
            "description": "Create or update one semantic view and save a new version.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("view", json!({ "type": "object" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &["view"],
            )
        }),
        json!({
            "name": "control_view_delete",
            "description": "Delete one semantic view and save a new version.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("viewId", json!({ "type": "string" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &["viewId"],
            )
        }),
        json!({
            "name": "version_save",
            "description": "Persist the latest successful draft as a new saved version.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "thread_fork_from_target",
            "description": "Save the latest draft or saved target into a new thread.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "version_restore",
            "description": "Restore an existing saved version.",
            "inputSchema": with_identity(
                &[("messageId", json!({ "type": "string" }))],
                &["messageId"],
            )
        }),
        json!({
            "name": "user_confirm_request",
            "description": "Show a confirmation dialog with clickable buttons in the Ecky UI. Use this instead of asking in the chat terminal. Blocks until the user responds or the timeout expires.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "The question or statement to show the user." },
                    "buttons": { "type": "array", "items": { "type": "string" }, "description": "Button labels. Defaults to [\"Yes\", \"No\"]." },
                    "requestId": { "type": "string", "description": "Optional stable ID for deduplication." },
                    "timeoutSecs": { "type": "number", "description": "Seconds to wait before timing out. Default 120, max 600." }
                },
                "required": ["message"]
            }
        }),
        json!({
            "name": "request_user_prompt",
            "description": "Request free-text input from the human in the Ecky UI. Blocks until the user types and submits a prompt, or the timeout expires. Use this to drive the conversation loop: ask the user what they want, wait for their reply, then act on it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Optional context message shown to the user above the input (e.g. 'What would you like me to build?')." },
                    "requestId": { "type": "string", "description": "Optional stable ID for deduplication." },
                    "timeoutSecs": { "type": "number", "description": "Seconds to wait. Default 300, max 1800." }
                }
            }
        }),
        json!({
            "name": "finalize_thread",
            "description": "Mark a design session as finalized (complete). Moves the thread to inventory. The thread can be re-opened later with reopen.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "threadId": { "type": "string", "description": "The thread to finalize." }
                },
                "required": ["threadId"]
            }
        }),
    ]
}

/// Default port range tried (in random order) when no port is configured.
const MCP_PORT_RANGE_START: u16 = 39249;
const MCP_PORT_RANGE_END: u16 = 39258; // 10 candidates

/// Try to bind `preferred` if given, otherwise try 39249 first (stable default for agent
/// configs), then fall back to 39250–39258 in random order so a second Ecky instance still
/// gets a port rather than a hard crash.
/// Returns `(actual_port, listener)` on success, or an error string on failure.
async fn pick_listener(preferred: Option<u16>) -> Result<(u16, TcpListener), String> {
    use rand::seq::SliceRandom;

    if let Some(p) = preferred {
        return TcpListener::bind(format!("127.0.0.1:{}", p))
            .await
            .map(|l| (p, l))
            .map_err(|e| format!("Port {} is unavailable: {}", p, e));
    }

    // Try the stable default first so agent configs stay valid across restarts.
    if let Ok(l) = TcpListener::bind(format!("127.0.0.1:{}", MCP_PORT_RANGE_START)).await {
        return Ok((MCP_PORT_RANGE_START, l));
    }

    // Default port is taken — likely another Ecky is running. Try the rest randomly so the
    // second instance still gets a usable (though non-default) port.
    let mut fallback: Vec<u16> = (MCP_PORT_RANGE_START + 1..=MCP_PORT_RANGE_END).collect();
    fallback.shuffle(&mut rand::thread_rng());

    for p in &fallback {
        if let Ok(l) = TcpListener::bind(format!("127.0.0.1:{}", p)).await {
            eprintln!(
                "[MCP] Default port {} busy — is another Ecky running? Using {} instead.",
                MCP_PORT_RANGE_START, p
            );
            return Ok((*p, l));
        }
    }

    Err(format!(
        "All ports {}-{} are in use — is another Ecky instance already running?",
        MCP_PORT_RANGE_START, MCP_PORT_RANGE_END
    ))
}

/// `serve_http_on_port` is called from lib.rs and respects the user-configured port.
/// When `port` is None, picks a random available port from the default range.
pub async fn serve_http_on_port(
    state: AppState,
    app: Arc<dyn PathResolver + Send + Sync>,
    handle: tauri::AppHandle,
    port: Option<u16>,
) -> io::Result<()> {
    let (actual_port, listener) = match pick_listener(port).await {
        Ok(x) => x,
        Err(msg) => {
            state.set_mcp_status(false, Some(msg.clone()));
            return Err(io::Error::new(io::ErrorKind::AddrInUse, msg));
        }
    };

    let endpoint_url = format!("http://127.0.0.1:{}/mcp", actual_port);
    eprintln!("[MCP] Listening on {}", endpoint_url);
    {
        let mut status = state.mcp_status.lock().unwrap();
        status.running = true;
        status.endpoint_url = endpoint_url;
        status.last_startup_error = None;
    }
    // Clear stale sessions from previous run.
    {
        let conn = state.db.lock().await;
        let _ = conn.execute("DELETE FROM agent_sessions", []);
    }
    let router = Router::new()
        .route(
            "/mcp",
            post(handle_http_post)
                .delete(handle_http_delete)
                .get(handle_http_get),
        )
        .with_state(HttpServerState {
            state: state.clone(),
            app,
            handle,
        });
    let result = axum::serve(listener, router).await;
    if let Err(err) = &result {
        state.set_mcp_status(false, Some(err.to_string()));
    } else {
        state.set_mcp_status(false, None);
    }
    result.map_err(io::Error::other)
}

async fn handle_http_get(State(_server): State<HttpServerState>, headers: HeaderMap) -> Response {
    if let Some(response) = validate_origin(&headers) {
        return response;
    }
    plain_text_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "GET is not supported for this MCP endpoint.",
    )
}

async fn handle_http_delete(State(server): State<HttpServerState>, headers: HeaderMap) -> Response {
    if let Some(response) = validate_origin(&headers) {
        return response;
    }

    let Some(session_id) = session_header(&headers) else {
        return plain_text_response(StatusCode::NOT_FOUND, "Unknown MCP session.");
    };

    if get_session(&server.state, &session_id).await.is_none() {
        return plain_text_response(StatusCode::NOT_FOUND, "Unknown MCP session.");
    }

    match remove_session(&server.state, &session_id).await {
        Ok(()) => {
            emit_sessions_changed(&server.state, &server.handle).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(err) => plain_text_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

async fn handle_http_post(
    State(server): State<HttpServerState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if let Some(response) = validate_origin(&headers) {
        return response;
    }

    let req = match serde_json::from_str::<JsonRpcRequest>(&body) {
        Ok(req) => req,
        Err(err) => {
            let payload = json_rpc_error(None, -32700, format!("Parse error: {}", err));
            return json_http_response(StatusCode::BAD_REQUEST, &payload, None);
        }
    };

    if req.jsonrpc != "2.0" {
        let payload = json_rpc_error(req.id, -32600, "Only JSON-RPC 2.0 is supported.");
        return json_http_response(StatusCode::BAD_REQUEST, &payload, None);
    }

    if req.method == "initialize" {
        let init = req
            .params
            .clone()
            .and_then(|params| serde_json::from_value::<InitializeParams>(params).ok())
            .unwrap_or_default();
        let host_label = init
            .client_info
            .and_then(|info| info.name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "MCP Host".to_string());
        let protocol_version =
            negotiated_protocol_version(init.protocol_version.as_deref()).to_string();
        let session_id = create_session(&server.state, host_label).await;
        emit_sessions_changed(&server.state, &server.handle).await;
        let payload = json_rpc_result(
            req.id,
            json!({
                "protocolVersion": protocol_version,
                "capabilities": {
                    "tools": {},
                    "resources": {},
                    "prompts": {}
                },
                "serverInfo": {
                    "name": "ecky-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        );
        return json_http_response(StatusCode::OK, &payload, Some(&session_id));
    }

    let Some(session_id) = session_header(&headers) else {
        let payload = json_rpc_error(req.id, -32001, "Unknown MCP session.");
        return json_http_response(StatusCode::NOT_FOUND, &payload, None);
    };

    if get_session(&server.state, &session_id).await.is_none() {
        // Auto-resurrect: server may have restarted and lost in-memory session state.
        // Re-create the session so the client can continue without re-initializing.
        let mut sessions = server.state.mcp_sessions.lock().await;
        sessions.insert(
            session_id.clone(),
            McpSessionState::new("mcp-http".to_string(), "reconnected".to_string()),
        );
    }

    if req.id.is_none() && req.method.starts_with("notifications/") {
        return empty_response(StatusCode::ACCEPTED);
    }

    let payload = dispatch_request(&server, &session_id, req).await;
    emit_sessions_changed(&server.state, &server.handle).await;
    json_http_response(StatusCode::OK, &payload, Some(&session_id))
}

async fn dispatch_request(
    server: &HttpServerState,
    session_id: &str,
    req: JsonRpcRequest,
) -> JsonRpcResponse {
    match req.method.as_str() {
        "ping" => json_rpc_result(req.id, json!({})),
        "resources/list" => json_rpc_result(
            req.id,
            json!({ "resources": resource_definitions(&server.state) }),
        ),
        "resources/read" => {
            match serde_json::from_value::<ReadResourceParams>(req.params.unwrap_or_default()) {
                Ok(params) => match read_resource_text(&server.state, &params.uri) {
                    Some(text) => json_rpc_result(
                        req.id,
                        json!({
                            "contents": [
                                {
                                    "uri": params.uri,
                                    "mimeType": "text/plain",
                                    "text": text
                                }
                            ]
                        }),
                    ),
                    None => {
                        json_rpc_error(req.id, -32602, format!("Unknown resource: {}", params.uri))
                    }
                },
                Err(err) => json_rpc_error(req.id, -32602, format!("Invalid params: {}", err)),
            }
        }
        "prompts/list" => json_rpc_result(req.id, json!({ "prompts": prompt_definitions() })),
        "prompts/get" => {
            match serde_json::from_value::<GetPromptParams>(req.params.unwrap_or_default()) {
                Ok(params) => {
                    let _ = params.arguments;
                    match prompt_payload(&server.state, &params.name) {
                        Some(prompt) => json_rpc_result(req.id, prompt),
                        None => json_rpc_error(
                            req.id,
                            -32602,
                            format!("Unknown prompt: {}", params.name),
                        ),
                    }
                }
                Err(err) => json_rpc_error(req.id, -32602, format!("Invalid params: {}", err)),
            }
        }
        "tools/list" => json_rpc_result(req.id, json!({ "tools": tool_definitions() })),
        "tools/call" => {
            match serde_json::from_value::<CallToolParams>(req.params.unwrap_or_default()) {
                Ok(params) => match dispatch_tool_call(server, session_id, params).await {
                    Ok((value, next_target)) => {
                        if next_target.is_some() {
                            set_session_target(&server.state, session_id, next_target).await;
                        }
                        mcp_tool_success(req.id, &value)
                    }
                    Err(err) => mcp_tool_error(req.id, &err),
                },
                Err(err) => json_rpc_error(req.id, -32602, format!("Invalid params: {}", err)),
            }
        }
        _ => json_rpc_error(req.id, -32601, format!("Method not found: {}", req.method)),
    }
}

async fn dispatch_tool_call(
    server: &HttpServerState,
    session_id: &str,
    params: CallToolParams,
) -> AppResult<(Value, Option<McpTargetRef>)> {
    let session = get_session(&server.state, session_id)
        .await
        .ok_or_else(|| AppError::not_found("MCP session not found."))?;
    let current_ctx = current_context(session_id, &session);
    let args = params.arguments.unwrap_or_else(|| json!({}));

    match params.name.as_str() {
        "health_check" => {
            let response =
                handlers::handle_health_check(&server.state, server.app.as_ref()).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "session_log_in" => {
            let req_args: SessionLoginRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_session_log_in(&server.state, req_args, &current_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "session_log_out" => {
            let req_args: SessionLogoutRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_session_log_out(&server.state, req_args, &current_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "resume_session" => {
            let req_args: SessionResumeRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_session_resume(&server.state, req_args, &current_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "workspace_overview" => {
            let req_args = serde_json::from_value::<WorkspaceOverviewRequest>(args).unwrap_or(
                WorkspaceOverviewRequest {
                    agent_label: None,
                    llm_model_id: None,
                    llm_model_label: None,
                },
            );
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                None,
                None,
            )
            .await?;
            let conn = server.state.db.lock().await;
            let recent_threads = db::get_recent_threads_limited(&conn, 5)
                .map_err(|e| AppError::persistence(e.to_string()))?
                .into_iter()
                .map(thread_list_entry)
                .collect::<Vec<_>>();
            let lease_info = db::get_active_target_lease(
                &conn,
                &target.thread_id,
                &target.message_id,
                target.model_id.as_deref(),
            )
            .map_err(|e| AppError::persistence(e.to_string()))?
            .filter(|lease| lease.session_id != session_id);
            drop(conn);

            let _ = req_args;
            let response = WorkspaceOverviewResponse {
                agent_brief: workspace_overview_brief(&server.state),
                control_surface: workspace_control_surface(&target),
                default_target: WorkspaceOverviewTarget {
                    thread_id: target.thread_id.clone(),
                    message_id: target.message_id.clone(),
                    title: target.title.clone(),
                    version_name: target.version_name.clone(),
                    model_id: target.model_id.clone(),
                    has_draft: target.has_draft,
                },
                recent_threads,
                lease_info,
            };
            Ok((
                serde_json::to_value(response).unwrap(),
                Some(McpTargetRef {
                    thread_id: target.thread_id,
                    message_id: target.message_id,
                    model_id: target.model_id,
                }),
            ))
        }
        "thread_list" => {
            let response = handlers::handle_thread_list(&server.state).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "thread_get" => {
            let req_args: ThreadGetRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_thread_get(&server.state, req_args).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "agent_identity_set" => {
            let req_args: AgentIdentitySetRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            update_session_state(&server.state, session_id, |session| {
                if let Some(agent_label) = req_args
                    .agent_label
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    session.agent_label = agent_label.to_string();
                }
                session.llm_model_id = req_args.llm_model_id.as_ref().and_then(|value| {
                    let trimmed = value.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                });
                session.llm_model_label = req_args.llm_model_label.as_ref().and_then(|value| {
                    let trimmed = value.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                });
            })
            .await?;
            let updated = get_session(&server.state, session_id)
                .await
                .ok_or_else(|| AppError::not_found("MCP session not found."))?;
            let response = current_context(session_id, &updated).as_identity_response();
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "target_get" => {
            let mut req_args =
                serde_json::from_value::<TargetGetRequest>(args).unwrap_or(TargetGetRequest {
                    identity: AgentIdentityOverride::default(),
                    thread_id: None,
                    message_id: None,
                });
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            let response = handlers::handle_target_get(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await?;
            let value = serde_json::to_value(&response).unwrap();
            let next_target = target_ref_from_value(&value);
            Ok((value, next_target))
        }
        "params_patch_and_render" => {
            let mut req_args: ParamsPatchRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_params_patch_and_render(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "macro_replace_and_render" => {
            let mut req_args: MacroReplaceRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_macro_replace_and_render(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "semantic_manifest_get" => {
            let mut req_args = serde_json::from_value::<SemanticManifestRequest>(args).unwrap_or(
                SemanticManifestRequest {
                    identity: AgentIdentityOverride::default(),
                    thread_id: None,
                    message_id: None,
                },
            );
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            let response = handlers::handle_semantic_manifest_get(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await?;
            let value = serde_json::to_value(&response).unwrap();
            let next_target = target_ref_from_value(&value);
            Ok((value, next_target))
        }
        "control_primitive_save" => {
            let mut req_args: ControlPrimitiveSaveRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_control_primitive_save(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    let _ = server.handle.emit("history-updated", ());
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "control_primitive_delete" => {
            let mut req_args: ControlPrimitiveDeleteRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_control_primitive_delete(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    let _ = server.handle.emit("history-updated", ());
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "control_view_save" => {
            let mut req_args: ControlViewSaveRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_control_view_save(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    let _ = server.handle.emit("history-updated", ());
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "control_view_delete" => {
            let mut req_args: ControlViewDeleteRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_control_view_delete(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    let _ = server.handle.emit("history-updated", ());
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "version_save" => {
            let mut req_args =
                serde_json::from_value::<VersionSaveRequest>(args).unwrap_or(VersionSaveRequest {
                    identity: AgentIdentityOverride::default(),
                    thread_id: None,
                    message_id: None,
                    title: None,
                    version_name: None,
                });
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_version_save(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    let _ = server.handle.emit("history-updated", ());
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "thread_fork_from_target" => {
            let mut req_args =
                serde_json::from_value::<ThreadForkRequest>(args).unwrap_or(ThreadForkRequest {
                    identity: AgentIdentityOverride::default(),
                    thread_id: None,
                    message_id: None,
                    title: None,
                    version_name: None,
                });
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_thread_fork_from_target(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    let _ = server.handle.emit("history-updated", ());
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "version_restore" => {
            let req_args: VersionRestoreRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                None,
                Some(req_args.message_id.clone()),
            )
            .await?;
            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            match handlers::handle_version_restore(&server.state, req_args, &current_ctx).await {
                Ok(response) => {
                    let value = serde_json::to_value(&response).unwrap();
                    let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "user_confirm_request" => {
            let req: UserConfirmRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_user_confirm_request(
                &server.state,
                &server.handle,
                req,
                &current_ctx,
            )
            .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "request_user_prompt" => {
            let req: UserPromptRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_request_user_prompt(
                &server.state,
                &server.handle,
                req,
                &current_ctx,
            )
            .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "finalize_thread" => {
            let req_args: FinalizeThreadRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_finalize_thread(&server.state, req_args).await?;
            let _ = server.handle.emit("history-updated", ());
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        _ => Err(AppError::validation(format!(
            "Unknown tool: {}",
            params.name
        ))),
    }
}
