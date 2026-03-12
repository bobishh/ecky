use crate::commands::design::{derive_framework_controls, reconcile_framework_params};
use crate::db;
use crate::models::{
    validate_design_output, validate_design_params, validate_model_manifest, validate_ui_spec,
    AgentOrigin, AppError, AppResult, AppState, ArtifactBundle, DesignOutput, DesignParams,
    InteractionMode, MacroDialect, Message, MessageRole, MessageStatus, ModelManifest,
    PathResolver, UiSpec,
};
use crate::persist_thread_summary;
use crate::services::session::{build_runtime_snapshot, write_last_snapshot};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub async fn add_manual_version(
    thread_id: String,
    title: String,
    version_name: String,
    macro_code: String,
    parameters: DesignParams,
    ui_spec: UiSpec,
    artifact_bundle: Option<ArtifactBundle>,
    model_manifest: Option<ModelManifest>,
    response_text: Option<String>,
    agent_origin: Option<AgentOrigin>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<String> {
    let framework_parsed = derive_framework_controls(&macro_code)?;
    let (ui_spec, parameters, macro_dialect) = if let Some(parsed) = framework_parsed {
        (
            UiSpec {
                fields: parsed.fields.clone(),
            },
            reconcile_framework_params(&parsed.fields, &parameters, &parsed.params),
            MacroDialect::CadFrameworkV1,
        )
    } else {
        (ui_spec, parameters, MacroDialect::Legacy)
    };

    validate_ui_spec(&ui_spec)?;
    validate_design_params(&parameters, &ui_spec)?;
    if let Some(manifest) = model_manifest.as_ref() {
        validate_model_manifest(manifest)?;
        if let Some(bundle) = artifact_bundle.as_ref() {
            if manifest.model_id != bundle.model_id {
                return Err(AppError::validation(
                    "Model manifest does not match artifact bundle model id.",
                ));
            }
        }
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let db = state.db.lock().await;

    let output = DesignOutput {
        title: title.clone(),
        version_name,
        response: response_text
            .clone()
            .unwrap_or_else(|| "Manual edit committed as new version.".to_string()),
        interaction_mode: InteractionMode::Design,
        macro_code,
        macro_dialect,
        ui_spec,
        initial_params: parameters,
        post_processing: None,
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
    db::create_or_update_thread(&db, &thread_id, &title, now, thread_traits.as_ref())
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
