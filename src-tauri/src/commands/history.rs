use crate::models::{AppError, AppResult, AppState, Thread};
use crate::services::history as history_service;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn get_history(state: State<'_, AppState>) -> AppResult<Vec<Thread>> {
    let conn = state.db.lock().await;
    history_service::get_history(&conn)
}

#[tauri::command]
#[specta::specta]
pub async fn get_thread(state: State<'_, AppState>, id: String) -> AppResult<Thread> {
    let conn = state.db.lock().await;
    history_service::get_thread(&conn, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_history(state: State<'_, AppState>) -> AppResult<()> {
    let conn = state.db.lock().await;
    crate::db::clear_history(&conn).map_err(|err| AppError::persistence(err.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_thread(id: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state.db.lock().await;
    crate::db::delete_thread(&conn, &id).map_err(|err| AppError::persistence(err.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn rename_thread(id: String, title: String, state: State<'_, AppState>) -> AppResult<()> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("Thread title cannot be empty."));
    }

    let conn = state.db.lock().await;
    let changed = crate::db::update_thread_title(&conn, &id, trimmed)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;
    if changed {
        Ok(())
    } else {
        Err(AppError::not_found("Thread not found."))
    }
}

#[tauri::command]
#[specta::specta]
pub async fn delete_version(message_id: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state.db.lock().await;
    history_service::delete_version(&conn, &message_id)
}

#[tauri::command]
#[specta::specta]
pub async fn restore_version(message_id: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state.db.lock().await;
    history_service::restore_version(&conn, &message_id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_deleted_messages(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::DeletedMessage>> {
    let conn = state.db.lock().await;
    crate::db::get_deleted_messages(&conn)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn hide_deleted_message(message_id: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state.db.lock().await;
    let changed = crate::db::hide_deleted_message(&conn, &message_id)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;
    if changed {
        Ok(())
    } else {
        Err(AppError::not_found(
            "Deleted message not found or already hidden.",
        ))
    }
}

#[tauri::command]
#[specta::specta]
pub async fn finalize_thread(id: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state.db.lock().await;
    history_service::finalize_thread(&conn, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn reopen_thread(id: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state.db.lock().await;
    history_service::reopen_thread(&conn, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_inventory(state: State<'_, AppState>) -> AppResult<Vec<Thread>> {
    let conn = state.db.lock().await;
    history_service::get_inventory(&conn)
}
