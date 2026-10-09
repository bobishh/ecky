use crate::commands::design::{
    derive_framework_controls, parse_macro_params, reconcile_framework_params,
};
use crate::contracts::infer_macro_dialect_from_code;
use crate::contracts::{
    AgentOrigin, AppError, AppResult, ArtifactBundle, DesignOutput, DesignParams, InteractionMode,
    MacroDialect, Message, MessageRole, MessageStatus, ModelManifest, PostProcessingSpec, UiSpec,
    validate_design_output, validate_design_params, validate_model_manifest,
    validate_model_runtime_bundle, validate_ui_spec,
};
use crate::db;
use crate::models::{AppState, PathResolver};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyParamHealReport {
    pub added_keys: Vec<String>,
    pub dropped_keys: Vec<String>,
    pub carried_keys: Vec<String>,
}

pub fn is_param_schema_mismatch(error: &AppError) -> bool {
    error.code == crate::contracts::AppErrorCode::Validation
        && (error.message.starts_with("initialParams is missing '")
            || error
                .message
                .starts_with("initialParams contains undeclared key '"))
}

pub fn auto_heal_legacy_params(
    macro_code: &str,
    current_ui_spec: &UiSpec,
    current_params: &DesignParams,
    carry_over: Option<&DesignParams>,
) -> AppResult<Option<(UiSpec, DesignParams, LegacyParamHealReport)>> {
    let parsed = parse_macro_params(macro_code.to_string());
    if parsed.fields.is_empty() && parsed.params.is_empty() {
        return Ok(None);
    }

    let next_ui_spec = if parsed.fields.is_empty() {
        current_ui_spec.clone()
    } else {
        UiSpec {
            fields: parsed.fields.clone(),
        }
    };

    let mut next_params = parsed.params.clone();
    let mut carried_keys = Vec::new();
    for source in [Some(current_params), carry_over].into_iter().flatten() {
        for (key, value) in source {
            if next_params.contains_key(key) {
                next_params.insert(key.clone(), value.clone());
                if !carried_keys.iter().any(|existing| existing == key) {
                    carried_keys.push(key.clone());
                }
            }
        }
    }

    validate_ui_spec(&next_ui_spec)?;
    validate_design_params(&next_params, &next_ui_spec)?;

    let current_keys = current_params
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let next_keys = next_params
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let added_keys = next_keys.difference(&current_keys).cloned().collect();
    let dropped_keys = current_keys.difference(&next_keys).cloned().collect();

    Ok(Some((
        next_ui_spec,
        next_params,
        LegacyParamHealReport {
            added_keys,
            dropped_keys,
            carried_keys,
        },
    )))
}

pub struct AddManualVersionRequest {
    pub thread_id: String,
    pub title: String,
    pub version_name: String,
    pub macro_code: String,
    pub source_language: Option<crate::contracts::SourceLanguage>,
    pub geometry_backend: Option<crate::contracts::GeometryBackend>,
    pub parameters: DesignParams,
    pub ui_spec: UiSpec,
    pub post_processing: Option<PostProcessingSpec>,
    pub artifact_bundle: Option<ArtifactBundle>,
    pub model_manifest: Option<ModelManifest>,
    pub response_text: Option<String>,
    pub agent_origin: Option<AgentOrigin>,
    pub status: Option<MessageStatus>,
    pub error_message: Option<String>,
}

fn resolve_macro_contracts(
    macro_code: &str,
    parameters: &DesignParams,
    ui_spec: &UiSpec,
) -> AppResult<(UiSpec, DesignParams, MacroDialect)> {
    let inferred_macro_dialect = infer_macro_dialect_from_code(macro_code);
    let framework_parsed = if inferred_macro_dialect == MacroDialect::EckyIrV0 {
        None
    } else {
        derive_framework_controls(macro_code)?
    };

    if let Some(parsed) = framework_parsed {
        Ok((
            UiSpec {
                fields: parsed.fields.clone(),
            },
            reconcile_framework_params(&parsed.fields, parameters, &parsed.params),
            MacroDialect::CadFrameworkV1,
        ))
    } else if inferred_macro_dialect == MacroDialect::EckyIrV0 {
        let parsed = parse_macro_params(macro_code.to_string());
        Ok((
            UiSpec {
                fields: parsed.fields.clone(),
            },
            reconcile_framework_params(&parsed.fields, parameters, &parsed.params),
            MacroDialect::EckyIrV0,
        ))
    } else {
        Ok((ui_spec.clone(), parameters.clone(), MacroDialect::Legacy))
    }
}

pub(crate) fn resolve_manual_authoring_context(
    macro_dialect: MacroDialect,
    source_language: Option<crate::contracts::SourceLanguage>,
    geometry_backend: Option<crate::contracts::GeometryBackend>,
) -> (
    crate::contracts::EngineKind,
    crate::contracts::SourceLanguage,
    crate::contracts::GeometryBackend,
) {
    let resolved_source = source_language.unwrap_or(match macro_dialect {
        MacroDialect::EckyIrV0 => crate::contracts::SourceLanguage::EckyIrV0,
        MacroDialect::Build123d => crate::contracts::SourceLanguage::EckyIrV0,
        _ => crate::contracts::SourceLanguage::LegacyPython,
    });
    let engine_kind = resolved_source.to_engine_kind();
    let resolved_backend = geometry_backend.unwrap_or(match resolved_source {
        crate::contracts::SourceLanguage::EckyIrV0 => crate::contracts::GeometryBackend::EckyRust,
        crate::contracts::SourceLanguage::Build123d => crate::contracts::GeometryBackend::EckyRust,
        crate::contracts::SourceLanguage::LegacyPython => {
            crate::contracts::GeometryBackend::Freecad
        }
    });
    (engine_kind, resolved_source, resolved_backend)
}

fn same_manual_version_payload(left: &DesignOutput, right: &DesignOutput) -> bool {
    left.title == right.title
        && left.interaction_mode == right.interaction_mode
        && left.macro_code == right.macro_code
        && left.macro_dialect == right.macro_dialect
        && left.engine_kind == right.engine_kind
        && left.source_language == right.source_language
        && left.geometry_backend == right.geometry_backend
        && left.ui_spec == right.ui_spec
        && left.initial_params == right.initial_params
        && left.post_processing == right.post_processing
}

pub async fn add_manual_version(
    request: AddManualVersionRequest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<String> {
    let AddManualVersionRequest {
        thread_id,
        title,
        version_name,
        macro_code,
        source_language,
        geometry_backend,
        parameters,
        ui_spec,
        post_processing,
        artifact_bundle,
        model_manifest,
        response_text,
        agent_origin,
        status,
        error_message,
    } = request;

    let requested_status = status.unwrap_or(MessageStatus::Success);
    let (ui_spec, parameters, macro_dialect) = if requested_status != MessageStatus::Success {
        // Non-terminal immutable versions preserve attempted values verbatim. Reconciliation is
        // a successful-outcome operation and must not replace invalid user input before append.
        (
            ui_spec,
            parameters,
            infer_macro_dialect_from_code(&macro_code),
        )
    } else {
        resolve_macro_contracts(&macro_code, &parameters, &ui_spec)?
    };
    let (ui_spec, parameters) = crate::contracts::reconcile_post_processing_controls(
        &ui_spec,
        &parameters,
        post_processing.as_ref(),
    );

    if requested_status == MessageStatus::Success {
        validate_ui_spec(&ui_spec)?;
        validate_design_params(&parameters, &ui_spec)?;
    }
    if let Some(manifest) = model_manifest.as_ref() {
        if let Some(bundle) = artifact_bundle.as_ref() {
            validate_model_runtime_bundle(manifest, bundle)?;
        } else {
            validate_model_manifest(manifest)?;
        }
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let configured_root = state.config.lock().unwrap().projects_root.clone();
    let db = state.db.lock().await;

    let (engine_kind, source_language, geometry_backend) =
        resolve_manual_authoring_context(macro_dialect.clone(), source_language, geometry_backend);
    let output = DesignOutput {
        title: title.clone(),
        version_name,
        response: response_text
            .clone()
            .unwrap_or_else(|| "Manual edit appended as new version.".to_string()),
        interaction_mode: InteractionMode::Design,
        macro_code,
        macro_dialect,
        engine_kind,
        source_language,
        geometry_backend,
        ui_spec,
        initial_params: parameters,
        post_processing,
    };
    if requested_status == MessageStatus::Success {
        validate_design_output(&output)?;
    }
    let thread_traits = if db::get_thread_title(&db, &thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .is_none()
    {
        Some(crate::generate_genie_traits())
    } else {
        None
    };
    db::create_or_update_thread(&db, &thread_id, &title, now, thread_traits.as_ref())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let status = requested_status;
    let content = error_message
        .or(response_text)
        .unwrap_or_else(|| match status {
            MessageStatus::Error => "Manual edit failed.".to_string(),
            MessageStatus::Working | MessageStatus::Pending => {
                "Manual edit pending validation.".to_string()
            }
            _ => "Manual edit appended as new version.".to_string(),
        });
    let model_id = artifact_bundle
        .as_ref()
        .map(|bundle| bundle.model_id.as_str())
        .or_else(|| {
            model_manifest
                .as_ref()
                .map(|manifest| manifest.model_id.as_str())
        });
    if let Some(existing) = db::get_thread_latest_version(&db, &thread_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .filter(|message| {
            message.output.as_ref().is_some_and(|existing_output| {
                same_manual_version_payload(existing_output, &output)
            })
        })
    {
        db::update_message_status_and_output(
            &db,
            &existing.id,
            db::MessageStatusUpdate {
                status: &status,
                output: Some(&output),
                usage: existing.usage.as_ref(),
                artifact_bundle: artifact_bundle.as_ref(),
                model_manifest: model_manifest.as_ref(),
                structural_verification: existing.structural_verification.as_ref(),
                visual_kind: existing.visual_kind.as_ref(),
                content: Some(&content),
            },
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
        crate::thread_source_binding::refresh_on_version_append(
            app,
            &db,
            configured_root.as_deref(),
            &thread_id,
            &title,
            &output.macro_code,
            &existing.id,
            model_id,
            Some(&existing.id),
        )?;
        if status == MessageStatus::Success && artifact_bundle.is_some() {
            if let Err(error) =
                crate::component_package_runtime::capture_latest_successful_components(
                    app,
                    &db,
                    &thread_id,
                    &existing.id,
                    &output.macro_code,
                )
            {
                state.push_log(format!(
                    "Component capture failed for version {}: {error}",
                    existing.id
                ));
            }
        }
        drop(db);
        state
            .authoring_actor_registry
            .invalidate_authoring_actors_for_thread(&thread_id)
            .await;
        return Ok(existing.id);
    }

    let msg_id = Uuid::new_v4().to_string();
    let msg = Message {
        id: msg_id.clone(),
        role: MessageRole::Assistant,
        content,
        status,
        output: Some(output),
        usage: None,
        artifact_bundle: artifact_bundle.clone(),
        model_manifest: model_manifest.clone(),
        structural_verification: None,
        agent_origin,
        image_data: None,
        visual_kind: None,
        attachment_images: Vec::new(),
        timestamp: now,
    };

    // Some design commits intentionally persist output before runtime hydrate.
    db::add_legacy_message(&db, &thread_id, &msg)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    crate::thread_source_binding::refresh_on_version_append(
        app,
        &db,
        configured_root.as_deref(),
        &thread_id,
        &title,
        &msg.output
            .as_ref()
            .expect("manual version has output")
            .macro_code,
        &msg_id,
        model_id,
        Some(&msg_id),
    )?;
    if msg.status == MessageStatus::Success && artifact_bundle.is_some() {
        if let Err(error) = crate::component_package_runtime::capture_latest_successful_components(
            app,
            &db,
            &thread_id,
            &msg_id,
            &msg.output
                .as_ref()
                .expect("manual version has output")
                .macro_code,
        ) {
            state.push_log(format!(
                "Component capture failed for version {msg_id}: {error}"
            ));
        }
    }
    drop(db);
    state
        .authoring_actor_registry
        .invalidate_authoring_actors_for_thread(&thread_id)
        .await;

    Ok(msg_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{ParamValue, UiField};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn legacy_macro() -> &'static str {
        r#"
params = {
    "top_conn_left": 12,
    "top_conn_back": 18,
}
"#
    }

    #[test]
    fn auto_heal_legacy_params_rebuilds_ui_spec_and_params_from_legacy_macro() {
        let current_ui_spec = UiSpec {
            fields: vec![UiField::Number {
                key: "top_conn_left".to_string(),
                label: "Top Conn Left".to_string(),
                min: None,
                max: None,
                step: None,
                min_from: None,
                max_from: None,
                frozen: false,
            }],
        };
        let current_params = BTreeMap::from([
            ("top_conn_back".to_string(), ParamValue::Number(18.0)),
            ("stale".to_string(), ParamValue::Number(99.0)),
        ]);
        let carry_over = BTreeMap::from([("top_conn_left".to_string(), ParamValue::Number(24.0))]);

        let healed = auto_heal_legacy_params(
            legacy_macro(),
            &current_ui_spec,
            &current_params,
            Some(&carry_over),
        )
        .expect("heal result")
        .expect("healed");

        assert_eq!(healed.0.fields.len(), 2);
        assert_eq!(
            healed.1.get("top_conn_left"),
            Some(&ParamValue::Number(24.0))
        );
        assert_eq!(
            healed.1.get("top_conn_back"),
            Some(&ParamValue::Number(18.0))
        );
        assert!(!healed.1.contains_key("stale"));
        assert!(healed.2.added_keys.iter().any(|key| key == "top_conn_left"));
        assert!(healed.2.dropped_keys.iter().any(|key| key == "stale"));
        assert!(
            healed
                .2
                .carried_keys
                .iter()
                .any(|key| key == "top_conn_left")
        );
    }

    #[test]
    fn auto_heal_legacy_params_returns_none_when_parser_finds_nothing() {
        let healed = auto_heal_legacy_params(
            "print('hello')",
            &UiSpec { fields: Vec::new() },
            &DesignParams::new(),
            None,
        )
        .expect("result");

        assert!(healed.is_none());
    }

    #[test]
    fn param_schema_mismatch_detection_only_matches_initial_param_shape_errors() {
        assert!(is_param_schema_mismatch(&AppError::validation(
            "initialParams is missing 'top_conn_left'."
        )));
        assert!(is_param_schema_mismatch(&AppError::validation(
            "initialParams contains undeclared key 'top_conn_back'."
        )));
        assert!(!is_param_schema_mismatch(&AppError::validation(
            "uiSpec contains duplicate field key 'x'."
        )));
    }

    #[test]
    fn resolve_macro_contracts_skips_framework_python_parse_for_ecky_source() {
        let macro_code = r#"
(model
  (params
    (number duplo_height_blocks 5 :label "duplo height blocks")
    (number flat_start 48 :label "flat start")
    (number ramp_length 192 :label "ramp length")
    (number flat_end 48 :label "flat end"))
  (part body
    (build
      (shape dz (* duplo_height_blocks 19.2))
      (shape L (+ flat_start ramp_length flat_end))
      (result (box L 10 dz)))))
"#;

        let (ui_spec, params, dialect) = resolve_macro_contracts(
            macro_code,
            &DesignParams::new(),
            &UiSpec { fields: Vec::new() },
        )
        .expect("ecky macro should bypass python parser");

        assert_eq!(dialect, MacroDialect::EckyIrV0);
        assert!(
            ui_spec
                .fields
                .iter()
                .any(|field| field.key() == "duplo_height_blocks")
        );
        assert_eq!(
            params.get("duplo_height_blocks"),
            Some(&ParamValue::Number(5.0))
        );
    }

    #[test]
    fn resolve_manual_authoring_context_preserves_ecky_ir_build123d_combo() {
        let (engine_kind, source_language, geometry_backend) = resolve_manual_authoring_context(
            MacroDialect::EckyIrV0,
            Some(crate::contracts::SourceLanguage::EckyIrV0),
            Some(crate::contracts::GeometryBackend::Build123d),
        );

        assert_eq!(engine_kind, crate::contracts::EngineKind::EckyIrV0);
        assert_eq!(source_language, crate::contracts::SourceLanguage::EckyIrV0);
        assert_eq!(
            geometry_backend,
            crate::contracts::GeometryBackend::Build123d
        );
    }

    struct TestPathResolver {
        root: PathBuf,
    }

    impl PathResolver for TestPathResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn manual_request(thread_id: &str, title: &str, source: &str) -> AddManualVersionRequest {
        AddManualVersionRequest {
            thread_id: thread_id.to_string(),
            title: title.to_string(),
            version_name: "V1".to_string(),
            macro_code: source.to_string(),
            source_language: Some(crate::contracts::SourceLanguage::EckyIrV0),
            geometry_backend: Some(crate::contracts::GeometryBackend::EckyRust),
            parameters: DesignParams::new(),
            ui_spec: UiSpec { fields: Vec::new() },
            post_processing: None,
            artifact_bundle: None,
            model_manifest: None,
            response_text: None,
            agent_origin: None,
            status: None,
            error_message: None,
        }
    }

    fn test_config() -> crate::contracts::Config {
        crate::contracts::Config {
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
            default_engine_kind: crate::contracts::EngineKind::Freecad,
            default_source_language: crate::contracts::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::contracts::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
    }

    #[tokio::test]
    async fn manual_version_appends_while_preserving_pending_external_edit() {
        let root =
            std::env::temp_dir().join(format!("ecky-manual-binding-{}", uuid::Uuid::new_v4()));
        let db_path = root.join("history.sqlite");
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::init_db(&db_path).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let resolver = TestPathResolver { root };
        let baseline = "(model (part body (box 3 3 3)))";

        add_manual_version(
            manual_request("thread-1", "Bracket", baseline),
            &state,
            &resolver,
        )
        .await
        .expect("first manual version");

        let binding = {
            let conn = state.db.lock().await;
            crate::thread_source_binding::get_binding(&conn, "thread-1")
                .unwrap()
                .expect("binding")
        };
        let source_path = PathBuf::from(&binding.source_path);
        let pending = "(model (part body (box 9 9 9)))";
        std::fs::write(&source_path, pending).unwrap();

        let before = {
            let conn = state.db.lock().await;
            crate::db::get_thread_messages(&conn, "thread-1")
                .unwrap()
                .len()
        };
        add_manual_version(
            manual_request("thread-1", "Bracket", "(model (part body (box 5 5 5)))"),
            &state,
            &resolver,
        )
        .await
        .expect("pending external bytes do not block append");
        assert_eq!(std::fs::read_to_string(source_path).unwrap(), pending);
        let after = {
            let conn = state.db.lock().await;
            crate::db::get_thread_messages(&conn, "thread-1")
                .unwrap()
                .len()
        };
        assert_eq!(after, before + 1, "manual edit remains a new version");
    }

    #[tokio::test]
    async fn failed_manual_edit_is_retained_as_error_head() {
        let root = std::env::temp_dir().join(format!("ecky-manual-error-{}", uuid::Uuid::new_v4()));
        let db_path = root.join("history.sqlite");
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::init_db(&db_path).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let resolver = TestPathResolver { root };
        let invalid = "(model (part body (box 3 3";
        let mut request = manual_request("thread-1", "Broken", invalid);
        request.status = Some(MessageStatus::Error);
        request.error_message = Some("line 1: unexpected end".to_string());

        let message_id = add_manual_version(request, &state, &resolver)
            .await
            .expect("failed source must append");

        let conn = state.db.lock().await;
        let head = crate::db::get_thread_latest_version(&conn, "thread-1")
            .expect("latest")
            .expect("error head");
        assert_eq!(head.id, message_id);
        assert_eq!(head.status, MessageStatus::Error);
        assert_eq!(head.content, "line 1: unexpected end");
        assert_eq!(head.output.unwrap().macro_code, invalid);
        assert!(head.artifact_bundle.is_none());
    }

    #[tokio::test]
    async fn unchanged_manual_retry_updates_same_version_instead_of_appending_duplicate() {
        let root = std::env::temp_dir().join(format!("ecky-manual-retry-{}", uuid::Uuid::new_v4()));
        let db_path = root.join("history.sqlite");
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::init_db(&db_path).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let resolver = TestPathResolver { root };
        let source = "(model (part body (box 3 3 3)))";
        let mut failed = manual_request("thread-1", "Retry", source);
        failed.status = Some(MessageStatus::Error);
        failed.error_message = Some("backend unavailable".to_string());

        let first_id = add_manual_version(failed, &state, &resolver)
            .await
            .expect("failed attempt");
        let retry_id = add_manual_version(
            manual_request("thread-1", "Retry", source),
            &state,
            &resolver,
        )
        .await
        .expect("successful retry");

        assert_eq!(retry_id, first_id);
        let conn = state.db.lock().await;
        let versions = crate::db::get_thread_messages(&conn, "thread-1")
            .expect("messages")
            .into_iter()
            .filter(|message| message.output.is_some())
            .collect::<Vec<_>>();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].status, MessageStatus::Success);
    }
}
