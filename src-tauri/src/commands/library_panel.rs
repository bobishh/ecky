use tauri::{AppHandle, State};

use crate::contracts::{AppResult, Config};
use crate::models::AppState;
use crate::services::library_panel::{self, LibraryPanelIntent, LibraryPanelProjection};

#[tauri::command]
#[specta::specta]
pub async fn library_panel_intent(
    intent: LibraryPanelIntent,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<LibraryPanelProjection> {
    match intent {
        LibraryPanelIntent::LoadComponents => {
            library_panel::load_component_packages_for_state(&app, state.inner()).await
        }
        LibraryPanelIntent::InstallPackage { archive_path } => {
            library_panel::install_component_package(&app, &archive_path)
        }
        LibraryPanelIntent::LoadFreecad { query, page } => {
            let config = state
                .config
                .lock()
                .map_err(|_| crate::contracts::AppError::persistence("config state lock failed"))?
                .clone();
            library_panel::load_freecad_page(config, query, page)
        }
        LibraryPanelIntent::SetFreecadRoot { root, query } => {
            let current = state
                .config
                .lock()
                .map_err(|_| crate::contracts::AppError::persistence("config state lock failed"))?
                .clone();
            let updated = library_panel::config_with_freecad_root(&current, &root)?;
            let projection = library_panel::load_freecad_page(updated.clone(), query, 0)?;
            persist_library_config(updated, state.inner(), &app)?;
            Ok(projection)
        }
    }
}

fn persist_library_config(config: Config, state: &AppState, app: &AppHandle) -> AppResult<()> {
    let config_dir = crate::models::PathResolver::try_app_config_dir(app)?;
    let saved = crate::commands::config::persist_config_transaction(&config_dir, config, state)?;
    let state_for_sync = state.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::mcp::runtime::sync_auto_agent_supervisors(state_for_sync);
    });
    for warning in saved.warnings {
        state.push_log(warning);
    }
    Ok(())
}
