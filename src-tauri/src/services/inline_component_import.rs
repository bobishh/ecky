use crate::component_import_runtime::{
    copy_inline_library_component_at, LibraryComponentSelection,
};
use crate::contracts::{
    AppError, AppResult, DesignOutput, DesignParams, MacroDialect, SourceLanguage,
};
use crate::models::{AppState, PathResolver};
use crate::services::manual_code::{ManualCodeApplyRequest, ManualCodeApplyResponse};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInlineComponentImportInput {
    pub thread_id: String,
    pub base_message_id: String,
    pub expected_source_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_mm: Option<[f64; 3]>,
    #[serde(flatten)]
    pub component: LibraryComponentSelection,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InlineComponentImportPreview {
    pub model_stl_path: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInlineComponentImportResult {
    pub version: ManualCodeApplyResponse,
    pub source_digest: String,
    pub entry_symbol: String,
    pub part_key: String,
}

struct InlineComponentImportContext {
    source: String,
    source_path: PathBuf,
    base_design: DesignOutput,
}

pub async fn preview_inline_component_import(
    input: ApplyInlineComponentImportInput,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<InlineComponentImportPreview> {
    let context = checked_context(&input, state).await?;
    let imported = copy_inline_library_component_at(app, input.component, &context.source, None)?;
    let source = imported.standalone_source()?;
    let artifact = crate::services::render::render_model_with_previous_manifest_silent(
        &source,
        &DesignParams::new(),
        Some(MacroDialect::EckyIrV0),
        Some(context.base_design.geometry_backend),
        None,
        None,
        state,
        app,
    )
    .await?;
    if artifact.model_stl_path.trim().is_empty() {
        return Err(AppError::validation(
            "Component preview has no STL geometry.",
        ));
    }
    Ok(InlineComponentImportPreview {
        model_stl_path: artifact.model_stl_path,
    })
}

pub async fn apply_inline_component_import(
    input: ApplyInlineComponentImportInput,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ApplyInlineComponentImportResult> {
    let context = checked_context(&input, state).await?;
    let thread_id = input.thread_id.trim();
    let base_message_id = input.base_message_id.trim();
    let imported =
        copy_inline_library_component_at(app, input.component, &context.source, input.position_mm)?;
    let request = manual_code_request(
        thread_id,
        base_message_id,
        &context,
        imported.authored_source,
    );
    let version = crate::services::manual_code::apply_manual_code_guarded(
        request,
        Some(base_message_id.to_string()),
        context.base_design.macro_code,
        state,
        app,
    )
    .await?;
    let canonical_source = std::fs::read_to_string(&context.source_path).map_err(|error| {
        AppError::persistence(format!(
            "Failed to reread bound component import source '{}': {error}",
            context.source_path.display()
        ))
    })?;

    Ok(ApplyInlineComponentImportResult {
        version,
        source_digest: crate::services::render_snapshot::canonical_source_digest(&canonical_source),
        entry_symbol: imported.entry_symbol,
        part_key: imported.part_key,
    })
}

async fn checked_context(
    input: &ApplyInlineComponentImportInput,
    state: &AppState,
) -> AppResult<InlineComponentImportContext> {
    let thread_id = require_id(&input.thread_id, "Component import thread id")?;
    let base_message_id = require_id(&input.base_message_id, "Component import base message id")?;
    let expected_source_digest = require_id(
        &input.expected_source_digest,
        "Component import expected source digest",
    )?;
    let conn = state.db.lock().await;
    let base_message = crate::db::get_thread_latest_version(&conn, thread_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .filter(|message| message.id == base_message_id)
        .ok_or_else(|| {
            AppError::conflict(
                "The target version changed before component import. Open the latest model version and try again.",
            )
        })?;
    let base_design = base_message.output.ok_or_else(|| {
        AppError::validation(format!(
            "Component import base message '{}' has no design output.",
            base_message_id
        ))
    })?;
    if base_design.source_language != SourceLanguage::EckyIrV0 {
        return Err(AppError::validation(
            "Component placement requires Ecky source.",
        ));
    }
    let binding = crate::thread_source_binding::get_binding(&conn, thread_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| {
            AppError::not_found(format!(
                "Design thread '{}' has no bound model.ecky source.",
                thread_id
            ))
        })?;
    let source_path = PathBuf::from(binding.source_path);
    let source = std::fs::read_to_string(&source_path).map_err(|error| {
        AppError::persistence(format!(
            "Failed to read bound component import source '{}': {error}",
            source_path.display()
        ))
    })?;

    let actual_source_digest = crate::services::render_snapshot::canonical_source_digest(&source);
    if actual_source_digest != expected_source_digest {
        return Err(AppError::conflict(format!(
            "Bound source changed before component import: expected digest '{}', actual digest '{}'.",
            expected_source_digest, actual_source_digest
        )));
    }

    Ok(InlineComponentImportContext {
        source,
        source_path,
        base_design,
    })
}

fn manual_code_request(
    thread_id: &str,
    base_message_id: &str,
    context: &InlineComponentImportContext,
    source: String,
) -> ManualCodeApplyRequest {
    ManualCodeApplyRequest {
        thread_id: thread_id.to_string(),
        base_message_id: Some(base_message_id.to_string()),
        source,
        persist: true,
        title: Some(context.base_design.title.clone()),
        version_name: Some("Inline component import".to_string()),
        ui_spec: context.base_design.ui_spec.clone(),
        parameters: context.base_design.initial_params.clone(),
        post_processing: context.base_design.post_processing.clone(),
        source_language: Some(context.base_design.source_language),
        geometry_backend: Some(context.base_design.geometry_backend),
    }
}

fn require_id<'a>(value: &'a str, label: &str) -> AppResult<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::validation(format!("{} cannot be empty.", label)));
    }
    Ok(value)
}
