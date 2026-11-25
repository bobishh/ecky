use serde::{Deserialize, Serialize};
use specta::Type;

use super::Attachment;
use super::{CodexDialogueMessage, CodexQueuedPrompt, CodexTakeoverRuntime, ProviderTurnTrace};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgyVoicePhase {
    Waiting,
    Starting,
    Connecting,
    Connected,
    Idle,
    Closed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgyVoiceSnapshot {
    pub ecky_thread_id: String,
    pub phase: AgyVoicePhase,
    pub detail: Option<String>,
    pub terminal_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgyProviderBinding {
    pub ecky_thread_id: String,
    pub agy_conversation_id: String,
    pub label: String,
    pub cwd: String,
    pub bootstrap_version: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub steer: bool,
    pub stop: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgyProviderSnapshot {
    pub binding: AgyProviderBinding,
    pub messages: Vec<CodexDialogueMessage>,
    pub live_messages: Vec<CodexDialogueMessage>,
    pub turn_traces: Vec<ProviderTurnTrace>,
    pub next_cursor: Option<String>,
    pub backwards_cursor: Option<String>,
    pub runtime: CodexTakeoverRuntime,
    pub queue: Vec<CodexQueuedPrompt>,
    pub capabilities: ProviderCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgyMessagePageInput {
    pub ecky_thread_id: String,
    pub cursor: Option<String>,
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgyMessagePage {
    pub messages: Vec<CodexDialogueMessage>,
    pub next_cursor: Option<String>,
    pub backwards_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgyPromptInput {
    pub ecky_thread_id: String,
    pub prompt_text: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgyStopInput {
    pub ecky_thread_id: String,
    pub turn_id: String,
}
