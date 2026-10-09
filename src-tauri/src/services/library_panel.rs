use std::path::Path;

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
    ComponentPackages {
        package_headers: Vec<ComponentPackageHeader>,
        components: Vec<component_package_runtime::ExtractedComponentSearchResult>,
        indexing_diagnostics: Vec<String>,
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
    Ok(LibraryPanelProjection::ComponentPackages {
        package_headers: component_package_runtime::list_installed_component_package_headers(app)?,
        components: component_package_runtime::search_extracted_components(app, "", usize::MAX)?,
        indexing_diagnostics: Vec::new(),
    })
}

#[derive(Debug, Default)]
pub struct ComponentLibraryBackfillReport {
    pub thread_titles: std::collections::HashMap<String, String>,
    pub diagnostics: Vec<String>,
}

/// Backfill successful history through the same index path used by the UI and
/// MCP search. The DB guard covers selection, latest validation, and pointer
/// publication so an older completion cannot regress a newer revision.
pub async fn backfill_latest_component_sources(
    app: &dyn PathResolver,
    state: &crate::models::AppState,
) -> AppResult<ComponentLibraryBackfillReport> {
    let mut report = ComponentLibraryBackfillReport::default();
    let conn = state.db.lock().await;
    let targets = crate::db::latest_successful_component_capture_targets(&conn)
        .map_err(|error| AppError::persistence(error.to_string()))?;
    for target in targets {
        match crate::db::get_thread_message_version(&conn, &target.thread_id, &target.message_id) {
            Ok(Some(message)) => {
                match crate::db::get_thread_title(&conn, &target.thread_id) {
                    Ok(Some(title)) => {
                        report.thread_titles.insert(target.thread_id.clone(), title);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let detail = format!(
                            "Thread {}: failed to read project title: {error}",
                            target.thread_id
                        );
                        state.push_log(format!("Component library backfill failed: {detail}"));
                        report.diagnostics.push(detail);
                    }
                }
                if let (Some(design), true) = (message.output, message.artifact_bundle.is_some()) {
                    if let Err(error) =
                        component_package_runtime::capture_latest_successful_components(
                            app,
                            &conn,
                            &target.thread_id,
                            &target.message_id,
                            &design.macro_code,
                        )
                    {
                        let detail = format!("Version {}: {error}", target.message_id);
                        state.push_log(format!("Component library backfill failed: {detail}"));
                        report.diagnostics.push(detail);
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                let detail = format!(
                    "Version {}: failed to read source: {error}",
                    target.message_id
                );
                state.push_log(format!("Component library backfill failed: {detail}"));
                report.diagnostics.push(detail);
            }
        }
    }
    Ok(report)
}

pub async fn load_component_packages_for_state(
    app: &dyn PathResolver,
    state: &crate::models::AppState,
) -> AppResult<LibraryPanelProjection> {
    let backfill = backfill_latest_component_sources(app, state).await?;
    let mut projection = load_component_packages(app)?;
    if let LibraryPanelProjection::ComponentPackages {
        components,
        indexing_diagnostics: output,
        ..
    } = &mut projection
    {
        for component in components {
            component.thread_title = component
                .thread_id
                .as_ref()
                .and_then(|thread_id| backfill.thread_titles.get(thread_id).cloned());
        }
        *output = backfill.diagnostics;
    }
    Ok(projection)
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
    async fn library_load_backfills_latest_success_without_appending_versions_or_indexing_failures()
    {
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
        assert!(
            components
                .iter()
                .any(|component| component.name == "clip" && component.origin == "local")
        );
        assert!(!components.iter().any(|component| component.name == "ghost"));
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
    }
}
