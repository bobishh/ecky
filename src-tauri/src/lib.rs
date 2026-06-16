#![allow(unexpected_cfgs)]
#![allow(
    clippy::bool_assert_comparison,
    clippy::derivable_impls,
    clippy::explicit_auto_deref,
    clippy::if_same_then_else,
    clippy::len_zero,
    clippy::manual_is_multiple_of,
    clippy::manual_map,
    clippy::map_identity,
    clippy::needless_borrow,
    clippy::needless_range_loop,
    clippy::redundant_guards,
    clippy::result_large_err,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

pub mod bindings;
pub mod build123d;
pub mod cad_transpile;
pub mod commands;
pub mod component_extract;
pub mod component_package_runtime;
pub mod context;
pub mod contracts;
pub mod db;
pub mod displacement;
pub mod ecky_cad_host;
pub mod ecky_core_ir;
pub mod agent_prompt;
pub mod ecky_deterministic;
pub mod ecky_ir;
pub mod ecky_ir_patterns;
pub mod ecky_language_surface;
pub mod ecky_scheme;
pub mod freecad;
pub mod freecad_library;
pub mod legacy_python_to_ecky_ir;
pub mod lithophane;
pub mod llm;
pub mod llm_context;
pub mod mcp;
pub mod model_runtime;
pub mod models;
pub mod project_mirror;
pub mod runtime_capabilities;
pub mod services;
pub mod sketch_brep_validation;
pub mod sketch_draft_runtime;
pub mod source_flavor;
pub mod topology_target_ids;

#[cfg(test)]
pub(crate) fn build123d_test_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use tokio::time::sleep;
use uuid::Uuid;

use crate::context::*;
use crate::models::{
    AppState, Attachment, DesignOutput, GenieTraits, LastDesignSnapshot, PathResolver,
    ThreadReference,
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

fn init_history_db_with_recovery(
    config_dir: &Path,
) -> Result<(rusqlite::Connection, Vec<String>), String> {
    let db_path = config_dir.join("history.sqlite");
    match db::init_db(&db_path) {
        Ok(conn) => Ok((conn, Vec::new())),
        Err(initial_err) => {
            let mut warnings = vec![format!(
                "[BOOT] Failed to initialize history database at {}: {}",
                db_path.display(),
                initial_err
            )];

            if db_path.exists() {
                let backup_path = config_dir.join(format!(
                    "history.unreadable.{}.sqlite",
                    Uuid::new_v4().simple()
                ));
                fs::rename(&db_path, &backup_path).map_err(|rename_err| {
                    format!(
                        "[BOOT] History database init failed at {}: {}. Recovery rename to {} also failed: {}",
                        db_path.display(),
                        initial_err,
                        backup_path.display(),
                        rename_err
                    )
                })?;
                warnings.push(format!(
                    "[BOOT] Moved unreadable history database to {}",
                    backup_path.display()
                ));
            }

            let recovered = db::init_db(&db_path).map_err(|recovery_err| {
                format!(
                    "[BOOT] Recovery init failed for history database at {} after initial error {}: {}",
                    db_path.display(),
                    initial_err,
                    recovery_err
                )
            })?;

            warnings.push(format!(
                "[BOOT] Recreated history database at {}",
                db_path.display()
            ));
            Ok((recovered, warnings))
        }
    }
}

fn has_explicit_max_verify_attempts(raw: &serde_json::Value) -> bool {
    raw.get("maxVerifyAttempts").is_some() || raw.get("max_verify_attempts").is_some()
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AttachmentReferenceMeta {
    pub path: String,
    pub name: String,
    pub explanation: String,
    pub kind: String,
    pub data_url: Option<String>,
}

pub(crate) fn summarize_reference(kind: &str, name: &str, content: &str) -> String {
    let intro = match kind {
        "python_macro" => "Python macro reference",
        "attachment" | "attachment_meta" => "Attachment reference",
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
        for (ordinal_offset, attachment) in (100..).zip(attachments.iter()) {
            let ext = attachment
                .path
                .split('.')
                .next_back()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| attachment.name.split('.').next_back())
                .unwrap_or("png")
                .to_lowercase();
            let is_python = matches!(ext.as_str(), "py" | "fcmacro");
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
                kind: "attachment_meta".to_string(),
                name: attachment.name.clone(),
                content: serde_json::to_string(&AttachmentReferenceMeta {
                    path: attachment.path.clone(),
                    name: attachment.name.clone(),
                    explanation: attachment.explanation.clone(),
                    data_url: attachment.data_url.clone(),
                    kind: if is_python {
                        "cad".to_string()
                    } else {
                        match attachment.kind {
                            crate::contracts::AttachmentKind::Image => "image".to_string(),
                            crate::contracts::AttachmentKind::Cad => "cad".to_string(),
                        }
                    },
                })
                .unwrap_or_default(),
                summary,
                pinned: true,
                created_at,
            };
            db::add_thread_reference(conn, &reference).map_err(|e| e.to_string())?;
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
    const THREAD_SUMMARY_CONTEXT_LIMIT: usize = 32;
    let messages =
        db::get_recent_thread_messages_for_summary(conn, thread_id, THREAD_SUMMARY_CONTEXT_LIMIT)
            .map_err(|e| e.to_string())?;
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
            final_response: Some("Answering the question without generating geometry.".to_string()),
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
            final_response: None,
            usage: None,
        }
    } else {
        models::IntentDecision {
            intent_mode: "design".to_string(),
            confidence: 0.55,
            response: "This looks like a geometry change request.".to_string(),
            final_response: None,
            usage: None,
        }
    }
}

pub(crate) const TECHNICAL_SYSTEM_PROMPT: &str = r#"Return a JSON object with:
1. "title": 2-5 words project title.
2. "version_name": Short descriptive name for this iteration.
3. "response": short end-user text for Ecky Einacs's speech bubble (1-3 concise sentences).
4. "interaction_mode": "design" or "question".
5. "macro_code": source code that matches TARGET AUTHORING CONTEXT for this turn.
6. "ui_spec": { "fields": [ { "key": string, "label": string, "type": "range"|"number"|"select"|"checkbox"|"image" } ] }
7. "initial_params": { "key": value }
8. "post_processing": { "displacement": { "image_param": string, "projection": "planar"|"cylindrical"|"spherical", "depth_mm": number, "invert": bool } } (Optional)

CRITICAL RULES:
- UNITS: ALL dimensions are in MILLIMETERS (mm).
- CONTEXT PRIORITY: Any section labeled "ACTUAL CURRENT ... (AUTHORITATIVE)" is the real current state. Treat it as source of truth, not an example/template.
- TARGET PRIORITY: "TARGET AUTHORING CONTEXT (AUTHORITATIVE FOR THIS TURN)" tells you which source language/backend to emit in `macro_code`.
- MIGRATION PRIORITY: If "MIGRATION POLICY (AUTHORITATIVE)" says preserve current context, do not rewrite into another language/backend unless the user explicitly asks or faithful completion is impossible otherwise.
- UI: Focus on 'key', 'label' and 'type'. 
  - Use 'number' for all numeric parameters. NEVER use 'range'.
  - Use 'min_from' and 'max_from' keys in the 'ui_spec' fields to link parameter boundaries to other keys (e.g., inner_radius max_from outer_radius).
  - Ensure geometry stays sane and valid across all parameter permutations.
  - For file-picking inputs, use `type: "image"` and leave the matching initial param empty or omit it.
  - For lithophanes, expose only the image field by default. Keep projection, invert, and depth inside `post_processing` unless the user explicitly asks to tweak them.
  - If `AVAILABLE LOCAL ASSETS` is present and the user wants a lithophane without providing a new image, prefer a relevant listed asset over inventing a fake file path.
- PARAMETERS: Follow the parameter access conventions required by the active source language/framework. Keep `ui_spec`, `initial_params`, and `macro_code` aligned.
- FRAMEWORK: If an "ACTUAL CURRENT CAD FRAMEWORK" block is present, follow it strictly and use the provided CAD SDK. Do not invent custom control classes or custom registries.
- FRAMEWORK DEFAULT: Prefer the CAD SDK and `CONTROLS` for all new designs and substantial edits. Legacy raw-params macros are a fallback, not the default.
- FRAMEWORK MIGRATION: If the current design is legacy and the requested edit needs richer controls such as `type: "image"` inputs, stable typed controls, or cleaner parameter structure, you MAY migrate the design to the CAD framework while preserving the existing geometry intent.
- FRAMEWORK ENFORCEMENT: When using the CAD SDK, `CONTROLS` inside `macro_code` is the source of truth. The backend derives `ui_spec` and `initial_params` from `CONTROLS` and may reject malformed framework macros.
- FRAMEWORK PARAMS: When using the CAD SDK, raw `params` access is allowed only inside `registry.bind(params)` during config bootstrap. Use `cfg` for geometry.
- NO BRACES: NEVER use `{var}` style interpolation inside the macro_code string.
- CLEANUP: You MUST remove any parameters from "ui_spec" and "initial_params" that are no longer used in the current "macro_code". Do not accumulate parameters from previous designs.
- PRINTABILITY: Prefer geometry that is straightforward to 3D print (manifold solids, reasonable wall thickness, avoid fragile or unsupported details unless requested).
- PRINTABILITY REPORTING: If printability risks remain, mention them explicitly at the end of "response" as a separate sentence prefixed with `PRINTING RISKS:`.
- LITHOPHANE DEFAULTS: If you return `post_processing.displacement`, choose projection automatically from the model intent: `planar` for flat faces, `cylindrical` for wrapped round walls, `spherical` only for globe-like surfaces.
- LITHOPHANE NO-OP: If the image parameter is empty, the displacement should no-op so the base geometry still previews correctly.
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
            model: "gemini-2.5-flash".to_string(),
            light_model: "gemini-2.5-flash-lite".to_string(),
            base_url: "".to_string(),
            enabled: false,
            vision_overrides: std::collections::HashMap::new(),
        }],
        selected_engine_id: "default-gemini".to_string(),
        freecad_cmd: String::new(),
        cad_text_font_path: String::new(),
        freecad_library_roots: Vec::new(),
        assets: vec![],
        microwave: None,
        voice: crate::models::VoiceConfig::default(),
        mcp: crate::models::McpConfig::default(),
        has_seen_onboarding: false,
        connection_type: None,
        default_engine_kind: crate::models::EngineKind::Freecad,
        default_source_language: crate::models::SourceLanguage::LegacyPython,
        default_geometry_backend: crate::models::GeometryBackend::Freecad,
        max_generation_attempts: 3,
        max_verify_attempts: 2,
        projects_root: None,
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(move |app| {
            let config_dir = app.handle().app_config_dir();
            let app_data_dir = app.handle().app_data_dir();
            if !config_dir.exists() {
                fs::create_dir_all(&config_dir)?;
            }
            if !app_data_dir.exists() {
                fs::create_dir_all(&app_data_dir)?;
            }

            let mut config = default_config;
            let mut has_explicit_mcp_mode = false;
            let mut has_explicit_primary_agent = false;
            let mut has_explicit_max_verify_attempts_field = false;
            let config_path = config_dir.join("config.json");
            if config_path.exists() {
                if let Ok(data) = fs::read_to_string(&config_path) {
                    if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&data) {
                        has_explicit_mcp_mode =
                            raw.get("mcp").and_then(|mcp| mcp.get("mode")).is_some();
                        has_explicit_primary_agent = raw
                            .get("mcp")
                            .and_then(|mcp| mcp.get("primaryAgentId"))
                            .is_some();
                        has_explicit_max_verify_attempts_field =
                            has_explicit_max_verify_attempts(&raw);
                    }
                    if let Ok(c) = serde_json::from_str::<crate::models::Config>(&data) {
                        config = c;
                    }
                }
            }
            let mut should_persist_config = false;
            if crate::commands::assets::sync_image_assets_into_config(app.handle(), &mut config)? {
                should_persist_config = true;
            }
            if !has_explicit_mcp_mode {
                let next_mode = crate::mcp::runtime::default_mcp_mode(&config);
                if config.mcp.mode != next_mode {
                    config.mcp.mode = next_mode;
                    should_persist_config = true;
                }
            }
            if !has_explicit_primary_agent
                || crate::mcp::runtime::ensure_primary_agent_id(&mut config)
            {
                should_persist_config = true;
            }
            if !has_explicit_max_verify_attempts_field && config.max_verify_attempts != 2 {
                config.max_verify_attempts = 2;
                should_persist_config = true;
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
            let (conn, startup_warnings) = init_history_db_with_recovery(&config_dir)
                .map_err(|err| tauri::Error::Io(std::io::Error::other(err)))?;
            let read_conn = db::init_db(&db_path).map_err(|err| {
                tauri::Error::Io(std::io::Error::other(format!(
                    "Failed to open read history database at {}: {}",
                    db_path.display(),
                    err
                )))
            })?;
            if let Ok(interrupted) = db::mark_interrupted_pending_messages(&conn) {
                if interrupted > 0 {
                    eprintln!(
                        "[BOOT] recovered {} interrupted pending request(s) as error",
                        interrupted
                    );
                }
            }
            let _ = migrate_legacy_references(&conn);

            let mcp_port = config.mcp.port;
            let state =
                AppState::new_with_read_connection(config, last_snapshot, conn, Some(read_conn));
            state.set_app_handle(app.handle().clone());
            app.manage(state.clone());
            for warning in startup_warnings {
                eprintln!("{}", warning);
                state.push_log(warning);
            }

            {
                let resolver: Arc<dyn PathResolver + Send + Sync> = Arc::new(app.handle().clone());
                let server_state = state.clone();
                let server_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        if let Err(err) = crate::mcp::server::serve_http_on_port(
                            server_state.clone(),
                            resolver.clone(),
                            server_handle.clone(),
                            mcp_port,
                        )
                        .await
                        {
                            eprintln!("[MCP] HTTP server stopped: {}", err);
                            server_state.set_mcp_status(false, Some(err.to_string()));
                            sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                        break;
                    }
                });
            }

            crate::mcp::runtime::initialize_auto_agent_supervisors(state.clone());

            {
                // Project folder watcher: external edits to
                // <projects>/<slug>/model.ecky auto-apply as new versions
                // ("edit in place" for editors and LLM file skills).
                let resolver: Arc<dyn PathResolver + Send + Sync> = Arc::new(app.handle().clone());
                let watcher_state = state.clone();
                let watcher_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    let mut watcher = crate::mcp::handlers::ProjectFolderWatcher::new();
                    let ctx = crate::mcp::handlers::project_folder_watcher_context();
                    loop {
                        sleep(Duration::from_secs(1)).await;
                        let events = watcher.tick(&watcher_state, resolver.as_ref(), &ctx).await;
                        if events.is_empty() {
                            continue;
                        }
                        if events.iter().any(|event| {
                            matches!(
                                event,
                                crate::mcp::handlers::ProjectFolderWatchEvent::Applied { .. }
                            )
                        }) {
                            let _ = watcher_handle.emit("history-updated", ());
                        }
                        let _ = watcher_handle.emit("project-folder-sync", &events);
                        for event in &events {
                            if let crate::mcp::handlers::ProjectFolderWatchEvent::ApplyFailed {
                                slug,
                                error,
                            } = event
                            {
                                watcher_state.push_log(format!(
                                    "[PROJECT] folder `{slug}` apply failed: {error}"
                                ));
                            }
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(builder.invoke_handler());

    if let Err(err) = app.run(context) {
        eprintln!("[BOOT] Failed to run tauri application: {}", err);
    }
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
    fn generate_genie_traits_returns_current_profile() {
        let traits = generate_genie_traits();
        assert_eq!(traits.version, crate::models::GENIE_TRAITS_VERSION);
        assert!(traits.seed > 0);
        assert!((10..=24).contains(&traits.vertex_count));
    }

    #[test]
    fn detects_explicit_max_verify_attempts_in_camel_or_snake_case() {
        assert!(has_explicit_max_verify_attempts(&serde_json::json!({
            "maxVerifyAttempts": 0
        })));
        assert!(has_explicit_max_verify_attempts(&serde_json::json!({
            "max_verify_attempts": 0
        })));
        assert!(!has_explicit_max_verify_attempts(&serde_json::json!({})));
    }

    #[test]
    fn explicit_question_only_markers_force_question_mode() {
        assert!(is_explicit_question_only_request(
            "answer only: why is this thin?"
        ));
        assert!(is_explicit_question_only_request(
            "только ответь, почему тут дырка?"
        ));

        let fallback = fallback_intent("just answer, do not generate anything");
        assert_eq!(fallback.intent_mode, "question");
    }

    #[test]
    fn init_history_db_with_recovery_moves_unreadable_path_and_recreates_database() {
        let temp_root =
            std::env::temp_dir().join(format!("ecky-history-recovery-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&temp_root).expect("temp root should be created");

        let db_path = temp_root.join("history.sqlite");
        fs::create_dir_all(&db_path).expect("poisoned database path should be a directory");

        let (conn, warnings) =
            init_history_db_with_recovery(&temp_root).expect("recovery should succeed");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))
            .expect("recovered database should be queryable");
        assert!(count > 0);
        assert!(db_path.is_file());
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("Moved unreadable history database")));
        assert!(fs::read_dir(&temp_root)
            .expect("temp root should be readable")
            .filter_map(Result::ok)
            .any(|entry| {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                file_name.starts_with("history.unreadable.") && entry.path().is_dir()
            }));

        fs::remove_dir_all(&temp_root).expect("temp root should be cleaned up");
    }
}
