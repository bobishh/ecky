use tauri::State;

use crate::models::{AppState, DesignOutput};

#[tauri::command]
pub async fn get_last_design(state: State<'_, AppState>) -> Result<Option<(DesignOutput, Option<String>)>, String> {
    let last = state.last_design.lock().unwrap();
    let thread_id = state.last_thread_id.lock().unwrap();
    Ok(last.as_ref().map(|d| (d.clone(), thread_id.clone())))
}
