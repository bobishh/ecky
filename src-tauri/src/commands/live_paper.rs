use tauri::{AppHandle, Emitter, State};

use crate::contracts::{AppError, AppResult};
use crate::models::AppState;
use crate::services::live_paper::{hub, LivePaperPrimitive, LivePaperProfile, LivePaperSnapshot};

pub fn forward_events(app: AppHandle) {
    let mut events = hub().subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(snapshot) => {
                    let _ = app.emit("live-paper-state", snapshot);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if let Some(snapshot) = hub().current() {
                        let _ = app.emit("live-paper-state", snapshot);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[tauri::command]
#[specta::specta]
pub async fn start_live_paper(
    scope_key: String,
    view: crate::contracts::SketchView,
    state: State<'_, AppState>,
) -> AppResult<LivePaperSnapshot> {
    if scope_key.trim().is_empty()
        || scope_key.len() > 4096
        || view == crate::contracts::SketchView::Custom
    {
        return Err(AppError::validation(
            "Select a model before starting live paper.",
        ));
    }
    let base = state
        .capture_server_url
        .lock()
        .map_err(|_| AppError::internal("Camera server state unavailable."))?
        .clone()
        .ok_or_else(|| {
            AppError::validation(
                "LAN camera service is starting. Retry when Ecky's capture server is ready.",
            )
        })?;
    if let Some(previous) = hub().current() {
        let _ = hub().stop(&previous.session_id);
        crate::live_paper_rtc::close_session(&previous.session_id).await;
    }
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || hub().start_scoped(&base, scope_key, view))
            .await
            .map_err(|error| AppError::internal(format!("Paper capture worker failed: {error}")))?;
    let trust = state
        .capture_trust_url
        .lock()
        .map_err(|_| AppError::internal("Camera trust service state unavailable."))?
        .clone();
    hub()
        .set_trust_url(&snapshot.session_id, trust)
        .map_err(AppError::validation)
}

#[tauri::command]
#[specta::specta]
pub fn get_live_paper() -> Option<LivePaperSnapshot> {
    hub().current()
}

#[tauri::command]
#[specta::specta]
pub async fn stop_live_paper(session_id: String) -> AppResult<LivePaperSnapshot> {
    let snapshot = hub().stop(&session_id).map_err(AppError::validation)?;
    crate::live_paper_rtc::close_session(&session_id).await;
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub async fn edit_live_paper(
    session_id: String,
    primitive: LivePaperPrimitive,
) -> AppResult<LivePaperSnapshot> {
    tauri::async_runtime::spawn_blocking(move || hub().edit(&session_id, primitive))
        .await
        .map_err(|error| AppError::internal(format!("Paper edit worker failed: {error}")))?
        .map_err(AppError::validation)
}

#[tauri::command]
#[specta::specta]
pub async fn confirm_live_paper(
    session_id: String,
    primitive_id: String,
) -> AppResult<LivePaperSnapshot> {
    tauri::async_runtime::spawn_blocking(move || hub().confirm(&session_id, &primitive_id))
        .await
        .map_err(|error| AppError::internal(format!("Paper confirmation worker failed: {error}")))?
        .map_err(AppError::validation)
}

#[tauri::command]
#[specta::specta]
pub async fn reject_live_paper(
    session_id: String,
    primitive_id: String,
) -> AppResult<LivePaperSnapshot> {
    tauri::async_runtime::spawn_blocking(move || hub().reject(&session_id, &primitive_id))
        .await
        .map_err(|error| AppError::internal(format!("Paper removal worker failed: {error}")))?
        .map_err(AppError::validation)
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_live_paper_profile(
    session_id: String,
    scope_key: String,
) -> AppResult<LivePaperProfile> {
    let snapshot = hub()
        .current()
        .filter(|s| s.session_id == session_id && s.scope_key == scope_key)
        .ok_or_else(|| {
            AppError::validation(
                "Paper capture was replaced. Select its current accepted geometry.",
            )
        })?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::services::live_paper::prepare_profile(&snapshot)
    })
    .await
    .map_err(|error| AppError::internal(format!("Paper profile worker failed: {error}")))?
    .map_err(AppError::validation)
}
