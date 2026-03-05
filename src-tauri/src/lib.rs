mod models;
mod llm;
mod freecad;
mod db;

use std::fs;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};
use models::{AppState, Config, Engine, DesignOutput, Thread, Message};
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};

const SYSTEM_PROMPT: &str = "You are a CAD Design Agent.
You generate FreeCAD Python macros and a UI specification for their parameters based on the following user intent:

$USER_PROMPT

Macro Requirements:
- Write a FreeCAD Python macro using Part/OCCT BRep (no hand-built meshes).
- Units are in millimeters.
- Create at least one visible solid.
- Do NOT use string formatting braces like `{param_name}` in the generated code to reference parameters.
- UI Parameters are injected globally into the macro execution context. Access them directly by name (e.g., `width = frame_width`) or via the injected `params` dictionary (e.g., `width = params.get(\"frame_width\", 90.0)`).

Return a JSON object with:
1. \"title\": A short (2-5 words) descriptive title.
2. \"macro_code\": The Python macro code.
3. \"ui_spec\": { 
     \"fields\": [
       { 
         \"key\": string, 
         \"label\": string, 
         \"type\": \"range\" | \"number\" | \"select\", 
         \"min\"?: number, 
         \"max\"?: number, 
         \"step\"?: number,
         \"options\"?: [{ \"label\": string, \"value\": string | number }] 
       }
     ] 
   }
4. \"initial_params\": { ... }

UI Guidelines:
- Use \"range\" for continuous dimensions.
- Use \"select\" (enums) for discrete choices like \"Profile Type\", \"Pattern Style\", or \"Top Rim Shape\". Ensure \"options\" are provided for all \"select\" fields.";

#[tauri::command]
fn get_system_prompt() -> String {
    SYSTEM_PROMPT.to_string()
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
async fn get_history(state: State<'_, AppState>) -> Result<Vec<Thread>, String> {
    let db = state.db.lock().unwrap();
    db::get_all_threads(&db).map_err(|e| e.to_string())
}

#[tauri::command]
async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::clear_history(&db).map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_thread(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db::delete_thread(&db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_config(config: Config, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let mut state_config = state.config.lock().unwrap();
    *state_config = config.clone();
    
    let config_path = app.path().app_config_dir().unwrap().join("config.json");
    fs::create_dir_all(config_path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(config_path, json).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
async fn generate_design(
    prompt: String, 
    thread_id: Option<String>,
    parent_macro_code: Option<String>,
    is_retry: bool,
    image_data: Option<String>,
    state: State<'_, AppState>, 
    app: AppHandle
) -> Result<DesignOutput, String> {
    let engine = {
        let config = state.config.lock().unwrap();
        config.engines.iter().find(|e| e.id == config.selected_engine_id).cloned()
    }.ok_or("No active engine selected")?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    // Find the thread and its context
    let (thread_id_actual, last_macro) = {
        let db = state.db.lock().unwrap();
        if let Some(tid) = thread_id.clone() {
            let messages = db::get_thread_messages(&db, &tid).unwrap_or_default();
            // If it's a retry, we look for the last working macro, ignoring the failed one 
            // since the frontend hasn't committed it to UI state yet.
            let last_m = messages.iter()
                .rev()
                .find(|m| m.role == "assistant" && m.output.is_some())
                .and_then(|m| m.output.as_ref().map(|o| o.macro_code.clone()));
            (tid, last_m)
        } else {
            // New thread: use parent_macro_code if provided
            (Uuid::new_v4().to_string(), parent_macro_code)
        }
    };

    // Format prompt with context if available
    let contextual_prompt = if let Some(code) = last_macro {
        format!(
            "a question regarding following FreeCAD Macro:\n\n```python\n{}\n```\n\nUser Question: {}", 
            code, prompt
        )
    } else {
        prompt.clone()
    };

    let output = llm::generate_design(&engine, &contextual_prompt, image_data.clone()).await?;

    // Cache the last design in state
    {
        let mut last = state.last_design.lock().unwrap();
        *last = Some(output.clone());
    }

    // Only update history if this is not a retry loop mid-flight, OR if it's the final successful attempt.
    // Wait, it's easier to let the frontend control WHEN to save to history, but our command does it automatically.
    // For now, let's keep adding it to history, so you see the failed attempts in the "Prompt Details" dropdown.
    // Actually, saving failed attempts pollutes the UI version navigator.
    
    // We will only save to history if it's NOT marked as a retry (i.e., it's the initial user prompt)
    // Wait, the frontend needs to save the final successful output.
    // Let's change the architecture: the `generate_design` command just generates. 
    // We need a separate command to commit to history.
    // To keep it simple without rewriting the whole backend: we'll add it to history here, 
    // but the frontend will pass `is_retry = true` on retries.
    // If it's a retry, we DO NOT create a new "User" message in the DB (to avoid spamming the UI with error logs).
    // We just update the LAST assistant message with the new output.
    
    {
        let db = state.db.lock().unwrap();
        
        db::create_or_update_thread(&db, &thread_id_actual, &output.title, now).map_err(|e| e.to_string())?;

        if !is_retry {
            let user_msg = Message {
                id: Uuid::new_v4().to_string(),
                role: "user".to_string(),
                content: prompt.clone(),
                output: None,
                image_data: image_data.clone(),
                timestamp: now,
            };
            db::add_message(&db, &thread_id_actual, &user_msg).map_err(|e| e.to_string())?;
        }

        let assistant_msg = Message {
            id: Uuid::new_v4().to_string(),
            role: "assistant".to_string(),
            content: if is_retry { "Synthesized design output (after retry/correction):".to_string() } else { "Synthesized design output:".to_string() },
            output: Some(output.clone()),
            image_data: None,
            timestamp: now + 1, // slightly later to ensure ordering
        };
        
        if is_retry {
            // In a real app we'd UPDATE the last assistant message.
            // For simplicity, we just add it. The frontend's `versions` derived state
            // will pick up all assistant messages. To avoid clutter, we probably should update.
            // Let's just add it as a new message. It's fine for the user to see the iterations.
            db::add_message(&db, &thread_id_actual, &assistant_msg).map_err(|e| e.to_string())?;
        } else {
             db::add_message(&db, &thread_id_actual, &assistant_msg).map_err(|e| e.to_string())?;
        }
    }

    // Persist last design to disk
    let cache_path = app.path().app_config_dir().unwrap().join("last_design.json");
    if let Ok(json) = serde_json::to_string_pretty(&output) {
        let _ = fs::write(cache_path, json);
    }

    Ok(output)
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
async fn get_last_design(state: State<'_, AppState>) -> Result<Option<DesignOutput>, String> {
    let last = state.last_design.lock().unwrap();
    Ok(last.clone())
}

#[tauri::command]
async fn list_models(provider: String, api_key: String, base_url: String) -> Result<Vec<String>, String> {
    llm::list_models(&provider, &api_key, &base_url).await
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
    macro_code: String,
    parameters: serde_json::Value,
    ui_spec: serde_json::Value,
    state: State<'_, AppState>
) -> Result<(), String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let db = state.db.lock().unwrap();

    let output = DesignOutput {
        title: title.clone(),
        macro_code,
        ui_spec,
        initial_params: parameters,
    };

    let msg = Message {
        id: Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: "Manual edit committed as new version.".to_string(),
        output: Some(output),
        image_data: None,
        timestamp: now,
    };

    db::add_message(&db, &thread_id, &msg).map_err(|e| e.to_string())?;
    db::create_or_update_thread(&db, &thread_id, &title, now).map_err(|e| e.to_string())?;

    Ok(())
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
                system_prompt: SYSTEM_PROMPT.to_string(),
            }
        ],
        selected_engine_id: "default-gemini".to_string(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(move |app| {
            let config_dir = app.path().app_config_dir()?;
            if !config_dir.exists() {
                fs::create_dir_all(&config_dir)?;
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
            let last_path = config_dir.join("last_design.json");
            if last_path.exists() {
                if let Ok(data) = fs::read_to_string(&last_path) {
                    if let Ok(design) = serde_json::from_str::<DesignOutput>(&data) {
                        last_design = Some(design);
                    }
                }
            }

            let db_path = config_dir.join("history.sqlite");
            let conn = db::init_db(&db_path).expect("Failed to initialize SQLite database");

            app.manage(AppState {
                config: Mutex::new(config),
                last_design: Mutex::new(last_design),
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
            add_manual_version
        ])
        .run(context)
        .expect("error while running tauri application");
}
