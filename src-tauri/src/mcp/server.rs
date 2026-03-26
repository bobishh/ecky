use crate::db;
use crate::mcp::contracts::*;
use crate::mcp::handlers;
use crate::models::{
    AppError, AppErrorCode, AppResult, AppState, McpSessionState, McpTargetRef, PathResolver,
    TargetLeaseInfo, ViewportScreenshotCapture,
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
use tokio::sync::oneshot;
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
    preview_stl_path: Option<String>,
    viewer_assets: Vec<crate::contracts::ViewerAsset>,
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
    if value
        .get("content")
        .map(|content| content.is_array())
        .unwrap_or(false)
    {
        return json_rpc_result(id, value.clone());
    }
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

fn parse_image_data_url(data_url: &str) -> AppResult<(String, String)> {
    let Some(rest) = data_url.strip_prefix("data:") else {
        return Err(AppError::validation(
            "Viewport screenshot did not return a data URL.",
        ));
    };
    let Some((metadata, payload)) = rest.split_once(',') else {
        return Err(AppError::validation(
            "Viewport screenshot data URL is malformed.",
        ));
    };
    let mut parts = metadata.split(';');
    let mime_type = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::validation("Viewport screenshot is missing a MIME type."))?;
    if !parts.any(|part| part.eq_ignore_ascii_case("base64")) {
        return Err(AppError::validation(
            "Viewport screenshot must use base64 encoding.",
        ));
    }
    if payload.trim().is_empty() {
        return Err(AppError::validation(
            "Viewport screenshot payload is empty.",
        ));
    }
    Ok((mime_type.to_string(), payload.to_string()))
}

fn build_model_screenshot_result(
    requested_target: &ResolvedTargetRef,
    capture: &ViewportScreenshotCapture,
) -> AppResult<Value> {
    let (mime_type, image_payload) = parse_image_data_url(&capture.data_url)?;
    let source = capture.source.trim();
    let summary = if capture.thread_id == requested_target.thread_id
        && capture.message_id == requested_target.message_id
    {
        format!(
            "Viewport screenshot from {} for {} / {}.",
            if source.is_empty() {
                "live-view"
            } else {
                source
            },
            capture.thread_id,
            capture.message_id,
        )
    } else {
        format!(
            "Viewport screenshot from {} captured {} / {} while {} / {} was requested.",
            if source.is_empty() {
                "current-view"
            } else {
                source
            },
            capture.thread_id,
            capture.message_id,
            requested_target.thread_id,
            requested_target.message_id,
        )
    };

    Ok(json!({
        "content": [
            {
                "type": "image",
                "mimeType": mime_type,
                "data": image_payload,
            },
            {
                "type": "text",
                "text": summary,
            }
        ],
        "structuredContent": {
            "threadId": capture.thread_id,
            "messageId": capture.message_id,
            "modelId": capture.model_id,
            "requestedThreadId": requested_target.thread_id,
            "requestedMessageId": requested_target.message_id,
            "requestedModelId": requested_target.model_id,
            "source": capture.source,
            "includeOverlays": capture.include_overlays,
            "camera": capture.camera,
            "width": capture.width,
            "height": capture.height,
            "capturedAt": now_secs(),
        }
    }))
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

async fn create_session(state: &AppState, host_label: String, client_kind: String) -> String {
    let session_id = format!("mcp-http-{}", Uuid::new_v4());
    let mut sessions = state.mcp_sessions.lock().await;
    sessions.insert(
        session_id.clone(),
        McpSessionState::new(client_kind, host_label),
    );
    session_id
}

fn managed_agent_id_from_uri(uri: &axum::http::Uri) -> Option<String> {
    uri.query().and_then(|query| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(key, value)| {
                (key == "managedAgentId" && !value.trim().is_empty()).then(|| value.to_string())
            })
    })
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
        session.last_target = target.clone();
    }
    drop(sessions);
    crate::mcp::runtime::associate_session_target(state, session_id, target.as_ref());
}

async fn remove_session(state: &AppState, session_id: &str) -> AppResult<()> {
    state.mcp_sessions.lock().await.remove(session_id);
    if crate::mcp::runtime::runtime_snapshot_by_session_id(state, session_id).is_some() {
        crate::mcp::runtime::mark_agent_disconnected_for_session(
            state,
            session_id,
            Some("Agent disconnected from Ecky's MCP server.".to_string()),
        );
    }
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
    let runtime_thread_id = crate::mcp::runtime::runtime_snapshot_by_session_id(state, session_id)
        .and_then(|snapshot| snapshot.pending_thread_id);

    let conn = state.db.lock().await;
    let stored_session = db::get_sessions_by_ids(&conn, &[session_id.to_string()])
        .map_err(|e| AppError::persistence(e.to_string()))?
        .into_iter()
        .next();

    let target = if let Some(message_id) = explicit_message_id {
        crate::services::target::resolve_editable_target(
            &conn,
            app,
            explicit_thread_id,
            Some(message_id),
        )?
    } else if let Some(thread_id) = explicit_thread_id {
        let message_id = db::get_latest_successful_message_id_in_thread(&conn, &thread_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| {
                AppError::validation(format!("Thread {} has no successful versions.", thread_id))
            })?;
        crate::services::target::resolve_editable_target(
            &conn,
            app,
            Some(thread_id),
            Some(message_id),
        )?
    } else if let Some(cached_target) = cached_target {
        let still_exists = db::get_message_thread_id(&conn, &cached_target.message_id)
            .map_err(|e| AppError::persistence(e.to_string()))?;
        if still_exists.as_deref() == Some(cached_target.thread_id.as_str()) {
            crate::services::target::resolve_editable_target(
                &conn,
                app,
                Some(cached_target.thread_id),
                Some(cached_target.message_id),
            )?
        } else {
            return Err(AppError::validation(
                "Cached MCP session target is no longer valid. Re-bind the session to an explicit thread/version.",
            ));
        }
    } else if let Some(thread_id) = runtime_thread_id.or_else(|| {
        stored_session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
    }) {
        let message_id = db::get_latest_successful_message_id_in_thread(&conn, &thread_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| {
                AppError::validation(format!("Thread {} has no successful versions.", thread_id))
            })?;
        crate::services::target::resolve_editable_target(
            &conn,
            app,
            Some(thread_id),
            Some(message_id),
        )?
    } else {
        return Err(AppError::validation(
            "No bound MCP session target is available. Provide threadId/messageId or re-bind the session first.",
        ));
    };

    let design = target.design_output.clone();
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
    let model_id = target.model_id();
    let runtime_bundle = target.artifact_bundle.clone();

    Ok(ResolvedTargetRef {
        thread_id: target.thread_id,
        message_id: target.message_id,
        model_id,
        preview_stl_path: runtime_bundle
            .as_ref()
            .map(|bundle| bundle.preview_stl_path.clone()),
        viewer_assets: runtime_bundle
            .map(|bundle| bundle.viewer_assets)
            .unwrap_or_default(),
        title: design.title,
        version_name: design.version_name,
        has_draft: false,
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

async fn request_model_screenshot(
    server: &HttpServerState,
    session_id: &str,
    req: GetModelScreenshotRequest,
) -> AppResult<Value> {
    let target = resolve_target_for_session(
        &server.state,
        server.app.as_ref(),
        session_id,
        req.thread_id.clone(),
        req.message_id.clone(),
    )
    .await?;
    let preview_stl_path = target.preview_stl_path.clone().ok_or_else(|| {
        AppError::validation("Target does not have a preview STL available for screenshots.")
    })?;
    let timeout_secs = req.timeout_secs.unwrap_or(90).clamp(5, 600);
    let request_id = Uuid::new_v4().to_string();
    let include_overlays = req.include_overlays.unwrap_or(true);
    let (tx, rx) = oneshot::channel::<Result<ViewportScreenshotCapture, String>>();

    {
        let mut channels = server.state.viewport_screenshot_channels.lock().await;
        channels.insert(request_id.clone(), tx);
    }

    server
        .handle
        .emit(
            "agent-viewport-screenshot-request",
            AgentViewportScreenshotEvent {
                request_id: request_id.clone(),
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
                preview_stl_path,
                viewer_assets: target.viewer_assets.clone(),
                include_overlays,
                camera: req.camera.clone(),
            },
        )
        .map_err(|e| AppError::internal(format!("Failed to emit screenshot event: {}", e)))?;

    let capture = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx)
        .await
        .map_err(|_| {
            let state_clone = server.state.viewport_screenshot_channels.clone();
            let id_clone = request_id.clone();
            tokio::spawn(async move {
                state_clone.lock().await.remove(&id_clone);
            });
            AppError::internal(format!(
                "Viewport screenshot timed out after {} seconds.",
                timeout_secs
            ))
        })?
        .map_err(|_| AppError::internal("Viewport screenshot channel closed unexpectedly."))?
        .map_err(AppError::validation)?;

    build_model_screenshot_result(&target, &capture)
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

fn thread_list_entry(
    thread: crate::models::Thread,
    claim_owner: Option<crate::models::AgentSession>,
) -> ThreadListEntry {
    ThreadListEntry {
        thread_id: thread.id,
        title: thread.title,
        updated_at: thread.updated_at,
        version_count: thread.version_count,
        pending_count: thread.pending_count,
        queued_count: thread.queued_count,
        error_count: thread.error_count,
        status: thread.status,
        finalized_at: thread.finalized_at,
        claim_owner,
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
            "- For semantic controls, use knobs/views that reflect the actual model structure.\n",
            "- Semantic views are curated user-facing editing contexts layered on top of raw uiSpec/params; they do not replace the raw control surface.\n",
            "- Reuse or extend an existing relevant view before creating a new one.\n",
            "- Create or edit views when grouping related controls will make the design easier to edit or understand.\n",
            "- Do not remove or bypass existing views just because the same values can be changed through raw controls.\n\n",
            "Recommended startup sequence:\n",
            "1. Read ecky://guides/system-prompt and ecky://guides/modeling-guidelines.\n",
            "2. If the thread engine is Ecky IR v0, read ecky://guides/ecky-ir-v0 before you generate or replace macro code.\n",
            "3. Read ecky://guides/cad-sdk when you need the actual helper surface instead of guessing SDK functions.\n",
            "4. Call workspace_overview.\n",
            "5. If workspace_overview says the thread has no saved versions yet, use that thread context plus the guides to create the first version instead of calling target_meta_get.\n",
            "6. Otherwise call target_meta_get.\n",
            "7. For macro geometry/orientation questions, call target_macro_get.\n",
            "8. For exact chunks, call target_detail_get(section=...).\n",
            "9. Call semantic_manifest_get when semantic bindings or existing views matter; inspect current views before inventing new groupings.\n",
            "10. Use target_get only when you truly need the full payload.\n",
            "11. Then mutate with params_patch_and_render, macro_replace_and_render, or semantic tools, keeping semantic views aligned with the underlying raw params.\n",
            "12. Use measurement_annotation tools to encode what dimensions mean instead of leaving that meaning only in prose.\n",
            "13. For long steps, call long_action_notice and long_action_clear so Ecky can show clean busy status without scraping terminal text.\n"
        ),
        selected_engine_label(state)
    )
}

fn workspace_overview_brief(state: &AppState) -> WorkspaceOverviewBrief {
    let engine_kind = state.config.lock().unwrap().default_engine_kind.clone();
    let is_ir = engine_kind == crate::models::EngineKind::EckyIrV0;
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
            "Treat semantic views as curated user-facing control contexts layered over raw uiSpec/params."
                .to_string(),
        ],
        resources: vec![
            "ecky://guides/system-prompt".to_string(),
            "ecky://guides/technical-system-prompt".to_string(),
            "ecky://guides/modeling-guidelines".to_string(),
            "ecky://guides/ecky-ir-v0".to_string(),
            "ecky://guides/cad-sdk".to_string(),
        ],
        next_steps: if is_ir {
            vec![
                "Read ecky://guides/system-prompt if you have not loaded Ecky guidance yet."
                    .to_string(),
                "Read ecky://guides/ecky-ir-v0 NOW — this thread uses the Ecky IR v0 engine. Do not write Python."
                    .to_string(),
                "Call target_meta_get for a lightweight summary of the current editable target."
                    .to_string(),
                "Use target_macro_get for macro reasoning, target_detail_get(section=...) for exact chunks, and target_get only as a full-payload fallback."
                    .to_string(),
                "If the target has a semantic manifest, inspect existing views before creating new control groupings."
                    .to_string(),
            ]
        } else {
            vec![
                "Read ecky://guides/system-prompt if you have not loaded Ecky guidance yet."
                    .to_string(),
                "Read ecky://guides/ecky-ir-v0 when the thread is using the Ecky IR engine."
                    .to_string(),
                "Read ecky://guides/cad-sdk if you need the actual helper functions available in cad_sdk.py."
                    .to_string(),
                "Call target_meta_get for a lightweight summary of the current editable target."
                    .to_string(),
                "Use target_macro_get for macro reasoning, target_detail_get(section=...) for exact chunks, and target_get only as a full-payload fallback."
                    .to_string(),
                "If the target has a semantic manifest, inspect existing views before creating new control groupings."
                    .to_string(),
            ]
        },
        cad_sdk_snippet: if is_ir {
            Some(crate::commands::generation::ecky_ir_v0_guide_text().to_string())
        } else {
            Some(include_str!("../../../model-runtime/cad_sdk.py").to_string())
        },
    }
}

fn workspace_control_surface(target: &ResolvedTargetRef) -> WorkspaceControlSurface {
    let mut hints = vec![];
    if target.ui_field_count > 0 {
        hints.push(format!(
            "This target exposes {} uiSpec fields. Use target_detail_get(section=\"uiSpec\") to inspect exact control keys, defaults, and option values.",
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
            "Semantic manifest is present with {} control primitives, {} relations, and {} views. Views are curated user-facing groupings layered over raw controls. Use semantic_manifest_get to inspect bindings and current views before editing them with control_view/control_primitive/measurement_annotation tools.",
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

fn workspace_control_surface_for_empty_thread(
    thread: &crate::contracts::Thread,
) -> WorkspaceControlSurface {
    WorkspaceControlSurface {
        ui_field_count: 0,
        range_count: 0,
        number_count: 0,
        select_count: 0,
        checkbox_count: 0,
        parameter_count: 0,
        has_semantic_manifest: false,
        control_primitive_count: 0,
        control_relation_count: 0,
        control_view_count: 0,
        hints: vec![
            "This thread has no saved versions yet. There is no editable target to inspect with target_meta_get/target_macro_get yet.".to_string(),
            format!(
                "The thread currently has {} queued user message(s). Drain and answer the whole batch before you build the first version.",
                thread.queued_count
            ),
            "Use the selected engine guide plus the thread history to create the first version for this thread.".to_string(),
        ],
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
        json!({
            "uri": "ecky://guides/ecky-ir-v0",
            "name": "Ecky IR v0",
            "description": "Canonical Ecky IR v0 syntax and modeling rules for the experimental Rust engine.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/cad-sdk",
            "name": "cad_sdk.py",
            "description": "The actual CAD framework helpers available to Ecky-generated macros.",
            "mimeType": "text/plain"
        }),
    ]
}

fn read_resource_text(state: &AppState, uri: &str) -> Option<String> {
    match uri {
        "ecky://guides/system-prompt" => Some(selected_engine_prompt(state)),
        "ecky://guides/technical-system-prompt" => Some(crate::TECHNICAL_SYSTEM_PROMPT.to_string()),
        "ecky://guides/modeling-guidelines" => Some(workflow_guide_text(state)),
        "ecky://guides/ecky-ir-v0" => {
            Some(crate::commands::generation::ecky_ir_v0_guide_text().to_string())
        }
        "ecky://guides/cad-sdk" => {
            Some(include_str!("../../../model-runtime/cad_sdk.py").to_string())
        }
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
            "description": "Notify the workspace that an agent has joined and bind this MCP session to a thread. External/passive agents should call thread_list/thread_get first, then pass threadId (or messageId). Managed active agents may inherit the wake target. If another live agent already owns that thread, the call fails unless stealThread is true.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agentLabel": { "type": "string" },
                    "threadId": { "type": "string", "description": "Thread to bind this session to." },
                    "messageId": { "type": "string", "description": "Optional version message inside that thread. If provided without threadId, Ecky derives the thread from the message." },
                    "modelId": { "type": "string", "description": "Optional model id for the bound target." },
                    "stealThread": { "type": "boolean", "description": "Explicitly take over a thread that is currently claimed by another live agent session." }
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
            "name": "target_meta_get",
            "description": "Fetch a lightweight summary of the current editable target. Preferred default read step after workspace_overview.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "target_macro_get",
            "description": "Fetch only the active editable macro payload for geometry, orientation, and structure reasoning. Prefer this over target_get for macro questions.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "target_detail_get",
            "description": "Fetch one exact chunk of the active editable target by section. Use this instead of target_get when you only need uiSpec, params, or artifactBundle. latestDraft is deprecated and currently always null.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("section", json!({
                        "type": "string",
                        "enum": ["uiSpec", "initialParams", "artifactBundle", "latestDraft"]
                    }))
                ],
                &["section"],
            )
        }),
        json!({
            "name": "target_get",
            "description": "Fetch the full current editable target payload. Expensive; prefer target_meta_get, target_macro_get, or target_detail_get unless you truly need everything.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "get_model_screenshot",
            "description": "Capture the current model viewport as Ecky can see it. Defaults to the visible workbench view; if the requested target is not open, Ecky asks the user how to proceed.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("includeOverlays", json!({ "type": "boolean", "description": "Whether to include the current drawing overlay in live captures. Defaults to true." })),
                    ("camera", json!({
                        "type": "object",
                        "properties": {
                            "position": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
                            "target": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
                            "zoom": { "type": "number" },
                            "fov": { "type": "number" }
                        }
                    })),
                    ("timeoutSecs", json!({ "type": "number", "description": "Seconds to wait for the UI capture flow. Default 90, max 600." }))
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
                "For numeric parameters, prefer number; range is legacy-only unless explicitly needed. range/number: min, max, step (numbers). ",
                "select: options array of {label, value} objects — MUST have at least one option. ",
                "checkbox: no extra fields. ",
                "image: use for file-picker inputs (e.g. a reference photo) — no extra fields, value is an absolute file path string once chosen by the user. ",
                "parameters is a flat key→value map matching uiSpec field keys. ",
                "For image fields, the parameter may be omitted or set to an empty string until the user picks a file in the UI."
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
            "name": "measurement_annotation_save",
            "description": "Create or update one measurement semantic annotation and save a new version.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("annotation", json!({ "type": "object" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &["annotation"],
            )
        }),
        json!({
            "name": "measurement_annotation_delete",
            "description": "Delete one measurement semantic annotation and save a new version.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("annotationId", json!({ "type": "string" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &["annotationId"],
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
            "description": "Request text input from the human in the Ecky UI for a specific thread. Blocks until the user submits or the timeout expires. Use the session's bound thread from session_log_in, or pass threadId/messageId explicitly to reassert the same target. Ecky will not guess from the current workspace view. If timeoutSecs is omitted, Ecky uses the configured MCP prompt timeout. The response includes promptText/attachments plus threadId/threadTitle for the bound thread context. Each attachment path is already an absolute local file path staged by Ecky and should be opened directly with your normal file/image tools; do not rewrite or guess a different path. A timeout is normal when the user does not answer right away; poll again later or call session_log_out if you are leaving the workspace. In active MCP mode, call this again immediately after each completed user-facing turn so Ecky can queue the next message.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Optional context message shown to the user above the input (e.g. 'What would you like me to build?')." },
                    "requestId": { "type": "string", "description": "Optional stable ID for deduplication." },
                    "timeoutSecs": { "type": "number", "description": "Seconds to wait. If omitted, Ecky uses the configured MCP prompt timeout. Max 1800." },
                    "threadId": { "type": "string", "description": "Optional explicit thread target. Required if the session is not already bound." },
                    "messageId": { "type": "string", "description": "Optional explicit version message target. If provided without threadId, Ecky derives the thread from the message." },
                    "modelId": { "type": "string", "description": "Optional model id for the explicit target." }
                }
            }
        }),
        json!({
            "name": "mark_as_read",
            "description": "Claim queued user thread messages after you read them from thread_get/thread_list. Pass any one pending user message id from the thread; Ecky will drain the whole pending batch for that thread into the current turn.",
            "inputSchema": with_identity(
                &[
                    ("messageId", json!({ "type": "string" })),
                    ("threadId", json!({ "type": "string" }))
                ],
                &["messageId"],
            )
        }),
        json!({
            "name": "session_reply_save",
            "description": "Save one final assistant reply into the current thread history. Use this for final user-facing text or fatal turn-ending errors, not for step-by-step progress. After saving the final reply for a turn, immediately call request_user_prompt again.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("body", json!({ "type": "string" })),
                    ("fatal", json!({ "type": "boolean" }))
                ],
                &["body"],
            )
        }),
        json!({
            "name": "session_activity_set",
            "description": "Set the current MCP session activity state so Ecky can drive bubble, microwave, and timer UX without scraping terminal text. Use this for any long or meaningful step.",
            "inputSchema": with_identity(
                &[
                    ("phase", json!({ "type": "string" })),
                    ("label", json!({ "type": "string" })),
                    ("detail", json!({ "type": "string" })),
                    ("attentionKind", json!({ "type": "string" }))
                ],
                &["phase"],
            )
        }),
        json!({
            "name": "session_activity_clear",
            "description": "Clear the current explicit MCP session activity state after a step finishes. Optionally set the next phase or idle status text.",
            "inputSchema": with_identity(
                &[
                    ("phase", json!({ "type": "string" })),
                    ("statusText", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "long_action_notice",
            "description": "Compatibility alias for session_activity_set. Prefer session_activity_set for new agents.",
            "inputSchema": with_identity(
                &[
                    ("message", json!({ "type": "string" })),
                    ("phase", json!({ "type": "string" })),
                    ("details", json!({ "type": "string" }))
                ],
                &["message"],
            )
        }),
        json!({
            "name": "long_action_clear",
            "description": "Compatibility alias for session_activity_clear. Prefer session_activity_clear for new agents.",
            "inputSchema": with_identity(
                &[
                    ("phase", json!({ "type": "string" })),
                    ("statusText", json!({ "type": "string" }))
                ],
                &[],
            )
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
    uri: axum::http::Uri,
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
        let managed_agent_id = managed_agent_id_from_uri(&uri).filter(|agent_id| {
            crate::mcp::runtime::runtime_snapshot_by_id(&server.state, agent_id).is_some()
        });
        let client_kind = if managed_agent_id.is_some() {
            "managed-mcp-http".to_string()
        } else {
            "mcp-http".to_string()
        };
        let session_id = create_session(&server.state, host_label, client_kind).await;
        if let Some(agent_id) = managed_agent_id {
            crate::mcp::runtime::bind_managed_http_session(
                &server.state,
                &agent_id,
                &session_id,
                Some("Connected to Ecky MCP.".to_string()),
            );
        }
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
            let live_bound_thread_id = server
                .state
                .mcp_sessions
                .lock()
                .await
                .get(session_id)
                .and_then(|session| session.bound_thread_id.clone())
                .or_else(|| {
                    crate::mcp::runtime::runtime_snapshot_by_session_id(&server.state, session_id)
                        .and_then(|snapshot| snapshot.pending_thread_id)
                });
            let target_result = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                None,
                None,
            )
            .await;
            let claim_owners = handlers::claim_owners_by_thread(&server.state).await;
            let conn = server.state.db.lock().await;
            let recent_threads = db::get_recent_threads_limited(&conn, 5)
                .map_err(|e| AppError::persistence(e.to_string()))?
                .into_iter()
                .map(|thread| {
                    let thread_id = thread.id.clone();
                    thread_list_entry(thread, claim_owners.get(&thread_id).cloned())
                })
                .collect::<Vec<_>>();

            let _ = req_args;
            let (response, next_target) = match target_result {
                Ok(target) => {
                    let lease_info = db::get_active_target_lease(
                        &conn,
                        &target.thread_id,
                        &target.message_id,
                        target.model_id.as_deref(),
                    )
                    .map_err(|e| AppError::persistence(e.to_string()))?
                    .filter(|lease| lease.session_id != session_id);
                    let next_target = Some(McpTargetRef {
                        thread_id: target.thread_id.clone(),
                        message_id: target.message_id.clone(),
                        model_id: target.model_id.clone(),
                    });
                    (
                        WorkspaceOverviewResponse {
                            agent_brief: workspace_overview_brief(&server.state),
                            control_surface: workspace_control_surface(&target),
                            default_target: WorkspaceOverviewTarget {
                                thread_id: target.thread_id.clone(),
                                message_id: Some(target.message_id.clone()),
                                title: target.title.clone(),
                                version_name: Some(target.version_name.clone()),
                                model_id: target.model_id.clone(),
                                has_draft: target.has_draft,
                                has_version: true,
                                claim_owner: claim_owners.get(&target.thread_id).cloned(),
                            },
                            recent_threads,
                            lease_info,
                        },
                        next_target,
                    )
                }
                Err(err) if err.message.contains("has no successful versions") => {
                    let stored_thread_id =
                        db::get_sessions_by_ids(&conn, &[session_id.to_string()])
                            .map_err(|e| AppError::persistence(e.to_string()))?
                            .into_iter()
                            .next()
                            .and_then(|session| session.thread_id);
                    let thread_id = live_bound_thread_id.or(stored_thread_id).ok_or(err)?;
                    let thread = crate::services::history::get_thread(&conn, &thread_id)?;
                    (
                        WorkspaceOverviewResponse {
                            agent_brief: workspace_overview_brief(&server.state),
                            control_surface: workspace_control_surface_for_empty_thread(&thread),
                            default_target: WorkspaceOverviewTarget {
                                thread_id: thread.id.clone(),
                                message_id: None,
                                title: thread.title.clone(),
                                version_name: None,
                                model_id: None,
                                has_draft: false,
                                has_version: false,
                                claim_owner: claim_owners.get(&thread.id).cloned(),
                            },
                            recent_threads,
                            lease_info: None,
                        },
                        None,
                    )
                }
                Err(err) => return Err(err),
            };
            drop(conn);
            Ok((serde_json::to_value(response).unwrap(), next_target))
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
            let active_mode_enabled = {
                let config = server.state.config.lock().unwrap().clone();
                crate::mcp::runtime::active_mode_enabled(&config)
            };
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
            if active_mode_enabled
                && crate::mcp::runtime::runtime_snapshot_by_session_id(&server.state, session_id)
                    .is_some()
            {
                crate::mcp::runtime::mark_managed_session_active(
                    &server.state,
                    session_id,
                    None,
                    updated.llm_model_label.clone(),
                    Some("Connected to Ecky MCP.".to_string()),
                );
            }
            let response = current_context(session_id, &updated).as_identity_response();
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "target_meta_get" => {
            let mut req_args =
                serde_json::from_value::<TargetMetaRequest>(args).unwrap_or(TargetMetaRequest {
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
            let response = handlers::handle_target_meta_get(
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
        "target_macro_get" => {
            let mut req_args =
                serde_json::from_value::<TargetMacroRequest>(args).unwrap_or(TargetMacroRequest {
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
            let response = handlers::handle_target_macro_get(
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
        "target_detail_get" => {
            let mut req_args: TargetDetailRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
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
            let response = handlers::handle_target_detail_get(
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
        "get_model_screenshot" => {
            let req_args: GetModelScreenshotRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let value = request_model_screenshot(server, session_id, req_args).await?;
            Ok((value, None))
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
            let target_result = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await;
            match target_result {
                Ok(target) => {
                    // Normal path: existing version found, acquire lease and replace.
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
                        &action_ctx,
                    )
                    .await
                    {
                        Ok(response) => {
                            let value = serde_json::to_value(&response).unwrap();
                            let next_target =
                                target_ref_from_value(&value).unwrap_or(lease_target.clone());
                            move_or_refresh_lease(
                                &server.state,
                                &action_ctx,
                                &lease_target,
                                &next_target,
                            )
                            .await?;
                            Ok((value, Some(next_target)))
                        }
                        Err(err) => {
                            let _ =
                                release_lease(&server.state, &action_ctx.session_id, &lease_target)
                                    .await;
                            Err(err)
                        }
                    }
                }
                Err(ref e)
                    if e.code == crate::contracts::AppErrorCode::Validation
                        && e.message.contains("has no successful versions") =>
                {
                    // Bootstrap path: thread exists but has no versions yet.
                    // Skip lease acquisition — there is nothing to compete for.
                    match handlers::handle_macro_replace_and_render(
                        &server.state,
                        server.app.as_ref(),
                        req_args,
                        &action_ctx,
                    )
                    .await
                    {
                        Ok(response) => {
                            let value = serde_json::to_value(&response).unwrap();
                            let next_target = target_ref_from_value(&value);
                            Ok((value, next_target))
                        }
                        Err(err) => Err(err),
                    }
                }
                Err(e) => Err(e),
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
        "measurement_annotation_save" => {
            let mut req_args: MeasurementAnnotationSaveRequest =
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
            match handlers::handle_measurement_annotation_save(
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
        "measurement_annotation_delete" => {
            let mut req_args: MeasurementAnnotationDeleteRequest =
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
            match handlers::handle_measurement_annotation_delete(
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
        "mark_as_read" => {
            let req: MarkAsReadRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_mark_as_read(&server.state, req, &current_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "session_reply_save" => {
            let req: SessionReplySaveRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_session_reply_save(&server.state, req, &current_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "session_activity_set" => {
            let req: SessionActivitySetRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_session_activity_set(&server.state, req, &current_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "session_activity_clear" => {
            let req: SessionActivityClearRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_session_activity_clear(&server.state, req, &current_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "long_action_notice" => {
            let req: LongActionNoticeRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_long_action_notice(&server.state, req, &current_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "long_action_clear" => {
            let req: LongActionClearRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_long_action_clear(&server.state, req, &current_ctx).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Config, McpConfig};
    use rusqlite::Connection;

    fn test_state() -> AppState {
        AppState::new(
            Config {
                engines: Vec::new(),
                selected_engine_id: String::new(),
                freecad_cmd: String::new(),
                assets: Vec::new(),
                microwave: None,
                mcp: McpConfig::default(),
                has_seen_onboarding: true,
                connection_type: None,
                default_engine_kind: crate::models::EngineKind::Freecad,
            },
            None,
            Connection::open_in_memory().expect("memory db"),
        )
    }

    #[test]
    fn tool_definitions_include_get_model_screenshot() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(
            tool_names.iter().any(|name| name == "get_model_screenshot"),
            "expected get_model_screenshot in {:?}",
            tool_names
        );
    }

    #[test]
    fn tool_definitions_include_target_read_split_tools() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names.iter().any(|name| name == "target_meta_get"));
        assert!(tool_names.iter().any(|name| name == "target_macro_get"));
        assert!(tool_names.iter().any(|name| name == "target_detail_get"));
        assert!(tool_names.iter().any(|name| name == "target_get"));
    }

    #[test]
    fn tool_definitions_include_measurement_annotation_tools() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names
            .iter()
            .any(|name| name == "measurement_annotation_save"));
        assert!(tool_names
            .iter()
            .any(|name| name == "measurement_annotation_delete"));
    }

    #[test]
    fn tool_definitions_include_long_action_activity_tools() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names.iter().any(|name| name == "long_action_notice"));
        assert!(tool_names.iter().any(|name| name == "long_action_clear"));
    }

    #[test]
    fn guidance_prefers_meta_macro_and_detail_over_target_get() {
        let state = test_state();
        let workflow = workflow_guide_text(&state);
        let brief = workspace_overview_brief(&state);

        assert!(workflow.contains("ecky://guides/ecky-ir-v0"));
        assert!(workflow.contains("call target_meta_get"));
        assert!(workflow.contains("call target_macro_get"));
        assert!(workflow.contains("call target_detail_get(section=...)"));
        assert!(workflow.contains("Use target_get only when you truly need the full payload"));
        assert!(workflow.contains("measurement_annotation tools"));
        assert!(workflow.contains("long_action_notice"));
        assert!(!workflow.contains("If needed, call target_get or thread_get"));

        assert!(brief
            .resources
            .iter()
            .any(|resource| resource == "ecky://guides/ecky-ir-v0"));
        assert!(brief
            .next_steps
            .iter()
            .any(|step| step.contains("ecky://guides/ecky-ir-v0")));
        assert!(brief
            .next_steps
            .iter()
            .any(|step| step.contains("target_meta_get")));
    }

    #[test]
    fn ecky_ir_resource_exposes_canonical_sample() {
        let state = test_state();
        let ir_guide =
            read_resource_text(&state, "ecky://guides/ecky-ir-v0").expect("ir guide resource");

        assert!(ir_guide.contains("(model ...)"));
        assert!(ir_guide.contains("rounded-polygon"));
        assert!(ir_guide.contains("postProcessing.lithophaneAttachments"));
        assert!(resource_definitions(&state)
            .into_iter()
            .any(|resource| resource.get("uri").and_then(Value::as_str)
                == Some("ecky://guides/ecky-ir-v0")));
    }

    #[test]
    fn mcp_tool_success_preserves_rich_content_payloads() {
        let payload = json!({
            "content": [
                { "type": "text", "text": "hello" }
            ],
            "structuredContent": {
                "source": "visible-live"
            }
        });

        let response = mcp_tool_success(Some(json!(1)), &payload);
        assert_eq!(response.result, Some(payload));
    }

    #[test]
    fn parse_image_data_url_accepts_base64_images() {
        let (mime_type, payload) =
            parse_image_data_url("data:image/jpeg;base64,Zm9v").expect("valid data URL");
        assert_eq!(mime_type, "image/jpeg");
        assert_eq!(payload, "Zm9v");
    }

    #[test]
    fn build_model_screenshot_result_includes_image_and_metadata() {
        let requested_target = ResolvedTargetRef {
            thread_id: "thread-1".to_string(),
            message_id: "message-1".to_string(),
            model_id: Some("model-1".to_string()),
            preview_stl_path: Some("/tmp/model.stl".to_string()),
            viewer_assets: vec![],
            title: "Widget".to_string(),
            version_name: "V1".to_string(),
            has_draft: false,
            ui_field_count: 0,
            range_count: 0,
            number_count: 0,
            select_count: 0,
            checkbox_count: 0,
            parameter_count: 0,
            has_semantic_manifest: false,
            control_primitive_count: 0,
            control_relation_count: 0,
            control_view_count: 0,
        };
        let capture = ViewportScreenshotCapture {
            data_url: "data:image/jpeg;base64,Zm9v".to_string(),
            width: 1280,
            height: 720,
            camera: crate::contracts::ViewportCameraState {
                position: [1.0, 2.0, 3.0],
                target: [0.0, 0.0, 0.0],
                zoom: None,
                fov: Some(45.0),
            },
            source: "visible-live".to_string(),
            thread_id: "thread-1".to_string(),
            message_id: "message-1".to_string(),
            model_id: Some("model-1".to_string()),
            include_overlays: true,
        };

        let result = build_model_screenshot_result(&requested_target, &capture)
            .expect("screenshot payload should be valid");
        assert_eq!(result["content"][0]["type"], "image");
        assert_eq!(result["content"][0]["mimeType"], "image/jpeg");
        assert_eq!(result["content"][0]["data"], "Zm9v");
        assert_eq!(result["structuredContent"]["source"], "visible-live");
        assert_eq!(result["structuredContent"]["threadId"], "thread-1");
        assert_eq!(result["structuredContent"]["width"], 1280);
        assert_eq!(result["structuredContent"]["includeOverlays"], true);
    }
}
