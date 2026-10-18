use serde::{Deserialize, Serialize};
use specta::Type;

use super::Attachment;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexThreadSummary {
    pub id: String,
    pub name: Option<String>,
    pub preview: String,
    pub cwd: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub model_provider: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTakeoverBinding {
    pub ecky_thread_id: String,
    pub codex_thread_id: String,
    pub label: String,
    pub cwd: String,
    pub bootstrap_version: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Durable updates route by Ecky ownership even before a UI has loaded its binding.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderHistoryUpdate {
    pub ecky_thread_id: String,
    pub thread_id: String,
    pub method: String,
}

impl CodexProviderHistoryUpdate {
    pub fn for_binding(binding: &CodexTakeoverBinding, method: &str) -> Self {
        Self {
            ecky_thread_id: binding.ecky_thread_id.clone(),
            thread_id: binding.codex_thread_id.clone(),
            method: method.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderEventKind {
    Assistant,
    Activity,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexDialogueMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub status: String,
    pub timestamp: i64,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_kind: Option<ProviderEventKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTurnTrace {
    pub turn_id: String,
    pub status: String,
    pub messages: Vec<CodexDialogueMessage>,
    pub completed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTakeoverSnapshot {
    pub binding: CodexTakeoverBinding,
    pub messages: Vec<CodexDialogueMessage>,
    pub live_messages: Vec<CodexDialogueMessage>,
    pub turn_traces: Vec<ProviderTurnTrace>,
    pub next_cursor: Option<String>,
    pub backwards_cursor: Option<String>,
    pub runtime: CodexTakeoverRuntime,
    pub queue: Vec<CodexQueuedPrompt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTakeoverRuntime {
    pub phase: String,
    pub active_turn_id: Option<String>,
    pub error: Option<String>,
}

impl Default for CodexTakeoverRuntime {
    fn default() -> Self {
        Self {
            phase: "idle".to_string(),
            active_turn_id: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexQueuedPrompt {
    pub id: String,
    pub ecky_thread_id: String,
    pub prompt_text: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    pub status: String,
    #[serde(default)]
    pub can_cancel: bool,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexMessagePageInput {
    pub ecky_thread_id: String,
    pub cursor: Option<String>,
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexMessagePage {
    pub messages: Vec<CodexDialogueMessage>,
    pub next_cursor: Option<String>,
    pub backwards_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexPromptInput {
    pub ecky_thread_id: String,
    pub prompt_text: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexSteerInput {
    pub ecky_thread_id: String,
    pub prompt_text: String,
    pub expected_turn_id: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexStopInput {
    pub ecky_thread_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexVoiceStartInput {
    pub ecky_thread_id: String,
    pub session_id: String,
    pub sdp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexVoiceConnection {
    pub thread_id: String,
    pub session_id: String,
    pub sdp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexVoiceStopInput {
    pub ecky_thread_id: String,
    pub session_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_history_notifications_include_both_thread_identities() {
        let binding = CodexTakeoverBinding {
            ecky_thread_id: "ecky-first-voice".into(),
            codex_thread_id: "codex-cursor".into(),
            label: "Worm".into(),
            cwd: "/project".into(),
            bootstrap_version: 1,
            created_at: 1,
            updated_at: 1,
        };
        for method in ["binding/created", "history/persisted"] {
            let value =
                serde_json::to_value(CodexProviderHistoryUpdate::for_binding(&binding, method))
                    .unwrap();
            assert_eq!(
                value,
                serde_json::json!({
                    "eckyThreadId": "ecky-first-voice", "threadId": "codex-cursor", "method": method,
                })
            );
        }
    }
}
