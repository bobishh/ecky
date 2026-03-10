use base64::{engine::general_purpose, Engine as _};
use std::fs;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

#[tauri::command]
#[specta::specta]
pub async fn upload_asset(
    source_path: String,
    name: String,
    format: String,
    app: AppHandle,
) -> crate::models::AppResult<crate::models::Asset> {
    let app_data_dir = app.path().app_data_dir().unwrap();
    let assets_dir = app_data_dir.join("assets");
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir)
            .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
    }

    let id = Uuid::new_v4().to_string();
    let file_name = format!("{}.{}", id, format.to_lowercase());
    let target_path = assets_dir.join(&file_name);

    fs::copy(&source_path, &target_path)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;

    Ok(crate::models::Asset {
        id,
        name,
        path: target_path.to_string_lossy().to_string(),
        format,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn save_recorded_audio(
    base64_data: String,
    name: String,
    app: AppHandle,
) -> crate::models::AppResult<crate::models::Asset> {
    let app_data_dir = app.path().app_data_dir().unwrap();
    let assets_dir = app_data_dir.join("assets");
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir)
            .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
    }

    let id = Uuid::new_v4().to_string();
    let file_name = format!("{}.webm", id);
    let target_path = assets_dir.join(&file_name);

    let bytes = general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|err| crate::models::AppError::validation(err.to_string()))?;
    fs::write(&target_path, bytes)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;

    Ok(crate::models::Asset {
        id,
        name,
        path: target_path.to_string_lossy().to_string(),
        format: "WEBM".to_string(),
    })
}
