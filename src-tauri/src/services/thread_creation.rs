use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;
use uuid::Uuid;

use crate::contracts::{
    AppError, AppResult, DesignParams, GeometryBackend, PostProcessingSpec, SourceLanguage, UiSpec,
    WorkspaceProjection,
};
use crate::models::{AppState, PathResolver};
use crate::services::manual_code::{apply_manual_code, ManualCodeApplyRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DesignThreadCreationMode {
    Blank,
    Macro,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDesignThreadIntent {
    pub mode: DesignThreadCreationMode,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub base_thread_id: Option<String>,
    #[serde(default)]
    pub base_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatedThreadSourceDocument {
    pub folder: String,
    pub file: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateDesignThreadResponse {
    pub thread_id: String,
    pub source_document: CreatedThreadSourceDocument,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parser_matched: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_version_error: Option<AppError>,
    pub workspace: WorkspaceProjection,
}

fn normalized_title(title: Option<&str>) -> String {
    title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Untitled design")
        .to_string()
}

fn validated_source(intent: &CreateDesignThreadIntent) -> AppResult<Option<String>> {
    match intent.mode {
        DesignThreadCreationMode::Blank => {
            if intent
                .source
                .as_deref()
                .is_some_and(|source| !source.is_empty())
            {
                return Err(AppError::validation(
                    "Blank design thread intent must not include source.",
                ));
            }
            Ok(None)
        }
        DesignThreadCreationMode::Macro => intent
            .source
            .as_deref()
            .filter(|source| !source.trim().is_empty())
            .map(str::to_string)
            .map(Some)
            .ok_or_else(|| {
                AppError::validation("Macro design thread intent requires non-empty source.")
            }),
    }
}

struct InitialAuthoringContext {
    source_language: SourceLanguage,
    geometry_backend: GeometryBackend,
    parameters: DesignParams,
    ui_spec: UiSpec,
    post_processing: Option<PostProcessingSpec>,
}

async fn initial_authoring_context(
    intent: &CreateDesignThreadIntent,
    state: &AppState,
) -> AppResult<InitialAuthoringContext> {
    match (
        intent.base_thread_id.as_deref(),
        intent.base_message_id.as_deref(),
    ) {
        (Some(base_thread_id), Some(base_message_id)) => {
            if intent.mode != DesignThreadCreationMode::Macro {
                return Err(AppError::validation(
                    "Design thread base identity is valid only for macro mode.",
                ));
            }
            let message = {
                let conn = state.db.lock().await;
                crate::db::get_thread_message_version(&conn, base_thread_id, base_message_id)
                    .map_err(|error| AppError::persistence(error.to_string()))?
                    .ok_or_else(|| {
                        AppError::not_found(format!(
                            "Design thread base version '{base_message_id}' was not found in thread '{base_thread_id}'."
                        ))
                    })?
            };
            let output = message.output.ok_or_else(|| {
                AppError::validation(format!(
                    "Design thread base version '{base_message_id}' has no authoring source context."
                ))
            })?;
            Ok(InitialAuthoringContext {
                source_language: output.source_language,
                geometry_backend: output.geometry_backend,
                parameters: output.initial_params,
                ui_spec: output.ui_spec,
                post_processing: output.post_processing,
            })
        }
        (None, None) => {
            let config = state.config.lock().map_err(|_| {
                AppError::internal("Design thread configuration lock was poisoned.")
            })?;
            Ok(InitialAuthoringContext {
                source_language: config.default_source_language,
                geometry_backend: config.default_geometry_backend,
                parameters: DesignParams::new(),
                ui_spec: UiSpec::default(),
                post_processing: None,
            })
        }
        _ => Err(AppError::validation(
            "Design thread baseThreadId and baseMessageId must be supplied together.",
        )),
    }
}

fn remove_new_folder(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(path).map_err(|error| {
        AppError::persistence(format!(
            "Failed to remove incomplete design thread folder '{}': {error}",
            path.display()
        ))
    })
}

async fn create_bound_thread(
    thread_id: &str,
    title: &str,
    projects_root: Option<&str>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<crate::thread_source_binding::ThreadSourceBinding> {
    let planned_folder =
        crate::thread_source_binding::binding_folder(app, projects_root, title, thread_id)?;
    let folder_existed = planned_folder.exists();
    let result: AppResult<crate::thread_source_binding::ThreadSourceBinding> = {
        let conn = state.db.lock().await;
        (|| {
            let transaction = conn
                .unchecked_transaction()
                .map_err(|error| AppError::persistence(error.to_string()))?;
            let now = crate::thread_source_binding::now_secs();
            crate::db::create_or_update_thread(&transaction, thread_id, title, now, None)
                .map_err(|error| AppError::persistence(error.to_string()))?;
            let binding = crate::thread_source_binding::bind_new_thread(
                app,
                &transaction,
                projects_root,
                thread_id,
                title,
            )?;
            transaction
                .commit()
                .map_err(|error| AppError::persistence(error.to_string()))?;
            Ok(binding)
        })()
    };

    match result {
        Ok(binding) => Ok(binding),
        Err(error) => {
            if !folder_existed {
                remove_new_folder(&planned_folder)?;
            }
            Err(error)
        }
    }
}

fn source_document(
    binding: &crate::thread_source_binding::ThreadSourceBinding,
) -> AppResult<CreatedThreadSourceDocument> {
    let source = std::fs::read_to_string(&binding.source_path).map_err(|error| {
        AppError::persistence(format!(
            "Failed to read created design source '{}': {error}",
            binding.source_path
        ))
    })?;
    Ok(CreatedThreadSourceDocument {
        folder: binding.folder_path.clone(),
        file: binding.source_path.clone(),
        source,
    })
}

pub async fn create_design_thread(
    intent: CreateDesignThreadIntent,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<CreateDesignThreadResponse> {
    let source = validated_source(&intent)?;
    let authoring_context = initial_authoring_context(&intent, state).await?;
    let title = normalized_title(intent.title.as_deref());
    let projects_root = {
        let config = state
            .config
            .lock()
            .map_err(|_| AppError::internal("Design thread configuration lock was poisoned."))?;
        config.projects_root.clone()
    };
    let thread_id = Uuid::new_v4().to_string();
    let mut binding =
        create_bound_thread(&thread_id, &title, projects_root.as_deref(), state, app).await?;

    let initial = match source.as_deref() {
        Some(source_text) => Some(
            apply_manual_code(
                ManualCodeApplyRequest {
                    thread_id: thread_id.clone(),
                    base_message_id: None,
                    source: source_text.to_string(),
                    persist: true,
                    title: Some(title.clone()),
                    version_name: Some("V1".to_string()),
                    ui_spec: authoring_context.ui_spec,
                    parameters: authoring_context.parameters,
                    post_processing: authoring_context.post_processing,
                    source_language: Some(authoring_context.source_language),
                    geometry_backend: Some(authoring_context.geometry_backend),
                },
                state,
                app,
            )
            .await?,
        ),
        None => None,
    };

    if let Some(initial_response) = initial.as_ref() {
        let conn = state.db.lock().await;
        binding = crate::thread_source_binding::get_binding(&conn, &thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .ok_or_else(|| {
                AppError::persistence(format!(
                    "Created design thread '{}' lost its source binding.",
                    thread_id
                ))
            })?;
        drop(conn);
        if initial_response.message_id.is_none() {
            if let Some(source_text) = source.as_deref() {
                crate::project_mirror::write_bound_source(
                    Path::new(&binding.source_path),
                    source_text,
                )?;
            }
        }
    }
    let source_document = source_document(&binding)?;
    let preferred_message_id = initial
        .as_ref()
        .and_then(|response| response.message_id.as_deref());
    let workspace = {
        let conn = state.db.lock().await;
        crate::services::history::get_workspace_projection(
            &conn,
            &thread_id,
            preferred_message_id,
            Some(20),
        )?
    };
    let initial_version_id = initial
        .as_ref()
        .and_then(|response| response.message_id.clone());
    let snapshot_id = initial
        .as_ref()
        .and_then(|response| response.snapshot_id.clone());
    let parser_matched = initial.as_ref().map(|response| response.parser_matched);
    let initial_version_error = initial.and_then(|response| response.error);
    if initial_version_id.is_none() {
        *state.last_snapshot.lock().unwrap() = None;
        crate::services::session::write_last_snapshot(app, None);
    }

    Ok(CreateDesignThreadResponse {
        thread_id,
        source_document,
        initial_version_id,
        snapshot_id,
        parser_matched,
        initial_version_error,
        workspace,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_boundary_rejects_caller_owned_thread_identity() {
        let error = serde_json::from_value::<CreateDesignThreadIntent>(serde_json::json!({
            "mode": "blank",
            "title": "New design",
            "threadId": "frontend-owned"
        }))
        .expect_err("thread identity belongs to Rust");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn intent_boundary_accepts_exact_immutable_base_identity() {
        serde_json::from_value::<CreateDesignThreadIntent>(serde_json::json!({
            "mode": "macro",
            "title": "Edited fork",
            "source": "(model)",
            "baseThreadId": "source-thread",
            "baseMessageId": "source-version"
        }))
        .expect("base identity is caller input, not caller-owned lifecycle state");
    }

    #[test]
    fn mode_controls_source_admission_before_thread_creation() {
        let blank_with_source = CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Blank,
            title: None,
            source: Some("(model)".into()),
            base_thread_id: None,
            base_message_id: None,
        };
        assert!(validated_source(&blank_with_source).is_err());

        let macro_without_source = CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Macro,
            title: None,
            source: None,
            base_thread_id: None,
            base_message_id: None,
        };
        assert!(validated_source(&macro_without_source).is_err());
    }
}
