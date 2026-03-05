use tauri::State;

use crate::models::AppState;
use crate::db;

#[tauri::command]
pub async fn get_history(state: State<'_, AppState>) -> Result<Vec<crate::models::Thread>, String> {
    let db = state.db.lock().unwrap();
    db::get_all_threads(&db).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
pub async fn get_thread(id: String, state: State<'_, AppState>) -> Result<crate::models::Thread, String> {
    let db = state.db.lock().unwrap();
    let title = db::get_thread_title(&db, &id).map_err(|e| e.to_string())?.ok_or("Thread not found")?;
    let summary = db::get_thread_summary(&db, &id).map_err(|e| e.to_string())?.unwrap_or_default();
    let messages = db::get_thread_messages(&db, &id).map_err(|e| e.to_string())?;
    
    let mut stmt = db.prepare("SELECT genie_traits FROM threads WHERE id = ?1").map_err(|e| e.to_string())?;
    let genie_traits_str: Option<String> = stmt.query_row([&id], |row| row.get(0)).map_err(|e| e.to_string())?;
    let genie_traits: Option<serde_json::Value> = genie_traits_str.and_then(|s| serde_json::from_str(&s).ok());

    let updated_at = messages.last().map(|m| m.timestamp).unwrap_or(0);
    let version_count = messages.iter().filter(|m| m.role == "assistant" && m.output.is_some()).count();

    Ok(crate::models::Thread {
        id,
        title,
        summary,
        messages,
        updated_at,
        genie_traits,
        version_count,
    })
}

#[tauri::command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::clear_history(&db).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
pub async fn delete_thread(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::delete_thread(&db, &id).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
pub async fn delete_version(message_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::delete_message(&db, &message_id).map_err(|e: rusqlite::Error| e.to_string())
}
