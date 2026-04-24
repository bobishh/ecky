use crate::commands::design::{
    derive_framework_controls, parse_macro_params, reconcile_framework_params,
};
use crate::contracts::infer_macro_dialect_from_code;
use crate::db;
use crate::models::{
    validate_design_output, validate_design_params, validate_model_manifest,
    validate_model_runtime_bundle, validate_ui_spec, AgentOrigin, AppError, AppResult, AppState,
    ArtifactBundle, DesignOutput, DesignParams, InteractionMode, MacroDialect, Message,
    MessageRole, MessageStatus, ModelManifest, PathResolver, PostProcessingSpec, UiSpec,
};
use crate::persist_thread_summary;
use crate::services::session::{build_runtime_snapshot, write_last_snapshot};
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
    pub source_language: Option<crate::models::SourceLanguage>,
    pub geometry_backend: Option<crate::models::GeometryBackend>,
    pub parameters: DesignParams,
    pub ui_spec: UiSpec,
    pub post_processing: Option<PostProcessingSpec>,
    pub artifact_bundle: Option<ArtifactBundle>,
    pub model_manifest: Option<ModelManifest>,
    pub response_text: Option<String>,
    pub agent_origin: Option<AgentOrigin>,
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

fn resolve_manual_authoring_context(
    macro_dialect: MacroDialect,
    source_language: Option<crate::models::SourceLanguage>,
    geometry_backend: Option<crate::models::GeometryBackend>,
) -> (
    crate::models::EngineKind,
    crate::models::SourceLanguage,
    crate::models::GeometryBackend,
) {
    let resolved_source = source_language.unwrap_or(match macro_dialect {
        MacroDialect::EckyIrV0 => crate::models::SourceLanguage::EckyIrV0,
        MacroDialect::Build123d => crate::models::SourceLanguage::Build123d,
        _ => crate::models::SourceLanguage::LegacyPython,
    });
    let engine_kind = resolved_source.to_engine_kind();
    let resolved_backend = geometry_backend.unwrap_or(match resolved_source {
        crate::models::SourceLanguage::EckyIrV0 => crate::models::GeometryBackend::EckyRust,
        crate::models::SourceLanguage::Build123d => crate::models::GeometryBackend::Build123d,
        crate::models::SourceLanguage::LegacyPython => crate::models::GeometryBackend::Freecad,
    });
    (engine_kind, resolved_source, resolved_backend)
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
    } = request;

    let (ui_spec, parameters, macro_dialect) =
        resolve_macro_contracts(&macro_code, &parameters, &ui_spec)?;
    let (ui_spec, parameters) = crate::models::reconcile_post_processing_controls(
        &ui_spec,
        &parameters,
        post_processing.as_ref(),
    );

    validate_ui_spec(&ui_spec)?;
    validate_design_params(&parameters, &ui_spec)?;
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
    let db = state.db.lock().await;

    let (engine_kind, source_language, geometry_backend) =
        resolve_manual_authoring_context(macro_dialect.clone(), source_language, geometry_backend);
    let output = DesignOutput {
        title: title.clone(),
        version_name,
        response: response_text
            .clone()
            .unwrap_or_else(|| "Manual edit committed as new version.".to_string()),
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
    validate_design_output(&output)?;

    let thread_traits = if db::get_thread_title(&db, &thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .is_none()
    {
        Some(crate::generate_genie_traits())
    } else {
        None
    };
    db::create_or_update_thread(
        &db,
        &thread_id,
        &title,
        now,
        thread_traits.as_ref(),
        Some(output.engine_kind),
        Some(output.source_language),
        Some(output.geometry_backend),
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;

    let msg_id = Uuid::new_v4().to_string();
    let msg = Message {
        id: msg_id.clone(),
        role: MessageRole::Assistant,
        content: response_text
            .unwrap_or_else(|| "Manual edit committed as new version.".to_string()),
        status: MessageStatus::Success,
        output: Some(output),
        usage: None,
        artifact_bundle: artifact_bundle.clone(),
        model_manifest: model_manifest.clone(),
        agent_origin,
        image_data: None,
        visual_kind: None,
        attachment_images: Vec::new(),
        timestamp: now,
    };

    db::add_message(&db, &thread_id, &msg).map_err(|err| AppError::persistence(err.to_string()))?;
    let _ = persist_thread_summary(&db, &thread_id, &title);
    let snapshot = build_runtime_snapshot(
        msg.output.clone(),
        Some(thread_id.clone()),
        Some(msg_id.clone()),
        artifact_bundle,
        model_manifest,
        None,
    );
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
    }
    write_last_snapshot(app, Some(&snapshot));

    Ok(msg_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{ParamValue, UiField};
    use std::collections::BTreeMap;

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
        assert!(healed
            .2
            .carried_keys
            .iter()
            .any(|key| key == "top_conn_left"));
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
        assert!(ui_spec
            .fields
            .iter()
            .any(|field| field.key() == "duplo_height_blocks"));
        assert_eq!(
            params.get("duplo_height_blocks"),
            Some(&ParamValue::Number(5.0))
        );
    }

    #[test]
    fn resolve_manual_authoring_context_preserves_ecky_ir_build123d_combo() {
        let (engine_kind, source_language, geometry_backend) = resolve_manual_authoring_context(
            MacroDialect::EckyIrV0,
            Some(crate::models::SourceLanguage::EckyIrV0),
            Some(crate::models::GeometryBackend::Build123d),
        );

        assert_eq!(engine_kind, crate::models::EngineKind::EckyIrV0);
        assert_eq!(source_language, crate::models::SourceLanguage::EckyIrV0);
        assert_eq!(geometry_backend, crate::models::GeometryBackend::Build123d);
    }
}
