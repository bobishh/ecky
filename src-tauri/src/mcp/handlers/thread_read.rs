use super::{claim_owner_for_thread, AgentContext, THREAD_MESSAGE_CONTENT_MAX_CHARS};
use crate::mcp::contracts::{
    AgentIdentityOverride, AgentIdentityResponse, AgentIdentitySetRequest, ThreadGetRequest,
    ThreadGetResponse, ThreadMessageEntry, ThreadMessagesRequest, ThreadMessagesResponse,
};
use crate::models::{AppResult, AppState};
use crate::services::history;

fn compact_message_content(content: &str) -> String {
    crate::context::compact_text(content, THREAD_MESSAGE_CONTENT_MAX_CHARS)
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
