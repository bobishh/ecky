use tauri::{State, AppHandle, Manager};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use std::fs;

use crate::models::{AppState, DesignOutput, Message};
use crate::{db, persist_thread_summary};

#[tauri::command]
pub async fn add_manual_version(
    thread_id: String,
    title: String,
    version_name: String,
    macro_code: String,
    parameters: serde_json::Value,
    ui_spec: serde_json::Value,
    state: State<'_, AppState>
) -> Result<String, String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let db = state.db.lock().unwrap();

    let output = DesignOutput {
        title: title.clone(),
        version_name,
        response: "Manual edit committed as new version.".to_string(),
        interaction_mode: "design".to_string(),
        macro_code,
        ui_spec,
        initial_params: parameters,
    };

    let msg_id = Uuid::new_v4().to_string();
    let msg = Message {
        id: msg_id.clone(),
        role: "assistant".to_string(),
        content: "Manual edit committed as new version.".to_string(),
        status: "success".to_string(),
        output: Some(output),
        image_data: None,
        timestamp: now,
    };

    db::add_message(&db, &thread_id, &msg).map_err(|e: rusqlite::Error| e.to_string())?;
    
    let thread_traits = if db::get_thread_title(&db, &thread_id).unwrap_or(None).is_none() {
        Some(crate::generate_genie_traits())
    } else {
        None
    };
    db::create_or_update_thread(&db, &thread_id, &title, now, thread_traits.as_ref()).map_err(|e: rusqlite::Error| e.to_string())?;
    let _ = persist_thread_summary(&db, &thread_id, &title);

    Ok(msg_id)
}

#[tauri::command]
pub async fn update_ui_spec(
    message_id: String,
    ui_spec: serde_json::Value,
    state: State<'_, AppState>,
    app: AppHandle
) -> Result<(), String> {
    let (updated_output, updated_thread_id) = {
        let db = state.db.lock().unwrap();
        db::update_message_ui_spec(&db, &message_id, &ui_spec).map_err(|e| e.to_string())?;
        db::get_message_output_and_thread(&db, &message_id).map_err(|e| e.to_string())?
    }
    .ok_or("Message output not found for ui_spec update")?;

    {
        let mut last = state.last_design.lock().unwrap();
        *last = Some(updated_output.clone());
        let mut last_tid = state.last_thread_id.lock().unwrap();
        *last_tid = Some(updated_thread_id.clone());
    }

    let cache_path = app.path().app_config_dir().unwrap().join("last_design.json");
    let session_data = json!({
        "design": updated_output,
        "thread_id": Some(updated_thread_id)
    });
    if let Ok(json) = serde_json::to_string_pretty(&session_data) {
        let _ = fs::write(cache_path, json);
    }

    Ok(())
}

#[tauri::command]
pub async fn update_parameters(
    message_id: String,
    parameters: serde_json::Value,
    state: State<'_, AppState>,
    app: AppHandle
) -> Result<(), String> {
    let (updated_output, updated_thread_id) = {
        let db = state.db.lock().unwrap();
        db::update_message_parameters(&db, &message_id, &parameters).map_err(|e| e.to_string())?;
        db::get_message_output_and_thread(&db, &message_id).map_err(|e| e.to_string())?
    }
    .ok_or("Message output not found for parameter update")?;

    {
        let mut last = state.last_design.lock().unwrap();
        *last = Some(updated_output.clone());
        let mut last_tid = state.last_thread_id.lock().unwrap();
        *last_tid = Some(updated_thread_id.clone());
    }

    let cache_path = app.path().app_config_dir().unwrap().join("last_design.json");
    let session_data = json!({
        "design": updated_output,
        "thread_id": Some(updated_thread_id)
    });
    if let Ok(json) = serde_json::to_string_pretty(&session_data) {
        let _ = fs::write(cache_path, json);
    }

    Ok(())
}
