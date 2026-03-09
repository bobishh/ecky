use tauri::State;

use crate::db;
use crate::models::AppState;

fn rust_profile_enabled() -> bool {
    std::env::var("DRY_PROFILE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[tauri::command]
pub async fn get_history(state: State<'_, AppState>) -> Result<Vec<crate::models::Thread>, String> {
    let db = state.db.lock().await;
    let threads = db::get_all_threads(&db).map_err(|e: rusqlite::Error| e.to_string())?;
    if rust_profile_enabled() {
        eprintln!("[RPROF] history.get_history threads={}", threads.len());
    }
    Ok(threads)
}

#[tauri::command]
pub async fn get_thread(
    id: String,
    state: State<'_, AppState>,
) -> Result<crate::models::Thread, String> {
    let db = state.db.lock().await;
    let title = db::get_thread_title(&db, &id)
        .map_err(|e| e.to_string())?
        .ok_or("Thread not found")?;
    let summary = db::get_thread_summary(&db, &id)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let messages = db::get_thread_messages(&db, &id).map_err(|e| e.to_string())?;
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

    let mut stmt = db
        .prepare("SELECT genie_traits FROM threads WHERE id = ?1")
        .map_err(|e| e.to_string())?;
    let genie_traits_str: Option<String> = stmt
        .query_row([&id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let genie_traits: Option<serde_json::Value> =
        genie_traits_str.and_then(|s| serde_json::from_str(&s).ok());

    let updated_at = messages.last().map(|m| m.timestamp).unwrap_or(0);
    let version_count = messages
        .iter()
        .filter(|m| m.role == "assistant" && m.output.is_some())
        .count();
    let pending_count = messages
        .iter()
        .filter(|m| m.role == "assistant" && m.status == "pending")
        .count();
    let error_count = messages
        .iter()
        .filter(|m| m.role == "assistant" && m.status == "error")
        .count();

    Ok(crate::models::Thread {
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
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().await;
    db::clear_history(&db).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
pub async fn delete_thread(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().await;
    db::delete_thread(&db, &id).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
pub async fn delete_version(message_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().await;
    db::delete_message(&db, &message_id).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
pub async fn restore_version(message_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().await;
    db::restore_message(&db, &message_id).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
pub async fn get_deleted_messages(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let db = state.db.lock().await;
    db::get_deleted_messages(&db).map_err(|e: rusqlite::Error| e.to_string())
}
