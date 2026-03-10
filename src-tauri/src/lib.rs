#![allow(unexpected_cfgs)]

pub mod bindings;
pub mod commands;
pub mod context;
pub mod contracts;
pub mod db;
pub mod freecad;
pub mod llm;
pub mod models;

use std::fs;
use std::sync::Mutex;
use tauri::Manager;
use uuid::Uuid;

use crate::context::*;
use crate::models::{
    AppState, Attachment, DesignOutput, GenieTraits, LastDesignSnapshot, ThreadReference,
};

use rand::Rng;

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn set_macos_process_name(name: &str) {
    use cocoa::base::{id, nil};
    use cocoa::foundation::{NSAutoreleasePool, NSString};
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let process_info: id = msg_send![class!(NSProcessInfo), processInfo];
        let ns_name = NSString::alloc(nil).init_str(name);
        let _: () = msg_send![process_info, setProcessName: ns_name];
    }
}

#[cfg(not(target_os = "macos"))]
fn set_macos_process_name(_name: &str) {}

pub fn generate_genie_traits() -> GenieTraits {
    let mut rng = rand::thread_rng();
    GenieTraits::from_seed(rng.gen::<u32>())
}

pub(crate) fn extract_code_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut cursor = text;
    while let Some(start) = cursor.find("```") {
        let after_ticks = &cursor[start + 3..];
        let Some(end) = after_ticks.find("```") else {
            break;
        };
        let block = &after_ticks[..end];
        let normalized = if let Some(newline) = block.find('\n') {
            let first_line = block[..newline].trim().to_lowercase();
            let rest = block[newline + 1..].trim();
            if first_line.is_empty() || first_line.contains("python") || first_line.contains("py") {
                rest.to_string()
            } else {
                block.trim().to_string()
            }
        } else {
            block.trim().to_string()
        };
        if !normalized.is_empty() {
            blocks.push(normalized);
        }
        cursor = &after_ticks[end + 3..];
    }
    blocks
}

pub(crate) fn looks_like_python_macro(text: &str) -> bool {
    let lowered = text.to_lowercase();
    let signal_count = [
        "import freecad",
        "import part",
        "app.activedocument",
        "app.newdocument",
        "params.get(",
        "doc.recompute(",
        "part::feature",
        "part.make",
        "vector(",
        "placemen",
    ]
    .iter()
    .filter(|needle| lowered.contains(**needle))
    .count();
    signal_count >= 2 || (lowered.contains("import ") && lowered.contains("if doc is none"))
}

const PINNED_REFERENCE_SUMMARY_MAX_CHARS: usize = 200;
const PINNED_REFERENCE_CONTENT_MAX_CHARS: usize = 2200;

pub(crate) fn summarize_reference(kind: &str, name: &str, content: &str) -> String {
    let intro = match kind {
        "python_macro" => "Python macro reference",
        "attachment" => "Attachment reference",
        _ => "Reference",
    };
    let first_line = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    if first_line.is_empty() {
        compact_text(
            &format!("{}: {}", intro, name),
            PINNED_REFERENCE_SUMMARY_MAX_CHARS,
        )
    } else {
        compact_text(
            &format!("{} [{}]: {}", intro, name, first_line.trim()),
            PINNED_REFERENCE_SUMMARY_MAX_CHARS,
        )
    }
}

fn extract_prompt_references(
    thread_id: &str,
    message_id: &str,
    prompt: &str,
    created_at: u64,
) -> Vec<ThreadReference> {
    let mut refs = Vec::new();
    let code_blocks = extract_code_blocks(prompt);
    if !code_blocks.is_empty() {
        for (idx, block) in code_blocks.into_iter().enumerate() {
            if looks_like_python_macro(&block) {
                refs.push(ThreadReference {
                    id: Uuid::new_v4().to_string(),
                    thread_id: thread_id.to_string(),
                    source_message_id: Some(message_id.to_string()),
                    ordinal: idx as i64,
                    kind: "python_macro".to_string(),
                    name: format!("prompt_macro_{}", idx + 1),
                    content: compact_text(&block, PINNED_REFERENCE_CONTENT_MAX_CHARS),
                    summary: summarize_reference(
                        "python_macro",
                        &format!("prompt_macro_{}", idx + 1),
                        &block,
                    ),
                    pinned: true,
                    created_at,
                });
            }
        }
    } else if looks_like_python_macro(prompt) {
        refs.push(ThreadReference {
            id: Uuid::new_v4().to_string(),
            thread_id: thread_id.to_string(),
            source_message_id: Some(message_id.to_string()),
            ordinal: 0,
            kind: "python_macro".to_string(),
            name: "prompt_macro_1".to_string(),
            content: compact_text(prompt, PINNED_REFERENCE_CONTENT_MAX_CHARS),
            summary: summarize_reference("python_macro", "prompt_macro_1", prompt),
            pinned: true,
            created_at,
        });
    }
    refs
}

pub(crate) fn persist_user_prompt_references(
    conn: &rusqlite::Connection,
    thread_id: &str,
    message_id: &str,
    prompt: &str,
    attachments: Option<&Vec<Attachment>>,
    created_at: u64,
) -> Result<(), String> {
    for reference in extract_prompt_references(thread_id, message_id, prompt, created_at) {
        db::add_thread_reference(conn, &reference).map_err(|e| e.to_string())?;
    }

    if let Some(attachments) = attachments {
        let mut ordinal_offset = 100;
        for attachment in attachments {
            let ext = attachment
                .path
                .split('.')
                .next_back()
                .unwrap_or("png")
                .to_lowercase();
            let is_python = matches!(ext.as_str(), "py" | "fcmacro");
            let content = if is_python {
                fs::read_to_string(&attachment.path).unwrap_or_default()
            } else {
                String::new()
            };
            let summary = compact_text(
                &format!(
                    "{} attachment [{}]: {}",
                    if is_python {
                        "Python macro"
                    } else {
                        "External"
                    },
                    attachment.name,
                    attachment.explanation
                ),
                PINNED_REFERENCE_SUMMARY_MAX_CHARS,
            );
            let reference = ThreadReference {
                id: Uuid::new_v4().to_string(),
                thread_id: thread_id.to_string(),
                source_message_id: Some(message_id.to_string()),
                ordinal: ordinal_offset,
                kind: if is_python {
                    "python_macro".to_string()
                } else {
                    "attachment".to_string()
                },
                name: attachment.name.clone(),
                content: compact_text(&content, PINNED_REFERENCE_CONTENT_MAX_CHARS),
                summary,
                pinned: true,
                created_at,
            };
            db::add_thread_reference(conn, &reference).map_err(|e| e.to_string())?;
            ordinal_offset += 1;
        }
    }

    Ok(())
}

fn migrate_legacy_references(conn: &rusqlite::Connection) -> Result<(), String> {
    let threads = db::get_all_threads(conn).map_err(|e| e.to_string())?;
    for thread in threads {
        for message in thread
            .messages
            .iter()
            .filter(|m| m.role == crate::models::MessageRole::User)
        {
            persist_user_prompt_references(
                conn,
                &thread.id,
                &message.id,
                &message.content,
                None,
                message.timestamp,
            )?;
        }
        if !thread.summary.trim().is_empty() {
            continue;
        }
        let summary = build_thread_summary(&thread.title, &thread.messages);
        db::update_thread_summary(conn, &thread.id, &summary).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn persist_thread_summary(
    conn: &rusqlite::Connection,
    thread_id: &str,
    title: &str,
) -> Result<String, String> {
    let messages =
        db::get_thread_messages_for_context(conn, thread_id).map_err(|e| e.to_string())?;
    let summary = build_thread_summary(title, &messages);
    db::update_thread_summary(conn, thread_id, &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

pub(crate) fn is_explicit_question_only_request(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    p.starts_with("/ask ")
        || [
            "answer only",
            "just answer",
            "only answer",
            "do not generate",
            "don't generate",
            "without generating",
            "no generation",
            "do not change the model",
            "don't change the model",
            "without changing the model",
            "только ответь",
            "только ответ",
            "просто ответь",
            "без генерации",
            "не генерируй",
            "не меняй модель",
            "не трогай модель",
        ]
        .iter()
        .any(|marker| p.contains(marker))
}

pub(crate) fn fallback_intent(prompt: &str) -> models::IntentDecision {
    let p = prompt.to_lowercase();
    if is_explicit_question_only_request(prompt) {
        return models::IntentDecision {
            intent_mode: "question".to_string(),
            confidence: 0.95,
            response: "Answering the question without generating geometry.".to_string(),
            usage: None,
        };
    }
    let has_question_signal = p.contains('?')
        || p.contains("explain")
        || p.contains("why")
        || p.contains("how")
        || p.contains("what");
    let has_design_signal = p.contains("generate")
        || p.contains("create")
        || p.contains("make")
        || p.contains("add")
        || p.contains("remove")
        || p.contains("change")
        || p.contains("update")
        || p.contains("set")
        || p.contains("resize")
        || p.contains("connector")
        || p.contains("diameter");

    if has_question_signal && !has_design_signal {
        models::IntentDecision {
            intent_mode: "question".to_string(),
            confidence: 0.55,
            response: "Thinking not deep enough. This looks like a question.".to_string(),
            usage: None,
        }
    } else {
        models::IntentDecision {
            intent_mode: "design".to_string(),
            confidence: 0.55,
            response: "This looks like a geometry change request.".to_string(),
            usage: None,
        }
    }
}

pub const DEFAULT_PROMPT: &str = r#"You are a CAD Design Agent.
You generate FreeCAD Python macros and a UI specification for their parameters based on the following user intent:

$USER_PROMPT

Macro Requirements:
- Write a FreeCAD Python macro using Part/OCCT BRep (no hand-built meshes).
- Units are in millimeters.
- Create at least one visible solid.
- Do NOT use string formatting braces like `{param_name}` in the generated code to reference parameters.
- UI Parameters are injected globally into the macro execution context. Access them directly by name (e.g., `width = frame_width`) or via the injected `params` dictionary (e.g., `width = params.get("frame_width", 90.0)`).
- Prefer print-friendly geometry for common 3D printing workflows (FDM/SLA): avoid non-manifold solids, inaccessible trapped volumes, fragile tiny features, and extreme unsupported overhangs unless explicitly requested.
- Keep practical wall thickness and clearances when dimensions permit.

Return a JSON object with:
1. "title": A short (2-5 words) descriptive title.
2. "version_name": Short descriptive name for this iteration.
3. "response": short end-user text for Ecky's speech bubble (1-4 concise sentences). If there are 3D printing risks, add a separate final sentence starting with `PRINTING RISKS:`.
4. "interaction_mode": "design" or "question".
5. "macro_code": The Python macro code.
6. "ui_spec": { 
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
7. "initial_params": { ... }

UI Guidelines:
- Use "range" for continuous dimensions.
- Use "select" (enums) for discrete choices. Ensure "options" are provided.
- Use "checkbox" for boolean flags (e.g., "Show Holes"). Value will be true or false.
"#;

pub(crate) const TECHNICAL_SYSTEM_PROMPT: &str = r#"Return a JSON object with:
1. "title": 2-5 words project title.
2. "version_name": Short descriptive name for this iteration.
3. "response": short end-user text for the advisor speech bubble (1-3 concise sentences).
4. "interaction_mode": "design" or "question".
5. "macro_code": FreeCAD Python code.
6. "ui_spec": { "fields": [ { "key": string, "label": string, "type": "range"|"number"|"select"|"checkbox" } ] }
7. "initial_params": { "key": value }

CRITICAL RULES:
- UNITS: ALL dimensions are in MILLIMETERS (mm).
- CONTEXT PRIORITY: Any section labeled "ACTUAL CURRENT ... (AUTHORITATIVE)" is the real current state. Treat it as source of truth, not an example/template.
- UI: Focus on 'key', 'label' and 'type'. 
  - Use 'number' for all numeric parameters. NEVER use 'range'.
  - Use 'min_from' and 'max_from' keys in the 'ui_spec' fields to link parameter boundaries to other keys (e.g., inner_radius max_from outer_radius).
  - Ensure geometry stays sane and valid across all parameter permutations.
- PARAMETERS: Access parameters directly by name (e.g. `L = connector_length`) or via `params.get("key", default)`.
- NO BRACES: NEVER use `{var}` style interpolation inside the macro_code string.
- CLEANUP: You MUST remove any parameters from "ui_spec" and "initial_params" that are no longer used in the current "macro_code". Do not accumulate parameters from previous designs.
- PRINTABILITY: Prefer geometry that is straightforward to 3D print (manifold solids, reasonable wall thickness, avoid fragile or unsupported details unless requested).
- PRINTABILITY REPORTING: If printability risks remain, mention them explicitly at the end of "response" as a separate sentence prefixed with `PRINTING RISKS:`.
- If USER_INTENT_MODE is "QUESTION_ONLY":
  - Set "interaction_mode" to "question".
  - Use "response" to explain the current design/code.
  - Keep "macro_code", "ui_spec", and "initial_params" aligned with the existing design context unless the user explicitly asks to modify geometry.
- If USER_INTENT_MODE is "DESIGN_EDIT":
  - Set "interaction_mode" to "design".
  - Use "response" as a short summary of what changed.
"#;

pub fn run() {
    set_macos_process_name("Ecky CAD");
    let context = tauri::generate_context!();
    let builder = crate::bindings::builder();

    let default_config = crate::models::Config {
        engines: vec![crate::models::Engine {
            id: "default-gemini".to_string(),
            name: "Google Gemini".to_string(),
            provider: "gemini".to_string(),
            api_key: "".to_string(),
            model: "gemini-2.0-flash".to_string(),
            light_model: "gemini-2.0-flash-lite".to_string(),
            base_url: "".to_string(),
            system_prompt: DEFAULT_PROMPT.to_string(),
        }],
        selected_engine_id: "default-gemini".to_string(),
        freecad_cmd: String::new(),
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
                    if let Ok(c) = serde_json::from_str::<crate::models::Config>(&data) {
                        config = c;
                    }
                }
            }
            let mut should_persist_config = false;
            for engine in config.engines.iter_mut() {
                let prompt = engine.system_prompt.trim();
                if prompt.is_empty() || prompt == "You are a CAD expert." {
                    engine.system_prompt = DEFAULT_PROMPT.to_string();
                    should_persist_config = true;
                }
            }
            if should_persist_config {
                if let Ok(data) = serde_json::to_string_pretty(&config) {
                    if let Err(err) = fs::write(&config_path, data) {
                        eprintln!("Failed to persist migrated config prompts: {}", err);
                    }
                }
            }

            let mut last_snapshot = None;
            let last_path = config_dir.join("last_design.json");
            if last_path.exists() {
                if let Ok(data) = fs::read_to_string(&last_path) {
                    if let Ok(session) = serde_json::from_str::<LastDesignSnapshot>(&data) {
                        last_snapshot = Some(session);
                    } else if let Ok(design) = serde_json::from_str::<DesignOutput>(&data) {
                        last_snapshot = Some(LastDesignSnapshot {
                            design: Some(design),
                            thread_id: None,
                            message_id: None,
                            artifact_bundle: None,
                            model_manifest: None,
                            selected_part_id: None,
                        });
                    }
                }
            }

            let db_path = config_dir.join("history.sqlite");
            let conn = db::init_db(&db_path).expect("Failed to initialize SQLite database");
            let _ = migrate_legacy_references(&conn);

            app.manage(AppState {
                config: Mutex::new(config),
                last_snapshot: Mutex::new(last_snapshot),
                db: tokio::sync::Mutex::new(conn),
                render_lock: tokio::sync::Mutex::new(()),
            });

            Ok(())
        })
        .invoke_handler(builder.invoke_handler())
        .run(context)
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_code_blocks ---

    #[test]
    fn extract_code_blocks_python_block() {
        let input = "Here is code:\n```python\nimport FreeCAD\nprint('hi')\n```\nDone.";
        let blocks = extract_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("import FreeCAD"));
        assert!(blocks[0].contains("print('hi')"));
        // Language identifier should be stripped
        assert!(!blocks[0].contains("python"));
    }

    #[test]
    fn extract_code_blocks_empty_input() {
        let blocks = extract_code_blocks("no code blocks here");
        assert!(blocks.is_empty());
    }

    #[test]
    fn extract_code_blocks_multiple_blocks() {
        let input = "```python\nblock1\n```\ntext\n```py\nblock2\n```";
        let blocks = extract_code_blocks(input);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], "block1");
        assert_eq!(blocks[1], "block2");
    }

    #[test]
    fn extract_code_blocks_strips_language_identifier() {
        let input = "```python\ncode here\n```";
        let blocks = extract_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], "code here");
    }

    #[test]
    fn extract_code_blocks_no_language_identifier() {
        let input = "```\nplain code\n```";
        let blocks = extract_code_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], "plain code");
    }

    // --- looks_like_python_macro ---

    #[test]
    fn looks_like_python_macro_freecad_code() {
        let code = "import FreeCAD\nimport Part\ndoc = App.ActiveDocument";
        assert!(looks_like_python_macro(code));
    }

    #[test]
    fn looks_like_python_macro_false_for_random_text() {
        let text = "This is just some random text about nothing.";
        assert!(!looks_like_python_macro(text));
    }

    #[test]
    fn looks_like_python_macro_needs_two_signals() {
        // Only one signal should not be enough
        let one_signal = "import FreeCAD\nprint('hello')";
        assert!(!looks_like_python_macro(one_signal));

        // Two signals should pass
        let two_signals = "import FreeCAD\nimport Part";
        assert!(looks_like_python_macro(two_signals));
    }

    #[test]
    fn looks_like_python_macro_alternative_pattern() {
        // Tests the `import` + `if doc is none` alternative
        let code = "import something\nif doc is None:\n    pass";
        assert!(looks_like_python_macro(code));
    }

    // --- summarize_reference ---

    #[test]
    fn summarize_reference_python_macro() {
        let result = summarize_reference("python_macro", "my_macro", "import FreeCAD\nprint('hi')");
        assert!(result.contains("Python macro reference"));
        assert!(result.contains("my_macro"));
        assert!(result.contains("import FreeCAD"));
    }

    #[test]
    fn summarize_reference_attachment() {
        let result = summarize_reference("attachment", "file.stl", "binary data here");
        assert!(result.contains("Attachment reference"));
        assert!(result.contains("file.stl"));
    }

    #[test]
    fn summarize_reference_empty_content() {
        let result = summarize_reference("python_macro", "empty_macro", "");
        assert!(result.contains("Python macro reference"));
        assert!(result.contains("empty_macro"));
    }

    #[test]
    fn summarize_reference_unknown_kind() {
        let result = summarize_reference("something_else", "ref", "content");
        assert!(result.contains("Reference"));
        assert!(result.contains("ref"));
    }

    #[test]
    fn generate_genie_traits_returns_v2_profile() {
        let traits = generate_genie_traits();
        assert_eq!(traits.version, crate::models::GENIE_TRAITS_VERSION);
        assert!(traits.seed > 0);
        assert!((10..=24).contains(&traits.vertex_count));
    }

    #[test]
    fn explicit_question_only_markers_force_question_mode() {
        assert!(is_explicit_question_only_request("answer only: why is this thin?"));
        assert!(is_explicit_question_only_request("только ответь, почему тут дырка?"));

        let fallback = fallback_intent("just answer, do not generate anything");
        assert_eq!(fallback.intent_mode, "question");
    }
}
