use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::models::{AppResult, AppState, LastDesignSnapshot};

fn last_snapshot_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap()
        .join("last_design.json")
}

pub(crate) fn write_last_snapshot(app: &AppHandle, snapshot: Option<&LastDesignSnapshot>) {
    let path = last_snapshot_path(app);
    match snapshot {
        Some(snapshot) => {
            if let Ok(serialized) = serde_json::to_string_pretty(snapshot) {
                let _ = fs::write(path, serialized);
            }
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

pub(crate) fn build_runtime_snapshot(
    design: Option<crate::models::DesignOutput>,
    thread_id: Option<String>,
    message_id: Option<String>,
    artifact_bundle: Option<crate::models::ArtifactBundle>,
    model_manifest: Option<crate::models::ModelManifest>,
    selected_part_id: Option<String>,
) -> LastDesignSnapshot {
    LastDesignSnapshot {
        design,
        thread_id,
        message_id,
        artifact_bundle,
        model_manifest,
        selected_part_id,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_last_design(state: State<'_, AppState>) -> AppResult<Option<LastDesignSnapshot>> {
    Ok(state.last_snapshot.lock().unwrap().clone())
}

#[tauri::command]
#[specta::specta]
pub async fn save_last_design(
    snapshot: Option<LastDesignSnapshot>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = snapshot.clone();
    }
    write_last_snapshot(&app, snapshot.as_ref());
    Ok(())
}
