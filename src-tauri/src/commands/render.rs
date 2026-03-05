use tauri::{State, AppHandle, Manager};

use crate::models::AppState;
use crate::freecad;

#[tauri::command]
pub async fn render_stl(macro_code: String, parameters: serde_json::Value, state: State<'_, AppState>, app: AppHandle) -> Result<String, String> {
    let _guard = state.render_lock.lock().await;
    let result = freecad::render(&macro_code, &parameters, &app);
    if result.is_ok() {
        let app_data_dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        freecad::evict_cache_if_needed(&app_data_dir);
    }
    result
}

#[tauri::command]
pub async fn get_default_macro(app: AppHandle) -> Result<String, String> {
    freecad::get_default_macro(&app)
}

#[tauri::command]
pub async fn get_mess_stl_path(app: AppHandle) -> Result<String, String> {
    let mut path = std::path::PathBuf::from("../templates/mess.stl");
    if !path.exists() {
        let resource_path = app.path().resource_dir().unwrap_or_default();
        path = resource_path.join("templates/mess.stl");
    }
    if !path.exists() {
        path = std::path::PathBuf::from("templates/mess.stl");
    }
    
    if !path.exists() {
        return Err("mess.stl not found in templates directory".to_string());
    }
    
    Ok(path.to_str().ok_or("Invalid path")?.to_string())
}

#[tauri::command]
pub async fn export_file(source_path: String, target_path: String) -> Result<(), String> {
    std::fs::copy(&source_path, &target_path).map_err(|e| format!("Failed to export file: {}", e))?;
    Ok(())
}
