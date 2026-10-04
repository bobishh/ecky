use sha2::Digest;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{Emitter, State};

use crate::contracts::{
    AgyMessagePage, AgyMessagePageInput, AgyPromptInput, AgyProviderBinding, AgyProviderSnapshot,
    AgyStopInput, AppError, AppResult, Attachment, ProviderCapabilities,
};
use crate::models::{AppState, PathResolver};
use crate::provider_turn::ProviderTurnIntent;
use crate::services::{agy_provider, codex_takeover};

static AGY_BINDING_CREATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static AGY_QUEUE_WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();
const AGY_QUEUE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn require_agy_provider_mode(state: &AppState) -> AppResult<()> {
    let configured = state.config.lock().unwrap().connection_type.clone();
    if configured.as_deref() == Some("provider:agy") {
        return Ok(());
    }
    Err(AppError::validation(format!(
        "Agy provider send requires Settings connectionType provider:agy; current value is {}.",
        configured.as_deref().unwrap_or("unset")
    )))
}

fn configured_agy_model(state: &AppState) -> Option<String> {
    let model = state
        .config
        .lock()
        .unwrap()
        .provider_models
        .agy
        .trim()
        .to_string();
    (!model.is_empty()).then_some(model)
}

#[derive(Debug, Clone)]
struct AgyEvalDispatch {
    seed: crate::llm_eval::EvalRunSeed,
    fallback_turn_id: String,
    fallback_started_at: i64,
}

async fn eval_starting_identity_for(
    state: &AppState,
    ecky_thread_id: &str,
) -> AppResult<Option<(String, Option<String>)>> {
    let conn = state.db.lock().await;
    crate::llm_eval::latest_version_identity(&conn, ecky_thread_id).map_err(AppError::persistence)
}

fn eval_run_seed(
    binding: &AgyProviderBinding,
    run_id: &str,
    prompt: &str,
    model: Option<String>,
    starting: Option<(String, Option<String>)>,
    turn_intent: ProviderTurnIntent,
    jev_route: Option<crate::llm_eval::EvalJevRoute>,
) -> crate::llm_eval::EvalRunSeed {
    let (starting_version_id, starting_input_digest) =
        starting.map_or((None, None), |(id, digest)| (Some(id), digest));
    crate::llm_eval::EvalRunSeed {
        run_id: run_id.to_string(),
        thread_id: binding.ecky_thread_id.clone(),
        external_thread_id: binding.agy_conversation_id.clone(),
        provider: agy_provider::AGY_PROVIDER_ID.into(),
        model,
        effort: None,
        prompt_version: format!("agy-provider-v{}", agy_provider::AGY_BOOTSTRAP_VERSION),
        prompt: prompt.to_string(),
        starting_version_id,
        starting_input_digest,
        expected_red_rounds: 0,
        turn_intent,
        answer_first_required: false,
        jev_route,
    }
}

fn agy_pre_dispatch_eval_run(
    mut seed: crate::llm_eval::EvalRunSeed,
    queue_id: &str,
    prompt: &str,
    started_at: i64,
    completed_at: i64,
    diagnostic: &str,
    route: Option<&crate::jev_classifier::AcceptedRoute>,
) -> crate::llm_eval::EvalRun {
    if let Some(route) = route {
        seed.turn_intent = route.intent;
        seed.answer_first_required = route.answer_first;
        seed.jev_route = Some(agy_eval_route(route, seed.model.clone()));
    }
    let mut events = vec![agy_eval_system_event(
        0,
        "request",
        "admitted",
        "Agy queued request admitted",
        Some(serde_json::json!({
            "queueId": queue_id,
            "promptSha256": format!("sha256:{:x}", sha2::Sha256::digest(prompt.as_bytes())),
            "promptChars": prompt.chars().count(),
        })),
        None,
        None,
        started_at,
    )];
    let (jev_state, jev_output) = match route {
        Some(route) => (
            "accepted",
            Some(serde_json::json!({
                "intent": route.intent.as_str(),
                "intentConfidence": route.action_confidence,
                "intentProbabilities": route.action_probabilities,
                "answerRequested": route.answer_requested,
                "answerFirst": route.answer_first,
                "model": route.model,
                "modelReason": route.model_reason,
                "contextTruncated": route.context_truncated,
                "currentPromptTruncated": route.current_prompt_truncated,
                "classifierModel": route.classifier_model,
                "classifierInputTokens": route.classifier_input_tokens,
                "classifierOutputTokens": route.classifier_output_tokens,
                "classifierLatencyMs": route.classifier_latency_ms,
            })),
        ),
        None => ("error", None),
    };
    events.push(agy_eval_system_event(
        1,
        "jev",
        jev_state,
        "Jev route result before Agy dispatch",
        None,
        jev_output,
        route.is_none().then(|| diagnostic.to_string()),
        completed_at,
    ));
    if route.is_some() {
        events.push(agy_eval_system_event(
            2,
            "delivery",
            "failed",
            "Agy delivery did not yield a provider turn ID",
            None,
            None,
            Some(diagnostic.to_string()),
            completed_at,
        ));
    }
    crate::llm_eval::build_codex_eval_run(
        seed,
        "",
        started_at,
        completed_at,
        "failed_pre_dispatch",
        None,
        Some(diagnostic.to_string()),
        events,
        Vec::new(),
    )
}

fn agy_preflight_failure_eval_run(
    seed: crate::llm_eval::EvalRunSeed,
    prompt: &str,
    started_at: i64,
    completed_at: i64,
    diagnostic: &str,
) -> crate::llm_eval::EvalRun {
    let events = vec![
        agy_eval_system_event(
            0,
            "request",
            "admitted",
            "Agy request admitted",
            Some(serde_json::json!({
                "promptSha256": format!("sha256:{:x}", sha2::Sha256::digest(prompt.as_bytes())),
                "promptChars": prompt.chars().count(),
            })),
            None,
            None,
            started_at,
        ),
        agy_eval_system_event(
            1,
            "request.preflight",
            "error",
            "Agy request failed before Jev classification",
            None,
            None,
            Some(diagnostic.to_string()),
            completed_at,
        ),
    ];
    crate::llm_eval::build_codex_eval_run(
        seed,
        "",
        started_at,
        completed_at,
        "failed_pre_dispatch",
        None,
        Some(diagnostic.to_string()),
        events,
        Vec::new(),
    )
}

fn agy_provider_failure_eval_run(
    mut seed: crate::llm_eval::EvalRunSeed,
    prompt: &str,
    external_thread_id: &str,
    turn_id: &str,
    started_at: i64,
    completed_at: i64,
    diagnostic: &str,
) -> crate::llm_eval::EvalRun {
    seed.external_thread_id = external_thread_id.to_string();
    let jev_enabled = seed.jev_route.is_some();
    let delivery_event = agy_eval_system_event(
        0,
        "delivery",
        "started",
        "Agy provider returned a native turn ID",
        None,
        Some(serde_json::json!({ "providerTurnId": turn_id })),
        None,
        started_at,
    );
    let failure_event = agy_eval_system_event(
        0,
        "provider.persistence",
        "error",
        "Agy turn stopped after application persistence failed",
        None,
        None,
        Some(diagnostic.to_string()),
        completed_at,
    );
    let events = if jev_enabled {
        vec![delivery_event, failure_event]
    } else {
        vec![
            agy_eval_system_event(
                0,
                "request",
                "admitted",
                "Agy request admitted",
                Some(serde_json::json!({
                    "promptSha256": format!("sha256:{:x}", sha2::Sha256::digest(prompt.as_bytes())),
                    "promptChars": prompt.chars().count(),
                })),
                None,
                None,
                started_at,
            ),
            delivery_event,
            failure_event,
        ]
    };
    if let Some(route) = seed.jev_route.as_ref() {
        seed.answer_first_required = route.answer_first;
    }
    crate::llm_eval::build_codex_eval_run(
        seed,
        turn_id,
        started_at,
        completed_at,
        "error",
        None,
        Some(diagnostic.to_string()),
        events,
        Vec::new(),
    )
}

fn agy_eval_system_event(
    sequence: u64,
    name: &str,
    state: &str,
    summary: &str,
    input: Option<serde_json::Value>,
    output: Option<serde_json::Value>,
    error: Option<String>,
    occurred_at: i64,
) -> crate::llm_eval::EvalEvent {
    crate::llm_eval::EvalEvent {
        sequence,
        step_index: None,
        kind: crate::llm_eval::EvalEventKind::System,
        state: state.to_string(),
        name: Some(name.to_string()),
        summary: Some(summary.to_string()),
        input: input.map(crate::llm_eval::EvalPayload::new),
        output: output.map(crate::llm_eval::EvalPayload::new),
        error,
        occurred_at,
    }
}

async fn persist_agy_pre_dispatch_failure<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    seed: crate::llm_eval::EvalRunSeed,
    queue_id: &str,
    prompt: &str,
    started_at: i64,
    diagnostic: &str,
    route: Option<&crate::jev_classifier::AcceptedRoute>,
    redaction_secret: Option<&str>,
) {
    let diagnostic = redaction_secret
        .filter(|secret| !secret.is_empty())
        .map(|secret| diagnostic.replace(secret, "[REDACTED]"))
        .unwrap_or_else(|| diagnostic.to_string());
    let run = agy_pre_dispatch_eval_run(
        seed,
        queue_id,
        prompt,
        started_at,
        now_seconds(),
        &diagnostic,
        route,
    );
    if let Err(error) = crate::llm_eval::persist_run(&app.app_data_dir(), &run) {
        state.push_log(format!(
            "[LLM EVAL] Agy pre-dispatch persistence failed: {error}"
        ));
    }
}

async fn persist_agy_preflight_failure<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    seed: crate::llm_eval::EvalRunSeed,
    prompt: &str,
    started_at: i64,
    diagnostic: &str,
    redaction_secret: Option<&str>,
) {
    let diagnostic = redaction_secret
        .filter(|secret| !secret.is_empty())
        .map(|secret| diagnostic.replace(secret, "[REDACTED]"))
        .unwrap_or_else(|| diagnostic.to_string());
    let run = agy_preflight_failure_eval_run(seed, prompt, started_at, now_seconds(), &diagnostic);
    if let Err(error) = crate::llm_eval::persist_run(&app.app_data_dir(), &run) {
        state.push_log(format!(
            "[LLM EVAL] Agy preflight persistence failed: {error}"
        ));
    }
}

async fn persist_agy_provider_failure<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    seed: crate::llm_eval::EvalRunSeed,
    prompt: &str,
    external_thread_id: &str,
    turn_id: &str,
    started_at: i64,
    diagnostic: &str,
    redaction_secret: Option<&str>,
) {
    let diagnostic = redaction_secret
        .filter(|secret| !secret.is_empty())
        .map(|secret| diagnostic.replace(secret, "[REDACTED]"))
        .unwrap_or_else(|| diagnostic.to_string());
    let mut seed = seed;
    seed.external_thread_id = external_thread_id.to_string();
    let run = agy_provider_failure_eval_run(
        seed,
        prompt,
        external_thread_id,
        turn_id,
        started_at,
        now_seconds(),
        &diagnostic,
    );
    if let Err(error) = crate::llm_eval::persist_run(&app.app_data_dir(), &run) {
        state.push_log(format!(
            "[LLM EVAL] Agy provider failure persistence failed: {error}"
        ));
    }
}

fn agy_eval_route(
    route: &crate::jev_classifier::AcceptedRoute,
    ceiling: Option<String>,
) -> crate::llm_eval::EvalJevRoute {
    crate::llm_eval::EvalJevRoute {
        policy_version: route.policy_version.to_string(),
        intent_confidence_threshold: crate::jev_classifier::ACTION_CONFIDENCE_MIN,
        intent_margin_threshold: crate::jev_classifier::ACTION_MARGIN_MIN,
        model_confidence_threshold: crate::jev_classifier::ACTION_CONFIDENCE_MIN,
        model_margin_threshold: crate::jev_classifier::ACTION_MARGIN_MIN,
        intent_confidence: route.action_confidence,
        intent_probabilities: route.action_probabilities.clone(),
        answer_requested: route.answer_requested,
        answer_first: route.answer_first,
        model_ceiling: ceiling,
        model_confidence: Some(route.model_confidence),
        model_probabilities: route.model_probabilities.clone(),
        model_reason: route.model_reason.clone(),
        context_truncated: route.context_truncated,
        current_prompt_truncated: route.current_prompt_truncated,
        classifier_input_tokens: route.classifier_input_tokens,
        classifier_output_tokens: route.classifier_output_tokens,
        classifier_model: route.classifier_model.clone(),
        classifier_latency_ms: route.classifier_latency_ms,
        model_catalog_version: route.model_catalog_version.clone(),
        model_catalog_valid_until: route.model_catalog_valid_until.clone(),
        native_tool_coverage: crate::llm_eval::unknown_native_tool_coverage(),
    }
}

fn agy_policy_for_route(
    route: Option<&crate::jev_classifier::AcceptedRoute>,
) -> crate::provider_turn::ProviderTurnPolicy {
    route.map_or_else(
        crate::provider_turn::ProviderTurnPolicy::prompt_based,
        |route| crate::provider_turn::ProviderTurnPolicy::routed(route.intent, route.answer_first),
    )
}

async fn agy_classifier_request(
    state: &AppState,
    binding: &AgyProviderBinding,
    head: &crate::contracts::CodexQueuedPrompt,
    prompt: &str,
) -> AppResult<crate::jev_classifier::ClassifierRequest> {
    agy_classifier_request_for(
        state,
        &binding.ecky_thread_id,
        Some(&binding.agy_conversation_id),
        &head.attachments,
        prompt,
    )
    .await
}

async fn agy_classifier_request_for(
    state: &AppState,
    ecky_thread_id: &str,
    conversation_id: Option<&str>,
    attachments: &[Attachment],
    prompt: &str,
) -> AppResult<crate::jev_classifier::ClassifierRequest> {
    let summary = {
        let conn = state.db.lock().await;
        crate::db::get_thread_summary(&conn, ecky_thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .unwrap_or_default()
    };
    Ok(crate::jev_classifier::ClassifierRequest::bounded(
        prompt,
        std::iter::empty(),
        &summary,
        &format!(
            "owned Agy conversation {}; configured model ceiling {}",
            conversation_id.unwrap_or("new conversation"),
            configured_agy_model(state)
                .as_deref()
                .unwrap_or("provider default")
        ),
        attachments
            .iter()
            .map(|attachment| crate::jev_classifier::AttachmentModality {
                kind: attachment.kind.as_str().to_owned(),
                explanation: attachment.explanation.clone(),
            })
            .collect(),
        Vec::new(),
    ))
}

fn require_mcp_endpoint(state: &AppState) -> AppResult<String> {
    let status = state.mcp_status();
    if status.running && !status.endpoint_url.trim().is_empty() {
        return Ok(status.endpoint_url);
    }
    Err(AppError::provider(status.last_startup_error.unwrap_or_else(|| {
        format!(
            "Ecky MCP endpoint {} is not running; Agy provider conversation cannot start without CAD tools.",
            status.endpoint_url
        )
    })))
}

async fn project_title(state: &AppState, ecky_thread_id: &str) -> AppResult<String> {
    let conn = state.db.lock().await;
    crate::db::get_thread_title(&conn, ecky_thread_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| AppError::not_found(format!("Ecky thread {ecky_thread_id} was not found.")))
}

async fn canonical_handoff(state: &AppState, ecky_thread_id: &str) -> String {
    let conn = state.db.lock().await;
    let context =
        crate::context::assemble_context(&conn, Some(ecky_thread_id.to_string()), None, None);
    let provider_dialogue = {
        let stmt = conn.prepare(
            "SELECT role, content
             FROM (
                 SELECT role, content, created_at, id
                 FROM agent_provider_messages
                 WHERE ecky_thread_id = ?1
                 ORDER BY created_at DESC, id DESC
                 LIMIT 4
             )
             ORDER BY created_at ASC, id ASC",
        );
        match stmt {
            Ok(mut stmt) => {
                let rows = stmt
                    .query_map([ecky_thread_id], |row| {
                        let role: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        Ok(format!(
                            "{}: {}",
                            role.to_uppercase(),
                            crate::context::compact_text(&content, 2048)
                        ))
                    })
                    .ok();
                rows.map(|r| {
                    r.filter_map(|res| res.ok())
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
                .unwrap_or_default()
            }
            Err(_) => String::new(),
        }
    };
    let recent_dialogue = if !provider_dialogue.trim().is_empty() {
        provider_dialogue
    } else if !context.recent_dialogue.trim().is_empty() {
        context.recent_dialogue
    } else {
        "[none]".to_string()
    };
    format!(
        "THREAD SUMMARY\n{}\n\nRECENT DIALOGUE\n{}\n\nDESIGN DIGEST\n{}\n\nARTIFACT DIGEST\n{}",
        if context.summary.trim().is_empty() {
            "[none]"
        } else {
            &context.summary
        },
        recent_dialogue,
        if context.design_digest.trim().is_empty() {
            "[none]"
        } else {
            &context.design_digest
        },
        if context.artifact_digest.trim().is_empty() {
            "[none]"
        } else {
            &context.artifact_digest
        },
    )
}

async fn provider_project_cwd<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    ecky_thread_id: &str,
    title: &str,
) -> AppResult<String> {
    let has_version = {
        let conn = state.db.lock().await;
        crate::db::get_thread_latest_version(&conn, ecky_thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .is_some()
    };
    if has_version {
        let exported = crate::mcp::handlers::handle_project_folder_export(
            state,
            app,
            crate::mcp::handlers::ProjectFolderExportRequest {
                identity: crate::mcp::contracts::AgentIdentityOverride::default(),
                thread_id: Some(ecky_thread_id.to_string()),
                message_id: None,
                slug: None,
            },
            &crate::mcp::handlers::AgentContext {
                session_id: format!("agy-provider:{ecky_thread_id}"),
                client_kind: "provider".to_string(),
                host_label: "Ecky".to_string(),
                agent_label: "Agy".to_string(),
                llm_model_id: None,
                llm_model_label: None,
            },
        )
        .await?;
        return Ok(exported.folder);
    }
    let configured_root = state.config.lock().unwrap().projects_root.clone();
    let slug = crate::project_mirror::project_slug(title, ecky_thread_id);
    let path = crate::project_mirror::project_dir(app, configured_root.as_deref(), &slug)?;
    std::fs::create_dir_all(&path).map_err(|error| {
        AppError::persistence(format!(
            "Failed to create Ecky Agy workspace '{}': {error}",
            path.display()
        ))
    })?;
    Ok(path.to_string_lossy().into_owned())
}

const AGY_PROVIDER_TOOL_GUIDE: &str = r#"# Ecky provider tool guide

- Obey the `[TURN POLICY]` in the current prompt. It is turn-scoped and overrides unfinished work from prior turns.
- `ANSWER` and `CLARIFY`: do not call tools or inspect/edit project files. Answer or ask one concise question immediately.
- `INSPECT`: use only advertised read-only tools. Never edit project files.
- `MODIFY`: project writes are allowed only for the bounded change explicitly requested in the current user message.
- Provider target is already pre-bound by Ecky. Do not call `thread_borrow` for the assigned thread. Use it only when the user explicitly asks to switch to another existing Ecky thread.
- For `INSPECT` or `MODIFY`, first call `workspace_overview`. Confirm `defaultTarget.threadId` matches the assigned thread.
- Before editing, read `agentBrief.primaryGuideUri` and every URI in `agentBrief.mustRead` through MCP resources. Use `capability_search` and `capability_enable` before guessing specialist tool names.
- When `defaultTarget.sourcePath` exists, inspect and edit that exact file. The watcher appends, validates, and previews settled changes. Do not export first or call a manual commit/finalize operation.
- Follow inspect -> validate -> preview -> verify. Prefer MCP/normal file tools. Browser work is only for explicit web or UI requests.
- Stop after a bounded repair attempt. Surface exact diagnostics instead of repeating the same tool/action loop.
"#;

#[derive(Debug)]
struct AgyWorkspaceMaterialization {
    config_path: String,
    guide_path: String,
    bound_endpoint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgyPromptPhase {
    Bootstrap,
    Resume,
    Continuation,
}

#[cfg(test)]
fn materialize_agy_mcp_config(
    cwd: &str,
    endpoint: &str,
    ecky_thread_id: &str,
) -> AppResult<AgyWorkspaceMaterialization> {
    materialize_agy_mcp_config_with_policy(
        cwd,
        endpoint,
        ecky_thread_id,
        crate::provider_turn::ProviderTurnPolicy::for_intent(
            crate::provider_turn::ProviderTurnIntent::Modify,
        ),
    )
}

fn materialize_agy_mcp_config_with_policy(
    cwd: &str,
    endpoint: &str,
    ecky_thread_id: &str,
    policy: crate::provider_turn::ProviderTurnPolicy,
) -> AppResult<AgyWorkspaceMaterialization> {
    let agents_dir = Path::new(cwd).join(".agents");
    std::fs::create_dir_all(&agents_dir).map_err(|error| {
        AppError::persistence(format!(
            "Failed to create Agy workspace config directory '{}': {error}",
            agents_dir.display()
        ))
    })?;
    let plugin_dir = agents_dir.join("plugins").join("ecky-provider");
    let rules_dir = plugin_dir.join("rules");
    std::fs::create_dir_all(&rules_dir).map_err(|error| {
        AppError::persistence(format!(
            "Failed to create Agy provider plugin directory '{}': {error}",
            rules_dir.display()
        ))
    })?;
    let path = plugin_dir.join("mcp_config.json");
    let manifest_path = plugin_dir.join("plugin.json");
    let guide_path = rules_dir.join("AGENTS.md");
    let bound_endpoint =
        crate::mcp::server::provider_bound_endpoint_with_policy(endpoint, ecky_thread_id, policy);
    let mut root = if path.exists() {
        let raw = std::fs::read_to_string(&path).map_err(|error| {
            AppError::persistence(format!("Failed to read '{}': {error}", path.display()))
        })?;
        serde_json::from_str::<serde_json::Value>(&raw).map_err(|error| {
            AppError::validation(format!(
                "Existing Agy MCP config '{}' is invalid JSON: {error}",
                path.display()
            ))
        })?
    } else {
        serde_json::json!({})
    };
    let root_object = root.as_object_mut().ok_or_else(|| {
        AppError::validation(format!(
            "Existing Agy MCP config '{}' must contain a JSON object.",
            path.display()
        ))
    })?;
    let servers = root_object
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| AppError::validation("Agy mcpServers must be a JSON object."))?;
    servers.insert(
        "ecky_mcp".to_string(),
        serde_json::json!({ "serverUrl": bound_endpoint }),
    );
    let encoded = serde_json::to_string_pretty(&root).map_err(|error| {
        AppError::persistence(format!("Failed to encode Agy MCP config: {error}"))
    })?;
    std::fs::write(&path, format!("{encoded}\n")).map_err(|error| {
        AppError::persistence(format!("Failed to write '{}': {error}", path.display()))
    })?;
    std::fs::write(
        &manifest_path,
        "{\n  \"name\": \"ecky-provider\",\n  \"disabled\": false\n}\n",
    )
    .map_err(|error| {
        AppError::persistence(format!(
            "Failed to write '{}': {error}",
            manifest_path.display()
        ))
    })?;
    std::fs::write(&guide_path, AGY_PROVIDER_TOOL_GUIDE).map_err(|error| {
        AppError::persistence(format!(
            "Failed to write '{}': {error}",
            guide_path.display()
        ))
    })?;
    Ok(AgyWorkspaceMaterialization {
        config_path: path.to_string_lossy().into_owned(),
        guide_path: guide_path.to_string_lossy().into_owned(),
        bound_endpoint,
    })
}

#[cfg(test)]
fn provider_prompt(
    phase: AgyPromptPhase,
    ecky_thread_id: &str,
    title: &str,
    cwd: &str,
    endpoint: &str,
    mcp_config_path: &str,
    tool_guide_path: &str,
    handoff: &str,
    prompt: &str,
    attachments: &[Attachment],
) -> String {
    provider_prompt_with_policy(
        phase,
        ecky_thread_id,
        title,
        cwd,
        endpoint,
        mcp_config_path,
        tool_guide_path,
        handoff,
        prompt,
        attachments,
        crate::provider_turn::ProviderTurnPolicy::for_intent(
            crate::provider_turn::ProviderTurnIntent::Modify,
        ),
    )
}

fn provider_prompt_with_policy(
    phase: AgyPromptPhase,
    ecky_thread_id: &str,
    title: &str,
    cwd: &str,
    endpoint: &str,
    mcp_config_path: &str,
    tool_guide_path: &str,
    handoff: &str,
    prompt: &str,
    attachments: &[Attachment],
    policy: crate::provider_turn::ProviderTurnPolicy,
) -> String {
    let attachment_manifest = build_agy_attachment_manifest(attachments);
    let policy_prompt = policy.prompt_contract();
    if phase == AgyPromptPhase::Continuation {
        return format!(
            "[ECKY USER TURN v{}]\n{policy_prompt}\nThe current user message is the only authority for this turn. Do not resume unfinished work from earlier turns unless this message explicitly requests it. Continue the already pre-bound Ecky provider conversation. Do not call `thread_borrow`.\n\n[USER MESSAGE]\n{prompt}{attachment_manifest}",
            agy_provider::AGY_BOOTSTRAP_VERSION,
        );
    }
    let phase_label = match phase {
        AgyPromptPhase::Bootstrap => "THREAD BOOTSTRAP",
        AgyPromptPhase::Resume => "CONTEXT REFRESH",
        AgyPromptPhase::Continuation => unreachable!(),
    };
    format!(
        "[ECKY {phase_label} v{}]\n{policy_prompt}\nYou are Agy inside Ecky CAD. This provider conversation belongs only to Ecky thread {ecky_thread_id} ({title}).\nCanonical workspace: {cwd}\nWorkspace MCP config: {mcp_config_path}\nRequired MCP endpoint: {endpoint} under ecky_mcp. The workspace plugin overrides any same-named global server for this project.\nThis MCP connection is already pre-bound to thread {ecky_thread_id}. Do not call `thread_borrow`; it is only for an intentional switch to another existing target.\nRead the provider tool guide first: {tool_guide_path}. Only for MODIFY, call `workspace_overview`, verify its target, and read `agentBrief.primaryGuideUri` plus every URI in `agentBrief.mustRead` before editing.\nUse MCP inspect -> validate -> preview -> verify only for MODIFY; the bound file watcher creates the version, so do not call a manual commit/finalize operation. Never invent thread ids or import foreign conversations. Treat the context below as canonical across API/MCP/Codex/Agy switching.\nWhen useful, cite the bound source in the user-facing answer as `[model.ecky]({cwd}/model.ecky:LINE)` so Ecky can open the exact line. Do not include internal `messageId` or `modelId` fields in the user-facing answer; keep those identifiers only in internal tool evidence.\nEarlier conversation history can be inspected anytime via the MCP tool `thread_messages_get`.\n\n{handoff}\n\n[USER MESSAGE]\n{prompt}{attachment_manifest}",
        agy_provider::AGY_BOOTSTRAP_VERSION,
    )
}

fn build_agy_attachment_manifest(attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let entries = attachments
        .iter()
        .map(|attachment| {
            let label = if attachment.name.trim().is_empty() {
                attachment.kind.as_str()
            } else {
                attachment.name.trim()
            };
            let location = if attachment.path.trim().is_empty() {
                "inline payload (prepare attachment for an absolute path)".to_string()
            } else {
                attachment.path.trim().to_string()
            };
            format!("- {label} [{}]: {location}", attachment.kind.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n\n[ATTACHMENT MANIFEST]\n{entries}")
}

async fn binding_for(state: &AppState, ecky_thread_id: &str) -> AppResult<AgyProviderBinding> {
    let conn = state.db.lock().await;
    agy_provider::get_binding(&conn, ecky_thread_id)?.ok_or_else(|| {
        AppError::not_found(format!(
            "Ecky thread {ecky_thread_id} has no owned Agy conversation."
        ))
    })
}

pub(crate) async fn activate_bound_writer(
    _state: &AppState,
    _ecky_thread_id: &str,
) -> AppResult<()> {
    // Agy `--conversation` is an active resume, not a passive subscription.
    // Writer acquisition therefore happens only while delivering a claimed prompt.
    Ok(())
}

async fn snapshot_for(
    state: &AppState,
    binding: AgyProviderBinding,
    cursor: Option<&str>,
) -> AppResult<AgyProviderSnapshot> {
    let (page, queue) = {
        let conn = state.db.lock().await;
        (
            agy_provider::message_page(&conn, &binding.ecky_thread_id, cursor)?,
            agy_provider::list_queue(&conn, &binding.ecky_thread_id)?,
        )
    };
    let mut runtime = state
        .agy_provider
        .runtime(&binding.agy_conversation_id)
        .await;
    if runtime.error.is_none() {
        runtime.error = queue
            .first()
            .filter(|item| item.status == "failed")
            .and_then(|item| item.error.clone());
    }
    Ok(AgyProviderSnapshot {
        messages: page.messages,
        next_cursor: page.next_cursor,
        backwards_cursor: page.backwards_cursor,
        runtime,
        live_messages: state
            .agy_provider
            .live_messages(&binding.agy_conversation_id)
            .await,
        turn_traces: state
            .agy_provider
            .turn_traces(&binding.agy_conversation_id)
            .await,
        queue,
        binding,
        capabilities: ProviderCapabilities {
            steer: false,
            stop: true,
        },
    })
}

async fn emit_provider_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    binding: &AgyProviderBinding,
    method: &str,
) {
    let _ = app.emit(
        "agy-provider-updated",
        serde_json::json!({
            "conversationId": binding.agy_conversation_id,
            "method": method,
        }),
    );
}

fn spawn_turn_finalizer<R: tauri::Runtime>(
    state: AppState,
    app: tauri::AppHandle<R>,
    binding: AgyProviderBinding,
    queue_id: String,
    eval_dispatch: AgyEvalDispatch,
    result: tokio::sync::oneshot::Receiver<AppResult<agy_provider::AgyTurnResult>>,
) {
    tauri::async_runtime::spawn(async move {
        let outcome = match result.await {
            Ok(result) => result,
            Err(_) => Err(AppError::provider(
                "Agy turn result channel closed unexpectedly.",
            )),
        };
        let recovered_failure = if outcome.is_err() {
            state
                .agy_provider
                .take_failed_turn_result(
                    &binding.agy_conversation_id,
                    &eval_dispatch.fallback_turn_id,
                )
                .await
        } else {
            None
        };
        {
            let conn = state.db.lock().await;
            let answer_result = outcome.as_ref().ok().or(recovered_failure.as_ref());
            if let Some(result) = answer_result {
                if let Err(error) =
                    agy_provider::persist_terminal_answer(&conn, &binding.ecky_thread_id, result)
                {
                    state.push_log(format!(
                        "[AGY] terminal answer persistence failed: {}",
                        codex_takeover::error_text(&error)
                    ));
                }
            }
            match &outcome {
                Ok(result) if result.status == "SUCCESS" => {
                    let _ = codex_takeover::complete_queue_item(&conn, &queue_id);
                    let title = crate::db::get_thread_title(&conn, &binding.ecky_thread_id)
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| binding.label.clone());
                    let ecky_messages =
                        crate::db::get_thread_messages_for_context(&conn, &binding.ecky_thread_id)
                            .unwrap_or_default();
                    let canonical = crate::context::build_thread_summary(&title, &ecky_messages);
                    if let Ok(page) =
                        agy_provider::message_page(&conn, &binding.ecky_thread_id, None)
                    {
                        let handoff = codex_takeover::build_provider_handoff_summary_for(
                            "AGY",
                            &canonical,
                            &page.messages,
                        );
                        let _ = crate::db::update_thread_summary(
                            &conn,
                            &binding.ecky_thread_id,
                            &handoff,
                        );
                    }
                }
                Ok(result) if matches!(result.status.as_str(), "CANCELED" | "INTERRUPTED") => {
                    let _ = codex_takeover::complete_queue_item(&conn, &queue_id);
                }
                Ok(result) => {
                    let raw = result
                        .error
                        .clone()
                        .unwrap_or_else(|| result.status.clone());
                    let _ = codex_takeover::fail_queue_item(&conn, &queue_id, &raw, now_seconds());
                }
                Err(error) => {
                    let raw = codex_takeover::error_text(&error);
                    let _ = codex_takeover::fail_queue_item(&conn, &queue_id, &raw, now_seconds());
                }
            }
        }
        let started_at = outcome
            .as_ref()
            .map(|result| result.started_at)
            .unwrap_or_else(|_| {
                recovered_failure
                    .as_ref()
                    .map(|result| result.started_at)
                    .unwrap_or(eval_dispatch.fallback_started_at)
            });
        let completed_at = outcome
            .as_ref()
            .map(|result| result.completed_at)
            .unwrap_or_else(|_| {
                recovered_failure
                    .as_ref()
                    .map(|result| result.completed_at)
                    .unwrap_or_else(now_seconds)
            });
        let versions = {
            let conn = state.db.lock().await;
            crate::llm_eval::version_outcomes_for_window(
                &conn,
                &binding.ecky_thread_id,
                eval_dispatch.seed.starting_version_id.as_deref(),
                started_at,
                completed_at,
            )
            .unwrap_or_else(|error| {
                state.push_log(format!("[LLM EVAL] version linkage failed: {error}"));
                Vec::new()
            })
        };
        let run = match (outcome, recovered_failure) {
            (Ok(result), _) | (Err(_), Some(result)) => {
                crate::llm_eval::build_agy_eval_run(eval_dispatch.seed, result, versions)
            }
            (Err(error), None) => crate::llm_eval::build_failed_eval_run(
                eval_dispatch.seed,
                eval_dispatch.fallback_turn_id,
                started_at,
                completed_at,
                codex_takeover::error_text(&error),
                versions,
            ),
        };
        match crate::llm_eval::persist_run(&app.app_data_dir(), &run) {
            Ok(files) => {
                state.push_log(format!("[LLM EVAL] persisted {}", files.run_dir.display()))
            }
            Err(error) => state.push_log(format!("[LLM EVAL] persistence failed: {error}")),
        }
        emit_provider_update(&app, &binding, "turn/terminal").await;
        state
            .clear_provider_turn_policy(&binding.ecky_thread_id)
            .await;
        let _ = dispatch_queue_for(&app, &state, &binding).await;
        emit_provider_update(&app, &binding, "queue/dispatched").await;
        AGY_QUEUE_WAKE.notify_one();
    });
}

async fn dispatch_queue_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    binding: &AgyProviderBinding,
) -> AppResult<()> {
    dispatch_queue_for_impl(app, state, binding, None).await
}

#[cfg(test)]
async fn dispatch_queue_for_with_classifier<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    binding: &AgyProviderBinding,
    classifier: &dyn crate::jev_classifier::TurnClassifier,
) -> AppResult<()> {
    dispatch_queue_for_impl(app, state, binding, Some(classifier)).await
}

async fn dispatch_queue_for_impl<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    binding: &AgyProviderBinding,
    classifier_override: Option<&dyn crate::jev_classifier::TurnClassifier>,
) -> AppResult<()> {
    let runtime = state
        .agy_provider
        .runtime(&binding.agy_conversation_id)
        .await;
    if runtime.active_turn_id.is_some() || runtime.phase == "stopping" {
        return Ok(());
    }
    let head = {
        let conn = state.db.lock().await;
        agy_provider::queue_head(&conn, &binding.ecky_thread_id)?
    };
    let Some(head) = head else { return Ok(()) };
    if head.status != "queued" {
        return Ok(());
    }

    let raw_prompt = head.prompt_text.trim();
    if raw_prompt == "/compact" {
        let now = now_seconds();
        let claimed = {
            let conn = state.db.lock().await;
            codex_takeover::claim_queue_item(&conn, &head.id, now)?
        };
        if !claimed {
            return Ok(());
        }
        state
            .agy_provider
            .discard_conversation_session(&binding.agy_conversation_id)
            .await;
        let new_conv_id = uuid::Uuid::new_v4().to_string();
        let updated_binding = {
            let conn = state.db.lock().await;
            let b = agy_provider::rotate_binding(
                &conn,
                &binding.ecky_thread_id,
                &new_conv_id,
                "manual compaction",
                now,
            )?;
            agy_provider::insert_message(
                &conn,
                &binding.ecky_thread_id,
                &binding.agy_conversation_id,
                "user",
                "/compact",
                "success",
                now,
            )?;
            agy_provider::insert_message(
                &conn,
                &binding.ecky_thread_id,
                &new_conv_id,
                "assistant",
                "Сессия Antigravity CLI скомпактизирована. Контекст очищен, история сохранена в Ecky и доступна агенту через `thread_messages_get`.",
                "success",
                now,
            )?;
            codex_takeover::complete_queue_item(&conn, &head.id)?;
            b
        };
        emit_provider_update(&app, &updated_binding, "binding/compacted").await;
        emit_provider_update(&app, &updated_binding, "queue/dispatched").await;
        return Ok(());
    }

    let (force_compaction, effective_prompt_text) =
        if let Some(stripped) = raw_prompt.strip_prefix("/compact ") {
            (true, stripped.trim().to_string())
        } else {
            (false, head.prompt_text.clone())
        };

    let conversation_messages_count = {
        let conn = state.db.lock().await;
        agy_provider::count_conversation_messages(
            &conn,
            &binding.ecky_thread_id,
            &binding.agy_conversation_id,
        )?
    };
    let needs_compaction = force_compaction || conversation_messages_count >= 20;

    let claimed = {
        let conn = state.db.lock().await;
        codex_takeover::claim_queue_item(&conn, &head.id, now_seconds())?
    };
    if !claimed {
        return Ok(());
    }
    let config_snapshot = state.config.lock().unwrap().clone();
    let eval_run_id = uuid::Uuid::new_v4().to_string();
    let eval_started_at = now_seconds();
    let eval_starting = match eval_starting_identity_for(state, &binding.ecky_thread_id).await {
        Ok(starting) => starting,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                let seed = eval_run_seed(
                    binding,
                    &eval_run_id,
                    &effective_prompt_text,
                    configured_agy_model(state),
                    None,
                    ProviderTurnIntent::Clarify,
                    None,
                );
                persist_agy_preflight_failure(
                    app,
                    state,
                    seed,
                    &effective_prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            let conn = state.db.lock().await;
            codex_takeover::fail_queue_item(
                &conn,
                &head.id,
                &codex_takeover::error_text(&error),
                now_seconds(),
            )?;
            return Err(error);
        }
    };
    let mut eval_seed = eval_run_seed(
        binding,
        &eval_run_id,
        &effective_prompt_text,
        configured_agy_model(state),
        eval_starting,
        ProviderTurnIntent::Clarify,
        None,
    );
    let request = if config_snapshot.jev_classifier.enabled {
        match agy_classifier_request(state, binding, &head, &effective_prompt_text).await {
            Ok(request) => Some(request),
            Err(error) => {
                persist_agy_preflight_failure(
                    app,
                    state,
                    eval_seed.clone(),
                    &effective_prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
                let conn = state.db.lock().await;
                codex_takeover::fail_queue_item(
                    &conn,
                    &head.id,
                    &codex_takeover::error_text(&error),
                    now_seconds(),
                )?;
                return Err(error);
            }
        }
    } else {
        None
    };
    let route_result = match request {
        Some(request) => Some(match classifier_override {
            Some(classifier) => classifier.classify(request).await,
            None => {
                crate::jev_classifier::classify_configured_request(
                    &config_snapshot.jev_classifier,
                    request,
                )
                .await
            }
        }),
        None => None,
    };
    let route = match route_result {
        Some(Ok(route)) => Some(route),
        Some(Err(error)) => {
            persist_agy_pre_dispatch_failure(
                app,
                state,
                eval_seed.clone(),
                &head.id,
                &effective_prompt_text,
                eval_started_at,
                &codex_takeover::error_text(&error),
                None,
                Some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await;
            let conn = state.db.lock().await;
            codex_takeover::fail_queue_item(
                &conn,
                &head.id,
                &codex_takeover::error_text(&error),
                now_seconds(),
            )?;
            return Err(error);
        }
        None => None,
    };
    if let Some(route) = route.as_ref() {
        eval_seed.turn_intent = route.intent;
        eval_seed.answer_first_required = route.answer_first;
        eval_seed.jev_route = Some(agy_eval_route(route, configured_agy_model(state)));
    }
    let current_config = state.config.lock().unwrap().clone();
    let current_binding = match binding_for(state, &binding.ecky_thread_id).await {
        Ok(binding) => binding,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_pre_dispatch_failure(
                    app,
                    state,
                    eval_seed.clone(),
                    &head.id,
                    &effective_prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    route.as_ref(),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            let conn = state.db.lock().await;
            codex_takeover::fail_queue_item(
                &conn,
                &head.id,
                &codex_takeover::error_text(&error),
                now_seconds(),
            )?;
            return Err(error);
        }
    };
    let current_runtime = state
        .agy_provider
        .runtime(&binding.agy_conversation_id)
        .await;
    if current_binding.agy_conversation_id != binding.agy_conversation_id
        || current_config.jev_classifier.enabled != config_snapshot.jev_classifier.enabled
        || current_config.jev_classifier.api_key != config_snapshot.jev_classifier.api_key
        || current_config.provider_models.agy != config_snapshot.provider_models.agy
        || current_runtime.phase == "stopping"
        || current_runtime.active_turn_id.is_some()
    {
        let error = AppError::conflict(
            "Jev route became stale before Agy delivery; retry this queued turn.",
        );
        if config_snapshot.jev_classifier.enabled {
            persist_agy_pre_dispatch_failure(
                app,
                state,
                eval_seed.clone(),
                &head.id,
                &effective_prompt_text,
                eval_started_at,
                &codex_takeover::error_text(&error),
                route.as_ref(),
                Some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await;
        }
        let conn = state.db.lock().await;
        codex_takeover::fail_queue_item(
            &conn,
            &head.id,
            &codex_takeover::error_text(&error),
            now_seconds(),
        )?;
        return Err(error);
    }

    let policy = agy_policy_for_route(route.as_ref());
    let intent = policy.intent();
    let endpoint = match require_mcp_endpoint(state) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_pre_dispatch_failure(
                    app,
                    state,
                    eval_seed.clone(),
                    &head.id,
                    &effective_prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    route.as_ref(),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            let conn = state.db.lock().await;
            codex_takeover::fail_queue_item(
                &conn,
                &head.id,
                &codex_takeover::error_text(&error),
                now_seconds(),
            )?;
            return Err(error);
        }
    };
    let requested_bound_endpoint = crate::mcp::server::provider_bound_endpoint_with_policy(
        &endpoint,
        &binding.ecky_thread_id,
        policy,
    );
    let warm_session = if needs_compaction {
        state
            .agy_provider
            .discard_conversation_session(&binding.agy_conversation_id)
            .await;
        false
    } else {
        state
            .agy_provider
            .has_compatible_session_with_policy(
                &binding.agy_conversation_id,
                configured_agy_model(state).as_deref(),
                Some(&requested_bound_endpoint),
                policy,
            )
            .await
    };
    let prompt_phase = if needs_compaction {
        AgyPromptPhase::Bootstrap
    } else if warm_session {
        AgyPromptPhase::Continuation
    } else {
        AgyPromptPhase::Resume
    };
    let handoff = if warm_session && intent == crate::provider_turn::ProviderTurnIntent::Modify {
        String::new()
    } else {
        canonical_handoff(state, &binding.ecky_thread_id).await
    };
    let workspace = match materialize_agy_mcp_config_with_policy(
        &binding.cwd,
        &endpoint,
        &binding.ecky_thread_id,
        policy,
    ) {
        Ok(workspace) => workspace,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_pre_dispatch_failure(
                    app,
                    state,
                    eval_seed.clone(),
                    &head.id,
                    &effective_prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    route.as_ref(),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            let conn = state.db.lock().await;
            codex_takeover::fail_queue_item(
                &conn,
                &head.id,
                &codex_takeover::error_text(&error),
                now_seconds(),
            )?;
            return Err(error);
        }
    };
    let prompt = provider_prompt_with_policy(
        prompt_phase,
        &binding.ecky_thread_id,
        &binding.label,
        &binding.cwd,
        &workspace.bound_endpoint,
        &workspace.config_path,
        &workspace.guide_path,
        &handoff,
        &effective_prompt_text,
        &head.attachments,
        policy,
    );
    let model = route
        .as_ref()
        .and_then(|route| route.model.clone())
        .or_else(|| configured_agy_model(state));
    eval_seed.model = model.clone();
    eval_seed.turn_intent = intent;
    let fallback_started_at = now_seconds();
    let delivery_claimed = {
        let conn = state.db.lock().await;
        codex_takeover::begin_queue_delivery(&conn, &head.id, now_seconds())?
    };
    if !delivery_claimed {
        if config_snapshot.jev_classifier.enabled {
            persist_agy_pre_dispatch_failure(
                app,
                state,
                eval_seed,
                &head.id,
                &effective_prompt_text,
                eval_started_at,
                "Queued Agy request was cancelled before provider delivery.",
                route.as_ref(),
                Some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await;
        }
        return Ok(());
    }
    if let Some(route) = route.as_ref() {
        let conn = state.db.lock().await;
        crate::services::jev_classifications::save_accepted(
            &conn,
            &binding.ecky_thread_id,
            "agy",
            &head.id,
            None,
            route.intent,
            &route.action_probabilities,
            now_seconds(),
        )?;
        let _ = app.emit(
            "jev-classification-accepted",
            serde_json::json!({"threadId": binding.ecky_thread_id}),
        );
    }
    state
        .set_provider_turn_policy(&binding.ecky_thread_id, policy)
        .await;
    let started = if needs_compaction {
        match state
            .agy_provider
            .start_new_turn_with_policy(
                &binding.cwd,
                &prompt,
                model.as_deref(),
                Some(&workspace.bound_endpoint),
                policy,
            )
            .await
        {
            Ok(started) => started,
            Err(error) => {
                if config_snapshot.jev_classifier.enabled {
                    persist_agy_pre_dispatch_failure(
                        app,
                        state,
                        eval_seed.clone(),
                        &head.id,
                        &effective_prompt_text,
                        eval_started_at,
                        &codex_takeover::error_text(&error),
                        route.as_ref(),
                        Some(config_snapshot.jev_classifier.api_key.as_str()),
                    )
                    .await;
                }
                state
                    .clear_provider_turn_policy(&binding.ecky_thread_id)
                    .await;
                let conn = state.db.lock().await;
                codex_takeover::fail_queue_item(
                    &conn,
                    &head.id,
                    &codex_takeover::error_text(&error),
                    now_seconds(),
                )?;
                return Err(error);
            }
        }
    } else {
        match state
            .agy_provider
            .start_turn_with_policy(
                &binding.agy_conversation_id,
                &binding.cwd,
                &prompt,
                model.as_deref(),
                Some(&workspace.bound_endpoint),
                policy,
            )
            .await
        {
            Ok(started) => started,
            Err(error) => {
                if config_snapshot.jev_classifier.enabled {
                    persist_agy_pre_dispatch_failure(
                        app,
                        state,
                        eval_seed.clone(),
                        &head.id,
                        &effective_prompt_text,
                        eval_started_at,
                        &codex_takeover::error_text(&error),
                        route.as_ref(),
                        Some(config_snapshot.jev_classifier.api_key.as_str()),
                    )
                    .await;
                }
                state
                    .clear_provider_turn_policy(&binding.ecky_thread_id)
                    .await;
                let conn = state.db.lock().await;
                codex_takeover::fail_queue_item(
                    &conn,
                    &head.id,
                    &codex_takeover::error_text(&error),
                    now_seconds(),
                )?;
                return Err(error);
            }
        }
    };

    let active_binding = if needs_compaction {
        let conn = state.db.lock().await;
        match agy_provider::rotate_binding(
            &conn,
            &binding.ecky_thread_id,
            &started.conversation_id,
            if force_compaction {
                "manual compaction"
            } else {
                "auto compaction threshold"
            },
            now_seconds(),
        ) {
            Ok(binding) => binding,
            Err(error) => {
                drop(conn);
                if config_snapshot.jev_classifier.enabled {
                    persist_agy_provider_failure(
                        app,
                        state,
                        eval_seed.clone(),
                        &effective_prompt_text,
                        &started.conversation_id,
                        &started.turn_id,
                        eval_started_at,
                        &codex_takeover::error_text(&error),
                        Some(config_snapshot.jev_classifier.api_key.as_str()),
                    )
                    .await;
                }
                let _ = state
                    .agy_provider
                    .stop_turn(&started.conversation_id, &started.turn_id)
                    .await;
                state
                    .clear_provider_turn_policy(&binding.ecky_thread_id)
                    .await;
                return Err(error);
            }
        }
    } else {
        binding.clone()
    };

    let persistence_result = {
        let conn = state.db.lock().await;
        (|| -> AppResult<()> {
            agy_provider::record_process_lease(
                &conn,
                &head.id,
                &started.conversation_id,
                &started.process,
                now_seconds(),
            )?;
            agy_provider::insert_message_with_id_and_attachments(
                &conn,
                &format!("agy:user:{}", head.id),
                &active_binding.ecky_thread_id,
                &active_binding.agy_conversation_id,
                "user",
                &effective_prompt_text,
                &head.attachments,
                "success",
                head.created_at,
            )?;
            if let Some(route) = route.as_ref() {
                crate::services::jev_classifications::save_accepted(
                    &conn,
                    &binding.ecky_thread_id,
                    "agy",
                    &head.id,
                    Some(&format!("agy:user:{}", head.id)),
                    route.intent,
                    &route.action_probabilities,
                    now_seconds(),
                )?;
            }
            Ok(())
        })()
    };
    if let Err(error) = persistence_result {
        if config_snapshot.jev_classifier.enabled {
            persist_agy_provider_failure(
                app,
                state,
                eval_seed.clone(),
                &effective_prompt_text,
                &started.conversation_id,
                &started.turn_id,
                eval_started_at,
                &codex_takeover::error_text(&error),
                Some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await;
        }
        let _ = state
            .agy_provider
            .stop_turn(&started.conversation_id, &started.turn_id)
            .await;
        state
            .clear_provider_turn_policy(&binding.ecky_thread_id)
            .await;
        let conn = state.db.lock().await;
        codex_takeover::fail_queue_item(
            &conn,
            &head.id,
            &codex_takeover::error_text(&error),
            now_seconds(),
        )?;
        return Err(error);
    }
    if route.is_some() {
        let _ = app.emit(
            "jev-classification-accepted",
            serde_json::json!({"threadId": binding.ecky_thread_id}),
        );
    }
    let eval_dispatch = AgyEvalDispatch {
        seed: eval_seed,
        fallback_turn_id: started.turn_id.clone(),
        fallback_started_at,
    };
    spawn_turn_finalizer(
        state.clone(),
        app.clone(),
        active_binding,
        head.id,
        eval_dispatch,
        started.result,
    );
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn compact_agy_provider(
    ecky_thread_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> AppResult<AgyProviderSnapshot> {
    require_agy_provider_mode(&state)?;
    let binding = binding_for(&state, &ecky_thread_id).await?;
    let runtime = state
        .agy_provider
        .runtime(&binding.agy_conversation_id)
        .await;
    if runtime.active_turn_id.is_some() || runtime.phase == "stopping" {
        return Err(AppError::conflict(
            "Cannot compact while a turn is active or stopping.",
        ));
    }
    state
        .agy_provider
        .discard_conversation_session(&binding.agy_conversation_id)
        .await;
    let new_conv_id = uuid::Uuid::new_v4().to_string();
    let now = now_seconds();
    let updated_binding = {
        let conn = state.db.lock().await;
        let b = agy_provider::rotate_binding(
            &conn,
            &ecky_thread_id,
            &new_conv_id,
            "manual compaction",
            now,
        )?;
        agy_provider::insert_message(
            &conn,
            &ecky_thread_id,
            &new_conv_id,
            "assistant",
            "Сессия Antigravity CLI скомпактизирована. Контекст очищен, история сохранена в Ecky и доступна агенту через `thread_messages_get`.",
            "success",
            now,
        )?;
        b
    };
    emit_provider_update(&app, &updated_binding, "binding/compacted").await;
    emit_provider_update(&app, &updated_binding, "queue/dispatched").await;
    snapshot_for(&state, updated_binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_agy_provider(
    ecky_thread_id: String,
    state: State<'_, AppState>,
) -> AppResult<Option<AgyProviderSnapshot>> {
    let binding = {
        let conn = state.db.lock().await;
        agy_provider::get_binding(&conn, &ecky_thread_id)?
    };
    match binding {
        Some(binding) => snapshot_for(&state, binding, None).await.map(Some),
        None => Ok(None),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_agy_provider_messages(
    input: AgyMessagePageInput,
    state: State<'_, AppState>,
) -> AppResult<AgyMessagePage> {
    let conn = state.db.lock().await;
    agy_provider::message_page(&conn, &input.ecky_thread_id, input.cursor.as_deref())
}

#[tauri::command]
#[specta::specta]
pub async fn send_agy_provider_prompt(
    input: AgyPromptInput,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AgyProviderSnapshot> {
    require_agy_provider_mode(&state)?;
    if let Some(binding) = {
        let conn = state.db.lock().await;
        agy_provider::get_binding(&conn, &input.ecky_thread_id)?
    } {
        {
            let conn = state.db.lock().await;
            agy_provider::enqueue_prompt_with_attachments(
                &conn,
                &input.ecky_thread_id,
                &input.prompt_text,
                &input.attachments,
                now_seconds(),
            )?;
        }
        AGY_QUEUE_WAKE.notify_one();
        let dispatch_state = state.inner().clone();
        let dispatch_app = app.clone();
        let dispatch_binding = binding.clone();
        tauri::async_runtime::spawn(async move {
            let _ = dispatch_queue_for(&dispatch_app, &dispatch_state, &dispatch_binding).await;
            emit_provider_update(&dispatch_app, &dispatch_binding, "queue/dispatched").await;
        });
        return snapshot_for(&state, binding, None).await;
    }

    let _creation = AGY_BINDING_CREATE_LOCK.lock().await;
    if let Some(binding) = {
        let conn = state.db.lock().await;
        agy_provider::get_binding(&conn, &input.ecky_thread_id)?
    } {
        drop(_creation);
        return send_existing(input, app, state, binding).await;
    }
    let config_snapshot = state.config.lock().unwrap().clone();
    let eval_run_id = uuid::Uuid::new_v4().to_string();
    let eval_started_at = now_seconds();
    let starting = match eval_starting_identity_for(&state, &input.ecky_thread_id).await {
        Ok(starting) => starting,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                let provisional_binding = AgyProviderBinding {
                    ecky_thread_id: input.ecky_thread_id.clone(),
                    agy_conversation_id: String::new(),
                    label: String::new(),
                    cwd: String::new(),
                    bootstrap_version: agy_provider::AGY_BOOTSTRAP_VERSION,
                    created_at: eval_started_at,
                    updated_at: eval_started_at,
                };
                let seed = eval_run_seed(
                    &provisional_binding,
                    &eval_run_id,
                    &input.prompt_text,
                    configured_agy_model(&state),
                    None,
                    ProviderTurnIntent::Clarify,
                    None,
                );
                persist_agy_preflight_failure(
                    &app,
                    &state,
                    seed,
                    &input.prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            return Err(error);
        }
    };
    let provisional_binding = AgyProviderBinding {
        ecky_thread_id: input.ecky_thread_id.clone(),
        agy_conversation_id: String::new(),
        label: String::new(),
        cwd: String::new(),
        bootstrap_version: agy_provider::AGY_BOOTSTRAP_VERSION,
        created_at: eval_started_at,
        updated_at: eval_started_at,
    };
    let mut eval_seed = eval_run_seed(
        &provisional_binding,
        &eval_run_id,
        &input.prompt_text,
        configured_agy_model(&state),
        starting.clone(),
        ProviderTurnIntent::Clarify,
        None,
    );
    let route = if config_snapshot.jev_classifier.enabled {
        let request = match agy_classifier_request_for(
            state.inner(),
            // Temporary binding facts are sufficient here; no provider conversation exists yet.
            &input.ecky_thread_id,
            None,
            &input.attachments,
            &input.prompt_text,
        )
        .await
        {
            Ok(request) => request,
            Err(error) => {
                persist_agy_preflight_failure(
                    &app,
                    &state,
                    eval_seed.clone(),
                    &input.prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
                return Err(error);
            }
        };
        match crate::jev_classifier::classify_configured_request(
            &config_snapshot.jev_classifier,
            request,
        )
        .await
        {
            Ok(route) => Some(route),
            Err(error) => {
                persist_agy_pre_dispatch_failure(
                    &app,
                    &state,
                    eval_seed.clone(),
                    &eval_run_id,
                    &input.prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    None,
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
                return Err(error);
            }
        }
    } else {
        None
    };
    if let Some(route) = route.as_ref() {
        eval_seed.turn_intent = route.intent;
        eval_seed.answer_first_required = route.answer_first;
        eval_seed.jev_route = Some(agy_eval_route(route, configured_agy_model(&state)));
    }
    let current_config = state.config.lock().unwrap().clone();
    if current_config.jev_classifier.enabled != config_snapshot.jev_classifier.enabled
        || current_config.jev_classifier.api_key != config_snapshot.jev_classifier.api_key
        || current_config.provider_models.agy != config_snapshot.provider_models.agy
    {
        let error = AppError::conflict(
            "Jev route became stale before new Agy conversation delivery; retry this request.",
        );
        if config_snapshot.jev_classifier.enabled {
            persist_agy_pre_dispatch_failure(
                &app,
                &state,
                eval_seed.clone(),
                &eval_run_id,
                &input.prompt_text,
                eval_started_at,
                &codex_takeover::error_text(&error),
                route.as_ref(),
                Some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await;
        }
        return Err(error);
    }
    let endpoint = match require_mcp_endpoint(&state) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_pre_dispatch_failure(
                    &app,
                    &state,
                    eval_seed.clone(),
                    &eval_run_id,
                    &input.prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    route.as_ref(),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            return Err(error);
        }
    };
    let policy = agy_policy_for_route(route.as_ref());
    let intent = policy.intent();
    let title = match project_title(&state, &input.ecky_thread_id).await {
        Ok(title) => title,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_pre_dispatch_failure(
                    &app,
                    &state,
                    eval_seed.clone(),
                    &eval_run_id,
                    &input.prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    route.as_ref(),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            return Err(error);
        }
    };
    let cwd = match provider_project_cwd(&app, &state, &input.ecky_thread_id, &title).await {
        Ok(cwd) => cwd,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_pre_dispatch_failure(
                    &app,
                    &state,
                    eval_seed.clone(),
                    &eval_run_id,
                    &input.prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    route.as_ref(),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            return Err(error);
        }
    };
    let handoff = canonical_handoff(&state, &input.ecky_thread_id).await;
    let workspace = match materialize_agy_mcp_config_with_policy(
        &cwd,
        &endpoint,
        &input.ecky_thread_id,
        policy,
    ) {
        Ok(workspace) => workspace,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_pre_dispatch_failure(
                    &app,
                    &state,
                    eval_seed.clone(),
                    &eval_run_id,
                    &input.prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    route.as_ref(),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            return Err(error);
        }
    };
    let prompt = provider_prompt_with_policy(
        AgyPromptPhase::Bootstrap,
        &input.ecky_thread_id,
        &title,
        &cwd,
        &workspace.bound_endpoint,
        &workspace.config_path,
        &workspace.guide_path,
        &handoff,
        &input.prompt_text,
        &input.attachments,
        policy,
    );
    let model = route
        .as_ref()
        .and_then(|route| route.model.clone())
        .or_else(|| configured_agy_model(&state));
    eval_seed.model = model.clone();
    let fallback_started_at = now_seconds();
    state
        .set_provider_turn_policy(&input.ecky_thread_id, policy)
        .await;
    let started = match state
        .agy_provider
        .start_new_turn_with_policy(
            &cwd,
            &prompt,
            model.as_deref(),
            Some(&workspace.bound_endpoint),
            policy,
        )
        .await
    {
        Ok(started) => started,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_pre_dispatch_failure(
                    &app,
                    &state,
                    eval_seed.clone(),
                    &eval_run_id,
                    &input.prompt_text,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    route.as_ref(),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            state
                .clear_provider_turn_policy(&input.ecky_thread_id)
                .await;
            return Err(error);
        }
    };
    let binding = {
        let conn = state.db.lock().await;
        match agy_provider::bind_owned_conversation(
            &conn,
            &input.ecky_thread_id,
            &started.conversation_id,
            &title,
            &cwd,
            now_seconds(),
        ) {
            Ok(binding) => binding,
            Err(error) => {
                drop(conn);
                if config_snapshot.jev_classifier.enabled {
                    persist_agy_provider_failure(
                        &app,
                        &state,
                        eval_seed.clone(),
                        &input.prompt_text,
                        &started.conversation_id,
                        &started.turn_id,
                        eval_started_at,
                        &codex_takeover::error_text(&error),
                        Some(config_snapshot.jev_classifier.api_key.as_str()),
                    )
                    .await;
                }
                let _ = state
                    .agy_provider
                    .stop_turn(&started.conversation_id, &started.turn_id)
                    .await;
                state
                    .clear_provider_turn_policy(&input.ecky_thread_id)
                    .await;
                return Err(error);
            }
        }
    };
    let queue_result = {
        let conn = state.db.lock().await;
        agy_provider::enqueue_prompt_with_attachments(
            &conn,
            &input.ecky_thread_id,
            &input.prompt_text,
            &input.attachments,
            now_seconds(),
        )
        .and_then(|item| {
            codex_takeover::claim_queue_item(&conn, &item.id, now_seconds())?;
            Ok(item)
        })
    };
    let queue = match queue_result {
        Ok(queue) => queue,
        Err(error) => {
            if config_snapshot.jev_classifier.enabled {
                persist_agy_provider_failure(
                    &app,
                    &state,
                    eval_seed.clone(),
                    &input.prompt_text,
                    &started.conversation_id,
                    &started.turn_id,
                    eval_started_at,
                    &codex_takeover::error_text(&error),
                    Some(config_snapshot.jev_classifier.api_key.as_str()),
                )
                .await;
            }
            let _ = state
                .agy_provider
                .stop_turn(&started.conversation_id, &started.turn_id)
                .await;
            state
                .clear_provider_turn_policy(&input.ecky_thread_id)
                .await;
            return Err(error);
        }
    };
    if let Some(route) = route.as_ref() {
        let conn = state.db.lock().await;
        crate::services::jev_classifications::save_accepted(
            &conn,
            &input.ecky_thread_id,
            "agy",
            &queue.id,
            None,
            route.intent,
            &route.action_probabilities,
            now_seconds(),
        )?;
        let _ = app.emit(
            "jev-classification-accepted",
            serde_json::json!({"threadId": input.ecky_thread_id}),
        );
    }
    let persistence_result = {
        let conn = state.db.lock().await;
        (|| -> AppResult<()> {
            agy_provider::record_process_lease(
                &conn,
                &queue.id,
                &started.conversation_id,
                &started.process,
                now_seconds(),
            )?;
            agy_provider::insert_message_with_id_and_attachments(
                &conn,
                &format!("agy:user:{}", queue.id),
                &binding.ecky_thread_id,
                &binding.agy_conversation_id,
                "user",
                &input.prompt_text,
                &input.attachments,
                "success",
                queue.created_at,
            )?;
            if let Some(route) = route.as_ref() {
                crate::services::jev_classifications::save_accepted(
                    &conn,
                    &input.ecky_thread_id,
                    "agy",
                    &queue.id,
                    Some(&format!("agy:user:{}", queue.id)),
                    route.intent,
                    &route.action_probabilities,
                    now_seconds(),
                )?;
            }
            Ok(())
        })()
    };
    if let Err(error) = persistence_result {
        if config_snapshot.jev_classifier.enabled {
            persist_agy_provider_failure(
                &app,
                &state,
                eval_seed.clone(),
                &input.prompt_text,
                &started.conversation_id,
                &started.turn_id,
                eval_started_at,
                &codex_takeover::error_text(&error),
                Some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await;
        }
        let _ = state
            .agy_provider
            .stop_turn(&started.conversation_id, &started.turn_id)
            .await;
        state
            .clear_provider_turn_policy(&input.ecky_thread_id)
            .await;
        let conn = state.db.lock().await;
        codex_takeover::fail_queue_item(
            &conn,
            &queue.id,
            &codex_takeover::error_text(&error),
            now_seconds(),
        )?;
        return Err(error);
    }
    if route.is_some() {
        let _ = app.emit(
            "jev-classification-accepted",
            serde_json::json!({"threadId": input.ecky_thread_id}),
        );
    }
    let eval_seed = eval_run_seed(
        &binding,
        &eval_run_id,
        &input.prompt_text,
        model,
        starting,
        intent,
        route
            .as_ref()
            .map(|route| agy_eval_route(route, configured_agy_model(&state))),
    );
    let eval_dispatch = AgyEvalDispatch {
        seed: eval_seed,
        fallback_turn_id: started.turn_id.clone(),
        fallback_started_at,
    };
    spawn_turn_finalizer(
        state.inner().clone(),
        app,
        binding.clone(),
        queue.id,
        eval_dispatch,
        started.result,
    );
    snapshot_for(&state, binding, None).await
}

async fn send_existing(
    input: AgyPromptInput,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    binding: AgyProviderBinding,
) -> AppResult<AgyProviderSnapshot> {
    {
        let conn = state.db.lock().await;
        agy_provider::enqueue_prompt_with_attachments(
            &conn,
            &input.ecky_thread_id,
            &input.prompt_text,
            &input.attachments,
            now_seconds(),
        )?;
    }
    let dispatch_state = state.inner().clone();
    let dispatch_app = app.clone();
    let dispatch_binding = binding.clone();
    tauri::async_runtime::spawn(async move {
        let _ = dispatch_queue_for(&dispatch_app, &dispatch_state, &dispatch_binding).await;
    });
    snapshot_for(&state, binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn dispatch_agy_prompt_queue(
    ecky_thread_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AgyProviderSnapshot> {
    let binding = binding_for(&state, &ecky_thread_id).await?;
    dispatch_queue_for(&app, &state, &binding).await?;
    snapshot_for(&state, binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_agy_provider(
    input: AgyStopInput,
    state: State<'_, AppState>,
) -> AppResult<AgyProviderSnapshot> {
    let binding = binding_for(&state, &input.ecky_thread_id).await?;
    state
        .agy_provider
        .stop_turn(&binding.agy_conversation_id, &input.turn_id)
        .await?;
    snapshot_for(&state, binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_agy_queued_prompt(
    ecky_thread_id: String,
    queue_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AgyProviderSnapshot> {
    let binding = binding_for(&state, &ecky_thread_id).await?;
    {
        let conn = state.db.lock().await;
        agy_provider::retry_queue_item(&conn, &ecky_thread_id, &queue_id, now_seconds())?;
    }
    dispatch_queue_for(&app, &state, &binding).await?;
    snapshot_for(&state, binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_agy_queued_prompt(
    ecky_thread_id: String,
    queue_id: String,
    state: State<'_, AppState>,
) -> AppResult<AgyProviderSnapshot> {
    let binding = binding_for(&state, &ecky_thread_id).await?;
    {
        let conn = state.db.lock().await;
        agy_provider::remove_queue_item(&conn, &ecky_thread_id, &queue_id)?;
        crate::services::jev_classifications::discard_unbound(
            &conn,
            &ecky_thread_id,
            "agy",
            &queue_id,
        )?;
    }
    snapshot_for(&state, binding, None).await
}

pub fn initialize_agy_queue_supervisor(state: AppState, app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(AGY_QUEUE_POLL_INTERVAL) => {}
                _ = AGY_QUEUE_WAKE.notified() => {}
            }
            if state.config.lock().unwrap().connection_type.as_deref() != Some("provider:agy") {
                continue;
            }
            let bindings = {
                let conn = state.db.lock().await;
                match agy_provider::pending_queue_bindings(&conn) {
                    Ok(bindings) => bindings,
                    Err(error) => {
                        state.push_log(format!(
                            "[AGY] prompt queue scan failed: {}",
                            codex_takeover::error_text(&error)
                        ));
                        continue;
                    }
                }
            };
            for binding in bindings {
                let dispatch_state = state.clone();
                let dispatch_app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = dispatch_queue_for(&dispatch_app, &dispatch_state, &binding).await;
                    emit_provider_update(&dispatch_app, &binding, "queue/dispatched").await;
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        build_agy_attachment_manifest, dispatch_queue_for_with_classifier,
        materialize_agy_mcp_config, provider_prompt, provider_prompt_with_policy, AgyPromptPhase,
    };
    use crate::contracts::{
        Attachment, AttachmentKind, Config, EngineKind, GeometryBackend, SourceLanguage,
    };
    use crate::provider_turn::{ProviderTurnIntent, ProviderTurnPolicy};

    #[cfg(unix)]
    struct EnvRestore(Vec<(&'static str, Option<std::ffi::OsString>)>);

    #[cfg(unix)]
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    #[cfg(unix)]
    struct CountingClassifier {
        calls: std::sync::atomic::AtomicUsize,
        result: Result<crate::jev_classifier::AcceptedRoute, crate::contracts::AppError>,
    }

    #[cfg(unix)]
    impl crate::jev_classifier::TurnClassifier for CountingClassifier {
        fn classify<'a>(
            &'a self,
            _request: crate::jev_classifier::ClassifierRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::contracts::AppResult<crate::jev_classifier::AcceptedRoute>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.result.clone()
            })
        }
    }

    #[cfg(unix)]
    fn config(jev_enabled: bool, projects_root: &std::path::Path) -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: Default::default(),
            mcp: Default::default(),
            fem_compute: Default::default(),
            has_seen_onboarding: true,
            connection_type: Some("provider:agy".into()),
            provider_models: Default::default(),
            jev_classifier: crate::contracts::JevClassifierConfig {
                enabled: jev_enabled,
                api_key: if jev_enabled {
                    "test-key".into()
                } else {
                    String::new()
                },
            },
            default_engine_kind: EngineKind::Freecad,
            default_source_language: SourceLanguage::LegacyPython,
            default_geometry_backend: GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: Some(projects_root.to_string_lossy().into_owned()),
        }
    }

    #[cfg(unix)]
    fn fake_agy_cli(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(
            path,
            r##"#!/usr/bin/env python3
import json, os, sys
if "--version" in sys.argv:
    print("Antigravity CLI 1.1.15")
    raise SystemExit(0)
with open(os.environ["ECKY_AGY_TEST_STARTS"], "a", encoding="utf-8") as f:
    f.write("started\n")
print(json.dumps({"event":"init","conversation_id":"agy-session-1"}), flush=True)
sys.stdin.readline()
print(json.dumps({"event":"result","result":{"conversation_id":"agy-session-1","status":"SUCCESS","response":"done"}}), flush=True)
"##,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn queued_dispatch_obeys_disabled_failure_and_stale_jev_before_provider_start() {
        use std::sync::atomic::Ordering;

        let directory =
            std::env::temp_dir().join(format!("ecky-agy-global-route-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let cli = directory.join("agy-fixture");
        fake_agy_cli(&cli);
        let start_log = directory.join("provider-starts.log");
        let _restore = EnvRestore(
            ["ECKY_AGY_BIN", "ECKY_AGY_TEST_STARTS", "ECKY_APP_DATA_DIR"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        );
        std::env::set_var("ECKY_AGY_BIN", &cli);
        std::env::set_var("ECKY_AGY_TEST_STARTS", &start_log);
        std::env::set_var("ECKY_APP_DATA_DIR", directory.join("app-data"));

        let db_path = directory.join("ecky.sqlite");
        let conn = crate::db::init_db(&db_path).unwrap();
        conn.execute(
            "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params!["ecky-1", "Dryer", 1i64],
        )
        .unwrap();
        let binding = crate::services::agy_provider::bind_owned_conversation(
            &conn,
            "ecky-1",
            "agy-session-1",
            "Dryer",
            directory.to_string_lossy().as_ref(),
            1,
        )
        .unwrap();
        let app = tauri::test::mock_app();
        let state =
            crate::models::AppState::new(config(false, &directory.join("projects")), None, conn);
        state.set_mcp_status(true, None);

        let disabled = CountingClassifier {
            calls: std::sync::atomic::AtomicUsize::new(0),
            result: Ok(crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Answer,
                None,
                false,
            )),
        };
        {
            let conn = state.db.lock().await;
            crate::services::agy_provider::enqueue_prompt(&conn, "ecky-1", "legacy turn", 2)
                .unwrap();
        }
        dispatch_queue_for_with_classifier(app.handle(), &state, &binding, &disabled)
            .await
            .unwrap();
        assert_eq!(disabled.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read_to_string(&start_log).unwrap().lines().count(),
            1
        );
        state
            .agy_provider
            .discard_conversation_session("agy-session-1")
            .await;
        {
            let conn = state.db.lock().await;
            conn.execute(
                "DELETE FROM agent_prompt_queue WHERE ecky_thread_id = ?1 AND provider = 'agy'",
                ["ecky-1"],
            )
            .unwrap();
        }

        {
            let conn = state.db.lock().await;
            crate::services::agy_provider::enqueue_prompt(&conn, "ecky-1", "fail closed", 3)
                .unwrap();
        }
        {
            state.config.lock().unwrap().jev_classifier.enabled = true;
        }
        let failing = CountingClassifier {
            calls: std::sync::atomic::AtomicUsize::new(0),
            result: Err(crate::contracts::AppError::provider("fixture Jev failure")),
        };
        let failed_dispatch =
            dispatch_queue_for_with_classifier(app.handle(), &state, &binding, &failing).await;
        assert!(
            failed_dispatch.is_err(),
            "expected classifier failure; calls={}, result={failed_dispatch:?}",
            failing.calls.load(Ordering::SeqCst)
        );
        assert_eq!(failing.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read_to_string(&start_log).unwrap().lines().count(),
            1
        );
        {
            let conn = state.db.lock().await;
            conn.execute(
                "DELETE FROM agent_prompt_queue WHERE ecky_thread_id = ?1 AND provider = 'agy'",
                ["ecky-1"],
            )
            .unwrap();
        }

        {
            let conn = state.db.lock().await;
            crate::services::agy_provider::enqueue_prompt(&conn, "ecky-1", "stale route", 4)
                .unwrap();
            assert_eq!(
                crate::services::agy_provider::queue_head(&conn, "ecky-1")
                    .unwrap()
                    .unwrap()
                    .status,
                "queued"
            );
        }
        let state_for_classifier = state.clone();
        let stale = CountingClassifier {
            calls: std::sync::atomic::AtomicUsize::new(0),
            result: Ok(crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Plan,
                None,
                false,
            )),
        };
        // Mutate config during classification to invalidate accepted snapshot.
        struct StaleClassifier<'a> {
            calls: &'a CountingClassifier,
            state: crate::models::AppState,
        }
        impl crate::jev_classifier::TurnClassifier for StaleClassifier<'_> {
            fn classify<'a>(
                &'a self,
                request: crate::jev_classifier::ClassifierRequest,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = crate::contracts::AppResult<
                                crate::jev_classifier::AcceptedRoute,
                            >,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin(async move {
                    let result = self.calls.classify(request).await?;
                    self.state.config.lock().unwrap().jev_classifier.api_key = "rotated".into();
                    Ok(result)
                })
            }
        }
        let stale_classifier = StaleClassifier {
            calls: &stale,
            state: state_for_classifier,
        };
        assert!(state.config.lock().unwrap().jev_classifier.enabled);
        let stale_dispatch =
            dispatch_queue_for_with_classifier(app.handle(), &state, &binding, &stale_classifier)
                .await;
        assert!(
            stale_dispatch.is_err(),
            "expected stale route failure; calls={}, phase={}, runtime_turn={:?}, result={stale_dispatch:?}",
            stale.calls.load(Ordering::SeqCst),
            state.agy_provider.runtime("agy-session-1").await.phase,
            state.agy_provider.runtime("agy-session-1").await.active_turn_id
        );
        assert_eq!(stale.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read_to_string(&start_log).unwrap().lines().count(),
            1
        );

        {
            let conn = state.db.lock().await;
            conn.execute(
                "DELETE FROM agent_prompt_queue WHERE ecky_thread_id = ?1 AND provider = 'agy'",
                ["ecky-1"],
            )
            .unwrap();
        }
        state.config.lock().unwrap().jev_classifier.api_key = "test-key".into();
        let cancelled = {
            let conn = state.db.lock().await;
            crate::services::agy_provider::enqueue_prompt(
                &conn,
                "ecky-1",
                "cancel before provider delivery",
                5,
            )
            .unwrap()
        };
        struct CancellingClassifier {
            calls: std::sync::atomic::AtomicUsize,
            state: crate::models::AppState,
            queue_id: String,
        }
        impl crate::jev_classifier::TurnClassifier for CancellingClassifier {
            fn classify<'a>(
                &'a self,
                _request: crate::jev_classifier::ClassifierRequest,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = crate::contracts::AppResult<
                                crate::jev_classifier::AcceptedRoute,
                            >,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin(async move {
                    self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let conn = self.state.db.lock().await;
                    crate::services::agy_provider::remove_queue_item(
                        &conn,
                        "ecky-1",
                        &self.queue_id,
                    )?;
                    Ok(crate::jev_classifier::AcceptedRoute::test_route(
                        ProviderTurnIntent::Modify,
                        None,
                        false,
                    ))
                })
            }
        }
        let cancelling = CancellingClassifier {
            calls: std::sync::atomic::AtomicUsize::new(0),
            state: state.clone(),
            queue_id: cancelled.id,
        };
        dispatch_queue_for_with_classifier(app.handle(), &state, &binding, &cancelling)
            .await
            .unwrap();
        assert_eq!(cancelling.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read_to_string(&start_log).unwrap().lines().count(),
            1
        );
        {
            let conn = state.db.lock().await;
            assert!(crate::services::agy_provider::list_queue(&conn, "ecky-1")
                .unwrap()
                .is_empty());
        }

        state
            .agy_provider
            .discard_conversation_session("agy-session-1")
            .await;
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn answer_policy_terminates_prior_authoring_authority() {
        let prompt = provider_prompt_with_policy(
            AgyPromptPhase::Continuation,
            "ecky-thread-1",
            "Dryer",
            "/workspace/dryer",
            "http://127.0.0.1:39249/mcp?providerThreadId=ecky-thread-1&providerTurnIntent=answer",
            "/workspace/dryer/.agents/plugins/ecky-provider/mcp_config.json",
            "/workspace/dryer/.agents/plugins/ecky-provider/rules/AGENTS.md",
            "THREAD SUMMARY\nlarge canonical handoff",
            "ты ответить можешь? че происходит?",
            &[],
            ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer),
        );

        assert!(prompt.contains("Intent: ANSWER"));
        assert!(prompt.contains("Do not call tools"));
        assert!(prompt.contains("Do not inspect or edit project files"));
        assert!(prompt.contains("Do not resume unfinished work from earlier turns"));
        assert!(prompt.contains("Answer the current user message immediately"));
    }

    #[test]
    fn accepted_jev_route_controls_the_policy_sent_to_agy() {
        let route = crate::jev_classifier::AcceptedRoute::test_route(
            ProviderTurnIntent::Answer,
            None,
            false,
        );
        let policy = super::agy_policy_for_route(Some(&route));
        assert_eq!(policy.intent(), ProviderTurnIntent::Answer);
        assert!(!policy.allows_any_tool());
        assert!(!policy.allows_project_writes());
        let prompt = provider_prompt_with_policy(
            AgyPromptPhase::Continuation,
            "ecky-thread-1",
            "Dryer",
            "/workspace/dryer",
            "http://127.0.0.1:39249/mcp?providerThreadId=ecky-thread-1&providerTurnIntent=answer",
            "/workspace/dryer/.agents/plugins/ecky-provider/mcp_config.json",
            "/workspace/dryer/.agents/plugins/ecky-provider/rules/AGENTS.md",
            "",
            "answer this request",
            &[],
            policy,
        );
        assert!(prompt.contains("Intent: ANSWER"));
        assert!(prompt.contains("answer this request"));
    }

    #[test]
    fn disabled_jev_route_keeps_legacy_prompt_based_agy_policy() {
        let policy = super::agy_policy_for_route(None);
        assert!(policy.is_prompt_based());
        assert!(policy.allows_any_tool());
        assert!(policy.allows_project_writes());
    }

    #[test]
    fn agy_classifier_failure_trace_has_request_and_error_without_provider_turn() {
        let binding = crate::contracts::AgyProviderBinding {
            ecky_thread_id: "ecky-1".into(),
            agy_conversation_id: "agy-session-1".into(),
            label: "Dryer".into(),
            cwd: "/workspace/dryer".into(),
            bootstrap_version: 1,
            created_at: 10,
            updated_at: 10,
        };
        let seed = super::eval_run_seed(
            &binding,
            "run-1",
            "Describe the current part",
            Some("agy-model".into()),
            None,
            ProviderTurnIntent::Clarify,
            None,
        );
        let run = super::agy_pre_dispatch_eval_run(
            seed,
            "queue-1",
            "Describe the current part",
            100,
            101,
            "Jev classifier timeout",
            None,
        );
        assert_eq!(run.run_id, "run-1");
        assert!(run.turn_id.is_empty());
        assert_eq!(run.status, "failed_pre_dispatch");
        assert_eq!(run.external_thread_id, "agy-session-1");
        assert_eq!(run.events.len(), 2);
        assert_eq!(run.events[0].name.as_deref(), Some("request"));
        assert_eq!(run.events[1].name.as_deref(), Some("jev"));
        assert!(run
            .events
            .iter()
            .all(|event| event.kind == crate::llm_eval::EvalEventKind::System));
        assert!(run.raw_error.as_deref().unwrap().contains("timeout"));
    }

    #[test]
    fn agy_post_start_persistence_failure_keeps_observed_turn_id() {
        let binding = crate::contracts::AgyProviderBinding {
            ecky_thread_id: "ecky-1".into(),
            agy_conversation_id: "agy-session-1".into(),
            label: "Dryer".into(),
            cwd: "/workspace/dryer".into(),
            bootstrap_version: 1,
            created_at: 10,
            updated_at: 10,
        };
        let mut seed = super::eval_run_seed(
            &binding,
            "run-post-start-1",
            "Describe the current part",
            Some("agy-model".into()),
            None,
            ProviderTurnIntent::Answer,
            None,
        );
        seed.jev_route = Some(crate::llm_eval::EvalJevRoute {
            policy_version: crate::jev_classifier::CLASSIFIER_POLICY_VERSION.into(),
            intent_confidence_threshold: 0.65,
            intent_margin_threshold: 0.15,
            model_confidence_threshold: 0.65,
            model_margin_threshold: 0.15,
            intent_confidence: 0.91,
            intent_probabilities: std::collections::BTreeMap::from([("answer".into(), 0.91)]),
            answer_requested: true,
            answer_first: false,
            model_ceiling: None,
            model_confidence: None,
            model_probabilities: std::collections::BTreeMap::new(),
            model_reason: "configured Agy model".into(),
            context_truncated: false,
            current_prompt_truncated: false,
            classifier_input_tokens: Some(30),
            classifier_output_tokens: Some(12),
            classifier_model: Some("jev-1.13.0".into()),
            classifier_latency_ms: Some(100),
            model_catalog_version: None,
            model_catalog_valid_until: None,
            native_tool_coverage: crate::llm_eval::unknown_native_tool_coverage(),
        });
        let run = super::agy_provider_failure_eval_run(
            seed,
            "Describe the current part",
            "agy-session-actual-77",
            "turn-provider-77",
            100,
            101,
            "Could not persist provider lease",
        );

        assert_eq!(run.run_id, "run-post-start-1");
        assert_eq!(run.turn_id, "turn-provider-77");
        assert_eq!(run.external_thread_id, "agy-session-actual-77");
        assert_eq!(run.status, "error");
        assert!(run.route.jev.is_some());
        assert!(run
            .events
            .iter()
            .any(|event| { event.name.as_deref() == Some("delivery") && event.output.is_some() }));
        assert!(run.events.iter().any(|event| {
            event.name.as_deref() == Some("provider.persistence")
                && event.error.as_deref() == Some("Could not persist provider lease")
        }));
    }

    #[test]
    fn provider_prompt_requests_clickable_bound_source_evidence_without_internal_ids() {
        let prompt = provider_prompt(
            AgyPromptPhase::Bootstrap,
            "ecky-thread-1",
            "Dryer",
            "/workspace/dryer",
            "http://127.0.0.1:39249/mcp",
            "/workspace/dryer/.agy/mcp_config.json",
            "/workspace/dryer/.agents/ecky-provider-tools.md",
            "Current target: dryer",
            "Increase capacity.",
            &[],
        );

        assert!(prompt.contains("[model.ecky](/workspace/dryer/model.ecky:LINE)"));
        assert!(prompt.contains("Do not include internal `messageId` or `modelId`"));
        assert!(prompt.contains("Read the provider tool guide first"));
        assert!(prompt.contains("already pre-bound"));
        assert!(prompt.contains("Do not call `thread_borrow`"));
    }

    #[test]
    fn warm_continuation_sends_user_turn_without_repeating_canonical_handoff() {
        let prompt = provider_prompt(
            AgyPromptPhase::Continuation,
            "ecky-thread-1",
            "Dryer",
            "/workspace/dryer",
            "http://127.0.0.1:39249/mcp?providerThreadId=ecky-thread-1",
            "/workspace/dryer/.agents/plugins/ecky-provider/mcp_config.json",
            "/workspace/dryer/.agents/plugins/ecky-provider/rules/AGENTS.md",
            "THREAD SUMMARY\nlarge canonical handoff",
            "Increase capacity.",
            &[],
        );

        assert!(prompt.contains("[ECKY USER TURN v3]"));
        assert!(prompt.contains("Increase capacity."));
        assert!(!prompt.contains("large canonical handoff"));
        assert!(!prompt.contains("THREAD BOOTSTRAP"));
    }

    #[test]
    fn agy_manifest_preserves_absolute_attachment_paths() {
        let manifest = build_agy_attachment_manifest(&[
            Attachment {
                path: "/Users/test/mcp-attachments/ref.png".to_string(),
                name: "ref.png".to_string(),
                explanation: String::new(),
                data_url: None,
                kind: AttachmentKind::Image,
            },
            Attachment {
                path: "/Users/test/model.step".to_string(),
                name: "model.step".to_string(),
                explanation: String::new(),
                data_url: None,
                kind: AttachmentKind::Cad,
            },
        ]);

        assert!(manifest.contains("[ATTACHMENT MANIFEST]"));
        assert!(manifest.contains("ref.png [image]: /Users/test/mcp-attachments/ref.png"));
        assert!(manifest.contains("model.step [cad]: /Users/test/model.step"));
    }

    #[test]
    fn workspace_mcp_config_prebinds_exact_thread_and_writes_tool_guide() {
        let directory = std::env::temp_dir().join(format!(
            "ecky-agy-workspace-config-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();

        let materialized = materialize_agy_mcp_config(
            directory.to_str().unwrap(),
            "http://127.0.0.1:39249/mcp",
            "ecky-thread-1",
        )
        .unwrap();
        let config = std::fs::read_to_string(&materialized.config_path).unwrap();
        let guide = std::fs::read_to_string(&materialized.guide_path).unwrap();
        let manifest =
            std::fs::read_to_string(directory.join(".agents/plugins/ecky-provider/plugin.json"))
                .unwrap();

        assert!(config.contains("http://127.0.0.1:39249/mcp?providerThreadId=ecky-thread-1"));
        assert!(config.contains("\"ecky_mcp\""));
        assert!(!config.contains("\"ecky_provider_mcp\""));
        assert!(guide.contains("Provider target is already pre-bound"));
        assert!(guide.contains("Do not call `thread_borrow`"));
        assert!(guide.contains("workspace_overview"));
        assert!(guide.contains("agentBrief.primaryGuideUri"));
        assert!(materialized
            .config_path
            .ends_with(".agents/plugins/ecky-provider/mcp_config.json"));
        assert!(materialized
            .guide_path
            .ends_with(".agents/plugins/ecky-provider/rules/AGENTS.md"));
        assert!(manifest.contains("\"disabled\": false"));

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn bootstrap_and_compaction_prompt_instructs_agent_on_thread_messages_get() {
        let prompt = provider_prompt_with_policy(
            AgyPromptPhase::Bootstrap,
            "ecky-thread-1",
            "Dryer",
            "/workspace/dryer",
            "http://127.0.0.1:39249/mcp?providerThreadId=ecky-thread-1",
            "/workspace/dryer/.agents/plugins/ecky-provider/mcp_config.json",
            "/workspace/dryer/.agents/plugins/ecky-provider/rules/AGENTS.md",
            "THREAD SUMMARY\ncompacted history",
            "делай",
            &[],
            ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify),
        );

        assert!(prompt.contains("[ECKY THREAD BOOTSTRAP"));
        assert!(prompt.contains("Earlier conversation history can be inspected anytime via the MCP tool `thread_messages_get`."));
        assert!(prompt.contains("THREAD SUMMARY\ncompacted history"));
        assert!(prompt.contains("[USER MESSAGE]\nделай"));
    }
}
