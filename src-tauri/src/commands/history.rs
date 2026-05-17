use crate::models::{AppError, AppResult, AppState, Message, Thread, ThreadMessagesPage};
use crate::services::history as history_service;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn get_history(state: State<'_, AppState>) -> AppResult<Vec<Thread>> {
    if let Some(read_conn) = state.db_read.as_ref() {
        let conn = read_conn.lock().await;
        history_service::get_history(&conn)
    } else {
        let conn = state.db.lock().await;
        history_service::get_history(&conn)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_thread(state: State<'_, AppState>, id: String) -> AppResult<Thread> {
    if let Some(read_conn) = state.db_read.as_ref() {
        let conn = read_conn.lock().await;
        history_service::get_thread(&conn, &id)
    } else {
        let conn = state.db.lock().await;
        history_service::get_thread(&conn, &id)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_thread_latest_version(
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<Option<Message>> {
    if let Some(read_conn) = state.db_read.as_ref() {
        let conn = read_conn.lock().await;
        history_service::get_thread_latest_version(&conn, &thread_id)
    } else {
        let conn = state.db.lock().await;
        history_service::get_thread_latest_version(&conn, &thread_id)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_thread_message_version(
    state: State<'_, AppState>,
    thread_id: String,
    message_id: String,
) -> AppResult<Option<Message>> {
    if let Some(read_conn) = state.db_read.as_ref() {
        let conn = read_conn.lock().await;
        history_service::get_thread_message_version(&conn, &thread_id, &message_id)
    } else {
        let conn = state.db.lock().await;
        history_service::get_thread_message_version(&conn, &thread_id, &message_id)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_thread_messages_page(
    state: State<'_, AppState>,
    thread_id: String,
    before: Option<u64>,
    limit: Option<usize>,
    include_visual_payloads: bool,
) -> AppResult<ThreadMessagesPage> {
    if let Some(read_conn) = state.db_read.as_ref() {
        let conn = read_conn.lock().await;
        history_service::get_thread_messages_page(
            &conn,
            &thread_id,
            before,
            limit,
            include_visual_payloads,
        )
    } else {
        let conn = state.db.lock().await;
        history_service::get_thread_messages_page(
            &conn,
            &thread_id,
            before,
            limit,
            include_visual_payloads,
        )
    }
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
    let changed = crate::db::delete_thread(&conn, &id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    if changed {
        Ok(())
    } else {
        Err(AppError::not_found("Thread not found."))
    }
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
    if let Some(read_conn) = state.db_read.as_ref() {
        let conn = read_conn.lock().await;
        crate::db::get_deleted_messages(&conn)
            .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))
    } else {
        let conn = state.db.lock().await;
        crate::db::get_deleted_messages(&conn)
            .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))
    }
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
pub async fn finalize_thread(
    id: String,
    message_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = state.db.lock().await;
    history_service::finalize_thread(&conn, &id, message_id.as_deref())
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
    if let Some(read_conn) = state.db_read.as_ref() {
        let conn = read_conn.lock().await;
        history_service::get_inventory(&conn)
    } else {
        let conn = state.db.lock().await;
        history_service::get_inventory(&conn)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_thread_window_layout(
    thread_id: String,
    state: State<'_, AppState>,
) -> AppResult<Option<crate::models::ThreadWindowLayout>> {
    if let Some(read_conn) = state.db_read.as_ref() {
        let conn = read_conn.lock().await;
        crate::db::get_thread_window_layout(&conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))
    } else {
        let conn = state.db.lock().await;
        crate::db::get_thread_window_layout(&conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))
    }
}

#[tauri::command]
#[specta::specta]
pub async fn save_thread_window_layout(
    thread_id: String,
    layout: crate::models::ThreadWindowLayout,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = state.db.lock().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let saved = crate::db::save_thread_window_layout(&conn, &thread_id, &layout, now)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    if saved {
        Ok(())
    } else {
        Err(AppError::not_found("Thread not found."))
    }
}
