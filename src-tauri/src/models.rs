use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub system_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub engines: Vec<Engine>,
    pub selected_engine_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DesignOutput {
    #[serde(default = "default_title")]
    pub title: String,
    pub macro_code: String,
    pub ui_spec: serde_json::Value,
    pub initial_params: serde_json::Value,
}

fn default_title() -> String {
    "Untitled Design".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub id: String,
    pub role: String, // "user" or "assistant"
    pub content: String,
    pub output: Option<DesignOutput>,
    pub image_data: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub messages: Vec<Message>,
    pub updated_at: u64,
}

pub struct AppState {
    pub config: Mutex<Config>,
    pub last_design: Mutex<Option<DesignOutput>>,
    pub db: Mutex<rusqlite::Connection>,
}
