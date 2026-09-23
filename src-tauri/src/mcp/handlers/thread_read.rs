use super::{claim_owner_for_thread, AgentContext, THREAD_MESSAGE_CONTENT_MAX_CHARS};
use crate::contracts::{AppError, AppResult};
use crate::mcp::contracts::{
    AgentIdentityOverride, AgentIdentityResponse, AgentIdentitySetRequest, ThreadGetRequest,
    ThreadGetResponse, ThreadMessageEntry, ThreadMessagesRequest, ThreadMessagesResponse,
};
use crate::models::AppState;
use crate::services::history;

fn compact_message_content(content: &str) -> String {
    crate::context::compact_text(content, THREAD_MESSAGE_CONTENT_MAX_CHARS)
}

pub async fn handle_thread_get(
    state: &AppState,
    req: ThreadGetRequest,
) -> AppResult<ThreadGetResponse> {
    let conn = state.db.lock().await;
    let thread = history::get_thread_summary(&conn, &req.thread_id)?;
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
    let roles = req.roles.as_ref().map(|raw_roles| {
        raw_roles
            .iter()
            .filter_map(|role| match role.trim().to_ascii_lowercase().as_str() {
                "user" => Some(crate::contracts::MessageRole::User),
                "assistant" => Some(crate::contracts::MessageRole::Assistant),
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let conn = state.db.lock().await;
    let page = history::get_thread_messages_page_filtered(
        &conn,
        &req.thread_id,
        req.before.clone(),
        req.limit,
        roles.as_deref(),
    )?;

    let provider_messages: Vec<ThreadMessageEntry> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, role, content, status, created_at
                 FROM agent_provider_messages
                 WHERE ecky_thread_id = ?1
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|err| AppError::persistence(err.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![&req.thread_id], |row| {
                let id: String = row.get(0)?;
                let role: String = row.get(1)?;
                let content: String = row.get(2)?;
                let status: String = row.get(3)?;
                let created_at: i64 = row.get(4)?;
                Ok((id, role, content, status, created_at))
            })
            .map_err(|err| AppError::persistence(err.to_string()))?;
        let mut entries = Vec::new();
        for row in rows {
            let (id, role, content, status, created_at) =
                row.map_err(|err| AppError::persistence(err.to_string()))?;
            let matches_role = match roles.as_deref() {
                Some(allowed) => allowed.iter().any(|r| r.as_str() == role),
                None => true,
            };
            if matches_role {
                entries.push(ThreadMessageEntry {
                    id,
                    role,
                    status,
                    timestamp: created_at.max(0) as u64,
                    content: compact_message_content(&content),
                    has_output: false,
                    has_artifacts: false,
                    has_manifest: false,
                });
            }
        }
        entries
    };
    drop(conn);

    let mut compact_messages: Vec<ThreadMessageEntry> = page
        .messages
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
            has_output: m
                .version_summary
                .as_ref()
                .is_some_and(|version| version.has_output),
            has_artifacts: m
                .version_summary
                .as_ref()
                .is_some_and(|version| version.has_runtime),
            has_manifest: m
                .version_summary
                .as_ref()
                .is_some_and(|version| version.has_manifest),
        })
        .collect();

    for pm in provider_messages {
        if !compact_messages.iter().any(|m| m.id == pm.id) {
            compact_messages.push(pm);
        }
    }
    compact_messages.sort_by_key(|m| m.timestamp);
    if let Some(limit) = req.limit {
        if compact_messages.len() > limit {
            let start = compact_messages.len() - limit;
            compact_messages = compact_messages.split_off(start);
        }
    }

    Ok(ThreadMessagesResponse {
        thread_id: req.thread_id,
        messages: compact_messages,
        next_cursor: page.next_before,
        has_more: page.has_more,
        observed_bytes: page.observed_bytes,
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
