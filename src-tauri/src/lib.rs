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

pub mod agent_prompt;
pub mod bindings;
pub mod build_queue;
pub mod cad_source_adapters;
pub mod cad_transpile;
pub mod campaign_definition;
pub mod campaign_projects;
pub mod capture_brep_validation;
pub mod capture_deviation;
pub mod capture_guidance;
pub mod capture_mesh_crop;
pub mod capture_reconstruction;
pub mod capture_runs;
pub mod capture_server;
pub mod commands;
pub mod component_extract;
pub mod component_import_runtime;
pub mod component_package_runtime;
pub mod component_step_runtime;
pub mod config_store;
pub mod context;
pub mod context_envelope;
pub mod contracts;
pub mod db;
pub mod displacement;
pub mod ecky_cad_host;
pub mod ecky_core_ir;
pub mod ecky_deterministic;
pub mod ecky_ir;
pub mod ecky_ir_patterns;
pub mod ecky_language_surface;
pub mod ecky_scheme;
pub mod exploration_cycle;
pub mod exploration_eval;
pub mod exploration_prompt;
pub mod exploration_run_registry;
pub mod exploration_scheduler;
pub mod exploration_store;
pub mod external_shapes;
pub mod fem_engineering;
pub mod fem_topology_reconstruction;
pub mod freecad;
pub mod freecad_library;
pub mod gmsh_mesher;
mod image_sampling;
pub mod jev_classifier;
pub mod legacy_python_to_ecky_ir;
pub mod lithophane;
pub mod llm;
pub mod llm_context;
pub mod llm_eval;
pub mod mcp;
pub mod model_runtime;
pub mod models;
pub mod netgen_mesher;
pub mod project_mirror;
pub mod provider_turn;
pub mod raster_trace;
pub mod runtime_capabilities;
pub mod services;
pub mod shape_summary;
pub mod sketch_brep_validation;
pub mod sketch_draft_runtime;
pub mod source_flavor;
pub mod steel_data;
pub mod strict_edn;
pub mod surface_trim_cap;
pub mod surface_trim_cut;
pub mod surface_trim_diagnostics;
pub mod surface_trim_external_shapes;
pub mod surface_trim_mesh;
pub mod surface_trim_runtime;
pub mod surface_trim_source;
pub mod thread_lifecycle;
pub mod thread_source_binding;
pub mod topology_target_ids;
pub mod transport_budget;
pub mod web_content_recovery;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use tokio::time::sleep;
use uuid::Uuid;

use crate::context::*;
use crate::contracts::{
    AppResult, Attachment, Config, GenieTraits, IntentDecision, ThreadReference,
};
use crate::models::{AppState, PathResolver};

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
        Err(initial_err) => Err(format!(
            "[BOOT] Failed to initialize history database at {}. Database preserved; startup aborted: {}",
            db_path.display(),
            initial_err
        )),
    }
}

fn load_startup_config(
    app: &dyn PathResolver,
    config_dir: &Path,
    default: Config,
) -> AppResult<(Config, crate::models::ConfigPersistenceStatus)> {
    let outcome = crate::config_store::load_config(config_dir, default, |mut config, markers| {
        if !markers.mcp_mode_present {
            config.mcp.mode = crate::mcp::runtime::default_mcp_mode(&config);
        }
        if config.mcp.mode == crate::contracts::McpMode::Active {
            config.mcp.mode = crate::contracts::McpMode::Passive;
        }
        if !markers.primary_agent_id_present {
            crate::mcp::runtime::ensure_primary_agent_id(&mut config);
        }
        Ok(config)
    })?;
    let mut config = outcome.config;
    let mut warnings = outcome.warnings;
    let primary_changed = crate::mcp::runtime::ensure_primary_agent_id(&mut config);
    let assets_changed = crate::commands::assets::sync_image_assets_into_config(app, &mut config)
        .map_err(|_| {
        crate::contracts::AppError::persistence("config startup failed: asset-scan")
    })?;
    let mut cleanup_pending = outcome.cleanup_pending;
    let derived_changed = primary_changed || assets_changed;
    if derived_changed && outcome.source != crate::config_store::ConfigSource::Default {
        let saved = crate::config_store::save_config(config_dir, config.clone())?;
        warnings.extend(saved.warnings);
        cleanup_pending = saved.cleanup_pending;
    }
    warnings.sort();
    warnings.dedup();
    Ok((
        config,
        crate::models::ConfigPersistenceStatus {
            cleanup_pending,
            warnings,
        },
    ))
}

fn load_startup_config_or_default(
    app: &dyn PathResolver,
    config_dir: &Path,
    default: Config,
) -> (Config, crate::models::ConfigPersistenceStatus) {
    match load_startup_config(app, config_dir, default.clone()) {
        Ok(loaded) => loaded,
        Err(err) => (
            default,
            crate::models::ConfigPersistenceStatus {
                cleanup_pending: false,
                warnings: vec![format!(
                    "[BOOT] Config load failed; running with defaults: {err}"
                )],
            },
        ),
    }
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
    const MIGRATION_KEY: &str = "legacy-prompt-references-projected-v2";
    let already_applied = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE key = ?1)",
            [MIGRATION_KEY],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|e| e.to_string())?;
    if already_applied {
        return Ok(());
    }
    let threads = db::get_all_threads(conn).map_err(|e| e.to_string())?;
    let rows = db::get_legacy_user_prompt_rows(conn).map_err(|e| e.to_string())?;
    for (thread_id, message_id, content, timestamp) in rows {
        persist_user_prompt_references(conn, &thread_id, &message_id, &content, None, timestamp)?;
    }
    for thread in threads {
        if !thread.summary.trim().is_empty() {
            continue;
        }
        let messages = db::get_recent_thread_messages_for_summary(conn, &thread.id, 32)
            .map_err(|e| e.to_string())?;
        let summary = build_thread_summary(&thread.title, &messages);
        db::update_thread_summary(conn, &thread.id, &summary).map_err(|e| e.to_string())?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations(key, applied_at) VALUES (?1, CAST(strftime('%s','now') AS INTEGER))",
        [MIGRATION_KEY],
    )
    .map_err(|e| e.to_string())?;
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

pub(crate) fn fallback_intent(prompt: &str) -> IntentDecision {
    let p = prompt.to_lowercase();
    if is_explicit_question_only_request(prompt) {
        return IntentDecision {
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
        IntentDecision {
            intent_mode: "question".to_string(),
            confidence: 0.55,
            response: "Thinking not deep enough. This looks like a question.".to_string(),
            final_response: None,
            usage: None,
        }
    } else {
        IntentDecision {
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
9. "next_action": { "action": "BUILD"|"ASK"|"STOP", "source_version_id": string, "hypothesis": string, "change_scope": string, "expected_evidence": string, "budget_cost": non-negative integer, "question": string|null, "blocked_decision": string|null }

The `next_action` object is transient controller input. When an exploration
cycle envelope is present, return it in this same authoring response; do not
nest it inside the design and do not put lifecycle fields in `macro_code`.
BUILD, ASK, and STOP are the only valid actions. Use the exact current version
identity from the context. BUILD requires non-empty hypothesis, change_scope,
and expected_evidence. ASK requires a concrete question and blocked_decision;
its BUILD-only fields may be empty strings. STOP may also leave BUILD-only
fields empty and explains why no bounded action is justified in response. For legacy
non-cycle/question calls, this field may be omitted.
ASK and STOP still return the complete existing design object (use the exact
unchanged source when no geometry change is requested); the controller ignores
that object and does not persist it as a new version.

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
- BINDINGS: NEVER use `(define ...)` inside `(model ...)`. It will fail with a misleading TypeMismatch error. Use `(let* ...)` inside each `(part ...)` to compute derived values from params — e.g. `(part body (let* ((half (/ frame_length 2))) (box half 10 10)))`. Top-level `(define (fn args) ...)` helper functions OUTSIDE `(model ...)` are allowed and correct for reusable pure functions.
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
- When an exploration cycle is active, always return `next_action`, even when
  PLAN and BUILD happen in this same provider turn. Do not emit a second
  executable plan or require another model call just to restate the action. For
  QUESTION_ONLY outside a cycle, the field may be omitted.
"#;

pub fn run() {
    set_macos_process_name("Ecky CAD");
    let context = tauri::generate_context!();
    let builder = crate::bindings::builder();

    let default_config = crate::contracts::Config {
        engines: vec![crate::contracts::Engine {
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
        voice: crate::contracts::VoiceConfig::default(),
        mcp: crate::contracts::McpConfig::default(),
        fem_compute: crate::contracts::FemComputeConfig::default(),
        has_seen_onboarding: false,
        connection_type: None,
        provider_models: crate::contracts::ProviderModels::default(),
        jev_classifier: Default::default(),
        default_engine_kind: crate::contracts::EngineKind::Freecad,
        default_source_language: crate::contracts::SourceLanguage::LegacyPython,
        default_geometry_backend: crate::contracts::GeometryBackend::Freecad,
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

            let (config, config_persistence_status) =
                load_startup_config_or_default(app.handle(), &config_dir, default_config);

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
            let recovery_now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if let Err(error) =
                crate::exploration_store::mark_in_flight_interrupted(&conn, recovery_now)
            {
                eprintln!(
                    "[BOOT] failed to mark in-flight exploration cycles interrupted: {error}"
                );
            }
            let startup_now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            match crate::services::agy_provider::reconcile_stale_deliveries(
                &conn,
                &crate::services::agy_provider::SystemAgyProcessReaper,
                startup_now,
            ) {
                Ok(reconciled) if reconciled > 0 => {
                    eprintln!("[BOOT] reconciled {reconciled} interrupted Agy provider delivery(s)")
                }
                Ok(_) => {}
                Err(error) => eprintln!("[BOOT] failed to reconcile Agy providers: {error}"),
            }
            let _ = crate::services::codex_takeover::recover_stale_sending(&conn, startup_now);
            let _ = migrate_legacy_references(&conn);
            let last_snapshot = crate::services::session::read_last_snapshot(app.handle(), &conn);

            let mcp_port = config.mcp.port;
            let state =
                AppState::new_with_read_connection(config, last_snapshot, conn, Some(read_conn));
            *state.config_persistence_status.lock().unwrap() = config_persistence_status.clone();
            state.set_app_handle(app.handle().clone());
            app.manage(state.clone());
            if let Err(error) = crate::web_content_recovery::install(app.handle()) {
                state.push_log(format!(
                    "[WEB_CONTENT] failed to install termination hook: {error}"
                ));
            }
            for warning in startup_warnings {
                eprintln!("{}", warning);
                state.push_log(warning);
            }
            for warning in config_persistence_status.warnings {
                eprintln!("[BOOT] {warning}");
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

            {
                let resolver: Arc<dyn PathResolver + Send + Sync> = Arc::new(app.handle().clone());
                let capture_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) =
                        crate::capture_server::serve(capture_state.clone(), resolver).await
                    {
                        let message = format!("[CAPTURE] LAN service stopped: {error}");
                        eprintln!("{message}");
                        capture_state.push_log(message);
                    }
                });
            }

            crate::mcp::runtime::initialize_auto_agent_supervisors(state.clone());
            crate::commands::codex_takeover::initialize_codex_queue_supervisor(
                state.clone(),
                app.handle().clone(),
            );
            crate::commands::agy_provider::initialize_agy_queue_supervisor(
                state.clone(),
                app.handle().clone(),
            );

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
                    let mut transport = crate::mcp::handlers::ProjectFolderWatchTransport::new(
                        &watcher_state,
                        resolver.as_ref(),
                    )
                    .await;
                    let ctx = crate::mcp::handlers::project_folder_watcher_context();
                    loop {
                        transport.wait_until(watcher.next_settle_deadline()).await;
                        let events = watcher.tick(&watcher_state, resolver.as_ref(), &ctx).await;
                        if events.is_empty() {
                            continue;
                        }
                        if let Some((thread_id, message_id, kind)) =
                            events.iter().find_map(|event| match event {
                                crate::mcp::handlers::ProjectFolderWatchEvent::Applied {
                                    thread_id,
                                    message_id,
                                    ..
                                } => Some((
                                    thread_id.clone(),
                                    message_id.clone(),
                                    "project-folder-applied",
                                )),
                                crate::mcp::handlers::ProjectFolderWatchEvent::ApplyFailed {
                                    thread_id,
                                    message_id,
                                    ..
                                } => Some((
                                    thread_id.clone(),
                                    message_id.clone(),
                                    "project-folder-failed",
                                )),
                                _ => None,
                            })
                        {
                            let event = crate::models::next_history_changed_event(
                                Some(thread_id),
                                Some(message_id),
                                kind,
                            );
                            let _ = watcher_handle.emit("history-updated", event);
                        }
                        let _ = watcher_handle.emit("project-folder-sync", &events);
                        for event in &events {
                            if let crate::mcp::handlers::ProjectFolderWatchEvent::ApplyFailed {
                                slug,
                                error,
                                ..
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

    let app = match app.build(context) {
        Ok(app) => app,
        Err(err) => {
            eprintln!("[BOOT] Failed to build tauri application: {}", err);
            return;
        }
    };
    let provider_shutdown_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.run(move |app_handle, event| {
        if !matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) || provider_shutdown_started.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let state = app_handle.state::<AppState>();
        tauri::async_runtime::block_on(state.agy_provider.shutdown_all());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StartupPaths(std::path::PathBuf);

    impl PathResolver for StartupPaths {
        fn app_config_dir(&self) -> std::path::PathBuf {
            self.0.join("config")
        }
        fn app_data_dir(&self) -> std::path::PathBuf {
            self.0.join("data")
        }
        fn resource_path(&self, _path: &str) -> Option<std::path::PathBuf> {
            None
        }
    }

    fn startup_config() -> Config {
        Config {
            engines: vec![],
            selected_engine_id: "selected".into(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: vec![],
            assets: vec![],
            microwave: None,
            voice: crate::contracts::VoiceConfig::default(),
            mcp: crate::contracts::McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: false,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: crate::contracts::EngineKind::EckyIrV0,
            default_source_language: crate::contracts::SourceLanguage::EckyIrV0,
            default_geometry_backend: crate::contracts::GeometryBackend::Build123d,
            max_generation_attempts: 3,
            max_verify_attempts: 2,
            projects_root: None,
        }
    }

    fn startup_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ecky-startup-config-{}", Uuid::new_v4()))
    }

    #[test]
    fn bdd_startup_json_only_backfills_edn_and_removes_json() {
        let root = startup_root();
        let paths = StartupPaths(root.clone());
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        let mut legacy = serde_json::to_value(startup_config()).unwrap();
        legacy["mcp"]["autoAgents"] = serde_json::json!([{
            "id": "agent",
            "label": "Agent",
            "cmd": "sentinel-secret-command",
            "args": [],
            "enabled": true,
            "startOnDemand": true
        }]);
        legacy["mcp"].as_object_mut().unwrap().remove("mode");
        legacy["mcp"]
            .as_object_mut()
            .unwrap()
            .remove("primaryAgentId");
        legacy.as_object_mut().unwrap().remove("maxVerifyAttempts");
        fs::write(
            paths.app_config_dir().join("config.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let (loaded, _) =
            load_startup_config(&paths, &paths.app_config_dir(), startup_config()).unwrap();
        assert_eq!(loaded.mcp.mode, crate::contracts::McpMode::Passive);
        assert_eq!(loaded.mcp.primary_agent_id.as_deref(), Some("agent"));
        assert_eq!(loaded.max_verify_attempts, 2);
        assert!(!loaded.mcp.auto_agents[0].start_on_demand);
        assert!(paths.app_config_dir().join("config.edn").exists());
        assert!(!paths.app_config_dir().join("config.json").exists());
        let disk = crate::config_store::load_config(
            &paths.app_config_dir(),
            startup_config(),
            |config, _| Ok(config),
        )
        .unwrap()
        .config;
        assert_eq!(disk, loaded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_startup_invalid_edn_fails_closed_without_deleting_json() {
        let root = startup_root();
        let paths = StartupPaths(root.clone());
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        let invalid_edn = "sentinel-secret-edn [";
        let invalid_json = "{sentinel-secret-json";
        fs::write(paths.app_config_dir().join("config.edn"), invalid_edn).unwrap();
        fs::write(paths.app_config_dir().join("config.json"), invalid_json).unwrap();
        let error =
            load_startup_config(&paths, &paths.app_config_dir(), startup_config()).unwrap_err();
        assert!(!error.to_string().contains("sentinel-secret"));
        assert_eq!(
            fs::read_to_string(paths.app_config_dir().join("config.edn")).unwrap(),
            invalid_edn
        );
        assert_eq!(
            fs::read_to_string(paths.app_config_dir().join("config.json")).unwrap(),
            invalid_json
        );
        assert!(paths.app_config_dir().join("config.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_startup_invalid_edn_uses_defaults_without_overwriting_the_file() {
        let root = startup_root();
        let paths = StartupPaths(root.clone());
        let invalid_edn = "invalid-shape [";
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::write(paths.app_config_dir().join("config.edn"), invalid_edn).unwrap();

        let default = startup_config();
        let (loaded, status) =
            load_startup_config_or_default(&paths, &paths.app_config_dir(), default.clone());

        assert_eq!(loaded, default);
        assert_eq!(
            fs::read_to_string(paths.app_config_dir().join("config.edn")).unwrap(),
            invalid_edn
        );
        assert_eq!(
            status.warnings,
            vec!["[BOOT] Config load failed; running with defaults: config.edn: invalid-data"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_startup_repairs_stale_edn_primary_agent_and_persists_repair() {
        let root = startup_root();
        let paths = StartupPaths(root.clone());
        let mut stale = startup_config();
        stale.mcp.auto_agents.push(crate::contracts::AutoAgent {
            id: "agent".into(),
            label: "Agent".into(),
            cmd: "command".into(),
            model: None,
            args: vec![],
            enabled: true,
            start_on_demand: false,
        });
        stale.mcp.primary_agent_id = Some("missing".into());
        crate::config_store::save_config(&paths.app_config_dir(), stale).unwrap();

        let (loaded, _) =
            load_startup_config(&paths, &paths.app_config_dir(), startup_config()).unwrap();

        assert_eq!(loaded.mcp.primary_agent_id.as_deref(), Some("agent"));
        let disk = crate::config_store::load_config(
            &paths.app_config_dir(),
            startup_config(),
            |config, _| Ok(config),
        )
        .unwrap()
        .config;
        assert_eq!(disk.mcp.primary_agent_id.as_deref(), Some("agent"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_startup_reports_static_asset_scan_error_without_path() {
        let root = startup_root();
        let paths = StartupPaths(root.clone());
        fs::create_dir_all(&root).unwrap();
        fs::write(paths.app_data_dir(), "sentinel-secret-path").unwrap();

        let error =
            load_startup_config(&paths, &paths.app_config_dir(), startup_config()).unwrap_err();

        assert_eq!(error.message, "config startup failed: asset-scan");
        assert!(!error.to_string().contains("sentinel-secret"));
        assert!(!error
            .to_string()
            .contains(&root.to_string_lossy().to_string()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_startup_surfaces_cleanup_pending_and_retries_next_load() {
        let root = startup_root();
        let paths = StartupPaths(root.clone());
        crate::config_store::save_config(&paths.app_config_dir(), startup_config()).unwrap();
        let json = paths
            .app_config_dir()
            .join(crate::config_store::CONFIG_JSON_FILE);
        fs::create_dir(&json).unwrap();

        let (_, pending) =
            load_startup_config(&paths, &paths.app_config_dir(), startup_config()).unwrap();
        assert!(pending.cleanup_pending);
        assert_eq!(pending.warnings, vec!["config.json: cleanup-pending"]);

        fs::remove_dir(&json).unwrap();
        fs::write(&json, "{stale-invalid-json").unwrap();
        let (_, retried) =
            load_startup_config(&paths, &paths.app_config_dir(), startup_config()).unwrap();
        assert!(!retried.cleanup_pending);
        assert!(retried.warnings.is_empty());
        assert!(!json.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_startup_default_keeps_files_absent() {
        let root = startup_root();
        let paths = StartupPaths(root.clone());
        let _ = load_startup_config(&paths, &paths.app_config_dir(), startup_config()).unwrap();
        assert!(!paths.app_config_dir().join("config.edn").exists());
        assert!(!paths.app_config_dir().join("config.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

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
        assert_eq!(traits.version, crate::contracts::GENIE_TRAITS_VERSION);
        assert!(traits.seed > 0);
        assert!((10..=24).contains(&traits.vertex_count));
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
    fn init_history_db_with_recovery_preserves_unreadable_database() {
        let temp_root =
            std::env::temp_dir().join(format!("ecky-history-recovery-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&temp_root).expect("temp root should be created");

        let db_path = temp_root.join("history.sqlite");
        fs::create_dir_all(&db_path).expect("poisoned database path should be a directory");

        let error = init_history_db_with_recovery(&temp_root)
            .expect_err("unreadable database must stop startup");

        assert!(error.contains("Failed to initialize history database"));
        assert!(
            db_path.is_dir(),
            "original database path must remain intact"
        );
        assert!(!fs::read_dir(&temp_root)
            .expect("temp root should be readable")
            .filter_map(Result::ok)
            .any(|entry| {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                file_name.starts_with("history.unreadable.")
            }));

        fs::remove_dir_all(&temp_root).expect("temp root should be cleaned up");
    }
}
