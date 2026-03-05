use tauri::{State, AppHandle, Manager};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use std::fs;
use base64::{Engine as _, engine::general_purpose};

use crate::models::{AppState, DesignOutput, Message, Attachment, GenerateOutput, CommitOutput, IntentDecision, QuestionReply};
use crate::context::*;
use crate::{db, llm, persist_user_prompt_references, persist_thread_summary, fallback_intent, TECHNICAL_SYSTEM_PROMPT};

#[tauri::command]
pub async fn generate_design(
    prompt: String, 
    thread_id: Option<String>,
    parent_macro_code: Option<String>,
    working_design: Option<DesignOutput>,
    is_retry: bool,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    question_mode: Option<bool>,
    state: State<'_, AppState>, 
    app: AppHandle
) -> Result<GenerateOutput, String> {
    let engine = {
        let config = state.config.lock().unwrap();
        config.engines.iter().find(|e| e.id == config.selected_engine_id).cloned()
    }.ok_or("No active engine selected")?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let question_mode = question_mode.unwrap_or(false);

    let ctx = {
        let db = state.db.lock().unwrap();
        crate::context::assemble_context(&db, thread_id, working_design, parent_macro_code)
    };

    let intent_mode = if question_mode { "QUESTION_ONLY" } else { "DESIGN_EDIT" };
    let contextual_prompt = format_contextual_prompt(&ctx, &prompt, TECHNICAL_SYSTEM_PROMPT, intent_mode);

    let mut images = Vec::new();
    if let Some(ref main_img) = image_data {
        images.push(main_img.clone());
    }

    if let Some(atts) = &attachments {
        for att in atts {
            if att.r#type == "image" {
                if let Ok(bytes) = fs::read(&att.path) {
                    let b64 = general_purpose::STANDARD.encode(bytes);
                    let ext = att.path.split('.').last().unwrap_or("png").to_lowercase();
                    let mime = if ext == "jpg" || ext == "jpeg" { "image/jpeg" } else { "image/png" };
                    images.push(format!("data:{};base64,{}", mime, b64));
                }
            }
        }
    }

    let result: Result<DesignOutput, String> = llm::generate_design(&engine, &contextual_prompt, images).await;

    let (status, content, output): (String, String, Option<DesignOutput>) = match result {
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
        },
        Err(raw_body) => ("error".to_string(), format!("LLM Response (Unparsed): {}", raw_body), None)
    };

    let assistant_msg_id = Uuid::new_v4().to_string();
    let thread_id_actual = ctx.thread_id.clone();

    if let Some(out) = output {
        Ok(GenerateOutput { design: out, thread_id: thread_id_actual, message_id: assistant_msg_id })
    } else {
        Err(format!("ERR_ID:{}|{}", thread_id_actual, content))
    }
}

#[tauri::command]
pub async fn commit_generated_version(
    thread_id: String,
    prompt: String,
    design: DesignOutput,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<CommitOutput, String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let thread_title = design.title.clone();
    let assistant_text = if design.response.trim().is_empty() {
        "Synthesized design output.".to_string()
    } else {
        design.response.clone()
    };

    let user_msg = Message {
        id: Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: prompt.clone(),
        status: "success".to_string(),
        output: None,
        image_data: image_data.clone(),
        timestamp: now,
    };
    let assistant_msg = Message {
        id: Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: assistant_text,
        status: "success".to_string(),
        output: Some(design.clone()),
        image_data: None,
        timestamp: now + 1,
    };

    {
        let db = state.db.lock().unwrap();
        let thread_traits = if db::get_thread_title(&db, &thread_id).unwrap_or(None).is_none() {
            Some(crate::generate_genie_traits())
        } else {
            None
        };
        db::create_or_update_thread(&db, &thread_id, &thread_title, now, thread_traits.as_ref()).map_err(|e: rusqlite::Error| e.to_string())?;
        db::add_message(&db, &thread_id, &user_msg).map_err(|e: rusqlite::Error| e.to_string())?;
        persist_user_prompt_references(&db, &thread_id, &user_msg.id, &prompt, attachments.as_ref(), now)?;
        db::add_message(&db, &thread_id, &assistant_msg).map_err(|e: rusqlite::Error| e.to_string())?;
        let _ = persist_thread_summary(&db, &thread_id, &thread_title);
    }

    {
        let mut last = state.last_design.lock().unwrap();
        *last = Some(design.clone());
        let mut last_tid = state.last_thread_id.lock().unwrap();
        *last_tid = Some(thread_id.clone());
    }

    let cache_path = app.path().app_config_dir().unwrap().join("last_design.json");
    let session_data = json!({
        "design": design,
        "thread_id": Some(thread_id.clone())
    });
    if let Ok(json) = serde_json::to_string_pretty(&session_data) {
        let _ = fs::write(cache_path, json);
    }

    Ok(CommitOutput {
        thread_id,
        message_id: assistant_msg.id,
    })
}

#[tauri::command]
pub async fn classify_intent(
    prompt: String,
    thread_id: Option<String>,
    context: Option<String>,
    state: State<'_, AppState>
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

    let backend_context = if thread_id.is_some() {
        let ctx = {
            let db = state.db.lock().unwrap();
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
        if let Some(c) = context.as_ref().filter(|c| !c.trim().is_empty()) {
            blocks.push(format!("CURRENT LIVE SNAPSHOT\n{}", c));
        }
        Some(blocks.join("\n\n"))
    } else {
        context
    };

    match llm::classify_intent(&engine, &prompt, backend_context.as_deref()).await {
        Ok(classification) => Ok(IntentDecision {
            intent_mode: classification.intent,
            confidence: classification.confidence,
            response: classification.response,
        }),
        Err(_) => Ok(fallback_intent(&prompt)),
    }
}

#[tauri::command]
pub async fn answer_question_light(
    prompt: String,
    response: String,
    thread_id: Option<String>,
    title_hint: Option<String>,
    image_data: Option<String>,
    attachments: Option<Vec<Attachment>>,
    state: State<'_, AppState>
) -> Result<QuestionReply, String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let thread_id_actual = thread_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    {
        let db = state.db.lock().unwrap();
        let existing_title = db::get_thread_title(&db, &thread_id_actual).map_err(|e| e.to_string())?;
        let thread_title = existing_title
            .or(title_hint)
            .unwrap_or_else(|| "Question Session".to_string());

        let thread_traits = if db::get_thread_title(&db, &thread_id_actual).unwrap_or(None).is_none() {
            Some(crate::generate_genie_traits())
        } else {
            None
        };
        db::create_or_update_thread(&db, &thread_id_actual, &thread_title, now, thread_traits.as_ref()).map_err(|e| e.to_string())?;

        let user_msg = Message {
            id: Uuid::new_v4().to_string(),
            role: "user".to_string(),
            content: prompt.clone(),
            status: "success".to_string(),
            output: None,
            image_data: image_data.clone(),
            timestamp: now,
        };
        db::add_message(&db, &thread_id_actual, &user_msg).map_err(|e| e.to_string())?;
        persist_user_prompt_references(&db, &thread_id_actual, &user_msg.id, &prompt, attachments.as_ref(), now)?;

        let assistant_msg = Message {
            id: Uuid::new_v4().to_string(),
            role: "assistant".to_string(),
            content: response.clone(),
            status: "success".to_string(),
            output: None,
            image_data: None,
            timestamp: now + 1,
        };
        db::add_message(&db, &thread_id_actual, &assistant_msg).map_err(|e| e.to_string())?;
        let _ = persist_thread_summary(&db, &thread_id_actual, &thread_title);
    }

    {
        let mut last_tid = state.last_thread_id.lock().unwrap();
        *last_tid = Some(thread_id_actual.clone());
    }

    Ok(QuestionReply {
        thread_id: thread_id_actual,
        response,
    })
}
