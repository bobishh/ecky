use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub light_model: String,
    pub base_url: String,
    pub system_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub format: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MicrowaveConfig {
    pub hum_id: Option<String>,
    pub ding_id: Option<String>,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub engines: Vec<Engine>,
    pub selected_engine_id: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
    pub microwave: Option<MicrowaveConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DesignOutput {
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default = "default_version_name")]
    pub version_name: String,
    #[serde(default)]
    pub response: String,
    #[serde(default = "default_interaction_mode")]
    pub interaction_mode: String,
    pub macro_code: String,
    pub ui_spec: serde_json::Value,
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
pub struct Message {
    pub id: String,
    pub role: String, // "user" or "assistant"
    pub content: String,
    pub status: String, // "success", "error"
    pub output: Option<DesignOutput>,
    pub image_data: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Thread {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub messages: Vec<Message>,
    pub updated_at: u64,
    pub genie_traits: Option<serde_json::Value>,
    #[serde(default)]
    pub version_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThreadReference {
    pub id: String,
    pub thread_id: String,
    pub source_message_id: Option<String>,
    pub ordinal: i64,
    pub kind: String,
    pub name: String,
    pub content: String,
    pub summary: String,
    pub pinned: bool,
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Attachment {
    pub path: String,
    pub name: String,
    pub explanation: String,
    pub r#type: String, // "image" or "cad"
}

#[derive(Debug, Serialize, Clone)]
pub struct GenerateOutput {
    pub design: DesignOutput,
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct CommitOutput {
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IntentDecision {
    pub intent_mode: String, // "question" | "design"
    pub confidence: f32,
    pub response: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct QuestionReply {
    pub thread_id: String,
    pub response: String,
}

pub struct AppState {
    pub config: Mutex<Config>,
    pub last_design: Mutex<Option<DesignOutput>>,
    pub last_thread_id: Mutex<Option<String>>,
    pub db: Mutex<rusqlite::Connection>,
    pub render_lock: tokio::sync::Mutex<()>,
}
