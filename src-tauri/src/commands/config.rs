use std::fs;
use tauri::{AppHandle, Manager, State};

use crate::models::AppState;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<crate::models::Config, String> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
pub async fn save_config(
    config: crate::models::Config,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let config_dir = app.path().app_config_dir().unwrap();
    let config_path = config_dir.join("config.json");

    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(config_path, data).map_err(|e| e.to_string())?;

    let mut state_config = state.config.lock().unwrap();
    *state_config = config;
    Ok(())
}

#[tauri::command]
pub async fn get_system_prompt() -> Result<String, String> {
    Ok(crate::DEFAULT_PROMPT.to_string())
}

#[tauri::command]
pub async fn list_models(
    provider: String,
    api_key: String,
    base_url: String,
) -> Result<Vec<String>, String> {
    crate::llm::list_models(&provider, &api_key, &base_url).await
}
