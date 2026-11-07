use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{Emitter, State};

use crate::contracts::{
    AppError, AppResult, Attachment, GrokMessagePage, GrokMessagePageInput, GrokPromptInput,
    GrokProviderBinding, GrokProviderSnapshot, GrokStopInput, ProviderCapabilities,
};
use crate::models::AppState;
use crate::provider_turn::ProviderTurnPolicy;
use crate::services::{codex_takeover, grok_provider};

static GROK_BINDING_CREATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static GROK_QUEUE_WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static GROK_DISPATCH_GATES: StdMutex<Option<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    StdMutex::new(None);
const GROK_QUEUE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn dispatch_gate(thread_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut gates = GROK_DISPATCH_GATES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    gates
        .get_or_insert_with(HashMap::new)
        .entry(thread_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn require_grok_provider_mode(state: &AppState) -> AppResult<()> {
    let configured = state.config.lock().unwrap().connection_type.clone();
    if configured.as_deref() == Some("provider:grok") {
        return Ok(());
    }
    Err(AppError::validation(format!(
        "Grok provider send requires Settings connectionType provider:grok; current value is {}.",
        configured.as_deref().unwrap_or("unset")
    )))
}

fn configured_grok_model(state: &AppState) -> Option<String> {
    let model = state
        .config
        .lock()
        .unwrap()
        .provider_models
        .grok
        .trim()
        .to_string();
    (!model.is_empty()).then_some(model)
}

fn require_mcp_endpoint(state: &AppState) -> AppResult<String> {
    let status = state.mcp_status();
    if status.running && !status.endpoint_url.trim().is_empty() {
        return Ok(status.endpoint_url);
    }
    Err(AppError::provider(status.last_startup_error.unwrap_or_else(|| {
        format!(
            "Ecky MCP endpoint {} is not running; Grok provider conversation cannot start without CAD tools.",
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
                session_id: format!("grok-provider:{ecky_thread_id}"),
                client_kind: "provider".to_string(),
                host_label: "Ecky".to_string(),
                agent_label: "Grok".to_string(),
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
            "Failed to create Ecky Grok workspace '{}': {error}",
            path.display()
        ))
    })?;
    Ok(path.to_string_lossy().into_owned())
}

async fn turn_policy(
    state: &AppState,
    ecky_thread_id: &str,
    session_id: Option<&str>,
    prompt: &str,
    attachments: &[Attachment],
) -> AppResult<ProviderTurnPolicy> {
    let config = state.config.lock().unwrap().clone();
    if !config.jev_classifier.enabled {
        return Ok(ProviderTurnPolicy::prompt_based());
    }
    let summary = {
        let conn = state.db.lock().await;
        crate::db::get_thread_summary(&conn, ecky_thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .unwrap_or_default()
    };
    let request = crate::jev_classifier::ClassifierRequest::bounded(
        prompt,
        std::iter::empty(),
        &summary,
        &format!(
            "owned Grok session {}; configured model ceiling {}",
            session_id.unwrap_or("new session"),
            configured_grok_model(state)
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
    );
    let route =
        crate::jev_classifier::classify_configured_request(&config.jev_classifier, request).await?;
    Ok(ProviderTurnPolicy::routed(route.intent, route.answer_first))
}

fn attachment_manifest(attachments: &[Attachment]) -> String {
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

fn grok_turn_prompt(
    continuation: bool,
    ecky_thread_id: &str,
    title: &str,
    cwd: &str,
    endpoint: &str,
    handoff: &str,
    prompt: &str,
    attachments: &[Attachment],
    policy: ProviderTurnPolicy,
) -> String {
    let wrapped =
        policy.wrap_user_message(&format!("{prompt}{}", attachment_manifest(attachments)));
    if continuation {
        return format!(
            "Continue the owned Ecky Grok session for thread {ecky_thread_id}. Do not call `thread_borrow`.\n\n{wrapped}"
        );
    }
    format!(
        "You are Grok inside Ecky CAD. This session belongs only to Ecky thread {ecky_thread_id} ({title}).\n\
Canonical workspace: {cwd}\n\
Required MCP server: ecky_mcp at {endpoint}. This connection is already pre-bound to thread {ecky_thread_id}. Do not call `thread_borrow`.\n\
Obey the turn policy below. For MODIFY, call `workspace_overview`, verify its target, and read `agentBrief.primaryGuideUri` plus every URI in `agentBrief.mustRead` before editing. Use inspect, validate, preview, and verify. The bound file watcher creates the version, so do not call a manual commit or finalize operation.\n\
When useful, cite the bound source as `[model.ecky]({cwd}/model.ecky:LINE)`.\n\n\
{handoff}\n\n\
{wrapped}"
    )
}

async fn handoff_summary(state: &AppState, ecky_thread_id: &str) -> String {
    let conn = state.db.lock().await;
    let summary = crate::db::get_thread_summary(&conn, ecky_thread_id)
        .ok()
        .flatten()
        .unwrap_or_default();
    let messages = grok_provider::message_page(&conn, ecky_thread_id, None)
        .map(|page| page.messages)
        .unwrap_or_default();
    codex_takeover::build_provider_handoff_summary_for("GROK", &summary, &messages)
}

async fn binding_for(state: &AppState, ecky_thread_id: &str) -> AppResult<GrokProviderBinding> {
    let conn = state.db.lock().await;
    grok_provider::get_binding(&conn, ecky_thread_id)?.ok_or_else(|| {
        AppError::not_found(format!(
            "Ecky thread {ecky_thread_id} has no owned Grok session."
        ))
    })
}

pub(crate) async fn activate_bound_writer(
    _state: &AppState,
    _ecky_thread_id: &str,
) -> AppResult<()> {
    Ok(())
}

async fn snapshot_for(
    state: &AppState,
    binding: GrokProviderBinding,
    cursor: Option<&str>,
) -> AppResult<GrokProviderSnapshot> {
    let (page, queue) = {
        let conn = state.db.lock().await;
        (
            grok_provider::message_page(&conn, &binding.ecky_thread_id, cursor)?,
            grok_provider::list_queue(&conn, &binding.ecky_thread_id)?,
        )
    };
    let mut runtime = state.grok_provider.runtime(&binding.grok_session_id).await;
    if runtime.error.is_none() {
        runtime.error = queue
            .first()
            .filter(|item| item.status == "failed")
            .and_then(|item| item.error.clone());
    }
    Ok(GrokProviderSnapshot {
        messages: page.messages,
        next_cursor: page.next_cursor,
        backwards_cursor: page.backwards_cursor,
        runtime,
        live_messages: state
            .grok_provider
            .live_messages(&binding.grok_session_id)
            .await,
        turn_traces: state
            .grok_provider
            .turn_traces(&binding.grok_session_id)
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
    binding: &GrokProviderBinding,
    method: &str,
) {
    let _ = app.emit(
        "grok-provider-updated",
        serde_json::json!({
            "conversationId": binding.grok_session_id,
            "method": method,
        }),
    );
}

async fn open_session(
    state: &AppState,
    binding: &GrokProviderBinding,
    policy: ProviderTurnPolicy,
    resume: bool,
) -> AppResult<String> {
    let endpoint = {
        let base = require_mcp_endpoint(state)?;
        crate::mcp::server::provider_bound_endpoint_with_policy(
            &base,
            &binding.ecky_thread_id,
            policy,
        )
    };
    state
        .grok_provider
        .ensure_session(grok_provider::GrokSessionRequest {
            cwd: binding.cwd.clone(),
            model: configured_grok_model(state),
            mcp_url: endpoint,
            resume_session_id: resume.then(|| binding.grok_session_id.clone()),
        })
        .await
}

async fn ensure_binding<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    ecky_thread_id: &str,
    prompt: &str,
    attachments: &[Attachment],
) -> AppResult<GrokProviderBinding> {
    if let Some(binding) = {
        let conn = state.db.lock().await;
        grok_provider::get_binding(&conn, ecky_thread_id)?
    } {
        return Ok(binding);
    }
    let _creation = GROK_BINDING_CREATE.lock().await;
    if let Some(binding) = {
        let conn = state.db.lock().await;
        grok_provider::get_binding(&conn, ecky_thread_id)?
    } {
        return Ok(binding);
    }
    let title = project_title(state, ecky_thread_id).await?;
    let cwd = provider_project_cwd(app, state, ecky_thread_id, &title).await?;
    let policy = turn_policy(state, ecky_thread_id, None, prompt, attachments).await?;
    let provisional = GrokProviderBinding {
        ecky_thread_id: ecky_thread_id.to_string(),
        grok_session_id: String::new(),
        label: title.clone(),
        cwd: cwd.clone(),
        bootstrap_version: grok_provider::GROK_BOOTSTRAP_VERSION,
        created_at: now_seconds(),
        updated_at: now_seconds(),
    };
    let session_id = open_session(state, &provisional, policy, false).await?;
    let now = now_seconds();
    let conn = state.db.lock().await;
    grok_provider::bind_owned_session(&conn, ecky_thread_id, &session_id, &title, &cwd, now)
}

async fn dispatch_queue_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    binding: &GrokProviderBinding,
) -> AppResult<()> {
    let gate = dispatch_gate(&binding.ecky_thread_id);
    let _guard = gate.lock().await;
    let mut binding = binding.clone();
    loop {
        let head = {
            let conn = state.db.lock().await;
            grok_provider::queue_head(&conn, &binding.ecky_thread_id)?
        };
        let Some(head) = head else { break };
        if head.status != "queued" {
            break;
        }
        if let Err(error) = deliver_claimed(app, state, &binding, &head).await {
            state.push_log(format!(
                "[GROK] prompt delivery failed: {}",
                codex_takeover::error_text(&error)
            ));
            break;
        }
        if let Some(latest) = {
            let conn = state.db.lock().await;
            grok_provider::get_binding(&conn, &binding.ecky_thread_id)?
        } {
            binding = latest;
        }
    }
    Ok(())
}

async fn deliver_claimed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    binding: &GrokProviderBinding,
    head: &crate::contracts::CodexQueuedPrompt,
) -> AppResult<()> {
    let claimed = {
        let conn = state.db.lock().await;
        codex_takeover::claim_queue_item(&conn, &head.id, now_seconds())?
    };
    if !claimed {
        return Ok(());
    }
    if let Err(error) = run_claimed_turn(app, state, binding, head).await {
        let conn = state.db.lock().await;
        let _ = codex_takeover::fail_queue_item(
            &conn,
            &head.id,
            &codex_takeover::error_text(&error),
            now_seconds(),
        );
        emit_provider_update(app, binding, "turn/persisted").await;
        return Err(error);
    }
    Ok(())
}

async fn run_claimed_turn<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    binding: &GrokProviderBinding,
    head: &crate::contracts::CodexQueuedPrompt,
) -> AppResult<()> {
    let policy = turn_policy(
        state,
        &binding.ecky_thread_id,
        Some(&binding.grok_session_id),
        &head.prompt_text,
        &head.attachments,
    )
    .await?;
    let session_id = open_session(state, binding, policy, true).await?;
    let binding = if session_id == binding.grok_session_id {
        binding.clone()
    } else {
        let conn = state.db.lock().await;
        grok_provider::rotate_binding(
            &conn,
            &binding.ecky_thread_id,
            &session_id,
            "grok-session-restart",
            now_seconds(),
        )?
    };
    let continuation = {
        let conn = state.db.lock().await;
        grok_provider::count_session_messages(&conn, &binding.ecky_thread_id, &session_id)? > 0
    };
    let endpoint = {
        let base = require_mcp_endpoint(state)?;
        crate::mcp::server::provider_bound_endpoint_with_policy(
            &base,
            &binding.ecky_thread_id,
            policy,
        )
    };
    let title = project_title(state, &binding.ecky_thread_id)
        .await
        .unwrap_or_else(|_| binding.label.clone());
    let handoff = if continuation {
        String::new()
    } else {
        handoff_summary(state, &binding.ecky_thread_id).await
    };
    let prompt = grok_turn_prompt(
        continuation,
        &binding.ecky_thread_id,
        &title,
        &binding.cwd,
        &endpoint,
        &handoff,
        &head.prompt_text,
        &head.attachments,
        policy,
    );
    {
        let conn = state.db.lock().await;
        let _ = codex_takeover::begin_queue_delivery(&conn, &head.id, now_seconds())?;
        let user = crate::contracts::CodexDialogueMessage {
            id: format!("grok:user:{}", head.id),
            role: "user".to_string(),
            content: head.prompt_text.clone(),
            status: "success".to_string(),
            timestamp: now_seconds(),
            attachments: head.attachments.clone(),
            provider_event_kind: None,
        };
        codex_takeover::persist_finished_provider_messages(
            &conn,
            &binding.ecky_thread_id,
            grok_provider::GROK_PROVIDER_ID,
            &session_id,
            &[user],
        )?;
    }
    let started = state.grok_provider.prompt(&session_id, &prompt).await?;
    let outcome = match started.result.await {
        Ok(result) => result,
        Err(_) => Err(AppError::provider(
            "Grok turn result channel closed unexpectedly.".to_string(),
        )),
    };
    {
        let conn = state.db.lock().await;
        match &outcome {
            Ok(result) => {
                if !result.response.trim().is_empty() {
                    let status = match result.status.as_str() {
                        "SUCCESS" => "success",
                        "INTERRUPTED" | "CANCELED" => "interrupted",
                        _ => "error",
                    };
                    let assistant = crate::contracts::CodexDialogueMessage {
                        id: format!("grok:assistant:{}", result.turn_id),
                        role: "assistant".to_string(),
                        content: result.response.clone(),
                        status: status.to_string(),
                        timestamp: now_seconds(),
                        attachments: Vec::new(),
                        provider_event_kind: None,
                    };
                    let _ = codex_takeover::persist_finished_provider_messages(
                        &conn,
                        &binding.ecky_thread_id,
                        grok_provider::GROK_PROVIDER_ID,
                        &session_id,
                        &[assistant],
                    );
                }
                if matches!(
                    result.status.as_str(),
                    "SUCCESS" | "INTERRUPTED" | "CANCELED"
                ) {
                    let _ = codex_takeover::complete_queue_item(&conn, &head.id);
                } else {
                    let raw = result
                        .error
                        .clone()
                        .unwrap_or_else(|| result.status.clone());
                    let _ = codex_takeover::fail_queue_item(&conn, &head.id, &raw, now_seconds());
                }
            }
            Err(error) => {
                let _ = codex_takeover::fail_queue_item(
                    &conn,
                    &head.id,
                    &codex_takeover::error_text(error),
                    now_seconds(),
                );
            }
        }
    }
    emit_provider_update(app, &binding, "turn/persisted").await;
    GROK_QUEUE_WAKE.notify_one();
    outcome.map(|_| ())
}

#[tauri::command]
#[specta::specta]
pub async fn get_grok_provider(
    ecky_thread_id: String,
    state: State<'_, AppState>,
) -> AppResult<Option<GrokProviderSnapshot>> {
    let binding = {
        let conn = state.db.lock().await;
        grok_provider::get_binding(&conn, &ecky_thread_id)?
    };
    match binding {
        Some(binding) => snapshot_for(&state, binding, None).await.map(Some),
        None => Ok(None),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_grok_provider_messages(
    input: GrokMessagePageInput,
    state: State<'_, AppState>,
) -> AppResult<GrokMessagePage> {
    let conn = state.db.lock().await;
    grok_provider::message_page(&conn, &input.ecky_thread_id, input.cursor.as_deref())
}

#[tauri::command]
#[specta::specta]
pub async fn send_grok_provider_prompt(
    input: GrokPromptInput,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<GrokProviderSnapshot> {
    require_grok_provider_mode(&state)?;
    if input.prompt_text.trim().is_empty() && input.attachments.is_empty() {
        return Err(AppError::validation(
            "Grok prompt must include text or attachments.",
        ));
    }
    let binding = ensure_binding(
        &app,
        &state,
        &input.ecky_thread_id,
        &input.prompt_text,
        &input.attachments,
    )
    .await?;
    {
        let conn = state.db.lock().await;
        grok_provider::enqueue_prompt_with_attachments(
            &conn,
            &input.ecky_thread_id,
            &input.prompt_text,
            &input.attachments,
            now_seconds(),
        )?;
    }
    GROK_QUEUE_WAKE.notify_one();
    let dispatch_state = state.inner().clone();
    let dispatch_app = app.clone();
    let dispatch_binding = binding.clone();
    tauri::async_runtime::spawn(async move {
        let _ = dispatch_queue_for(&dispatch_app, &dispatch_state, &dispatch_binding).await;
        emit_provider_update(&dispatch_app, &dispatch_binding, "queue/dispatched").await;
    });
    snapshot_for(&state, binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_grok_provider(
    input: GrokStopInput,
    state: State<'_, AppState>,
) -> AppResult<GrokProviderSnapshot> {
    let binding = binding_for(&state, &input.ecky_thread_id).await?;
    state
        .grok_provider
        .stop_turn(&binding.grok_session_id, &input.turn_id)
        .await?;
    snapshot_for(&state, binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_grok_queued_prompt(
    ecky_thread_id: String,
    queue_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<GrokProviderSnapshot> {
    require_grok_provider_mode(&state)?;
    let binding = binding_for(&state, &ecky_thread_id).await?;
    {
        let conn = state.db.lock().await;
        grok_provider::retry_queue_item(&conn, &ecky_thread_id, &queue_id, now_seconds())?;
    }
    GROK_QUEUE_WAKE.notify_one();
    dispatch_queue_for(&app, &state, &binding).await?;
    snapshot_for(&state, binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_grok_queued_prompt(
    ecky_thread_id: String,
    queue_id: String,
    state: State<'_, AppState>,
) -> AppResult<GrokProviderSnapshot> {
    let binding = binding_for(&state, &ecky_thread_id).await?;
    {
        let conn = state.db.lock().await;
        grok_provider::remove_queue_item(&conn, &ecky_thread_id, &queue_id)?;
    }
    snapshot_for(&state, binding, None).await
}

pub fn initialize_grok_queue_supervisor(state: AppState, app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(GROK_QUEUE_POLL_INTERVAL) => {}
                _ = GROK_QUEUE_WAKE.notified() => {}
            }
            if state.config.lock().unwrap().connection_type.as_deref() != Some("provider:grok") {
                continue;
            }
            let bindings = {
                let conn = state.db.lock().await;
                match grok_provider::pending_queue_bindings(&conn) {
                    Ok(bindings) => bindings,
                    Err(error) => {
                        state.push_log(format!(
                            "[GROK] prompt queue scan failed: {}",
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
    use super::grok_turn_prompt;
    use crate::contracts::{Attachment, AttachmentKind};
    use crate::provider_turn::{ProviderTurnIntent, ProviderTurnPolicy};

    #[test]
    fn bootstrap_prompt_names_the_prebound_ecky_endpoint() {
        let prompt = grok_turn_prompt(
            false,
            "thread-1",
            "Bracket",
            "/tmp/bracket",
            "http://127.0.0.1:39249/mcp?providerThreadId=thread-1",
            "THREAD SUMMARY\nnone",
            "add a 10mm hole",
            &[Attachment {
                path: "/tmp/bracket/sketch.png".into(),
                name: "sketch".into(),
                explanation: "front view".into(),
                data_url: None,
                kind: AttachmentKind::Image,
            }],
            ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify),
        );
        assert!(prompt.contains("thread-1"));
        assert!(prompt.contains("http://127.0.0.1:39249/mcp?providerThreadId=thread-1"));
        assert!(prompt.contains("add a 10mm hole"));
        assert!(prompt.contains("sketch.png"));
        assert!(prompt.contains("MODIFY"));
        assert!(prompt.contains("Do not call `thread_borrow`."));
    }
}
