use tauri::State;

use crate::contracts::AppResult;
use crate::models::AppState;
use crate::services::jev_classifications::{self, ClassificationResult};

#[tauri::command]
#[specta::specta]
pub async fn get_jev_classification_results(
    thread_id: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<ClassificationResult>> {
    let conn = state.db.lock().await;
    jev_classifications::list_for_thread(&conn, &thread_id)
}
