use crate::contracts::{
    AppError, AppErrorCode, AppResult, ArtifactBundle, AuthoringTargetRef, Config,
    FreecadLibraryImportRequest, FreecadLibrarySearchRequest, ModelManifest, TargetLeaseInfo,
};
use crate::db;
use crate::mcp::authoring::authoring_card_text;
use crate::mcp::contracts::*;
use crate::mcp::handlers;
use crate::mcp::handlers::AgentContext;
use crate::models::{
    AppState, McpSessionState, McpTargetRef, PathResolver, ViewportScreenshotCapture,
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
const PROVIDER_THREAD_QUERY: &str = "providerThreadId";
const LEASE_TTL_SECS: u64 = 45;
const MCP_PROTOCOL_MODERN: &str = "2026-07-28";
const MCP_PROTOCOL_LEGACY_LATEST: &str = "2025-06-18";
const MCP_PROTOCOL_LEGACY: &str = "2024-11-05";
const MCP_META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const MCP_META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
const MCP_META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const ECKY_CONTEXT_ID: &str = "eckyContextId";
const MODERN_LIST_TTL_MS: u64 = 300_000;
const MCP_TOOL_DISPATCH_STACK_BYTES: usize = 8 * 1024 * 1024;
const MCP_TOOL_DISPATCH_THREAD_NAME: &str = "mcp-tool-dispatch";

async fn run_on_mcp_tool_dispatch_stack<F, Fut, T>(task: F) -> AppResult<T>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = AppResult<T>> + 'static,
    T: Send + 'static,
{
    let runtime = tokio::runtime::Handle::current();
    let (sender, receiver) = oneshot::channel();
    std::thread::Builder::new()
        .name(MCP_TOOL_DISPATCH_THREAD_NAME.to_string())
        .stack_size(MCP_TOOL_DISPATCH_STACK_BYTES)
        .spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.block_on(task())))
                    .unwrap_or_else(|payload| {
                        let detail = payload
                            .downcast_ref::<&str>()
                            .map(|value| (*value).to_string())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "non-string panic payload".to_string());
                        Err(AppError::internal(format!(
                            "MCP tool dispatcher panicked: {detail}"
                        )))
                    });
            let _ = sender.send(result);
        })
        .map_err(|error| {
            AppError::internal(format!(
                "Failed to start MCP tool dispatcher thread: {error}"
            ))
        })?;

    receiver.await.map_err(|_| {
        AppError::internal("MCP tool dispatcher thread terminated without returning a result.")
    })?
}

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
    parameters: Option<crate::contracts::DesignParams>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
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
    parameters: Option<crate::contracts::DesignParams>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
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
    parameters: Option<crate::contracts::DesignParams>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
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
    parameters: Option<crate::contracts::DesignParams>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
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
    parameters: Option<crate::contracts::DesignParams>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
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
    parameters: Option<crate::contracts::DesignParams>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
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
    parameters: Option<crate::contracts::DesignParams>,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
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
    source_language: crate::contracts::SourceLanguage,
    geometry_backend: crate::contracts::GeometryBackend,
    model_stl_path: Option<String>,
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
        let event = crate::models::next_history_changed_event(None, None, "mcp");
        let _ = handle.emit("history-updated", event);
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

fn json_rpc_error_data(
    id: Option<Value>,
    code: i32,
    message: impl Into<String>,
    data: Value,
) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.into(),
            data: Some(data),
        }),
        id,
    }
}

fn modern_server_info() -> Value {
    json!({
        "name": "ecky-mcp",
        "version": env!("CARGO_PKG_VERSION")
    })
}

fn modernize_result(response: &mut JsonRpcResponse) {
    let Some(result) = response.result.as_mut().and_then(Value::as_object_mut) else {
        return;
    };
    result
        .entry("resultType".to_string())
        .or_insert_with(|| json!("complete"));
    let meta = result
        .entry("_meta".to_string())
        .or_insert_with(|| json!({}));
    if let Some(meta) = meta.as_object_mut() {
        meta.entry("io.modelcontextprotocol/serverInfo".to_string())
            .or_insert_with(modern_server_info);
    }
}

/// Above this serialized size (Unicode characters) an ordinary generic MCP
/// result is considered large: `structuredContent` keeps the full canonical
/// value available but the envelope surfaces observed/total counts and a
/// continuation hint steering callers toward bounded section/window reads.
/// Tunable in one place; intentionally generous so real authoring tool
/// results stay canonical while pathological payloads are flagged.
const MCP_RESPONSE_BUDGET_CHARS: usize = 32_000;

/// Hard transport safety ceiling. A generic result larger than this cannot be
/// shipped honestly, so the envelope fails with observed/allowed sizes instead
/// of silently truncating authoritative state. Far above the response budget so
/// every realistic tool result passes while runaway payloads are caught.
const MCP_TRANSPORT_LIMIT_CHARS: usize = 16_000_000;

/// Build a short, content-free text summary of a generic tool result: identity
/// keys (threadId/messageId/modelId when present), the top-level key list, and
/// array lengths. Never includes scalar values beyond identity or any array
/// item bodies, so it cannot leak large payloads (e.g. screenshot/source bytes).
fn concise_tool_summary(value: &Value) -> String {
    let identity = ["threadId", "messageId", "modelId"]
        .into_iter()
        .filter_map(|key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(|v| format!("{key}={v}"))
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut keys: Vec<String> = Vec::new();
    if let Some(obj) = value.as_object() {
        for (key, val) in obj {
            let len = val.as_array().map(|items| items.len());
            keys.push(match len {
                Some(count) => format!("{key}[{count}]"),
                None => key.clone(),
            });
        }
    }
    let mut summary = if identity.is_empty() {
        format!("Tool result. Keys: {}.", keys.join(", "))
    } else {
        format!("Tool result for {}. Keys: {}.", identity, keys.join(", "))
    };
    // Hard ceiling (char-safe, never splits a multi-byte UTF-8 sequence) so
    // the summary can never itself become a large payload.
    if summary.chars().count() > 500 {
        summary = format!("{}…", summary.chars().take(499).collect::<String>());
    }
    summary
}

/// Shared response-budget/continuation metadata for bounded MCP reads (the
/// generic success envelope plus the source/messages/AST/manifest/target
/// section-read tools). Carries observed/total counts and a continuation hint
/// only — never the payload bytes.
fn mcp_read_continuation(observed: usize, budget: usize) -> Value {
    json!({
        "large": true,
        "budgetChars": budget,
        "observedCount": observed,
        "recommendation": "Payload exceeded the MCP response-budget guidance. \
    structuredContent.data carries the complete canonical value; for model-context \
    efficiency prefer bounded section/window reads (target_macro_get, target_detail_get, \
    artifact_manifest_get, thread_messages_get, ecky_ast_get_node)."
    })
}

/// True when `structuredContent` is the large-read envelope shape (canonical
/// data nested under `data` alongside observedCount/continuation metadata)
/// rather than an ordinary value carried directly.
#[cfg(test)]
fn structured_content_is_bounded_read(structured: &Value) -> bool {
    structured.get("observedCount").is_some() && structured.get("continuation").is_some()
}

fn mcp_tool_success(id: Option<Value>, value: &Value) -> JsonRpcResponse {
    // Rich pre-built content payloads (tools that compose their own `content`
    // array — e.g. screenshot image+text, prompt attachments) pass through
    // unchanged so their content ordering and any structuredContent survive.
    if value
        .get("content")
        .map(|content| content.is_array())
        .unwrap_or(false)
    {
        return json_rpc_result(id, value.clone());
    }

    let observed = serde_json::to_string(value)
        .unwrap_or_default()
        .chars()
        .count();

    // Transport safety: never silently ship a runaway payload. Fail honestly
    // with observed/allowed sizes so the caller can re-request a bounded read.
    if observed > MCP_TRANSPORT_LIMIT_CHARS {
        return mcp_tool_error(
            id,
            &AppError::with_details(
                AppErrorCode::Validation,
                format!(
                    "MCP tool result exceeds the transport safety limit: observed {observed} \
chars, allowed {MCP_TRANSPORT_LIMIT_CHARS}."
                ),
                format!(
                    "observedCount={observed}; allowedCount={MCP_TRANSPORT_LIMIT_CHARS}; \
re-request with a bounded section/window read."
                ),
            ),
        );
    }

    let summary = concise_tool_summary(value);

    // Ordinary result: canonical machine output in structuredContent, concise
    // text summary in content (no pretty-printed JSON duplication).
    if observed <= MCP_RESPONSE_BUDGET_CHARS {
        return json_rpc_result(
            id,
            json!({
                "content": [
                    { "type": "text", "text": summary }
                ],
                "structuredContent": value,
            }),
        );
    }

    // Large result: structuredContent still carries the complete canonical
    // value (under `data`, so complete data remains available) plus the shared
    // observed/returned/total counts and a continuation hint. Text stays a
    // concise summary; nothing is silently truncated.
    let continuation = mcp_read_continuation(observed, MCP_RESPONSE_BUDGET_CHARS);
    json_rpc_result(
        id,
        json!({
            "content": [
                {
                    "type": "text",
                    "text": format!(
                        "{summary}  (large result: {observed} chars; full value in \
        structuredContent.data; prefer bounded section/window reads)"
                    )
                }
            ],
            "structuredContent": {
                "data": value,
                "observedCount": observed,
                "returnedCount": observed,
                "totalCount": observed,
                "truncated": false,
                "continuation": continuation,
            },
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
            // Byte-free image metadata only: the base64 payload lives once, in
            // the single MCP image `content` item above. Carrying `dataUrl` or a
            // duplicate `base64` here would triplicate the screenshot bytes.
            "image": {
                "mimeType": mime_type,
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
        Some(MCP_PROTOCOL_LEGACY_LATEST) => MCP_PROTOCOL_LEGACY_LATEST,
        Some(MCP_PROTOCOL_LEGACY) => MCP_PROTOCOL_LEGACY,
        _ => MCP_PROTOCOL_LEGACY_LATEST,
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

#[derive(Debug, Clone)]
struct ModernRequestMetadata {
    client_name: String,
}

fn request_meta(req: &JsonRpcRequest) -> Option<&serde_json::Map<String, Value>> {
    req.params
        .as_ref()
        .and_then(|params| params.get("_meta"))
        .and_then(Value::as_object)
}

fn request_uses_modern_mcp(headers: &HeaderMap, req: &JsonRpcRequest) -> bool {
    request_meta(req)
        .and_then(|meta| meta.get(MCP_META_PROTOCOL_VERSION))
        .is_some()
        || headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.trim() == MCP_PROTOCOL_MODERN)
        || req.method == "server/discover"
}

fn decoded_mcp_header_value(value: &str) -> Option<String> {
    let value = value.trim();
    let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|value| value.strip_suffix("?="))
    else {
        return Some(value.to_string());
    };
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

fn mirrored_request_name(req: &JsonRpcRequest) -> Option<&str> {
    let params = req.params.as_ref()?;
    match req.method.as_str() {
        "tools/call" | "prompts/get" => params.get("name").and_then(Value::as_str),
        "resources/read" => params.get("uri").and_then(Value::as_str),
        _ => None,
    }
}

fn header_mismatch(id: Option<Value>, message: impl Into<String>) -> JsonRpcResponse {
    json_rpc_error(id, -32020, format!("Header mismatch: {}", message.into()))
}

fn validate_modern_request(
    headers: &HeaderMap,
    req: &JsonRpcRequest,
) -> Result<ModernRequestMetadata, JsonRpcResponse> {
    let meta = request_meta(req).ok_or_else(|| {
        json_rpc_error(
            req.id.clone(),
            -32602,
            "Modern MCP requests require params._meta.",
        )
    })?;
    let protocol_version = meta
        .get(MCP_META_PROTOCOL_VERSION)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            json_rpc_error(
                req.id.clone(),
                -32602,
                format!("Missing required _meta field {MCP_META_PROTOCOL_VERSION}."),
            )
        })?;
    if protocol_version != MCP_PROTOCOL_MODERN {
        return Err(json_rpc_error_data(
            req.id.clone(),
            -32022,
            "Unsupported protocol version",
            json!({
                "supported": [MCP_PROTOCOL_MODERN],
                "requested": protocol_version
            }),
        ));
    }
    if !meta
        .get(MCP_META_CLIENT_CAPABILITIES)
        .is_some_and(Value::is_object)
    {
        return Err(json_rpc_error(
            req.id.clone(),
            -32602,
            format!("Missing required _meta field {MCP_META_CLIENT_CAPABILITIES}."),
        ));
    }

    let header_protocol = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok());
    if header_protocol != Some(protocol_version) {
        return Err(header_mismatch(
            req.id.clone(),
            "MCP-Protocol-Version is missing or differs from params._meta.",
        ));
    }
    let header_method = headers
        .get("mcp-method")
        .and_then(|value| value.to_str().ok());
    if header_method != Some(req.method.as_str()) {
        return Err(header_mismatch(
            req.id.clone(),
            "Mcp-Method is missing or differs from the JSON-RPC method.",
        ));
    }
    if let Some(expected_name) = mirrored_request_name(req) {
        let actual_name = headers
            .get("mcp-name")
            .and_then(|value| value.to_str().ok())
            .and_then(decoded_mcp_header_value);
        if actual_name.as_deref() != Some(expected_name) {
            return Err(header_mismatch(
                req.id.clone(),
                "Mcp-Name is missing or differs from params.name/params.uri.",
            ));
        }
    }

    let client_name = meta
        .get(MCP_META_CLIENT_INFO)
        .and_then(Value::as_object)
        .and_then(|info| info.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Modern MCP Client")
        .to_string();
    Ok(ModernRequestMetadata { client_name })
}

async fn create_session(state: &AppState, host_label: String, client_kind: String) -> String {
    let session_id = format!("mcp-http-{}", Uuid::new_v4());
    state
        .mcp_session_registry
        .insert(
            session_id.clone(),
            McpSessionState::new(client_kind, host_label),
        )
        .await;
    session_id
}

fn encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn decode_query_value(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

pub fn provider_bound_endpoint(endpoint_url: &str, thread_id: &str) -> String {
    let separator = if endpoint_url.contains('?') { '&' } else { '?' };
    format!(
        "{endpoint_url}{separator}{PROVIDER_THREAD_QUERY}={}",
        encode_query_value(thread_id)
    )
}

fn provider_thread_id_from_uri(uri: &axum::http::Uri) -> Option<String> {
    uri.query().and_then(|query| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(key, value)| {
                (key == PROVIDER_THREAD_QUERY)
                    .then(|| decode_query_value(value))
                    .flatten()
                    .filter(|value| !value.trim().is_empty())
            })
    })
}

async fn prebind_provider_session(
    state: &AppState,
    session_id: &str,
    thread_id: Option<String>,
) -> AppResult<()> {
    let Some(thread_id) = thread_id else {
        return Ok(());
    };
    {
        let conn = state.db.lock().await;
        if db::get_visible_thread_title(&conn, &thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .is_none()
        {
            return Err(AppError::not_found(format!(
                "Provider MCP target thread {thread_id} was not found."
            )));
        }
    }
    let session = get_session(state, session_id)
        .await
        .ok_or_else(|| AppError::not_found("MCP session not found."))?;
    let context = current_context(session_id, &session);
    handlers::ensure_thread_claim(state, &context, &thread_id, true).await?;
    update_session_state(state, session_id, |session| {
        session.bound_thread_id = Some(thread_id);
        session.client_kind = "provider-mcp-http".to_string();
    })
    .await
}

fn requested_ecky_context_id(req: &JsonRpcRequest) -> Option<String> {
    req.params
        .as_ref()
        .and_then(|params| params.get("arguments"))
        .and_then(|arguments| arguments.get(ECKY_CONTEXT_ID))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn resolve_modern_context(
    state: &AppState,
    metadata: &ModernRequestMetadata,
    requested: Option<String>,
    managed: bool,
) -> AppResult<String> {
    if let Some(context_id) = requested {
        if !context_id.starts_with("ecky-context-") {
            return Err(AppError::validation(
                "eckyContextId is malformed. Start a new context by omitting it.",
            ));
        }
        if get_session(state, &context_id).await.is_none() {
            return Err(AppError::not_found(
                "Ecky MCP context expired or is unknown. Omit eckyContextId to create a new context.",
            ));
        }
        return Ok(context_id);
    }

    let context_id = format!("ecky-context-{}", Uuid::new_v4());
    state
        .mcp_session_registry
        .insert(
            context_id.clone(),
            McpSessionState::new(
                if managed {
                    "managed-mcp-http".to_string()
                } else {
                    "mcp-http".to_string()
                },
                metadata.client_name.clone(),
            ),
        )
        .await;
    Ok(context_id)
}

fn attach_ecky_context_id(value: &mut Value, context_id: &str) {
    if let Some(structured) = value
        .get_mut("structuredContent")
        .and_then(Value::as_object_mut)
    {
        structured.insert(ECKY_CONTEXT_ID.to_string(), json!(context_id));
        return;
    }
    if let Some(object) = value.as_object_mut() {
        object.insert(ECKY_CONTEXT_ID.to_string(), json!(context_id));
    }
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
    state.mcp_session_registry.get(session_id).await
}

async fn update_session_state<F>(state: &AppState, session_id: &str, f: F) -> AppResult<()>
where
    F: FnOnce(&mut McpSessionState),
{
    if state.mcp_session_registry.update(session_id, f).await {
        Ok(())
    } else {
        Err(AppError::not_found("MCP session not found."))
    }
}

async fn set_session_target(state: &AppState, session_id: &str, target: Option<McpTargetRef>) {
    state
        .mcp_session_registry
        .update(session_id, |session| {
            session.last_target = target.clone();
        })
        .await;
    crate::mcp::runtime::associate_session_target(state, session_id, target.as_ref());
}

async fn remove_session(state: &AppState, session_id: &str) -> AppResult<()> {
    // Close pending prompts before removing the session (close_single_prompt needs the session entry).
    state
        .close_prompts_for_session(session_id, "session_disconnected")
        .await;
    state.mcp_session_registry.remove(session_id).await;
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
/// Uses the in-memory MCP session registry as authoritative source of live connections,
/// then fetches full DB records for those IDs.
async fn emit_sessions_changed(state: &AppState, handle: &tauri::AppHandle) {
    use tauri::Emitter;
    let live_ids: Vec<String> = state
        .mcp_session_registry
        .list()
        .await
        .into_iter()
        .map(|(id, _)| id)
        .collect();
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

fn target_resolution_error(
    kind: AuthoringTargetResolutionFailureKind,
    requested_target: AuthoringTargetRef,
    resolved_target: Option<AuthoringTargetRef>,
) -> AppError {
    let evidence = AuthoringTargetResolutionEvidence {
        kind,
        requested_target,
        resolved_target,
    };
    let (code, message) = match kind {
        AuthoringTargetResolutionFailureKind::NotFound => (
            AppErrorCode::NotFound,
            "Requested authoring target was not found.",
        ),
        AuthoringTargetResolutionFailureKind::Stale => (
            AppErrorCode::Conflict,
            "Requested authoring target is stale.",
        ),
    };
    AppError::with_details(
        code,
        message,
        serde_json::to_string(&evidence).unwrap_or_else(|_| "{}".to_string()),
    )
}

fn draft_target_ref(preview: &handlers::SessionRenderPreview) -> AuthoringTargetRef {
    AuthoringTargetRef::Draft {
        thread_id: preview.thread_id.clone(),
        preview_id: preview.preview_id.clone(),
        session_id: preview.session_id.clone(),
    }
}

async fn legacy_authoring_target_ref(
    state: &AppState,
    session_id: &str,
    explicit_thread_id: Option<&str>,
    explicit_message_id: Option<&str>,
) -> AppResult<Option<AuthoringTargetRef>> {
    match (explicit_thread_id, explicit_message_id) {
        (Some(thread_id), Some(message_id)) => {
            let saved_exists = {
                let conn = state.db.lock().await;
                db::get_visible_message_thread_id(&conn, message_id)
                    .map_err(|e| AppError::persistence(e.to_string()))?
                    .is_some()
            };
            if saved_exists {
                return Ok(Some(AuthoringTargetRef::SavedVersion {
                    thread_id: thread_id.to_string(),
                    message_id: message_id.to_string(),
                }));
            }

            if let Some(session) = get_session(state, session_id).await {
                let ctx = current_context(session_id, &session);
                if let Some(preview) = handlers::resolve_session_render_preview_for_preview_id(
                    state,
                    &ctx,
                    Some(thread_id),
                    message_id,
                )
                .await?
                {
                    return Ok(Some(draft_target_ref(&preview)));
                }
            }

            Ok(Some(AuthoringTargetRef::SavedVersion {
                thread_id: thread_id.to_string(),
                message_id: message_id.to_string(),
            }))
        }
        (None, Some(message_id)) => {
            let saved_thread_id = {
                let conn = state.db.lock().await;
                db::get_visible_message_thread_id(&conn, message_id)
                    .map_err(|e| AppError::persistence(e.to_string()))?
            };
            if let Some(thread_id) = saved_thread_id {
                return Ok(Some(AuthoringTargetRef::SavedVersion {
                    thread_id,
                    message_id: message_id.to_string(),
                }));
            }
            if let Some(session) = get_session(state, session_id).await {
                let ctx = current_context(session_id, &session);
                if let Some(preview) = handlers::resolve_session_render_preview_for_preview_id(
                    state, &ctx, None, message_id,
                )
                .await?
                {
                    return Ok(Some(draft_target_ref(&preview)));
                }
                if let Some(thread_id) = session.bound_thread_id {
                    return Ok(Some(AuthoringTargetRef::SavedVersion {
                        thread_id,
                        message_id: message_id.to_string(),
                    }));
                }
            }
            Err(AppError::validation(
                "Legacy messageId needs threadId when no saved version or draft matches it.",
            ))
        }
        (Some(thread_id), None) => Ok(Some(AuthoringTargetRef::LatestSaved {
            thread_id: thread_id.to_string(),
        })),
        (None, None) => {
            let Some(session) = get_session(state, session_id).await else {
                return Ok(None);
            };
            let ctx = current_context(session_id, &session);
            let target =
                handlers::resolve_session_render_preview_for_request(state, &ctx, None, None)
                    .await?
                    .as_ref()
                    .map(draft_target_ref);
            Ok(target)
        }
    }
}

async fn resolve_target_for_session(
    state: &AppState,
    app: &dyn PathResolver,
    session_id: &str,
    explicit_thread_id: Option<String>,
    explicit_message_id: Option<String>,
) -> AppResult<ResolvedTargetRef> {
    let target_ref = legacy_authoring_target_ref(
        state,
        session_id,
        explicit_thread_id.as_deref(),
        explicit_message_id.as_deref(),
    )
    .await?;
    resolve_authoring_target_for_session(state, app, session_id, target_ref).await
}

async fn resolve_authoring_target_for_session(
    state: &AppState,
    app: &dyn PathResolver,
    session_id: &str,
    target_ref: Option<AuthoringTargetRef>,
) -> AppResult<ResolvedTargetRef> {
    if let Some(AuthoringTargetRef::Draft {
        thread_id,
        preview_id,
        session_id: draft_session_id,
    }) = target_ref.as_ref()
    {
        if draft_session_id != session_id {
            return Err(target_resolution_error(
                AuthoringTargetResolutionFailureKind::Stale,
                target_ref.clone().expect("draft target ref"),
                None,
            ));
        }
        let preview = if let Some(session) = get_session(state, session_id).await {
            let ctx = current_context(session_id, &session);
            handlers::resolve_session_render_preview_for_preview_id(
                state,
                &ctx,
                Some(thread_id),
                preview_id,
            )
            .await?
        } else {
            None
        };
        if let Some(preview) = preview {
            let design = preview.design_output.clone();
            let (range_count, number_count, select_count, checkbox_count) = design
                .ui_spec
                .fields
                .iter()
                .fold((0, 0, 0, 0), |acc, field| match field {
                    crate::contracts::UiField::Range { .. } => (acc.0 + 1, acc.1, acc.2, acc.3),
                    crate::contracts::UiField::Number { .. } => (acc.0, acc.1 + 1, acc.2, acc.3),
                    crate::contracts::UiField::Select { .. } => (acc.0, acc.1, acc.2 + 1, acc.3),
                    crate::contracts::UiField::Checkbox { .. } => (acc.0, acc.1, acc.2, acc.3 + 1),
                    crate::contracts::UiField::Image { .. } => acc,
                });
            return Ok(ResolvedTargetRef {
                thread_id: preview.thread_id,
                message_id: preview.preview_id,
                model_id: Some(preview.artifact_bundle.model_id.clone()),
                source_language: design.source_language,
                geometry_backend: design.geometry_backend,
                model_stl_path: Some(preview.artifact_bundle.model_stl_path),
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
        let resolved_target = {
            let conn = state.db.lock().await;
            db::get_agent_draft_by_preview_id(&conn, preview_id)
                .map_err(|e| AppError::persistence(e.to_string()))?
                .map(|draft| AuthoringTargetRef::Draft {
                    thread_id: draft.thread_id,
                    preview_id: draft.preview_id,
                    session_id: draft.session_id,
                })
        };
        return Err(target_resolution_error(
            if resolved_target.is_some() {
                AuthoringTargetResolutionFailureKind::Stale
            } else {
                AuthoringTargetResolutionFailureKind::NotFound
            },
            target_ref.clone().expect("draft target ref"),
            resolved_target,
        ));
    }

    let (cached_target, live_bound_thread_id) = {
        let sessions = state.mcp_session_registry.with_sessions().lock().await;
        let session = sessions.get(session_id);
        (
            session.and_then(|session| session.last_target.clone()),
            session.and_then(|session| session.bound_thread_id.clone()),
        )
    };
    let runtime_thread_id = crate::mcp::runtime::runtime_snapshot_by_session_id(state, session_id)
        .and_then(|snapshot| snapshot.pending_thread_id);

    let conn = state.db.lock().await;
    let stored_session = db::get_sessions_by_ids(&conn, &[session_id.to_string()])
        .map_err(|e| AppError::persistence(e.to_string()))?
        .into_iter()
        .next();

    let target = match target_ref.as_ref() {
        Some(AuthoringTargetRef::SavedVersion {
            thread_id,
            message_id,
        }) => match crate::services::target::resolve_editable_target(
            &conn,
            app,
            Some(thread_id.clone()),
            Some(message_id.clone()),
        ) {
            Ok(target) => target,
            Err(err) if err.code == AppErrorCode::NotFound => {
                let resolved_target = db::get_visible_message_thread_id(&conn, message_id)
                    .map_err(|e| AppError::persistence(e.to_string()))?
                    .map(|thread_id| AuthoringTargetRef::SavedVersion {
                        thread_id,
                        message_id: message_id.clone(),
                    });
                return Err(target_resolution_error(
                    if resolved_target.is_some() {
                        AuthoringTargetResolutionFailureKind::Stale
                    } else {
                        AuthoringTargetResolutionFailureKind::NotFound
                    },
                    target_ref.clone().expect("saved target ref"),
                    resolved_target,
                ));
            }
            Err(err) => return Err(err),
        },
        Some(AuthoringTargetRef::LatestSaved { thread_id }) => {
            match crate::services::target::resolve_editable_target(
                &conn,
                app,
                Some(thread_id.clone()),
                None,
            ) {
                Ok(target) => target,
                Err(err) if err.code == AppErrorCode::NotFound => {
                    return Err(target_resolution_error(
                        AuthoringTargetResolutionFailureKind::NotFound,
                        target_ref.clone().expect("latest saved target ref"),
                        None,
                    ));
                }
                Err(err) => return Err(err),
            }
        }
        Some(AuthoringTargetRef::Draft { .. }) => unreachable!("drafts return above"),
        None => {
            if let Some(cached_target) = cached_target {
                let still_exists =
                    db::get_visible_message_thread_id(&conn, &cached_target.message_id)
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
            } else if let Some(thread_id) =
                live_bound_thread_id.or(runtime_thread_id).or_else(|| {
                    stored_session
                        .as_ref()
                        .and_then(|session| session.thread_id.clone())
                })
            {
                crate::services::target::resolve_editable_target(&conn, app, Some(thread_id), None)?
            } else {
                return Err(AppError::validation(
                    "No bound MCP session target is available. Provide threadId/messageId or re-bind the session first.",
                ));
            }
        }
    };

    let design = target.design_output.clone();
    let (range_count, number_count, select_count, checkbox_count) = design
        .ui_spec
        .fields
        .iter()
        .fold((0, 0, 0, 0), |acc, field| match field {
            crate::contracts::UiField::Range { .. } => (acc.0 + 1, acc.1, acc.2, acc.3),
            crate::contracts::UiField::Number { .. } => (acc.0, acc.1 + 1, acc.2, acc.3),
            crate::contracts::UiField::Select { .. } => (acc.0, acc.1, acc.2 + 1, acc.3),
            crate::contracts::UiField::Checkbox { .. } => (acc.0, acc.1, acc.2, acc.3 + 1),
            crate::contracts::UiField::Image { .. } => acc,
        });
    let model_id = target.model_id();
    let runtime_bundle = target.artifact_bundle.clone();

    Ok(ResolvedTargetRef {
        thread_id: target.thread_id,
        message_id: target.message_id,
        model_id,
        source_language: design.source_language,
        geometry_backend: design.geometry_backend,
        model_stl_path: runtime_bundle
            .as_ref()
            .map(|bundle| bundle.model_stl_path.clone()),
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
        .mcp_session_registry
        .with_sessions()
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
    let model_stl_path = target.model_stl_path.clone().ok_or_else(|| {
        AppError::validation("Target does not have a model STL available for screenshots.")
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
                model_stl_path,
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
            if ctx.client_kind == "provider-mcp-http" {
                db::delete_target_lease(
                    &conn,
                    &active.session_id,
                    &active.thread_id,
                    &active.message_id,
                    active.model_id.as_deref(),
                )
                .map_err(|error| AppError::persistence(error.to_string()))?;
            } else {
                let details = serde_json::to_string_pretty(&active).unwrap_or_default();
                return Err(AppError::with_details(
                    AppErrorCode::Conflict,
                    "Target is currently leased by another agent.",
                    details,
                ));
            }
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
    thread: crate::contracts::Thread,
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

fn capture_guided_result_input_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "schemaVersion": { "type": "integer", "const": 1 },
            "requestId": { "type": "string" },
            "guideCanonicalDigest": { "type": "string" },
            "unresolvedAssumptions": {
                "type": "array",
                "maxItems": 64,
                "items": { "type": "string", "maxLength": 2048 }
            },
            "inferredRegions": {
                "type": "array",
                "maxItems": 64,
                "items": { "type": "string", "maxLength": 2048 }
            }
        },
        "required": [
            "schemaVersion",
            "requestId",
            "guideCanonicalDigest",
            "unresolvedAssumptions",
            "inferredRegions"
        ]
    })
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
    source_language: crate::contracts::SourceLanguage,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
) -> (&'static str, &'static str) {
    match source_language {
        crate::contracts::SourceLanguage::EckyIrV0 => (".ecky", "ecky"),
        crate::contracts::SourceLanguage::Build123d => (".ecky", "ecky"),
        crate::contracts::SourceLanguage::LegacyPython => match geometry_backend {
            Some(crate::contracts::GeometryBackend::Freecad) => (".FCMacro", "freecad"),
            _ => (".py", "freecad"),
        },
    }
}

fn backend_hint(geometry_backend: Option<crate::contracts::GeometryBackend>) -> &'static str {
    match geometry_backend {
        Some(crate::contracts::GeometryBackend::Build123d) => "mesh",
        Some(crate::contracts::GeometryBackend::Freecad) => "freecad",
        Some(crate::contracts::GeometryBackend::EckyRust) => "mesh",
        None => "default",
    }
}

fn backend_guide_uri(
    geometry_backend: Option<crate::contracts::GeometryBackend>,
) -> Option<&'static str> {
    match geometry_backend {
        Some(crate::contracts::GeometryBackend::Build123d) => Some("ecky://guides/ecky-rust"),
        Some(crate::contracts::GeometryBackend::Freecad) => Some("ecky://guides/freecad"),
        Some(crate::contracts::GeometryBackend::EckyRust) => Some("ecky://guides/ecky-rust"),
        None => None,
    }
}

fn surface_manifest_uri(
    geometry_backend: Option<crate::contracts::GeometryBackend>,
) -> Option<&'static str> {
    match geometry_backend {
        Some(crate::contracts::GeometryBackend::Build123d) => {
            Some("ecky://guides/surface-manifest/ecky-rust")
        }
        Some(crate::contracts::GeometryBackend::Freecad) => {
            Some("ecky://guides/surface-manifest/freecad")
        }
        Some(crate::contracts::GeometryBackend::EckyRust) => {
            Some("ecky://guides/surface-manifest/ecky-rust")
        }
        None => None,
    }
}

fn surface_reference_uri(
    geometry_backend: Option<crate::contracts::GeometryBackend>,
) -> Option<&'static str> {
    match geometry_backend {
        Some(crate::contracts::GeometryBackend::Build123d) => {
            Some("ecky://guides/surface-reference/ecky-rust")
        }
        Some(crate::contracts::GeometryBackend::Freecad) => {
            Some("ecky://guides/surface-reference/freecad")
        }
        Some(crate::contracts::GeometryBackend::EckyRust) => {
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
            "- Backend metadata decides how `.ecky` renders: `build123d`, `freecad`, or Ecky native `mesh` (aliases: `native`, legacy `eckyRust`).\n",
            "- Ecky native backend runs the controlled CAD runtime pipeline: parse -> expand -> typecheck -> lower -> validate. Direct OCCT may accelerate supported native renders, but the public backend setting is still `mesh`/`native`, not `directOcct`.\n",
            "- Never promise STEP unless artifact truth proves it: call `artifact_manifest_get` or `target_detail_get(section=\"artifactBundle\")` first and require `hasStepExport=true`, or confirm `exportArtifacts` contains `format=step`.\n",
            "- Use `artifact_manifest_get` for full machine-readable artifactBundle/modelManifest JSON. Use `target_detail_get(section=\"exportArtifacts\")` for the STEP path/detail; artifactBundle digest exposes `geometryBackend`, `geometryRepresentation`, `facetedStep`, `analyticStep`, `sourceMeshDigests`, `edgeTargetCount`, `faceTargetCount`, `exportFormats`, `hasStepExport`, and `stepExportPath` for fast routing.\n",
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
            "- Surface manifests: `ecky://guides/surface-manifest/freecad`, `ecky://guides/surface-manifest/ecky-rust`.\n\n",
            "- Surface references: `ecky://guides/surface-reference/freecad`, `ecky://guides/surface-reference/ecky-rust`.\n\n",
            "Tool discovery (compact managed sessions):\n",
            "- Managed MCP sessions start with a NARROW `tools/list`: core workflow tools plus `capability_search`/`capability_enable` only. Specialist schemas (target reads, source edits, AST edits, semantic, verify/printability, components/library, project files) are absent until enabled, and the server advertises `tools.listChanged`.\n",
            "- Use `capability_search` (optional `query`) to find which group owns a tool or capability without loading every schema, then `capability_enable` with the group id to load that group's schemas for this session. Enabling a group emits `notifications/tools/list_changed` and the next `tools/list` reflects it.\n",
            "- Prefer explicit detail reads over full reads: `target_detail_get(section=...)`, `target_macro_get`, `artifact_manifest_get`, and `thread_messages_get` return bounded chunks; reserve `target_get` for when the whole payload is truly needed. External clients that need the whole catalogue may pass `profile: full` to `tools/list` (standard opaque `cursor`/`pageSize` pagination is honored).\n\n",
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
            "- Prefer typed/static errors and `verify_generated_model` first; screenshot verification second.\n",
            "- After every preview/render that creates a user-visible version, call `verify_generated_model`; it attaches evidence to that same version.\n",
            "- If verification is red and repairable, patch source/params and preview again. Each changed draft is retained. If the repair cap is exhausted, report capped red honestly with exact issue codes/messages.\n",
            "- Use get_model_screenshot to visually verify geometric edits after `verify_generated_model` passes.\n\n",
            "Recommended startup sequence:\n",
            "1. Call workspace_overview. It resolves sourceLanguage, geometryBackend, primaryGuideUri, and compatibilityManifestUri. (Managed sessions: the compact `tools/list` already covers workspace_overview; use capability_search/capability_enable to load specialist groups on demand.)\n",
            "2. Read only `agentBrief.primaryGuideUri` / `agentBrief.mustRead` for normal authoring.\n",
            "3. Read `agentBrief.compatibilityManifestUri` only when checking whether a concrete `.ecky` form/op is supported by the resolved backend. Read prose backend guides only after lowerer/render errors or artifact/export claims.\n",
            "4. Call workspace_overview, then target_meta_get. If a targetless session is choosing an existing thread, call thread_borrow; if this is a brand-new design with no target, call thread_create first. Ecky provider sessions are already pre-bound and must not borrow their assigned thread again.\n",
            "5. Inspect sourcePath/sourceState from target metadata. When sourcePath is present, read and edit that file with normal file tools. Only when sourcePath is absent, use target_macro_get/macro_buffer_get for source edits. Use artifact_manifest_get for full artifact JSON and target_detail_get(section=...) for exact chunks.\n",
            "6. Use target_get only when you truly need the full payload.\n",
            "7. If semantic bindings matter, call semantic_manifest_get before changing views or annotations.\n",
            "8. For a bound sourcePath, edit the file directly; the project-folder watcher appends, validates, and previews settled edits automatically. Do not export first. Only for an unbound legacy target, mutate with params_preview_render, macro_buffer_replace_and_preview, macro_preview_render, or semantic tools.\n",
            "9. For preview/render tools: Call verify_generated_model on the preview/render draft. If red, repair source/params and preview again until green or repair cap exhausted.\n",
            "10. Edits append versions before validation. Preview/render and verify attach their outcomes to that same version automatically; there is no commit or finalize step. For a FEM-verified claim, use fem_validate -> fem_mesh_preview -> fem_run -> fem_result_get -> fem_publish_verified_result. Capture returned threadId/messageId/modelId in output evidence. If capped red remains, report exact red issues.\n",
            "11. Never update history.sqlite directly. State mutations must go through MCP tools.\n",
            "12. Use measurement_annotation tools for dimension meaning, and long_action_notice/long_action_clear for slow work.\n"
        ),
        selected_engine_label(state),
        authoring_card_text()
    )
}

fn workspace_overview_brief(
    state: &AppState,
    source_language: Option<crate::contracts::SourceLanguage>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
) -> WorkspaceOverviewBrief {
    let resolved_lang =
        source_language.unwrap_or_else(|| state.config.lock().unwrap().default_source_language);
    let (lang_str, dialect_str) = match resolved_lang {
        crate::contracts::SourceLanguage::EckyIrV0 => ("ecky".to_string(), "ecky".to_string()),
        crate::contracts::SourceLanguage::Build123d => {
            ("build123d".to_string(), "build123d".to_string())
        }
        crate::contracts::SourceLanguage::LegacyPython => {
            ("freecad".to_string(), "cadFrameworkV1".to_string())
        }
    };
    let (file_extension, source_hint) = workspace_source_hints(resolved_lang, geometry_backend);
    let backend = backend_hint(geometry_backend);
    let primary_guide_uri = match resolved_lang {
        crate::contracts::SourceLanguage::EckyIrV0 => ecky_source_uri_for_backend(geometry_backend),
        crate::contracts::SourceLanguage::Build123d => "ecky://guides/ecky-rust",
        crate::contracts::SourceLanguage::LegacyPython => "ecky://guides/freecad",
    }
    .to_string();
    let compatibility_manifest_uri = if resolved_lang == crate::contracts::SourceLanguage::EckyIrV0
    {
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
                crate::contracts::SourceLanguage::EckyIrV0 => "ecky",
                crate::contracts::SourceLanguage::Build123d => "build123d",
                crate::contracts::SourceLanguage::LegacyPython => "freecad",
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
            "ecky://guides/ecky-source/freecad".to_string(),
            "ecky://guides/ecky-source/ecky-rust".to_string(),
            "ecky://guides/freecad".to_string(),
            "ecky://guides/ecky-rust".to_string(),
            "ecky://guides/surface-manifest/freecad".to_string(),
            "ecky://guides/surface-manifest/ecky-rust".to_string(),
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
            "uri": "ecky://guides/ecky-source/freecad",
            "name": "Ecky Source (.ecky, FreeCAD)",
            "description": "Canonical `.ecky` language guide with FreeCAD backend support table.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/ecky-source/ecky-rust",
            "name": "Ecky Source (.ecky, mesh/eckyRust)",
            "description": "Canonical `.ecky` language guide with mesh/eckyRust backend support table.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/freecad",
            "name": "Ecky on FreeCAD",
            "description": "Backend guide for `.ecky` source when geometryBackend=freecad.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": "ecky://guides/ecky-rust",
            "name": "Ecky on mesh/eckyRust",
            "description": "Backend guide for `.ecky` source when geometryBackend=mesh/eckyRust.",
            "mimeType": "text/plain"
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

fn surface_manifest_backend_for_uri(uri: &str) -> Option<crate::contracts::GeometryBackend> {
    match uri {
        "ecky://guides/surface-manifest/freecad" => {
            Some(crate::contracts::GeometryBackend::Freecad)
        }
        "ecky://guides/surface-manifest/ecky-rust" => {
            Some(crate::contracts::GeometryBackend::EckyRust)
        }
        _ => None,
    }
}

fn surface_reference_backend_for_uri(uri: &str) -> Option<crate::contracts::GeometryBackend> {
    match uri {
        "ecky://guides/surface-reference/freecad" => {
            Some(crate::contracts::GeometryBackend::Freecad)
        }
        "ecky://guides/surface-reference/ecky-rust" => {
            Some(crate::contracts::GeometryBackend::EckyRust)
        }
        _ => None,
    }
}

fn surface_manifest_json(backend: crate::contracts::GeometryBackend) -> Value {
    let manifest = crate::ecky_language_surface::supported_surface_manifest(backend);
    json!({
        "backend": manifest.backend,
        "referenceUri": surface_reference_uri_for_backend(backend),
        "modelClauses": manifest.model_clauses,
        "modelWrappers": manifest.model_wrappers,
        "componentPlacementForms": manifest.component_placement_forms,
        "coreConstants": manifest.core_constants,
        "expressionForms": manifest.expression_forms,
        "numericHelpers": manifest.numeric_helpers,
        "pointListHelpers": manifest.point_list_helpers,
        "booleanHelpers": manifest.boolean_helpers,
        "cadOps": manifest.cad_ops,
        "wallPatternModes": manifest.wall_pattern_modes,
        "typedHolePolicy": manifest.typed_hole_policy,
    })
}

fn surface_reference_json(backend: crate::contracts::GeometryBackend) -> Value {
    serde_json::to_value(crate::ecky_language_surface::supported_surface_reference(
        backend,
    ))
    .unwrap_or_else(|_| json!({ "backend": backend, "entries": [] }))
}

fn surface_reference_uri_for_backend(backend: crate::contracts::GeometryBackend) -> &'static str {
    match backend {
        crate::contracts::GeometryBackend::Build123d => "ecky://guides/surface-reference/ecky-rust",
        crate::contracts::GeometryBackend::Freecad => "ecky://guides/surface-reference/freecad",
        crate::contracts::GeometryBackend::EckyRust => "ecky://guides/surface-reference/ecky-rust",
    }
}

fn ecky_source_uri_for_backend(backend: Option<crate::contracts::GeometryBackend>) -> &'static str {
    match backend {
        Some(crate::contracts::GeometryBackend::Build123d) => "ecky://guides/ecky-source/ecky-rust",
        Some(crate::contracts::GeometryBackend::Freecad) => "ecky://guides/ecky-source/freecad",
        Some(crate::contracts::GeometryBackend::EckyRust) => "ecky://guides/ecky-source/ecky-rust",
        None => "ecky://guides/ecky-source",
    }
}

fn ecky_source_backend_for_uri(uri: &str) -> Option<crate::contracts::GeometryBackend> {
    match uri {
        "ecky://guides/ecky-source/freecad" => Some(crate::contracts::GeometryBackend::Freecad),
        "ecky://guides/ecky-source/ecky-rust" => Some(crate::contracts::GeometryBackend::EckyRust),
        _ => None,
    }
}

fn read_resource_text(state: &AppState, uri: &str) -> Option<String> {
    let configured_backend = || {
        state
            .config
            .lock()
            .ok()
            .map(|config| config.default_geometry_backend)
            .unwrap_or(crate::contracts::GeometryBackend::EckyRust)
    };
    if let Some(backend) = ecky_source_backend_for_uri(uri) {
        return Some(crate::agent_prompt::mcp_language_reference(backend));
    }
    match uri {
        "ecky://guides/authoring-card" => Some(authoring_card_text().to_string()),
        "ecky://guides/technical-system-prompt" => Some(
            crate::agent_prompt::mcp_language_reference(configured_backend()),
        ),
        "ecky://guides/modeling-guidelines" => Some(workflow_guide_text(state)),
        "ecky://guides/ecky-source" | "ecky://guides/ecky-ir-v0" => Some(
            crate::agent_prompt::mcp_language_reference(configured_backend()),
        ),
        "ecky://guides/freecad" | "ecky://guides/cad-sdk" => Some(
            crate::agent_prompt::mcp_language_reference(crate::contracts::GeometryBackend::Freecad),
        ),
        "ecky://guides/ecky-rust" | "ecky://guides/mesh" => {
            Some(crate::agent_prompt::mcp_language_reference(
                crate::contracts::GeometryBackend::EckyRust,
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
    state
        .mcp_session_registry
        .mark_resource_read(session_id, uri)
        .await;
}

fn required_authoring_guide_uris(
    source_language: crate::contracts::SourceLanguage,
    _geometry_backend: crate::contracts::GeometryBackend,
) -> Vec<&'static str> {
    match source_language {
        crate::contracts::SourceLanguage::EckyIrV0 => vec!["ecky://guides/ecky-source"],
        crate::contracts::SourceLanguage::Build123d
        | crate::contracts::SourceLanguage::LegacyPython => Vec::new(),
    }
}

async fn missing_authoring_guide_uris(
    state: &AppState,
    session_id: &str,
    source_language: crate::contracts::SourceLanguage,
    geometry_backend: crate::contracts::GeometryBackend,
) -> Vec<&'static str> {
    let required = required_authoring_guide_uris(source_language, geometry_backend);
    if required.is_empty() {
        return required;
    }

    let read_uris = state.mcp_session_registry.read_resources(session_id).await;

    required
        .into_iter()
        .filter(|uri| !read_uris.contains(*uri))
        .collect()
}

async fn session_bypasses_resource_read_guard(state: &AppState, session_id: &str) -> bool {
    let sessions = state.mcp_session_registry.with_sessions().lock().await;
    let Some(session) = sessions.get(session_id) else {
        return false;
    };
    session.client_kind.ends_with("mcp-http")
}

async fn ensure_authoring_guides_read(
    state: &AppState,
    session_id: &str,
    source_language: crate::contracts::SourceLanguage,
    geometry_backend: crate::contracts::GeometryBackend,
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
    source_language: crate::contracts::SourceLanguage,
    geometry_backend: crate::contracts::GeometryBackend,
    requested_geometry_backend: Option<crate::contracts::GeometryBackend>,
) -> (
    crate::contracts::SourceLanguage,
    crate::contracts::GeometryBackend,
) {
    let geometry_backend = if source_language == crate::contracts::SourceLanguage::EckyIrV0 {
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
    crate::contracts::SourceLanguage,
    crate::contracts::GeometryBackend,
) {
    (
        config.default_source_language,
        req.geometry_backend
            .unwrap_or(config.default_geometry_backend),
    )
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

/// The full MCP tool catalog (AST authoring enabled), for offline skill
/// generation. Mirrors what the server returns for `tools/list`.
pub fn export_mcp_tool_catalog() -> Vec<Value> {
    tool_definitions_with_ast_enabled(true)
}

// ── OpenSpec `agent-context-budgeting` §5: typed MCP capability groups ────
//
// Tool definitions are partitioned into typed capability groups so compact
// managed MCP sessions can start narrow (core workflow + capability controls)
// and load specialist schemas on demand, while a full compatibility profile
// still exposes the whole catalogue. Every dispatched and every defined tool
// belongs to exactly one group; a drift test enforces that invariant.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum CapabilityGroup {
    Core,
    TargetReads,
    SourceEdits,
    AstEdits,
    SemanticControls,
    VerifyPrintability,
    FemAnalysis,
    ComponentsLibrary,
    ProjectFiles,
    SessionActivity,
}

impl CapabilityGroup {
    /// Stable, wire-facing group id (kebab-case). Stored in session state and
    /// accepted by the `capability_enable` control.
    pub(crate) fn id(self) -> &'static str {
        match self {
            CapabilityGroup::Core => "core",
            CapabilityGroup::TargetReads => "target-reads",
            CapabilityGroup::SourceEdits => "source-edits",
            CapabilityGroup::AstEdits => "ast-edits",
            CapabilityGroup::SemanticControls => "semantic-controls",
            CapabilityGroup::VerifyPrintability => "verify-printability",
            CapabilityGroup::FemAnalysis => "fem-analysis",
            CapabilityGroup::ComponentsLibrary => "components-library",
            CapabilityGroup::ProjectFiles => "project-files",
            CapabilityGroup::SessionActivity => "session-activity",
        }
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            CapabilityGroup::Core => "Core session & workspace",
            CapabilityGroup::TargetReads => "Target & thread reads",
            CapabilityGroup::SourceEdits => "Source / buffer edits",
            CapabilityGroup::AstEdits => "AST edits (ecky)",
            CapabilityGroup::SemanticControls => "Semantic controls",
            CapabilityGroup::VerifyPrintability => "Verify & printability",
            CapabilityGroup::FemAnalysis => "Native FEM analysis",
            CapabilityGroup::ComponentsLibrary => "Components & library",
            CapabilityGroup::ProjectFiles => "Project files",
            CapabilityGroup::SessionActivity => "Session activity & notices",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            CapabilityGroup::Core => {
                "Bootstrap, identity, session/thread lifecycle, user interaction, \
                 and capability discovery. Always present for managed sessions."
            }
            CapabilityGroup::TargetReads => {
                "Read targets, threads, messages, artifacts, and model comparisons."
            }
            CapabilityGroup::SourceEdits => {
                "Macro/buffer/params source edits, preview render, version lifecycle \
                 and restore. Macro buffer tools are absent when AST authoring \
                 is enabled."
            }
            CapabilityGroup::AstEdits => {
                "Experimental `.ecky` Core AST authoring (get/patch/set/replace). \
                 Available only when `mcp.eckyAstAuthoring` is enabled."
            }
            CapabilityGroup::SemanticControls => {
                "Semantic manifests, control primitives/views, measurement \
                 annotations, selector/dependency/constraint resolution."
            }
            CapabilityGroup::VerifyPrintability => {
                "Structural verification, printability analysis/recipes, \
                 screenshots."
            }
            CapabilityGroup::FemAnalysis => {
                "Read-only native Tet4 study validation, mesh preview, solve, convergence, cancellation, and immutable artifacts."
            }
            CapabilityGroup::ComponentsLibrary => {
                "Component extract/search/get and FreeCAD library search/import."
            }
            CapabilityGroup::ProjectFiles => {
                "Project-folder mirror export/status/apply for external editors."
            }
            CapabilityGroup::SessionActivity => {
                "Session reply/activity state, long-action notices, and \
                 confirmation requests."
            }
        }
    }

    /// All groups in stable display order (core first).
    pub(crate) fn all() -> &'static [CapabilityGroup] {
        &[
            CapabilityGroup::Core,
            CapabilityGroup::TargetReads,
            CapabilityGroup::SourceEdits,
            CapabilityGroup::AstEdits,
            CapabilityGroup::SemanticControls,
            CapabilityGroup::VerifyPrintability,
            CapabilityGroup::FemAnalysis,
            CapabilityGroup::ComponentsLibrary,
            CapabilityGroup::ProjectFiles,
            CapabilityGroup::SessionActivity,
        ]
    }

    pub(crate) fn from_id(id: &str) -> Option<CapabilityGroup> {
        CapabilityGroup::all()
            .iter()
            .copied()
            .find(|group| group.id() == id)
    }
}

/// Map a tool name to its exactly-one capability group. This is the single
/// source of truth for the partition; the drift test asserts every defined and
/// dispatched tool resolves here, and that no tool resolves twice.
pub(crate) fn tool_capability_group(name: &str) -> Option<CapabilityGroup> {
    let group = match name {
        // ── Core session & workspace (+ capability discovery controls) ──────
        "health_check"
        | "exploration_run_start"
        | "exploration_cycle_get"
        | "exploration_cycle_active_get"
        | "exploration_cycle_events"
        | "exploration_cycle_answer"
        | "exploration_run_stop"
        | "workspace_overview"
        | "agent_identity_set"
        | "session_log_in"
        | "session_log_out"
        | "resume_session"
        | "thread_list"
        | "thread_create"
        | "thread_borrow"
        | "ui_dispatch"
        | "mark_as_read"
        | "request_user_prompt"
        | "finalize_thread"
        | "capability_search"
        | "capability_enable" => CapabilityGroup::Core,
        // ── Target & thread reads ──────────────────────────────────────────
        "target_meta_get"
        | "target_macro_get"
        | "target_detail_get"
        | "target_get"
        | "thread_meta_get"
        | "thread_messages_get"
        | "thread_get"
        | "artifact_manifest_get"
        | "artifact_feature_graph_get"
        | "compare_models" => CapabilityGroup::TargetReads,
        // ── Source / buffer edits ──────────────────────────────────────────
        "macro_buffer_get"
        | "macro_buffer_replace_range"
        | "macro_buffer_apply_patch"
        | "macro_buffer_preview_render"
        | "macro_buffer_replace_and_preview"
        | "macro_preview_render"
        | "params_preview_render"
        | "concept_preview_save"
        | "version_delete"
        | "version_restore"
        | "thread_fork_from_target" => CapabilityGroup::SourceEdits,
        // ── AST edits (ecky) ───────────────────────────────────────────────
        "ecky_ast_get"
        | "ecky_ast_inspect"
        | "ecky_ast_get_node"
        | "ecky_ast_patch_validate"
        | "ecky_ast_replace_and_render"
        | "ecky_ast_patch_preview"
        | "ecky_ast_set_number"
        | "ecky_ast_set_string"
        | "ecky_ast_set_select"
        | "ecky_ast_replace_call"
        | "ecky_ast_insert_binding"
        | "ecky_ast_delete_binding"
        | "ecky_ast_rename_binding_scoped" => CapabilityGroup::AstEdits,
        // ── Semantic controls ──────────────────────────────────────────────
        "semantic_manifest_get"
        | "semantic_manifest_detail_get"
        | "control_primitive_save"
        | "control_primitive_delete"
        | "control_view_save"
        | "control_view_delete"
        | "measurement_annotation_save"
        | "measurement_annotation_delete"
        | "semantic_transform_preview"
        | "ecky_dependency_get"
        | "ecky_selector_resolve"
        | "ecky_constraints_validate" => CapabilityGroup::SemanticControls,
        // ── Verify & printability ──────────────────────────────────────────
        "verify_generated_model"
        | "get_structural_verification_summary"
        | "printability_analyze"
        | "printability_transform_recipes_get"
        | "get_model_screenshot" => CapabilityGroup::VerifyPrintability,
        // ── Native FEM analysis ────────────────────────────────────────────
        "fem_validate"
        | "fem_mesh_preview"
        | "fem_run"
        | "fem_cancel"
        | "fem_result_get"
        | "fem_convergence"
        | "fem_topology_run"
        | "fem_topology_reconstruct"
        | "fem_publish_verified_result" => CapabilityGroup::FemAnalysis,
        // ── Components & library ───────────────────────────────────────────
        "component_extract"
        | "component_search"
        | "component_get"
        | "component_import"
        | "freecad_library_search"
        | "freecad_library_import" => CapabilityGroup::ComponentsLibrary,
        // ── Project files ──────────────────────────────────────────────────
        "project_folder_export" | "project_folder_status" => CapabilityGroup::ProjectFiles,
        // ── Session activity & notices ─────────────────────────────────────
        "session_reply_save"
        | "session_activity_set"
        | "session_activity_clear"
        | "long_action_notice"
        | "long_action_clear"
        | "user_confirm_request" => CapabilityGroup::SessionActivity,
        _ => return None,
    };
    Some(group)
}

/// Partition an ordered tool list into `(group, tools)` pairs in stable group
/// order, dropping tools whose name is not mapped to a group (none expected —
/// the drift test guards this) and groups that own no tool in this config
/// (e.g. `ast-edits` when AST authoring is disabled).
pub(crate) fn partition_tools_by_group(tools: &[Value]) -> Vec<(CapabilityGroup, Vec<Value>)> {
    CapabilityGroup::all()
        .iter()
        .map(|group| {
            let owned: Vec<Value> = tools
                .iter()
                .filter(|tool| {
                    tool_capability_group(tool.get("name").and_then(Value::as_str).unwrap_or(""))
                        == Some(*group)
                })
                .cloned()
                .collect();
            (*group, owned)
        })
        .filter(|(_, owned)| !owned.is_empty())
        .collect()
}

/// Tools for a single capability group (defined tools in the requested config).
#[cfg(test)]
pub(crate) fn group_tool_definitions(
    group: CapabilityGroup,
    ecky_ast_authoring: bool,
) -> Vec<Value> {
    tool_definitions_with_ast_enabled(ecky_ast_authoring)
        .into_iter()
        .filter(|tool| {
            tool_capability_group(tool.get("name").and_then(Value::as_str).unwrap_or(""))
                == Some(group)
        })
        .collect()
}

/// Compact managed `tools/list` view: core workflow tools (always) plus the
/// session-enabled specialist groups, in stable group order. Pagination is
/// applied by the caller (compact normally fits a single page).
pub(crate) fn compact_managed_tool_definitions(
    enabled_group_ids: &std::collections::HashSet<String>,
    ecky_ast_authoring: bool,
) -> Vec<Value> {
    let all = tool_definitions_with_ast_enabled(ecky_ast_authoring);
    let mut enabled_groups: Vec<CapabilityGroup> = enabled_group_ids
        .iter()
        .filter_map(|id| CapabilityGroup::from_id(id))
        .collect();
    // Stable order regardless of HashSet iteration.
    enabled_groups.sort_by_key(|group| {
        CapabilityGroup::all()
            .iter()
            .position(|candidate| candidate == group)
    });
    let wanted: std::collections::HashSet<CapabilityGroup> = std::iter::once(CapabilityGroup::Core)
        .chain(enabled_groups)
        .collect();
    all.into_iter()
        .filter(|tool| {
            tool_capability_group(tool.get("name").and_then(Value::as_str).unwrap_or(""))
                .map(|group| wanted.contains(&group))
                .unwrap_or(false)
        })
        .collect()
}

/// Decode an opaque pagination cursor into a byte offset. Returns 0 for
/// missing/unreadable cursors so a bad cursor degrades to the first page
/// rather than erroring.
pub(crate) fn decode_tools_cursor(cursor: Option<&str>) -> usize {
    let Some(cursor) = cursor else {
        return 0;
    };
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(cursor.as_bytes())
        .ok();
    decoded
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| {
            text.strip_prefix("offset:")
                .and_then(|rest| rest.parse::<usize>().ok())
        })
        .unwrap_or(0)
}

/// Encode a byte offset as an opaque pagination cursor.
pub(crate) fn encode_tools_cursor(offset: usize) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(format!("offset:{offset}").as_bytes())
}

/// Apply standard opaque-cursor pagination to an ordered tool list. Returns the
/// page slice and the next cursor when more tools remain.
pub(crate) fn paginate_tools(
    tools: &[Value],
    cursor: Option<&str>,
    page_size: Option<usize>,
) -> (Vec<Value>, Option<String>) {
    let start = decode_tools_cursor(cursor).min(tools.len());
    let remaining = tools.len().saturating_sub(start);
    let page_size = page_size.filter(|size| *size > 0);
    let take = page_size
        .map(|size| size.min(remaining))
        .unwrap_or(remaining);
    let end = start + take;
    let page = tools[start..end].to_vec();
    let next_cursor = if end < tools.len() {
        Some(encode_tools_cursor(end))
    } else {
        None
    };
    (page, next_cursor)
}

/// `tools/list` request parameters. `profile` selects compact-managed vs full
/// compatibility; `cursor`/`pageSize` drive standard opaque pagination.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolsListParams {
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    page_size: Option<usize>,
}

const MCP_PROFILE_COMPACT_MANAGED: &str = "compact-managed";
const MCP_PROFILE_FULL: &str = "full";

/// Resolve a `tools/list` request to the page of tool schemas and an optional
/// standard continuation cursor. Managed sessions (client_kind =
/// `managed-mcp-http`) default to compact-managed discovery unless an explicit
/// profile is supplied; all other sessions default to the full compatibility
/// catalogue so existing clients keep seeing every tool.
async fn resolve_tools_list(
    state: &AppState,
    session_id: &str,
    params: &ToolsListParams,
    ecky_ast_authoring: bool,
) -> (Vec<Value>, Option<String>) {
    let managed = get_session(state, session_id)
        .await
        .map(|session| session.client_kind == "managed-mcp-http")
        .unwrap_or(false);
    let profile = params.profile.as_deref().unwrap_or(if managed {
        MCP_PROFILE_COMPACT_MANAGED
    } else {
        MCP_PROFILE_FULL
    });

    if profile == MCP_PROFILE_COMPACT_MANAGED {
        let enabled = state.mcp_session_registry.enabled_groups(session_id).await;
        let tools = compact_managed_tool_definitions(&enabled, ecky_ast_authoring);
        // Compact discovery normally fits one page; honor an explicit pageSize
        // for clients that still want to page core + enabled groups.
        paginate_tools(&tools, params.cursor.as_deref(), params.page_size)
    } else {
        // Full compatibility profile: every enabled tool, paginated.
        let tools = tool_definitions_with_ast_enabled(ecky_ast_authoring);
        paginate_tools(&tools, params.cursor.as_deref(), params.page_size)
    }
}

fn tool_definitions_with_ast_enabled(ecky_ast_authoring: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "health_check",
            "description": "Confirm server is alive and can reach storage/runtime.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "exploration_run_start",
            "description": "Submit an exploration objective to the shared Rust PLAN/BUILD/VERIFY/DECIDE controller. The controller owns provider calls, immutable version append, verification, retries, budget, queueing, and terminal decisions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "requestId": { "type": "string" },
                    "threadId": { "type": "string" },
                    "prompt": { "type": "string" },
                    "baseVersionId": { "type": "string" },
                    "attachments": { "type": "array", "items": { "type": "object" } },
                    "options": { "type": "object" },
                    "acceptanceCriteria": { "type": "array", "items": { "type": "string" } },
                    "hardConstraints": { "type": "array", "items": { "type": "string" } },
                    "softPreferences": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["requestId", "threadId", "prompt"]
            }
        }),
        json!({
            "name": "exploration_cycle_get",
            "description": "Read one compact durable exploration-cycle packet.",
            "inputSchema": { "type": "object", "properties": { "cycleId": { "type": "string" } }, "required": ["cycleId"] }
        }),
        json!({
            "name": "exploration_cycle_active_get",
            "description": "Read the compact active exploration-cycle projection for one thread. Returns null when no cycle is active.",
            "inputSchema": { "type": "object", "properties": { "threadId": { "type": "string" } }, "required": ["threadId"] }
        }),
        json!({
            "name": "exploration_cycle_events",
            "description": "Read a bounded page of detailed exploration events and raw evidence.",
            "inputSchema": { "type": "object", "properties": { "cycleId": { "type": "string" }, "afterSequence": { "type": "integer" }, "limit": { "type": "integer", "maximum": 200 } }, "required": ["cycleId"] }
        }),
        json!({
            "name": "exploration_cycle_answer",
            "description": "Submit user input for the persisted ASK blocking this cycle. The Rust controller validates and owns the resulting transition.",
            "inputSchema": { "type": "object", "properties": { "cycleId": { "type": "string" }, "answer": { "type": "string" } }, "required": ["cycleId", "answer"] }
        }),
        json!({
            "name": "exploration_run_stop",
            "description": "Request cancellation of backend-owned work for one request and stop its active durable cycle without changing version identity.",
            "inputSchema": { "type": "object", "properties": { "requestId": { "type": "string" }, "threadId": { "type": "string" } }, "required": ["requestId", "threadId"] }
        }),
        json!({
            "name": "workspace_overview",
            "description": "Fast entrypoint: resolve the default editable target, list recent threads, and report any conflicting lease.",
            "inputSchema": with_identity(&[], &[])
        }),
        json!({
            "name": "capability_search",
            "description": "Discover MCP capability groups and their tools without loading every schema. Compact managed sessions start with only core workflow tools; call this to find the specialist group (target reads, source edits, AST edits, semantic controls, verify/printability, components/library, project files, session activity) that owns a tool or capability, then capability_enable to load its schemas for tools/list. Optional query filters by group title/description or tool name/description; omit it to list every group with its tool names.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Optional case-insensitive search over group titles/descriptions and tool names/descriptions." }
                }
            }
        }),
        json!({
            "name": "capability_enable",
            "description": "Enable a specialist capability group for this session so its tool schemas appear in tools/list, then emit notifications/tools/list_changed. Compact managed sessions start with core only; use capability_search to find the group id first. Enabled groups are session-scoped. Returns the updated enabledGroups list.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": { "type": "string", "description": "Capability group id, e.g. target-reads, source-edits, ast-edits, semantic-controls, verify-printability, components-library, project-files, session-activity." }
                },
                "required": ["group"]
            }
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
            "name": "project_folder_export",
            "description": "Compatibility/recovery export. Bound targets already expose sourcePath/sourceFolder; edit sourcePath directly and let the watcher sync settled edits. Use export only to seed/reseed a missing unbound folder. Existing bindings retain their exact stored folder.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "threadId": { "type": "string", "description": "Thread to mirror. Defaults to the session target." },
                    "messageId": { "type": "string", "description": "Version message to mirror. Defaults to the active version." },
                    "slug": { "type": "string", "description": "Folder name. Defaults to a deterministic slug from title + thread id." }
                }
            }
        }),
        json!({
            "name": "project_folder_status",
            "description": "Read-only sync classification of a project folder: clean | fileChanged | threadAdvanced | conflict | missing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string" }
                },
                "required": ["slug"]
            }
        }),
        json!({
            "name": "component_extract",
            "description": "Lift an existing part subtree into a closed, copy-inline `define-component` snippet. Local port ids/types/frames/fit metadata are preserved; ports that depend on unresolved parent/world bindings block extraction. Referenced model params become the signature. Optionally saves the component into the component library.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Full .ecky model source containing the part." },
                    "partKey": { "type": "string", "description": "Key of the part/feature to extract." },
                    "name": { "type": "string", "description": "Component name. Defaults to the part key." },
                    "description": { "type": "string", "description": "One-line description surfaced by component_search." },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "threadId": { "type": "string", "description": "Provenance thread id." },
                    "messageId": { "type": "string", "description": "Provenance message id." },
                    "save": { "type": "boolean", "description": "Save to the component library (default false)." }
                },
                "required": ["source", "partKey"]
            }
        }),
        json!({
            "name": "component_search",
            "description": "Search the component library by compact header (name, one-liner, param keys, tags, port ids and compatibility types). Header-only: never returns component bodies; use component_get for source.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Case-insensitive match against name, description, and tags. Omit for all." },
                    "limit": { "type": "number", "description": "Max results (default 20, max 100)." }
                }
            }
        }),
        json!({
            "name": "component_get",
            "description": "Fetch one library component by name: full copy-inline `define-component` source plus its header.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "component_import",
            "description": "Copy-inline an installed package component into active Ecky model source and add one instantiated part. Returns self-contained authored source; never emits import-component or a dependency lock.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "packageId": { "type": "string" },
                    "version": { "type": "string" },
                    "componentId": { "type": "string" },
                    "source": { "type": "string", "description": "Current active `(model ...)` source." }
                },
                "required": ["packageId", "version", "componentId", "source"]
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
                "Use this after thread_list/thread_get only when choosing or intentionally switching existing work. ",
                "Provider connections created by Ecky are already pre-bound and MUST NOT call thread_borrow for their assigned thread. ",
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
            "description": "Fetch active editable source metadata plus a 1-based line window, authoringContext, and artifactDigest. Pass startLine/endLine for a specific range. For bound targets edit sourcePath instead; macro-buffer edits are compatibility-only when sourcePath is absent.",
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
            "description": "Compatibility-only for targets without sourcePath: open source into this session's buffer. Bound targets edit sourcePath directly and let the watcher sync settled edits. Returns digest, artifactDigest, lineCount, and a 1-based line window.",
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
            "description": "Deprecated for bound targets; edit sourcePath instead. Compatibility-only line replacement for an unbound session buffer. Requires expectedDigest.",
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
            "description": "Deprecated for bound targets; patch sourcePath instead. Compatibility-only unified diff for an unbound session buffer. Requires expectedDigest.",
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
            "description": "Compatibility-only for targets without sourcePath: validate/render the session macro buffer. Bound targets edit sourcePath directly and let the watcher sync settled edits. Returns artifactDigest; check hasStepExport before promising STEP.",
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
            "description": "Deprecated for bound targets; edit sourcePath directly and let the watcher sync settled edits. Compatibility-only replace-and-preview for targets without sourcePath. Returns artifactDigest with hasStepExport artifact truth.",
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
            "name": "fem_validate",
            "description": "Validate one authored native linear-static study against the current exact rendered model. Resolves durable BRep face tags, units, material, loads, supports, closed OCCT boundary, evidence, budgets, and stale source identity. Does not mesh, solve, edit source, or create history.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("jobId", json!({ "type": "string" })),
                    ("analysisName", json!({ "type": "string" })),
                    ("budgets", json!({ "type": "object", "description": "Optional bounded FEM budgets; defaults remain finite." })),
                    ("control", json!({ "type": "object", "description": "Optional meshing/solver tolerances; defaults remain deterministic." }))
                ],
                &["analysisName"],
            )
        }),
        json!({
            "name": "fem_run",
            "description": "Run the validated authored study through automatic external Gmsh HXT exact-BRep meshing, Tet4 assembly, Faer sparse solve, postprocess, engineering gates, and atomic immutable publication. Returns compact summaries and artifact paths only; bulk arrays stay outside message EDN. Never edits source or commits history.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("jobId", json!({ "type": "string", "description": "Caller-selected id enables a concurrent fem_cancel request." })),
                    ("analysisName", json!({ "type": "string" })),
                    ("budgets", json!({ "type": "object" })),
                    ("control", json!({ "type": "object" }))
                ],
                &["analysisName"],
            )
        }),
        json!({
            "name": "fem_mesh_preview",
            "description": "Generate and validate the pinned native Tet4 volume mesh without assembling or solving. Publishes immutable node/cell/boundary arrays plus quality and exact BRep face-group coverage. Never edits source or commits history.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("jobId", json!({ "type": "string", "description": "Caller-selected id enables a concurrent fem_cancel request." })),
                    ("analysisName", json!({ "type": "string" })),
                    ("budgets", json!({ "type": "object" })),
                    ("control", json!({ "type": "object" }))
                ],
                &["analysisName"],
            )
        }),
        json!({
            "name": "fem_cancel",
            "description": "Request cancellation of one running FEM job at its next declared safe boundary. Returns whether the job was found.",
            "inputSchema": with_identity(
                &[("jobId", json!({ "type": "string" }))],
                &["jobId"],
            )
        }),
        json!({
            "name": "fem_result_get",
            "description": "Read and revalidate one immutable FEM result manifest by exact analysis and solution digests. Returns compact metadata and binary artifact paths, never bulk arrays.",
            "inputSchema": with_identity(
                &[
                    ("analysisIdentityDigest", json!({ "type": "string" })),
                    ("solutionDigest", json!({ "type": "string" })),
                    ("maximumResultBytes", json!({ "type": "number" }))
                ],
                &["analysisIdentityDigest", "solutionDigest"],
            )
        }),
        json!({
            "name": "fem_convergence",
            "description": "Run at least three explicit coarse-to-fine native Tet4 levels sequentially. Returns per-level immutable identities, quality, counts, residual, extrema, relative deltas, and separate displacement/stress status. Rising unconverged stress is marked suspectedSingularity.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("jobId", json!({ "type": "string" })),
                    ("analysisName", json!({ "type": "string" })),
                    ("meshSizesMm", json!({ "type": "array", "minItems": 3, "items": { "type": "number" } })),
                    ("displacementRelativeTolerance", json!({ "type": "number" })),
                    ("stressRelativeTolerance", json!({ "type": "number" })),
                    ("budgets", json!({ "type": "object" })),
                    ("control", json!({ "type": "object" }))
                ],
                &["analysisName", "meshSizesMm", "displacementRelativeTolerance", "stressRelativeTolerance"],
            )
        }),
        json!({
            "name": "fem_topology_run",
            "description": "Resolve the watcher-synchronized authored FEM study, build or reuse its immutable Tet4 mesh internally, then run or resume bounded native SIMP topology optimization. fem_mesh_preview is optional diagnostics, never a prerequisite. Publishes a binary checkpoint, binary density arrays, and a VTU density preview; no topology JSON artifact manifest. Output is analysis evidence: no exact BRep, production STEP, stress acceptance, fatigue, nonlinear contact, or print certification.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("jobId", json!({ "type": "string", "description": "Caller-selected id enables fem_cancel." })),
                    ("analysisName", json!({ "type": "string" })),
                    ("budgets", json!({ "type": "object", "description": "Optional bounded FEM mesh budgets." })),
                    ("control", json!({ "type": "object", "description": "Optional deterministic mesh controls." })),
                    ("resumeStateDigest", json!({ "type": ["string", "null"] }))
                ],
                &["analysisName"],
            )
        }),
        json!({
            "name": "fem_topology_reconstruct",
            "description": "Consume one converged generic Tet4 topology density artifact, retain the component connecting every authored support/load anchor, reject islands, and emit one closed manifold polyhedron expression admitted for the faceted BRep bridge. Never infers product semantics or mutates model state. The candidate still requires preview plus independent FEM before publication.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("analysisName", json!({ "type": "string" })),
                    ("analysisIdentityDigest", json!({ "type": "string" })),
                    ("meshContentDigest", json!({ "type": "string" })),
                    ("inputDigest", json!({ "type": "string" })),
                    ("stateDigest", json!({ "type": "string" })),
                    ("densityThreshold", json!({ "type": "number", "minimum": 0, "maximum": 1, "default": 0.5 }))
                ],
                &["analysisName", "analysisIdentityDigest", "meshContentDigest", "inputDigest", "stateDigest"],
            )
        }),
        json!({
            "name": "fem_publish_verified_result",
            "description": "Publish FEM evidence for an already-created structurally green preview only when an immutable native FEM result remains decision-ready and exactly matches the preview source and OCCT analysis boundary. Reloads and verifies the result server-side; stale, red, corrupt, or caller-mismatched evidence cannot publish.",
            "inputSchema": with_identity(
                &[
                    ("threadId", json!({ "type": "string" })),
                    ("messageId", json!({ "type": "string" })),
                    ("analysisName", json!({ "type": "string" })),
                    ("analysisIdentityDigest", json!({ "type": "string" })),
                    ("solutionDigest", json!({ "type": "string" })),
                    ("resultDigest", json!({ "type": "string" })),
                    ("captureGuidedResult", capture_guided_result_input_schema())
                ],
                &["analysisName", "analysisIdentityDigest", "solutionDigest", "resultDigest"],
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"],
                        "description": "Optional: Explicitly choose geometry backend for Ecky source. `mesh`/`native` selects Ecky native lowering; `eckyRust` stays as a legacy alias."
                    }))
                ],
                &["parameterPatch"],
            )
        }),
        json!({
            "name": "macro_preview_render",
            "description": concat!(
                "Compatibility-only when sourcePath is absent. Bound targets edit sourcePath directly and let the watcher sync settled edits. Replace macro code and rerender a draft. Returns artifactDigest; check hasStepExport before promising STEP. ",
                "IMPORTANT: check workspace_overview.agentBrief.summary and rules — if sourceLanguage is `ecky`, macroCode MUST be current `.ecky` source (starting with `(model ...)`). geometryBackend chooses FreeCAD interop or native Ecky lowering; source extension does not. ",
                "Authoring uses pure lispy Ecky source compiled to internal Core IR or the selected backend. `define`, `lambda`, `let`, `let*`, `if`, and generic helpers like `range`, `map`, `filter`, `reduce`, `zip`, `enumerate`, `linspace`, and `flat-map` are allowed; `set!`, assignment, rebinding, and mutation are not. Current `let` bindings are parallel, so same-frame bindings cannot depend on earlier siblings; use `let*` or nested `let` for sequential dependencies. `(define ...)` is NOT valid inside `(model ...)`; use `let*` inside `(part ...)` for computed values from params, and reserve top-level `(define (fn args) ...)` for reusable helper functions outside `(model ...)`. ",
                "When workspace_overview.agentBrief.summary reports sourceLanguage `ecky`, uiSpec and parameters are auto-derived from the params block. For existing targets, omit parameters: macro_preview_render preserves current target params. Use params_preview_render for numeric changes. parameters only seeds first versions. ",
                "uiSpec.fields is an array of control descriptors — each field MUST have: key (string), label (string), type (one of: range|number|select|checkbox|image). ",
                "For numeric parameters, prefer number; range only when explicitly needed. range/number: min, max, step (numbers). ",
                "select: options array of {label, value} objects — MUST have at least one option. ",
                "checkbox: no extra fields. ",
                "image: use for file-picker inputs (e.g. a reference photo) — no extra fields, value is an absolute file path string once chosen by the user. ",
                "parameters is a flat key→value map matching uiSpec field keys. ",
                "For image fields, the parameter may be omitted or set to an empty string until the user picks a file in the UI. ",
                "If macroCode came from a target_macro_get/macro_buffer_get window read (windowStartLine/windowEndLine/truncated), include sourceWindow with those raw observed/window/full-size details; a truncated window submitted as a full replacement is rejected unless sourceWindow.acknowledgesTruncation is true."
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"],
                        "description": "Optional: Explicitly choose geometry backend for Ecky source. `mesh`/`native` selects Ecky native lowering; `eckyRust` stays as a legacy alias."
                    })),
                    ("sourceWindow", json!({
                        "type": "object",
                        "description": "Optional acknowledgement linking macroCode to a target_macro_get read window. Required to submit a truncated window as a full replacement: carries the read's raw observed/window/full-size line details and acknowledgesTruncation.",
                        "properties": {
                            "fullSizeLineCount": { "type": "number" },
                            "windowStartLine": { "type": "number" },
                            "windowEndLine": { "type": "number" },
                            "observedLineCount": { "type": "number" },
                            "acknowledgesTruncation": { "type": "boolean" }
                        }
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
            "description": "Compare two STL models using volume and bounding-box metrics.",
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
            "name": "version_delete",
            "description": "Move one saved version to trash by exact message id. Uses the same recoverable history deletion path as the Ecky UI.",
            "inputSchema": with_identity(
                &[("messageId", json!({ "type": "string" }))],
                &["messageId"],
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
            "description": "Set thread-scoped activity for the current bound MCP target so Ecky can drive bubble, microwave, and timer UX without scraping terminal text. Requires thread_borrow, thread_create, or another bound target first; targetless calls fail validation.",
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
            "description": "Clear thread-scoped activity for the current bound MCP target after a step finishes. Optionally set the next phase or idle status text. Targetless calls fail validation.",
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
            "description": "Compatibility alias for thread-scoped session_activity_set. Requires a bound target. Prefer session_activity_set for new agents.",
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
            "description": "Compatibility alias for thread-scoped session_activity_clear. Requires a bound target. Prefer session_activity_clear for new agents.",
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
            "description": "Run deterministic structural verification plus authored `(verify ...)` clauses on the generated model for the currently bound target/thread. Call after preview/render. Verification automatically attaches pass/fail evidence to the already-appended version; there is no commit or finalize step. Returns artifactDigest plus the full structured result including issue codes, metrics, and verifier source. If red, repair source/params and preview again, or report capped red honestly. Screenshot/VLM verification is secondary.",
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
            "description": "Read-only printability analysis for the active target/model model STL. Resolves the current editable target, reads the artifact bundle model STL path, and returns artifactDigest plus compact mesh/overhang/topology facts. Does not edit source or render.",
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
            "description": "Read-only supportless-FDM transform recipe slice for the active target/model model STL. Returns artifactDigest-guarded candidate recipes with action kind, rationale, estimated effect, target/sourceAnchor when known, and preview/apply support status. Does not edit source or render.",
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
            "description": "Create a source-consistent preview draft for supportless-FDM semantic recipes. Narrow v1 supports actionKind=reorient for sourceLanguage=ecky .ecky sources only, validates expectedArtifact {modelId, modelStlPath, contentHash}, and rejects chamfer/split as unsupported.",
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
                            "modelStlPath": { "type": "string" },
                            "contentHash": { "type": "string" }
                        },
                        "required": ["modelId", "modelStlPath", "contentHash"]
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"],
                        "description": "Optional: Explicitly choose geometry backend for Ecky source."
                    }))
                ],
                &["sourceDigest", "expectedNodeDigest"],
            )
        }));
        tools.push(json!({
            "name": "ecky_ast_patch_preview",
            "description": "Alias for ecky_ast_replace_and_render. Apply one guarded AST patch, append its changed source version, and render preview artifacts.",
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"],
                        "description": "Optional: Explicitly choose geometry backend for Ecky source."
                    }))
                ],
                &["sourceDigest", "expectedNodeDigest"],
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"]
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"]
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"]
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"]
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"]
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"]
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
                        "enum": ["freecad", "mesh", "native", "eckyRust"]
                    }))
                ],
                &["sourceDigest", "path", "expectedNodeDigest", "newName"],
            )
        }));
    }

    tools
}

fn modern_tool_definitions(ecky_ast_authoring: bool) -> Vec<Value> {
    let mut tools: Vec<Value> = tool_definitions_with_ast_enabled(ecky_ast_authoring)
        .into_iter()
        .filter(|tool| tool.get("name").and_then(Value::as_str) != Some("capability_enable"))
        .collect();
    for tool in &mut tools {
        if tool.get("name").and_then(Value::as_str) == Some("capability_search") {
            tool["description"] = json!("Summarize and search Ecky tool capability groups. Modern MCP returns a stable full catalog; this tool helps select relevant tools without changing tools/list.");
        }
        let Some(schema) = tool.get_mut("inputSchema").and_then(Value::as_object_mut) else {
            continue;
        };
        let properties = schema
            .entry("properties".to_string())
            .or_insert_with(|| json!({}));
        if let Some(properties) = properties.as_object_mut() {
            properties.entry(ECKY_CONTEXT_ID.to_string()).or_insert_with(|| {
                json!({
                    "type": "string",
                    "description": "Opaque Ecky application-state handle returned by a prior tool call. Pass it on related calls; omit it to create a new context. Contexts last until session_log_out or app restart."
                })
            });
        }
    }
    tools.sort_by(|left, right| {
        left.get("name")
            .and_then(Value::as_str)
            .cmp(&right.get("name").and_then(Value::as_str))
    });
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
        let _ = crate::db::delete_all_agent_sessions(&conn);
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

async fn dispatch_modern_request(
    server: &HttpServerState,
    uri: &axum::http::Uri,
    metadata: &ModernRequestMetadata,
    req: JsonRpcRequest,
) -> (StatusCode, JsonRpcResponse) {
    let id = req.id.clone();
    let ecky_ast_authoring = server.state.config.lock().unwrap().mcp.ecky_ast_authoring;
    let mut response = match req.method.as_str() {
        "server/discover" => json_rpc_result(
            id,
            json!({
                "supportedVersions": [MCP_PROTOCOL_MODERN],
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": {},
                    "prompts": {}
                },
                "instructions": "Use explicit threadId/messageId targets and carry eckyContextId between related stateful Ecky tool calls.",
                "ttlMs": MODERN_LIST_TTL_MS,
                "cacheScope": "private"
            }),
        ),
        "resources/list" => json_rpc_result(
            id,
            json!({
                "resources": resource_definitions(),
                "ttlMs": MODERN_LIST_TTL_MS,
                "cacheScope": "private"
            }),
        ),
        "resources/read" => {
            match serde_json::from_value::<ReadResourceParams>(req.params.unwrap_or_default()) {
                Ok(params) => match read_resource_content(&server.state, &params.uri) {
                    Some(content) => json_rpc_result(
                        id,
                        json!({
                            "contents": [{
                                "uri": params.uri,
                                "mimeType": content.mime_type,
                                "text": content.text
                            }],
                            "ttlMs": MODERN_LIST_TTL_MS,
                            "cacheScope": "private"
                        }),
                    ),
                    None => json_rpc_error(id, -32602, format!("Unknown resource: {}", params.uri)),
                },
                Err(err) => json_rpc_error(id, -32602, format!("Invalid params: {err}")),
            }
        }
        "prompts/list" => json_rpc_result(
            id,
            json!({
                "prompts": prompt_definitions(),
                "ttlMs": MODERN_LIST_TTL_MS,
                "cacheScope": "private"
            }),
        ),
        "prompts/get" => {
            match serde_json::from_value::<GetPromptParams>(req.params.unwrap_or_default()) {
                Ok(params) => {
                    let _ = params.arguments;
                    match prompt_payload(&server.state, &params.name) {
                        Some(prompt) => json_rpc_result(id, prompt),
                        None => {
                            json_rpc_error(id, -32602, format!("Unknown prompt: {}", params.name))
                        }
                    }
                }
                Err(err) => json_rpc_error(id, -32602, format!("Invalid params: {err}")),
            }
        }
        "tools/list" => {
            let params: ToolsListParams =
                serde_json::from_value(req.params.unwrap_or_default()).unwrap_or_default();
            let tools = modern_tool_definitions(ecky_ast_authoring);
            let (tools, next_cursor) =
                paginate_tools(&tools, params.cursor.as_deref(), params.page_size);
            let mut result = json!({
                "tools": tools,
                "ttlMs": MODERN_LIST_TTL_MS,
                "cacheScope": "private"
            });
            if let Some(cursor) = next_cursor {
                result["nextCursor"] = json!(cursor);
            }
            json_rpc_result(id, result)
        }
        "tools/call" => {
            let mut params = match serde_json::from_value::<CallToolParams>(
                req.params.clone().unwrap_or_default(),
            ) {
                Ok(params) => params,
                Err(err) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        json_rpc_error(id, -32602, format!("Invalid params: {err}")),
                    );
                }
            };
            if let Some(arguments) = params.arguments.as_mut().and_then(Value::as_object_mut) {
                arguments.remove(ECKY_CONTEXT_ID);
            }
            let known_tool = modern_tool_definitions(ecky_ast_authoring)
                .iter()
                .any(|tool| tool.get("name").and_then(Value::as_str) == Some(&params.name));
            if !known_tool {
                return (
                    StatusCode::BAD_REQUEST,
                    json_rpc_error(id, -32602, format!("Unknown tool: {}", params.name)),
                );
            }
            let tool_name = params.name.clone();
            let managed_agent_id = managed_agent_id_from_uri(uri).filter(|agent_id| {
                crate::mcp::runtime::runtime_snapshot_by_id(&server.state, agent_id).is_some()
            });
            let context_id = match resolve_modern_context(
                &server.state,
                metadata,
                requested_ecky_context_id(&req),
                managed_agent_id.is_some(),
            )
            .await
            {
                Ok(context_id) => context_id,
                Err(err) => {
                    let mut response = mcp_tool_error(id, &err);
                    modernize_result(&mut response);
                    return (StatusCode::OK, response);
                }
            };
            if let Err(error) = prebind_provider_session(
                &server.state,
                &context_id,
                provider_thread_id_from_uri(uri),
            )
            .await
            {
                let mut response = mcp_tool_error(id, &error);
                modernize_result(&mut response);
                return (StatusCode::OK, response);
            }
            if let Some(agent_id) = managed_agent_id {
                crate::mcp::runtime::bind_managed_http_session(
                    &server.state,
                    &agent_id,
                    &context_id,
                    Some("Connected to Ecky MCP.".to_string()),
                );
            }
            let dispatch_server = server.clone();
            let dispatch_context_id = context_id.clone();
            let dispatch_result = run_on_mcp_tool_dispatch_stack(move || async move {
                dispatch_tool_call(&dispatch_server, &dispatch_context_id, params).await
            })
            .await;
            match dispatch_result {
                Ok((mut value, next_target)) => {
                    if next_target.is_some() {
                        set_session_target(&server.state, &context_id, next_target).await;
                    }
                    if tool_name == "capability_search" {
                        value["profile"] = json!(MCP_PROFILE_FULL);
                        value["hint"] = json!("Modern tools/list is stable and already contains every available schema; use these groups only to choose relevant tools.");
                    }
                    if get_session(&server.state, &context_id).await.is_some() {
                        attach_ecky_context_id(&mut value, &context_id);
                    }
                    mcp_tool_success(id, &value)
                }
                Err(err) => mcp_tool_error(id, &err),
            }
        }
        _ => {
            return (
                StatusCode::NOT_FOUND,
                json_rpc_error(id, -32601, format!("Method not found: {}", req.method)),
            );
        }
    };
    modernize_result(&mut response);
    (StatusCode::OK, response)
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

    if request_uses_modern_mcp(&headers, &req) {
        let metadata = match validate_modern_request(&headers, &req) {
            Ok(metadata) => metadata,
            Err(payload) => {
                return json_http_response(StatusCode::BAD_REQUEST, &payload, None);
            }
        };
        let (status, payload) = dispatch_modern_request(&server, &uri, &metadata, req).await;
        if let Some(handle) = server.handle.as_ref() {
            emit_sessions_changed(&server.state, handle).await;
        }
        return json_http_response(status, &payload, None);
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
        if let Err(error) = prebind_provider_session(
            &server.state,
            &session_id,
            provider_thread_id_from_uri(&uri),
        )
        .await
        {
            server.state.mcp_session_registry.remove(&session_id).await;
            let payload = json_rpc_error(req.id, -32002, error.message);
            return json_http_response(StatusCode::NOT_FOUND, &payload, None);
        }
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
                    "tools": {
                        "listChanged": true
                    },
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
        let mut sessions = server
            .state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await;
        sessions.insert(
            session_id.clone(),
            McpSessionState::new("mcp-http".to_string(), String::new()),
        );
        drop(sessions);
        if let Err(error) = prebind_provider_session(
            &server.state,
            &session_id,
            provider_thread_id_from_uri(&uri),
        )
        .await
        {
            server.state.mcp_session_registry.remove(&session_id).await;
            let payload = json_rpc_error(req.id, -32002, error.message);
            return json_http_response(StatusCode::NOT_FOUND, &payload, None);
        }
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
            let params: ToolsListParams =
                serde_json::from_value(req.params.unwrap_or_default()).unwrap_or_default();
            let (tools, next_cursor) =
                resolve_tools_list(&server.state, session_id, &params, ecky_ast_authoring).await;
            let mut result = json!({ "tools": tools });
            if let Some(cursor) = next_cursor {
                result["nextCursor"] = json!(cursor);
            }
            json_rpc_result(req.id, result)
        }
        "tools/call" => {
            match serde_json::from_value::<CallToolParams>(req.params.unwrap_or_default()) {
                Ok(params) => {
                    let dispatch_server = server.clone();
                    let dispatch_session_id = session_id.to_string();
                    let dispatch_result = run_on_mcp_tool_dispatch_stack(move || async move {
                        dispatch_tool_call(&dispatch_server, &dispatch_session_id, params).await
                    })
                    .await;
                    match dispatch_result {
                        Ok((value, next_target)) => {
                            if next_target.is_some() {
                                set_session_target(&server.state, session_id, next_target).await;
                            }
                            mcp_tool_success(req.id, &value)
                        }
                        Err(err) => mcp_tool_error(req.id, &err),
                    }
                }
                Err(err) => json_rpc_error(req.id, -32602, format!("Invalid params: {}", err)),
            }
        }
        _ => json_rpc_error(req.id, -32601, format!("Method not found: {}", req.method)),
    }
}

#[cfg(test)]
fn dispatched_tool_names() -> std::collections::BTreeSet<String> {
    let source = include_str!("server.rs");
    let match_start = source
        .find("match params.name.as_str() {")
        .expect("dispatch match");
    let dispatch_body = &source[match_start..];
    let match_end = dispatch_body
        .find("            \"Unknown tool: {}\",")
        .expect("dispatch fallback");

    dispatch_body[..match_end]
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            // A dispatch arm looks like `"name" => …` (optionally several
            // `"a" | "b" => …`). Require the `=>` on the same line as the name
            // so inline `json!({ "key": … })` literals and error strings that
            // happen to start a line are not mistaken for tool arms.
            if !line.starts_with('"') || !line.contains("=>") {
                return None;
            }

            Some(
                line.split("=>")
                    .next()
                    .expect("dispatch arm")
                    .split('|')
                    .filter_map(|part| {
                        let part = part.trim();
                        part.strip_prefix('"')
                            .and_then(|rest| rest.split_once('"'))
                            .map(|(name, _)| name.to_string())
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
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

async fn dispatch_workspace_overview(
    server: &HttpServerState,
    session_id: &str,
    args: Value,
) -> AppResult<(Value, Option<McpTargetRef>)> {
    let req_args = serde_json::from_value::<WorkspaceOverviewRequest>(args).unwrap_or(
        WorkspaceOverviewRequest {
            agent_label: None,
            llm_model_id: None,
            llm_model_label: None,
        },
    );
    let live_bound_thread_id = server
        .state
        .mcp_session_registry
        .with_sessions()
        .lock()
        .await
        .get(session_id)
        .and_then(|session| session.bound_thread_id.clone())
        .or_else(|| {
            crate::mcp::runtime::runtime_snapshot_by_session_id(&server.state, session_id)
                .and_then(|snapshot| snapshot.pending_thread_id)
        });
    let target_state = server.state.clone();
    let target_app = server.app.clone();
    let target_session_id = session_id.to_string();
    let target_result = tauri::async_runtime::spawn(async move {
        resolve_target_for_session(
            &target_state,
            target_app.as_ref(),
            &target_session_id,
            None,
            None,
        )
        .await
    })
    .await
    .map_err(|error| AppError::internal(format!("Target resolver task failed: {error}")))?;
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
            // openspec thread-source-binding 4.1: expose the bound source
            // path / folder / state for the agent using the exact stored
            // binding path.
            let (source_path, source_folder, source_state) =
                handlers::resolve_target_source_binding(
                    &server.state,
                    server.app.as_ref(),
                    &conn,
                    &target.thread_id,
                    &target.title,
                );
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
                        source_path,
                        source_folder,
                        source_state,
                    },
                    recent_threads,
                    lease_info,
                },
                next_target,
            )
        }
        Err(err)
            if err.code == crate::contracts::AppErrorCode::Validation
                && crate::mcp::empty_thread_target::is_empty_thread_target_message(
                    &err.message,
                ) =>
        {
            let stored_thread_id = db::get_sessions_by_ids(&conn, &[session_id.to_string()])
                .map_err(|e| AppError::persistence(e.to_string()))?
                .into_iter()
                .next()
                .and_then(|session| session.thread_id);
            let thread_id = live_bound_thread_id.or(stored_thread_id).ok_or(err)?;
            let thread = crate::services::history::get_thread_summary(&conn, &thread_id)?;
            // openspec thread-source-binding 4.1: even a thread with no
            // successful versions may already be bound (threads bind on
            // creation), so expose the bound source view when present.
            let (source_path, source_folder, source_state) =
                handlers::resolve_target_source_binding(
                    &server.state,
                    server.app.as_ref(),
                    &conn,
                    &thread.id,
                    &thread.title,
                );
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
                        source_path,
                        source_folder,
                        source_state,
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

// OpenSpec `agent-context-budgeting` §5.2/§5.3: capability discovery/enable
// controls for compact managed MCP sessions.

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitySearchParams {
    #[serde(default)]
    query: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapabilityEnableParams {
    group: String,
}

fn dispatch_capability_search(
    state: &AppState,
    args: &Value,
) -> AppResult<(Value, Option<McpTargetRef>)> {
    let params: CapabilitySearchParams = serde_json::from_value(args.clone()).unwrap_or_default();
    let ecky_ast_authoring = state.config.lock().unwrap().mcp.ecky_ast_authoring;
    let tools = tool_definitions_with_ast_enabled(ecky_ast_authoring);
    let partitioned = partition_tools_by_group(&tools);
    let query = params.query.as_deref().map(str::trim).unwrap_or("");
    let lower_query = query.to_ascii_lowercase();

    let matched_groups: Vec<Value> = partitioned
        .iter()
        .filter_map(|(group, group_tools)| {
            let group_hits: Vec<&Value> = if lower_query.is_empty() {
                group_tools.iter().collect()
            } else {
                group_tools
                    .iter()
                    .filter(|tool| {
                        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
                        let desc = tool
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        name.to_ascii_lowercase().contains(&lower_query)
                            || desc.to_ascii_lowercase().contains(&lower_query)
                    })
                    .collect()
            };

            let group_text_matches = lower_query.is_empty()
                || group.title().to_ascii_lowercase().contains(&lower_query)
                || group.description().to_ascii_lowercase().contains(&lower_query)
                || group.id().contains(&lower_query);

            if !lower_query.is_empty() && group_hits.is_empty() && !group_text_matches {
                return None;
            }

            let tools_summary: Vec<Value> = group_tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.get("name").and_then(Value::as_str).unwrap_or(""),
                        "description": tool.get("description").and_then(Value::as_str).unwrap_or(""),
                    })
                })
                .collect();
            let matched_tools: Vec<&str> = if lower_query.is_empty() {
                Vec::new()
            } else {
                group_hits
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                    .collect()
            };
            Some(json!({
                "id": group.id(),
                "title": group.title(),
                "description": group.description(),
                "toolCount": group_tools.len(),
                "matchedTools": matched_tools,
                "tools": tools_summary,
            }))
        })
        .collect();

    let total_tools: usize = matched_groups
        .iter()
        .filter_map(|group| group.get("toolCount").and_then(Value::as_u64))
        .map(|count| count as usize)
        .sum();

    Ok((
        json!({
            "profile": MCP_PROFILE_COMPACT_MANAGED,
            "query": query,
            "groups": matched_groups.clone(),
            "totalGroups": matched_groups.len(),
            "totalTools": total_tools,
            "hint": "Call capability_enable with a group id to load its tool schemas into tools/list for this session."
        }),
        None,
    ))
}

async fn dispatch_capability_enable(
    state: &AppState,
    session_id: &str,
    args: &Value,
) -> AppResult<(Value, Option<McpTargetRef>)> {
    let params: CapabilityEnableParams = serde_json::from_value(args.clone())
        .map_err(|e| AppError::validation(format!("Invalid capability_enable params: {e}")))?;
    let group = CapabilityGroup::from_id(params.group.trim()).ok_or_else(|| {
        AppError::validation(format!(
            "Unknown capability group: {}. Use capability_search to list valid group ids.",
            params.group
        ))
    })?;

    let already_enabled = state
        .mcp_session_registry
        .enabled_groups(session_id)
        .await
        .contains(group.id());
    if !already_enabled {
        state
            .mcp_session_registry
            .enable_group(session_id, group.id())
            .await;
    }

    // Emit standard `notifications/tools/list_changed` for this session. This
    // Streamable-HTTP server answers each request with one JSON-RPC object, so
    // the notification is queued for delivery to managed agents on their next
    // poll; the next `tools/list` already reflects the enabled group.
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/tools/list_changed"
    });
    state
        .mcp_session_registry
        .enqueue_notification(session_id, notification)
        .await;

    let enabled: Vec<String> = {
        let set = state.mcp_session_registry.enabled_groups(session_id).await;
        let mut ids: Vec<String> = set.iter().cloned().collect();
        ids.sort_by_key(|id| {
            CapabilityGroup::from_id(id)
                .and_then(|group| {
                    CapabilityGroup::all()
                        .iter()
                        .position(|candidate| candidate == &group)
                })
                .unwrap_or(usize::MAX)
        });
        ids
    };

    Ok((
        json!({
            "group": { "id": group.id(), "title": group.title() },
            "enabledGroups": enabled,
            "listChanged": true,
            "alreadyEnabled": already_enabled,
            "hint": "The session tool list changed; the next tools/list includes this group's schemas."
        }),
        None,
    ))
}

/// Drain queued MCP server→client notifications for a session (test/probe hook
/// for the `notifications/tools/list_changed` emission path).
#[cfg(test)]
pub(crate) async fn drain_pending_mcp_notifications(
    state: &AppState,
    session_id: &str,
) -> Vec<Value> {
    state
        .mcp_session_registry
        .drain_notifications(session_id)
        .await
}

fn default_fem_budgets() -> crate::contracts::FemBudgetLimitsDto {
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

fn default_fem_control() -> crate::contracts::FemPipelineControlDto {
    crate::contracts::FemPipelineControlDto {
        envelope_mm: 0.1,
        minimum_scaled_jacobian: 1.0e-6,
        maximum_runtime_ms: 10 * 60 * 1000,
        relative_solver_tolerance: 1.0e-8,
        thread_count: 0,
    }
}

async fn resolve_mcp_fem_request(
    server: &HttpServerState,
    session_id: &str,
    request: FemTargetRequest,
    operation: &str,
) -> AppResult<(crate::contracts::FemStudyRequest, McpTargetRef)> {
    let target = resolve_target_for_session(
        &server.state,
        server.app.as_ref(),
        session_id,
        request.thread_id,
        request.message_id,
    )
    .await?;
    if target.source_language != crate::contracts::SourceLanguage::EckyIrV0 {
        return Err(AppError::validation(format!(
            "{operation} requires sourceLanguage=ecky; target is {}.",
            target.source_language.as_str()
        )));
    }
    let editable = {
        let conn = server.state.db.lock().await;
        crate::services::target::resolve_editable_target(
            &conn,
            server.app.as_ref(),
            Some(target.thread_id.clone()),
            Some(target.message_id.clone()),
        )?
    };
    let model_id = editable.model_id().ok_or_else(|| {
        AppError::validation(format!("{operation} requires a rendered model artifact."))
    })?;
    let lease_target = McpTargetRef {
        thread_id: editable.thread_id,
        message_id: editable.message_id,
        model_id: Some(model_id.clone()),
    };
    let job_id = request
        .job_id
        .unwrap_or_else(|| format!("fem-mcp-{}", Uuid::new_v4().simple()));
    Ok((
        crate::contracts::FemStudyRequest {
            job_id,
            model_id,
            source: editable.design_output.macro_code,
            analysis_name: request.analysis_name,
            budgets: request.budgets.unwrap_or_else(default_fem_budgets),
            control: request.control.unwrap_or_else(default_fem_control),
        },
        lease_target,
    ))
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
        "exploration_run_start" => {
            let mut args = args;
            args.as_object_mut()
                .ok_or_else(|| AppError::validation("exploration_run_start requires an object."))?
                .insert("kind".to_string(), json!("controller"));
            let input: crate::contracts::exploration_run::StartExplorationRunInput =
                serde_json::from_value(args)
                    .map_err(|error| AppError::validation(error.to_string()))?;
            let response =
                handlers::handle_exploration_run_start(&server.state, server.app.as_ref(), input)
                    .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "exploration_cycle_get" => {
            let cycle_id = args
                .get("cycleId")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::validation("exploration_cycle_get requires cycleId."))?;
            let response =
                handlers::handle_exploration_cycle_get(&server.state, cycle_id.to_string()).await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "exploration_cycle_active_get" => {
            let thread_id = args
                .get("threadId")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::validation("exploration_cycle_active_get requires threadId.")
                })?;
            let response =
                handlers::handle_active_exploration_cycle_get(&server.state, thread_id.to_string())
                    .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "exploration_cycle_events" => {
            let cycle_id = args.get("cycleId").and_then(Value::as_str).ok_or_else(|| {
                AppError::validation("exploration_cycle_events requires cycleId.")
            })?;
            let response = handlers::handle_exploration_cycle_events(
                &server.state,
                cycle_id.to_string(),
                args.get("afterSequence").and_then(Value::as_u64),
                args.get("limit")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize),
            )
            .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "exploration_cycle_answer" => {
            let cycle_id = args.get("cycleId").and_then(Value::as_str).ok_or_else(|| {
                AppError::validation("exploration_cycle_answer requires cycleId.")
            })?;
            let answer = args
                .get("answer")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::validation("exploration_cycle_answer requires answer."))?;
            let response = handlers::handle_exploration_cycle_answer(
                &server.state,
                cycle_id.to_string(),
                answer.to_string(),
            )
            .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "exploration_run_stop" => {
            let input: crate::contracts::exploration_run::StopExplorationRunInput =
                serde_json::from_value(args)
                    .map_err(|error| AppError::validation(error.to_string()))?;
            let response = handlers::handle_exploration_run_stop(&server.state, input).await?;
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
            Box::pin(dispatch_workspace_overview(server, session_id, args)).await
        }
        "capability_search" => dispatch_capability_search(&server.state, &args),
        "capability_enable" => dispatch_capability_enable(&server.state, session_id, &args).await,
        "project_folder_export" => {
            let req_args: handlers::ProjectFolderExportRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_project_folder_export(
                &server.state,
                server.app.as_ref(),
                req_args,
                &current_ctx,
            )
            .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "project_folder_status" => {
            let req_args: handlers::ProjectFolderStatusRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_project_folder_status(
                &server.state,
                server.app.as_ref(),
                req_args,
            )
            .await?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "component_extract" => {
            let req_args: handlers::ComponentExtractToolRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_component_extract(server.app.as_ref(), req_args)?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "component_search" => {
            let req_args: handlers::ComponentSearchToolRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_component_search(server.app.as_ref(), req_args)?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "component_get" => {
            let req_args: handlers::ComponentGetToolRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_component_get(server.app.as_ref(), req_args)?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "component_import" => {
            let req_args: handlers::ComponentImportToolRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = handlers::handle_component_import(server.app.as_ref(), req_args)?;
            Ok((serde_json::to_value(response).unwrap(), None))
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
            if !matches!(
                extension.as_str(),
                "fcstd" | "step" | "stp" | "stl" | "obj" | "3mf"
            ) {
                return Err(AppError::validation(format!(
                    "FreeCAD library format '{}' is not importable yet.",
                    extension
                )));
            }
            let _guard = server.state.acquire_geometry_render().await;
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
                _ => unreachable!("validated FreeCAD library extension"),
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
            let response = handlers::handle_thread_create(
                &server.state,
                server.app.as_ref(),
                req_args,
                &action_ctx,
            )
            .await?;
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
        "fem_validate" => {
            let req_args: FemTargetRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let (request, target) =
                resolve_mcp_fem_request(server, session_id, req_args, "fem_validate").await?;
            let response = crate::commands::fem::validate_fem_study_with_resolver(
                request,
                server.app.as_ref(),
            )?;
            Ok((serde_json::to_value(response).unwrap(), Some(target)))
        }
        "fem_run" => {
            let req_args: FemTargetRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let (request, target) =
                resolve_mcp_fem_request(server, session_id, req_args, "fem_run").await?;
            let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
            {
                let mut jobs = server.state.fem_cancellations.lock().await;
                if jobs.contains_key(&request.job_id) {
                    return Err(AppError::conflict(format!(
                        "FEM job '{}' is already running.",
                        request.job_id
                    )));
                }
                jobs.insert(request.job_id.clone(), cancellation.clone());
            }
            let job_id = request.job_id.clone();
            let resolver = server.app.clone();
            let joined = tauri::async_runtime::spawn_blocking(move || {
                crate::commands::fem::run_fem_study_with_resolver_subscribed(
                    request,
                    resolver.as_ref(),
                    cancellation,
                    |_| {},
                )
            })
            .await;
            server.state.fem_cancellations.lock().await.remove(&job_id);
            let response = joined.map_err(|error| {
                AppError::internal(format!("FEM MCP job thread failed: {error}"))
            })??;
            Ok((serde_json::to_value(response).unwrap(), Some(target)))
        }
        "fem_mesh_preview" => {
            let req_args: FemTargetRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let (request, target) =
                resolve_mcp_fem_request(server, session_id, req_args, "fem_mesh_preview").await?;
            let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
            {
                let mut jobs = server.state.fem_cancellations.lock().await;
                if jobs.contains_key(&request.job_id) {
                    return Err(AppError::conflict(format!(
                        "FEM job '{}' is already running.",
                        request.job_id
                    )));
                }
                jobs.insert(request.job_id.clone(), cancellation.clone());
            }
            let job_id = request.job_id.clone();
            let resolver = server.app.clone();
            let joined = tauri::async_runtime::spawn_blocking(move || {
                crate::commands::fem::preview_fem_mesh_with_resolver_subscribed(
                    request,
                    resolver.as_ref(),
                    cancellation,
                    |_| {},
                )
            })
            .await;
            server.state.fem_cancellations.lock().await.remove(&job_id);
            let response = joined.map_err(|error| {
                AppError::internal(format!("FEM MCP mesh thread failed: {error}"))
            })??;
            Ok((serde_json::to_value(response).unwrap(), Some(target)))
        }
        "fem_cancel" => {
            let req_args: FemCancelToolRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let jobs = server.state.fem_cancellations.lock().await;
            let cancellation_requested = jobs.get(&req_args.job_id).is_some_and(|cancelled| {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                true
            });
            Ok((
                json!({
                    "jobId": req_args.job_id,
                    "cancellationRequested": cancellation_requested,
                }),
                None,
            ))
        }
        "fem_convergence" => {
            let req_args: FemConvergenceToolRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let (study, target) =
                resolve_mcp_fem_request(server, session_id, req_args.target, "fem_convergence")
                    .await?;
            let request = crate::contracts::FemConvergenceRequest {
                study,
                mesh_sizes_mm: req_args.mesh_sizes_mm,
                displacement_relative_tolerance: req_args.displacement_relative_tolerance,
                stress_relative_tolerance: req_args.stress_relative_tolerance,
            };
            let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let job_id = request.study.job_id.clone();
            {
                let mut jobs = server.state.fem_cancellations.lock().await;
                if jobs.contains_key(&job_id) {
                    return Err(AppError::conflict(format!(
                        "FEM job '{job_id}' is already running."
                    )));
                }
                jobs.insert(job_id.clone(), cancellation.clone());
            }
            let resolver = server.app.clone();
            let joined = tauri::async_runtime::spawn_blocking(move || {
                crate::commands::fem::run_fem_convergence_with_resolver(
                    request,
                    resolver.as_ref(),
                    cancellation.as_ref(),
                    |_| {},
                )
            })
            .await;
            server.state.fem_cancellations.lock().await.remove(&job_id);
            let response = joined.map_err(|error| {
                AppError::internal(format!("FEM MCP convergence thread failed: {error}"))
            })??;
            Ok((serde_json::to_value(response).unwrap(), Some(target)))
        }
        "fem_topology_run" => {
            let req_args: FemTopologyToolRequest = serde_json::from_value(args)
                .map_err(|error| AppError::validation(error.to_string()))?;
            let (mut study, target) =
                resolve_mcp_fem_request(server, session_id, req_args.target, "fem_topology_run")
                    .await?;
            crate::commands::fem::apply_fem_compute_policy(&config.fem_compute, &mut study);
            let runtime_policy =
                crate::commands::fem::FemTopologyRuntimePolicy::from_compute(&config.fem_compute);
            let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let job_id = study.job_id.clone();
            {
                let mut jobs = server.state.fem_cancellations.lock().await;
                if jobs.contains_key(&job_id) {
                    return Err(AppError::conflict(format!(
                        "FEM job '{job_id}' is already running."
                    )));
                }
                jobs.insert(job_id.clone(), cancellation.clone());
            }
            let resolver = server.app.clone();
            let request = crate::contracts::FemTopologyRunRequest {
                study,
                resume_state_digest: req_args.resume_state_digest,
            };
            let joined = tauri::async_runtime::spawn_blocking(move || {
                crate::commands::fem::run_fem_topology_with_resolver(
                    request,
                    &runtime_policy,
                    resolver.as_ref(),
                    cancellation.as_ref(),
                )
            })
            .await;
            server.state.fem_cancellations.lock().await.remove(&job_id);
            let response = joined.map_err(|error| {
                AppError::internal(format!("FEM topology MCP worker failed: {error}"))
            })??;
            Ok((serde_json::to_value(response).unwrap(), Some(target)))
        }
        "fem_result_get" => {
            let req_args: FemResultGetToolRequest =
                serde_json::from_value(args).map_err(|e| AppError::validation(e.to_string()))?;
            let response = crate::commands::fem::read_fem_result_with_resolver(
                crate::contracts::FemResultReadRequest {
                    analysis_identity_digest: req_args.analysis_identity_digest,
                    solution_digest: req_args.solution_digest,
                    maximum_result_bytes: req_args
                        .maximum_result_bytes
                        .unwrap_or(128 * 1024 * 1024),
                },
                server.app.as_ref(),
            )?;
            Ok((serde_json::to_value(response).unwrap(), None))
        }
        "fem_publish_verified_result" => {
            let mut req_args: FemVerifiedPublishRequest = serde_json::from_value(args)
                .map_err(|error| AppError::validation(error.to_string()))?;
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
            match handlers::handle_publish_fem_verified_result(
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
                    emit_history_updated(server);
                    Ok((value, Some(next_target)))
                }
                Err(error) => {
                    let _ =
                        release_lease(&server.state, &action_ctx.session_id, &lease_target).await;
                    Err(error)
                }
            }
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
                    crate::mcp::source_window_guard::validate_macro_source_window_replacement(
                        &req_args.macro_code,
                        req_args.source_window.as_ref(),
                    )?;
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
                        && crate::mcp::empty_thread_target::is_empty_thread_target_message(
                            &e.message,
                        ) =>
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
        "version_delete" => {
            let req_args: VersionDeleteRequest =
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
            match handlers::handle_version_delete(&server.state, req_args, &current_ctx).await {
                Ok(response) => {
                    release_lease(&server.state, &action_ctx.session_id, &lease_target).await?;
                    emit_history_updated(server);
                    Ok((serde_json::to_value(&response).unwrap(), None))
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
            )
            .await?;
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
            )
            .await?;
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
    let label = model_manifest.document.document_label.trim();
    let document_name = model_manifest.document.document_name.trim();
    let requested_title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            (label.is_empty() && document_name.is_empty() && !request.item.name.trim().is_empty())
                .then(|| request.item.name.trim().to_string())
        });
    let thread_id = request
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| current_thread_id.map(str::to_string))
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let projection = crate::services::imported_model::persist_imported_runtime(
        Some(thread_id),
        requested_title,
        artifact_bundle,
        model_manifest,
        state,
        app,
    )
    .await?;

    let target = McpTargetRef {
        thread_id: projection.thread_id.clone(),
        message_id: projection.message_id.clone(),
        model_id: Some(projection.artifact_bundle.model_id.clone()),
    };
    Ok((
        json!({
            "threadId": projection.thread_id,
            "messageId": projection.message_id,
            "title": projection.title,
            "artifactBundle": projection.artifact_bundle,
            "modelManifest": projection.model_manifest
        }),
        target,
    ))
}

pub fn connection_uses_embedded_mcp(connection_type: Option<&str>) -> bool {
    connection_type == Some("mcp")
        || connection_type.is_some_and(|value| value.starts_with("provider:"))
}

fn app_mode_blocks_external_mcp_tools(config: &Config) -> Option<&'static str> {
    if connection_uses_embedded_mcp(config.connection_type.as_deref()) {
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
    use crate::contracts::{
        ArtifactBundle, DesignOutput, GeometryBackend, InteractionMode, MacroDialect, Message,
        MessageRole, MessageStatus, ModelManifest,
    };
    use crate::contracts::{Config, McpConfig};
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
            voice: crate::contracts::VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            default_engine_kind: crate::contracts::EngineKind::Freecad,
            default_source_language: crate::contracts::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::contracts::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
    }

    fn test_state() -> AppState {
        AppState::new(
            test_config(),
            None,
            Connection::open_in_memory().expect("memory db"),
        )
    }

    fn read_surface_manifest_resource(state: &AppState, uri: &str) -> Value {
        let content = read_resource_content(state, uri).expect("surface JSON resource");
        assert_eq!(content.mime_type, "application/json");
        serde_json::from_str(&content.text).expect("valid surface JSON resource")
    }

    #[test]
    fn first_version_macro_context_uses_config_without_content_fallback() {
        let mut config = test_config();
        config.default_source_language = crate::contracts::SourceLanguage::EckyIrV0;
        config.default_geometry_backend = crate::contracts::GeometryBackend::EckyRust;
        let request = MacroReplaceRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: None,
            macro_code: "python_looking_text_that_must_not_select_legacy".to_string(),
            macro_dialect: None,
            ui_spec: None,
            parameters: None,
            post_processing: None,
            geometry_backend: None,
            source_window: None,
        };

        assert_eq!(
            first_version_macro_request_authoring_context(&config, &request),
            (
                crate::contracts::SourceLanguage::EckyIrV0,
                crate::contracts::GeometryBackend::EckyRust,
            )
        );
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
                vision_overrides: std::collections::HashMap::new(),
            }],
            selected_engine_id: "engine-1".to_string(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::contracts::VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: Some("api_key".to_string()),
            provider_models: crate::contracts::ProviderModels::default(),
            default_engine_kind: crate::contracts::EngineKind::Freecad,
            default_source_language: crate::contracts::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::contracts::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
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
                    vision_overrides: std::collections::HashMap::new(),
                }],
                selected_engine_id: "engine-1".to_string(),
                freecad_cmd: String::new(),
                cad_text_font_path: String::new(),
                freecad_library_roots: Vec::new(),
                assets: Vec::new(),
                microwave: None,
                voice: crate::contracts::VoiceConfig::default(),
                mcp: McpConfig::default(),
                fem_compute: crate::contracts::FemComputeConfig::default(),
                has_seen_onboarding: true,
                connection_type: Some("mcp".to_string()),
                provider_models: crate::contracts::ProviderModels::default(),
                default_engine_kind: crate::contracts::EngineKind::EckyIrV0,
                default_source_language: crate::contracts::SourceLanguage::EckyIrV0,
                default_geometry_backend: crate::contracts::GeometryBackend::EckyRust,
                max_generation_attempts: 3,
                max_verify_attempts: 0,
                projects_root: None,
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
            engine_kind: crate::contracts::EngineKind::EckyIrV0,
            source_language: crate::contracts::SourceLanguage::EckyIrV0,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            ui_spec: crate::contracts::UiSpec::default(),
            initial_params: std::collections::BTreeMap::new(),
            post_processing: None,
        }
    }

    fn ecky_test_bundle(model_id: &str) -> ArtifactBundle {
        ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::EckyIrV0,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            source_language: crate::contracts::SourceLanguage::EckyIrV0,
            content_hash: format!("hash-{model_id}"),
            artifact_version: 1,
            fcstd_path: format!("/tmp/{model_id}.FCStd"),
            manifest_path: format!("/tmp/{model_id}.json"),
            macro_path: Some(format!("/tmp/{model_id}.ecky")),
            model_stl_path: format!("/tmp/{model_id}.stl"),
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
            geometry_provenance: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: crate::contracts::EngineKind::EckyIrV0,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            source_language: crate::contracts::SourceLanguage::EckyIrV0,
            document: crate::contracts::DocumentMetadata {
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
            preview_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            tagged_anchors: std::collections::BTreeMap::new(),
            feature_graph: None,
            correspondence_graph: None,
            analysis_declarations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: crate::contracts::ManifestEnrichmentState {
                status: crate::contracts::EnrichmentStatus::None,
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
                    structural_verification: None,
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
        state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await
            .insert(
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

    async fn test_blank_dispatch_server(session_id: &str) -> HttpServerState {
        let config = test_mcp_engine_state("openai", "gpt-5.4")
            .config
            .lock()
            .unwrap()
            .clone();
        let conn = crate::db::init_db(&test_db_path("dispatch-empty-thread")).expect("db");
        let state = AppState::new(config, None, conn);
        let root = std::env::temp_dir().join(format!("ecky-mcp-server-root-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create test root");
        let resolver = Arc::new(TestPathResolver { root });
        let now = now_secs();

        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-empty", "Empty Thread", now, None)
                .expect("create empty thread");
        }

        state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await
            .insert(
                session_id.to_string(),
                McpSessionState {
                    client_kind: "mcp-http".to_string(),
                    host_label: "Codex".to_string(),
                    agent_label: "codex".to_string(),
                    llm_model_id: None,
                    llm_model_label: Some("gpt-5.4".to_string()),
                    bound_thread_id: Some("thread-empty".to_string()),
                    last_target: None,
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
                    updated_at: now,
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

    #[tokio::test]
    async fn workspace_overview_returns_bound_thread_before_its_first_version() {
        let session_id = "session-empty-thread-overview";
        let server = test_blank_dispatch_server(session_id).await;

        let response =
            dispatch_tool_call_jsonrpc(&server, session_id, "workspace_overview", json!({})).await;
        let payload = parse_mcp_tool_payload(&response);

        assert_ne!(payload["code"], "validation", "{payload:#}");
        assert_eq!(payload["defaultTarget"]["threadId"], "thread-empty");
        assert_eq!(payload["defaultTarget"]["hasVersion"], false);
        assert!(payload["defaultTarget"]["messageId"].is_null());
    }

    #[tokio::test]
    async fn exploration_stop_dispatches_cancellation_intent_to_shared_registry() {
        let session_id = "session-exploration-stop";
        let server = test_blank_dispatch_server(session_id).await;

        let response = dispatch_tool_call_jsonrpc(
            &server,
            session_id,
            "exploration_run_stop",
            json!({
                "requestId": "request-stop-1",
                "threadId": "thread-empty"
            }),
        )
        .await;

        assert!(response.error.is_none(), "{response:#?}");
        assert!(
            server
                .state
                .exploration_run_registry
                .is_cancelled("request-stop-1")
                .await,
            "MCP stop must reach the shared Rust run registry"
        );
    }

    #[tokio::test]
    async fn provider_prebind_resolves_workspace_without_thread_borrow() {
        let (state, resolver) = seed_dispatch_ecky_target("(model)").await;
        let session_id = "provider-prebound-session";
        {
            let mut sessions = state.mcp_session_registry.with_sessions().lock().await;
            let mut old = McpSessionState::new("mcp-http".to_string(), "Old Agy".to_string());
            old.bound_thread_id = Some("thread-1".to_string());
            sessions.insert("old-provider-session".to_string(), old);
            sessions.insert(
                session_id.to_string(),
                McpSessionState::new("mcp-http".to_string(), "Agy".to_string()),
            );
        }
        prebind_provider_session(&state, session_id, Some("thread-1".to_string()))
            .await
            .expect("prebind provider thread");
        let server = HttpServerState {
            state,
            app: resolver,
            handle: None,
        };

        let response =
            dispatch_tool_call_jsonrpc(&server, session_id, "workspace_overview", json!({})).await;
        let payload = parse_mcp_tool_payload(&response);
        assert_eq!(
            payload["defaultTarget"]["threadId"], "thread-1",
            "{payload:#}"
        );
        let session = server
            .state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await
            .get(session_id)
            .cloned()
            .expect("provider session");
        assert_eq!(session.client_kind, "provider-mcp-http");
        assert_eq!(session.bound_thread_id.as_deref(), Some("thread-1"));
        assert!(server
            .state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await
            .get("old-provider-session")
            .is_some_and(|session| session.bound_thread_id.is_none()));
    }

    #[test]
    fn provider_thread_query_round_trips_encoded_ids() {
        let endpoint = provider_bound_endpoint("http://127.0.0.1:39249/mcp", "thread 1/ä");
        let uri: axum::http::Uri = endpoint.parse().expect("provider endpoint URI");
        assert_eq!(
            provider_thread_id_from_uri(&uri).as_deref(),
            Some("thread 1/ä")
        );
    }

    #[tokio::test]
    async fn provider_writer_replaces_stale_target_lease_without_agent_side_borrow() {
        let (state, _) = seed_dispatch_ecky_target("(model)").await;
        let now = now_secs();
        {
            let conn = state.db.lock().await;
            db::upsert_target_lease(
                &conn,
                &TargetLeaseInfo {
                    session_id: "old-session".to_string(),
                    thread_id: "thread-1".to_string(),
                    message_id: "msg-1".to_string(),
                    model_id: Some("model-base".to_string()),
                    host_label: "Old Host".to_string(),
                    agent_label: "Old Agent".to_string(),
                    acquired_at: now,
                    expires_at: now + LEASE_TTL_SECS,
                },
            )
            .unwrap();
        }
        let target = McpTargetRef {
            thread_id: "thread-1".to_string(),
            message_id: "msg-1".to_string(),
            model_id: Some("model-base".to_string()),
        };
        let context = handlers::AgentContext {
            session_id: "provider-session".to_string(),
            client_kind: "provider-mcp-http".to_string(),
            host_label: "Ecky".to_string(),
            agent_label: "Agy".to_string(),
            llm_model_id: None,
            llm_model_label: None,
        };

        acquire_lease(&state, &context, &target)
            .await
            .expect("provider replaces stale external lease");
        let conn = state.db.lock().await;
        let active = db::get_active_target_lease(&conn, "thread-1", "msg-1", Some("model-base"))
            .unwrap()
            .expect("active provider lease");
        assert_eq!(active.session_id, "provider-session");
    }

    #[tokio::test]
    async fn version_delete_jsonrpc_moves_exact_saved_version_to_trash() {
        let server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "session-version-delete").await;

        let response = dispatch_tool_call_jsonrpc(
            &server,
            "session-version-delete",
            "version_delete",
            json!({ "messageId": "msg-1" }),
        )
        .await;
        let payload = parse_mcp_tool_payload(&response);

        assert_eq!(payload["threadId"], "thread-1");
        assert_eq!(payload["messageId"], "msg-1");
        assert_eq!(payload["deleted"], true);

        let conn = server.state.db.lock().await;
        let deleted = db::get_deleted_messages(&conn).expect("deleted messages");
        assert!(deleted.iter().any(|message| message.id == "msg-1"));
    }

    #[tokio::test]
    async fn tool_dispatch_worker_has_headroom_above_default_tokio_stack() {
        let thread_name = run_on_mcp_tool_dispatch_stack(|| async {
            let mut stack_probe = [0_u8; 3 * 1024 * 1024];
            for offset in (0..stack_probe.len()).step_by(4096) {
                stack_probe[offset] = 1;
            }
            std::hint::black_box(&mut stack_probe);
            Ok(std::thread::current()
                .name()
                .unwrap_or_default()
                .to_string())
        })
        .await
        .expect("dedicated MCP dispatch stack");

        assert_eq!(thread_name, MCP_TOOL_DISPATCH_THREAD_NAME);
    }

    fn parse_mcp_tool_payload(response: &JsonRpcResponse) -> Value {
        let result = response.result.as_ref().expect("json-rpc result");
        // Ordinary/bounded success: canonical machine output lives in
        // structuredContent. Large reads nest the payload under `data` alongside
        // observedCount/continuation metadata; ordinary reads carry the value
        // directly.
        if let Some(structured) = result.get("structuredContent") {
            if structured_content_is_bounded_read(structured) {
                return structured["data"].clone();
            }
            return structured.clone();
        }
        // Error (`isError`) or rich-content payloads carry their JSON in
        // content[0].text.
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

    fn run_async_test_with_stack<F, Fut>(stack_size: usize, run: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        std::thread::Builder::new()
            .name("mcp-server-stack-test".to_string())
            .stack_size(stack_size)
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

    fn run_async_test_with_large_stack<F, Fut>(run: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        run_async_test_with_stack(64 * 1024 * 1024, run);
    }

    #[test]
    fn targetless_workspace_overview_fits_bounded_worker_stack() {
        run_async_test_with_large_stack(|| async {
            let session_id = "session-workspace-overview-stack";
            let server = test_dispatch_server("(model)", session_id).await;
            {
                let mut sessions = server
                    .state
                    .mcp_session_registry
                    .with_sessions()
                    .lock()
                    .await;
                let session = sessions.get_mut(session_id).expect("session");
                session.bound_thread_id = None;
                session.last_target = None;
            }

            std::thread::Builder::new()
                .name("mcp-workspace-overview-worker-stack".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("test runtime");
                    runtime.block_on(async move {
                        let response = dispatch_tool_call_jsonrpc(
                            &server,
                            session_id,
                            "workspace_overview",
                            json!({}),
                        )
                        .await;
                        let result = response.result.expect("JSON-RPC result");
                        assert_eq!(result["isError"], true);
                        assert!(result["content"][0]["text"]
                            .as_str()
                            .unwrap_or_default()
                            .contains("No bound MCP session target"));
                    });
                })
                .expect("spawn worker-stack test")
                .join()
                .expect("join worker-stack test");
        });
    }

    // openspec thread-source-binding 4.1: workspace_overview.defaultTarget
    // exposes the bound source path/folder/state using the exact stored
    // binding path (camelCase JSON).
    #[test]
    fn workspace_overview_default_target_exposes_bound_source_path_folder_state() {
        run_async_test_with_large_stack(|| async {
            let session_id = "session-workspace-bound-source";
            let server = test_dispatch_server("(model (part body (box 8 8 4)))", session_id).await;
            // Bind thread-1 through the commit-sync flow (the realistic path
            // for a thread that already has a committed version).
            let binding = {
                let conn = server.state.db.lock().await;
                crate::thread_source_binding::refresh_on_version_append(
                    server.app.as_ref(),
                    &conn,
                    server.state.config.lock().unwrap().projects_root.as_deref(),
                    "thread-1",
                    "Wrapper Path",
                    "(model (part body (box 8 8 4)))",
                    "msg-1",
                    None,
                    Some("msg-1"),
                )
                .expect("refresh binds on first commit")
            };

            let response =
                dispatch_tool_call_jsonrpc(&server, session_id, "workspace_overview", json!({}))
                    .await;
            let payload = parse_mcp_tool_payload(&response);
            let default_target = payload.get("defaultTarget").expect("defaultTarget present");
            let source_path = default_target
                .get("sourcePath")
                .and_then(Value::as_str)
                .expect("sourcePath present for bound target");
            let source_folder = default_target
                .get("sourceFolder")
                .and_then(Value::as_str)
                .expect("sourceFolder present for bound target");
            let source_state = default_target
                .get("sourceState")
                .and_then(Value::as_str)
                .expect("sourceState present for bound target");
            assert_eq!(source_path, binding.source_path);
            assert_eq!(source_folder, binding.folder_path);
            assert!(source_path.ends_with("model.ecky"));
            assert_eq!(source_state, "clean");
        });
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

    // BDD dispatch-level RED -> GREEN for checkbox 10.3 (render-snapshot-
    // authority): a real bounded `target_macro_get` window, when its lines are
    // submitted back as the full `macroCode` to `macro_preview_render` with
    // `sourceWindow.acknowledgesTruncation=false`, must be rejected by the
    // dispatch guard BEFORE a lease is acquired or a render is attempted. The
    // positive branch (explicit `acknowledgesTruncation=true` passes the guard)
    // is already proven by the unit guard in `source_window_guard.rs` without
    // the heavy render infrastructure this dispatch path would require.
    #[tokio::test]
    async fn macro_preview_render_rejects_truncated_target_macro_window_before_render() {
        // 1. Seed a target whose full macro spans several lines.
        let source = "(model\n  ; line two\n  ; line three\n  ; line four\n  ; line five\n  ; line six\n  (part body (box 1 2 3)))";
        let session_id = "session-source-window-guard";
        let server = test_dispatch_server(source, session_id).await;

        // 2. Read a real BOUNDED window from target_macro_get (lines 3..5 of 7).
        let window_payload = parse_mcp_tool_payload(
            &dispatch_tool_call_jsonrpc(
                &server,
                session_id,
                "target_macro_get",
                json!({
                    "threadId": "thread-1",
                    "messageId": "msg-1",
                    "startLine": 3,
                    "endLine": 5
                }),
            )
            .await,
        );
        let full_size_line_count = window_payload["lineCount"].as_u64().expect("lineCount");
        let window_start_line = window_payload["windowStartLine"]
            .as_u64()
            .expect("windowStartLine");
        let window_end_line = window_payload["windowEndLine"]
            .as_u64()
            .expect("windowEndLine");
        let window_lines = window_payload["lines"]
            .as_array()
            .expect("lines array")
            .iter()
            .map(|line| {
                line.get("text")
                    .and_then(Value::as_str)
                    .expect("line text")
                    .to_string()
            })
            .collect::<Vec<_>>();
        let observed_line_count = window_lines.len();
        let submitted_macro_code = window_lines.join("\n");

        // Sanity: this really is a truncated window (3..5 of 7), so the guard
        // path is the one under test.
        assert_eq!(window_start_line, 3);
        assert_eq!(window_end_line, 5);
        assert_eq!(full_size_line_count, 7);
        assert!(window_start_line > 1 || window_end_line < full_size_line_count);

        // 3. Submit that exact window back as a full replacement WITHOUT
        //    acknowledging the truncation.
        let response = dispatch_tool_call_jsonrpc(
            &server,
            session_id,
            "macro_preview_render",
            json!({
                "threadId": "thread-1",
                "messageId": "msg-1",
                "macroCode": submitted_macro_code,
                "sourceWindow": {
                    "fullSizeLineCount": full_size_line_count,
                    "windowStartLine": window_start_line,
                    "windowEndLine": window_end_line,
                    "observedLineCount": observed_line_count,
                    "acknowledgesTruncation": false
                }
            }),
        )
        .await;

        // 4. The dispatch guard rejects it as a validation error before any
        //    render/lease work. isError is set and the raw AppError survives.
        let result = response.result.as_ref().expect("json-rpc result");
        assert_eq!(result["isError"], true, "truncated window must be rejected");
        let err_payload = parse_mcp_tool_payload(&response);
        assert_eq!(err_payload["code"], "validation");
        let message = err_payload["message"].as_str().expect("error message");
        assert!(
            message.contains("truncated target_macro_get window"),
            "expected truncation rejection, got: {message}"
        );
        assert!(message.contains("acknowledgesTruncation"));
        // The rejection must reference the declared window bounds, proving the
        // guard consumed the sourceWindow metadata rather than guessing.
        assert!(message.contains("lines 3..5"));
        assert!(message.contains("fullSizeLineCount 7"));
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
    fn provider_integration_mode_keeps_embedded_mcp_tools_enabled() {
        let mut config = test_api_key_config();
        config.connection_type = Some("provider:codex".to_string());

        assert_eq!(app_mode_blocks_external_mcp_tools(&config), None);
        ensure_mcp_tool_allowed_for_app_mode(&config, "session_log_in")
            .expect("provider adapter must reach Ecky MCP tools");
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
    fn tool_definitions_include_component_library_tools() {
        let tool_names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>();

        for expected in [
            "component_extract",
            "component_search",
            "component_get",
            "component_import",
            "project_folder_export",
            "project_folder_status",
        ] {
            assert!(
                tool_names.iter().any(|name| name == expected),
                "expected {expected} in {tool_names:?}"
            );
        }
    }

    #[test]
    fn component_extract_tool_handler_extracts_and_saves_to_library() {
        let resolver = TestPathResolver {
            root: std::env::temp_dir().join(format!(
                "ecky-mcp-component-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )),
        };
        let source = r#"
            (model
              (params (number width 12 :label "Width"))
              (part bracket (box width 4 2)))
        "#;
        let response = handlers::handle_component_extract(
            &resolver,
            serde_json::from_value(json!({
                "source": source,
                "partKey": "bracket",
                "description": "Test bracket",
                "tags": ["bracket"],
                "save": true
            }))
            .expect("request parses"),
        )
        .expect("extract");
        assert_eq!(response.name, "bracket");
        assert!(response
            .component_source
            .contains("(define-component bracket"));
        assert!(response.saved_path.is_some());

        let search = handlers::handle_component_search(
            &resolver,
            serde_json::from_value(json!({ "query": "bracket" })).expect("request parses"),
        )
        .expect("search");
        assert!(!search.results.is_empty());
        assert!(search.results.iter().any(|entry| entry.name == "bracket"));

        let record = handlers::handle_component_get(
            &resolver,
            serde_json::from_value(json!({ "name": "bracket" })).expect("request parses"),
        )
        .expect("get");
        assert!(record.source.contains("(define-component bracket"));
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
    fn exploration_tools_submit_intent_without_caller_authored_lifecycle_facts() {
        let tools = tool_definitions();
        let names = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();

        for expected in [
            "exploration_run_start",
            "exploration_cycle_get",
            "exploration_cycle_active_get",
            "exploration_cycle_events",
            "exploration_cycle_answer",
            "exploration_run_stop",
        ] {
            assert!(
                names.contains(&expected),
                "missing exploration tool {expected}"
            );
        }
        for forbidden in [
            "exploration_cycle_start",
            "exploration_cycle_next",
            "exploration_cycle_stop",
        ] {
            assert!(
                !names.contains(&forbidden),
                "MCP must not expose lifecycle-authoring tool {forbidden}"
            );
        }

        let start = tools
            .iter()
            .find(|tool| tool["name"] == "exploration_run_start")
            .expect("run start tool");
        let properties = start["inputSchema"]["properties"]
            .as_object()
            .expect("start properties");
        assert!(properties.contains_key("requestId"));
        assert!(properties.contains_key("threadId"));
        assert!(properties.contains_key("prompt"));
        for forbidden in [
            "action",
            "phase",
            "buildStarted",
            "resultVersionId",
            "verification",
            "decision",
        ] {
            assert!(
                !properties.contains_key(forbidden),
                "caller must not submit lifecycle fact {forbidden}"
            );
        }
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
    fn topology_tool_starts_from_authored_study_without_caller_model_controls_or_mesh_digests() {
        let tool = tool_definitions()
            .into_iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("fem_topology_run"))
            .expect("topology tool");
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .expect("topology properties");
        assert!(properties.contains_key("analysisName"));
        assert!(!properties.contains_key("controls"));
        assert!(!properties.contains_key("analysisIdentityDigest"));
        assert!(!properties.contains_key("meshContentDigest"));
        assert!(!tool["description"]
            .as_str()
            .expect("description")
            .contains("from an immutable fem_mesh_preview artifact"));
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
        assert!(!tool_names
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
            is_blank: false,
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
                &crate::contracts::Message {
                    id: "user-1".to_string(),
                    role: crate::contracts::MessageRole::User,
                    content: "make a thing".to_string(),
                    status: crate::contracts::MessageStatus::Working,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    structural_verification: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
        }
        state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await
            .insert(
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
    async fn explicit_saved_message_id_does_not_resolve_draft_base_alias() {
        let session_id = "session-explicit-saved-target";
        let server = test_dispatch_server("(model (part body (box 1 2 3)))", session_id).await;
        let session = server
            .state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await
            .get(session_id)
            .cloned()
            .expect("MCP session");
        let ctx = current_context(session_id, &session);
        let preview = handlers::store_session_render_preview(
            &server.state,
            server.app.as_ref(),
            &ctx,
            handlers::StoreSessionRenderPreviewRequest {
                thread_id: "thread-1".to_string(),
                base_message_id: Some("msg-1".to_string()),
                design_output: ecky_test_design(
                    "Draft version",
                    "V-draft",
                    "(model (part draft (box 4 5 6)))",
                ),
                artifact_bundle: ecky_test_bundle("model-draft"),
                model_manifest: ecky_test_manifest("model-draft"),
                draft_feedback: None,
            },
        )
        .await
        .expect("store draft preview");

        let saved = resolve_target_for_session(
            &server.state,
            server.app.as_ref(),
            session_id,
            Some("thread-1".to_string()),
            Some("msg-1".to_string()),
        )
        .await
        .expect("saved target");
        assert_eq!(saved.message_id, "msg-1");
        assert_eq!(saved.model_id.as_deref(), Some("model-base"));
        assert!(!saved.has_draft);

        let draft = resolve_target_for_session(
            &server.state,
            server.app.as_ref(),
            session_id,
            Some("thread-1".to_string()),
            Some(preview.preview_id.clone()),
        )
        .await
        .expect("draft target");
        assert_eq!(draft.message_id, preview.preview_id);
        assert_eq!(draft.model_id.as_deref(), Some("model-draft"));
        assert!(draft.has_draft);
    }

    #[tokio::test]
    async fn tagged_draft_ref_requires_exact_preview_session_and_thread() {
        let session_id = "session-tagged-draft-target";
        let server = test_dispatch_server("(model (part body (box 1 2 3)))", session_id).await;
        let session = server
            .state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await
            .get(session_id)
            .cloned()
            .expect("MCP session");
        let ctx = current_context(session_id, &session);
        let preview = handlers::store_session_render_preview(
            &server.state,
            server.app.as_ref(),
            &ctx,
            handlers::StoreSessionRenderPreviewRequest {
                thread_id: "thread-1".to_string(),
                base_message_id: Some("msg-1".to_string()),
                design_output: ecky_test_design(
                    "Draft version",
                    "V-draft",
                    "(model (part draft (box 4 5 6)))",
                ),
                artifact_bundle: ecky_test_bundle("model-draft"),
                model_manifest: ecky_test_manifest("model-draft"),
                draft_feedback: None,
            },
        )
        .await
        .expect("store draft preview");
        let preview_id = preview.preview_id.clone();

        let resolved = resolve_authoring_target_for_session(
            &server.state,
            server.app.as_ref(),
            session_id,
            Some(AuthoringTargetRef::Draft {
                thread_id: "thread-1".to_string(),
                preview_id: preview_id.clone(),
                session_id: session_id.to_string(),
            }),
        )
        .await
        .expect("exact draft target");
        assert!(resolved.has_draft);

        let requested = AuthoringTargetRef::Draft {
            thread_id: "thread-other".to_string(),
            preview_id: preview_id.clone(),
            session_id: session_id.to_string(),
        };
        let err = resolve_authoring_target_for_session(
            &server.state,
            server.app.as_ref(),
            session_id,
            Some(requested.clone()),
        )
        .await
        .expect_err("wrong thread must not resolve draft");
        assert_eq!(err.code, AppErrorCode::Conflict);
        let evidence: Value = serde_json::from_str(err.details.as_deref().expect("evidence"))
            .expect("target-resolution JSON");
        assert_eq!(evidence["kind"], "stale");
        assert_eq!(
            evidence["requestedTarget"],
            serde_json::to_value(requested).unwrap()
        );
        assert_eq!(evidence["resolvedTarget"]["kind"], "draft");
        assert_eq!(evidence["resolvedTarget"]["threadId"], "thread-1");

        let wrong_session = AuthoringTargetRef::Draft {
            thread_id: "thread-1".to_string(),
            preview_id,
            session_id: "session-other".to_string(),
        };
        let err = resolve_authoring_target_for_session(
            &server.state,
            server.app.as_ref(),
            session_id,
            Some(wrong_session.clone()),
        )
        .await
        .expect_err("wrong session must not resolve draft");
        assert_eq!(err.code, AppErrorCode::Conflict);
        let evidence: Value = serde_json::from_str(err.details.as_deref().expect("evidence"))
            .expect("target-resolution JSON");
        assert_eq!(evidence["kind"], "stale");
        assert_eq!(
            evidence["requestedTarget"],
            serde_json::to_value(wrong_session).unwrap()
        );
        assert!(evidence["resolvedTarget"].is_null());
    }

    #[tokio::test]
    async fn missing_tagged_draft_returns_typed_requested_and_resolved_evidence() {
        let session_id = "session-missing-tagged-draft";
        let server = test_dispatch_server("(model (part body (box 1 2 3)))", session_id).await;
        let requested = AuthoringTargetRef::Draft {
            thread_id: "thread-1".to_string(),
            preview_id: "preview-missing".to_string(),
            session_id: session_id.to_string(),
        };

        let err = resolve_authoring_target_for_session(
            &server.state,
            server.app.as_ref(),
            session_id,
            Some(requested.clone()),
        )
        .await
        .expect_err("missing preview must not fall back to saved version");
        assert_eq!(err.code, AppErrorCode::NotFound);
        let evidence: Value = serde_json::from_str(err.details.as_deref().expect("evidence"))
            .expect("target-resolution JSON");
        assert_eq!(evidence["kind"], "notFound");
        assert_eq!(
            evidence["requestedTarget"],
            serde_json::to_value(requested).unwrap()
        );
        assert!(evidence["resolvedTarget"].is_null());
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
            offset: 0,
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
        assert_eq!(message.role, crate::contracts::MessageRole::Assistant);
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
        assert!(required.contains(&"modelStlPath"));
        assert!(required.contains(&"contentHash"));
    }

    #[test]
    fn tool_definitions_are_all_dispatched() {
        let defined = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<std::collections::BTreeSet<_>>();
        let dispatched = dispatched_tool_names();

        let missing = defined.difference(&dispatched).cloned().collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "tool_definitions advertised tools without dispatch handlers: {:?}",
            missing
        );
    }

    // ── OpenSpec agent-context-budgeting §5.1: capability-group drift ──────
    #[test]
    fn capability_group_drift_every_defined_and_dispatched_tool_in_exactly_one_group() {
        use std::collections::{BTreeMap, BTreeSet};

        // The authoritative tool roster is the union of both AST-authoring
        // configs. The source-scraping `dispatched_tool_names()` helper also
        // picks up nested match-arm literals (e.g. file-extension arms inside
        // freecad_library_import), so we intersect it with the defined roster
        // to recover the real dispatched tools and re-assert defined⊆dispatched.
        let mut defined_names: BTreeSet<String> = BTreeSet::new();
        for ast_enabled in [false, true] {
            for tool in tool_definitions_with_ast_enabled(ast_enabled) {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("tool has a name")
                    .to_string();
                defined_names.insert(name);
            }
        }
        let dispatched = dispatched_tool_names();
        let missing_handler: Vec<String> = defined_names.difference(&dispatched).cloned().collect();
        assert!(
            missing_handler.is_empty(),
            "every defined tool must have a dispatch handler; missing handlers: {missing_handler:?}"
        );

        let real_dispatched: BTreeSet<String> =
            dispatched.intersection(&defined_names).cloned().collect();
        let all_names: BTreeSet<String> = defined_names
            .iter()
            .cloned()
            .chain(real_dispatched)
            .collect();

        // (1) Every defined and dispatched tool must resolve to exactly one
        //     capability group.
        let mut unmapped: Vec<String> = Vec::new();
        for name in &all_names {
            if tool_capability_group(name).is_none() {
                unmapped.push(name.clone());
            }
        }
        assert!(
            unmapped.is_empty(),
            "every defined/dispatched tool must belong to exactly one capability \
             group; unmapped tools: {unmapped:?}"
        );

        // (2) tool_capability_group is a pure name→group function, so a name
        //     cannot belong to two groups. Verify by counting the partition for
        //     each AST config: every defined tool lands in exactly one bucket.
        for ast_enabled in [false, true] {
            let tools = tool_definitions_with_ast_enabled(ast_enabled);
            let total = tools.len();
            let mut grouped = 0usize;
            let mut by_group: BTreeMap<&'static str, usize> = BTreeMap::new();
            for tool in &tools {
                let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
                let group = tool_capability_group(name).expect("mapped above");
                *by_group.entry(group.id()).or_insert(0) += 1;
                grouped += 1;
            }
            assert_eq!(
                grouped, total,
                "partition dropped tools for ast_enabled={ast_enabled}"
            );
            // Every group present in this config owns at least one tool.
            assert!(
                by_group.values().all(|count| *count > 0),
                "empty groups in partition for ast_enabled={ast_enabled}: {by_group:?}"
            );
        }

        // (3) The capability discovery/enable controls live in core so compact
        //     managed sessions can advertise them without loading specialists.
        assert_eq!(
            tool_capability_group("capability_search"),
            Some(CapabilityGroup::Core),
            "capability_search must belong to the core group"
        );
        assert_eq!(
            tool_capability_group("capability_enable"),
            Some(CapabilityGroup::Core),
            "capability_enable must belong to the core group"
        );

        // (4) No specialist tool (e.g. ecky_ast_get_node) leaks into core, so a
        //     compact session cannot accidentally advertise it.
        assert_ne!(
            tool_capability_group("ecky_ast_get_node"),
            Some(CapabilityGroup::Core),
            "specialist AST tools must not belong to core"
        );

        // (5) Core stays narrow enough for compact discovery (core only, no
        //     enabled groups).
        let core_only = compact_managed_tool_definitions(&std::collections::HashSet::new(), true);
        assert!(
            core_only.len() <= 15,
            "compact-managed core must stay narrow (<=15), got {}",
            core_only.len()
        );

        // (6) Every advertised capability group has at least one tool across
        //     the AST configs, so no group id is an empty promise.
        let every_group_has_tools = CapabilityGroup::all().iter().all(|group| {
            [false, true]
                .iter()
                .any(|ast_enabled| !group_tool_definitions(*group, *ast_enabled).is_empty())
        });
        assert!(
            every_group_has_tools,
            "every capability group must own at least one tool in some config"
        );
    }

    // ── §5.4 full compatibility profile preserves every name + paginates ────
    #[test]
    fn full_compatibility_profile_preserves_every_tool_name_and_paginates_with_opaque_cursor() {
        let all = tool_definitions_with_ast_enabled(true);
        let all_names: std::collections::BTreeSet<String> = all
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();

        // Page through the full catalogue with a small page size and an opaque
        // cursor; collect every observed name and confirm no name is renamed or
        // dropped, and that the final page has no nextCursor.
        let mut observed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        loop {
            let (page, next) = paginate_tools(&all, cursor.as_deref(), Some(25));
            pages += 1;
            for tool in &page {
                observed.insert(
                    tool.get("name")
                        .and_then(Value::as_str)
                        .expect("tool name")
                        .to_string(),
                );
            }
            cursor = next;
            if cursor.is_none() {
                break;
            }
            assert!(pages <= 20, "pagination did not terminate");
        }

        assert!(
            pages > 1,
            "full catalogue spanning pages must produce more than one page"
        );
        assert_eq!(
            observed, all_names,
            "full compatibility profile must preserve every existing tool name"
        );

        // Opaque cursor: a client-supplied cursor must be opaque base64 and
        // round-trip through decode_tools_cursor.
        let (first_page, first_next) = paginate_tools(&all, None, Some(10));
        assert_eq!(first_page.len(), 10);
        let next = first_next.expect("more tools remain");
        assert!(
            !next.contains(':'),
            "cursor must be opaque (no plaintext offset delimiter): {next}"
        );
        assert_eq!(
            decode_tools_cursor(Some(&next)),
            10,
            "opaque cursor must decode back to the offset"
        );
    }

    #[tokio::test]
    async fn ecky_authoring_tools_require_guide_reads_before_source_changes() {
        let state = test_mcp_engine_state("openai", "gpt-5.4");

        let err = ensure_authoring_guides_read(
            &state,
            "session-1",
            crate::contracts::SourceLanguage::EckyIrV0,
            crate::contracts::GeometryBackend::Build123d,
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
            crate::contracts::SourceLanguage::EckyIrV0,
            crate::contracts::GeometryBackend::Build123d,
        ) {
            mark_session_resource_read(&state, "session-1", uri).await;
        }

        ensure_authoring_guides_read(
            &state,
            "session-1",
            crate::contracts::SourceLanguage::EckyIrV0,
            crate::contracts::GeometryBackend::Build123d,
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
            crate::contracts::SourceLanguage::EckyIrV0,
            crate::contracts::GeometryBackend::Freecad,
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
            crate::contracts::SourceLanguage::LegacyPython,
            crate::contracts::GeometryBackend::Freecad,
            "macro_preview_render",
        )
        .await
        .expect("legacy source edits should not require ecky guide resources");
    }

    #[tokio::test]
    async fn mcp_http_sessions_bypass_resource_read_guard_for_ecky_authoring_tools() {
        let state = test_mcp_engine_state("openai", "gpt-5.4");
        state
            .mcp_session_registry
            .with_sessions()
            .lock()
            .await
            .insert(
                "session-http".to_string(),
                McpSessionState::new("mcp-http".to_string(), "Codex".to_string()),
            );

        ensure_authoring_guides_read(
            &state,
            "session-http",
            crate::contracts::SourceLanguage::EckyIrV0,
            crate::contracts::GeometryBackend::Build123d,
            "macro_preview_render",
        )
        .await
        .expect("tool-only mcp-http sessions cannot satisfy resources/read guard");
    }

    #[test]
    fn fem_mcp_surface_exposes_guarded_inspect_to_verified_publish_flow() {
        let tools = tool_definitions_with_ast_enabled(true);
        for name in [
            "ecky_ast_get",
            "fem_validate",
            "fem_mesh_preview",
            "fem_run",
            "fem_topology_run",
            "fem_topology_reconstruct",
            "fem_result_get",
            "verify_generated_model",
            "fem_publish_verified_result",
        ] {
            assert!(
                tools
                    .iter()
                    .any(|tool| tool.get("name").and_then(Value::as_str) == Some(name)),
                "missing MCP FEM flow stage {name}"
            );
        }
        assert_eq!(
            tool_capability_group("fem_publish_verified_result"),
            Some(CapabilityGroup::FemAnalysis)
        );
        let topology = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("fem_topology_run"))
            .expect("FEM topology tool");
        let topology_description = topology["description"].as_str().unwrap();
        for invariant in [
            "binary density",
            "no exact BRep",
            "production STEP",
            "stress acceptance",
        ] {
            assert!(
                topology_description.contains(invariant),
                "missing topology boundary {invariant}"
            );
        }
        let reconstruction = tools
            .iter()
            .find(|tool| {
                tool.get("name").and_then(Value::as_str) == Some("fem_topology_reconstruct")
            })
            .expect("generic FEM topology reconstruction tool");
        let reconstruction_description = reconstruction["description"].as_str().unwrap();
        for invariant in [
            "generic",
            "closed manifold",
            "faceted BRep",
            "independent FEM",
        ] {
            assert!(
                reconstruction_description.contains(invariant),
                "missing reconstruction invariant {invariant}"
            );
        }
        let publish = tools
            .iter()
            .find(|tool| {
                tool.get("name").and_then(Value::as_str) == Some("fem_publish_verified_result")
            })
            .expect("FEM verified publish tool");
        let description = publish["description"].as_str().unwrap();
        for invariant in [
            "decision-ready",
            "source",
            "OCCT analysis boundary",
            "stale",
            "red",
            "corrupt",
        ] {
            assert!(
                description.contains(invariant),
                "missing invariant {invariant}"
            );
        }
        let required = publish["inputSchema"]["required"]
            .as_array()
            .expect("required fields");
        for name in [
            "analysisName",
            "analysisIdentityDigest",
            "solutionDigest",
            "resultDigest",
        ] {
            assert!(required.iter().any(|value| value.as_str() == Some(name)));
        }
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
        assert!(description.contains("authored `(verify ...)` clauses"));
        assert!(description.contains("automatically attaches pass/fail evidence"));
        assert!(description.contains("no commit or finalize step"));
        assert!(!tools.iter().any(|tool| {
            tool.get("name").and_then(Value::as_str) == Some("commit_preview_version")
        }));

        let printability = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some("printability_analyze"))
            .expect("printability_analyze tool");
        let description = printability
            .get("description")
            .and_then(Value::as_str)
            .expect("printability_analyze description");
        assert!(description.contains("Read-only"));
        assert!(description.contains("model STL"));
        assert!(description.contains("artifactDigest"));
    }

    #[test]
    fn export_mcp_tool_catalog_lists_core_tools() {
        let catalog = export_mcp_tool_catalog();
        assert!(!catalog.is_empty());
        let names: std::collections::BTreeSet<String> = catalog
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .map(|name| name.to_string())
            .collect();
        for expected in ["health_check", "workspace_overview", "macro_preview_render"] {
            assert!(names.contains(expected), "missing tool: {expected}");
        }
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
    fn preview_render_tools_expose_native_backend_aliases() {
        let tools = tool_definitions();

        for tool_name in ["params_preview_render", "macro_preview_render"] {
            let tool = tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(tool_name))
                .expect("preview render tool");
            let backend_enum = tool
                .get("inputSchema")
                .and_then(|schema| schema.get("properties"))
                .and_then(|properties| properties.get("geometryBackend"))
                .and_then(|backend| backend.get("enum"))
                .and_then(Value::as_array)
                .expect("geometryBackend enum");
            let values = backend_enum
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();

            assert!(values.contains(&"mesh"), "{tool_name} missing mesh");
            assert!(values.contains(&"native"), "{tool_name} missing native");
            assert!(
                values.contains(&"eckyRust"),
                "{tool_name} missing legacy alias"
            );
        }
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
        assert!(text.contains("call `verify_generated_model`; it attaches evidence"));
        assert!(text.contains("Each changed draft is retained"));
        assert!(text.contains("report capped red honestly"));
        for uri in [
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

        for expected in [
            "(model ...)",
            "Current fileExtension: `.ecky`.",
            "Current sourceLanguage: `ecky`.",
            "`mesh` and `polyhedron`",
            "`protrude`",
            "single perspective image",
            "faceted poly-BRep",
        ] {
            assert!(ir_guide.contains(expected), "guide missing `{expected}`");
        }
        assert!(resource_definitions()
            .into_iter()
            .any(|resource| resource.get("uri").and_then(Value::as_str)
                == Some("ecky://guides/ecky-source")));
        assert!(resource_definitions()
            .into_iter()
            .any(|resource| resource.get("uri").and_then(Value::as_str)
                == Some("ecky://guides/ecky-rust")));
        assert!(!resource_definitions()
            .into_iter()
            .any(|resource| resource.get("uri").and_then(Value::as_str)
                == Some("ecky://guides/ecky-ir-v0")));
    }

    #[test]
    fn mcp_language_resources_share_language_but_preserve_tool_access() {
        let state = test_state();
        let configured = state.config.lock().unwrap().default_geometry_backend;
        for (uri, backend) in [
            ("ecky://guides/ecky-source", configured),
            ("ecky://guides/ecky-ir-v0", configured),
            ("ecky://guides/technical-system-prompt", configured),
            ("ecky://guides/freecad", GeometryBackend::Freecad),
            ("ecky://guides/cad-sdk", GeometryBackend::Freecad),
            ("ecky://guides/ecky-rust", GeometryBackend::EckyRust),
            ("ecky://guides/mesh", GeometryBackend::EckyRust),
            (
                "ecky://guides/ecky-source/freecad",
                GeometryBackend::Freecad,
            ),
            (
                "ecky://guides/ecky-source/ecky-rust",
                GeometryBackend::EckyRust,
            ),
        ] {
            let resource = read_resource_text(&state, uri).expect("language resource exists");
            assert!(
                !resource.contains("You have no tools"),
                "{uri} must not give API-only instructions to an MCP caller"
            );
            assert!(
                resource.contains("sourcePath"),
                "{uri} must explain bound-file edits"
            );
            assert!(
                resource.contains("verify_generated_model"),
                "{uri} must require render evidence"
            );
            let api = crate::agent_prompt::agent_language_reference(backend);
            let language_marker = "# Ecky language reference";
            assert_eq!(
                resource.split_once(language_marker).unwrap().1,
                api.split_once(language_marker).unwrap().1,
                "{uri} must retain the same language body and operation catalogue"
            );
        }
    }

    #[test]
    fn mcp_surface_manifest_resources_are_listed_with_json_mime() {
        let resources = resource_definitions();

        for uri in [
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
                "coreConstants",
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

        {
            let uri = "ecky://guides/surface-manifest/freecad";
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
                    .contains("native mesh only")
        }));
    }

    #[test]
    fn mcp_surface_reference_resources_are_listed_and_readable() {
        let state = test_state();
        let resources = resource_definitions();

        for (uri, backend, wall_expected) in [
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
    fn freecad_resource_exposes_backend_guidance_without_retired_extensions() {
        let state = test_state();
        let guide =
            read_resource_text(&state, "ecky://guides/freecad").expect("freecad guide resource");

        assert!(guide.contains("Current fileExtension: `.ecky`."));
        assert!(guide.contains("Current sourceLanguage: `ecky`."));
        assert!(guide.contains("Target geometryBackend: `freecad`."));
        assert!(guide.contains("Return one complete `(model ...)` program."));
        assert!(guide.contains("Backend support is authoritative."));
        assert!(guide.contains("Write top-level `verify` clauses"));
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

    fn compact_test_design(macro_code: &str) -> crate::contracts::DesignOutput {
        crate::contracts::DesignOutput {
            title: "Render".to_string(),
            version_name: "V-render".to_string(),
            response: "ok".to_string(),
            interaction_mode: crate::contracts::InteractionMode::Design,
            macro_code: macro_code.to_string(),
            macro_dialect: crate::contracts::MacroDialect::Legacy,
            engine_kind: crate::contracts::EngineKind::Freecad,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            ui_spec: crate::contracts::UiSpec::default(),
            initial_params: std::collections::BTreeMap::from([(
                "diameter".to_string(),
                crate::contracts::ParamValue::Number(42.0),
            )]),
            post_processing: None,
        }
    }

    fn compact_test_bundle(model_id: &str) -> crate::contracts::ArtifactBundle {
        crate::contracts::ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::Freecad,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            content_hash: format!("hash-{model_id}"),
            artifact_version: 1,
            fcstd_path: format!("/tmp/{model_id}.FCStd"),
            manifest_path: format!("/tmp/{model_id}.json"),
            macro_path: Some(format!("/tmp/{model_id}.py")),
            model_stl_path: format!("/tmp/{model_id}.stl"),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: vec![crate::contracts::ExportArtifact {
                geometry_provenance: None,
                label: "STEP".to_string(),
                format: "step".to_string(),
                path: format!("/tmp/{model_id}.step"),
                role: "cad-exchange".to_string(),
            }],
        }
    }

    fn compact_test_manifest(model_id: &str) -> crate::contracts::ModelManifest {
        crate::contracts::ModelManifest {
            geometry_provenance: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: crate::contracts::EngineKind::Freecad,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            document: crate::contracts::DocumentMetadata {
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
            preview_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            tagged_anchors: std::collections::BTreeMap::new(),
            feature_graph: None,
            correspondence_graph: None,
            analysis_declarations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: crate::contracts::ManifestEnrichmentState {
                status: crate::contracts::EnrichmentStatus::None,
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
            has_model_stl: true,
            viewer_asset_count: 0,
            edge_target_count: 0,
            face_target_count: 0,
            export_format_count: 1,
            export_formats: vec!["step".to_string()],
            has_step_export: true,
            step_export_path: Some("/tmp/model-render.step".to_string()),
            multipart: false,
            geometry_representation: None,
            faceted_step: false,
            analytic_step: false,
            source_mesh_digests: Vec::new(),
            component_placement_count: 0,
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
            has_model_stl: true,
            viewer_asset_count: 0,
            edge_target_count: 0,
            face_target_count: 0,
            export_format_count: 0,
            export_formats: Vec::new(),
            has_step_export: false,
            step_export_path: None,
            multipart: false,
            geometry_representation: None,
            faceted_step: false,
            analytic_step: false,
            source_mesh_digests: Vec::new(),
            component_placement_count: 0,
        };
        let manifest = compact_test_manifest("model-render");
        let mut design = compact_test_design("(model\n  (box 10 20 30))");
        design.source_language = crate::contracts::SourceLanguage::EckyIrV0;
        design.geometry_backend = crate::contracts::GeometryBackend::Build123d;

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
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            model_stl_path: Some("/tmp/model.stl".to_string()),
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
        // 6.4: structured screenshot metadata is byte-free. The base64 payload
        // lives exactly once in the MCP image content item; structuredContent
        // carries MIME type/identity/dimensions/camera only — no `dataUrl` and
        // no duplicate `base64`.
        assert!(!recursive_has_key(&result["structuredContent"], "dataUrl"));
        assert!(!recursive_has_key(&result["structuredContent"], "base64"));
        assert_eq!(
            result["structuredContent"]["image"]["mimeType"],
            "image/jpeg"
        );
    }

    // ── OpenSpec agent-context-budgeting section 1: OUTER RED (MCP) ────────
    //
    // These pin the MCP discovery/result/screenshot contract before any wiring
    // lands. They are expected to FAIL today (red) for the missing capability.
    // No capability grouping, projection, pagination, structured-content, or
    // screenshot de-duplication is implemented here — only the smallest
    // protocol assertions at the JSON-RPC envelope seam.

    fn jsonrpc(id: i64, method: &str, params: Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: Some(params),
            id: Some(json!(id)),
        }
    }

    // ── 1.3 compact managed MCP discovery / capability controls ────────────
    #[tokio::test]
    async fn compact_managed_tools_list_is_narrow_with_capability_controls() {
        let server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "session-compact-list").await;

        // Compact managed discovery: tools/list must return only core workflow
        // tools plus capability discovery/enable controls — specialist group
        // schemas stay absent until a group is enabled.
        let compact = dispatch_request(
            &server,
            "session-compact-list",
            jsonrpc(1, "tools/list", json!({ "profile": "compact-managed" })),
        )
        .await;
        let compact_result = compact.result.as_ref().expect("tools/list result");
        let tools = compact_result["tools"]
            .as_array()
            .expect("tools/list returns a tools array");
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("tool name"))
            .collect();

        // RED — today tools/list ignores any profile and returns the full eager
        // catalogue (~80 tools), so a compact managed session is not narrow.
        assert!(
            tools.len() <= 15,
            "compact-managed tools/list must return core + discovery controls \
             (<= 15 schemas); today it eagerly returns {} tools",
            tools.len()
        );

        // RED — specialist group schemas must be absent until explicitly enabled.
        assert!(
            !names.contains(&"ecky_ast_get_node"),
            "specialist AST tools must be absent from compact-managed discovery \
             until a capability group is enabled"
        );

        // RED — a capability discovery/enable control must be advertised so the
        // agent can load specialist groups on demand (and, downstream, trigger
        // `notifications/tools/list_changed`).
        assert!(
            names
                .iter()
                .any(|n| { n.contains("capability") || n.contains("search_tools") }),
            "compact-managed discovery must advertise a capability search/enable control; \
             found {} tools, none named capability/search_tools",
            names.len()
        );
    }

    // ── 1.3 listChanged advertisement + full-compatibility pagination ──────
    #[tokio::test]
    async fn mcp_server_advertises_list_changed_and_paginates_full_catalogue() {
        let server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "session-full-page").await;

        // (a) RED — initialize capabilities must advertise `tools.listChanged`
        //     so compact managed clients can react to on-demand capability
        //     enablement. Today capabilities.tools is an empty object.
        let init_body = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "clientInfo": { "name": "outer-red-client" }
            },
            "id": 1
        }))
        .expect("serialize initialize");
        let response = handle_http_post(
            axum::extract::State(server.clone()),
            "/".parse::<axum::http::Uri>().expect("uri"),
            HeaderMap::new(),
            init_body,
        )
        .await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("initialize body bytes");
        let init_payload: Value =
            serde_json::from_slice(&bytes).expect("initialize json-rpc payload");
        assert_eq!(
            init_payload["result"]["capabilities"]["tools"]["listChanged"],
            json!(true),
            "initialize must advertise tools.listChanged so compact managed clients \
             can react to on-demand capability enablement"
        );

        // (b) RED — full-compatibility pagination: a large catalogue requested
        //     with a small page must span pages and report a standard
        //     continuation cursor. Today tools/list ignores cursor/pageSize and
        //     returns one unbounded page.
        let paged = dispatch_request(
            &server,
            "session-full-page",
            jsonrpc(
                2,
                "tools/list",
                json!({ "profile": "full", "cursor": null, "pageSize": 25 }),
            ),
        )
        .await;
        assert!(
            paged
                .result
                .as_ref()
                .and_then(|v| v.get("nextCursor"))
                .is_some(),
            "full-compatibility tools/list must honor standard cursor pagination and \
             return nextCursor when the catalogue spans pages; today it returns one page"
        );
    }

    fn modern_request_headers(method: &str, name: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept",
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert(
            "mcp-protocol-version",
            HeaderValue::from_static("2026-07-28"),
        );
        headers.insert(
            "mcp-method",
            HeaderValue::from_str(method).expect("method header"),
        );
        if let Some(name) = name {
            headers.insert(
                "mcp-name",
                HeaderValue::from_str(name).expect("name header"),
            );
        }
        headers
    }

    fn modern_request_body(id: u64, method: &str, params: Value) -> String {
        let mut params = params.as_object().cloned().unwrap_or_default();
        params.insert(
            "_meta".to_string(),
            json!({
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientInfo": {
                    "name": "modern-contract-test",
                    "version": "1.0.0"
                },
                "io.modelcontextprotocol/clientCapabilities": {}
            }),
        );
        serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))
        .expect("modern request body")
    }

    #[tokio::test]
    async fn modern_mcp_discovery_is_stateless_and_self_describing() {
        let server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "legacy-session").await;
        let response = handle_http_post(
            axum::extract::State(server.clone()),
            "/mcp".parse::<axum::http::Uri>().expect("uri"),
            modern_request_headers("server/discover", None),
            modern_request_body(1, "server/discover", json!({})),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers().get(SESSION_HEADER).is_none(),
            "modern MCP must not emit protocol-level session ids"
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("discovery body");
        let payload: Value = serde_json::from_slice(&bytes).expect("discovery json");
        assert_eq!(payload["result"]["resultType"], "complete");
        assert_eq!(payload["result"]["supportedVersions"][0], "2026-07-28");
        assert_eq!(
            payload["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "ecky-mcp"
        );
        assert!(payload["result"]["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn modern_mcp_rejects_mirrored_header_mismatch() {
        let server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "legacy-session").await;
        let response = handle_http_post(
            axum::extract::State(server),
            "/mcp".parse::<axum::http::Uri>().expect("uri"),
            modern_request_headers("tools/list", None),
            modern_request_body(1, "resources/list", json!({})),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("mismatch body");
        let payload: Value = serde_json::from_slice(&bytes).expect("mismatch json");
        assert_eq!(payload["error"]["code"], -32020);
    }

    #[tokio::test]
    async fn modern_mcp_returns_explicit_context_handle_and_cacheable_full_catalogue() {
        let server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "legacy-session").await;
        let response = handle_http_post(
            axum::extract::State(server.clone()),
            "/mcp".parse::<axum::http::Uri>().expect("uri"),
            modern_request_headers("tools/list", None),
            modern_request_body(
                1,
                "tools/list",
                json!({ "profile": "compact-managed", "pageSize": 200 }),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(SESSION_HEADER).is_none());
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("tools body");
        let payload: Value = serde_json::from_slice(&bytes).expect("tools json");
        assert_eq!(payload["result"]["resultType"], "complete");
        assert_eq!(payload["result"]["cacheScope"], "private");
        assert!(payload["result"]["ttlMs"].as_u64().unwrap_or_default() > 0);
        let tools = payload["result"]["tools"].as_array().expect("tools array");
        assert!(
            tools.len() > 15,
            "modern catalogue cannot depend on session state"
        );
        assert!(tools
            .iter()
            .all(|tool| { tool["inputSchema"]["properties"]["eckyContextId"].is_object() }));

        let call_response = handle_http_post(
            axum::extract::State(server.clone()),
            "/mcp".parse::<axum::http::Uri>().expect("uri"),
            modern_request_headers("tools/call", Some("health_check")),
            modern_request_body(
                2,
                "tools/call",
                json!({ "name": "health_check", "arguments": {} }),
            ),
        )
        .await;
        assert_eq!(call_response.status(), StatusCode::OK);
        assert!(call_response.headers().get(SESSION_HEADER).is_none());
        let bytes = axum::body::to_bytes(call_response.into_body(), usize::MAX)
            .await
            .expect("tool body");
        let payload: Value = serde_json::from_slice(&bytes).expect("tool json");
        let context_id = payload["result"]["structuredContent"]["eckyContextId"]
            .as_str()
            .unwrap_or_default();
        assert!(context_id.starts_with("ecky-context-"));

        let reused_response = handle_http_post(
            axum::extract::State(server),
            "/mcp".parse::<axum::http::Uri>().expect("uri"),
            modern_request_headers("tools/call", Some("health_check")),
            modern_request_body(
                3,
                "tools/call",
                json!({
                    "name": "health_check",
                    "arguments": { "eckyContextId": context_id }
                }),
            ),
        )
        .await;
        let bytes = axum::body::to_bytes(reused_response.into_body(), usize::MAX)
            .await
            .expect("reused tool body");
        let reused_payload: Value = serde_json::from_slice(&bytes).expect("reused tool json");
        assert_eq!(
            reused_payload["result"]["structuredContent"]["eckyContextId"],
            context_id
        );
    }

    // ── §5.2 capability search returns matching groups and tool names ──────
    #[tokio::test]
    async fn capability_search_returns_matching_groups_and_tools() {
        let server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "session-cap-search").await;

        // (a) No query → lists every group with its tool names, without
        //     loading specialist schemas into the compact tools/list.
        let all = dispatch_tool_call_jsonrpc(
            &server,
            "session-cap-search",
            "capability_search",
            json!({}),
        )
        .await;
        let payload = parse_mcp_tool_payload(&all);
        let groups = payload["groups"].as_array().expect("groups array");
        assert!(
            groups.len() >= 8,
            "capability_search without a query lists every capability group"
        );
        let group_ids: Vec<&str> = groups
            .iter()
            .filter_map(|group| group["id"].as_str())
            .collect();
        assert!(group_ids.contains(&"core"));
        assert!(group_ids.contains(&"ast-edits"));

        // (b) Scoped query → only groups/tools matching the term.
        let scoped = dispatch_tool_call_jsonrpc(
            &server,
            "session-cap-search",
            "capability_search",
            json!({ "query": "printability" }),
        )
        .await;
        let scoped_payload = parse_mcp_tool_payload(&scoped);
        let scoped_groups = scoped_payload["groups"].as_array().expect("scoped groups");
        let scoped_ids: Vec<&str> = scoped_groups
            .iter()
            .filter_map(|group| group["id"].as_str())
            .collect();
        assert!(
            scoped_ids.contains(&"verify-printability"),
            "scoped query returns the matching group: {scoped_ids:?}"
        );
        assert!(
            !scoped_ids.contains(&"components-library"),
            "scoped query excludes unrelated groups: {scoped_ids:?}"
        );
        // The specialist schemas are still absent from the compact tools/list.
        let compact = dispatch_request(
            &server,
            "session-cap-search",
            jsonrpc(3, "tools/list", json!({ "profile": "compact-managed" })),
        )
        .await;
        let compact_names: Vec<&str> = compact.result.as_ref().expect("result")["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name"))
            .collect();
        assert!(
            !compact_names.contains(&"printability_analyze"),
            "capability_search must not load specialist schemas into the compact list"
        );
    }

    // ── §5.2/§5.3 capability enable: session-scoped group + list_changed ──
    #[tokio::test]
    async fn capability_enable_loads_session_group_emits_list_changed_and_updates_tools_list() {
        let server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "session-cap-enable").await;

        // Before enable, the compact list is core-only and lacks the AST group.
        let before = dispatch_request(
            &server,
            "session-cap-enable",
            jsonrpc(1, "tools/list", json!({ "profile": "compact-managed" })),
        )
        .await;
        let before_names: Vec<&str> = before.result.as_ref().expect("result")["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name"))
            .collect();
        assert!(!before_names.contains(&"ecky_ast_get_node"));

        // Enable the AST group for this session only.
        let enable = dispatch_tool_call_jsonrpc(
            &server,
            "session-cap-enable",
            "capability_enable",
            json!({ "group": "ast-edits" }),
        )
        .await;
        let enable_payload = parse_mcp_tool_payload(&enable);
        assert_eq!(enable_payload["group"]["id"], "ast-edits");
        assert_eq!(
            enable_payload["enabledGroups"],
            json!(["ast-edits"]),
            "enabledGroups reflects the session-scoped enable"
        );
        assert_eq!(
            enable_payload["listChanged"], true,
            "capability_enable signals the list changed"
        );

        // The server emitted a standard notifications/tools/list_changed for
        // this session (queued for delivery; drained via the probe hook).
        let notifications =
            drain_pending_mcp_notifications(&server.state, "session-cap-enable").await;
        assert_eq!(
            notifications.len(),
            1,
            "exactly one list_changed notification queued"
        );
        assert_eq!(
            notifications[0]["method"], "notifications/tools/list_changed",
            "queued notification is the standard tools/list_changed method"
        );

        // The next compact tools/list now includes the enabled group's schemas.
        let after = dispatch_request(
            &server,
            "session-cap-enable",
            jsonrpc(2, "tools/list", json!({ "profile": "compact-managed" })),
        )
        .await;
        let after_names: Vec<&str> = after.result.as_ref().expect("result")["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name"))
            .collect();
        assert!(
            after_names.contains(&"ecky_ast_get_node"),
            "enabled group schemas appear in the next compact tools/list"
        );

        // Session scoping: a sibling session on a fresh AppState (no enabled
        // groups) still has a core-only compact list and does not see this
        // session's enabled AST group.
        let sibling_server =
            test_dispatch_server("(model (part body (box 1 2 3)))", "session-cap-sibling").await;
        let sibling = dispatch_request(
            &sibling_server,
            "session-cap-sibling",
            jsonrpc(1, "tools/list", json!({ "profile": "compact-managed" })),
        )
        .await;
        let sibling_names: Vec<&str> = sibling.result.as_ref().expect("result")["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name"))
            .collect();
        assert!(
            !sibling_names.contains(&"ecky_ast_get_node"),
            "enabled groups are session-scoped and do not leak across sessions"
        );
    }

    // ── §5.3 managed sessions default to compact discovery ────────────────
    #[tokio::test]
    async fn managed_session_defaults_to_compact_managed_discovery() {
        let (state, resolver) = seed_dispatch_ecky_target("(model (part body (box 1 2 3)))").await;
        // Override the seed: mark this session as a managed-mcp-http session so
        // tools/list defaults to compact-managed without an explicit profile.
        {
            let mut sessions = state.mcp_session_registry.with_sessions().lock().await;
            sessions.insert(
                "session-managed-default".to_string(),
                McpSessionState {
                    client_kind: "managed-mcp-http".to_string(),
                    host_label: "ManagedAgent".to_string(),
                    agent_label: "managed".to_string(),
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
        }
        let server = HttpServerState {
            state,
            app: resolver,
            handle: None,
        };

        let compact = dispatch_request(
            &server,
            "session-managed-default",
            jsonrpc(1, "tools/list", json!({})),
        )
        .await;
        let tools = compact.result.as_ref().expect("result")["tools"]
            .as_array()
            .expect("tools");
        assert!(
            tools.len() <= 15,
            "managed session defaults to compact discovery (<=15 tools), got {}",
            tools.len()
        );
        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("name"))
            .collect();
        assert!(
            names.iter().any(|n| n.contains("capability")),
            "managed compact default advertises a capability control"
        );
    }

    // ── 1.4 screenshot single image payload, byte-free metadata ────────────
    fn recursive_has_key(value: &Value, key: &str) -> bool {
        match value {
            Value::Object(map) => {
                map.contains_key(key) || map.values().any(|v| recursive_has_key(v, key))
            }
            Value::Array(items) => items.iter().any(|v| recursive_has_key(v, key)),
            _ => false,
        }
    }

    #[test]
    fn screenshot_payload_carries_image_bytes_once_with_byte_free_metadata() {
        let requested_target = ResolvedTargetRef {
            thread_id: "thread-1".to_string(),
            message_id: "message-1".to_string(),
            model_id: Some("model-1".to_string()),
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            model_stl_path: Some("/tmp/model.stl".to_string()),
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
            data_url: "data:image/png;base64,QUJDREVG=".to_string(),
            width: 640,
            height: 480,
            camera: crate::contracts::ViewportCameraState {
                position: [0.0, 0.0, 5.0],
                target: [0.0, 0.0, 0.0],
                zoom: None,
                fov: Some(50.0),
            },
            source: "visible-live".to_string(),
            thread_id: "thread-1".to_string(),
            message_id: "message-1".to_string(),
            model_id: Some("model-1".to_string()),
            include_overlays: false,
        };

        let result = build_model_screenshot_result(&requested_target, &capture)
            .expect("screenshot payload should be valid");

        // (1) GREEN evidence — image bytes appear exactly once, in a single MCP
        //     image content item.
        let image_items = result["content"]
            .as_array()
            .expect("content array")
            .iter()
            .filter(|item| item["type"] == "image")
            .collect::<Vec<_>>();
        assert_eq!(
            image_items.len(),
            1,
            "screenshot bytes live in one image content item"
        );
        assert_eq!(image_items[0]["data"], "QUJDREVG=");
        assert_eq!(image_items[0]["mimeType"], "image/png");

        // (2) RED — structured metadata must be byte-free: no `dataUrl` and no
        //     `base64` field anywhere under structuredContent. Today both are
        //     present (structuredContent.image.dataUrl and .base64), duplicating
        //     the image bytes a second and third time.
        let structured = result
            .get("structuredContent")
            .expect("structuredContent present");
        assert!(
            !recursive_has_key(structured, "dataUrl"),
            "structuredContent must not carry a dataUrl (duplicate bytes)"
        );
        assert!(
            !recursive_has_key(structured, "base64"),
            "structuredContent must not carry a base64 field (duplicate bytes)"
        );

        // (3) GREEN evidence — byte-free identity/dimensions/camera/source/
        //     capture metadata is still carried alongside the single image.
        assert_eq!(structured["threadId"], "thread-1");
        assert_eq!(structured["modelId"], "model-1");
        assert_eq!(structured["width"], 640);
        assert_eq!(structured["height"], 480);
        assert_eq!(structured["source"], "visible-live");
        assert!(structured.get("camera").is_some());
        assert!(structured.get("capturedAt").is_some());
        assert!(
            recursive_has_key(structured, "mimeType"),
            "MIME type carried as byte-free metadata"
        );
    }

    // ── 1.5 large tool result: structuredContent / concise text / truncation / continuation ──
    #[test]
    fn large_tool_result_uses_structured_content_concise_text_truncation_and_continuation() {
        // A large structured tool result carrying NO pre-built `content` array,
        // so it flows through the generic `mcp_tool_success` envelope.
        let rows = (0..2000)
            .map(|i| json!({ "index": i, "label": format!("row-{i}") }))
            .collect::<Vec<_>>();
        let large: Value = json!({
            "threadId": "thread-1",
            "messageId": "msg-1",
            "rows": rows,
            "totalCount": 2000
        });

        let response = mcp_tool_success(Some(json!(1)), &large);
        let result = response.result.expect("json-rpc result");

        // (1) RED — canonical machine output must live in `structuredContent`.
        //     Today the generic envelope pretty-prints the whole value into text
        //     and emits no structuredContent.
        assert!(
            result.get("structuredContent").is_some(),
            "large tool result must expose canonical JSON in structuredContent, \
             not pretty-print the whole payload into text"
        );

        // (2) RED — text content must be a concise summary and must NOT repeat
        //     the full JSON payload.
        let text = result["content"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["text"].as_str())
            .unwrap_or("");
        assert!(
            text.chars().count() <= 2_000,
            "text content must be a concise summary, not a full pretty-printed JSON copy \
             ({} chars today)",
            text.chars().count()
        );
        assert!(
            !text.contains("row-1999"),
            "text summary must not repeat the full payload"
        );

        // (3) RED — large reads must report observed/returned/total truncation
        //     counts.
        assert!(
            result.get("observedCount").is_some()
                || result["structuredContent"].get("observedCount").is_some(),
            "large read must report observed/returned/total truncation counts"
        );

        // (4) RED — large reads must report continuation metadata.
        assert!(
            result.get("nextCursor").is_some()
                || result["structuredContent"].get("continuation").is_some()
                || result["structuredContent"].get("nextCursor").is_some(),
            "large read must report continuation metadata (cursor / next read)"
        );
    }

    // ── 6.1 compatibility: ordinary success → structuredContent + concise text ──
    #[test]
    fn ordinary_tool_success_exposes_canonical_structured_content_and_concise_text() {
        // An ordinary structured tool result (no pre-built `content` array).
        let value = json!({
            "threadId": "thread-1",
            "messageId": "msg-1",
            "modelId": "model-1",
            "applied": true,
            "tokens": ["a", "b", "c"],
        });
        let response = mcp_tool_success(Some(json!(1)), &value);
        let result = response.result.expect("json-rpc result");

        // Canonical machine output lives in structuredContent verbatim.
        assert_eq!(result["structuredContent"], value);

        // content carries a concise text summary, not a pretty-printed JSON copy.
        let text = result["content"][0]["text"].as_str().expect("summary text");
        assert!(text.chars().count() <= 2_000, "summary must be concise");
        assert!(
            text != serde_json::to_string_pretty(&value).unwrap(),
            "text must not duplicate the canonical JSON"
        );
        assert!(text.contains("thread-1"), "summary carries identity");
        assert!(
            text.contains("tokens[3]"),
            "summary carries key/array shape"
        );
    }

    // ── 6.2 compatibility: large read keeps complete data + shared metadata ──
    #[test]
    fn bounded_large_read_keeps_complete_data_and_reports_size_and_continuation() {
        // A payload above the response budget: full data must remain available.
        let rows = (0..2000)
            .map(|i| json!({ "index": i, "label": format!("row-{i}") }))
            .collect::<Vec<_>>();
        let large = json!({ "threadId": "thread-1", "rows": rows, "totalCount": 2000 });

        let response = mcp_tool_success(Some(json!(1)), &large);
        let result = response.result.expect("json-rpc result");
        let structured = &result["structuredContent"];

        // Complete canonical data remains available under `data` (no loss).
        assert_eq!(structured["data"], large);
        // Shared observed/returned/total counts are reported, byte-free.
        let observed = structured["observedCount"].as_u64().expect("observedCount");
        assert!(observed > 0);
        assert_eq!(structured["returnedCount"], structured["observedCount"]);
        assert_eq!(structured["totalCount"], structured["observedCount"]);
        assert_eq!(structured["truncated"], false);
        // Continuation metadata is present and carries no payload bytes.
        assert!(structured.get("continuation").is_some());
        let continuation_str = serde_json::to_string(&structured["continuation"]).unwrap();
        assert!(!continuation_str.contains("row-1999"));
    }

    #[test]
    fn full_thread_payload_above_one_megabyte_remains_transportable() {
        let thread = json!({
            "threadId": "thread-1",
            "attachment": "x".repeat(3_100_000),
        });

        let response = mcp_tool_success(Some(json!(1)), &thread);
        let result = response.result.expect("json-rpc result");

        assert_ne!(result["isError"], true);
        assert_eq!(result["structuredContent"]["data"], thread);
    }

    // ── 6.5 compatibility: tool-origin errors → MCP isError + raw details ──
    #[test]
    fn mcp_tool_error_uses_is_error_with_raw_details_not_generic_advice() {
        // A tool-origin error carrying raw, actionable provider details.
        let error = AppError::with_details(
            AppErrorCode::Provider,
            "Provider rejected the request.",
            "HTTP 429: rate_limit_exceeded for model=gpt-x (req_abc). Retry after 30s.",
        );

        let response = mcp_tool_error(Some(json!(1)), &error);
        let result = response.result.expect("json-rpc result");

        // MCP `isError` is set.
        assert_eq!(result["isError"], true);

        // The raw, actionable details are preserved verbatim in the text payload.
        let text = result["content"][0]["text"].as_str().expect("error text");
        let parsed: Value = serde_json::from_str(text).expect("error payload is JSON");
        assert_eq!(parsed["message"], "Provider rejected the request.");
        assert_eq!(parsed["code"], "provider");
        assert!(parsed["details"]
            .as_str()
            .unwrap_or_default()
            .contains("HTTP 429: rate_limit_exceeded"));

        // No generic credential/API-key advice replaces the raw error.
        let lower = text.to_ascii_lowercase();
        assert!(
            !lower.contains("check your api key")
                && !lower.contains("verify your credentials")
                && !lower.contains("invalid api key"),
            "tool-origin errors must not be replaced with generic credential advice: {text}"
        );
    }

    // ── 6.3 compatibility: explicit full reads fail honestly at the transport limit ──
    #[test]
    fn transport_limit_failure_is_honest_with_observed_and_allowed_sizes() {
        // A pathological payload exceeding the transport safety ceiling. Full
        // reads stay explicit up to this ceiling; past it the envelope fails
        // honestly instead of silently truncating authoritative state.
        let huge = "x".repeat(MCP_TRANSPORT_LIMIT_CHARS + 1);
        let value = json!({ "threadId": "thread-1", "blob": huge });

        let response = mcp_tool_success(Some(json!(1)), &value);
        let result = response.result.expect("json-rpc result");

        // Fails as an MCP error (isError), not a silent truncation.
        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().expect("error text");
        let parsed: Value = serde_json::from_str(text).expect("error JSON");
        // Reports observed and allowed sizes honestly.
        let details = parsed["details"].as_str().unwrap_or_default();
        assert!(
            details.contains("observedCount="),
            "transport-limit failure must report the observed size: {details}"
        );
        assert!(
            details.contains(&format!("allowedCount={MCP_TRANSPORT_LIMIT_CHARS}")),
            "transport-limit failure must report the allowed size: {details}"
        );
    }
}
