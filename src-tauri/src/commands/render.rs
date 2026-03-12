use std::collections::BTreeSet;

use tauri::{AppHandle, State};

use super::session::write_last_snapshot;
use crate::db;
use crate::freecad;
use crate::models::{
    AppResult, AppState, ArtifactBundle, DesignOutput, DesignParams, InteractionMode, MacroDialect,
    ManifestBounds, ModelManifest, ModelSourceKind, ParamValue, UiField, UiSpec,
};

fn humanize_parameter_key(key: &str) -> String {
    key.split(|ch: char| matches!(ch, '_' | '-' | '.'))
        .filter(|token| !token.is_empty())
        .map(|token| {
            let mut chars = token.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn infer_imported_dimension_value(key: &str, bounds: Option<&ManifestBounds>) -> f64 {
    let Some(bounds) = bounds else {
        return 0.0;
    };

    if key.ends_with("_height") {
        (bounds.z_max - bounds.z_min).max(0.0)
    } else if key.ends_with("_depth") {
        (bounds.y_max - bounds.y_min).max(0.0)
    } else {
        (bounds.x_max - bounds.x_min).max(0.0)
    }
}

fn build_imported_ui_spec(manifest: &ModelManifest) -> UiSpec {
    let mut keys = BTreeSet::new();

    for group in &manifest.parameter_groups {
        if !group.editable {
            continue;
        }
        for key in &group.parameter_keys {
            keys.insert(key.clone());
        }
    }

    for part in &manifest.parts {
        if !part.editable {
            continue;
        }
        for key in &part.parameter_keys {
            keys.insert(key.clone());
        }
    }

    UiSpec {
        fields: keys
            .into_iter()
            .map(|key| UiField::Range {
                label: humanize_parameter_key(&key),
                key,
                min: Some(0.0),
                max: None,
                step: Some(1.0),
                min_from: None,
                max_from: None,
                frozen: false,
            })
            .collect(),
    }
}

fn build_imported_params(
    manifest: &ModelManifest,
    existing_params: &DesignParams,
    ui_spec: &UiSpec,
) -> DesignParams {
    let mut next = DesignParams::new();

    for field in &ui_spec.fields {
        let key = field.key().to_string();
        if let Some(value) = existing_params.get(&key) {
            next.insert(key, value.clone());
            continue;
        }

        let source_part = manifest.parts.iter().find(|part| {
            part.parameter_keys
                .iter()
                .any(|part_key| part_key == field.key())
        });
        next.insert(
            key,
            ParamValue::Number(infer_imported_dimension_value(
                field.key(),
                source_part.and_then(|part| part.bounds.as_ref()),
            )),
        );
    }

    next
}

fn build_imported_output(
    manifest: &ModelManifest,
    existing_output: Option<&DesignOutput>,
) -> DesignOutput {
    let ui_spec = build_imported_ui_spec(manifest);
    let existing_params = existing_output
        .map(|output| output.initial_params.clone())
        .unwrap_or_default();
    let initial_params = build_imported_params(manifest, &existing_params, &ui_spec);
    let title = if manifest.document.document_label.trim().is_empty() {
        if manifest.document.document_name.trim().is_empty() {
            "Imported FreeCAD Model".to_string()
        } else {
            manifest.document.document_name.clone()
        }
    } else {
        manifest.document.document_label.clone()
    };

    DesignOutput {
        title,
        version_name: existing_output
            .map(|output| output.version_name.clone())
            .unwrap_or_else(|| "Imported".to_string()),
        response: "Imported FreeCAD model.".to_string(),
        interaction_mode: InteractionMode::Design,
        macro_code: String::new(),
        macro_dialect: MacroDialect::Legacy,
        ui_spec,
        initial_params,
        post_processing: None,
    }
}

use crate::services::render::{
    self as render_service, configured_freecad_cmd, is_freecad_available,
};

#[tauri::command]
#[specta::specta]
pub async fn check_freecad(state: State<'_, AppState>) -> AppResult<bool> {
    Ok(is_freecad_available(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn render_stl(
    macro_code: String,
    parameters: DesignParams,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<String> {
    render_service::render_stl(&macro_code, &parameters, &state, &app).await
}

#[tauri::command]
#[specta::specta]
pub async fn render_model(
    macro_code: String,
    parameters: DesignParams,
    post_processing: Option<crate::contracts::PostProcessingSpec>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    render_service::render_model(
        &macro_code,
        &parameters,
        post_processing.as_ref(),
        &state,
        &app,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn import_fcstd(
    source_path: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
    let result = freecad::import_fcstd(
        &source_path,
        configured_freecad_cmd(&state).as_deref(),
        &app,
    );
    if result.is_ok() {
        let runtime_cache_dir = freecad::runtime_cache_dir(&app)?;
        freecad::evict_cache_if_needed(&runtime_cache_dir);
    }
    result
}

#[tauri::command]
#[specta::specta]
pub async fn apply_imported_model(
    artifact_bundle: ArtifactBundle,
    manifest: ModelManifest,
    parameters: DesignParams,
    message_id: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
    let (next_bundle, next_manifest) = freecad::apply_imported_model(
        &artifact_bundle,
        &manifest,
        &parameters,
        configured_freecad_cmd(&state).as_deref(),
        &app,
    )?;

    let mut persisted_output: Option<DesignOutput> = None;
    if let Some(message_id) = message_id.as_ref() {
        let db = state.db.lock().await;
        db::update_message_model_manifest(&db, message_id, &next_manifest).map_err(
            |err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()),
        )?;
        db::update_message_artifact_bundle(&db, message_id, &next_bundle).map_err(
            |err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()),
        )?;

        let existing_output = db::get_message_output_and_thread(&db, message_id)
            .map_err(|err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()))?
            .map(|(output, _)| output);
        let mut imported_output = build_imported_output(&next_manifest, existing_output.as_ref());
        imported_output.initial_params = parameters.clone();
        db::update_message_output(&db, message_id, &imported_output)
            .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
        persisted_output = Some(imported_output);
    }

    let snapshot_to_write = {
        let mut last = state.last_snapshot.lock().unwrap();
        if let Some(snapshot) = last.as_mut() {
            let snapshot_matches_model = snapshot
                .model_manifest
                .as_ref()
                .map(|current| current.model_id.as_str() == next_bundle.model_id.as_str())
                .unwrap_or(false)
                || snapshot
                    .artifact_bundle
                    .as_ref()
                    .map(|bundle| bundle.model_id.as_str() == next_bundle.model_id.as_str())
                    .unwrap_or(false);
            let snapshot_matches_message = message_id
                .as_deref()
                .map(|id| snapshot.message_id.as_deref() == Some(id))
                .unwrap_or(true);

            if snapshot_matches_model && snapshot_matches_message {
                snapshot.artifact_bundle = Some(next_bundle.clone());
                snapshot.model_manifest = Some(next_manifest.clone());
                if let Some(output) = persisted_output.clone() {
                    snapshot.design = Some(output);
                }
                Some(snapshot.clone())
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(snapshot) = snapshot_to_write.as_ref() {
        write_last_snapshot(&app, Some(snapshot));
    }

    let runtime_cache_dir = freecad::runtime_cache_dir(&app)?;
    freecad::evict_cache_if_needed(&runtime_cache_dir);
    Ok(next_bundle)
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_manifest(model_id: String, app: AppHandle) -> AppResult<ModelManifest> {
    freecad::get_model_manifest(&app, &model_id)
}

#[tauri::command]
#[specta::specta]
pub async fn save_model_manifest(
    model_id: String,
    manifest: ModelManifest,
    message_id: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    freecad::save_model_manifest(&app, &model_id, &manifest)?;

    let mut persisted_output: Option<DesignOutput> = None;

    if let Some(message_id) = message_id.as_ref() {
        let db = state.db.lock().await;
        db::update_message_model_manifest(&db, message_id, &manifest).map_err(
            |err: rusqlite::Error| crate::models::AppError::persistence(err.to_string()),
        )?;

        if matches!(manifest.source_kind, ModelSourceKind::ImportedFcstd) {
            let existing_output = db::get_message_output_and_thread(&db, message_id)
                .map_err(|err: rusqlite::Error| {
                    crate::models::AppError::persistence(err.to_string())
                })?
                .map(|(output, _)| output);
            let imported_output = build_imported_output(&manifest, existing_output.as_ref());
            db::update_message_output(&db, message_id, &imported_output)
                .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
            persisted_output = Some(imported_output);
        }
    }

    let snapshot_to_write = {
        let mut last = state.last_snapshot.lock().unwrap();
        let Some(snapshot) = last.as_mut() else {
            return Ok(());
        };

        let snapshot_matches_model = snapshot
            .model_manifest
            .as_ref()
            .map(|current| current.model_id.as_str() == model_id.as_str())
            .unwrap_or(false)
            || snapshot
                .artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.as_str() == model_id.as_str())
                .unwrap_or(false);
        let snapshot_matches_message = message_id
            .as_deref()
            .map(|id| snapshot.message_id.as_deref() == Some(id))
            .unwrap_or(true);

        if snapshot_matches_model && snapshot_matches_message {
            snapshot.model_manifest = Some(manifest.clone());
            if let Some(output) = persisted_output.clone() {
                snapshot.design = Some(output);
            }
            Some(snapshot.clone())
        } else {
            None
        }
    };

    if let Some(snapshot) = snapshot_to_write.as_ref() {
        write_last_snapshot(&app, Some(snapshot));
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_default_macro(app: AppHandle) -> AppResult<String> {
    freecad::get_default_macro(&app)
}

#[tauri::command]
#[specta::specta]
pub async fn get_mess_stl_path(app: AppHandle) -> AppResult<String> {
    let path = freecad::resolve_resource_path(
        &app,
        "templates/mess.stl",
        &["../templates/mess.stl", "templates/mess.stl"],
    )?;

    Ok(path
        .to_str()
        .ok_or_else(|| crate::models::AppError::internal("Invalid mess STL path."))?
        .to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn export_file(source_path: String, target_path: String) -> AppResult<()> {
    std::fs::copy(&source_path, &target_path).map_err(|err| {
        crate::models::AppError::persistence(format!("Failed to export file: {}", err))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        Advisory, AdvisoryCondition, AdvisorySeverity, ControlPrimitive, ControlPrimitiveKind,
        ControlView, ControlViewScope, ControlViewSection, ControlViewSource, DocumentMetadata,
        EnrichmentStatus, ManifestEnrichmentState, ParameterGroup, PartBinding, PrimitiveBinding,
        SelectionTarget, SelectionTargetKind, MODEL_RUNTIME_SCHEMA_VERSION,
    };

    fn sample_imported_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "imported-fcstd-test".to_string(),
            source_kind: ModelSourceKind::ImportedFcstd,
            document: DocumentMetadata {
                document_name: "Imported Shell".to_string(),
                document_label: "Imported Shell".to_string(),
                source_path: Some("/tmp/model.FCStd".to_string()),
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: vec![PartBinding {
                part_id: "part-outer-shell".to_string(),
                freecad_object_name: "OuterShell001".to_string(),
                label: "Outer Shell".to_string(),
                kind: "Part::Feature".to_string(),
                semantic_role: Some("body".to_string()),
                viewer_asset_path: Some("/tmp/outer-shell.stl".to_string()),
                viewer_node_ids: vec!["OuterShell001".to_string()],
                parameter_keys: vec![
                    "outer_shell_width".to_string(),
                    "outer_shell_depth".to_string(),
                    "outer_shell_height".to_string(),
                ],
                editable: true,
                bounds: Some(ManifestBounds {
                    x_min: 0.0,
                    y_min: 0.0,
                    z_min: 0.0,
                    x_max: 34.0,
                    y_max: 30.0,
                    z_max: 22.0,
                }),
                volume: None,
                area: None,
            }],
            parameter_groups: vec![ParameterGroup {
                group_id: "proposal-bind-proposal-outershell".to_string(),
                label: "Expose Outer Shell dimensions".to_string(),
                parameter_keys: vec![
                    "outer_shell_width".to_string(),
                    "outer_shell_depth".to_string(),
                    "outer_shell_height".to_string(),
                ],
                part_ids: vec!["part-outer-shell".to_string()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(0),
            }],
            control_primitives: vec![ControlPrimitive {
                primitive_id: "primitive-outer-shell-size".to_string(),
                label: "Outer Shell Size".to_string(),
                kind: ControlPrimitiveKind::Number,
                source: ControlViewSource::Generated,
                part_ids: vec!["part-outer-shell".to_string()],
                bindings: vec![PrimitiveBinding {
                    parameter_key: "outer_shell_width".to_string(),
                    scale: 1.0,
                    offset: 0.0,
                    min: None,
                    max: None,
                }],
                editable: true,
                order: 0,
            }],
            control_relations: Vec::new(),
            control_views: vec![ControlView {
                view_id: "view-outer-shell".to_string(),
                label: "Outer Shell".to_string(),
                scope: ControlViewScope::Part,
                part_ids: vec!["part-outer-shell".to_string()],
                primitive_ids: vec!["primitive-outer-shell-size".to_string()],
                sections: vec![ControlViewSection {
                    section_id: "section-primary".to_string(),
                    label: "Primary".to_string(),
                    primitive_ids: vec!["primitive-outer-shell-size".to_string()],
                    collapsed: false,
                }],
                is_default: true,
                source: ControlViewSource::Generated,
                status: EnrichmentStatus::Accepted,
                order: 0,
            }],
            advisories: vec![Advisory {
                advisory_id: "advisory-outer-shell".to_string(),
                label: "Shell note".to_string(),
                severity: AdvisorySeverity::Info,
                primitive_ids: vec!["primitive-outer-shell-size".to_string()],
                view_ids: vec!["view-outer-shell".to_string()],
                message: "Imported shell dimensions drive preview transforms.".to_string(),
                condition: AdvisoryCondition::Always,
                threshold: None,
            }],
            selection_targets: vec![SelectionTarget {
                part_id: "part-outer-shell".to_string(),
                viewer_node_id: "OuterShell001".to_string(),
                label: "Outer Shell".to_string(),
                kind: SelectionTargetKind::Part,
                editable: true,
            }],
            warnings: vec![
                "Imported FCStd bindings were accepted from heuristic proposals.".to_string(),
            ],
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::Accepted,
                proposals: Vec::new(),
            },
        }
    }

    #[test]
    fn build_imported_output_synthesizes_numeric_controls_from_manifest() {
        let output = build_imported_output(&sample_imported_manifest(), None);

        assert_eq!(output.title, "Imported Shell");
        assert_eq!(output.macro_code, "");
        assert_eq!(output.ui_spec.fields.len(), 3);
        assert_eq!(
            output.initial_params.get("outer_shell_width"),
            Some(&ParamValue::Number(34.0))
        );
        assert_eq!(
            output.initial_params.get("outer_shell_depth"),
            Some(&ParamValue::Number(30.0))
        );
        assert_eq!(
            output.initial_params.get("outer_shell_height"),
            Some(&ParamValue::Number(22.0))
        );
    }
}
