use base64::{engine::general_purpose, Engine as _};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};
use uuid::Uuid;

use super::session::{build_runtime_snapshot, write_last_snapshot};
use crate::context::*;
use crate::models::{
    validate_design_output, AppError, AppErrorCode, AppResult, AppState, ArtifactBundle,
    Attachment, AttachmentKind, DesignOutput, FinalizeStatus, GenerateDesignOptions,
    GenerateOutput, IntentDecision, InteractionMode, MacroDialect, Message, MessageRole,
    MessageStatus, ModelManifest, UiSpec, UsageSummary,
};
use crate::{
    db, fallback_intent, freecad, llm, persist_thread_summary, persist_user_prompt_references,
    TECHNICAL_SYSTEM_PROMPT,
};

fn selected_engine(state: &State<'_, AppState>) -> AppResult<crate::models::Engine> {
    let config = state.config.lock().unwrap();
    let engine = config
        .engines
        .iter()
        .find(|candidate| candidate.id == config.selected_engine_id)
        .cloned()
        .ok_or_else(|| AppError::validation("No active engine selected."))?;

    if engine.provider != "ollama" && engine.api_key.trim().is_empty() {
        return Err(AppError::validation(format!(
            "Selected engine '{}' has no API key configured.",
            engine.name
        )));
    }

    Ok(engine)
}

fn prepare_images(image_data: Option<String>, attachments: Option<Vec<Attachment>>) -> Vec<String> {
    let mut images = Vec::new();
    if let Some(main_image) = image_data {
        images.push(main_image);
    }
    if let Some(attachments) = attachments {
        for attachment in attachments {
            if attachment.kind == AttachmentKind::Image {
                if let Some(data_url) = attachment_image_data_url(&attachment) {
                    images.push(data_url);
                }
            }
        }
    }
    images
}

fn attachment_image_data_url(attachment: &Attachment) -> Option<String> {
    let bytes = fs::read(&attachment.path).ok()?;
    let b64 = general_purpose::STANDARD.encode(bytes);
    let ext = attachment
        .path
        .split('.')
        .next_back()
        .unwrap_or("png")
        .to_lowercase();
    let mime = if ext == "jpg" || ext == "jpeg" {
        "image/jpeg"
    } else {
        "image/png"
    };
    Some(format!("data:{};base64,{}", mime, b64))
}

fn collect_attachment_images(attachments: Option<&Vec<Attachment>>) -> Vec<String> {
    attachments
        .into_iter()
        .flat_map(|items| items.iter())
        .filter(|attachment| attachment.kind == AttachmentKind::Image)
        .filter_map(attachment_image_data_url)
        .collect()
}

fn build_visual_input_notes(
    image_data: Option<&String>,
    attachments: Option<&Vec<Attachment>>,
) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut image_index = 1usize;

    if image_data.is_some() {
        lines.push(format!(
            "Image {} is the current 3D viewport screenshot.",
            image_index
        ));
        lines.push(
            "If it contains colored strokes, arrows, circles, or hand-drawn marks, treat them as explicit user annotations highlighting the intended area or requested change."
                .to_string(),
        );
        image_index += 1;
    }

    if let Some(attachments) = attachments {
        for attachment in attachments {
            if attachment.kind != AttachmentKind::Image {
                continue;
            }
            let explanation = attachment.explanation.trim();
            if explanation.is_empty() {
                lines.push(format!(
                    "Image {} is attachment '{}' from the user.",
                    image_index, attachment.name
                ));
            } else {
                lines.push(format!(
                    "Image {} is attachment '{}' from the user. User note: {}",
                    image_index, attachment.name, explanation
                ));
            }
            image_index += 1;
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(format!("VISUAL INPUTS\n{}", lines.join("\n")))
    }
}

fn load_framework_contract(app: &AppHandle) -> Option<String> {
    let path = freecad::resolve_resource_path(
        app,
        "model-runtime/cad_framework_contract.md",
        &[
            "../model-runtime/cad_framework_contract.md",
            "model-runtime/cad_framework_contract.md",
        ],
    )
    .ok()?;
    fs::read_to_string(path).ok()
}

fn should_use_framework_for_generation(ctx: &PromptContext) -> bool {
    ctx.last_output
        .as_ref()
        .map(|output| output.macro_dialect.is_framework())
        .unwrap_or(true)
}

fn prepend_follow_up_context(prompt: String, follow_up_question: Option<&str>) -> String {
    let Some(question) = follow_up_question
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return prompt;
    };

    format!(
        "FOLLOW-UP CONTEXT (AUTHORITATIVE)\nThe assistant previously asked this unresolved clarification question:\n\"{}\"\n\nThe current user request is the answer to that question. Continue the pending design task using the user's answer. Do not repeat the same clarification question unless the new answer is still genuinely insufficient.\n\n{}",
        question, prompt
    )
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn generate_design(
    prompt: String,
    thread_id: Option<String>,
    parent_macro_code: Option<String>,
    working_design: Option<DesignOutput>,
    _is_retry: bool,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    options: Option<GenerateDesignOptions>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<GenerateOutput> {
    {
        let (explicit_mcp, no_enabled_engine) = {
            let config = state.config.lock().unwrap();
            let explicit_mcp = config.connection_type.as_deref() == Some("mcp");
            let no_enabled_engine = !config.engines.iter().any(|e| e.enabled);
            (explicit_mcp, no_enabled_engine)
        };

        if explicit_mcp || no_enabled_engine {
            let sessions = state.mcp_sessions.lock().await;
            if sessions.is_empty() && no_enabled_engine {
                return Err(AppError::validation(
                    "No active engine or MCP agent found. Switch to API Key mode in Settings → Agents, or connect an agent."
                ));
            }
            // In MCP mode the frontend routes user input through request_user_prompt,
            // not through generate. If we somehow get here, do nothing.
            return Err(AppError::validation(
                "In MCP mode, generation is driven by your external agent.",
            ));
        }
    }
    let engine = selected_engine(&state)?;
    let options = options.unwrap_or_default();
    let question_mode = options.question_mode.unwrap_or(false);
    let follow_up_question = options.follow_up_question;
    let ctx = {
        let db = state.db.lock().await;
        crate::context::assemble_context(&db, thread_id, working_design, parent_macro_code)
    };
    let intent_mode = if question_mode {
        "QUESTION_ONLY"
    } else {
        "DESIGN_EDIT"
    };
    let framework_enabled = should_use_framework_for_generation(&ctx);
    let framework_contract = if framework_enabled {
        load_framework_contract(&app)
    } else {
        None
    };
    let contextual_prompt = format_contextual_prompt(
        &ctx,
        &prompt,
        TECHNICAL_SYSTEM_PROMPT,
        intent_mode,
        framework_contract.as_deref(),
    );
    let contextual_prompt =
        prepend_follow_up_context(contextual_prompt, follow_up_question.as_deref());
    let contextual_prompt =
        if let Some(notes) = build_visual_input_notes(image_data.as_ref(), attachments.as_ref()) {
            format!("{}\n\n{}", contextual_prompt, notes)
        } else {
            contextual_prompt
        };
    let images = prepare_images(image_data, attachments);

    let mut output = llm::generate_design(&engine, &contextual_prompt, images)
        .await
        .map_err(|raw_body| {
            AppError::with_details(
                AppErrorCode::Provider,
                "LLM response could not be parsed into a design output.",
                raw_body,
            )
        })?;

    if !question_mode {
        if framework_enabled {
            if let Some(parsed) =
                crate::commands::design::derive_framework_controls(&output.data.macro_code)?
            {
                output.data.ui_spec = UiSpec {
                    fields: parsed.fields.clone(),
                };
                output.data.initial_params = parsed.params;
                output.data.macro_dialect = MacroDialect::CadFrameworkV1;
            } else {
                output.data.macro_dialect = MacroDialect::Legacy;
            }
        } else {
            output.data.macro_dialect = MacroDialect::Legacy;
        }
    }

    if question_mode {
        output.data.interaction_mode = InteractionMode::Question;
        if let Some(previous) = &ctx.last_output {
            output.data.title = previous.title.clone();
            output.data.version_name = previous.version_name.clone();
            output.data.macro_code = previous.macro_code.clone();
            output.data.ui_spec = previous.ui_spec.clone();
            output.data.initial_params = previous.initial_params.clone();
            output.data.macro_dialect = previous.macro_dialect.clone();
        }
        if output.data.version_name.trim().is_empty() {
            output.data.version_name = "Q&A".to_string();
        }
        if output.data.response.trim().is_empty() {
            output.data.response = "Question answered. Geometry unchanged.".to_string();
        }
    }

    validate_design_output(&output.data)?;

    Ok(GenerateOutput {
        design: output.data,
        thread_id: ctx.thread_id,
        message_id: Uuid::new_v4().to_string(),
        usage: output.usage,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn init_generation_attempt(
    thread_id: String,
    prompt: String,
    attachments: Option<Vec<Attachment>>,
    image_data: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let assistant_message_id = Uuid::new_v4().to_string();
    let user_message_id = Uuid::new_v4().to_string();

    {
        let db = state.db.lock().await;
        if db::get_thread_title(&db, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .is_none()
        {
            let traits = crate::generate_genie_traits();
            let initial_title = {
                let chars: Vec<char> = prompt.chars().collect();
                if chars.len() > 30 {
                    format!("{}...", chars[..27].iter().collect::<String>())
                } else {
                    prompt.clone()
                }
            };
            db::create_or_update_thread(&db, &thread_id, &initial_title, now, Some(&traits))
                .map_err(|err| AppError::persistence(err.to_string()))?;
        }

        let attachment_images = collect_attachment_images(attachments.as_ref());
        let user_msg = Message {
            id: user_message_id.clone(),
            role: MessageRole::User,
            content: prompt.clone(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            agent_origin: None,
            image_data,
            attachment_images,
            timestamp: now,
        };
        db::add_message(&db, &thread_id, &user_msg)
            .map_err(|err| AppError::persistence(err.to_string()))?;
        persist_user_prompt_references(
            &db,
            &thread_id,
            &user_message_id,
            &prompt,
            attachments.as_ref(),
            now,
        )
        .map_err(AppError::persistence)?;

        let assistant_msg = Message {
            id: assistant_message_id.clone(),
            role: MessageRole::Assistant,
            content: "Generating...".to_string(),
            status: MessageStatus::Pending,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            agent_origin: None,
            image_data: None,
            attachment_images: Vec::new(),
            timestamp: now + 1,
        };
        db::add_message(&db, &thread_id, &assistant_msg)
            .map_err(|err| AppError::persistence(err.to_string()))?;
    }

    Ok(assistant_message_id)
}

#[tauri::command]
#[specta::specta]
pub async fn finalize_generation_attempt(
    message_id: String,
    status: FinalizeStatus,
    design: Option<DesignOutput>,
    usage: Option<UsageSummary>,
    artifact_bundle: Option<ArtifactBundle>,
    model_manifest: Option<ModelManifest>,
    error_message: Option<String>,
    response_text: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    if let Some(design) = design.as_ref() {
        validate_design_output(design)?;
    }

    let db = state.db.lock().await;
    let content = match status {
        FinalizeStatus::Success => {
            if let Some(design) = &design {
                if design.response.trim().is_empty() {
                    Some("Synthesized design output.".to_string())
                } else {
                    Some(design.response.clone())
                }
            } else {
                response_text.clone()
            }
        }
        FinalizeStatus::Error | FinalizeStatus::Discarded => error_message.clone(),
    };

    db::update_message_status_and_output(
        &db,
        &message_id,
        &match status {
            FinalizeStatus::Success => MessageStatus::Success,
            FinalizeStatus::Error => MessageStatus::Error,
            FinalizeStatus::Discarded => MessageStatus::Discarded,
        },
        design.as_ref(),
        usage.as_ref(),
        artifact_bundle.as_ref(),
        model_manifest.as_ref(),
        content.as_deref(),
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;

    if status == FinalizeStatus::Success {
        let thread_id = if let Some((_, _, thread_id)) =
            db::get_message_runtime_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
        {
            Some(thread_id)
        } else {
            db::get_message_output_and_thread(&db, &message_id)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .map(|(_, thread_id)| thread_id)
        };

        if let Some(thread_id) = thread_id {
            let title = design
                .as_ref()
                .map(|item| item.title.clone())
                .or_else(|| {
                    response_text.clone().map(|text| {
                        if text.len() > 30 {
                            format!("{}...", &text[..27])
                        } else {
                            text
                        }
                    })
                })
                .unwrap_or_else(|| "Question Session".to_string());
            let _ = persist_thread_summary(&db, &thread_id, &title);

            if design.is_some() || artifact_bundle.is_some() || model_manifest.is_some() {
                let snapshot = build_runtime_snapshot(
                    design,
                    Some(thread_id.clone()),
                    Some(message_id.clone()),
                    artifact_bundle,
                    model_manifest,
                    None,
                );
                {
                    let mut last = state.last_snapshot.lock().unwrap();
                    *last = Some(snapshot.clone());
                }
                write_last_snapshot(&app, Some(&snapshot));
            }
        }
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn classify_intent(
    prompt: String,
    thread_id: Option<String>,
    context: Option<String>,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    state: State<'_, AppState>,
) -> AppResult<IntentDecision> {
    let engine = selected_engine(&state)?;
    let explicit_question_only = crate::is_explicit_question_only_request(&prompt);
    let backend_context = if thread_id.is_some() {
        let ctx = {
            let db = state.db.lock().await;
            crate::context::assemble_context(&db, thread_id, None, None)
        };
        let mut blocks = Vec::new();
        if !ctx.summary.trim().is_empty() {
            blocks.push(format!("THREAD SUMMARY\n{}", ctx.summary));
        }
        if !ctx.recent_dialogue.trim().is_empty() {
            blocks.push(format!("RECENT DIALOGUE\n{}", ctx.recent_dialogue));
        }
        if !ctx.pinned_references.trim().is_empty() {
            blocks.push(format!("PINNED REFERENCES\n{}", ctx.pinned_references));
        }
        if let Some(current) = ctx.last_output.as_ref() {
            let ui_spec_json =
                serde_json::to_string_pretty(&current.ui_spec).unwrap_or_else(|_| "{}".to_string());
            let params_json = serde_json::to_string_pretty(&current.initial_params)
                .unwrap_or_else(|_| "{}".to_string());
            blocks.push(format!(
                "ACTUAL CURRENT FREECAD MACRO (AUTHORITATIVE, NOT A SAMPLE)\n```python\n{}\n```",
                current.macro_code
            ));
            blocks.push(format!(
                "ACTUAL CURRENT UI SPEC (AUTHORITATIVE)\n```json\n{}\n```",
                ui_spec_json
            ));
            blocks.push(format!(
                "ACTUAL CURRENT INITIAL PARAMS (AUTHORITATIVE)\n```json\n{}\n```",
                params_json
            ));
        }
        if let Some(frontend_context) = context.as_ref().filter(|value| !value.trim().is_empty()) {
            blocks.push(format!(
                "ACTUAL LIVE WORKING SNAPSHOT (FRONTEND)\n{}",
                frontend_context
            ));
        }
        Some(blocks.join("\n\n"))
    } else {
        context
    };

    let prompt =
        if let Some(notes) = build_visual_input_notes(image_data.as_ref(), attachments.as_ref()) {
            format!("{}\n\n{}", prompt, notes)
        } else {
            prompt
        };
    let images = prepare_images(image_data, attachments);
    match llm::classify_intent(&engine, &prompt, backend_context.as_deref(), images).await {
        Ok(classification) => {
            let llm::IntentClassification {
                intent,
                confidence,
                response,
                final_response,
            } = classification.data;
            let final_response = if explicit_question_only {
                final_response.clone().or_else(|| Some(response.clone()))
            } else {
                final_response.clone()
            };
            Ok(IntentDecision {
                intent_mode: if explicit_question_only {
                    "question".to_string()
                } else {
                    intent
                },
                confidence,
                response,
                final_response,
                usage: classification.usage,
            })
        }
        Err(_) => Ok(fallback_intent(&prompt)),
    }
}

#[cfg(test)]
mod tests {
    use super::prepend_follow_up_context;

    #[test]
    fn prepend_follow_up_context_is_noop_when_missing() {
        let prompt = "CURRENT DESIGN CONTEXT\n...".to_string();
        let result = prepend_follow_up_context(prompt.clone(), None);
        assert_eq!(result, prompt);
    }

    #[test]
    fn prepend_follow_up_context_adds_authoritative_block() {
        let result = prepend_follow_up_context(
            "CURRENT DESIGN CONTEXT\n...".to_string(),
            Some("Which side?"),
        );
        assert!(result.contains("FOLLOW-UP CONTEXT (AUTHORITATIVE)"));
        assert!(result.contains("\"Which side?\""));
        assert!(result.contains("Do not repeat the same clarification question"));
        assert!(result.ends_with("CURRENT DESIGN CONTEXT\n..."));
    }
}
