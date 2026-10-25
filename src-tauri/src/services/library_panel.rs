use crate::component_library_index::{ComponentIndexPhase, ComponentIndexStatus};
use std::path::Path;
use std::sync::Arc;
use tauri::Emitter;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::component_package_runtime;
use crate::contracts::{
    AppError, AppResult, ComponentPackageHeader, Config, FreecadLibraryItem,
    FreecadLibrarySearchRequest,
};
use crate::models::PathResolver;

const FREECAD_PAGE_SIZE: u32 = 100;

#[derive(Debug, Clone, Deserialize, Serialize, Type, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LibraryPanelIntent {
    LoadComponents,
    #[specta(rename_all = "camelCase")]
    SearchRegistry {
        query: String,
    },
    #[specta(rename_all = "camelCase")]
    InstallRegistryPackage {
        package_id: String,
        version: String,
    },
    #[specta(rename_all = "camelCase")]
    SetRegistry {
        registry_url: String,
        query: String,
    },
    RebuildComponentIndex,
    #[specta(rename_all = "camelCase")]
    InstallPackage {
        archive_path: String,
    },
    #[specta(rename_all = "camelCase")]
    LoadFreecad {
        query: String,
        page: u32,
    },
    #[specta(rename_all = "camelCase")]
    SetFreecadRoot {
        root: String,
        query: String,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LibraryPanelProjection {
    #[specta(rename_all = "camelCase")]
    RegistryPackages {
        registry_url: String,
        releases: Vec<crate::cairn::RegistryReleaseSummary>,
    },
    #[specta(rename_all = "camelCase")]
    ComponentPackages {
        package_headers: Vec<ComponentPackageHeader>,
        components: Vec<component_package_runtime::ExtractedComponentSearchResult>,
        indexing_diagnostics: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        index_status: Option<ComponentIndexStatus>,
    },
    #[specta(rename_all = "camelCase")]
    FreecadLibrary {
        freecad_library_roots: Vec<String>,
        items: Vec<FreecadLibraryItem>,
        page: u32,
        has_more: bool,
    },
}

pub fn load_component_packages(app: &dyn PathResolver) -> AppResult<LibraryPanelProjection> {
    let index = component_package_runtime::open_component_index(app)?;
    Ok(LibraryPanelProjection::ComponentPackages {
        package_headers: crate::catalog::package_headers(index.connection())?,
        components: component_package_runtime::search_extracted_components(app, "", usize::MAX)?,
        indexing_diagnostics: index.diagnostics()?,
        index_status: Some(index.status()?),
    })
}

/// History recovery is explicit background work, never part of a search request.
pub async fn backfill_latest_component_sources(
    app: Arc<dyn PathResolver>,
    state: &crate::models::AppState,
    force: bool,
) -> AppResult<()> {
    let targets = {
        let conn = state.db_read.as_ref().unwrap_or(&state.db).lock().await;
        crate::db::latest_successful_component_capture_targets(&conn)
            .map_err(|error| AppError::persistence(error.to_string()))?
    };
    let mut progress = ComponentIndexStatus {
        phase: ComponentIndexPhase::Indexing,
        completed: 0,
        total: u32::try_from(targets.len()).unwrap_or(u32::MAX),
        error: None,
    };
    let index = component_package_runtime::open_component_index(app.as_ref())?;
    index.set_status(&progress)?;
    notify_component_library_changed(state);
    for target in targets {
        let cached = !force
            && index
                .capture_diagnostics(&target.thread_id, &target.message_id)?
                .is_some();
        if !cached {
            let source = {
                let conn = state.db_read.as_ref().unwrap_or(&state.db).lock().await;
                crate::db::get_component_capture_input(&conn, &target.thread_id, &target.message_id)
                    .map_err(|error| AppError::persistence(error.to_string()))
            };
            let capture = match source {
                Ok(Some(input)) => {
                    let paths = app.clone();
                    let thread_id = target.thread_id.clone();
                    let message_id = target.message_id.clone();
                    // Parsing and immutable source writes must not hold the history lock.
                    tokio::task::spawn_blocking(move || {
                        component_package_runtime::prepare_component_capture_with_lock(
                            paths.as_ref(),
                            &thread_id,
                            &message_id,
                            &input.source,
                            input.lock.as_ref(),
                            input.source_language,
                            input.geometry_backend,
                        )
                    })
                    .await
                    .map_err(|error| AppError::internal(error.to_string()))?
                }
                Ok(None) => Err(AppError::persistence(
                    "Successful model source is unavailable.",
                )),
                Err(error) => Err(error),
            };
            match capture {
                Ok(capture) => {
                    let conn = state.db.lock().await;
                    if let Err(error) = component_package_runtime::publish_latest_component_capture(
                        app.as_ref(),
                        &conn,
                        &target.thread_id,
                        &target.message_id,
                        capture,
                    ) {
                        // Extraction diagnostics are already committed; storage failures are fatal.
                        if index
                            .capture_diagnostics(&target.thread_id, &target.message_id)?
                            .is_none()
                        {
                            return Err(error);
                        }
                        state.push_log(format!("Component indexing: {error}"));
                    }
                }
                Err(error) => {
                    let conn = state.db.lock().await;
                    if crate::db::is_latest_successful_component_capture(
                        &conn,
                        &target.thread_id,
                        &target.message_id,
                    )
                    .map_err(|error| AppError::persistence(error.to_string()))?
                    {
                        index.record_failure(
                            &target.thread_id,
                            &target.message_id,
                            &error.to_string(),
                        )?;
                    }
                }
            }
        }
        let title = {
            let conn = state.db_read.as_ref().unwrap_or(&state.db).lock().await;
            crate::db::get_thread_title(&conn, &target.thread_id)
                .map_err(|error| AppError::persistence(error.to_string()))?
        };
        if let Some(title) = title {
            index.set_thread_title(&target.thread_id, &title)?;
        }
        progress.completed += 1;
        if progress.completed.is_multiple_of(16) || progress.completed == progress.total {
            index.set_status(&progress)?;
            notify_component_library_changed(state);
        }
    }
    {
        let conn = state.db.lock().await;
        let current = crate::db::latest_successful_component_capture_targets(&conn)
            .map_err(|error| AppError::persistence(error.to_string()))?;
        component_package_runtime::open_component_index(app.as_ref())?.retain_threads(
            &current
                .into_iter()
                .map(|target| target.thread_id)
                .collect::<Vec<_>>(),
        )?;
    }
    progress.phase = if index.diagnostics()?.is_empty() {
        ComponentIndexPhase::Ready
    } else {
        ComponentIndexPhase::Failed
    };
    index.set_status(&progress)?;
    notify_component_library_changed(state);
    Ok(())
}

fn notify_component_library_changed(state: &crate::models::AppState) {
    if let Some(handle) = state
        .app_handle
        .lock()
        .ok()
        .and_then(|handle| handle.clone())
    {
        let _ = handle.emit("component-library-updated", ());
    }
}

/// Render publication only enqueues work; extraction never occupies its history writer lock.
pub fn schedule_component_recovery(
    app: &dyn PathResolver,
    state: &crate::models::AppState,
) -> AppResult<()> {
    struct Paths {
        config: std::path::PathBuf,
        data: std::path::PathBuf,
    }
    impl PathResolver for Paths {
        fn app_config_dir(&self) -> std::path::PathBuf {
            self.config.clone()
        }
        fn app_data_dir(&self) -> std::path::PathBuf {
            self.data.clone()
        }
        fn resource_path(&self, _: &str) -> Option<std::path::PathBuf> {
            None
        }
    }
    start_component_library_indexing(
        Arc::new(Paths {
            config: app.try_app_config_dir()?,
            data: app.app_data_dir(),
        }),
        state.clone(),
        false,
    )
}

pub fn start_component_library_indexing(
    app: Arc<dyn PathResolver>,
    state: crate::models::AppState,
    force: bool,
) -> AppResult<()> {
    if !state.component_index_worker.request(force) {
        return Ok(());
    }
    tauri::async_runtime::spawn(async move {
        loop {
            let force = state.component_index_worker.take_request();
            if let Err(error) = backfill_latest_component_sources(app.clone(), &state, force).await
            {
                state.push_log(format!("Component index recovery failed: {error}"));
                if let Ok(index) = component_package_runtime::open_component_index(app.as_ref()) {
                    let mut status = index.status().unwrap_or_default();
                    status.phase = ComponentIndexPhase::Failed;
                    status.error = Some(error.to_string());
                    let _ = index.set_status(&status);
                }
            }
            let again = state.component_index_worker.finish_iteration();
            notify_component_library_changed(&state);
            if !again {
                break;
            }
        }
    });
    Ok(())
}

pub async fn load_component_packages_for_state(
    app: &dyn PathResolver,
    state: &crate::models::AppState,
) -> AppResult<LibraryPanelProjection> {
    let mut projection = load_component_packages(app)?;
    if state.component_index_worker.is_running() {
        if let LibraryPanelProjection::ComponentPackages {
            index_status: Some(status),
            ..
        } = &mut projection
        {
            status.phase = ComponentIndexPhase::Indexing;
            status.error = None;
        }
    }
    Ok(projection)
}

pub async fn search_registry(
    app: &dyn PathResolver,
    config: &Config,
    query: &str,
) -> AppResult<LibraryPanelProjection> {
    let registry = crate::cairn::configured_registry(config);
    Ok(LibraryPanelProjection::RegistryPackages {
        registry_url: registry.into(),
        releases: crate::cairn::search_registry(app, registry, query).await?,
    })
}

pub fn install_component_package(
    app: &dyn PathResolver,
    archive_path: &str,
) -> AppResult<LibraryPanelProjection> {
    let archive_path = archive_path.trim();
    if archive_path.is_empty() {
        return Err(AppError::validation(
            "Component package archive path is required.",
        ));
    }
    component_package_runtime::install_component_package_to_store(app, Path::new(archive_path))?;
    load_component_packages(app)
}

pub fn config_with_freecad_root(config: &Config, root: &str) -> AppResult<Config> {
    let root = root.trim();
    if root.is_empty() {
        return Err(AppError::validation("FreeCAD library root is required."));
    }
    let mut updated = config.clone();
    updated.freecad_library_roots = vec![root.to_string()];
    Ok(updated)
}

pub fn load_freecad_page(
    config: Config,
    query: String,
    page: u32,
) -> AppResult<LibraryPanelProjection> {
    let mut items = crate::freecad_library::search_freecad_library(
        &FreecadLibrarySearchRequest {
            query,
            roots: Vec::new(),
            limit: Some(FREECAD_PAGE_SIZE + 1),
            offset: page.saturating_mul(FREECAD_PAGE_SIZE),
            include_architecture: false,
        },
        &config.freecad_library_roots,
    )?;
    let has_more = items.len() > FREECAD_PAGE_SIZE as usize;
    items.truncate(FREECAD_PAGE_SIZE as usize);
    Ok(LibraryPanelProjection::FreecadLibrary {
        freecad_library_roots: config.freecad_library_roots,
        items,
        page,
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_boundary_is_tagged_and_camel_case() {
        let value = serde_json::to_value(LibraryPanelIntent::SetFreecadRoot {
            root: "/library".to_string(),
            query: "bolt".to_string(),
        })
        .expect("serialize");

        assert_eq!(value["kind"], "setFreecadRoot");
        assert_eq!(value["root"], "/library");
        assert!(value.get("archive_path").is_none());
    }

    #[test]
    fn root_update_preserves_config_and_replaces_roots() {
        let config: Config = serde_json::from_value(serde_json::json!({
            "engines": [],
            "selectedEngineId": "engine-1",
            "freecadCmd": "",
            "cadTextFontPath": "",
            "assets": [],
            "voice": {},
            "mcp": {},
            "femCompute": {},
            "providerModels": {},
            "defaultEngineKind": "eckyIrV0",
            "defaultSourceLanguage": "eckyIrV0",
            "defaultGeometryBackend": "eckyRust",
            "maxGenerationAttempts": 3,
            "maxVerifyAttempts": 2
        }))
        .expect("config");
        let updated = config_with_freecad_root(&config, " /library ").expect("root");

        assert_eq!(updated.freecad_library_roots, vec!["/library"]);
        assert_eq!(updated.selected_engine_id, config.selected_engine_id);
    }

    #[derive(Clone)]
    struct TestPaths(std::path::PathBuf);

    impl PathResolver for TestPaths {
        fn app_config_dir(&self) -> std::path::PathBuf {
            self.0.join("config")
        }
        fn app_data_dir(&self) -> std::path::PathBuf {
            self.0.clone()
        }
        fn resource_path(&self, _: &str) -> Option<std::path::PathBuf> {
            None
        }
    }

    fn test_config() -> Config {
        Config {
            cairn: None,
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::contracts::VoiceConfig::default(),
            mcp: crate::contracts::McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: crate::contracts::EngineKind::EckyIrV0,
            default_source_language: crate::contracts::SourceLanguage::EckyIrV0,
            default_geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            max_generation_attempts: 3,
            max_verify_attempts: 2,
            projects_root: None,
        }
    }

    #[tokio::test]
    async fn given_indexed_components_when_history_is_busy_then_library_reads_without_waiting() {
        let resolver = TestPaths(
            std::env::temp_dir().join(format!("ecky-library-fast-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&resolver.0).unwrap();
        let conn = crate::db::init_db(&resolver.0.join("history.sqlite")).unwrap();
        let state = crate::models::AppState::new(test_config(), None, conn);
        component_package_runtime::capture_defined_components(
            &resolver,
            "indexed",
            "v1",
            "(model (part fast-clip (box 2 2 2)))",
        )
        .unwrap();
        let history_writer = state.db.lock().await;
        schedule_component_recovery(&resolver, &state).unwrap();
        let projection = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            load_component_packages_for_state(&resolver, &state),
        )
        .await
        .expect("search must not acquire the history writer lock")
        .unwrap();
        let LibraryPanelProjection::ComponentPackages {
            components,
            index_status,
            ..
        } = projection
        else {
            panic!("components")
        };
        assert!(components.iter().any(|entry| entry.name == "fast-clip"));
        assert_eq!(index_status.unwrap().phase, ComponentIndexPhase::Indexing);
        drop(history_writer);
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state.component_index_worker.wait_idle(),
        )
        .await
        .expect("queued recovery must finish after the history writer is released");
    }

    #[tokio::test]
    async fn background_recovery_indexes_latest_success_without_appending_versions_or_redecoding_history(
    ) {
        let resolver = TestPaths(
            std::env::temp_dir().join(format!("ecky-library-backfill-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&resolver.0).unwrap();
        let conn = crate::db::init_db(&resolver.0.join("history.sqlite")).unwrap();
        crate::capture_runs::ensure_schema(&conn).unwrap();
        let state = crate::models::AppState::new(test_config(), None, conn);
        {
            let conn = state.db.lock().await;
            crate::db::create_or_update_thread(&conn, "thread-backfill", "Backfill", 1, None)
                .unwrap();
            let design: crate::contracts::DesignOutput = serde_json::from_value(serde_json::json!({
                "title": "Backfill", "macroCode": "(define-component clip ((number width 4)) (box width 2 1))",
                "macroDialect": "eckyIrV0", "engineKind": "eckyIrV0", "sourceLanguage": "eckyIrV0", "geometryBackend": "eckyRust"
            })).unwrap();
            let bundle: crate::contracts::ArtifactBundle = serde_json::from_value(serde_json::json!({
                "modelId": "backfill-model", "sourceKind": "generated", "engineKind": "eckyIrV0",
                "sourceLanguage": "eckyIrV0", "geometryBackend": "eckyRust", "contentHash": "sha256:artifact",
                "fcstdPath": "", "manifestPath": "manifest", "modelStlPath": "model.stl"
            })).unwrap();
            for (id, status, source, bundle) in [
                (
                    "success",
                    crate::contracts::MessageStatus::Success,
                    design.macro_code.clone(),
                    Some(bundle.clone()),
                ),
                (
                    "failed",
                    crate::contracts::MessageStatus::Error,
                    "(define-component ghost () (box 1 1 1))".to_string(),
                    None,
                ),
            ] {
                let mut output = design.clone();
                output.macro_code = source;
                crate::db::add_message(
                    &conn,
                    "thread-backfill",
                    &crate::contracts::Message {
                        id: id.to_string(),
                        role: crate::contracts::MessageRole::Assistant,
                        content: String::new(),
                        status,
                        output: Some(output),
                        usage: None,
                        artifact_bundle: bundle,
                        model_manifest: None,
                        structural_verification: None,
                        agent_origin: None,
                        image_data: None,
                        visual_kind: None,
                        attachment_images: Vec::new(),
                        timestamp: if id == "success" { 1 } else { 2 },
                    },
                )
                .unwrap();
            }
        }
        backfill_latest_component_sources(Arc::new(resolver.clone()), &state, false)
            .await
            .unwrap();
        let projection = load_component_packages_for_state(&resolver, &state)
            .await
            .unwrap();
        let LibraryPanelProjection::ComponentPackages {
            components,
            indexing_diagnostics,
            ..
        } = projection
        else {
            panic!("component projection")
        };
        assert!(indexing_diagnostics.is_empty());
        assert!(components
            .iter()
            .any(|component| component.name == "clip" && component.origin == "local"));
        assert!(!components.iter().any(|component| component.name == "ghost"));
        // An indexed immutable version must not be decoded on later searches.
        // Poison only this isolated test's stored payload to prove the boundary.
        state
            .db
            .lock()
            .await
            .execute(
                "UPDATE messages SET output = 'unavailable' WHERE id = 'success'",
                [],
            )
            .unwrap();
        let repeated = load_component_packages_for_state(&resolver, &state)
            .await
            .unwrap();
        let LibraryPanelProjection::ComponentPackages {
            indexing_diagnostics,
            ..
        } = repeated
        else {
            panic!("component projection")
        };
        assert!(
            indexing_diagnostics.is_empty(),
            "indexed history was decoded again"
        );
        let conn = state.db.lock().await;
        assert_eq!(
            crate::db::get_thread_latest_version(&conn, "thread-backfill")
                .unwrap()
                .unwrap()
                .id,
            "failed"
        );
        let newer_id = "newer-success";
        let newer_source = "(define-component clip ((number width 5)) (box width 2 1))";
        let mut newer_design: crate::contracts::DesignOutput = serde_json::from_value(
            serde_json::json!({
                "title": "Backfill", "macroCode": newer_source,
                "macroDialect": "eckyIrV0", "engineKind": "eckyIrV0", "sourceLanguage": "eckyIrV0", "geometryBackend": "eckyRust"
            }),
        )
        .unwrap();
        newer_design.macro_code = newer_source.to_string();
        crate::db::add_message(
            &conn,
            "thread-backfill",
            &crate::contracts::Message {
                id: newer_id.to_string(),
                role: crate::contracts::MessageRole::Assistant,
                content: String::new(),
                status: crate::contracts::MessageStatus::Success,
                output: Some(newer_design),
                usage: None,
                artifact_bundle: Some(
                    serde_json::from_value(serde_json::json!({
                        "modelId": "backfill-model-new", "sourceKind": "generated", "engineKind": "eckyIrV0",
                        "sourceLanguage": "eckyIrV0", "geometryBackend": "eckyRust", "contentHash": "sha256:artifact-new",
                        "fcstdPath": "", "manifestPath": "manifest", "modelStlPath": "model.stl"
                    }))
                    .unwrap(),
                ),
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: 3,
            },
        )
        .unwrap();
        component_package_runtime::capture_latest_successful_components(
            &resolver,
            &conn,
            "thread-backfill",
            newer_id,
            newer_source,
        )
        .unwrap();
        component_package_runtime::capture_latest_successful_components(
            &resolver,
            &conn,
            "thread-backfill",
            "success",
            "(define-component clip ((number width 4)) (box width 2 1))",
        )
        .unwrap();
        let expected_digest = crate::component_extract::extract_defined_components(
            newer_source,
            "thread-backfill",
            newer_id,
        )
        .unwrap()[0]
            .header
            .revision_digest
            .clone()
            .unwrap();
        let identity = crate::component_extract::extract_defined_components(
            newer_source,
            "thread-backfill",
            newer_id,
        )
        .unwrap()[0]
            .header
            .component_id
            .clone()
            .unwrap();
        assert_eq!(
            component_package_runtime::read_component_by_id(&resolver, &identity, None)
                .unwrap()
                .revision_digest
                .as_deref(),
            Some(expected_digest.as_str())
        );
        drop(conn);
        state
            .db
            .lock()
            .await
            .execute(
                "UPDATE threads SET deleted_at = 10 WHERE id = 'thread-backfill'",
                [],
            )
            .unwrap();
        backfill_latest_component_sources(Arc::new(resolver.clone()), &state, false)
            .await
            .unwrap();
        assert!(
            !component_package_runtime::search_extracted_components(&resolver, "clip", 100)
                .unwrap()
                .iter()
                .any(|item| item.thread_id.as_deref() == Some("thread-backfill")),
            "deleted projects must leave discovery"
        );
        assert!(
            component_package_runtime::read_component_by_id(
                &resolver,
                &identity,
                Some(&expected_digest)
            )
            .is_ok(),
            "historical pinned source must survive discovery cleanup"
        );
    }
}
