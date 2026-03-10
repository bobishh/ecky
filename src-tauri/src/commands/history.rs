use tauri::State;

use crate::db;
use crate::models::{AppError, AppResult, AppState, MessageRole, MessageStatus, Thread};
use crate::persist_thread_summary;

fn rust_profile_enabled() -> bool {
    std::env::var("DRY_PROFILE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
pub async fn get_history(state: State<'_, AppState>) -> AppResult<Vec<Thread>> {
    let db = state.db.lock().await;
    let threads = db::get_all_threads(&db)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;
    if rust_profile_enabled() {
        eprintln!("[RPROF] history.get_history threads={}", threads.len());
    }
    Ok(threads)
}

#[tauri::command]
#[specta::specta]
pub async fn get_thread(id: String, state: State<'_, AppState>) -> AppResult<Thread> {
    let db = state.db.lock().await;
    let title = db::get_thread_title(&db, &id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    let summary = db::get_thread_summary(&db, &id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .unwrap_or_default();
    let messages =
        db::get_thread_messages(&db, &id).map_err(|err| AppError::persistence(err.to_string()))?;
    if rust_profile_enabled() {
        let image_count = messages.iter().filter(|m| m.image_data.is_some()).count();
        let image_chars: usize = messages
            .iter()
            .filter_map(|m| m.image_data.as_ref().map(|s| s.len()))
            .sum();
        eprintln!(
            "[RPROF] history.get_thread id={} messages={} images={} image_chars={}",
            id,
            messages.len(),
            image_count,
            image_chars
        );
    }

    let genie_traits = db::get_thread_genie_traits(&db, &id)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let updated_at = messages.last().map(|m| m.timestamp).unwrap_or(0);
    let version_count = messages
        .iter()
        .filter(|m| {
            m.role == MessageRole::Assistant && (m.output.is_some() || m.artifact_bundle.is_some())
        })
        .count();
    let pending_count = messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant && m.status == MessageStatus::Pending)
        .count();
    let error_count = messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant && m.status == MessageStatus::Error)
        .count();

    Ok(Thread {
        id,
        title,
        summary,
        messages,
        updated_at,
        genie_traits,
        version_count,
        pending_count,
        error_count,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn clear_history(state: State<'_, AppState>) -> AppResult<()> {
    let db = state.db.lock().await;
    db::clear_history(&db).map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_thread(id: String, state: State<'_, AppState>) -> AppResult<()> {
    let db = state.db.lock().await;
    db::delete_thread(&db, &id)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_version(message_id: String, state: State<'_, AppState>) -> AppResult<()> {
    let db = state.db.lock().await;
    let thread_id = db::delete_version_cluster(&db, &message_id)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;

    if let Some(thread_id) = thread_id {
        let title = db::get_thread_title(&db, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .unwrap_or_default();
        if db::has_visible_messages(&db, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
        {
            let _ = persist_thread_summary(&db, &thread_id, &title);
        } else {
            db::update_thread_summary(&db, &thread_id, "")
                .map_err(|err| AppError::persistence(err.to_string()))?;
        }
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn restore_version(message_id: String, state: State<'_, AppState>) -> AppResult<()> {
    let db = state.db.lock().await;
    let thread_id = db::restore_version_cluster(&db, &message_id)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;

    if let Some(thread_id) = thread_id {
        let title = db::get_thread_title(&db, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .unwrap_or_default();
        if db::has_visible_messages(&db, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
        {
            let _ = persist_thread_summary(&db, &thread_id, &title);
        }
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_deleted_messages(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::DeletedMessage>> {
    let db = state.db.lock().await;
    db::get_deleted_messages(&db)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn hide_deleted_message(message_id: String, state: State<'_, AppState>) -> AppResult<()> {
    let db = state.db.lock().await;
    let changed = db::hide_deleted_message(&db, &message_id)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;
    if changed {
        Ok(())
    } else {
        Err(AppError::not_found("Deleted message not found or already hidden."))
    }
}
