use crate::db;
use crate::mcp::authoring::authoring_card_text;
use crate::mcp::contracts::*;
use crate::mcp::handlers;
use crate::mcp::handlers::AgentContext;
use crate::models::{
    AppError, AppErrorCode, AppResult, AppState, ArtifactBundle, Config,
    FreecadLibraryImportRequest, FreecadLibrarySearchRequest, McpSessionState, McpTargetRef,
    Message, MessageRole, MessageStatus, ModelManifest, PathResolver, TargetLeaseInfo,
    ViewportScreenshotCapture,
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
#[serde(rename_all = "camelCase")]
struct EckyAstSetNumberCallRequest {
    #[serde(flatten)]
    identity: AgentIdentityOverride,
    thread_id: Option<String>,
    message_id: Option<String>,
    source_digest: String,
    path: String,
    expected_node_digest: String,
    value: f64,
    parameters: Option<crate::models::DesignParams>,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: Option<crate::models::GeometryBackend>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EckyAstSetStringCallRequest {
    #[serde(flatten)]
    identity: AgentIdentityOverride,
    thread_id: Option<String>,
    message_id: Option<String>,
    source_digest: String,
    path: String,
    expected_node_digest: String,
    value: String,
    parameters: Option<crate::models::DesignParams>,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: Option<crate::models::GeometryBackend>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EckyAstSetSelectCallRequest {
    #[serde(flatten)]
    identity: AgentIdentityOverride,
    thread_id: Option<String>,
    message_id: Option<String>,
    source_digest: String,
    path: String,
    expected_node_digest: String,
    value: serde_json::Value,
    parameters: Option<crate::models::DesignParams>,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: Option<crate::models::GeometryBackend>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EckyAstReplaceCallRequest {
    #[serde(flatten)]
    identity: AgentIdentityOverride,
    thread_id: Option<String>,
    message_id: Option<String>,
    source_digest: String,
    path: String,
    expected_node_digest: String,
    replacement_source: String,
    parameters: Option<crate::models::DesignParams>,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: Option<crate::models::GeometryBackend>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EckyAstInsertBindingCallRequest {
    #[serde(flatten)]
    identity: AgentIdentityOverride,
    thread_id: Option<String>,
    message_id: Option<String>,
    source_digest: String,
    path: String,
    expected_node_digest: String,
    binding_source: String,
    position: Option<String>,
    parameters: Option<crate::models::DesignParams>,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: Option<crate::models::GeometryBackend>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EckyAstDeleteBindingCallRequest {
    #[serde(flatten)]
    identity: AgentIdentityOverride,
    thread_id: Option<String>,
    message_id: Option<String>,
    source_digest: String,
    path: String,
    expected_node_digest: String,
    parameters: Option<crate::models::DesignParams>,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: Option<crate::models::GeometryBackend>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EckyAstRenameBindingCallRequest {
    #[serde(flatten)]
    identity: AgentIdentityOverride,
    thread_id: Option<String>,
    message_id: Option<String>,
    source_digest: String,
    path: String,
    expected_node_digest: String,
    new_name: String,
    parameters: Option<crate::models::DesignParams>,
    post_processing: Option<crate::models::PostProcessingSpec>,
    geometry_backend: Option<crate::models::GeometryBackend>,
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
    source_language: crate::models::SourceLanguage,
    geometry_backend: crate::models::GeometryBackend,
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
    handle: Option<tauri::AppHandle>,
}

fn require_server_handle<'a>(
    server: &'a HttpServerState,
    tool_name: &str,
) -> AppResult<&'a tauri::AppHandle> {
    server.handle.as_ref().ok_or_else(|| {
        AppError::internal(format!(
            "{tool_name} requires a live tauri AppHandle, but none is attached in this context."
        ))
    })
}

fn emit_history_updated(server: &HttpServerState) {
    if let Some(handle) = server.handle.as_ref() {
        let _ = handle.emit("history-updated", ());
    }
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
            "image": {
                "dataUrl": capture.data_url,
                "mimeType": mime_type,
                "base64": image_payload,
            },
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
            ));
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
    // Close pending prompts before removing the session (close_single_prompt needs the session entry).
    state
        .close_prompts_for_session(session_id, "session_disconnected")
        .await;
    state.mcp_sessions.lock().await.remove(session_id);
    if crate::mcp::runtime::runtime_snapshot_by_session_id(state, session_id).is_some() {
        crate::mcp::runtime::mark_agent_disconnected_for_session(
            state,
            session_id,
            Some("Agent disconnected from Ecky's MCP server.".to_string()),
        );
    }
    state
        .mcp_session_read_resources
        .lock()
        .await
        .remove(session_id);
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
    if let Some(session) = state.mcp_sessions.lock().await.get(session_id).cloned() {
        let ctx = current_context(session_id, &session);
        if let Some(preview) = handlers::resolve_session_render_preview_for_request(
            state,
            &ctx,
            explicit_thread_id.as_deref(),
            explicit_message_id.as_deref(),
        )
        .await?
        {
            let design = preview.design_output.clone();
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
            return Ok(ResolvedTargetRef {
                thread_id: preview.thread_id,
                message_id: preview.preview_id,
                model_id: Some(preview.artifact_bundle.model_id.clone()),
                source_language: design.source_language,
                geometry_backend: design.geometry_backend,
                preview_stl_path: Some(preview.artifact_bundle.preview_stl_path),
                viewer_assets: preview.artifact_bundle.viewer_assets,
                title: design.title,
                version_name: design.version_name,
                has_draft: true,
                ui_field_count: design.ui_spec.fields.len(),
                range_count,
                number_count,
                select_count,
                checkbox_count,
                parameter_count: design.initial_params.len(),
                has_semantic_manifest: true,
                control_primitive_count: preview.model_manifest.control_primitives.len(),
                control_relation_count: preview.model_manifest.control_relations.len(),
                control_view_count: preview.model_manifest.control_views.len(),
            });
        }
    }

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
        crate::services::target::resolve_editable_target(&conn, app, Some(thread_id), None)?
    } else if let Some(cached_target) = cached_target {
        let still_exists = db::get_visible_message_thread_id(&conn, &cached_target.message_id)
            .map_err(|e| AppError::persistence(e.to_string()))?;
        if still_exists.as_deref() == Some(cached_target.thread_id.as_str()) {
            let cached_thread_id = cached_target.thread_id.clone();
            let cached_message_id = cached_target.message_id.clone();
            match crate::services::target::resolve_editable_target(
                &conn,
                app,
                Some(cached_thread_id.clone()),
                Some(cached_message_id),
            ) {
                Ok(target) => target,
                Err(err) if err.code == AppErrorCode::NotFound => {
                    crate::services::target::resolve_editable_target(
                        &conn,
                        app,
                        Some(cached_thread_id),
                        None,
                    )?
                }
                Err(err) => return Err(err),
            }
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
        crate::services::target::resolve_editable_target(&conn, app, Some(thread_id), None)?
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
        source_language: design.source_language,
        geometry_backend: design.geometry_backend,
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

async fn bound_thread_id_for_session(state: &AppState, session_id: &str) -> Option<String> {
    if let Some(thread_id) = state
        .mcp_sessions
        .lock()
        .await
        .get(session_id)
        .and_then(|session| session.bound_thread_id.clone())
    {
        return Some(thread_id);
    }

    if let Some(thread_id) = crate::mcp::runtime::runtime_snapshot_by_session_id(state, session_id)
        .and_then(|snapshot| snapshot.pending_thread_id)
    {
        return Some(thread_id);
    }

    let conn = state.db.lock().await;
    db::get_sessions_by_ids(&conn, &[session_id.to_string()])
        .ok()
        .and_then(|sessions| {
            sessions
                .into_iter()
                .next()
                .and_then(|session| session.thread_id)
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
    let handle = require_server_handle(server, "get_model_screenshot")?;

    {
        let mut channels = server.state.viewport_screenshot_channels.lock().await;
        channels.insert(request_id.clone(), tx);
    }

    handle
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
        })
        .or_else(|| {
            value
                .get("artifactDigest")
                .and_then(|digest| digest.get("modelId"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    Some(McpTargetRef {
        thread_id,
        message_id,
        model_id,
    })
}

fn source_line_count(source: &str) -> usize {
    source.lines().count()
}

fn compact_macro_replace_response_value(response: &MacroReplaceResponse) -> Value {
    json!({
        "threadId": &response.thread_id,
        "messageId": &response.message_id,
        "modelId": &response.artifact_digest.model_id,
        "digest": crate::mcp::macro_buffer::source_digest(&response.macro_code),
        "lineCount": source_line_count(&response.macro_code),
        "artifactDigest": &response.artifact_digest,
        "structuralVerification": &response.structural_verification,
    })
}

fn ecky_ast_edit_operation_name(operation: &EckyAstEditOperation) -> &'static str {
    match operation {
        EckyAstEditOperation::Replace => "replace",
        EckyAstEditOperation::InsertBefore => "insertBefore",
        EckyAstEditOperation::InsertAfter => "insertAfter",
        EckyAstEditOperation::Delete => "delete",
        EckyAstEditOperation::Rename => "rename",
    }
}

fn ecky_literal_from_json(value: &Value) -> AppResult<String> {
    match value {
        Value::String(text) => {
            serde_json::to_string(text).map_err(|e| AppError::validation(e.to_string()))
        }
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(if *flag { "#t" } else { "#f" }.to_string()),
        _ => Err(AppError::validation(
            "set_select value must be string, number, or boolean.",
        )),
    }
}

fn legacy_stable_node_key_path_hint(stable_node_key: &str) -> Option<String> {
    let trimmed = stable_node_key.trim();
    if trimmed.starts_with('/') {
        return Some(trimmed.to_string());
    }

    for marker in ["path=", "nodePath=", "spanPath="] {
        let Some(start) = trimmed.find(marker) else {
            continue;
        };
        let tail = &trimmed[start + marker.len()..];
        let candidate = tail
            .split(['|', ';', ',', ' '])
            .next()
            .map(str::trim)
            .unwrap_or_default();
        if candidate.starts_with('/') {
            return Some(candidate.to_string());
        }
    }

    None
}

fn compact_ecky_ast_replace_and_render_response_value(
    response: &MacroReplaceResponse,
    operation: &str,
    edited_path: &str,
) -> Value {
    json!({
        "threadId": &response.thread_id,
        "messageId": &response.message_id,
        "modelId": &response.artifact_digest.model_id,
        "newSourceDigest": crate::mcp::macro_buffer::source_digest(&response.macro_code),
        "editedPath": edited_path,
        "operation": operation,
        "lineCount": source_line_count(&response.macro_code),
        "artifactDigest": &response.artifact_digest,
        "structuralVerification": &response.structural_verification,
    })
}

fn compact_params_patch_response_value(response: &ParamsPatchResponse) -> Value {
    json!({
        "threadId": &response.thread_id,
        "messageId": &response.message_id,
        "modelId": &response.artifact_digest.model_id,
        "mergedParamCount": response.merged_params.len(),
        "artifactDigest": &response.artifact_digest,
        "structuralVerification": &response.structural_verification,
    })
}

fn compact_macro_buffer_replace_and_preview_response_value(
    response: &MacroBufferReplaceAndRenderResponse,
) -> Value {
    json!({
        "threadId": &response.thread_id,
        "messageId": &response.message_id,
        "modelId": &response.artifact_digest.model_id,
        "digest": &response.digest,
        "lineCount": response.line_count,
        "artifactDigest": &response.artifact_digest,
        "structuralVerification": &response.structural_verification,
    })
}

fn thread_list_entry(
    conn: &rusqlite::Connection,
    thread: crate::models::Thread,
) -> Result<ThreadListEntry, AppError> {
    let latest_pending_message_id = db::get_latest_pending_user_message_id(conn, &thread.id)
        .map_err(|e| AppError::persistence(e.to_string()))?;
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
            if !model.is_empty() {
                let provider_lower = provider.to_ascii_lowercase();
                let model_lower = model.to_ascii_lowercase();
                if provider.is_empty()
                    || model_lower == provider_lower
                    || model_lower.starts_with(&format!("{}-", provider_lower))
                    || model_lower.starts_with(&format!("{}/", provider_lower))
                {
                    model.to_string()
                } else {
                    format!("{} ({})", model, provider)
                }
            } else if !provider.is_empty() {
                provider.to_string()
            } else if !engine.name.trim().is_empty() {
                engine.name.clone()
            } else {
                "default engine".to_string()
            }
        }
        None => "default engine".to_string(),
    }
}

fn workspace_source_hints(
    source_language: crate::models::SourceLanguage,
    geometry_backend: Option<crate::models::GeometryBackend>,
) -> (&'static str, &'static str) {
    match source_language {
        crate::models::SourceLanguage::EckyIrV0 => (".ecky", "ecky"),
        crate::models::SourceLanguage::Build123d => (".py", "build123d"),
        crate::models::SourceLanguage::LegacyPython => match geometry_backend {
            Some(crate::models::GeometryBackend::Freecad) => (".FCMacro", "freecad"),
            _ => (".py", "freecad"),
        },
    }
}

fn backend_hint(geometry_backend: Option<crate::models::GeometryBackend>) -> &'static str {
    match geometry_backend {
        Some(crate::models::GeometryBackend::Build123d) => "build123d",
        Some(crate::models::GeometryBackend::Freecad) => "freecad",
        Some(crate::models::GeometryBackend::EckyRust) => "mesh",
        None => "default",
    }
}

fn backend_guide_uri(
    geometry_backend: Option<crate::models::GeometryBackend>,
) -> Option<&'static str> {
    match geometry_backend {
        Some(crate::models::GeometryBackend::Build123d) => Some("ecky://guides/build123d"),
        Some(crate::models::GeometryBackend::Freecad) => Some("ecky://guides/freecad"),
        Some(crate::models::GeometryBackend::EckyRust) => Some("ecky://guides/ecky-rust"),
        None => None,
    }
}

fn surface_manifest_uri(
    geometry_backend: Option<crate::models::GeometryBackend>,
) -> Option<&'static str> {
    match geometry_backend {
        Some(crate::models::GeometryBackend::Build123d) => {
            Some("ecky://guides/surface-manifest/build123d")
        }
        Some(crate::models::GeometryBackend::Freecad) => {
            Some("ecky://guides/surface-manifest/freecad")
        }
        Some(crate::models::GeometryBackend::EckyRust) => {
            Some("ecky://guides/surface-manifest/ecky-rust")
        }
        None => None,
    }
}

fn surface_reference_uri(
    geometry_backend: Option<crate::models::GeometryBackend>,
) -> Option<&'static str> {
    match geometry_backend {
        Some(crate::models::GeometryBackend::Build123d) => {
            Some("ecky://guides/surface-reference/build123d")
        }
        Some(crate::models::GeometryBackend::Freecad) => {
            Some("ecky://guides/surface-reference/freecad")
        }
        Some(crate::models::GeometryBackend::EckyRust) => {
            Some("ecky://guides/surface-reference/ecky-rust")
        }
        None => None,
    }
}

fn workflow_guide_text(state: &AppState) -> String {
    format!(
        concat!(
            "Ecky MCP guide\n\n",
            "Purpose:\n",
            "- One public authored language: `.ecky`.\n",
            "- Backend metadata decides how `.ecky` renders: `build123d`, `freecad`, or `mesh`/`eckyRust`.\n",
            "- EckyRust direction is a controlled CAD runtime pipeline: parse -> expand -> typecheck -> lower -> validate. Direct OCCT is internal-only today: a STEP/STL fast path for supported Core IR, not a source/backend setting.\n",
            "- Never promise STEP unless artifact truth proves it: call `artifact_manifest_get` or `target_detail_get(section=\"artifactBundle\")` first and require `hasStepExport=true`, or confirm `exportArtifacts` contains `format=step`.\n",
            "- Use `artifact_manifest_get` for full machine-readable artifactBundle/modelManifest JSON. Use `target_detail_get(section=\"exportArtifacts\")` for the STEP path/detail; artifactBundle digest exposes `geometryBackend`, `edgeTargetCount`, `faceTargetCount`, `exportFormats`, `hasStepExport`, and `stepExportPath` for fast routing.\n",
            "- Use the current selected engine prompt as the design-policy baseline.\n\n",
            "Current engine:\n",
            "- {}\n\n",
            "{}\n",
            "Guide access:\n",
            "- Ecky guides are MCP resources. Use `resources/list` and `resources/read`.\n",
            "- `workspace_overview.agentBrief.primaryGuideUri` is the one normal must-read guide. Do not read all backend guides up front.\n",
            "- For `sourceLanguage=ecky`, write `.ecky`. The backend is a lowerer, not a different source language.\n",
            "- Read `compatibilityManifestUri` only when a concrete `.ecky` op/support question is uncertain.\n",
            "- Read prose backend guides only after a lowerer/render error or when making artifact/export claims.\n",
            "- Surface manifests: `ecky://guides/surface-manifest/build123d`, `ecky://guides/surface-manifest/freecad`, `ecky://guides/surface-manifest/ecky-rust`.\n\n",
            "- Surface references: `ecky://guides/surface-reference/build123d`, `ecky://guides/surface-reference/freecad`, `ecky://guides/surface-reference/ecky-rust`.\n\n",
            "Modeling rules:\n",
            "- Units are millimeters.\n",
            "- Prefer manifold printable solids with practical wall thickness and clearances.\n",
            "- For generated models, keep macroCode, uiSpec, and initialParams aligned.\n",
            "- Remove stale parameters that are no longer used.\n",
            "- Preserve the current thread/version intent unless explicitly asked to fork or restore.\n",
            "- In current `.ecky` source, authored sketch coordinates are literal. `extrude` preserves sketch X/Y and grows along +Z unless `:symmetric #t` is set.\n",
            "- Current `.ecky` compiler treats `let` bindings as parallel. Same-frame bindings cannot depend on earlier siblings; use `let*` or nested `let` for sequential dependencies.\n",
            "- `box`, `cylinder`, `cone`, and `sphere` accept `:align '(x y z)` using `min`, `center`, `max`.\n",
            "- Use `plane`, `location`, and `place` for local coordinates instead of compensating global translations.\n",
            "- Sample: `(extrude (polygon ((0 0) (100 0) (100 20) (0 20))) 8)` stays at `X=0..100`, `Y=0..20`.\n",
            "- Sample: `(box 40 20 10 :align '(min center min))` anchors `X=0`, centers `Y`, sits on `Z=0`.\n",
            "- Sample: `(place (location (plane :origin '(80 0 6)) :rotate '(0 90 0)) (cylinder 4 18))` uses local coordinates, not compensation math.\n",
            "- `ecky://guides/ecky-source` teaches the `.ecky` language. Backend guides explain lowerer-specific errors and export/artifact behavior; they are not normal startup reads.\n",
            "- JSON surface manifests are authoritative for supported forms, helpers, CAD ops, and wall-pattern modes. Use them on demand for concrete ops.\n",
            "- Reuse existing semantic views before inventing new control groupings.\n",
            "- Stay in the app loop. Use `mcp_request_user_prompt` for human replies.\n",
            "- Prefer typed/static errors and structural verification first; screenshot verification second.\n",
            "- Check the structuralVerification section when using target_get to ensure the generated model passed basic manifold and bounding box checks.\n",
            "- Use get_model_screenshot to visually verify geometric edits after structural checks.\n\n",
            "Recommended startup sequence:\n",
            "1. Call workspace_overview. It resolves sourceLanguage, geometryBackend, primaryGuideUri, and compatibilityManifestUri.\n",
            "2. Read only `agentBrief.primaryGuideUri` / `agentBrief.mustRead` for normal authoring.\n",
            "3. Read `agentBrief.compatibilityManifestUri` only when checking whether a concrete `.ecky` form/op is supported by the resolved backend. Read prose backend guides only after lowerer/render errors or artifact/export claims.\n",
            "4. Call workspace_overview, then target_meta_get. If choosing an existing thread, call thread_borrow; if this is a brand-new design with no target, call thread_create first.\n",
            "5. Use target_macro_get for macro reasoning, macro_buffer_get for line-numbered source edits, artifact_manifest_get for full artifact JSON, and target_detail_get(section=...) for exact chunks.\n",
            "6. Use target_get only when you truly need the full payload.\n",
            "7. If semantic bindings matter, call semantic_manifest_get before changing views or annotations.\n",
            "8. Then mutate with params_preview_render, macro_buffer_replace_and_preview, macro_preview_render, or semantic tools; prefer buffer replacement for non-trivial edits and use macro_preview_render for the first version after thread_create.\n",
            "9. Commit successful preview drafts with commit_preview_version; capture returned threadId/messageId/modelId in output evidence.\n",
            "10. Never update history.sqlite directly. State mutations must go through MCP tools.\n",
            "11. Use measurement_annotation tools for dimension meaning, and long_action_notice/long_action_clear for slow work.\n"
        ),
        selected_engine_label(state),
        authoring_card_text()
    )
}

fn workspace_overview_brief(
    state: &AppState,
    source_language: Option<crate::models::SourceLanguage>,
    geometry_backend: Option<crate::models::GeometryBackend>,
) -> WorkspaceOverviewBrief {
    let resolved_lang =
        source_language.unwrap_or_else(|| state.config.lock().unwrap().default_source_language);
    let (lang_str, dialect_str) = match resolved_lang {
        crate::models::SourceLanguage::EckyIrV0 => ("ecky".to_string(), "ecky".to_string()),
        crate::models::SourceLanguage::Build123d => {
            ("build123d".to_string(), "build123d".to_string())
        }
        crate::models::SourceLanguage::LegacyPython => {
            ("freecad".to_string(), "cadFrameworkV1".to_string())
        }
    };
    let (file_extension, source_hint) = workspace_source_hints(resolved_lang, geometry_backend);
    let backend = backend_hint(geometry_backend);
    let primary_guide_uri = match resolved_lang {
        crate::models::SourceLanguage::EckyIrV0 => "ecky://guides/ecky-source",
        crate::models::SourceLanguage::Build123d => "ecky://guides/build123d",
        crate::models::SourceLanguage::LegacyPython => "ecky://guides/freecad",
    }
    .to_string();
    let compatibility_manifest_uri = if resolved_lang == crate::models::SourceLanguage::EckyIrV0 {
        surface_manifest_uri(geometry_backend).map(str::to_string)
    } else {
        None
    };
    let backend_guide_uri = backend_guide_uri(geometry_backend).map(str::to_string);
    let mut read_when_needed = Vec::new();
    if let Some(uri) = &compatibility_manifest_uri {
        read_when_needed.push(uri.clone());
    }
    if let Some(uri) = &backend_guide_uri {
        read_when_needed.push(uri.clone());
    }
    if let Some(uri) = surface_reference_uri(geometry_backend).map(str::to_string) {
        read_when_needed.push(uri);
    }
    WorkspaceOverviewBrief {
        engine_label: selected_engine_label(state),
        source_language: lang_str,
        macro_dialect: dialect_str,
        geometry_backend: backend.to_string(),
        primary_guide_uri: primary_guide_uri.clone(),
        compatibility_manifest_uri: compatibility_manifest_uri.clone(),
        must_read: vec![primary_guide_uri],
        read_when_needed,
        summary: format!(
            "Current authoring surface: {} source. fileExtension={}. geometryBackend={}. Read the primary guide only for normal authoring; use the compatibility manifest on demand for concrete backend op support.",
            match resolved_lang {
                crate::models::SourceLanguage::EckyIrV0 => "ecky",
                crate::models::SourceLanguage::Build123d => "build123d",
                crate::models::SourceLanguage::LegacyPython => "freecad",
            },
            file_extension,
            backend,
        ),
        rules: vec![
            "Units: millimeters. Keep macroCode, uiSpec, and initialParams aligned; remove stale params.".to_string(),
            format!(
                "Canonical source uses fileExtension={}. geometryBackend={} is authoritative for lowering; compatibility manifests are on-demand support tables, not mandatory startup reading.",
                file_extension, backend
            ),
            "`.ecky` is the only public Ecky source extension. build123d/freecad are backend targets, not separate Ecky languages.".to_string(),
            "Preserve current thread/version intent unless explicitly asked to fork or restore.".to_string(),
            "Reuse semantic views when they already group the right controls.".to_string(),
            "For geometry edits, check typed/static errors and structuralVerification first; use get_model_screenshot second.".to_string(),
        ],
        resources: vec![
            "ecky://guides/authoring-card".to_string(),
            "ecky://guides/technical-system-prompt".to_string(),
            "ecky://guides/modeling-guidelines".to_string(),
            "ecky://guides/ecky-source".to_string(),
            "ecky://guides/freecad".to_string(),
            "ecky://guides/build123d".to_string(),
            "ecky://guides/ecky-rust".to_string(),
            "ecky://guides/surface-manifest/build123d".to_string(),
            "ecky://guides/surface-manifest/freecad".to_string(),
            "ecky://guides/surface-manifest/ecky-rust".to_string(),
            "ecky://guides/surface-reference/build123d".to_string(),
            "ecky://guides/surface-reference/freecad".to_string(),
            "ecky://guides/surface-reference/ecky-rust".to_string(),
        ],
        next_steps: vec![
            format!(
                "Read only `agentBrief.primaryGuideUri` / `agentBrief.mustRead` for normal authoring. For this target: sourceLanguage={} geometryBackend={}.",
                source_hint, backend
            ),
            "Call target_meta_get first for target summary.".to_string(),
            "Read `agentBrief.compatibilityManifestUri` only when checking a specific `.ecky` op/helper against the resolved backend. Read prose backend guides only after lowerer/render errors or artifact/export claims.".to_string(),
            "Use target_macro_get for reasoning, macro_buffer_get for digest-checked line edits, artifact_manifest_get for full artifact JSON, and target_detail_get(section=...) for exact chunks. For STEP claims, call artifact_manifest_get or target_detail_get(section=\"artifactBundle\") first; only promise STEP when hasStepExport=true or exportArtifacts contains format=step. Use target_detail_get(section=\"exportArtifacts\") for path/detail. Keep target_get as fallback.".to_string(),
            "Use mcp_request_user_prompt for human replies and long_action_notice for slow work.".to_string(),
        ],
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
            "Use agentBrief config/session defaults plus queued user requests to create the first version.".to_string(),
        ],
    }
}

fn resource_definitions() -> Vec<Value> {
    vec![
        json!({
            "uri": "ecky://guides/authoring-card",
            "name": "Ecky Authoring Card",
            "description": "Short immediate rules for writing or editing source safely before deeper guide reads.",
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
            "description": "Core modeling, printability, and workflow guidance for Ecky agents.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/ecky-source",
            "name": "Ecky Source (.ecky)",
            "description": "Canonical `.ecky` language guide. Backend metadata picks build123d, freecad, or mesh/eckyRust lowering.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/freecad",
            "name": "Ecky on FreeCAD",
            "description": "Backend guide for `.ecky` source when geometryBackend=freecad.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/build123d",
            "name": "Ecky on build123d",
            "description": "Backend guide for `.ecky` source when geometryBackend=build123d.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/ecky-rust",
            "name": "Ecky on mesh/eckyRust",
            "description": "Backend guide for `.ecky` source when geometryBackend=mesh/eckyRust.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/surface-manifest/build123d",
            "name": "Ecky build123d Supported Surface Manifest",
            "description": "Machine-readable `.ecky` supported authoring surface for geometryBackend=build123d.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "ecky://guides/surface-manifest/freecad",
            "name": "Ecky FreeCAD Supported Surface Manifest",
            "description": "Machine-readable `.ecky` supported authoring surface for geometryBackend=freecad.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "ecky://guides/surface-manifest/ecky-rust",
            "name": "EckyRust Supported Surface Manifest",
            "description": "Machine-readable `.ecky` supported authoring surface for geometryBackend=mesh/eckyRust.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "ecky://guides/surface-reference/build123d",
            "name": "Ecky build123d Surface Reference",
            "description": "Machine-readable `.ecky` signatures, descriptions, examples, determinism, and backend support for geometryBackend=build123d.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "ecky://guides/surface-reference/freecad",
            "name": "Ecky FreeCAD Surface Reference",
            "description": "Machine-readable `.ecky` signatures, descriptions, examples, determinism, and backend support for geometryBackend=freecad.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "ecky://guides/surface-reference/ecky-rust",
            "name": "EckyRust Surface Reference",
            "description": "Machine-readable `.ecky` signatures, descriptions, examples, determinism, and backend support for geometryBackend=mesh/eckyRust.",
            "mimeType": "application/json"
        }),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResourceContent {
    mime_type: &'static str,
    text: String,
}

fn surface_manifest_backend_for_uri(uri: &str) -> Option<crate::models::GeometryBackend> {
    match uri {
        "ecky://guides/surface-manifest/build123d" => {
            Some(crate::models::GeometryBackend::Build123d)
        }
        "ecky://guides/surface-manifest/freecad" => Some(crate::models::GeometryBackend::Freecad),
        "ecky://guides/surface-manifest/ecky-rust" => {
            Some(crate::models::GeometryBackend::EckyRust)
        }
        _ => None,
    }
}

fn surface_reference_backend_for_uri(uri: &str) -> Option<crate::models::GeometryBackend> {
    match uri {
        "ecky://guides/surface-reference/build123d" => {
            Some(crate::models::GeometryBackend::Build123d)
        }
        "ecky://guides/surface-reference/freecad" => Some(crate::models::GeometryBackend::Freecad),
        "ecky://guides/surface-reference/ecky-rust" => {
            Some(crate::models::GeometryBackend::EckyRust)
        }
        _ => None,
    }
}

fn surface_manifest_json(backend: crate::models::GeometryBackend) -> Value {
    let manifest = crate::ecky_language_surface::supported_surface_manifest(backend);
    json!({
        "backend": manifest.backend,
        "referenceUri": surface_reference_uri_for_backend(backend),
        "modelClauses": manifest.model_clauses,
        "modelWrappers": manifest.model_wrappers,
        "expressionForms": manifest.expression_forms,
        "numericHelpers": manifest.numeric_helpers,
        "pointListHelpers": manifest.point_list_helpers,
        "booleanHelpers": manifest.boolean_helpers,
        "cadOps": manifest.cad_ops,
        "wallPatternModes": manifest.wall_pattern_modes,
        "typedHolePolicy": manifest.typed_hole_policy,
    })
}

fn surface_reference_json(backend: crate::models::GeometryBackend) -> Value {
    serde_json::to_value(crate::ecky_language_surface::supported_surface_reference(
        backend,
    ))
    .unwrap_or_else(|_| json!({ "backend": backend, "entries": [] }))
}

fn surface_reference_uri_for_backend(backend: crate::models::GeometryBackend) -> &'static str {
    match backend {
        crate::models::GeometryBackend::Build123d => "ecky://guides/surface-reference/build123d",
        crate::models::GeometryBackend::Freecad => "ecky://guides/surface-reference/freecad",
        crate::models::GeometryBackend::EckyRust => "ecky://guides/surface-reference/ecky-rust",
    }
}

fn read_resource_text(state: &AppState, uri: &str) -> Option<String> {
    match uri {
        "ecky://guides/authoring-card" => Some(authoring_card_text().to_string()),
        "ecky://guides/technical-system-prompt" => Some(crate::TECHNICAL_SYSTEM_PROMPT.to_string()),
        "ecky://guides/modeling-guidelines" => Some(workflow_guide_text(state)),
        "ecky://guides/ecky-source" | "ecky://guides/ecky-ir-v0" => {
            Some(crate::commands::generation::ecky_source_guide_text())
        }
        "ecky://guides/freecad" | "ecky://guides/cad-sdk" => {
            Some(crate::commands::generation::freecad_guide_text())
        }
        "ecky://guides/build123d" => Some(crate::commands::generation::build123d_guide_text()),
        "ecky://guides/ecky-rust" | "ecky://guides/mesh" => {
            Some(crate::commands::generation::ecky_ir_v0_guide_text(
                crate::models::GeometryBackend::EckyRust,
            ))
        }
        _ => None,
    }
}

fn read_resource_content(state: &AppState, uri: &str) -> Option<ResourceContent> {
    if let Some(backend) = surface_manifest_backend_for_uri(uri) {
        return Some(ResourceContent {
            mime_type: "application/json",
            text: serde_json::to_string_pretty(&surface_manifest_json(backend)).unwrap(),
        });
    }

    if let Some(backend) = surface_reference_backend_for_uri(uri) {
        return Some(ResourceContent {
            mime_type: "application/json",
            text: serde_json::to_string_pretty(&surface_reference_json(backend)).unwrap(),
        });
    }

    read_resource_text(state, uri).map(|text| ResourceContent {
        mime_type: "text/plain",
        text,
    })
}

fn canonical_mcp_resource_uri(uri: &str) -> &str {
    match uri {
        "ecky://guides/ecky-ir-v0" => "ecky://guides/ecky-source",
        "ecky://guides/cad-sdk" => "ecky://guides/freecad",
        "ecky://guides/mesh" => "ecky://guides/ecky-rust",
        other => other,
    }
}

async fn mark_session_resource_read(state: &AppState, session_id: &str, uri: &str) {
    let uri = canonical_mcp_resource_uri(uri).to_string();
    let mut reads = state.mcp_session_read_resources.lock().await;
    reads.entry(session_id.to_string()).or_default().insert(uri);
}

fn required_authoring_guide_uris(
    source_language: crate::models::SourceLanguage,
    _geometry_backend: crate::models::GeometryBackend,
) -> Vec<&'static str> {
    match source_language {
        crate::models::SourceLanguage::EckyIrV0 => vec!["ecky://guides/ecky-source"],
        crate::models::SourceLanguage::Build123d | crate::models::SourceLanguage::LegacyPython => {
            Vec::new()
        }
    }
}

async fn missing_authoring_guide_uris(
    state: &AppState,
    session_id: &str,
    source_language: crate::models::SourceLanguage,
    geometry_backend: crate::models::GeometryBackend,
) -> Vec<&'static str> {
    let required = required_authoring_guide_uris(source_language, geometry_backend);
    if required.is_empty() {
        return required;
    }

    let reads = state.mcp_session_read_resources.lock().await;
    let Some(read_uris) = reads.get(session_id) else {
        return required;
    };

    required
        .into_iter()
        .filter(|uri| !read_uris.contains(*uri))
        .collect()
}

async fn session_bypasses_resource_read_guard(state: &AppState, session_id: &str) -> bool {
    let sessions = state.mcp_sessions.lock().await;
    let Some(session) = sessions.get(session_id) else {
        return false;
    };
    session.client_kind.ends_with("mcp-http")
}

async fn ensure_authoring_guides_read(
    state: &AppState,
    session_id: &str,
    source_language: crate::models::SourceLanguage,
    geometry_backend: crate::models::GeometryBackend,
    tool_name: &str,
) -> AppResult<()> {
    if session_bypasses_resource_read_guard(state, session_id).await {
        return Ok(());
    }
    let missing =
        missing_authoring_guide_uris(state, session_id, source_language, geometry_backend).await;
    if missing.is_empty() {
        return Ok(());
    }

    Err(AppError::validation(format!(
        "Read required MCP resources before calling {tool_name} for sourceLanguage={} geometryBackend={}: {}. Use resources/read for each URI, then retry.",
        source_language.as_str(),
        geometry_backend.as_str(),
        missing.join(", ")
    )))
}

async fn ensure_target_authoring_guides_read(
    state: &AppState,
    session_id: &str,
    target: &ResolvedTargetRef,
    tool_name: &str,
) -> AppResult<()> {
    ensure_authoring_guides_read(
        state,
        session_id,
        target.source_language,
        target.geometry_backend,
        tool_name,
    )
    .await
}

fn effective_existing_authoring_context(
    source_language: crate::models::SourceLanguage,
    geometry_backend: crate::models::GeometryBackend,
    requested_geometry_backend: Option<crate::models::GeometryBackend>,
) -> (
    crate::models::SourceLanguage,
    crate::models::GeometryBackend,
) {
    let geometry_backend = if source_language == crate::models::SourceLanguage::EckyIrV0 {
        requested_geometry_backend.unwrap_or(geometry_backend)
    } else {
        geometry_backend
    };
    (source_language, geometry_backend)
}

fn first_version_macro_request_authoring_context(
    config: &Config,
    req: &MacroReplaceRequest,
) -> (
    crate::models::SourceLanguage,
    crate::models::GeometryBackend,
) {
    let dialect = req
        .macro_dialect
        .clone()
        .unwrap_or_else(|| crate::models::infer_macro_dialect_from_code(&req.macro_code));
    match dialect {
        crate::models::MacroDialect::Legacy | crate::models::MacroDialect::CadFrameworkV1 => (
            crate::models::SourceLanguage::LegacyPython,
            crate::models::GeometryBackend::Freecad,
        ),
        crate::models::MacroDialect::Build123d => (
            crate::models::SourceLanguage::Build123d,
            crate::models::GeometryBackend::Build123d,
        ),
        crate::models::MacroDialect::EckyIrV0 => (
            crate::models::SourceLanguage::EckyIrV0,
            req.geometry_backend
                .unwrap_or(config.default_geometry_backend),
        ),
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
            let workflow = workflow_guide_text(state);
            Some(json!({
                "description": "Bootstrap prompt for Ecky agents connecting to MCP.",
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "{}\n\nAfter reading this, call `workspace_overview` before editing anything. Use sourceLanguage and geometryBackend from that response to choose the matching guide.",
                                workflow
                            )
                        }
                    }
                ]
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
fn tool_definitions() -> Vec<Value> {
    tool_definitions_with_ast_enabled(false)
}

fn tool_definitions_with_ast_enabled(ecky_ast_authoring: bool) -> Vec<Value> {
    let mut tools = vec![
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
            "name": "freecad_library_search",
            "description": "Search configured local FreeCAD-library folders for reusable FCStd/STEP/STL parts. Architecture folders are excluded unless includeArchitecture is true.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "roots": { "type": "array", "items": { "type": "string" }, "description": "Optional override roots. Omit to use config.freecadLibraryRoots." },
                    "limit": { "type": "number" },
                    "includeArchitecture": { "type": "boolean" }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "freecad_library_import",
            "description": "Import one FreeCAD-library search result into an Ecky thread. Materializes runtime artifacts, creates a visible imported model version, and returns threadId/messageId plus artifactBundle/modelManifest.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "item": { "type": "object", "description": "A FreecadLibraryItem returned by freecad_library_search." },
                    "threadId": { "type": "string", "description": "Optional existing thread to add the imported version to. Defaults to this MCP session target, or creates a new thread." },
                    "title": { "type": "string", "description": "Optional title for a new imported thread. Existing thread titles are preserved." }
                },
                "required": ["item"]
            }
        }),
        json!({
            "name": "session_log_in",
            "description": "Notify the workspace that an agent has joined. threadId/messageId are optional: pass them only to claim an initial target; omit them for a targetless session. A session may later work on other threads by calling thread_borrow, passing explicit threadId/messageId to tools, or calling thread_create. If another live agent already owns an explicit thread target, the call fails unless stealThread is true.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agentLabel": { "type": "string" },
                    "threadId": { "type": "string", "description": "Optional thread target to claim initially." },
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
            "description": "Lightweight browsing of available work targets. Includes queued/pending counts, pendingConfirm, and latestPendingMessageId so agents can sweep inbox threads without loading full histories.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "thread_create",
            "description": concat!(
                "Create a new blank thread and borrow it as this MCP session's current target. ",
                "Use this for a new design before calling macro_preview_render. ",
                "Authoring language/backend belong to the model version or session config, not the thread."
            ),
            "inputSchema": with_identity(
                &[
                    ("title", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "thread_borrow",
            "description": concat!(
                "Borrow an existing thread as this MCP session's current target without logging out/in. ",
                "Use this after thread_list/thread_get when choosing or switching existing work. ",
                "Pass messageId to target a specific version; otherwise pass threadId for the latest/default target."
            ),
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string", "description": "Thread to borrow as the current target." })),
                    ("messageId", json!({ "type": "string", "description": "Optional version message target. If provided without threadId, Ecky derives the thread." })),
                    ("modelId", json!({ "type": "string", "description": "Optional model id for the target." })),
                    ("stealThread", json!({ "type": "boolean", "description": "Explicitly take over a thread currently claimed by another live agent session." }))
                ],
                &[],
            )
        }),
        json!({
            "name": "thread_meta_get",
            "description": "Fetch thread metadata without messages. Includes pendingConfirm and latestPendingMessageId for inbox/claim workflows.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "threadId": { "type": "string" }
                },
                "required": ["threadId"]
            }
        }),
        json!({
            "name": "thread_messages_get",
            "description": "Fetch a slice of compact messages from a thread.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "threadId": { "type": "string" },
                    "limit": { "type": "number" },
                    "before": { "type": "string" },
                    "roles": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["threadId"]
            }
        }),
        json!({
            "name": "thread_get",
            "description": "Fetch a full thread with versions and runtime metadata. Expensive; prefer thread_meta_get/thread_messages_get.",
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
            "name": "ui_dispatch",
            "description": "Trigger a UI action in the Ecky frontend to show the user what you are doing (e.g. open the parameters window, highlight a specific slider).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["openWindow", "closeWindow", "highlightParam"],
                        "description": "The UI action to perform."
                    },
                    "target": {
                        "type": "string",
                        "description": "The target of the action (e.g., 'params', 'projects', or a specific parameter key)."
                    },
                    "value": {
                        "type": "object",
                        "description": "Optional value to show or preview."
                    }
                },
                "required": ["action", "target"]
            }
        }),
        json!({
            "name": "target_meta_get",
            "description": "Fetch a lightweight summary of the current editable target. Preferred default read step after workspace_overview. Includes scenePacket plus artifact routing flags hasArtifactBundle, hasRuntimeManifest, edgeTargetCount, faceTargetCount, exportFormats, hasStepExport, and stepExportPath; call artifact_manifest_get for full JSON.",
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
            "description": "Fetch active editable source metadata plus a 1-based line window, authoringContext, and artifactDigest. Pass startLine/endLine for a specific range. Prefer macro_buffer_get for edits.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("startLine", json!({ "type": "integer", "minimum": 1 })),
                    ("endLine", json!({ "type": "integer", "minimum": 1 }))
                ],
                &[],
            )
        }),
        json!({
            "name": "macro_buffer_get",
            "description": "Open the active target source into this MCP session's editable source buffer. Returns digest, artifactDigest, lineCount, and a 1-based line window only. Pass startLine/endLine for a specific range. Use before macro_buffer_replace_range, macro_buffer_apply_patch, or macro_buffer_preview_render.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("startLine", json!({ "type": "integer", "minimum": 1 })),
                    ("endLine", json!({ "type": "integer", "minimum": 1 }))
                ],
                &[],
            )
        }),
        json!({
            "name": "macro_buffer_replace_range",
            "description": "Edit this session's macro buffer by replacing one or more 1-based inclusive line ranges. Requires expectedDigest from macro_buffer_get or prior buffer edit.",
            "inputSchema": with_identity(
                &[
                    ("expectedDigest", json!({ "type": "string" })),
                    ("replacements", json!({
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["startLine", "endLine", "newText"],
                            "properties": {
                                "startLine": { "type": "number" },
                                "endLine": { "type": "number" },
                                "newText": { "type": "string" }
                            }
                        }
                    }))
                ],
                &["expectedDigest", "replacements"],
            )
        }),
        json!({
            "name": "macro_buffer_apply_patch",
            "description": "Apply a simple unified diff patch to this session's macro buffer. Requires expectedDigest from macro_buffer_get or prior buffer edit.",
            "inputSchema": with_identity(
                &[
                    ("expectedDigest", json!({ "type": "string" })),
                    ("patch", json!({ "type": "string" }))
                ],
                &["expectedDigest", "patch"],
            )
        }),
        json!({
            "name": "macro_buffer_preview_render",
            "description": "Validate/render this session's macro buffer through the existing macro_preview_render path. Preserves sourceLanguage, macroDialect, and geometryBackend captured by macro_buffer_get. Returns artifactDigest; check hasStepExport before promising STEP.",
            "inputSchema": with_identity(
                &[
                    ("expectedDigest", json!({ "type": "string" })),
                    ("uiSpec", json!({ "type": "object" })),
                    ("parameters", json!({ "type": "object" }))
                ],
                &["expectedDigest"],
            )
        }),
        json!({
            "name": "macro_buffer_replace_and_preview",
            "description": "Replace 1-based inclusive line ranges in this session's macro buffer, then validate/render a preview through macro_preview_render. Returns artifactDigest; check hasStepExport before promising STEP. Prefer separate edit then render for large changes.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("expectedDigest", json!({ "type": "string" })),
                    ("replacements", json!({
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["startLine", "endLine", "newText"],
                            "properties": {
                                "startLine": { "type": "number" },
                                "endLine": { "type": "number" },
                                "newText": { "type": "string" }
                            }
                        }
                    })),
                    ("uiSpec", json!({ "type": "object" })),
                    ("parameters", json!({ "type": "object" }))
                ],
                &["expectedDigest", "replacements"],
            )
        }),
        json!({
            "name": "target_detail_get",
            "description": "Fetch one exact chunk of the active editable target plus authoringContext by section. Use this instead of target_get when you only need uiSpec, params, artifact metadata, or compact shapeGraph slices. artifactBundle returns digest fields geometryBackend, edgeTargetCount, faceTargetCount, exportFormats, hasStepExport, and stepExportPath. shapeGraph returns compact parts/instances/constraints/debug/dependencies packets without full source text and includes sourceDigest/coreDigest for guarded follow-up patch flow. Do not promise STEP unless artifactBundle hasStepExport=true or exportArtifacts contains format=step. Use exportArtifacts for STEP path/detail.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("section", json!({
                        "type": "string",
                        "enum": ["uiSpec", "initialParams", "artifactBundle", "artifactPaths", "viewerAssets", "exportArtifacts", "latestDraft", "shapeGraph"]
                    })),
                    ("shapeGraphFilters", json!({
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["parts", "instances", "constraints", "debug", "dependencies"]
                        }
                    }))
                ],
                &["section"],
            )
        }),
        json!({
            "name": "artifact_manifest_get",
            "description": "Fetch the full machine-readable runtime artifact manifest for the active target/model. Returns artifactBundle, modelManifest, digest fields, and runtimeManifestValid after bundle/manifest validation. Use this before export promises or artifact-aware repair.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("modelId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "artifact_feature_graph_get",
            "description": "Read-only feature/correspondence graph query for the active target/model. Reads the runtime model manifest via model_runtime, so legacy manifests get v0 feature-graph backfill. Returns modelId, artifactDigest, featureGraph, and correspondenceGraph. Does not edit or render.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("modelId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "target_get",
            "description": "Fetch the full current editable target payload plus artifactDigest. Expensive; prefer target_meta_get, target_macro_get, macro_buffer_get, or target_detail_get unless you truly need everything. Do not promise STEP unless artifactDigest hasStepExport=true or artifactBundle exportArtifacts contains format=step.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "ecky_dependency_get",
            "description": "Read-only dependency graph query for sourceLanguage=ecky targets. Supported path shapes: /params/{key} and /targets/{targetId}. Param queries return Core source paths plus impact labels. Target queries return mapped featureIds, parameterKeys, targetIds, and source paths when feature/source bindings exist. Does not edit source or render.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("path", json!({
                        "type": "string",
                        "description": "Source path to inspect. Supported shapes: /params/{key}, /targets/{targetId}."
                    }))
                ],
                &["path"],
            )
        }),
        json!({
            "name": "ecky_selector_resolve",
            "description": "Resolve one selection target id/alias against active target model manifest. Returns durable/canonical ids, bound featureIds/parameterKeys, confidence (exact|inferred|ambiguous|none), plus provenanceCandidates (featureRole, sourceStableNodeKeys, operationKinds, primitiveIds) as best-effort hints. Does not edit source or render.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("targetId", json!({
                        "type": "string",
                        "description": "Selection target id or alias to resolve."
                    }))
                ],
                &["targetId"],
            )
        }),
        json!({
            "name": "ecky_constraints_validate",
            "description": "Read-only constraint validation for sourceLanguage=ecky targets. Compiles source and checks CoreParameter min/max/step/choices and params-level :relations (<, <=, >, >=) against provided parameters, or target initial/default parameters. Rows include status/message plus severity, involvedParamKeys, sourceStableNodeKeys, and relation/constraint metadata fields (constraintId, label, kind, sourceStableNodeKey, dependsOnParamKeys, affectsStableNodeKeys). Response also includes authoringLints for repeated anonymous geometry deltas like (+ param N) and (- param N) with suggested parameter names. Does not edit source or render.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("parameters", json!({
                        "type": "object",
                        "description": "Optional parameter values to validate. Omit to validate target initial parameters with Core defaults for missing keys."
                    }))
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
            "name": "concept_preview_save",
            "description": "Save a concept preview image produced by the connected MCP agent into the current bound thread. Ecky does not call any configured app model or provider for this tool.",
            "inputSchema": with_identity(
                &[
                    ("imageData", json!({ "type": "string", "description": "data:image URL generated by the MCP agent." })),
                    ("caption", json!({ "type": "string", "description": "Short note to show with the concept preview." })),
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" }))
                ],
                &["imageData"],
            )
        }),
        json!({
            "name": "params_preview_render",
            "description": "Patch a subset of parameters and rerender a draft. Works without prior browsing by resolving the default target automatically. Returns artifactDigest; check hasStepExport before promising STEP.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("parameterPatch", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d"],
                        "description": "Optional: Explicitly choose geometry backend for Ecky source. build123d is the stable OCCT target; freecad is the direct CAD target."
                    }))
                ],
                &["parameterPatch"],
            )
        }),
        json!({
            "name": "macro_preview_render",
            "description": concat!(
                "Replace macro code and rerender a draft. Returns artifactDigest; check hasStepExport before promising STEP. ",
                "IMPORTANT: check workspace_overview.agentBrief.summary and rules — if sourceLanguage is `ecky`, macroCode MUST be current `.ecky` source (starting with `(model ...)`). geometryBackend chooses build123d or freecad lowering; source extension does not. ",
                "Authoring uses pure lispy Ecky source compiled to internal Core IR or the selected backend. `define`, `lambda`, `let`, `let*`, `if`, and generic helpers like `range`, `map`, `filter`, `reduce`, `zip`, `enumerate`, `linspace`, and `flat-map` are allowed; `set!`, assignment, rebinding, and mutation are not. Current `let` bindings are parallel, so same-frame bindings cannot depend on earlier siblings; use `let*` or nested `let` for sequential dependencies. ",
                "When workspace_overview.agentBrief.summary reports sourceLanguage `ecky`, uiSpec and parameters are auto-derived from the params block. For existing targets, omit parameters: macro_preview_render preserves current target params. Use params_preview_render for numeric changes. parameters only seeds first versions. ",
                "uiSpec.fields is an array of control descriptors — each field MUST have: key (string), label (string), type (one of: range|number|select|checkbox|image). ",
                "For numeric parameters, prefer number; range only when explicitly needed. range/number: min, max, step (numbers). ",
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
                    ("parameters", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d"],
                        "description": "Optional: Explicitly choose geometry backend for Ecky source. build123d is the stable OCCT target; freecad is the direct CAD target."
                    }))
                ],
                &["macroCode"],
            )
        }),
        json!({
            "name": "semantic_manifest_get",
            "description": "Fetch a summary of the semantic manifest for the current generated-model target.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" }))
                ],
                &[],
            )
        }),
        json!({
            "name": "semantic_manifest_detail_get",
            "description": "Fetch one exact chunk of the semantic manifest by section.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("section", json!({
                        "type": "string",
                        "enum": ["controlPrimitives", "controlRelations", "controlViews", "advisories", "measurementAnnotations", "parts"]
                    }))
                ],
                &["section"],
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
            "name": "commit_preview_version",
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
            "name": "compare_models",
            "description": "Compare two STL models using build123d comparison engine. Returns volume and bounding box matching metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "refPath": { "type": "string", "description": "Path to reference STL file" },
                    "genPath": { "type": "string", "description": "Path to generated STL file" }
                },
                "required": ["refPath", "genPath"]
            }
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
            "description": "Request text input from the human in the Ecky UI for a specific thread. Blocks until the user submits or the timeout expires. Prefer thread_borrow/thread_create when choosing a target; pass threadId/messageId explicitly for one-off targeting. Otherwise Ecky uses the current session target from thread_borrow, thread_create, session_log_in, or a prior targeted prompt. Ecky will not guess from the current workspace view. If timeoutSecs is omitted, Ecky uses the configured MCP prompt timeout. The response includes promptText/attachments plus threadId/threadTitle for the target context. Image attachments may include inline dataUrl payloads; prefer those directly and avoid copying them into scratch folders. CAD attachments remain path-based. A timeout is normal when the user does not answer right away; poll again later or call session_log_out if you are leaving the workspace. In active MCP mode, call this again immediately after each completed user-facing turn so Ecky can queue the next message.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Optional context message shown to the user above the input (e.g. 'What would you like me to build?')." },
                    "requestId": { "type": "string", "description": "Optional stable ID for deduplication." },
                    "timeoutSecs": { "type": "number", "description": "Seconds to wait. If omitted, Ecky uses the configured MCP prompt timeout. Max 1800." },
                    "threadId": { "type": "string", "description": "Optional explicit thread target. Required if the session has no current target." },
                    "messageId": { "type": "string", "description": "Optional explicit version message target. If provided without threadId, Ecky derives the thread from the message." },
                    "modelId": { "type": "string", "description": "Optional model id for the explicit target." }
                }
            }
        }),
        json!({
            "name": "mark_as_read",
            "description": "Claim queued user thread messages after you inspect them. Pass latestPendingMessageId from thread_list/thread_meta_get, or any pending user message id from thread_get/thread_messages_get; Ecky will drain the whole pending batch for that thread into the current turn.",
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
        json!({
            "name": "verify_generated_model",
            "description": "Run deterministic structural verification on the generated model for the currently bound target/thread. Returns artifactDigest plus the full structured result including pass/fail, issue codes, metrics, and verifier source. This is the authoritative first check — screenshot/VLM verification is secondary.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("modelId", json!({ "type": "string" })),
                    ("originalPrompt", json!({ "type": "string" })),
                ],
                &[],
            )
        }),
        json!({
            "name": "get_structural_verification_summary",
            "description": "Lightweight summary of the structural verification result for quick agent routing. Returns artifactDigest, pass/fail, summary text, issue count, and verifier status without full issue details.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("modelId", json!({ "type": "string" })),
                ],
                &[],
            )
        }),
        json!({
            "name": "printability_analyze",
            "description": "Read-only printability analysis for the active target/model preview STL. Resolves the current editable target, reads the artifact bundle preview STL path, and returns artifactDigest plus compact mesh/overhang/topology facts. Does not edit source or render.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("modelId", json!({ "type": "string" })),
                ],
                &[],
            )
        }),
        json!({
            "name": "printability_transform_recipes_get",
            "description": "Read-only supportless-FDM transform recipe slice for the active target/model preview STL. Returns artifactDigest-guarded candidate recipes with action kind, rationale, estimated effect, target/sourceAnchor when known, and preview/apply support status. Does not edit source or render.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("modelId", json!({ "type": "string" })),
                ],
                &[],
            )
        }),
        json!({
            "name": "semantic_transform_preview",
            "description": "Create a source-consistent preview draft for supportless-FDM semantic recipes. Narrow v1 supports actionKind=reorient for sourceLanguage=ecky .ecky sources only, validates expectedArtifact {modelId, previewStlPath, contentHash}, and rejects chamfer/split as unsupported.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("modelId", json!({ "type": "string" })),
                    ("recipeId", json!({ "type": "string" })),
                    ("actionKind", json!({ "type": "string", "enum": ["reorient", "chamfer", "split"] })),
                    ("expectedArtifact", json!({
                        "type": "object",
                        "properties": {
                            "modelId": { "type": "string" },
                            "previewStlPath": { "type": "string" },
                            "contentHash": { "type": "string" }
                        },
                        "required": ["modelId", "previewStlPath", "contentHash"]
                    })),
                ],
                &["recipeId", "actionKind", "expectedArtifact"],
            )
        }),
    ];

    if ecky_ast_authoring {
        tools.retain(|tool| {
            !matches!(
                tool.get("name").and_then(Value::as_str),
                Some(
                    "macro_buffer_get"
                        | "macro_buffer_replace_range"
                        | "macro_buffer_apply_patch"
                        | "macro_buffer_preview_render"
                        | "macro_buffer_replace_and_preview"
                )
            )
        });
        tools.push(json!({
            "name": "ecky_ast_get",
            "description": "Experimental AST authoring read for sourceLanguage=ecky. Returns bounded Core AST nodes with stable structural paths, subtree digests, value kinds, spans, authoringContext, and artifactDigest. Optional includeSource returns bounded exact source slices only for source-addressable .ecky nodes. `nodeId` is debug-only and may change across unrelated edits; use `stableNodeKey` as the public handle. Use instead of macro_buffer_get when mcp.eckyAstAuthoring=true.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("depth", json!({ "type": "integer", "minimum": 0, "maximum": 12 })),
                    ("maxNodes", json!({ "type": "integer", "minimum": 1, "maximum": 500 })),
                    ("includeSource", json!({
                        "type": "boolean",
                        "description": "When true, source-addressable nodes include source.span, source.text, source.truncated, source.maxBytes, and source.byteLen. Omitted/false keeps compact node output."
                    }))
                ],
                &[],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_inspect",
            "description": "Alias for ecky_ast_get. Inspect bounded AST with stable keys and source addressability. `nodeId` is debug-only; use `stableNodeKey` for public references.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("depth", json!({ "type": "integer", "minimum": 0, "maximum": 12 })),
                    ("maxNodes", json!({ "type": "integer", "minimum": 1, "maximum": 500 })),
                    ("includeSource", json!({
                        "type": "boolean",
                        "description": "When true, source-addressable nodes include source.span, source.text, source.truncated, source.maxBytes, and source.byteLen. Omitted/false keeps compact node output."
                    }))
                ],
                &[],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_get_node",
            "description": "Resolve one exact AST node by stableNodeKey (preferred) or path. Returns a single-node bounded AST payload and optional source slice.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("stableNodeKey", json!({ "type": "string", "description": "Preferred public handle from ecky_ast_get/ecky_ast_inspect." })),
                    ("path", json!({ "type": "string", "description": "Fallback explicit path when stableNodeKey is unavailable." })),
                    ("includeSource", json!({
                        "type": "boolean",
                        "description": "When true, source-addressable node includes source slice metadata."
                    }))
                ],
                &[],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_patch_validate",
            "description": "Experimental AST authoring validation for sourceLanguage=ecky. Validates one source-addressable Core AST patch with sourceDigest and expectedNodeDigest guards, resolving stableNodeKey to path when provided, compiles the patched source, and returns compact diff metadata plus best-effort affectedNodeKeys and dependencyImpact summary. Supports replace/insertBefore/insertAfter/delete/rename. Does not render, create a draft, or acquire a lease.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("operation", json!({
                        "type": "string",
                        "enum": ["replace", "insertBefore", "insertAfter", "delete", "rename"],
                        "description": "Default replace. insertBefore/insertAfter require replacementSource. delete ignores replacementSource. rename requires newName."
                    })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("stableNodeKey", json!({ "type": "string", "description": "Preferred public handle from ecky_ast_get/ecky_ast_inspect." })),
                    ("path", json!({ "type": "string", "description": "Fallback explicit path when stableNodeKey is unavailable." })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("replacementSource", json!({
                        "type": "string",
                        "description": "Required for replace/insert operations. Empty or whitespace-only input is rejected."
                    })),
                    ("newName", json!({
                        "type": "string",
                        "description": "Required for rename."
                    }))
                ],
                &["sourceDigest", "expectedNodeDigest"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_replace_and_render",
            "description": "Experimental AST authoring mutation for sourceLanguage=ecky. Edits one source-addressable Core AST node by stableNodeKey (preferred) or path with sourceDigest and expectedNodeDigest guards, then renders a draft. operation defaults to replace; insertBefore/insertAfter add a sibling around the path; delete removes an arg or keyword pair; rename updates supported binding declarations plus in-scope references. Returns artifactDigest and structuralVerification; check hasStepExport before promising STEP.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("operation", json!({
                        "type": "string",
                        "enum": ["replace", "insertBefore", "insertAfter", "delete", "rename"],
                        "description": "Default replace. insertBefore/insertAfter require replacementSource. delete ignores replacementSource. rename requires newName."
                    })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("stableNodeKey", json!({ "type": "string", "description": "Preferred public handle from ecky_ast_get/ecky_ast_inspect." })),
                    ("path", json!({ "type": "string", "description": "Fallback explicit path when stableNodeKey is unavailable." })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("replacementSource", json!({
                        "type": "string",
                        "description": "Required for replace/insert operations. For keyword insert, pass the full keyword pair such as `:edges \"top\"`."
                    })),
                    ("newName", json!({
                        "type": "string",
                        "description": "Required for rename."
                    })),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"],
                        "description": "Optional: Explicitly choose geometry backend for Ecky source."
                    }))
                ],
                &["sourceDigest", "expectedNodeDigest"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_patch_preview",
            "description": "Alias for ecky_ast_replace_and_render. Apply one guarded AST patch and render preview artifact without committing history.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("operation", json!({
                        "type": "string",
                        "enum": ["replace", "insertBefore", "insertAfter", "delete", "rename"],
                        "description": "Default replace. insertBefore/insertAfter require replacementSource. delete ignores replacementSource. rename requires newName."
                    })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("stableNodeKey", json!({ "type": "string", "description": "Preferred public handle from ecky_ast_get/ecky_ast_inspect." })),
                    ("path", json!({ "type": "string", "description": "Fallback explicit path when stableNodeKey is unavailable." })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("replacementSource", json!({
                        "type": "string",
                        "description": "Required for replace/insert operations. For keyword insert, pass the full keyword pair such as `:edges \"top\"`."
                    })),
                    ("newName", json!({
                        "type": "string",
                        "description": "Required for rename."
                    })),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"],
                        "description": "Optional: Explicitly choose geometry backend for Ecky source."
                    }))
                ],
                &["sourceDigest", "expectedNodeDigest"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_patch_commit",
            "description": "Alias for commit_preview_version. Commit the latest successful preview draft into thread history.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("title", json!({ "type": "string" })),
                    ("versionName", json!({ "type": "string" }))
                ],
                &[],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_set_number",
            "description": "Set one numeric literal at a source-addressable AST path, then render preview. Wrapper over ecky_ast_replace_and_render operation=replace.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("value", json!({ "type": "number" })),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"]
                    }))
                ],
                &["sourceDigest", "path", "expectedNodeDigest", "value"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_set_string",
            "description": "Set one string literal at a source-addressable AST path, then render preview. Wrapper over ecky_ast_replace_and_render operation=replace.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("value", json!({ "type": "string" })),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"]
                    }))
                ],
                &["sourceDigest", "path", "expectedNodeDigest", "value"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_set_select",
            "description": "Set one select literal (string/number/boolean) at a source-addressable AST path, then render preview. Wrapper over ecky_ast_replace_and_render operation=replace.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("value", json!({})),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"]
                    }))
                ],
                &["sourceDigest", "path", "expectedNodeDigest", "value"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_replace_call",
            "description": "Replace one call expression at a source-addressable AST path, then render preview. Wrapper over ecky_ast_replace_and_render operation=replace.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("replacementSource", json!({ "type": "string" })),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"]
                    }))
                ],
                &["sourceDigest", "path", "expectedNodeDigest", "replacementSource"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_insert_binding",
            "description": "Insert one binding near the addressed binding path, then render preview. position defaults to after. Wrapper over ecky_ast_replace_and_render operation=insertAfter/insertBefore.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("bindingSource", json!({ "type": "string" })),
                    ("position", json!({ "type": "string", "enum": ["before", "after"] })),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"]
                    }))
                ],
                &["sourceDigest", "path", "expectedNodeDigest", "bindingSource"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_delete_binding",
            "description": "Delete one binding at the addressed path, then render preview. Wrapper over ecky_ast_replace_and_render operation=delete.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"]
                    }))
                ],
                &["sourceDigest", "path", "expectedNodeDigest"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_rename_binding_scoped",
            "description": "Rename one binding and in-scope references, then render preview. Wrapper over ecky_ast_replace_and_render operation=rename.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("sourceDigest", json!({ "type": "string" })),
                    ("path", json!({ "type": "string" })),
                    ("expectedNodeDigest", json!({ "type": "string" })),
                    ("newName", json!({ "type": "string" })),
                    ("parameters", json!({ "type": "object" })),
                    ("postProcessing", json!({ "type": "object" })),
                    ("geometryBackend", json!({
                        "type": "string",
                        "enum": ["freecad", "build123d", "mesh", "eckyRust"]
                    }))
                ],
                &["sourceDigest", "path", "expectedNodeDigest", "newName"],
            )
        }));
    }

    tools
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
            handle: Some(handle),
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
            if let Some(handle) = server.handle.as_ref() {
                emit_sessions_changed(&server.state, handle).await;
            }
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
        if let Some(handle) = server.handle.as_ref() {
            emit_sessions_changed(&server.state, handle).await;
        }
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
            McpSessionState::new("mcp-http".to_string(), String::new()),
        );
    }

    if req.id.is_none() && req.method.starts_with("notifications/") {
        return empty_response(StatusCode::ACCEPTED);
    }

    let payload = dispatch_request(&server, &session_id, req).await;
    if let Some(handle) = server.handle.as_ref() {
        emit_sessions_changed(&server.state, handle).await;
    }
    json_http_response(StatusCode::OK, &payload, Some(&session_id))
}

async fn dispatch_request(
    server: &HttpServerState,
    session_id: &str,
    req: JsonRpcRequest,
) -> JsonRpcResponse {
    match req.method.as_str() {
        "ping" => json_rpc_result(req.id, json!({})),
        "resources/list" => json_rpc_result(req.id, json!({ "resources": resource_definitions() })),
        "resources/read" => {
            match serde_json::from_value::<ReadResourceParams>(req.params.unwrap_or_default()) {
                Ok(params) => match read_resource_content(&server.state, &params.uri) {
                    Some(content) => {
                        mark_session_resource_read(&server.state, session_id, &params.uri).await;
                        json_rpc_result(
                            req.id,
                            json!({
                                "contents": [
                                    {
                                        "uri": params.uri,
                                        "mimeType": content.mime_type,
                                        "text": content.text
                                    }
                                ]
                            }),
                        )
                    }
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
        "tools/list" => {
            let ecky_ast_authoring = server.state.config.lock().unwrap().mcp.ecky_ast_authoring;
            json_rpc_result(
                req.id,
                json!({ "tools": tool_definitions_with_ast_enabled(ecky_ast_authoring) }),
            )
        }
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

#[cfg(test)]
fn dispatched_tool_names() -> Vec<&'static str> {
    vec![
        "health_check",
        "session_log_in",
        "session_log_out",
        "resume_session",
        "ui_dispatch",
        "workspace_overview",
        "freecad_library_search",
        "freecad_library_import",
        "thread_list",
        "thread_create",
        "thread_borrow",
        "thread_meta_get",
        "thread_messages_get",
        "thread_get",
        "agent_identity_set",
        "target_meta_get",
        "target_macro_get",
        "ecky_ast_get",
        "ecky_ast_inspect",
        "ecky_ast_get_node",
        "ecky_ast_patch_validate",
        "ecky_ast_replace_and_render",
        "ecky_ast_patch_preview",
        "ecky_ast_set_number",
        "ecky_ast_set_string",
        "ecky_ast_set_select",
        "ecky_ast_replace_call",
        "ecky_ast_insert_binding",
        "ecky_ast_delete_binding",
        "ecky_ast_rename_binding_scoped",
        "macro_buffer_get",
        "macro_buffer_replace_range",
        "macro_buffer_apply_patch",
        "macro_buffer_preview_render",
        "target_detail_get",
        "artifact_manifest_get",
        "artifact_feature_graph_get",
        "target_get",
        "ecky_dependency_get",
        "ecky_selector_resolve",
        "ecky_constraints_validate",
        "get_model_screenshot",
        "concept_preview_save",
        "params_preview_render",
        "macro_preview_render",
        "macro_buffer_replace_and_preview",
        "semantic_manifest_get",
        "semantic_manifest_detail_get",
        "control_primitive_save",
        "control_primitive_delete",
        "control_view_save",
        "control_view_delete",
        "measurement_annotation_save",
        "measurement_annotation_delete",
        "commit_preview_version",
        "ecky_ast_patch_commit",
        "thread_fork_from_target",
        "compare_models",
        "version_restore",
        "user_confirm_request",
        "request_user_prompt",
        "mark_as_read",
        "session_reply_save",
        "session_activity_set",
        "session_activity_clear",
        "long_action_notice",
        "long_action_clear",
        "finalize_thread",
        "verify_generated_model",
        "get_structural_verification_summary",
        "printability_analyze",
        "printability_transform_recipes_get",
        "semantic_transform_preview",
    ]
}

async fn execute_ecky_ast_replace_preview_call(
    server: &HttpServerState,
    session_id: &str,
    current_ctx: &AgentContext,
    mut req_args: EckyAstReplaceAndRenderRequest,
) -> AppResult<(Value, Option<McpTargetRef>)> {
    let action_ctx = current_ctx.with_override(&req_args.identity);
    let target = resolve_target_for_session(
        &server.state,
        server.app.as_ref(),
        session_id,
        req_args.thread_id.clone(),
        req_args.message_id.clone(),
    )
    .await?;
    let (source_language, geometry_backend) = effective_existing_authoring_context(
        target.source_language,
        target.geometry_backend,
        req_args.geometry_backend,
    );
    ensure_authoring_guides_read(
        &server.state,
        session_id,
        source_language,
        geometry_backend,
        "ecky_ast_replace_and_render",
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
    let edited_path = req_args
        .path
        .clone()
        .or_else(|| req_args.stable_node_key.clone())
        .unwrap_or_default();
    let operation = ecky_ast_edit_operation_name(&req_args.operation).to_string();
    match handlers::handle_ecky_ast_replace_and_render(
        &server.state,
        server.app.as_ref(),
        req_args,
        &action_ctx,
    )
    .await
    {
        Ok(response) => {
            let value = compact_ecky_ast_replace_and_render_response_value(
                &response,
                &operation,
                &edited_path,
            );
            let next_target = target_ref_from_value(&value).unwrap_or(lease_target.clone());
            move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target).await?;
            Ok((value, Some(next_target)))
        }
        Err(err) => {
            let _ = release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
            Err(err)
        }
    }
}

async fn dispatch_tool_call(
    server: &HttpServerState,
    session_id: &str,
    params: CallToolParams,
) -> AppResult<(Value, Option<McpTargetRef>)> {
    let config = server.state.config.lock().unwrap().clone();
    ensure_mcp_tool_allowed_for_app_mode(&config, &params.name)?;

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
        "ui_dispatch" => {
            let req_args: UiDispatchRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let handle = require_server_handle(server, "ui_dispatch")?;
            let response = handlers::handle_ui_dispatch(handle, req_args).await?;
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
                .map(|thread| thread_list_entry(&conn, thread))
                .collect::<AppResult<Vec<_>>>()?;

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
                            agent_brief: workspace_overview_brief(
                                &server.state,
                                Some(target.source_language),
                                Some(target.geometry_backend),
                            ),
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
                            agent_brief: workspace_overview_brief(&server.state, None, None),
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
        "freecad_library_search" => {
            let req_args: FreecadLibrarySearchRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = crate::freecad_library::search_freecad_library(
                &req_args,
                &config.freecad_library_roots,
            )?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "freecad_library_import" => {
            let req_args: FreecadLibraryImportRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let import_path = crate::freecad_library::import_path_from_request(&req_args)?;
            let source_path = import_path
                .to_str()
                .ok_or_else(|| AppError::internal("Invalid FreeCAD library import path."))?;
            let extension = import_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase())
                .unwrap_or_default();
            if matches!(extension.as_str(), "stl" | "obj" | "3mf") {
                let bundle = crate::freecad_library::import_mesh_from_request(
                    &req_args,
                    server.app.as_ref(),
                )?;
                let manifest = crate::model_runtime::read_model_manifest(
                    server.app.as_ref(),
                    &bundle.model_id,
                )?;
                let current_thread_id = session.bound_thread_id.as_deref().or_else(|| {
                    session
                        .last_target
                        .as_ref()
                        .map(|target| target.thread_id.as_str())
                });
                let (response, target) = persist_freecad_library_import_version(
                    &server.state,
                    server.app.as_ref(),
                    &req_args,
                    bundle,
                    manifest,
                    current_thread_id,
                )
                .await?;
                emit_history_updated(server);
                return Ok((response, Some(target)));
            }
            let _guard = server.state.render_lock.lock().await;
            let bundle = match extension.as_str() {
                "fcstd" => crate::freecad::import_fcstd(
                    source_path,
                    crate::services::render::configured_freecad_cmd(&server.state).as_deref(),
                    server.app.as_ref(),
                )?,
                "step" | "stp" => crate::freecad::import_step(
                    source_path,
                    crate::services::render::configured_freecad_cmd(&server.state).as_deref(),
                    server.app.as_ref(),
                )?,
                other => {
                    return Err(AppError::validation(format!(
                        "FreeCAD library format '{}' is not importable yet.",
                        other
                    )));
                }
            };
            let manifest =
                crate::model_runtime::read_model_manifest(server.app.as_ref(), &bundle.model_id)?;
            let current_thread_id = session.bound_thread_id.as_deref().or_else(|| {
                session
                    .last_target
                    .as_ref()
                    .map(|target| target.thread_id.as_str())
            });
            let (response, target) = persist_freecad_library_import_version(
                &server.state,
                server.app.as_ref(),
                &req_args,
                bundle,
                manifest,
                current_thread_id,
            )
            .await?;
            emit_history_updated(server);
            Ok((response, Some(target)))
        }
        "thread_list" => {
            let response = handlers::handle_thread_list(&server.state).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "thread_create" => {
            let req_args: ThreadCreateRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let response =
                handlers::handle_thread_create(&server.state, req_args, &action_ctx).await?;
            emit_history_updated(server);
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "thread_borrow" => {
            let req_args: ThreadBorrowRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let action_ctx = current_ctx.with_override(&req_args.identity);
            let response =
                handlers::handle_thread_borrow(&server.state, req_args, &action_ctx).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "thread_meta_get" => {
            let req_args: ThreadMetaRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_thread_meta_get(&server.state, req_args).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "thread_messages_get" => {
            let req_args: ThreadMessagesRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_thread_messages_get(&server.state, req_args).await?;
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
                    start_line: None,
                    end_line: None,
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
        "ecky_ast_get" | "ecky_ast_inspect" => {
            let mut req_args =
                serde_json::from_value::<EckyAstGetRequest>(args).unwrap_or(EckyAstGetRequest {
                    identity: AgentIdentityOverride::default(),
                    thread_id: None,
                    message_id: None,
                    path: None,
                    depth: None,
                    max_nodes: None,
                    include_source: None,
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
            let response = handlers::handle_ecky_ast_get(
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
        "ecky_ast_get_node" => {
            let req_node = serde_json::from_value::<EckyAstGetNodeRequest>(args.clone())
                .map_err(|e| AppError::validation(e.to_string()))?;
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_node.thread_id.clone(),
                req_node.message_id.clone(),
            )
            .await?;

            let stable_node_key = req_node
                .stable_node_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let explicit_path = req_node
                .path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);

            if stable_node_key.is_none() && explicit_path.is_none() {
                return Err(AppError::validation(
                    "ecky_ast_get_node requires stableNodeKey or path.",
                ));
            }

            let path = if let Some(path) = explicit_path {
                path
            } else {
                let inspect = handlers::handle_ecky_ast_get(
                    &server.state,
                    server.app.as_ref(),
                    EckyAstGetRequest {
                        identity: req_node.identity.clone(),
                        thread_id: Some(target.thread_id.clone()),
                        message_id: Some(target.message_id.clone()),
                        path: None,
                        depth: Some(12),
                        max_nodes: Some(500),
                        include_source: Some(false),
                    },
                    &current_ctx,
                )
                .await?;
                let stable_node_key = stable_node_key.unwrap_or_default();
                if let Some(path) = inspect
                    .nodes
                    .iter()
                    .find(|node| node.stable_node_key == stable_node_key)
                    .map(|node| node.path.clone())
                {
                    path
                } else if let Some(legacy_path) = legacy_stable_node_key_path_hint(&stable_node_key)
                {
                    inspect
                        .nodes
                        .iter()
                        .find(|node| node.path == legacy_path)
                        .map(|node| node.path.clone())
                        .ok_or_else(|| {
                            AppError::validation(format!(
                                "stableNodeKey not found in bounded AST window: {} (legacy path hint {}).",
                                stable_node_key, legacy_path
                            ))
                        })?
                } else {
                    return Err(AppError::validation(format!(
                        "stableNodeKey not found in bounded AST window: {}.",
                        stable_node_key
                    )));
                }
            };

            let response = handlers::handle_ecky_ast_get(
                &server.state,
                server.app.as_ref(),
                EckyAstGetRequest {
                    identity: req_node.identity,
                    thread_id: Some(target.thread_id.clone()),
                    message_id: Some(target.message_id.clone()),
                    path: Some(path),
                    depth: Some(0),
                    max_nodes: Some(32),
                    include_source: req_node.include_source,
                },
                &current_ctx,
            )
            .await?;
            let value = serde_json::to_value(&response).unwrap();
            let next_target = target_ref_from_value(&value);
            Ok((value, next_target))
        }
        "ecky_dependency_get" => {
            let mut req_args: EckyDependencyGetRequest =
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
            let response = handlers::handle_ecky_dependency_get(
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
        "ecky_selector_resolve" => {
            let mut req_args: EckySelectorResolveRequest =
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
            let response = handlers::handle_ecky_selector_resolve(
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
        "ecky_constraints_validate" => {
            let mut req_args: EckyConstraintsValidateRequest =
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
            let response = handlers::handle_ecky_constraints_validate(
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
        "ecky_ast_patch_validate" => {
            let mut req_args: EckyAstPatchValidateRequest =
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
            ensure_authoring_guides_read(
                &server.state,
                session_id,
                target.source_language,
                target.geometry_backend,
                "ecky_ast_patch_validate",
            )
            .await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            let response = handlers::handle_ecky_ast_patch_validate(
                &server.state,
                server.app.as_ref(),
                req_args,
                &action_ctx,
            )
            .await?;
            let value = serde_json::to_value(&response).unwrap();
            let next_target = target_ref_from_value(&value);
            Ok((value, next_target))
        }
        "ecky_ast_replace_and_render" | "ecky_ast_patch_preview" => {
            let req_args: EckyAstReplaceAndRenderRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            execute_ecky_ast_replace_preview_call(server, session_id, &current_ctx, req_args).await
        }
        "ecky_ast_set_number" => {
            let req: EckyAstSetNumberCallRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let req_args = EckyAstReplaceAndRenderRequest {
                identity: req.identity,
                thread_id: req.thread_id,
                message_id: req.message_id,
                operation: EckyAstEditOperation::Replace,
                source_digest: req.source_digest,
                stable_node_key: None,
                path: Some(req.path),
                expected_node_digest: req.expected_node_digest,
                replacement_source: Some(req.value.to_string()),
                new_name: None,
                parameters: req.parameters,
                post_processing: req.post_processing,
                geometry_backend: req.geometry_backend,
            };
            execute_ecky_ast_replace_preview_call(server, session_id, &current_ctx, req_args).await
        }
        "ecky_ast_set_string" => {
            let req: EckyAstSetStringCallRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let replacement = serde_json::to_string(&req.value)
                .map_err(|e| AppError::validation(e.to_string()))?;
            let req_args = EckyAstReplaceAndRenderRequest {
                identity: req.identity,
                thread_id: req.thread_id,
                message_id: req.message_id,
                operation: EckyAstEditOperation::Replace,
                source_digest: req.source_digest,
                stable_node_key: None,
                path: Some(req.path),
                expected_node_digest: req.expected_node_digest,
                replacement_source: Some(replacement),
                new_name: None,
                parameters: req.parameters,
                post_processing: req.post_processing,
                geometry_backend: req.geometry_backend,
            };
            execute_ecky_ast_replace_preview_call(server, session_id, &current_ctx, req_args).await
        }
        "ecky_ast_set_select" => {
            let req: EckyAstSetSelectCallRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let replacement = ecky_literal_from_json(&req.value)?;
            let req_args = EckyAstReplaceAndRenderRequest {
                identity: req.identity,
                thread_id: req.thread_id,
                message_id: req.message_id,
                operation: EckyAstEditOperation::Replace,
                source_digest: req.source_digest,
                stable_node_key: None,
                path: Some(req.path),
                expected_node_digest: req.expected_node_digest,
                replacement_source: Some(replacement),
                new_name: None,
                parameters: req.parameters,
                post_processing: req.post_processing,
                geometry_backend: req.geometry_backend,
            };
            execute_ecky_ast_replace_preview_call(server, session_id, &current_ctx, req_args).await
        }
        "ecky_ast_replace_call" => {
            let req: EckyAstReplaceCallRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let req_args = EckyAstReplaceAndRenderRequest {
                identity: req.identity,
                thread_id: req.thread_id,
                message_id: req.message_id,
                operation: EckyAstEditOperation::Replace,
                source_digest: req.source_digest,
                stable_node_key: None,
                path: Some(req.path),
                expected_node_digest: req.expected_node_digest,
                replacement_source: Some(req.replacement_source),
                new_name: None,
                parameters: req.parameters,
                post_processing: req.post_processing,
                geometry_backend: req.geometry_backend,
            };
            execute_ecky_ast_replace_preview_call(server, session_id, &current_ctx, req_args).await
        }
        "ecky_ast_insert_binding" => {
            let req: EckyAstInsertBindingCallRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let operation = match req.position.as_deref() {
                Some("before") => EckyAstEditOperation::InsertBefore,
                Some("after") | None => EckyAstEditOperation::InsertAfter,
                Some(other) => {
                    return Err(AppError::validation(format!(
                        "Unsupported position for ecky_ast_insert_binding: {other}. Use before|after."
                    )));
                }
            };
            let req_args = EckyAstReplaceAndRenderRequest {
                identity: req.identity,
                thread_id: req.thread_id,
                message_id: req.message_id,
                operation,
                source_digest: req.source_digest,
                stable_node_key: None,
                path: Some(req.path),
                expected_node_digest: req.expected_node_digest,
                replacement_source: Some(req.binding_source),
                new_name: None,
                parameters: req.parameters,
                post_processing: req.post_processing,
                geometry_backend: req.geometry_backend,
            };
            execute_ecky_ast_replace_preview_call(server, session_id, &current_ctx, req_args).await
        }
        "ecky_ast_delete_binding" => {
            let req: EckyAstDeleteBindingCallRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let req_args = EckyAstReplaceAndRenderRequest {
                identity: req.identity,
                thread_id: req.thread_id,
                message_id: req.message_id,
                operation: EckyAstEditOperation::Delete,
                source_digest: req.source_digest,
                stable_node_key: None,
                path: Some(req.path),
                expected_node_digest: req.expected_node_digest,
                replacement_source: None,
                new_name: None,
                parameters: req.parameters,
                post_processing: req.post_processing,
                geometry_backend: req.geometry_backend,
            };
            execute_ecky_ast_replace_preview_call(server, session_id, &current_ctx, req_args).await
        }
        "ecky_ast_rename_binding_scoped" => {
            let req: EckyAstRenameBindingCallRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let req_args = EckyAstReplaceAndRenderRequest {
                identity: req.identity,
                thread_id: req.thread_id,
                message_id: req.message_id,
                operation: EckyAstEditOperation::Rename,
                source_digest: req.source_digest,
                stable_node_key: None,
                path: Some(req.path),
                expected_node_digest: req.expected_node_digest,
                replacement_source: None,
                new_name: Some(req.new_name),
                parameters: req.parameters,
                post_processing: req.post_processing,
                geometry_backend: req.geometry_backend,
            };
            execute_ecky_ast_replace_preview_call(server, session_id, &current_ctx, req_args).await
        }
        "ecky_ast_patch_commit" => {
            let req_args: VersionSaveRequest =
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
            match handlers::handle_commit_preview_version(
                &server.state,
                server.app.as_ref(),
                req_args,
                &action_ctx,
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
        "macro_buffer_get" => {
            if server.state.config.lock().unwrap().mcp.ecky_ast_authoring {
                return Err(AppError::validation(
                    "macro_buffer_get is disabled while mcp.eckyAstAuthoring=true. Use ecky_ast_get.",
                ));
            }
            let mut req_args = serde_json::from_value::<MacroBufferGetRequest>(args).unwrap_or(
                MacroBufferGetRequest {
                    identity: AgentIdentityOverride::default(),
                    thread_id: None,
                    message_id: None,
                    start_line: None,
                    end_line: None,
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
            let response = handlers::handle_macro_buffer_get(
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
        "macro_buffer_replace_range" => {
            if server.state.config.lock().unwrap().mcp.ecky_ast_authoring {
                return Err(AppError::validation(
                    "macro_buffer_replace_range is disabled while mcp.eckyAstAuthoring=true.",
                ));
            }
            let req_args: MacroBufferReplaceAndRenderRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            ensure_authoring_guides_read(
                &server.state,
                session_id,
                target.source_language,
                target.geometry_backend,
                "macro_buffer_replace_range",
            )
            .await?;
            let response =
                handlers::handle_macro_buffer_replace_range(req_args, &current_ctx).await?;
            Ok((serde_json::to_value(&response).unwrap(), None))
        }
        "macro_buffer_apply_patch" => {
            if server.state.config.lock().unwrap().mcp.ecky_ast_authoring {
                return Err(AppError::validation(
                    "macro_buffer_apply_patch is disabled while mcp.eckyAstAuthoring=true.",
                ));
            }
            let req_args: MacroBufferApplyPatchRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                None,
                None,
            )
            .await?;
            ensure_authoring_guides_read(
                &server.state,
                session_id,
                target.source_language,
                target.geometry_backend,
                "macro_buffer_apply_patch",
            )
            .await?;
            let response =
                handlers::handle_macro_buffer_apply_patch(req_args, &current_ctx).await?;
            Ok((serde_json::to_value(&response).unwrap(), None))
        }
        "macro_buffer_preview_render" => {
            if server.state.config.lock().unwrap().mcp.ecky_ast_authoring {
                return Err(AppError::validation(
                    "macro_buffer_preview_render is disabled while mcp.eckyAstAuthoring=true.",
                ));
            }
            let req_args: MacroBufferRenderRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                None,
                None,
            )
            .await?;
            ensure_authoring_guides_read(
                &server.state,
                session_id,
                target.source_language,
                target.geometry_backend,
                "macro_buffer_preview_render",
            )
            .await?;
            let response = handlers::handle_macro_buffer_preview_render(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await?;
            let value = compact_macro_replace_response_value(&response);
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
        "artifact_manifest_get" => {
            let mut req_args: ArtifactManifestRequest =
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
            if req_args.model_id.is_none() {
                req_args.model_id = target.model_id.clone();
            }
            let response = handlers::handle_artifact_manifest_get(
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
        "artifact_feature_graph_get" => {
            let mut req_args: ArtifactFeatureGraphGetRequest =
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
            if req_args.model_id.is_none() {
                req_args.model_id = target.model_id.clone();
            }
            let response = handlers::handle_artifact_feature_graph_get(
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
        "concept_preview_save" => {
            let req_args: ConceptPreviewSaveRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response =
                handlers::handle_concept_preview_save(&server.state, req_args, &current_ctx)
                    .await?;
            let value = serde_json::json!({
                "threadId": response.thread_id,
                "messageId": response.message_id,
                "imageData": response.image_data,
                "caption": response.caption,
            });
            Ok((value, None))
        }
        "params_preview_render" => {
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
            let (source_language, geometry_backend) = effective_existing_authoring_context(
                target.source_language,
                target.geometry_backend,
                req_args.geometry_backend,
            );
            ensure_authoring_guides_read(
                &server.state,
                session_id,
                source_language,
                geometry_backend,
                "params_preview_render",
            )
            .await?;

            if let Some(handle) = server.handle.as_ref() {
                let _ = handle.emit(
                    "mcp://ui-dispatch",
                    AgentUiDispatchEvent {
                        action: "openWindow".to_string(),
                        target: "params".to_string(),
                        value: None,
                    },
                );
            }

            for (key, val) in &req_args.parameter_patch {
                if let Some(handle) = server.handle.as_ref() {
                    let _ = handle.emit(
                        "mcp://ui-dispatch",
                        AgentUiDispatchEvent {
                            action: "highlightParam".to_string(),
                            target: key.clone(),
                            value: serde_json::to_value(val).ok(),
                        },
                    );
                }
            }

            let lease_target = McpTargetRef {
                thread_id: target.thread_id.clone(),
                message_id: target.message_id.clone(),
                model_id: target.model_id.clone(),
            };
            acquire_lease(&server.state, &action_ctx, &lease_target).await?;
            req_args.thread_id = Some(target.thread_id.clone());
            req_args.message_id = Some(target.message_id.clone());
            match handlers::handle_params_preview_render(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = compact_params_patch_response_value(&response);
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
        "macro_preview_render" => {
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
                    let (source_language, geometry_backend) = effective_existing_authoring_context(
                        target.source_language,
                        target.geometry_backend,
                        req_args.geometry_backend,
                    );
                    ensure_authoring_guides_read(
                        &server.state,
                        session_id,
                        source_language,
                        geometry_backend,
                        "macro_preview_render",
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
                    match handlers::handle_macro_preview_render(
                        &server.state,
                        server.app.as_ref(),
                        req_args,
                        &action_ctx,
                    )
                    .await
                    {
                        Ok(response) => {
                            let value = compact_macro_replace_response_value(&response);
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
                    if req_args.thread_id.is_none() {
                        req_args.thread_id =
                            bound_thread_id_for_session(&server.state, session_id).await;
                    }
                    if req_args.thread_id.is_none() {
                        return Err(e.clone());
                    }
                    let config = server.state.config.lock().unwrap().clone();
                    let (source_language, geometry_backend) =
                        first_version_macro_request_authoring_context(&config, &req_args);
                    ensure_authoring_guides_read(
                        &server.state,
                        session_id,
                        source_language,
                        geometry_backend,
                        "macro_preview_render",
                    )
                    .await?;
                    match handlers::handle_macro_preview_render(
                        &server.state,
                        server.app.as_ref(),
                        req_args,
                        &action_ctx,
                    )
                    .await
                    {
                        Ok(response) => {
                            let value = compact_macro_replace_response_value(&response);
                            let next_target = target_ref_from_value(&value);
                            Ok((value, next_target))
                        }
                        Err(err) => Err(err),
                    }
                }
                Err(e) => Err(e),
            }
        }
        "macro_buffer_replace_and_preview" => {
            if server.state.config.lock().unwrap().mcp.ecky_ast_authoring {
                return Err(AppError::validation(
                    "macro_buffer_replace_and_preview is disabled while mcp.eckyAstAuthoring=true.",
                ));
            }
            let mut req_args: MacroBufferReplaceAndRenderRequest =
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
            let (source_language, geometry_backend) = effective_existing_authoring_context(
                target.source_language,
                target.geometry_backend,
                req_args.geometry_backend,
            );
            ensure_authoring_guides_read(
                &server.state,
                session_id,
                source_language,
                geometry_backend,
                "macro_buffer_replace_and_preview",
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
            match handlers::handle_macro_buffer_replace_and_preview(
                &server.state,
                server.app.as_ref(),
                req_args,
                &action_ctx,
            )
            .await
            {
                Ok(response) => {
                    let value = compact_macro_buffer_replace_and_preview_response_value(&response);
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
        "semantic_manifest_detail_get" => {
            let mut req_args: SemanticManifestDetailRequest =
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
            let response = handlers::handle_semantic_manifest_detail_get(
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
            ensure_target_authoring_guides_read(
                &server.state,
                session_id,
                &target,
                "control_primitive_save",
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
                    emit_history_updated(server);
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
            ensure_target_authoring_guides_read(
                &server.state,
                session_id,
                &target,
                "control_primitive_delete",
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
                    emit_history_updated(server);
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
            ensure_target_authoring_guides_read(
                &server.state,
                session_id,
                &target,
                "control_view_save",
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
                    emit_history_updated(server);
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
            ensure_target_authoring_guides_read(
                &server.state,
                session_id,
                &target,
                "control_view_delete",
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
                    emit_history_updated(server);
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
            ensure_target_authoring_guides_read(
                &server.state,
                session_id,
                &target,
                "measurement_annotation_save",
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
                    emit_history_updated(server);
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
            ensure_target_authoring_guides_read(
                &server.state,
                session_id,
                &target,
                "measurement_annotation_delete",
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
                    emit_history_updated(server);
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "commit_preview_version" => {
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
            match handlers::handle_commit_preview_version(
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
                    emit_history_updated(server);
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
                    emit_history_updated(server);
                    Ok((value, Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        "compare_models" => {
            let req_args: CompareModelsRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_compare_models(server.app.as_ref(), req_args).await?;
            Ok((serde_json::to_value(&response).unwrap(), None))
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
            let handle = require_server_handle(server, "user_confirm_request")?;
            let response =
                handlers::handle_user_confirm_request(&server.state, handle, req, &current_ctx)
                    .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "request_user_prompt" => {
            let req: UserPromptRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let handle = require_server_handle(server, "request_user_prompt")?;
            let response =
                handlers::handle_request_user_prompt(&server.state, handle, req, &current_ctx)
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
            emit_history_updated(server);
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "verify_generated_model" => {
            let req_args: VerifyGeneratedModelRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let model_id = req_args
                .model_id
                .or(target.model_id.clone())
                .ok_or_else(|| AppError::validation("No model_id available for verification."))?;
            let original_prompt = req_args.original_prompt.unwrap_or_default();
            let response = handlers::handle_verify_generated_model(
                &server.state,
                server.app.as_ref(),
                &target.thread_id,
                &target.message_id,
                &model_id,
                &original_prompt,
            )?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "get_structural_verification_summary" => {
            let req_args: StructuralVerificationSummaryRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let model_id = req_args
                .model_id
                .or(target.model_id.clone())
                .ok_or_else(|| AppError::validation("No model_id available for verification."))?;
            let response = handlers::handle_structural_verification_summary(
                &server.state,
                server.app.as_ref(),
                &target.thread_id,
                &target.message_id,
                &model_id,
            )?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "printability_analyze" => {
            let req_args: PrintabilityAnalyzeRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let model_id = req_args
                .model_id
                .or(target.model_id.clone())
                .ok_or_else(|| {
                    AppError::validation("No model_id available for printability analysis.")
                })?;
            let response = handlers::handle_printability_analyze(
                &server.state,
                server.app.as_ref(),
                &target.thread_id,
                &target.message_id,
                &model_id,
            )?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "printability_transform_recipes_get" => {
            let req_args: PrintabilityTransformRecipesGetRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let target = resolve_target_for_session(
                &server.state,
                server.app.as_ref(),
                session_id,
                req_args.thread_id.clone(),
                req_args.message_id.clone(),
            )
            .await?;
            let model_id = req_args
                .model_id
                .or(target.model_id.clone())
                .ok_or_else(|| {
                    AppError::validation(
                        "No model_id available for printability transform recipes.",
                    )
                })?;
            let response = handlers::handle_printability_transform_recipes_get(
                &server.state,
                server.app.as_ref(),
                &target.thread_id,
                &target.message_id,
                &model_id,
            )?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "semantic_transform_preview" => {
            let mut req_args: SemanticTransformPreviewRequest =
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
            ensure_authoring_guides_read(
                &server.state,
                session_id,
                target.source_language,
                target.geometry_backend,
                "semantic_transform_preview",
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
            if req_args.model_id.is_none() {
                req_args.model_id = target.model_id.clone();
            }
            match handlers::handle_semantic_transform_preview(
                &server.state,
                server.app.as_ref(),
                req_args,
                &action_ctx,
            )
            .await
            {
                Ok(response) => {
                    let next_target = McpTargetRef {
                        thread_id: response.thread_id.clone(),
                        message_id: response.preview_id.clone(),
                        model_id: Some(response.model_id.clone()),
                    };
                    move_or_refresh_lease(&server.state, &action_ctx, &lease_target, &next_target)
                        .await?;
                    Ok((serde_json::to_value(response).unwrap(), Some(next_target)))
                }
                Err(err) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(err)
                }
            }
        }
        _ => Err(AppError::validation(format!(
            "Unknown tool: {}",
            params.name
        ))),
    }
}

async fn persist_freecad_library_import_version(
    state: &AppState,
    app: &dyn PathResolver,
    request: &FreecadLibraryImportRequest,
    artifact_bundle: ArtifactBundle,
    model_manifest: ModelManifest,
    current_thread_id: Option<&str>,
) -> AppResult<(Value, McpTargetRef)> {
    crate::models::validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;

    let label = model_manifest.document.document_label.trim();
    let document_name = model_manifest.document.document_name.trim();
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| (!label.is_empty()).then_some(label))
        .or_else(|| (!document_name.is_empty()).then_some(document_name))
        .or_else(|| (!request.item.name.trim().is_empty()).then_some(request.item.name.trim()))
        .unwrap_or("FreeCAD Library Part")
        .to_string();
    let thread_id = request
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| current_thread_id.map(str::to_string))
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let now = now_secs();
    let db = state.db.lock().await;
    let existing_title = db::get_thread_title(&db, &thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    let thread_traits = if existing_title.is_none() {
        Some(crate::generate_genie_traits())
    } else {
        None
    };
    let thread_title = existing_title.as_deref().unwrap_or(&title);
    db::create_or_update_thread(&db, &thread_id, thread_title, now, thread_traits.as_ref())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let message_id = Uuid::new_v4().to_string();
    let content = if label.is_empty() {
        format!("Imported FreeCAD library part: {}.", request.item.name)
    } else {
        format!("Imported FreeCAD library part: {}.", label)
    };
    let message = Message {
        id: message_id.clone(),
        role: MessageRole::Assistant,
        content,
        status: MessageStatus::Success,
        output: None,
        usage: None,
        artifact_bundle: Some(artifact_bundle.clone()),
        model_manifest: Some(model_manifest.clone()),
        agent_origin: None,
        image_data: None,
        visual_kind: None,
        attachment_images: Vec::new(),
        timestamp: now,
    };
    db::add_message(&db, &thread_id, &message)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    let _ = crate::persist_thread_summary(&db, &thread_id, thread_title);
    drop(db);

    let snapshot = crate::services::session::build_runtime_snapshot(
        None,
        Some(thread_id.clone()),
        Some(message_id.clone()),
        Some(artifact_bundle.clone()),
        Some(model_manifest.clone()),
        None,
    );
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
    }
    crate::services::session::write_last_snapshot(app, Some(&snapshot));

    let target = McpTargetRef {
        thread_id: thread_id.clone(),
        message_id: message_id.clone(),
        model_id: Some(artifact_bundle.model_id.clone()),
    };
    Ok((
        json!({
            "threadId": thread_id,
            "messageId": message_id,
            "title": thread_title,
            "artifactBundle": artifact_bundle,
            "modelManifest": model_manifest
        }),
        target,
    ))
}

fn app_mode_blocks_external_mcp_tools(config: &Config) -> Option<&'static str> {
    if config.connection_type.as_deref() == Some("mcp") {
        return None;
    }

    let api_key_like_mode = config.connection_type.as_deref() == Some("api_key")
        || (config.connection_type.is_none() && config.engines.iter().any(|engine| engine.enabled));

    if api_key_like_mode {
        Some("app is in api key mode. External MCP tools are disabled.")
    } else {
        Some("app is not in mcp mode. External MCP tools are disabled.")
    }
}

fn tool_allowed_while_external_mcp_blocked(tool_name: &str) -> bool {
    matches!(tool_name, "health_check" | "session_log_out")
}

fn ensure_mcp_tool_allowed_for_app_mode(config: &Config, tool_name: &str) -> AppResult<()> {
    let Some(message) = app_mode_blocks_external_mcp_tools(config) else {
        return Ok(());
    };

    if tool_allowed_while_external_mcp_blocked(tool_name) {
        return Ok(());
    }

    Err(AppError::conflict(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Config, McpConfig};
    use crate::models::{
        ArtifactBundle, DesignOutput, InteractionMode, MacroDialect, Message, MessageRole,
        MessageStatus, ModelManifest,
    };
    use rusqlite::Connection;
    use std::collections::BTreeMap;
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
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::models::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
        }
    }

    fn test_state() -> AppState {
        AppState::new(
            test_config(),
            None,
            Connection::open_in_memory().expect("memory db"),
        )
    }

    fn test_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-mcp-server-{name}-{}", Uuid::new_v4()))
    }

    fn test_api_key_config() -> Config {
        Config {
            engines: vec![crate::contracts::Engine {
                id: "engine-1".to_string(),
                name: "Engine".to_string(),
                provider: "openai".to_string(),
                api_key: "key".to_string(),
                model: "gpt-5.4".to_string(),
                light_model: String::new(),
                base_url: String::new(),
                enabled: true,
            }],
            selected_engine_id: "engine-1".to_string(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig::default(),
            has_seen_onboarding: true,
            connection_type: Some("api_key".to_string()),
            default_engine_kind: crate::models::EngineKind::Freecad,
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::models::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
        }
    }

    fn test_mcp_engine_state(provider: &str, model: &str) -> AppState {
        AppState::new(
            Config {
                engines: vec![crate::contracts::Engine {
                    id: "engine-1".to_string(),
                    name: "Engine".to_string(),
                    provider: provider.to_string(),
                    api_key: "key".to_string(),
                    model: model.to_string(),
                    light_model: String::new(),
                    base_url: String::new(),
                    enabled: true,
                }],
                selected_engine_id: "engine-1".to_string(),
                freecad_cmd: String::new(),
                cad_text_font_path: String::new(),
                freecad_library_roots: Vec::new(),
                assets: Vec::new(),
                microwave: None,
                voice: crate::models::VoiceConfig::default(),
                mcp: McpConfig::default(),
                has_seen_onboarding: true,
                connection_type: Some("mcp".to_string()),
                default_engine_kind: crate::models::EngineKind::EckyIrV0,
                default_source_language: crate::models::SourceLanguage::EckyIrV0,
                default_geometry_backend: crate::models::GeometryBackend::Build123d,
                max_generation_attempts: 3,
                max_verify_attempts: 0,
            },
            None,
            Connection::open_in_memory().expect("memory db"),
        )
    }

    fn ecky_test_design(title: &str, version_name: &str, macro_code: &str) -> DesignOutput {
        DesignOutput {
            title: title.to_string(),
            version_name: version_name.to_string(),
            response: "ok".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: macro_code.to_string(),
            macro_dialect: MacroDialect::EckyIrV0,
            engine_kind: crate::models::EngineKind::EckyIrV0,
            source_language: crate::models::SourceLanguage::EckyIrV0,
            geometry_backend: crate::models::GeometryBackend::EckyRust,
            ui_spec: crate::models::UiSpec::default(),
            initial_params: std::collections::BTreeMap::new(),
            post_processing: None,
        }
    }

    fn ecky_test_bundle(model_id: &str) -> ArtifactBundle {
        ArtifactBundle {
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::EckyIrV0,
            geometry_backend: crate::models::GeometryBackend::EckyRust,
            source_language: crate::models::SourceLanguage::EckyIrV0,
            content_hash: format!("hash-{model_id}"),
            artifact_version: 1,
            fcstd_path: format!("/tmp/{model_id}.FCStd"),
            manifest_path: format!("/tmp/{model_id}.json"),
            macro_path: Some(format!("/tmp/{model_id}.ecky")),
            preview_stl_path: format!("/tmp/{model_id}.stl"),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        }
    }

    fn ecky_test_manifest(model_id: &str) -> ModelManifest {
        ModelManifest {
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: crate::models::EngineKind::EckyIrV0,
            geometry_backend: crate::models::GeometryBackend::EckyRust,
            source_language: crate::models::SourceLanguage::EckyIrV0,
            document: crate::models::DocumentMetadata {
                document_name: "Doc".to_string(),
                document_label: "Doc".to_string(),
                source_path: None,
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: Vec::new(),
            parameter_groups: Vec::new(),
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            feature_graph: None,
            correspondence_graph: None,
            warnings: Vec::new(),
            enrichment_state: crate::models::ManifestEnrichmentState {
                status: crate::models::EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    async fn seed_dispatch_ecky_target(macro_code: &str) -> (AppState, Arc<TestPathResolver>) {
        let config = {
            let state = test_mcp_engine_state("openai", "gpt-5.4");
            let config = state.config.lock().unwrap().clone();
            config
        };
        let conn = crate::db::init_db(&test_db_path("dispatch-ecky-target")).expect("db");
        let state = AppState::new(config, None, conn);
        state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

        let root = std::env::temp_dir().join(format!("ecky-mcp-server-root-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create test root");
        let resolver = Arc::new(TestPathResolver { root });
        let now = now_secs();
        let design = ecky_test_design("Wrapper Path", "V-wrapper", macro_code);
        let bundle = ecky_test_bundle("model-base");
        let manifest = ecky_test_manifest("model-base");

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
                    output: Some(design),
                    usage: None,
                    artifact_bundle: Some(bundle),
                    model_manifest: Some(manifest),
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

    async fn test_dispatch_server(macro_code: &str, session_id: &str) -> HttpServerState {
        let (state, resolver) = seed_dispatch_ecky_target(macro_code).await;
        state.mcp_sessions.lock().await.insert(
            session_id.to_string(),
            McpSessionState {
                client_kind: "mcp-http".to_string(),
                host_label: "Codex".to_string(),
                agent_label: "codex".to_string(),
                llm_model_id: None,
                llm_model_label: Some("gpt-5.4".to_string()),
                bound_thread_id: Some("thread-1".to_string()),
                last_target: Some(McpTargetRef {
                    thread_id: "thread-1".to_string(),
                    message_id: "msg-1".to_string(),
                    model_id: Some("model-base".to_string()),
                }),
                phase: Some("idle".to_string()),
                status_text: Some("ready".to_string()),
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

        HttpServerState {
            state,
            app: resolver,
            handle: None,
        }
    }

    async fn dispatch_tool_call_jsonrpc(
        server: &HttpServerState,
        session_id: &str,
        name: &str,
        arguments: Value,
    ) -> JsonRpcResponse {
        dispatch_request(
            server,
            session_id,
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "tools/call".to_string(),
                params: Some(json!({
                    "name": name,
                    "arguments": arguments
                })),
                id: Some(json!(1)),
            },
        )
        .await
    }

    fn parse_mcp_tool_payload(response: &JsonRpcResponse) -> Value {
        let result = response.result.as_ref().expect("json-rpc result");
        let text = result["content"][0]["text"]
            .as_str()
            .expect("tool payload text");
        serde_json::from_str(text).expect("tool payload JSON")
    }

    fn macro_source_from_target_macro_payload(payload: &Value) -> String {
        payload["lines"]
            .as_array()
            .expect("line array")
            .iter()
            .map(|line| {
                line.get("text")
                    .and_then(Value::as_str)
                    .expect("line text")
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn ecky_node_source_text(node: &Value) -> String {
        node.get("sourceSlice")
            .and_then(Value::as_str)
            .or_else(|| node["source"].get("text").and_then(Value::as_str))
            .expect("node source text")
            .to_string()
    }

    fn part_number_nodes(ast_payload: &Value) -> Vec<(String, String, String)> {
        ast_payload["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .filter_map(|node| {
                let path = node.get("path").and_then(Value::as_str)?;
                let value_kind = node.get("valueKind").and_then(Value::as_str)?;
                (path.starts_with("/parts/") && value_kind == "Number").then(|| {
                    (
                        path.to_string(),
                        node["digest"].as_str().expect("node digest").to_string(),
                        ecky_node_source_text(node),
                    )
                })
            })
            .collect()
    }

    fn part_number_node_map(ast_payload: &Value) -> BTreeMap<String, String> {
        part_number_nodes(ast_payload)
            .iter()
            .map(|(path, _digest, source)| (path.clone(), source.clone()))
            .collect()
    }

    fn run_async_test_with_large_stack<F, Fut>(run: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        std::thread::Builder::new()
            .name("mcp-server-large-stack-test".to_string())
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("test runtime");
                runtime.block_on(run());
            })
            .expect("spawn test thread")
            .join()
            .expect("join test thread");
    }

    #[test]
    fn ecky_ast_set_number_tool_path_updates_numeric_token_and_returns_preview_model() {
        run_async_test_with_large_stack(|| async {
            let source = format!(
                "; keep formatting + comment\n{}",
                include_str!("../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky")
            );
            let session_id = "session-ast-set-number";
            let server = test_dispatch_server(&source, session_id).await;

            let before_payload = parse_mcp_tool_payload(
                &dispatch_tool_call_jsonrpc(
                    &server,
                    session_id,
                    "target_macro_get",
                    json!({
                        "threadId": "thread-1",
                        "messageId": "msg-1"
                    }),
                )
                .await,
            );
            let before_source = macro_source_from_target_macro_payload(&before_payload);
            let source_digest = before_payload["digest"].as_str().expect("source digest");

            let ast_before_payload = parse_mcp_tool_payload(
                &dispatch_tool_call_jsonrpc(
                    &server,
                    session_id,
                    "ecky_ast_get",
                    json!({
                        "threadId": "thread-1",
                        "messageId": "msg-1",
                        "path": "/",
                        "depth": 16,
                        "maxNodes": 500,
                        "includeSource": true
                    }),
                )
                .await,
            );
            let numeric_nodes_before = part_number_nodes(&ast_before_payload);
            let numeric_before = part_number_node_map(&ast_before_payload);
            assert!(
                !numeric_before.is_empty(),
                "expected part numeric nodes in {}",
                serde_json::to_string_pretty(&ast_before_payload).unwrap_or_default()
            );
            let mut set_number_payload = None;
            let mut chosen_path = None;
            let mut failures = Vec::new();
            for (path, expected_node_digest, _old_node_value) in numeric_nodes_before.iter() {
                let payload = parse_mcp_tool_payload(
                    &dispatch_tool_call_jsonrpc(
                        &server,
                        session_id,
                        "ecky_ast_set_number",
                        json!({
                            "threadId": "thread-1",
                            "messageId": "msg-1",
                            "sourceDigest": source_digest,
                            "path": path,
                            "expectedNodeDigest": expected_node_digest,
                            "value": 0.45
                        }),
                    )
                    .await,
                );
                if payload.get("messageId").and_then(Value::as_str).is_some() {
                    chosen_path = Some(path.clone());
                    set_number_payload = Some(payload);
                    break;
                }
                failures.push(
                    payload["message"]
                        .as_str()
                        .unwrap_or("unknown set_number failure")
                        .to_string(),
                );
            }
            let path = chosen_path.expect("at least one candidate set_number path should succeed");
            let set_number_payload = set_number_payload.unwrap_or_else(|| {
                panic!(
                    "set_number failed across all numeric part nodes: {:?}",
                    failures
                )
            });
            assert!(
                set_number_payload
                    .get("messageId")
                    .and_then(Value::as_str)
                    .is_some(),
                "unexpected set_number payload: {}",
                serde_json::to_string_pretty(&set_number_payload).unwrap_or_default()
            );
            let preview_message_id = set_number_payload["messageId"]
                .as_str()
                .expect("preview message id");
            assert_ne!(preview_message_id, "msg-1");
            assert_eq!(set_number_payload["editedPath"], path.as_str());
            assert_eq!(set_number_payload["operation"], "replace");
            assert!(!set_number_payload["artifactDigest"]["modelId"]
                .as_str()
                .unwrap_or_default()
                .is_empty());

            let after_payload = parse_mcp_tool_payload(
                &dispatch_tool_call_jsonrpc(
                    &server,
                    session_id,
                    "target_macro_get",
                    json!({
                        "threadId": "thread-1",
                        "messageId": preview_message_id
                    }),
                )
                .await,
            );
            let after_source = macro_source_from_target_macro_payload(&after_payload);

            let ast_after_payload = parse_mcp_tool_payload(
                &dispatch_tool_call_jsonrpc(
                    &server,
                    session_id,
                    "ecky_ast_get",
                    json!({
                        "threadId": "thread-1",
                        "messageId": preview_message_id,
                        "path": "/",
                        "depth": 16,
                        "maxNodes": 500,
                        "includeSource": true
                    }),
                )
                .await,
            );
            let numeric_after = part_number_node_map(&ast_after_payload);
            let changed_paths = numeric_before
                .iter()
                .filter_map(|(node_path, before_value)| {
                    let after_value = numeric_after.get(node_path)?;
                    (after_value != before_value).then_some(node_path.to_string())
                })
                .collect::<Vec<_>>();

            assert!(after_source.contains("; keep formatting + comment"));
            assert_eq!(changed_paths, vec![path]);
            assert_eq!(
                numeric_after.get(&changed_paths[0]).map(String::as_str),
                Some("0.45")
            );
            assert_ne!(after_source, before_source);
        });
    }

    #[test]
    fn ecky_ast_set_number_wrapper_rejects_stale_source_digest() {
        run_async_test_with_large_stack(|| async {
            let source =
                include_str!("../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
            let session_id = "session-ast-stale-digest";
            let server = test_dispatch_server(source, session_id).await;

            let ast_payload = parse_mcp_tool_payload(
                &dispatch_tool_call_jsonrpc(
                    &server,
                    session_id,
                    "ecky_ast_get",
                    json!({
                        "threadId": "thread-1",
                        "messageId": "msg-1",
                        "path": "/",
                        "depth": 16,
                        "maxNodes": 500,
                        "includeSource": true
                    }),
                )
                .await,
            );
            let (path, expected_node_digest, _) = part_number_nodes(&ast_payload)
                .into_iter()
                .next()
                .expect("part number node");

            let response = dispatch_tool_call_jsonrpc(
                &server,
                session_id,
                "ecky_ast_set_number",
                json!({
                    "threadId": "thread-1",
                    "messageId": "msg-1",
                    "sourceDigest": "sha256:stale",
                    "path": &path,
                    "expectedNodeDigest": &expected_node_digest,
                    "value": 0.45
                }),
            )
            .await;
            let result = response.result.as_ref().expect("json-rpc result");
            assert_eq!(result["isError"], true);
            let err_payload = parse_mcp_tool_payload(&response);
            assert!(err_payload["message"]
                .as_str()
                .unwrap_or_default()
                .contains("digest mismatch"));
        });
    }

    #[tokio::test]
    async fn ecky_ast_get_node_tool_path_resolves_by_path_and_stable_key_with_bounded_slice() {
        let source = "(model\n  ; bounded source test\n  (part body (box 1 2 3)))";
        let session_id = "session-ast-get-node";
        let server = test_dispatch_server(source, session_id).await;
        let path = "/parts/body/root/call/args/1";

        let by_path_payload = parse_mcp_tool_payload(
            &dispatch_tool_call_jsonrpc(
                &server,
                session_id,
                "ecky_ast_get_node",
                json!({
                    "threadId": "thread-1",
                    "messageId": "msg-1",
                    "path": &path,
                    "includeSource": true
                }),
            )
            .await,
        );
        assert_eq!(by_path_payload["requestedPath"], path);
        let path_nodes = by_path_payload["nodes"].as_array().expect("nodes");
        assert_eq!(path_nodes.len(), 1);
        assert_eq!(path_nodes[0]["path"], path);
        let stable_node_key = path_nodes[0]["stableNodeKey"]
            .as_str()
            .expect("stable key")
            .to_string();
        let source_slice = ecky_node_source_text(&path_nodes[0]);
        assert!(source_slice.contains("2"));
        assert!(source_slice.len() <= source.len());

        let by_key_payload = parse_mcp_tool_payload(
            &dispatch_tool_call_jsonrpc(
                &server,
                session_id,
                "ecky_ast_get_node",
                json!({
                    "threadId": "thread-1",
                    "messageId": "msg-1",
                    "stableNodeKey": stable_node_key,
                    "includeSource": true
                }),
            )
            .await,
        );
        assert_eq!(by_key_payload["requestedPath"], path);
        let key_nodes = by_key_payload["nodes"].as_array().expect("nodes");
        assert_eq!(key_nodes.len(), 1);
        assert_eq!(key_nodes[0]["path"], path);
        assert_eq!(ecky_node_source_text(&key_nodes[0]), source_slice);
    }

    #[test]
    fn api_key_mode_blocks_external_mcp_tools() {
        let config = test_api_key_config();

        assert_eq!(
            app_mode_blocks_external_mcp_tools(&config),
            Some("app is in api key mode. External MCP tools are disabled.")
        );
        assert!(!tool_allowed_while_external_mcp_blocked("session_log_in"));

        let err = ensure_mcp_tool_allowed_for_app_mode(&config, "session_log_in")
            .expect_err("session_log_in should be blocked in api key mode");
        assert_eq!(err.code, AppErrorCode::Conflict);
        assert_eq!(
            err.message,
            "app is in api key mode. External MCP tools are disabled."
        );
    }

    #[test]
    fn health_check_and_logout_stay_allowed_while_blocked() {
        let config = test_api_key_config();

        ensure_mcp_tool_allowed_for_app_mode(&config, "health_check")
            .expect("health_check should stay allowed");
        ensure_mcp_tool_allowed_for_app_mode(&config, "session_log_out")
            .expect("session_log_out should stay allowed");
    }

    #[test]
    fn explicit_mcp_mode_keeps_tools_enabled() {
        let mut config = test_api_key_config();
        config.connection_type = Some("mcp".to_string());

        assert_eq!(app_mode_blocks_external_mcp_tools(&config), None);
        ensure_mcp_tool_allowed_for_app_mode(&config, "session_log_in")
            .expect("session_log_in should stay allowed in mcp mode");
    }

    #[test]
    fn legacy_local_config_without_connection_type_is_treated_as_api_key_mode() {
        let mut config = test_api_key_config();
        config.connection_type = None;

        let err = ensure_mcp_tool_allowed_for_app_mode(&config, "thread_list")
            .expect_err("legacy local configs should block external MCP tools");
        assert_eq!(
            err.message,
            "app is in api key mode. External MCP tools are disabled."
        );
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
    fn tool_definitions_include_concept_preview_save_without_generate() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names.iter().any(|name| name == "concept_preview_save"));
        assert!(!tool_names
            .iter()
            .any(|name| name == "concept_preview_generate"));
    }

    #[test]
    fn tool_definitions_include_target_read_split_tools() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names.iter().any(|name| name == "target_meta_get"));
        assert!(tool_names.iter().any(|name| name == "target_macro_get"));
        assert!(tool_names.iter().any(|name| name == "macro_buffer_get"));
        assert!(tool_names
            .iter()
            .any(|name| name == "macro_buffer_replace_range"));
        assert!(tool_names
            .iter()
            .any(|name| name == "macro_buffer_apply_patch"));
        assert!(tool_names
            .iter()
            .any(|name| name == "macro_buffer_preview_render"));
        assert!(tool_names.iter().any(|name| name == "target_detail_get"));
        assert!(tool_names
            .iter()
            .any(|name| name == "artifact_manifest_get"));
        assert!(tool_names.iter().any(|name| name == "target_get"));
        assert!(tool_names
            .iter()
            .any(|name| name == "macro_buffer_replace_and_preview"));
        assert!(!tool_names
            .iter()
            .any(|name| name == "ecky_ast_patch_validate"));
    }

    #[test]
    fn ast_authoring_tool_definitions_swap_buffer_tools_for_ast_tool() {
        let tool_names = tool_definitions_with_ast_enabled(true)
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names.iter().any(|name| name == "ecky_ast_get"));
        assert!(tool_names.iter().any(|name| name == "ecky_ast_inspect"));
        assert!(tool_names.iter().any(|name| name == "ecky_ast_get_node"));
        assert!(tool_names
            .iter()
            .any(|name| name == "ecky_ast_patch_validate"));
        assert!(tool_names
            .iter()
            .any(|name| name == "ecky_ast_replace_and_render"));
        assert!(tool_names
            .iter()
            .any(|name| name == "ecky_ast_patch_preview"));
        assert!(tool_names
            .iter()
            .any(|name| name == "ecky_ast_patch_commit"));
        assert!(tool_names.iter().any(|name| name == "ecky_ast_set_number"));
        assert!(tool_names.iter().any(|name| name == "ecky_ast_set_string"));
        assert!(tool_names.iter().any(|name| name == "ecky_ast_set_select"));
        assert!(tool_names
            .iter()
            .any(|name| name == "ecky_ast_replace_call"));
        assert!(tool_names
            .iter()
            .any(|name| name == "ecky_ast_insert_binding"));
        assert!(tool_names
            .iter()
            .any(|name| name == "ecky_ast_delete_binding"));
        assert!(tool_names
            .iter()
            .any(|name| name == "ecky_ast_rename_binding_scoped"));
        assert!(!tool_names.iter().any(|name| name == "macro_buffer_get"));
        assert!(!tool_names
            .iter()
            .any(|name| name == "macro_buffer_replace_range"));
        assert!(!tool_names
            .iter()
            .any(|name| name == "macro_buffer_replace_and_preview"));
    }

    #[test]
    fn ast_patch_tools_accept_stable_node_key_and_optional_path() {
        let tools = tool_definitions_with_ast_enabled(true);
        for tool_name in ["ecky_ast_patch_validate", "ecky_ast_replace_and_render"] {
            let tool = tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(tool_name))
                .expect("tool");
            let properties = tool["inputSchema"]["properties"]
                .as_object()
                .expect("properties");
            assert!(properties.contains_key("stableNodeKey"));
            assert!(properties.contains_key("path"));
            let required = tool["inputSchema"]["required"]
                .as_array()
                .expect("required")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            assert!(!required.contains(&"path"));
        }
    }

    #[test]
    fn tool_definitions_include_thread_create() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names.iter().any(|name| name == "thread_create"));
        assert!(!tool_names
            .iter()
            .any(|name| name == "thread_authoring_context_set"));
    }

    #[test]
    fn empty_thread_guidance_uses_config_session_defaults() {
        let thread = crate::contracts::Thread {
            id: "thread-1".to_string(),
            title: "Blank".to_string(),
            summary: String::new(),
            messages: Vec::new(),
            updated_at: now_secs(),
            genie_traits: None,
            version_count: 0,
            pending_count: 0,
            queued_count: 1,
            error_count: 0,
            status: crate::contracts::ThreadStatus::default(),
            finalized_at: None,
            pending_confirm: None,
        };

        let control_surface = workspace_control_surface_for_empty_thread(&thread);
        let hints = control_surface.hints.join("\n");

        assert!(hints.contains("config/session defaults"));
        assert!(!hints.contains("thread metadata"));
    }

    #[test]
    fn ecky_ast_set_select_literal_conversion_supports_scalar_values() {
        assert_eq!(
            ecky_literal_from_json(&Value::String("inner".to_string())).expect("string literal"),
            "\"inner\""
        );
        assert_eq!(
            ecky_literal_from_json(&json!(0.35)).expect("number literal"),
            "0.35"
        );
        assert_eq!(
            ecky_literal_from_json(&json!(true)).expect("bool true"),
            "#t"
        );
        assert_eq!(
            ecky_literal_from_json(&json!(false)).expect("bool false"),
            "#f"
        );
    }

    #[test]
    fn ecky_ast_set_select_literal_conversion_rejects_non_scalars() {
        let err = ecky_literal_from_json(&json!({"k":"v"})).expect_err("object should fail");
        assert_eq!(err.code, crate::contracts::AppErrorCode::Validation);
        assert!(err
            .message
            .contains("set_select value must be string, number, or boolean"));
    }

    #[test]
    fn legacy_stable_node_key_path_hint_extracts_supported_forms() {
        assert_eq!(
            legacy_stable_node_key_path_hint("/parts/body/root"),
            Some("/parts/body/root".to_string())
        );
        assert_eq!(
            legacy_stable_node_key_path_hint("legacy|path=/params/lens_bore_d|span=10:20"),
            Some("/params/lens_bore_d".to_string())
        );
        assert_eq!(
            legacy_stable_node_key_path_hint("nodePath=/parts/body/root/call/args/0"),
            Some("/parts/body/root/call/args/0".to_string())
        );
        assert_eq!(
            legacy_stable_node_key_path_hint("spanPath=/parts/body/root;start=1;end=2"),
            Some("/parts/body/root".to_string())
        );
    }

    #[test]
    fn legacy_stable_node_key_path_hint_returns_none_for_unknown_payload() {
        assert_eq!(legacy_stable_node_key_path_hint("sha256:abc"), None);
        assert_eq!(
            legacy_stable_node_key_path_hint("legacy|path=params/lens_bore_d"),
            None
        );
    }

    #[tokio::test]
    async fn cached_user_message_target_falls_back_to_thread_resolution() {
        let conn = crate::db::init_db(&test_db_path("cached-user-target")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let root = std::env::temp_dir().join(format!("ecky-mcp-server-root-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestPathResolver { root };
        let now = now_secs();
        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-1", "Thread", now, None).unwrap();
            db::add_message(
                &conn,
                "thread-1",
                &crate::models::Message {
                    id: "user-1".to_string(),
                    role: crate::models::MessageRole::User,
                    content: "make a thing".to_string(),
                    status: crate::models::MessageStatus::Working,
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
        }
        state.mcp_sessions.lock().await.insert(
            "session-1".to_string(),
            McpSessionState {
                client_kind: "mcp-http".to_string(),
                host_label: "Codex".to_string(),
                agent_label: "codex".to_string(),
                llm_model_id: None,
                llm_model_label: None,
                bound_thread_id: Some("thread-1".to_string()),
                last_target: Some(McpTargetRef {
                    thread_id: "thread-1".to_string(),
                    message_id: "user-1".to_string(),
                    model_id: None,
                }),
                phase: Some("working".to_string()),
                status_text: None,
                busy: true,
                activity_label: None,
                activity_started_at: None,
                attention_kind: None,
                waiting_on_prompt: false,
                current_turn_id: None,
                current_turn_thread_id: Some("thread-1".to_string()),
                current_turn_working_message_ids: vec!["user-1".to_string()],
                current_turn_working_version_message_id: None,
                updated_at: now,
            },
        );

        let err = resolve_target_for_session(&state, &resolver, "session-1", None, None)
            .await
            .expect_err("blank thread has no editable target");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("has no successful versions"));
        assert!(!err.message.contains("Message user-1 not found"));
    }

    #[tokio::test]
    async fn freecad_library_import_persists_imported_version_into_thread() {
        let conn = crate::db::init_db(&test_db_path("library-import-thread")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let source_root =
            std::env::temp_dir().join(format!("ecky-mcp-library-source-{}", Uuid::new_v4()));
        let app_root =
            std::env::temp_dir().join(format!("ecky-mcp-library-app-{}", Uuid::new_v4()));
        let source_dir = source_root.join("Generic objects");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::write(
            source_dir.join("30mm_button_generic.stl"),
            b"solid button\nendsolid button\n",
        )
        .unwrap();

        let search = FreecadLibrarySearchRequest {
            query: "30mm button".to_string(),
            roots: vec![source_root.to_string_lossy().to_string()],
            limit: Some(5),
            include_architecture: false,
        };
        let item = crate::freecad_library::search_freecad_library(&search, &[])
            .unwrap()
            .remove(0);
        let request = FreecadLibraryImportRequest {
            item,
            thread_id: None,
            title: None,
        };
        let resolver = TestPathResolver {
            root: app_root.clone(),
        };
        let bundle = crate::freecad_library::import_mesh_from_request(&request, &resolver).unwrap();
        let manifest =
            crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id).unwrap();

        let (response, target) = persist_freecad_library_import_version(
            &state, &resolver, &request, bundle, manifest, None,
        )
        .await
        .unwrap();

        let thread_id = response["threadId"].as_str().expect("thread id");
        let message_id = response["messageId"].as_str().expect("message id");
        assert_eq!(target.thread_id, thread_id);
        assert_eq!(target.message_id, message_id);
        let db = state.db.lock().await;
        let title = db::get_thread_title(&db, thread_id)
            .unwrap()
            .expect("thread");
        assert_eq!(title, "30mm button generic");
        let message = db::get_thread_message_version(&db, thread_id, message_id)
            .unwrap()
            .expect("message");
        assert_eq!(message.role, crate::models::MessageRole::Assistant);
        assert!(message.artifact_bundle.is_some());
        assert!(message.model_manifest.is_some());

        let _ = std::fs::remove_dir_all(source_root);
        let _ = std::fs::remove_dir_all(app_root);
    }

    #[test]
    fn tool_definitions_include_thread_borrow() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names.iter().any(|name| name == "thread_borrow"));
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
    fn tool_definitions_include_printability_transform_recipes_get() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names
            .iter()
            .any(|name| name == "printability_transform_recipes_get"));
    }

    #[test]
    fn tool_definitions_include_semantic_transform_preview() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        assert!(tool_names
            .iter()
            .any(|name| name == "semantic_transform_preview"));
    }

    #[test]
    fn semantic_transform_preview_schema_requires_content_hash_guard() {
        let tools = tool_definitions();
        let tool = tools
            .iter()
            .find(|tool| {
                tool.get("name").and_then(Value::as_str) == Some("semantic_transform_preview")
            })
            .expect("semantic transform preview tool");
        let required = tool["inputSchema"]["properties"]["expectedArtifact"]["required"]
            .as_array()
            .expect("expectedArtifact required fields")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();

        assert!(required.contains(&"modelId"));
        assert!(required.contains(&"previewStlPath"));
        assert!(required.contains(&"contentHash"));
    }

    #[test]
    fn tool_definitions_are_all_dispatched() {
        let defined = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<std::collections::BTreeSet<_>>();
        let dispatched = dispatched_tool_names()
            .into_iter()
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<_>>();

        let missing = defined.difference(&dispatched).cloned().collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "tool_definitions advertised tools without dispatch handlers: {:?}",
            missing
        );
    }

    #[tokio::test]
    async fn ecky_authoring_tools_require_guide_reads_before_source_changes() {
        let state = test_mcp_engine_state("openai", "gpt-5.4");

        let err = ensure_authoring_guides_read(
            &state,
            "session-1",
            crate::models::SourceLanguage::EckyIrV0,
            crate::models::GeometryBackend::Build123d,
            "macro_preview_render",
        )
        .await
        .expect_err("ecky source edits should be blocked until guides are read");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("Read required MCP resources"));
        assert!(err.message.contains("ecky://guides/ecky-source"));
        assert!(!err.message.contains("ecky://guides/authoring-card"));
        assert!(!err.message.contains("ecky://guides/modeling-guidelines"));
        assert!(!err.message.contains("ecky://guides/build123d"));
        assert!(!err
            .message
            .contains("ecky://guides/surface-manifest/build123d"));
        assert!(!err
            .message
            .contains("ecky://guides/surface-reference/build123d"));

        for uri in required_authoring_guide_uris(
            crate::models::SourceLanguage::EckyIrV0,
            crate::models::GeometryBackend::Build123d,
        ) {
            mark_session_resource_read(&state, "session-1", uri).await;
        }

        ensure_authoring_guides_read(
            &state,
            "session-1",
            crate::models::SourceLanguage::EckyIrV0,
            crate::models::GeometryBackend::Build123d,
            "macro_preview_render",
        )
        .await
        .expect("guide reads should unlock ecky source edits");
    }

    #[tokio::test]
    async fn legacy_ecky_source_resource_alias_satisfies_authoring_gate() {
        let state = test_mcp_engine_state("openai", "gpt-5.4");

        mark_session_resource_read(&state, "session-1", "ecky://guides/ecky-ir-v0").await;

        ensure_authoring_guides_read(
            &state,
            "session-1",
            crate::models::SourceLanguage::EckyIrV0,
            crate::models::GeometryBackend::Freecad,
            "macro_buffer_preview_render",
        )
        .await
        .expect("legacy resource alias should count as canonical ecky source guide");
    }

    #[tokio::test]
    async fn non_ecky_source_edits_do_not_require_ecky_guide_stack() {
        let state = test_mcp_engine_state("openai", "gpt-5.4");

        ensure_authoring_guides_read(
            &state,
            "session-1",
            crate::models::SourceLanguage::LegacyPython,
            crate::models::GeometryBackend::Freecad,
            "macro_preview_render",
        )
        .await
        .expect("legacy source edits should not require ecky guide resources");
    }

    #[tokio::test]
    async fn mcp_http_sessions_bypass_resource_read_guard_for_ecky_authoring_tools() {
        let state = test_mcp_engine_state("openai", "gpt-5.4");
        state.mcp_sessions.lock().await.insert(
            "session-http".to_string(),
            McpSessionState::new("mcp-http".to_string(), "Codex".to_string()),
        );

        ensure_authoring_guides_read(
            &state,
            "session-http",
            crate::models::SourceLanguage::EckyIrV0,
            crate::models::GeometryBackend::Build123d,
            "macro_preview_render",
        )
        .await
        .expect("tool-only mcp-http sessions cannot satisfy resources/read guard");
    }

    #[test]
    fn tool_descriptions_explain_step_artifact_truth() {
        let tools = tool_definitions();
        let target_meta = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("target_meta_get"))
            .expect("target_meta_get tool");
        let description = target_meta
            .get("description")
            .and_then(Value::as_str)
            .expect("target_meta_get description");
        assert!(description.contains("hasArtifactBundle"));
        assert!(description.contains("hasRuntimeManifest"));
        assert!(description.contains("edgeTargetCount"));
        assert!(description.contains("faceTargetCount"));
        assert!(description.contains("hasStepExport"));
        assert!(description.contains("stepExportPath"));
        assert!(description.contains("artifact_manifest_get"));

        let target_get = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("target_get"))
            .expect("target_get tool");
        let description = target_get
            .get("description")
            .and_then(Value::as_str)
            .expect("target_get description");
        assert!(description.contains("artifactDigest"));
        assert!(description.contains("Do not promise STEP"));

        let target_macro = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("target_macro_get"))
            .expect("target_macro_get tool");
        assert!(target_macro
            .get("description")
            .and_then(Value::as_str)
            .expect("target_macro_get description")
            .contains("artifactDigest"));

        let macro_buffer = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("macro_buffer_get"))
            .expect("macro_buffer_get tool");
        assert!(macro_buffer
            .get("description")
            .and_then(Value::as_str)
            .expect("macro_buffer_get description")
            .contains("artifactDigest"));

        for name in [
            "params_preview_render",
            "macro_preview_render",
            "macro_buffer_preview_render",
            "macro_buffer_replace_and_preview",
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
                .unwrap_or_else(|| panic!("{name} tool"));
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{name} description"));
            assert!(description.contains("artifactDigest"), "{name}");
            assert!(description.contains("hasStepExport"), "{name}");
        }

        let target_detail = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("target_detail_get"))
            .expect("target_detail_get tool");
        let description = target_detail
            .get("description")
            .and_then(Value::as_str)
            .expect("target_detail_get description");

        assert!(description.contains("artifactBundle"));
        assert!(description.contains("geometryBackend"));
        assert!(description.contains("exportFormats"));
        assert!(description.contains("hasStepExport"));
        assert!(description.contains("stepExportPath"));
        assert!(description.contains("Do not promise STEP"));
        assert!(description.contains("exportArtifacts contains format=step"));

        let artifact_manifest = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("artifact_manifest_get"))
            .expect("artifact_manifest_get tool");
        let description = artifact_manifest
            .get("description")
            .and_then(Value::as_str)
            .expect("artifact_manifest_get description");
        assert!(description.contains("machine-readable"));
        assert!(description.contains("artifactBundle"));
        assert!(description.contains("modelManifest"));
        assert!(description.contains("runtimeManifestValid"));

        let verification = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("verify_generated_model"))
            .expect("verify_generated_model tool");
        let description = verification
            .get("description")
            .and_then(Value::as_str)
            .expect("verify_generated_model description");
        assert!(description.contains("artifactDigest"));

        let printability = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("printability_analyze"))
            .expect("printability_analyze tool");
        let description = printability
            .get("description")
            .and_then(Value::as_str)
            .expect("printability_analyze description");
        assert!(description.contains("Read-only"));
        assert!(description.contains("preview STL"));
        assert!(description.contains("artifactDigest"));
    }

    #[test]
    fn guidance_prefers_meta_macro_and_detail_over_target_get() {
        let state = test_state();
        let workflow = workflow_guide_text(&state);
        let brief = workspace_overview_brief(
            &state,
            Some(crate::models::SourceLanguage::EckyIrV0),
            Some(crate::models::GeometryBackend::Build123d),
        );

        assert!(workflow.contains("ecky://guides/ecky-source"));
        assert!(workflow.contains("Ecky authoring card"));
        assert!(workflow.contains("(extrude (polygon"));
        assert!(workflow.contains("let*"));
        assert!(workflow.contains("macro_preview_render"));
        assert!(workflow.contains("thread_create"));
        assert!(workflow.contains("thread_borrow"));
        assert!(workflow.contains("resources/read"));
        assert!(workflow.contains("sourceLanguage=ecky"));
        assert!(workflow.contains("primaryGuideUri"));
        assert!(workflow.contains("compatibilityManifestUri"));
        assert!(workflow.contains("backend is a lowerer"));
        assert!(workflow.contains("prose backend guide"));
        assert!(workflow.contains("JSON surface manifests are authoritative"));
        assert!(workflow.contains("ecky://guides/surface-manifest/build123d"));
        assert!(workflow.contains("ecky://guides/surface-manifest/freecad"));
        assert!(workflow.contains("ecky://guides/surface-manifest/ecky-rust"));
        assert!(workflow.contains("parse -> expand -> typecheck -> lower -> validate"));
        assert!(workflow.contains("Direct OCCT is internal-only today"));
        assert!(workflow.contains("Never promise STEP unless artifact truth proves it"));
        assert!(workflow.contains("`artifact_manifest_get`"));
        assert!(workflow.contains("`target_detail_get(section=\"artifactBundle\")` first"));
        assert!(workflow.contains("hasStepExport=true"));
        assert!(workflow.contains("`exportArtifacts` contains `format=step`"));
        assert!(workflow.contains("full machine-readable artifactBundle/modelManifest JSON"));
        assert!(
            workflow.contains("`target_detail_get(section=\"exportArtifacts\")` for the STEP path")
        );
        assert!(workflow.contains("artifactBundle digest exposes `geometryBackend`, `edgeTargetCount`, `faceTargetCount`, `exportFormats`, `hasStepExport`, and `stepExportPath`"));
        assert!(workflow.contains("structural verification first"));
        assert!(workflow.contains("target_meta_get"));
        assert!(workflow.contains("target_macro_get"));
        assert!(workflow.contains("artifact_manifest_get"));
        assert!(workflow.contains("target_detail_get(section=...)"));
        assert!(workflow.contains("Use target_get only when you truly need the full payload"));
        assert!(workflow.contains("measurement_annotation tools"));
        assert!(workflow.contains("long_action_notice"));
        assert!(!workflow.contains("If needed, call target_get or thread_get"));
        assert!(!workflow.contains("disk"));

        assert!(brief
            .resources
            .iter()
            .any(|resource| resource == "ecky://guides/ecky-source"));
        assert!(brief
            .resources
            .iter()
            .any(|resource| resource == "ecky://guides/authoring-card"));
        assert!(brief
            .resources
            .iter()
            .any(|resource| resource == "ecky://guides/ecky-rust"));
        for uri in [
            "ecky://guides/surface-manifest/build123d",
            "ecky://guides/surface-manifest/freecad",
            "ecky://guides/surface-manifest/ecky-rust",
        ] {
            assert!(brief.resources.iter().any(|resource| resource == uri));
        }
        assert_eq!(brief.source_language, "ecky");
        assert_eq!(brief.geometry_backend, "build123d");
        assert_eq!(brief.primary_guide_uri, "ecky://guides/ecky-source");
        assert_eq!(
            brief.compatibility_manifest_uri.as_deref(),
            Some("ecky://guides/surface-manifest/build123d")
        );
        assert_eq!(
            brief.must_read,
            vec!["ecky://guides/ecky-source".to_string()]
        );
        assert!(brief
            .read_when_needed
            .iter()
            .any(|uri| uri == "ecky://guides/surface-manifest/build123d"));
        assert!(brief
            .read_when_needed
            .iter()
            .any(|uri| uri == "ecky://guides/build123d"));
        assert!(brief.summary.contains("fileExtension=.ecky"));
        assert!(brief.summary.contains("geometryBackend=build123d"));
        assert!(brief.summary.contains("compatibility manifest on demand"));
        assert!(!brief.summary.contains("mesh"));
        assert!(!brief.summary.contains("Compatibility"));
        assert!(brief
            .rules
            .iter()
            .any(|rule| rule.contains("compatibility manifests are on-demand")));
        assert!(brief
            .next_steps
            .iter()
            .any(|step| step.contains("geometryBackend=build123d")));
        let guide_step = brief
            .next_steps
            .iter()
            .find(|step| step.contains("mustRead"))
            .expect("brief guide step should route to mustRead");
        assert!(guide_step.contains("normal authoring"));
        assert!(brief
            .next_steps
            .iter()
            .any(|step| step.contains("compatibilityManifestUri")));
        assert!(brief
            .next_steps
            .iter()
            .any(|step| step.contains("target_meta_get")));
        assert!(brief.rules.len() <= 6, "{:?}", brief.rules);
        assert!(brief.next_steps.len() <= 5, "{:?}", brief.next_steps);
    }

    #[test]
    fn artifact_feature_graph_get_tool_definition_is_read_only_manifest_query() {
        let tools = tool_definitions();
        let tool = tools
            .iter()
            .find(|tool| {
                tool.get("name").and_then(Value::as_str) == Some("artifact_feature_graph_get")
            })
            .expect("artifact_feature_graph_get tool");
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .expect("artifact_feature_graph_get description");

        assert!(description.contains("Read-only"));
        assert!(description.contains("model_runtime"));
        assert!(description.contains("modelId"));
        assert!(description.contains("artifactDigest"));
        assert!(description.contains("featureGraph"));
        assert!(description.contains("correspondenceGraph"));
        assert!(description.contains("Does not edit or render"));
    }

    #[test]
    fn bootstrap_prompt_routes_guides_without_forcing_backend_reads() {
        let state = test_state();
        let prompt = prompt_payload(&state, "bootstrap_ecky").expect("bootstrap prompt");
        let text = prompt
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("content"))
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .expect("bootstrap prompt text");

        assert!(text.contains("sourceLanguage=ecky"));
        assert!(text.contains("primaryGuideUri"));
        assert!(text.contains("mustRead"));
        assert!(text.contains("compatibilityManifestUri"));
        assert!(text.contains("concrete"));
        assert!(text.contains("only after lowerer/render errors"));
        for uri in [
            "ecky://guides/surface-manifest/build123d",
            "ecky://guides/surface-manifest/freecad",
            "ecky://guides/surface-manifest/ecky-rust",
        ] {
            assert!(text.contains(uri), "missing {uri}");
        }
        assert!(!text.contains("read `ecky://guides/ecky-source` first, then the backend guide"));
    }

    #[test]
    fn authoring_card_resource_is_listed_and_readable() {
        let state = test_state();
        let resources = resource_definitions();
        assert!(resources.iter().any(|resource| {
            resource.get("uri").and_then(Value::as_str) == Some("ecky://guides/authoring-card")
        }));

        let guide =
            read_resource_text(&state, "ecky://guides/authoring-card").expect("authoring card");
        assert!(guide.contains("Ecky authoring card"));
        assert!(guide.contains("sourceLanguage=ecky"));
        assert!(guide.contains("geometryBackend"));
        assert!(guide.contains("macro_preview_render"));
        assert!(guide.contains("session config"));
        assert!(!guide.contains("thread config"));
    }

    #[test]
    fn selected_engine_label_deduplicates_provider_prefixed_model_names() {
        let state = test_mcp_engine_state("gemini", "gemini-2.5-flash");
        assert_eq!(selected_engine_label(&state), "gemini-2.5-flash");

        let openai_state = test_mcp_engine_state("openai", "gpt-5.4");
        assert_eq!(selected_engine_label(&openai_state), "gpt-5.4 (openai)");
    }

    #[test]
    fn ecky_ir_resource_exposes_canonical_sample() {
        let state = test_state();
        let ir_guide =
            read_resource_text(&state, "ecky://guides/ecky-source").expect("ir guide resource");

        assert!(ir_guide.contains("(model ...)"));
        assert!(ir_guide.contains("`.ecky`"));
        assert!(ir_guide.contains("fileExtension: `.ecky`."));
        assert!(ir_guide.contains("Current sourceLanguage: `ecky`."));
        assert!(ir_guide.contains("never from thread metadata"));
        assert!(ir_guide.contains("EckyRust is a controlled CAD runtime pipeline"));
        assert!(ir_guide.contains("parse -> expand -> typecheck -> lower -> validate"));
        assert!(ir_guide.contains("direct OCCT is an internal STEP/STL fast path"));
        assert!(ir_guide.contains("Do not promise STEP for every mesh/eckyRust render"));
        assert!(ir_guide.contains("structural verification first"));
        assert!(ir_guide.contains("Typed holes are supported only as CAD-VM planning placeholders"));
        assert!(ir_guide.contains("unfilled holes intentionally reject during render/lowering"));
        assert!(ir_guide.contains("range"));
        assert!(ir_guide.contains("Use `map`/`range` inside `part` geometry/list expressions"));
        assert!(ir_guide.contains("Static tuple destructuring is supported only for `zip`"));
        assert!(ir_guide.contains("Zip destructuring"));
        assert!(ir_guide.contains("`organic-loop`"));
        assert!(ir_guide.contains("`voronoi-cells`"));
        assert!(ir_guide.contains("`lorenz-points`"));
        assert!(ir_guide.contains("`rossler-points`"));
        assert!(ir_guide.contains("`logistic-bifurcation-points`"));
        assert!(ir_guide.contains("`henon-points`"));
        assert!(ir_guide.contains("Bounded literal counts/steps"));
        assert!(ir_guide.contains("Seeded helpers are deterministic"));
        assert!(ir_guide.contains("`wall-pattern`"));
        assert!(ir_guide.contains("`cellular`"));
        assert!(ir_guide.contains("`schwarz-p`"));
        assert!(ir_guide.contains("`schwarz-d`"));
        assert!(ir_guide.contains("`diamond-field`"));
        assert!(ir_guide.contains("`neovius`"));
        assert!(ir_guide.contains("`attractor-field`"));
        assert!(ir_guide.contains("mesh"));
        assert!(resource_definitions()
            .into_iter()
            .any(|resource| resource.get("uri").and_then(Value::as_str)
                == Some("ecky://guides/ecky-source")));
        assert!(resource_definitions()
            .into_iter()
            .any(|resource| resource.get("name").and_then(Value::as_str)
                == Some("Ecky on build123d")));
        assert!(resource_definitions()
            .into_iter()
            .any(|resource| resource.get("uri").and_then(Value::as_str)
                == Some("ecky://guides/ecky-rust")));
        assert!(!resource_definitions()
            .into_iter()
            .any(|resource| resource.get("uri").and_then(Value::as_str)
                == Some("ecky://guides/ecky-ir-v0")));
    }

    fn read_surface_manifest_resource(state: &AppState, uri: &str) -> Value {
        let content = read_resource_content(state, uri).expect("surface manifest resource");
        assert_eq!(content.mime_type, "application/json");
        serde_json::from_str(&content.text).expect("surface manifest json")
    }

    #[test]
    fn mcp_surface_manifest_resources_are_listed_with_json_mime() {
        let resources = resource_definitions();

        for uri in [
            "ecky://guides/surface-manifest/build123d",
            "ecky://guides/surface-manifest/freecad",
            "ecky://guides/surface-manifest/ecky-rust",
        ] {
            let resource = resources
                .iter()
                .find(|resource| resource.get("uri").and_then(Value::as_str) == Some(uri))
                .unwrap_or_else(|| panic!("missing manifest resource: {uri}"));

            assert_eq!(
                resource.get("mimeType").and_then(Value::as_str),
                Some("application/json")
            );
        }
    }

    #[test]
    fn mcp_surface_manifest_resources_read_backend_specific_json() {
        let state = test_state();

        for (uri, backend) in [
            ("ecky://guides/surface-manifest/build123d", "build123d"),
            ("ecky://guides/surface-manifest/freecad", "freecad"),
            ("ecky://guides/surface-manifest/ecky-rust", "mesh"),
        ] {
            let manifest = read_surface_manifest_resource(&state, uri);
            assert_eq!(
                manifest.get("backend").and_then(Value::as_str),
                Some(backend)
            );

            for key in [
                "modelClauses",
                "modelWrappers",
                "expressionForms",
                "numericHelpers",
                "pointListHelpers",
                "booleanHelpers",
                "cadOps",
                "wallPatternModes",
                "typedHolePolicy",
            ] {
                assert!(manifest.get(key).is_some(), "missing {key} in {uri}");
            }
        }

        for uri in [
            "ecky://guides/surface-manifest/build123d",
            "ecky://guides/surface-manifest/freecad",
        ] {
            let manifest = read_surface_manifest_resource(&state, uri);
            let cad_ops = manifest
                .get("cadOps")
                .and_then(Value::as_array)
                .expect("cadOps array");
            let wall_pattern_modes = manifest
                .get("wallPatternModes")
                .and_then(Value::as_array)
                .expect("wallPatternModes array");

            assert!(!cad_ops.iter().any(|op| op.as_str() == Some("wall-pattern")));
            assert!(wall_pattern_modes.is_empty());
        }

        let ecky_rust =
            read_surface_manifest_resource(&state, "ecky://guides/surface-manifest/ecky-rust");
        let ecky_rust_cad_ops = ecky_rust
            .get("cadOps")
            .and_then(Value::as_array)
            .expect("cadOps array");
        let ecky_rust_wall_pattern_modes = ecky_rust
            .get("wallPatternModes")
            .and_then(Value::as_array)
            .expect("wallPatternModes array");

        assert!(ecky_rust_cad_ops
            .iter()
            .any(|op| op.as_str() == Some("wall-pattern")));
        assert!(ecky_rust_wall_pattern_modes
            .iter()
            .any(|mode| mode.as_str() == Some("schwarz-p")));
        assert!(ecky_rust_wall_pattern_modes
            .iter()
            .any(|mode| mode.as_str() == Some("attractor-field")));
        assert!(ecky_rust.get("reference").is_none());
        assert_eq!(
            ecky_rust.get("referenceUri").and_then(Value::as_str),
            Some("ecky://guides/surface-reference/ecky-rust")
        );
        let reference =
            read_surface_manifest_resource(&state, "ecky://guides/surface-reference/ecky-rust");
        let helper_refs = reference
            .get("entries")
            .and_then(Value::as_array)
            .expect("reference entries");
        assert!(helper_refs.iter().any(|entry| {
            entry.get("name").and_then(Value::as_str) == Some("noise2")
                && entry.get("signature").and_then(Value::as_str) == Some("(noise2 x y seed)")
        }));
        assert!(helper_refs.iter().any(|entry| {
            entry.get("name").and_then(Value::as_str) == Some("wall-pattern")
                && entry
                    .get("backendSupport")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .contains("mesh/eckyRust only")
        }));
    }

    #[test]
    fn mcp_surface_reference_resources_are_listed_and_readable() {
        let state = test_state();
        let resources = resource_definitions();

        for (uri, backend, wall_expected) in [
            (
                "ecky://guides/surface-reference/build123d",
                "build123d",
                false,
            ),
            ("ecky://guides/surface-reference/freecad", "freecad", false),
            ("ecky://guides/surface-reference/ecky-rust", "mesh", true),
        ] {
            assert!(resources.iter().any(|resource| {
                resource.get("uri").and_then(Value::as_str) == Some(uri)
                    && resource.get("mimeType").and_then(Value::as_str) == Some("application/json")
            }));

            let content = read_resource_content(&state, uri).expect("surface reference resource");
            assert_eq!(content.mime_type, "application/json");
            let reference: Value =
                serde_json::from_str(&content.text).expect("surface reference json");
            assert_eq!(
                reference.get("backend").and_then(Value::as_str),
                Some(backend)
            );
            let entries = reference
                .get("entries")
                .and_then(Value::as_array)
                .expect("entries");
            for name in ["noise2", "fbm2", "voronoi2", "voronoi-cells"] {
                let entry = entries
                    .iter()
                    .find(|entry| entry.get("name").and_then(Value::as_str) == Some(name))
                    .unwrap_or_else(|| panic!("missing reference entry: {name}"));
                assert!(entry.get("signature").and_then(Value::as_str).is_some());
                assert!(entry.get("description").and_then(Value::as_str).is_some());
                assert!(entry.get("example").and_then(Value::as_str).is_some());
            }
            assert_eq!(
                entries
                    .iter()
                    .any(|entry| entry.get("name").and_then(Value::as_str) == Some("wall-pattern")),
                wall_expected
            );
        }
    }

    #[test]
    fn build123d_resource_exposes_file_hints_without_python_or_mesh_terms() {
        let state = test_state();
        let guide = read_resource_text(&state, "ecky://guides/build123d")
            .expect("build123d guide resource");

        assert!(guide.contains("Current fileExtension: `.ecky`."));
        assert!(guide.contains("Current sourceLanguage: `ecky`."));
        assert!(guide.contains("Target geometryBackend: `build123d`."));
        assert!(guide.contains("Return canonical Ecky source in `macro_code`."));
        assert!(guide.contains("Use `map`/`range` inside `part` geometry/list expressions"));
        assert!(guide.contains("Static tuple destructuring is supported only for `zip`"));
        assert!(guide.contains("Zip destructuring"));
        assert!(guide.contains("Wall-pattern is mesh/eckyRust only"));
        assert!(guide.contains("typed/static errors and structural verification first"));
        assert!(guide.contains("Typed holes are supported only as CAD-VM planning placeholders"));
        assert!(!guide.contains("Python"));
        assert!(!guide.contains("`wall-pattern`"));
        assert!(!guide.contains("`schwarz-p`"));
        assert!(!guide.contains("`schwarz-d`"));
        assert!(!guide.contains("`diamond-field`"));
        assert!(!guide.contains("`neovius`"));
        assert!(!guide.contains("`attractor-field`"));
    }

    #[test]
    fn freecad_resource_exposes_backend_guidance_without_retired_extensions() {
        let state = test_state();
        let guide =
            read_resource_text(&state, "ecky://guides/freecad").expect("freecad guide resource");

        assert!(guide.contains("Current fileExtension: `.ecky`."));
        assert!(guide.contains("Current sourceLanguage: `ecky`."));
        assert!(guide.contains("Target geometryBackend: `freecad`."));
        assert!(guide.contains("Return canonical Ecky source in `macro_code`."));
        assert!(guide.contains("Supported CAD ops for this backend"));
        assert!(guide.contains("Use `map`/`range` inside `part` geometry/list expressions"));
        assert!(guide.contains("Static tuple destructuring is supported only for `zip`"));
        assert!(guide.contains("Zip destructuring"));
        assert!(guide.contains("Wall-pattern is mesh/eckyRust only"));
        assert!(guide.contains("typed/static errors and structural verification first"));
        assert!(guide.contains("Typed holes are supported only as CAD-VM planning placeholders"));
        assert!(!guide.contains("`wall-pattern`"));
        assert!(!guide.contains("`schwarz-p`"));
        assert!(!guide.contains("`schwarz-d`"));
        assert!(!guide.contains("`diamond-field`"));
        assert!(!guide.contains("`neovius`"));
        assert!(!guide.contains("`attractor-field`"));
        assert!(!guide.contains(".frecky"));
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

    fn compact_test_design(macro_code: &str) -> crate::models::DesignOutput {
        crate::models::DesignOutput {
            title: "Render".to_string(),
            version_name: "V-render".to_string(),
            response: "ok".to_string(),
            interaction_mode: crate::models::InteractionMode::Design,
            macro_code: macro_code.to_string(),
            macro_dialect: crate::models::MacroDialect::Legacy,
            engine_kind: crate::models::EngineKind::Freecad,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            ui_spec: crate::models::UiSpec::default(),
            initial_params: std::collections::BTreeMap::from([(
                "diameter".to_string(),
                crate::models::ParamValue::Number(42.0),
            )]),
            post_processing: None,
        }
    }

    fn compact_test_bundle(model_id: &str) -> crate::models::ArtifactBundle {
        crate::models::ArtifactBundle {
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::Freecad,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            content_hash: format!("hash-{model_id}"),
            artifact_version: 1,
            fcstd_path: format!("/tmp/{model_id}.FCStd"),
            manifest_path: format!("/tmp/{model_id}.json"),
            macro_path: Some(format!("/tmp/{model_id}.py")),
            preview_stl_path: format!("/tmp/{model_id}.stl"),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: vec![crate::models::ExportArtifact {
                label: "STEP".to_string(),
                format: "step".to_string(),
                path: format!("/tmp/{model_id}.step"),
                role: "cad-exchange".to_string(),
            }],
        }
    }

    fn compact_test_manifest(model_id: &str) -> crate::models::ModelManifest {
        crate::models::ModelManifest {
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: crate::models::EngineKind::Freecad,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            document: crate::models::DocumentMetadata {
                document_name: "Doc".to_string(),
                document_label: "Doc".to_string(),
                source_path: None,
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: Vec::new(),
            parameter_groups: Vec::new(),
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            feature_graph: None,
            correspondence_graph: None,
            warnings: Vec::new(),
            enrichment_state: crate::models::ManifestEnrichmentState {
                status: crate::models::EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    #[test]
    fn mutation_tool_response_json_omits_heavy_runtime_payloads() {
        let bundle = compact_test_bundle("model-render");
        let digest = ArtifactBundleDigest {
            model_id: "model-render".to_string(),
            content_hash: "hash-model-render".to_string(),
            source_language: "legacyPython".to_string(),
            geometry_backend: "freecad".to_string(),
            has_preview_stl: true,
            viewer_asset_count: 0,
            edge_target_count: 0,
            face_target_count: 0,
            export_format_count: 1,
            export_formats: vec!["step".to_string()],
            has_step_export: true,
            step_export_path: Some("/tmp/model-render.step".to_string()),
            multipart: false,
        };
        let manifest = compact_test_manifest("model-render");
        let design = compact_test_design("render_macro()");

        let macro_response = MacroReplaceResponse {
            thread_id: "thread-1".to_string(),
            message_id: "msg-render".to_string(),
            macro_code: design.macro_code.clone(),
            ui_spec: design.ui_spec.clone(),
            initial_params: design.initial_params.clone(),
            artifact_bundle: bundle.clone(),
            model_manifest: manifest.clone(),
            structural_verification: None,
            artifact_digest: digest.clone(),
        };
        let params_response = ParamsPatchResponse {
            thread_id: "thread-1".to_string(),
            message_id: "msg-render".to_string(),
            merged_params: design.initial_params.clone(),
            artifact_bundle: bundle.clone(),
            model_manifest: manifest.clone(),
            design_output: design.clone(),
            structural_verification: None,
            artifact_digest: digest.clone(),
        };
        let buffer_response = MacroBufferReplaceAndRenderResponse {
            thread_id: "thread-1".to_string(),
            message_id: "msg-render".to_string(),
            digest: "source-digest".to_string(),
            line_count: 1,
            macro_code: design.macro_code,
            ui_spec: design.ui_spec,
            initial_params: design.initial_params,
            artifact_bundle: bundle,
            model_manifest: manifest,
            structural_verification: None,
            artifact_digest: digest,
        };

        for value in [
            compact_macro_replace_response_value(&macro_response),
            compact_params_patch_response_value(&params_response),
            compact_macro_buffer_replace_and_preview_response_value(&buffer_response),
        ] {
            assert_eq!(value["artifactDigest"]["modelId"], "model-render");
            assert!(value.get("artifactBundle").is_none());
            assert!(value.get("modelManifest").is_none());
            assert!(value.get("designOutput").is_none());
            assert!(value.get("macroCode").is_none());
            assert!(value.get("uiSpec").is_none());
            assert!(value.get("initialParams").is_none());
        }
    }

    #[test]
    fn ecky_ast_replace_and_render_response_json_reports_compact_edit_metadata() {
        let bundle = compact_test_bundle("model-render");
        let digest = ArtifactBundleDigest {
            model_id: "model-render".to_string(),
            content_hash: "hash-model-render".to_string(),
            source_language: "ecky".to_string(),
            geometry_backend: "build123d".to_string(),
            has_preview_stl: true,
            viewer_asset_count: 0,
            edge_target_count: 0,
            face_target_count: 0,
            export_format_count: 0,
            export_formats: Vec::new(),
            has_step_export: false,
            step_export_path: None,
            multipart: false,
        };
        let manifest = compact_test_manifest("model-render");
        let mut design = compact_test_design("(model\n  (box 10 20 30))");
        design.source_language = crate::models::SourceLanguage::EckyIrV0;
        design.geometry_backend = crate::models::GeometryBackend::Build123d;

        let response = MacroReplaceResponse {
            thread_id: "thread-1".to_string(),
            message_id: "msg-render".to_string(),
            macro_code: design.macro_code.clone(),
            ui_spec: design.ui_spec,
            initial_params: design.initial_params,
            artifact_bundle: bundle,
            model_manifest: manifest,
            structural_verification: None,
            artifact_digest: digest,
        };

        let value =
            compact_ecky_ast_replace_and_render_response_value(&response, "insertAfter", "body/0");

        assert_eq!(
            value["newSourceDigest"],
            crate::mcp::macro_buffer::source_digest(&response.macro_code)
        );
        assert_eq!(value["editedPath"], "body/0");
        assert_eq!(value["operation"], "insertAfter");
        assert_eq!(value["lineCount"], 2);
        assert!(value.get("macroCode").is_none());
        assert!(value.get("artifactBundle").is_none());
        assert!(value.get("modelManifest").is_none());
        assert!(value.get("uiSpec").is_none());
        assert!(value.get("initialParams").is_none());
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
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::EckyRust,
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
        assert_eq!(
            result["structuredContent"]["image"]["dataUrl"],
            "data:image/jpeg;base64,Zm9v"
        );
        assert_eq!(
            result["structuredContent"]["image"]["mimeType"],
            "image/jpeg"
        );
        assert_eq!(result["structuredContent"]["image"]["base64"], "Zm9v");
    }
}
