use crate::commands::design::{coerce_param_for_field, parse_macro_params};
use crate::contracts::{
    AppError, AppResult, ArtifactBundle, DesignOutput, DesignParams, GeometryBackend,
    InteractionMode, MessageStatus, ModelManifest, PostProcessingSpec, SourceLanguage, UiField,
    UiSpec, infer_macro_dialect_from_code, validate_design_output, validate_model_runtime_bundle,
};
use crate::models::{AppState, PathResolver};
use crate::services::design::{AddManualVersionRequest, add_manual_version};
use crate::services::render_snapshot::{RenderSnapshotInput, build_render_snapshot};
use crate::services::session::{build_saved_version_snapshot, write_last_snapshot};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManualCodeApplyRequest {
    pub thread_id: String,
    #[serde(default)]
    pub base_message_id: Option<String>,
    pub source: String,
    #[serde(default)]
    pub persist: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub version_name: Option<String>,
    #[serde(default)]
    pub ui_spec: UiSpec,
    #[serde(default)]
    pub parameters: DesignParams,
    #[serde(default)]
    pub post_processing: Option<PostProcessingSpec>,
    #[serde(default)]
    pub source_language: Option<SourceLanguage>,
    #[serde(default)]
    pub geometry_backend: Option<GeometryBackend>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManualCodeApplyResponse {
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub status: MessageStatus,
    pub design_output: DesignOutput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_bundle: Option<ArtifactBundle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_manifest: Option<ModelManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub parser_matched: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}

fn non_empty(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

const DRAFT_LITHOPHANE_ID_PREFIX: &str = "draft-litho-";

fn canonicalize_manual_post_processing(
    mut post_processing: Option<PostProcessingSpec>,
) -> Option<PostProcessingSpec> {
    if let Some(post_processing) = post_processing.as_mut() {
        for attachment in &mut post_processing.lithophane_attachments {
            if attachment.id.starts_with(DRAFT_LITHOPHANE_ID_PREFIX) {
                attachment.id = format!("litho-{}", uuid::Uuid::new_v4());
            }
        }
    }
    post_processing
}

fn merge_field(parsed: UiField, existing: Option<&UiField>) -> UiField {
    match (parsed, existing) {
        (
            UiField::Range {
                key,
                label,
                min,
                max,
                step,
                min_from,
                max_from,
                frozen: _,
            },
            Some(UiField::Range {
                label: old_label,
                min: old_min,
                max: old_max,
                step: old_step,
                min_from: old_min_from,
                max_from: old_max_from,
                frozen: old_frozen,
                ..
            }),
        ) => UiField::Range {
            key,
            label: if old_label.is_empty() {
                label
            } else {
                old_label.clone()
            },
            min: old_min.or(min),
            max: old_max.or(max),
            step: old_step.or(step),
            min_from: old_min_from.clone().or(min_from),
            max_from: old_max_from.clone().or(max_from),
            frozen: *old_frozen,
        },
        (
            UiField::Number {
                key,
                label,
                min,
                max,
                step,
                min_from,
                max_from,
                frozen: _,
            },
            Some(UiField::Number {
                label: old_label,
                min: old_min,
                max: old_max,
                step: old_step,
                min_from: old_min_from,
                max_from: old_max_from,
                frozen: old_frozen,
                ..
            }),
        ) => UiField::Number {
            key,
            label: if old_label.is_empty() {
                label
            } else {
                old_label.clone()
            },
            min: old_min.or(min),
            max: old_max.or(max),
            step: old_step.or(step),
            min_from: old_min_from.clone().or(min_from),
            max_from: old_max_from.clone().or(max_from),
            frozen: *old_frozen,
        },
        (
            UiField::Select {
                key,
                label,
                options,
                frozen: _,
            },
            Some(UiField::Select {
                label: old_label,
                options: old_options,
                frozen: old_frozen,
                ..
            }),
        ) => UiField::Select {
            key,
            label: if old_label.is_empty() {
                label
            } else {
                old_label.clone()
            },
            options: if old_options.is_empty() {
                options
            } else {
                old_options.clone()
            },
            frozen: *old_frozen,
        },
        (
            UiField::Checkbox {
                key,
                label,
                frozen: _,
            },
            Some(UiField::Checkbox {
                label: old_label,
                frozen: old_frozen,
                ..
            }),
        ) => UiField::Checkbox {
            key,
            label: if old_label.is_empty() {
                label
            } else {
                old_label.clone()
            },
            frozen: *old_frozen,
        },
        (
            UiField::Image {
                key,
                label,
                frozen: _,
            },
            Some(UiField::Image {
                label: old_label,
                frozen: old_frozen,
                ..
            }),
        ) => UiField::Image {
            key,
            label: if old_label.is_empty() {
                label
            } else {
                old_label.clone()
            },
            frozen: *old_frozen,
        },
        (parsed, _) => parsed,
    }
}

fn reconcile_controls(
    source: &str,
    current_ui_spec: &UiSpec,
    current_parameters: &DesignParams,
) -> (UiSpec, DesignParams, bool) {
    let parsed = parse_macro_params(source.to_string());
    if parsed.fields.is_empty() {
        return (current_ui_spec.clone(), current_parameters.clone(), false);
    }

    let fields = parsed
        .fields
        .into_iter()
        .map(|field| {
            let existing = current_ui_spec
                .fields
                .iter()
                .find(|existing| existing.key() == field.key());
            merge_field(field, existing)
        })
        .collect::<Vec<_>>();
    let mut parameters = DesignParams::new();
    for field in &fields {
        let key = field.key().to_string();
        parameters.insert(
            key.clone(),
            coerce_param_for_field(field, current_parameters.get(&key), parsed.params.get(&key)),
        );
    }
    (UiSpec { fields }, parameters, true)
}

fn requested_design(
    request: &ManualCodeApplyRequest,
    ui_spec: UiSpec,
    parameters: DesignParams,
) -> DesignOutput {
    let macro_dialect = infer_macro_dialect_from_code(&request.source);
    let (engine_kind, source_language, geometry_backend) =
        crate::services::design::resolve_manual_authoring_context(
            macro_dialect.clone(),
            request.source_language,
            request.geometry_backend,
        );
    let post_processing = canonicalize_manual_post_processing(request.post_processing.clone());
    let (ui_spec, parameters) = crate::contracts::reconcile_post_processing_controls(
        &ui_spec,
        &parameters,
        post_processing.as_ref(),
    );
    DesignOutput {
        title: non_empty(request.title.as_deref(), "Manual Edit"),
        version_name: non_empty(request.version_name.as_deref(), "V-manual"),
        response: "Manual code draft pending validation.".to_string(),
        interaction_mode: InteractionMode::Design,
        macro_code: request.source.clone(),
        macro_dialect,
        engine_kind,
        source_language,
        geometry_backend,
        ui_spec,
        initial_params: parameters,
        post_processing,
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{DRAFT_LITHOPHANE_ID_PREFIX, canonicalize_manual_post_processing};
    use crate::contracts::PostProcessingSpec;

    #[test]
    fn manual_apply_replaces_draft_lithophane_ids_and_preserves_existing_ids() {
        let post_processing: PostProcessingSpec = serde_json::from_value(serde_json::json!({
            "lithophaneAttachments": [
                {
                    "id": "draft-litho-local-key",
                    "enabled": true,
                    "source": { "kind": "file", "imagePath": "/tmp/a.png" }
                },
                {
                    "id": "litho-existing",
                    "enabled": true,
                    "source": { "kind": "file", "imagePath": "/tmp/b.png" }
                }
            ]
        }))
        .expect("post-processing fixture");

        let canonical = canonicalize_manual_post_processing(Some(post_processing))
            .expect("canonical post-processing");

        assert!(canonical.lithophane_attachments[0].id.starts_with("litho-"));
        assert!(
            !canonical.lithophane_attachments[0]
                .id
                .starts_with(DRAFT_LITHOPHANE_ID_PREFIX)
        );
        assert_eq!(canonical.lithophane_attachments[1].id, "litho-existing");
    }
}

fn version_request(
    thread_id: &str,
    design: &DesignOutput,
    status: MessageStatus,
) -> AddManualVersionRequest {
    AddManualVersionRequest {
        thread_id: thread_id.to_string(),
        title: design.title.clone(),
        version_name: design.version_name.clone(),
        macro_code: design.macro_code.clone(),
        source_language: Some(design.source_language),
        geometry_backend: Some(design.geometry_backend),
        parameters: design.initial_params.clone(),
        ui_spec: design.ui_spec.clone(),
        post_processing: design.post_processing.clone(),
        artifact_bundle: None,
        model_manifest: None,
        response_text: Some(design.response.clone()),
        agent_origin: None,
        status: Some(status),
        error_message: None,
    }
}

async fn previous_manifest(
    base_message_id: Option<&str>,
    state: &AppState,
) -> AppResult<Option<ModelManifest>> {
    let Some(message_id) = base_message_id else {
        return Ok(None);
    };
    let conn = state.db.lock().await;
    Ok(crate::db::get_message_runtime_and_thread(&conn, message_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .and_then(|(_, manifest, _)| manifest))
}

async fn write_bound_source(
    request: &ManualCodeApplyRequest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<()> {
    let configured_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    let binding = match crate::thread_source_binding::get_binding(&conn, &request.thread_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
    {
        Some(binding) => binding,
        None => {
            let title = crate::db::get_thread_title(&conn, &request.thread_id)
                .map_err(|error| AppError::persistence(error.to_string()))?
                .ok_or_else(|| {
                    AppError::not_found(format!(
                        "Design thread '{}' was not found for code preview.",
                        request.thread_id
                    ))
                })?;
            let latest = crate::db::get_thread_latest_version(&conn, &request.thread_id)
                .map_err(|error| AppError::persistence(error.to_string()))?
                .ok_or_else(|| {
                    AppError::not_found(format!(
                        "Design thread '{}' has no source version for code preview.",
                        request.thread_id
                    ))
                })?;
            let latest_source = latest
                .output
                .as_ref()
                .map(|output| output.macro_code.as_str());
            crate::thread_source_binding::backfill_binding(
                app,
                &conn,
                configured_root.as_deref(),
                &request.thread_id,
                &title,
                latest_source,
                Some(&latest.id),
                latest
                    .artifact_bundle
                    .as_ref()
                    .map(|bundle| bundle.model_id.as_str()),
            )?
        }
    };
    crate::project_mirror::write_bound_source(Path::new(&binding.source_path), &request.source)
}

async fn successful_no_op(
    request: &ManualCodeApplyRequest,
    design: &DesignOutput,
    parser_matched: bool,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<Option<ManualCodeApplyResponse>> {
    let candidate_digest = crate::services::render_snapshot::canonical_version_input_digest(
        design,
        &design.initial_params,
    )?;
    let latest = {
        let conn = state.db.lock().await;
        crate::db::get_thread_latest_version(&conn, &request.thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    };
    let Some(message) = latest.filter(|message| message.status == MessageStatus::Success) else {
        return Ok(None);
    };
    let Some(output) = message.output else {
        return Ok(None);
    };
    let existing_digest = crate::services::render_snapshot::canonical_version_input_digest(
        &output,
        &output.initial_params,
    )?;
    if existing_digest != candidate_digest {
        return Ok(None);
    }
    let (Some(artifact_bundle), Some(model_manifest)) =
        (message.artifact_bundle, message.model_manifest)
    else {
        return Ok(None);
    };
    validate_model_runtime_bundle(&model_manifest, &artifact_bundle)?;
    let snapshot_id = build_render_snapshot(RenderSnapshotInput {
        design: &output,
        effective_params: &output.initial_params,
        artifact_bundle: &artifact_bundle,
        model_manifest: &model_manifest,
    })?
    .snapshot_id;
    write_bound_source(request, state, app).await?;
    write_snapshot(
        request,
        Some(&message.id),
        &output,
        Some(&artifact_bundle),
        Some(&model_manifest),
        state,
        app,
    );
    Ok(Some(ManualCodeApplyResponse {
        thread_id: request.thread_id.clone(),
        base_message_id: request.base_message_id.clone(),
        message_id: Some(message.id),
        status: MessageStatus::Success,
        design_output: output,
        artifact_bundle: Some(artifact_bundle),
        model_manifest: Some(model_manifest),
        snapshot_id: Some(snapshot_id),
        parser_matched,
        error: None,
    }))
}

async fn attach_outcome(
    request: &ManualCodeApplyRequest,
    message_id: &str,
    design: &DesignOutput,
    status: MessageStatus,
    artifact_bundle: Option<&ArtifactBundle>,
    model_manifest: Option<&ModelManifest>,
    content: &str,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<()> {
    let configured_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    let owner_thread_id = crate::db::get_message_thread_id(&conn, message_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| AppError::persistence("Manual code version disappeared."))?;
    if owner_thread_id != request.thread_id {
        return Err(AppError::conflict(format!(
            "Manual code version '{}' belongs to thread '{}', not '{}'.",
            message_id, owner_thread_id, request.thread_id
        )));
    }
    crate::db::update_message_status_and_output(
        &conn,
        message_id,
        crate::db::MessageStatusUpdate {
            status: &status,
            output: Some(design),
            usage: None,
            artifact_bundle,
            model_manifest,
            structural_verification: None,
            visual_kind: None,
            content: Some(content),
        },
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    crate::thread_source_binding::refresh_on_version_append(
        app,
        &conn,
        configured_root.as_deref(),
        &request.thread_id,
        &design.title,
        &design.macro_code,
        message_id,
        artifact_bundle.map(|bundle| bundle.model_id.as_str()),
        Some(message_id),
    )?;
    if status == MessageStatus::Success && artifact_bundle.is_some() {
        if let Err(error) = crate::component_package_runtime::capture_latest_successful_components(
            app,
            &conn,
            &request.thread_id,
            message_id,
            &design.macro_code,
        ) {
            state.push_log(format!(
                "Component capture failed for version {message_id}: {error}"
            ));
        }
    }
    drop(conn);
    state
        .authoring_actor_registry
        .invalidate_authoring_actors_for_thread(&request.thread_id)
        .await;
    Ok(())
}

fn write_snapshot(
    request: &ManualCodeApplyRequest,
    message_id: Option<&str>,
    design: &DesignOutput,
    artifact_bundle: Option<&ArtifactBundle>,
    model_manifest: Option<&ModelManifest>,
    state: &AppState,
    app: &dyn PathResolver,
) {
    let Some(target_message_id) = message_id
        .map(str::to_string)
        .or_else(|| request.base_message_id.clone())
    else {
        return;
    };
    let snapshot = build_saved_version_snapshot(
        Some(design.clone()),
        request.thread_id.clone(),
        target_message_id,
        artifact_bundle.cloned(),
        model_manifest.cloned(),
        None,
    );
    *state.last_snapshot.lock().unwrap() = Some(snapshot.clone());
    write_last_snapshot(app, Some(&snapshot));
}

async fn fail_version(
    request: &ManualCodeApplyRequest,
    message_id: Option<String>,
    mut design: DesignOutput,
    parser_matched: bool,
    error: AppError,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ManualCodeApplyResponse> {
    design.response = "Manual code draft failed.".to_string();
    let content = error.to_string();
    if let Some(message_id) = message_id.as_deref() {
        attach_outcome(
            request,
            message_id,
            &design,
            MessageStatus::Error,
            None,
            None,
            &content,
            state,
            app,
        )
        .await?;
        write_snapshot(request, Some(message_id), &design, None, None, state, app);
    }
    Ok(ManualCodeApplyResponse {
        thread_id: request.thread_id.clone(),
        base_message_id: request.base_message_id.clone(),
        message_id,
        status: MessageStatus::Error,
        design_output: design,
        artifact_bundle: None,
        model_manifest: None,
        snapshot_id: None,
        parser_matched,
        error: Some(error),
    })
}

pub async fn apply_manual_code(
    request: ManualCodeApplyRequest,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ManualCodeApplyResponse> {
    let (ui_spec, parameters, parser_matched) =
        reconcile_controls(&request.source, &request.ui_spec, &request.parameters);
    let mut design = requested_design(&request, ui_spec, parameters);

    if request.persist {
        if let Some(response) =
            successful_no_op(&request, &design, parser_matched, state, app).await?
        {
            return Ok(response);
        }
    }

    let message_id = if request.persist {
        Some(
            add_manual_version(
                version_request(&request.thread_id, &design, MessageStatus::Working),
                state,
                app,
            )
            .await?,
        )
    } else {
        None
    };

    if let Err(error) = validate_design_output(&design) {
        return fail_version(
            &request,
            message_id,
            design,
            parser_matched,
            error,
            state,
            app,
        )
        .await;
    }

    let previous_manifest = previous_manifest(request.base_message_id.as_deref(), state).await?;
    let render_result = crate::services::render::render_model_with_previous_manifest(
        &design.macro_code,
        &design.initial_params,
        Some(design.macro_dialect.clone()),
        Some(design.geometry_backend),
        design.post_processing.as_ref(),
        previous_manifest.as_ref(),
        state,
        app,
    )
    .await;
    let artifact_bundle = match render_result {
        Ok(bundle) => bundle,
        Err(error) => {
            return fail_version(
                &request,
                message_id,
                design,
                parser_matched,
                error,
                state,
                app,
            )
            .await;
        }
    };
    let generated_manifest =
        match crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id) {
            Ok(manifest) => manifest,
            Err(error) => {
                return fail_version(
                    &request,
                    message_id,
                    design,
                    parser_matched,
                    error,
                    state,
                    app,
                )
                .await;
            }
        };
    let model_manifest = crate::mcp::handlers::carry_forward_semantic_manifest(
        previous_manifest.as_ref(),
        generated_manifest,
        &artifact_bundle,
    );
    let model_manifest = match crate::model_runtime::write_model_manifest(
        app,
        &artifact_bundle.model_id,
        &model_manifest,
    ) {
        Ok(manifest) => manifest,
        Err(error) => {
            return fail_version(
                &request,
                message_id,
                design,
                parser_matched,
                error,
                state,
                app,
            )
            .await;
        }
    };
    let artifact_bundle =
        match crate::model_runtime::read_artifact_bundle(app, &artifact_bundle.model_id) {
            Ok(bundle) => bundle,
            Err(error) => {
                return fail_version(
                    &request,
                    message_id,
                    design,
                    parser_matched,
                    error,
                    state,
                    app,
                )
                .await;
            }
        };
    if let Err(error) = validate_model_runtime_bundle(&model_manifest, &artifact_bundle) {
        return fail_version(
            &request,
            message_id,
            design,
            parser_matched,
            error,
            state,
            app,
        )
        .await;
    }

    let snapshot_id = match build_render_snapshot(RenderSnapshotInput {
        design: &design,
        effective_params: &design.initial_params,
        artifact_bundle: &artifact_bundle,
        model_manifest: &model_manifest,
    }) {
        Ok(snapshot) => snapshot.snapshot_id,
        Err(error) => {
            return fail_version(
                &request,
                message_id,
                design,
                parser_matched,
                error,
                state,
                app,
            )
            .await;
        }
    };

    if let Err(error) = write_bound_source(&request, state, app).await {
        return fail_version(
            &request,
            message_id,
            design,
            parser_matched,
            error,
            state,
            app,
        )
        .await;
    }

    design.response = if request.persist {
        "Manual edit appended as new version."
    } else {
        "Code draft applied."
    }
    .to_string();
    let content = design.response.clone();
    if let Some(message_id) = message_id.as_deref() {
        attach_outcome(
            &request,
            message_id,
            &design,
            MessageStatus::Success,
            Some(&artifact_bundle),
            Some(&model_manifest),
            &content,
            state,
            app,
        )
        .await?;
    }
    write_snapshot(
        &request,
        message_id.as_deref(),
        &design,
        Some(&artifact_bundle),
        Some(&model_manifest),
        state,
        app,
    );

    Ok(ManualCodeApplyResponse {
        thread_id: request.thread_id,
        base_message_id: request.base_message_id,
        message_id,
        status: MessageStatus::Success,
        design_output: design,
        artifact_bundle: Some(artifact_bundle),
        model_manifest: Some(model_manifest),
        snapshot_id: Some(snapshot_id),
        parser_matched,
        error: None,
    })
}
