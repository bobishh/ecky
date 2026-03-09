use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(alias = "api_key")]
    pub api_key: String,
    pub model: String,
    #[serde(default, alias = "light_model")]
    pub light_model: String,
    #[serde(alias = "base_url")]
    pub base_url: String,
    #[serde(alias = "system_prompt")]
    pub system_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub format: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MicrowaveConfig {
    #[serde(alias = "hum_id")]
    pub hum_id: Option<String>,
    #[serde(alias = "ding_id")]
    pub ding_id: Option<String>,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub engines: Vec<Engine>,
    #[serde(alias = "selected_engine_id")]
    pub selected_engine_id: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
    pub microwave: Option<MicrowaveConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DesignOutput {
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default = "default_version_name", alias = "version_name")]
    pub version_name: String,
    #[serde(default)]
    pub response: String,
    #[serde(default = "default_interaction_mode", alias = "interaction_mode")]
    pub interaction_mode: String,
    #[serde(alias = "macro_code")]
    pub macro_code: String,
    #[serde(alias = "ui_spec")]
    pub ui_spec: serde_json::Value,
    #[serde(alias = "initial_params")]
    pub initial_params: serde_json::Value,
}

fn default_title() -> String {
    "Untitled Design".to_string()
}

fn default_version_name() -> String {
    "V1".to_string()
}

fn default_interaction_mode() -> String {
    "design".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: String, // "user" or "assistant"
    pub content: String,
    pub status: String, // "success", "error"
    pub output: Option<DesignOutput>,
    #[serde(alias = "image_data")]
    pub image_data: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub messages: Vec<Message>,
    #[serde(alias = "updated_at")]
    pub updated_at: u64,
    #[serde(alias = "genie_traits")]
    pub genie_traits: Option<serde_json::Value>,
    #[serde(default, alias = "version_count")]
    pub version_count: usize,
    #[serde(default, alias = "pending_count")]
    pub pending_count: usize,
    #[serde(default, alias = "error_count")]
    pub error_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReference {
    pub id: String,
    #[serde(alias = "thread_id")]
    pub thread_id: String,
    #[serde(alias = "source_message_id")]
    pub source_message_id: Option<String>,
    pub ordinal: i64,
    pub kind: String,
    pub name: String,
    pub content: String,
    pub summary: String,
    pub pinned: bool,
    #[serde(alias = "created_at")]
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub path: String,
    pub name: String,
    pub explanation: String,
    pub r#type: String, // "image" or "cad"
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GenerateOutput {
    pub design: DesignOutput,
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommitOutput {
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IntentDecision {
    #[serde(alias = "intent_mode")]
    pub intent_mode: String, // "question" | "design"
    pub confidence: f32,
    pub response: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReply {
    pub thread_id: String,
    pub response: String,
}

pub struct AppState {
    pub config: Mutex<Config>,
    pub last_design: Mutex<Option<DesignOutput>>,
    pub last_thread_id: Mutex<Option<String>>,
    pub db: tokio::sync::Mutex<rusqlite::Connection>,
    pub render_lock: tokio::sync::Mutex<()>,
}
