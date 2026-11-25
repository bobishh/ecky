use crate::contracts::{
    BootProjection, Config, EngineKind, GeometryBackend, LastDesignSnapshot, McpMode,
    MicrowaveConfig, RuntimeCapabilities, SourceLanguage,
};
use crate::services::history;

const DEFAULT_STT_LANGUAGE_CODE: &str = "en-US";
const DEFAULT_MCP_PROMPT_TIMEOUT_SECS: u64 = 1800;

pub struct BootProjectionResult {
    pub projection: BootProjection,
    pub clear_last_snapshot: bool,
}

pub fn normalize_boot_config(config: &mut Config) -> bool {
    let original = config.clone();

    if !config.engines.is_empty() {
        if !config
            .engines
            .iter()
            .any(|engine| engine.id == config.selected_engine_id)
        {
            config.selected_engine_id = config.engines[0].id.clone();
        }
        for engine in &mut config.engines {
            engine.enabled = engine.id == config.selected_engine_id;
        }
    }

    if config.microwave.is_none() {
        config.microwave = Some(MicrowaveConfig {
            hum_id: None,
            ding_id: None,
            muted: false,
        });
    }

    config.voice.stt_language_code = match config.voice.stt_language_code.trim() {
        "" => DEFAULT_STT_LANGUAGE_CODE.to_string(),
        value => value.to_string(),
    };
    config.cad_text_font_path = config.cad_text_font_path.trim().to_string();
    config.freecad_library_roots = config
        .freecad_library_roots
        .iter()
        .map(|root| root.trim())
        .filter(|root| !root.is_empty())
        .map(str::to_string)
        .collect();
    config.mcp.mode = McpMode::Passive;
    if !(10..=1800).contains(&config.mcp.prompt_timeout_secs) {
        config.mcp.prompt_timeout_secs = DEFAULT_MCP_PROMPT_TIMEOUT_SECS;
    }
    crate::mcp::runtime::ensure_primary_agent_id(config);

    *config != original
}

pub fn repair_default_authoring_context(
    config: &mut Config,
    capabilities: &RuntimeCapabilities,
) -> bool {
    let removed_context = config.default_engine_kind == EngineKind::Build123d
        || config.default_source_language == SourceLanguage::Build123d
        || config.default_geometry_backend == GeometryBackend::Build123d;
    let available = crate::runtime_capabilities::capability_for_authoring_context(
        capabilities,
        config.default_source_language,
        config.default_geometry_backend,
    )
    .available;
    if !removed_context && available {
        return false;
    }

    let recommended = &capabilities.recommended_authoring_context;
    let changed = config.default_engine_kind != recommended.engine_kind
        || config.default_source_language != recommended.source_language
        || config.default_geometry_backend != recommended.geometry_backend;
    config.default_engine_kind = recommended.engine_kind;
    config.default_source_language = recommended.source_language;
    config.default_geometry_backend = recommended.geometry_backend;
    changed
}

pub fn select_first_available_model(
    config: &mut Config,
    engine_id: &str,
    models: &[String],
) -> bool {
    let Some(first) = models.first() else {
        return false;
    };
    let Some(engine) = config
        .engines
        .iter_mut()
        .find(|engine| engine.id == engine_id)
    else {
        return false;
    };
    if !engine.model.is_empty() && models.contains(&engine.model) {
        return false;
    }
    engine.model = first.clone();
    true
}

pub fn build_boot_projection(
    conn: &rusqlite::Connection,
    config: Config,
    last_snapshot: Option<&LastDesignSnapshot>,
    message_limit: Option<usize>,
) -> crate::contracts::AppResult<BootProjectionResult> {
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| crate::contracts::AppError::persistence(error.to_string()))?;
    let threads = history::get_history(&transaction)?;

    let pointed_thread_id = last_snapshot.and_then(|snapshot| snapshot.thread_id.as_deref());
    let pointed_message_id = last_snapshot.and_then(|snapshot| snapshot.message_id.as_deref());
    let fallback_thread_id = threads
        .iter()
        .find(|thread| !thread.is_blank)
        .map(|thread| thread.id.as_str());
    let target_thread_id = pointed_thread_id.or(fallback_thread_id);
    let pointed_thread_exists = pointed_thread_id
        .map(|thread_id| threads.iter().any(|thread| thread.id == thread_id))
        .unwrap_or(true);
    let workspace = if pointed_thread_exists {
        target_thread_id
            .map(|thread_id| {
                history::get_workspace_projection_read(
                    &transaction,
                    thread_id,
                    pointed_message_id,
                    message_limit,
                )
            })
            .transpose()?
    } else {
        None
    };

    transaction
        .commit()
        .map_err(|error| crate::contracts::AppError::persistence(error.to_string()))?;

    let projection = BootProjection {
        config,
        history: threads,
        workspace,
        selected_part_id: last_snapshot.and_then(|snapshot| snapshot.selected_part_id.clone()),
    };
    crate::transport_budget::require_serialized_budget(
        "bootProjection",
        &projection,
        crate::transport_budget::ORDINARY_JSON_MAX_BYTES,
        "thread summary pagination and get_workspace_projection",
    )?;
    Ok(BootProjectionResult {
        projection,
        clear_last_snapshot: pointed_thread_id.is_some() && !pointed_thread_exists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        DesignOutput, InteractionMode, MacroDialect, Message, MessageRole, MessageStatus,
        RuntimeAuthoringContext, RuntimeBackendCapability, UiSpec,
    };
    use std::collections::BTreeMap;

    fn config() -> Config {
        serde_json::from_value(serde_json::json!({
            "engines": [
                {"id":"one","name":"One","provider":"ollama","apiKey":"","model":"m","baseUrl":"","enabled":true},
                {"id":"two","name":"Two","provider":"ollama","apiKey":"","model":"m","baseUrl":"","enabled":true}
            ],
            "selectedEngineId": "missing",
            "freecadCmd": "",
            "cadTextFontPath": "  /tmp/font.ttf  ",
            "freecadLibraryRoots": [" /tmp/library ", ""],
            "assets": [],
            "microwave": null,
            "voice": {"sttLanguageCode":"  "},
            "mcp": {"mode":"active","promptTimeoutSecs":3,"autoAgents":[]},
            "defaultEngineKind":"build123d",
            "defaultSourceLanguage":"build123d",
            "defaultGeometryBackend":"build123d",
            "maxGenerationAttempts":3,
            "maxVerifyAttempts":2
        }))
        .expect("config")
    }

    fn capabilities() -> RuntimeCapabilities {
        let unavailable = RuntimeBackendCapability {
            available: false,
            detail: "missing".to_string(),
            path: None,
        };
        RuntimeCapabilities {
            freecad: unavailable.clone(),
            build123d: unavailable.clone(),
            direct_occt: unavailable,
            ecky_rust: RuntimeBackendCapability {
                available: true,
                detail: "bundled".to_string(),
                path: None,
            },
            recommended_authoring_context: RuntimeAuthoringContext {
                engine_kind: EngineKind::EckyIrV0,
                source_language: SourceLanguage::EckyIrV0,
                geometry_backend: GeometryBackend::EckyRust,
            },
        }
    }

    fn version(id: &str, timestamp: u64) -> Message {
        Message {
            id: id.to_string(),
            parent_message_id: None,
            role: MessageRole::Assistant,
            content: id.to_string(),
            status: MessageStatus::Success,
            output: Some(DesignOutput {
                title: "Boot".to_string(),
                version_name: id.to_string(),
                response: String::new(),
                interaction_mode: InteractionMode::Design,
                macro_code: "(model)".to_string(),
                macro_dialect: MacroDialect::EckyIrV0,
                engine_kind: EngineKind::EckyIrV0,
                source_language: SourceLanguage::EckyIrV0,
                geometry_backend: GeometryBackend::EckyRust,
                ui_spec: UiSpec::default(),
                initial_params: BTreeMap::new(),
                post_processing: None,
            }),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
            timestamp,
        }
    }

    #[test]
    fn boot_config_normalization_and_runtime_repair_are_backend_owned() {
        let mut config = config();

        assert!(normalize_boot_config(&mut config));
        assert_eq!(config.selected_engine_id, "one");
        assert!(config.engines[0].enabled);
        assert!(!config.engines[1].enabled);
        assert_eq!(config.voice.stt_language_code, "en-US");
        assert_eq!(config.freecad_library_roots, vec!["/tmp/library"]);
        assert_eq!(config.mcp.mode, McpMode::Passive);
        assert_eq!(config.mcp.prompt_timeout_secs, 1800);

        assert!(repair_default_authoring_context(
            &mut config,
            &capabilities()
        ));
        assert_eq!(config.default_engine_kind, EngineKind::EckyIrV0);
        assert_eq!(config.default_source_language, SourceLanguage::EckyIrV0);
        assert_eq!(config.default_geometry_backend, GeometryBackend::EckyRust);
    }

    #[test]
    fn boot_projection_reads_history_and_pointed_workspace_in_one_transaction() {
        let path = std::env::temp_dir().join(format!(
            "ecky-boot-projection-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = crate::db::init_db(&path).expect("schema");
        crate::db::create_or_update_thread(&conn, "thread-1", "Boot", 100, None).expect("thread");
        crate::db::add_message(&conn, "thread-1", &version("version-1", 100)).expect("version");
        let snapshot = LastDesignSnapshot {
            design: None,
            thread_id: Some("thread-1".to_string()),
            message_id: Some("version-1".to_string()),
            artifact_bundle: None,
            model_manifest: None,
            selected_part_id: Some("part-1".to_string()),
            target_ref: None,
        };

        let result = build_boot_projection(&conn, config(), Some(&snapshot), Some(20))
            .expect("boot projection");

        assert_eq!(result.projection.history.len(), 1);
        assert_eq!(
            result
                .projection
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.selected_version.as_ref())
                .map(|message| message.id.as_str()),
            Some("version-1")
        );
        assert_eq!(
            result.projection.selected_part_id.as_deref(),
            Some("part-1")
        );
        assert!(!result.clear_last_snapshot);

        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn model_catalog_selects_first_available_model_only_when_current_is_invalid() {
        let mut config = config();
        let models = vec!["model-a".to_string(), "model-b".to_string()];

        assert!(select_first_available_model(&mut config, "one", &models));
        assert_eq!(config.engines[0].model, "model-a");
        assert!(!select_first_available_model(&mut config, "one", &models));
    }
}
