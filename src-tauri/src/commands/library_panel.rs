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
        LibraryPanelIntent::SearchRegistry { query } => {
            let config = state
                .config
                .lock()
                .map_err(|_| crate::contracts::AppError::persistence("config state lock failed"))?
                .clone();
            library_panel::search_registry(&app, &config, &query).await
        }
        LibraryPanelIntent::InstallRegistryPackage {
            package_id,
            version,
        } => {
            let config = state
                .config
                .lock()
                .map_err(|_| crate::contracts::AppError::persistence("config state lock failed"))?
                .clone();
            crate::cairn::install_registry_package(
                std::sync::Arc::new(app.clone()),
                crate::cairn::configured_registry(&config),
                &package_id,
                &version,
            )
            .await?;
            library_panel::load_component_packages_for_state(&app, state.inner()).await
        }
        LibraryPanelIntent::SetRegistry {
            registry_url,
            query,
        } => {
            crate::contracts::validate_cairn_registry(&registry_url)?;
            let mut config = state
                .config
                .lock()
                .map_err(|_| crate::contracts::AppError::persistence("config state lock failed"))?
                .clone();
            config.cairn = Some(crate::contracts::CairnConfig {
                registry_url: registry_url.trim_end_matches('/').into(),
            });
            let projection = library_panel::search_registry(&app, &config, &query).await?;
            persist_library_config(config, state.inner(), &app)?;
            Ok(projection)
        }
        LibraryPanelIntent::LoadComponents => {
            library_panel::load_component_packages_for_state(&app, state.inner()).await
        }
        LibraryPanelIntent::RebuildComponentIndex => {
            library_panel::start_component_library_indexing(
                std::sync::Arc::new(app.clone()),
                state.inner().clone(),
                true,
            )?;
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
