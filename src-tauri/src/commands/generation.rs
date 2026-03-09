use base64::{engine::general_purpose, Engine as _};
use serde_json::json;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;
use crate::context::*;
use crate::models::{
    AppState, Attachment, DesignOutput, GenerateOutput, IntentDecision, Message,
};
use crate::{
    db, fallback_intent, llm, persist_thread_summary, persist_user_prompt_references,
    TECHNICAL_SYSTEM_PROMPT,
};
fn prepare_images(image_data: Option<String>, attachments: Option<Vec<Attachment>>) -> Vec<String> {
    let mut images = Vec::new();
    if let Some(main_img) = image_data {
        images.push(main_img);
    }
    if let Some(atts) = attachments {
        for att in atts {
            if att.r#type == "image" {
                if let Ok(bytes) = fs::read(&att.path) {
                    let b64 = general_purpose::STANDARD.encode(bytes);
                    let ext = att
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
                    images.push(format!("data:{};base64,{}", mime, b64));
                }
            }
        }
    }
    images
}
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn generate_design(
    prompt: String,
    thread_id: Option<String>,
    parent_macro_code: Option<String>,
    working_design: Option<DesignOutput>,
    _is_retry: bool,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    question_mode: Option<bool>,
    state: State<'_, AppState>,
    _app: AppHandle,
) -> Result<GenerateOutput, String> {
    let engine = {
        let config = state.config.lock().unwrap();
        config
            .engines
            .iter()
            .find(|e| e.id == config.selected_engine_id)
            .cloned()
    }
    .ok_or("No active engine selected")?;
    if engine.provider != "ollama" && engine.api_key.trim().is_empty() {
        return Err(format!(
            "Selected engine '{}' has no API key configured. Open Configuration, set API key, and save.",
            engine.name
        ));
    }
    let _now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let question_mode = question_mode.unwrap_or(false);
    let ctx = {
        let db = state.db.lock().await;
        crate::context::assemble_context(&db, thread_id, working_design, parent_macro_code)
    };
    let intent_mode = if question_mode {
        "QUESTION_ONLY"
    } else {
        "DESIGN_EDIT"
    };
    let contextual_prompt =
        format_contextual_prompt(&ctx, &prompt, TECHNICAL_SYSTEM_PROMPT, intent_mode);
    let images = prepare_images(image_data, attachments);
    let result: Result<DesignOutput, String> =
        llm::generate_design(&engine, &contextual_prompt, images).await;
    let (_status, content, output): (String, String, Option<DesignOutput>) = match result {
        Ok(mut out) => {
            if question_mode {
                out.interaction_mode = "question".to_string();
                if let Some(previous) = &ctx.last_output {
                    out.title = previous.title.clone();
                    out.version_name = previous.version_name.clone();
                    out.macro_code = previous.macro_code.clone();
                    out.ui_spec = previous.ui_spec.clone();
                    out.initial_params = previous.initial_params.clone();
                }
                if out.version_name.trim().is_empty() {
                    out.version_name = "Q&A".to_string();
                }
                if out.response.trim().is_empty() {
                    out.response = "Question answered. Geometry unchanged.".to_string();
                }
            } else if out.interaction_mode.trim().is_empty() {
                out.interaction_mode = "design".to_string();
            }
            let assistant_text = if out.response.trim().is_empty() {
                "Synthesized design output.".to_string()
            } else {
                out.response.clone()
            };
            ("success".to_string(), assistant_text, Some(out))
        }
        Err(raw_body) => (
            "error".to_string(),
            format!("LLM Response (Unparsed): {}", raw_body),
            None,
        ),
    };
    let assistant_msg_id = Uuid::new_v4().to_string();
    let thread_id_actual = ctx.thread_id.clone();
    if let Some(out) = output {
        Ok(GenerateOutput {
            design: out,
            thread_id: thread_id_actual,
            message_id: assistant_msg_id,
        })
    } else {
        Err(format!("ERR_ID:{}|{}", thread_id_actual, content))
    }
}
#[tauri::command]
pub async fn init_generation_attempt(
    thread_id: String,
    prompt: String,
    attachments: Option<Vec<Attachment>>,
    image_data: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let assistant_msg_id = Uuid::new_v4().to_string();
    let user_msg_id = Uuid::new_v4().to_string();
    {
        let db = state.db.lock().await;
        // Create thread if it doesn't exist (e.g. for "New Thread")
        if db::get_thread_title(&db, &thread_id)
            .unwrap_or(None)
            .is_none()
        {
            let traits = crate::generate_genie_traits();
            let initial_title = if prompt.len() > 30 {
                format!("{}...", &prompt[..27])
            } else {
                prompt.clone()
            };
            db::create_or_update_thread(&db, &thread_id, &initial_title, now, Some(&traits))
                .map_err(|e| e.to_string())?;
        }
        // 1. Add User Message (Prompt)
        let user_msg = Message {
            id: user_msg_id.clone(),
            role: "user".to_string(),
            content: prompt.clone(),
            status: "success".to_string(),
            output: None,
            image_data,
            timestamp: now,
        };
        db::add_message(&db, &thread_id, &user_msg).map_err(|e| e.to_string())?;
        persist_user_prompt_references(
            &db,
            &thread_id,
            &user_msg_id,
            &prompt,
            attachments.as_ref(),
            now,
        )?;
        // 2. Add Pending Assistant Message
        let assistant_msg = Message {
            id: assistant_msg_id.clone(),
            role: "assistant".to_string(),
            content: "Generating...".to_string(),
            status: "pending".to_string(),
            output: None,
            image_data: None,
            timestamp: now + 1,
        };
        db::add_message(&db, &thread_id, &assistant_msg).map_err(|e| e.to_string())?;
    }
    Ok(assistant_msg_id)
}
#[tauri::command]
pub async fn finalize_generation_attempt(
    message_id: String,
    status: String, // "success" | "error" | "discarded"
    design: Option<DesignOutput>,
    error_message: Option<String>,
    response_text: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let db = state.db.lock().await;
    let content = if status == "success" {
        if let Some(d) = &design {
            if d.response.is_empty() {
                Some("Synthesized design output.".to_string())
            } else {
                Some(d.response.clone())
            }
        } else {
            response_text.clone()
        }
    } else {
        error_message.clone()
    };
    db::update_message_status_and_output(
        &db,
        &message_id,
        &status,
        design.as_ref(),
        content.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    // If success, update thread summary/title
    if status == "success" {
        if let Some((_, thread_id)) =
            db::get_message_output_and_thread(&db, &message_id).unwrap_or(None)
        {
            let title = design
                .as_ref()
                .map(|d| d.title.clone())
                .or(response_text.clone().map(|t| {
                    if t.len() > 30 {
                        format!("{}...", &t[..27])
                    } else {
                        t
                    }
                }))
                .unwrap_or_else(|| "Question Session".to_string());
            let _ = persist_thread_summary(&db, &thread_id, &title);
            // Update memory/cache if it was a design success
            if let Some(d) = design {
                let mut last = state.last_design.lock().unwrap();
                *last = Some(d.clone());
                let mut last_tid = state.last_thread_id.lock().unwrap();
                *last_tid = Some(thread_id.clone());
                let cache_path = app
                    .path()
                    .app_config_dir()
                    .unwrap()
                    .join("last_design.json");
                let session_data = json!({
                    "design": d,
                    "thread_id": Some(thread_id)
                });
                if let Ok(json) = serde_json::to_string_pretty(&session_data) {
                    let _ = fs::write(cache_path, json);
                }
            }
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn classify_intent(
    prompt: String,
    thread_id: Option<String>,
    context: Option<String>,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    state: State<'_, AppState>,
) -> Result<IntentDecision, String> {
    let engine = {
        let config = state.config.lock().unwrap();
        config
            .engines
            .iter()
            .find(|e| e.id == config.selected_engine_id)
            .cloned()
    }
    .ok_or("No active engine selected")?;
    if engine.provider != "ollama" && engine.api_key.trim().is_empty() {
        return Err(format!(
            "Selected engine '{}' has no API key configured. Open Configuration, set API key, and save.",
            engine.name
        ));
    }
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
            let ui_spec_json = serde_json::to_string_pretty(&current.ui_spec)
                .unwrap_or_else(|_| "{}".to_string());
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
        if let Some(c) = context.as_ref().filter(|c| !c.trim().is_empty()) {
            blocks.push(format!(
                "ACTUAL LIVE WORKING SNAPSHOT (FRONTEND)\n{}",
                c
            ));
        }
        Some(blocks.join("\n\n"))
    } else {
        context
    };
    let images = prepare_images(image_data, attachments);
    match llm::classify_intent(&engine, &prompt, backend_context.as_deref(), images).await {
        Ok(classification) => Ok(IntentDecision {
            intent_mode: classification.intent,
            confidence: classification.confidence,
            response: classification.response,
        }),
        Err(_) => Ok(fallback_intent(&prompt)),
    }
}
