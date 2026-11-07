use serde::{Deserialize, Serialize};
use specta::Type;

use super::Attachment;
use super::{CodexDialogueMessage, CodexQueuedPrompt, CodexTakeoverRuntime, ProviderTurnTrace};

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokProviderBinding {
    pub ecky_thread_id: String,
    pub grok_session_id: String,
    pub label: String,
    pub cwd: String,
    pub bootstrap_version: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokProviderSnapshot {
    pub binding: GrokProviderBinding,
    pub messages: Vec<CodexDialogueMessage>,
    pub live_messages: Vec<CodexDialogueMessage>,
    pub turn_traces: Vec<ProviderTurnTrace>,
    pub next_cursor: Option<String>,
    pub backwards_cursor: Option<String>,
    pub runtime: CodexTakeoverRuntime,
    pub queue: Vec<CodexQueuedPrompt>,
    pub capabilities: super::ProviderCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokMessagePageInput {
    pub ecky_thread_id: String,
    pub cursor: Option<String>,
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokMessagePage {
    pub messages: Vec<CodexDialogueMessage>,
    pub next_cursor: Option<String>,
    pub backwards_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokPromptInput {
    pub ecky_thread_id: String,
    pub prompt_text: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokStopInput {
    pub ecky_thread_id: String,
    pub turn_id: String,
}
