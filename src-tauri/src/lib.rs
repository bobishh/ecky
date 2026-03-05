pub mod models;
pub mod db;
pub mod llm;
pub mod freecad;

use tauri::{State, AppHandle, Manager};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use std::fs;
use std::sync::Mutex;
use base64::{Engine as _, engine::general_purpose};

use crate::models::{AppState, Config, Engine, DesignOutput, Message};

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
async fn save_config(config: Config, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let config_dir = app.path().app_config_dir().unwrap();
    let config_path = config_dir.join("config.json");
    
    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(config_path, data).map_err(|e| e.to_string())?;
    
    let mut state_config = state.config.lock().unwrap();
    *state_config = config;
    Ok(())
}

#[tauri::command]
async fn get_history(state: State<'_, AppState>) -> Result<Vec<crate::models::Thread>, String> {
    let db = state.db.lock().unwrap();
    db::get_all_threads(&db).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::clear_history(&db).map_err(|e: rusqlite::Error| e.to_string())
}

#[tauri::command]
async fn delete_thread(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::delete_thread(&db, &id).map_err(|e: rusqlite::Error| e.to_string())
}

#[derive(serde::Serialize)]
struct GenerateOutput {
    design: DesignOutput,
    thread_id: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Attachment {
    pub path: String,
    pub name: String,
    pub explanation: String,
    pub r#type: String, // "image" or "cad"
}

#[tauri::command]
async fn generate_design(
    prompt: String, 
    thread_id: Option<String>,
    parent_macro_code: Option<String>,
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

    // Find the thread and its latest design context
    let (thread_id_actual, last_output) = {
        let db = state.db.lock().unwrap();
        if let Some(tid) = thread_id.clone() {
            let messages = db::get_thread_messages(&db, &tid).unwrap_or_default();
            let last_o = messages.iter()
                .rev()
                .find(|m| m.role == "assistant" && m.output.is_some())
                .and_then(|m| m.output.clone());
            (tid, last_o)
        } else {
            let fallback_output = parent_macro_code.map(|code| DesignOutput {
                title: "Untitled Design".to_string(),
                version_name: "V1".to_string(),
                response: String::new(),
                interaction_mode: "design".to_string(),
                macro_code: code,
                ui_spec: json!({ "fields": [] }),
                initial_params: json!({}),
            });
            (Uuid::new_v4().to_string(), fallback_output)
        }
    };

    // Construct technical context with attachments
    let mut full_prompt = prompt.clone();
    
    if let Some(atts) = &attachments {
        if !atts.is_empty() {
            full_prompt.push_str("\n\nUser provided additional context/attachments:");
            for att in atts {
                full_prompt.push_str(&format!("\n- Attachment: {} (Type: {}, Purpose: {})", att.name, att.r#type, att.explanation));
            }
        }
    }

    full_prompt = format!(
        "{}\n\n{}\n\nUSER_INTENT_MODE: {}",
        full_prompt,
        TECHNICAL_SYSTEM_PROMPT,
        if question_mode { "QUESTION_ONLY" } else { "DESIGN_EDIT" }
    );

    let contextual_prompt = if let Some(previous) = &last_output {
        let ui_spec_json = serde_json::to_string_pretty(&previous.ui_spec).unwrap_or_else(|_| "{}".to_string());
        let params_json = serde_json::to_string_pretty(&previous.initial_params).unwrap_or_else(|_| "{}".to_string());
        format!(
            "CURRENT DESIGN CONTEXT
Title: {}
Version: {}

Current FreeCAD Macro:
```python
{}
```

Current UI Spec:
```json
{}
```

Current Initial Params:
```json
{}
```

USER REQUEST:
{}",
            previous.title,
            previous.version_name,
            previous.macro_code,
            ui_spec_json,
            params_json,
            full_prompt
        )
    } else {
        full_prompt
    };

    // NOTE: In a more advanced version, we would also send CAD metadata 
    // from the attachment paths to multimodal LLMs.
    // For now, we provide the metadata/explanation and all provided images.

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
                if let Some(previous) = &last_output {
                    // Keep geometry state stable when user is asking about the existing model.
                    out.title = previous.title.clone();
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

    // DB update
    {
        let db = state.db.lock().unwrap();
        let thread_title = output.as_ref().map(|o| o.title.clone()).unwrap_or_else(|| "Failed Design Attempt".to_string());
        db::create_or_update_thread(&db, &thread_id_actual, &thread_title, now).map_err(|e: rusqlite::Error| e.to_string())?;

        if !is_retry {
            let user_msg = Message {
                id: Uuid::new_v4().to_string(),
                role: "user".to_string(),
                content: prompt.clone(),
                status: "success".to_string(),
                output: None,
                image_data: image_data.clone(),
                timestamp: now,
            };
            db::add_message(&db, &thread_id_actual, &user_msg).map_err(|e: rusqlite::Error| e.to_string())?;
        }

        let assistant_msg = Message {
            id: Uuid::new_v4().to_string(),
            role: "assistant".to_string(),
            content: content.clone(),
            status: status.clone(),
            output: output.clone(),
            image_data: None,
            timestamp: now + 1,
        };
        db::add_message(&db, &thread_id_actual, &assistant_msg).map_err(|e: rusqlite::Error| e.to_string())?;
    }

    if let Some(out) = output {
        let mut last = state.last_design.lock().unwrap();
        *last = Some(out.clone());
        let mut last_tid = state.last_thread_id.lock().unwrap();
        *last_tid = Some(thread_id_actual.clone());

        let cache_path = app.path().app_config_dir().unwrap().join("last_design.json");
        let session_data = json!({
            "design": out,
            "thread_id": Some(thread_id_actual.clone())
        });
        if let Ok(json) = serde_json::to_string_pretty(&session_data) {
            let _ = fs::write(cache_path, json);
        }
        Ok(GenerateOutput { design: out, thread_id: thread_id_actual })
    } else {
        // Return thread_id even on error so frontend can stay in context
        Err(format!("ERR_ID:{}|{}", thread_id_actual, content))
    }
}

#[tauri::command]
async fn render_stl(macro_code: String, parameters: serde_json::Value, app: AppHandle) -> Result<String, String> {
    freecad::render(&macro_code, &parameters, &app)
}

#[tauri::command]
async fn get_default_macro(app: AppHandle) -> Result<String, String> {
    freecad::get_default_macro(&app)
}

#[tauri::command]
async fn get_last_design(state: State<'_, AppState>) -> Result<Option<(DesignOutput, Option<String>)>, String> {
    let last = state.last_design.lock().unwrap();
    let thread_id = state.last_thread_id.lock().unwrap();
    Ok(last.as_ref().map(|d| (d.clone(), thread_id.clone())))
}

#[tauri::command]
async fn get_system_prompt() -> Result<String, String> {
    Ok(DEFAULT_PROMPT.to_string())
}

#[tauri::command]
async fn list_models(provider: String, api_key: String, base_url: String) -> Result<Vec<String>, String> {
    llm::list_models(&provider, &api_key, &base_url).await
}

#[tauri::command]
async fn update_ui_spec(message_id: String, ui_spec: serde_json::Value, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::update_message_ui_spec(&db, &message_id, &ui_spec).map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_parameters(message_id: String, parameters: serde_json::Value, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::update_message_parameters(&db, &message_id, &parameters).map_err(|e| e.to_string())
}

#[tauri::command]
async fn export_file(source_path: String, target_path: String) -> Result<(), String> {
    fs::copy(&source_path, &target_path).map_err(|e| format!("Failed to export file: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn add_manual_version(
    thread_id: String,
    title: String,
    version_name: String,
    macro_code: String,
    parameters: serde_json::Value,
    ui_spec: serde_json::Value,
    state: State<'_, AppState>
) -> Result<(), String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let db = state.db.lock().unwrap();

    let output = DesignOutput {
        title: title.clone(),
        version_name,
        response: "Manual edit committed as new version.".to_string(),
        interaction_mode: "design".to_string(),
        macro_code,
        ui_spec,
        initial_params: parameters,
    };

    let msg = Message {
        id: Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: "Manual edit committed as new version.".to_string(),
        status: "success".to_string(),
        output: Some(output),
        image_data: None,
        timestamp: now,
    };

    db::add_message(&db, &thread_id, &msg).map_err(|e: rusqlite::Error| e.to_string())?;
    db::create_or_update_thread(&db, &thread_id, &title, now).map_err(|e: rusqlite::Error| e.to_string())?;

    Ok(())
}

const DEFAULT_PROMPT: &str = r#"You are a CAD Design Agent.
You generate FreeCAD Python macros and a UI specification for their parameters based on the following user intent:

$USER_PROMPT

Macro Requirements:
- Write a FreeCAD Python macro using Part/OCCT BRep (no hand-built meshes).
- Units are in millimeters.
- Create at least one visible solid.
- Do NOT use string formatting braces like `{param_name}` in the generated code to reference parameters.
- UI Parameters are injected globally into the macro execution context. Access them directly by name (e.g., `width = frame_width`) or via the injected `params` dictionary (e.g., `width = params.get("frame_width", 90.0)`).

Return a JSON object with:
1. "title": A short (2-5 words) descriptive title.
2. "macro_code": The Python macro code.
3. "ui_spec": { 
     "fields": [
       { 
         "key": string, 
         "label": string, 
         "type": "range" | "number" | "select" | "checkbox", 
         "min"?: number, 
         "max"?: number, 
         "step"?: number,
         "options"?: [{ "label": string, "value": string | number }] 
       }
     ] 
   }
4. "initial_params": { ... }

UI Guidelines:
- Use "range" for continuous dimensions.
- Use "select" (enums) for discrete choices. Ensure "options" are provided.
- Use "checkbox" for boolean flags (e.g., "Show Holes"). Value will be true or false.
"#;

const TECHNICAL_SYSTEM_PROMPT: &str = r#"Return a JSON object with:
1. "title": 2-5 words project title.
2. "version_name": Short descriptive name for this iteration.
3. "response": short end-user text for the advisor speech bubble (1-3 concise sentences).
4. "interaction_mode": "design" or "question".
5. "macro_code": FreeCAD Python code.
6. "ui_spec": { "fields": [ { "key": string, "label": string, "type": "range"|"number"|"select"|"checkbox" } ] }
7. "initial_params": { "key": value }

CRITICAL RULES:
- UNITS: ALL dimensions are in MILLIMETERS (mm).
- UI: Focus on 'key', 'label' and 'type'. Don't worry about 'min'/'max' for ranges; the system will calculate bounds based on your 'initial_params'.
- PARAMETERS: Access parameters directly by name (e.g. `L = connector_length`) or via `params.get("key", default)`.
- NO BRACES: NEVER use `{var}` style interpolation inside the macro_code string.
- If USER_INTENT_MODE is "QUESTION_ONLY":
  - Set "interaction_mode" to "question".
  - Use "response" to explain the current design/code.
  - Keep "macro_code", "ui_spec", and "initial_params" aligned with the existing design context unless the user explicitly asks to modify geometry.
- If USER_INTENT_MODE is "DESIGN_EDIT":
  - Set "interaction_mode" to "design".
  - Use "response" as a short summary of what changed.
"#;

#[tauri::command]
async fn upload_asset(
    source_path: String,
    name: String,
    format: String,
    app: AppHandle
) -> Result<crate::models::Asset, String> {
    let app_data_dir = app.path().app_data_dir().unwrap();
    let assets_dir = app_data_dir.join("assets");
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir).map_err(|e| e.to_string())?;
    }

    let id = Uuid::new_v4().to_string();
    let file_name = format!("{}.{}", id, format.to_lowercase());
    let target_path = assets_dir.join(&file_name);

    fs::copy(&source_path, &target_path).map_err(|e| e.to_string())?;

    Ok(crate::models::Asset {
        id,
        name,
        path: target_path.to_string_lossy().to_string(),
        format,
    })
}

#[tauri::command]
async fn save_recorded_audio(
    base64_data: String,
    name: String,
    app: AppHandle
) -> Result<crate::models::Asset, String> {
    let app_data_dir = app.path().app_data_dir().unwrap();
    let assets_dir = app_data_dir.join("assets");
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir).map_err(|e| e.to_string())?;
    }

    let id = Uuid::new_v4().to_string();
    let file_name = format!("{}.webm", id); // MediaRecorder typically outputs webm/opus
    let target_path = assets_dir.join(&file_name);

    let bytes = general_purpose::STANDARD.decode(base64_data).map_err(|e| e.to_string())?;
    fs::write(&target_path, bytes).map_err(|e| e.to_string())?;

    Ok(crate::models::Asset {
        id,
        name,
        path: target_path.to_string_lossy().to_string(),
        format: "WEBM".to_string(),
    })
}

pub fn run() {
    let context = tauri::generate_context!();
    
    let default_config = Config {
        engines: vec![
            Engine {
                id: "default-gemini".to_string(),
                name: "Google Gemini".to_string(),
                provider: "gemini".to_string(),
                api_key: "".to_string(),
                model: "gemini-2.0-flash".to_string(),
                base_url: "".to_string(),
                system_prompt: DEFAULT_PROMPT.to_string(),
            }
        ],
        selected_engine_id: "default-gemini".to_string(),
        assets: vec![],
        microwave: None,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(move |app| {
            let config_dir = app.path().app_config_dir()?;
            let app_data_dir = app.path().app_data_dir()?;
            if !config_dir.exists() {
                fs::create_dir_all(&config_dir)?;
            }
            if !app_data_dir.exists() {
                fs::create_dir_all(&app_data_dir)?;
            }

            let mut config = default_config;
            let config_path = config_dir.join("config.json");
            if config_path.exists() {
                if let Ok(data) = fs::read_to_string(&config_path) {
                    if let Ok(c) = serde_json::from_str::<Config>(&data) {
                        config = c;
                    }
                }
            }

            let mut last_design = None;
            let mut last_thread_id = None;
            let last_path = config_dir.join("last_design.json");
            if last_path.exists() {
                if let Ok(data) = fs::read_to_string(&last_path) {
                    #[derive(serde::Deserialize)]
                    struct LastSession {
                        design: DesignOutput,
                        thread_id: Option<String>,
                    }
                    if let Ok(session) = serde_json::from_str::<LastSession>(&data) {
                        last_design = Some(session.design);
                        last_thread_id = session.thread_id;
                    } else if let Ok(design) = serde_json::from_str::<DesignOutput>(&data) {
                        // fallback for old format
                        last_design = Some(design);
                    }
                }
            }

            let db_path = config_dir.join("history.sqlite");
            let conn = db::init_db(&db_path).expect("Failed to initialize SQLite database");

            app.manage(AppState {
                config: Mutex::new(config),
                last_design: Mutex::new(last_design),
                last_thread_id: Mutex::new(last_thread_id),
                db: Mutex::new(conn),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_history,
            clear_history,
            delete_thread,
            generate_design,
            render_stl,
            list_models,
            get_default_macro,
            get_last_design,
            get_system_prompt,
            export_file,
            add_manual_version,
            update_ui_spec,
            update_parameters,
            upload_asset,
            save_recorded_audio
        ])
        .run(context)
        .expect("error while running tauri application");
}
