use std::fs;
use tauri::{AppHandle, Manager, State};

use crate::models::{AppResult, AppState, Config};

#[tauri::command]
#[specta::specta]
pub async fn get_config(state: State<'_, AppState>) -> AppResult<Config> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
#[specta::specta]
pub async fn save_config(
    config: Config,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    let config_dir = app.path().app_config_dir().unwrap();
    let config_path = config_dir.join("config.json");

    let data = serde_json::to_string_pretty(&config)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
    fs::write(config_path, data)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;

    let mut state_config = state.config.lock().unwrap();
    *state_config = config;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_system_prompt() -> AppResult<String> {
    Ok(crate::DEFAULT_PROMPT.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn list_models(
    provider: String,
    api_key: String,
    base_url: String,
) -> AppResult<Vec<String>> {
    crate::llm::list_models(&provider, &api_key, &base_url)
        .await
        .map_err(crate::models::AppError::provider)
}
