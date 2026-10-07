use std::collections::HashSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::Digest;
use tauri::{Emitter, Manager, State};

use crate::contracts::{
    AppError, AppResult, CodexDialogueMessage, CodexMessagePage, CodexMessagePageInput,
    CodexPromptInput, CodexSteerInput, CodexStopInput, CodexTakeoverBinding, CodexTakeoverSnapshot,
};
use crate::models::{AppState, PathResolver};
use crate::provider_turn::{ProviderTurnIntent, ProviderTurnPolicy};
use crate::services::codex_takeover;

static CODEX_BINDING_CREATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CODEX_HISTORY_BACKFILL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const CODEX_QUEUE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const CODEX_QUEUE_RETRY_DELAY_SECONDS: i64 = 3;

async fn persist_completed_codex_eval_runs(state: &AppState, app: &tauri::AppHandle) {
    persist_completed_codex_eval_runs_to(state, &app.app_data_dir()).await;
}

async fn persist_completed_codex_eval_runs_to(state: &AppState, root: &std::path::Path) {
    for mut run in state.codex_app_server.completed_eval_runs().await {
        let versions = if run.turn_id.is_empty() {
            Ok(Vec::new())
        } else {
            let conn = state.db.lock().await;
            crate::llm_eval::version_outcomes_for_window(
                &conn,
                &run.thread_id,
                run.case.starting_version_id.as_deref(),
                run.started_at,
                run.completed_at,
            )
        };
        match versions {
            Ok(versions) => run.versions = versions,
            Err(error) => {
                if state
                    .codex_app_server
                    .should_report_eval_persistence_error(&run.run_id)
                    .await
                {
                    state.push_log(format!("[CODEX] eval version linkage failed: {error}"));
                }
                continue;
            }
        }
        let persisted = match crate::llm_eval::persist_run(root, &run) {
            Ok(files) => {
                state.push_log(format!(
                    "[LLM EVAL] persisted Codex run {}",
                    files.run_dir.display()
                ));
                true
            }
            Err(error) => {
                if state
                    .codex_app_server
                    .should_report_eval_persistence_error(&run.run_id)
                    .await
                {
                    state.push_log(format!("[CODEX] eval persistence failed: {error}"));
                }
                false
            }
        };
        if persisted {
            state
                .codex_app_server
                .acknowledge_eval_run(&run.run_id)
                .await;
        }
    }
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn record_queue_delivery_error(
    conn: &rusqlite::Connection,
    queue_id: &str,
    error: &AppError,
) -> AppResult<()> {
    let text = codex_takeover::error_text(error);
    if codex_takeover::is_retryable_delivery_error(&text) {
        codex_takeover::defer_queue_item(
            conn,
            queue_id,
            &text,
            now_seconds() + CODEX_QUEUE_RETRY_DELAY_SECONDS,
        )
    } else {
        codex_takeover::fail_queue_item(conn, queue_id, &text, now_seconds())
    }
}

async fn fail_claimed_queue<T>(state: &AppState, queue_id: &str, error: AppError) -> AppResult<T> {
    let conn = state.db.lock().await;
    record_queue_delivery_error(&conn, queue_id, &error)?;
    Err(error)
}

fn require_mcp_endpoint(state: &AppState) -> AppResult<String> {
    let status = state.mcp_status();
    if status.running && !status.endpoint_url.trim().is_empty() {
        return Ok(status.endpoint_url);
    }
    Err(AppError::provider(
        status.last_startup_error.unwrap_or_else(|| {
            format!(
                "Ecky MCP endpoint {} is not running; Codex provider conversation cannot resume without CAD tools.",
                status.endpoint_url
            )
        }),
    ))
}

fn require_codex_provider_mode(state: &AppState) -> AppResult<()> {
    let configured = state.config.lock().unwrap().connection_type.clone();
    if configured.as_deref() == Some("provider:codex") {
        return Ok(());
    }
    Err(AppError::validation(format!(
        "Codex provider send requires Settings connectionType provider:codex; current value is {}.",
        configured.as_deref().unwrap_or("unset")
    )))
}

fn configured_codex_model(state: &AppState) -> Option<String> {
    let model = state
        .config
        .lock()
        .unwrap()
        .provider_models
        .codex
        .trim()
        .to_string();
    (!model.is_empty()).then_some(model)
}

async fn project_title(state: &AppState, ecky_thread_id: &str) -> AppResult<String> {
    let conn = state.db.lock().await;
    crate::db::get_thread_title(&conn, ecky_thread_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| AppError::not_found(format!("Ecky thread {ecky_thread_id} was not found.")))
}

async fn canonical_handoff(state: &AppState, ecky_thread_id: &str) -> AppResult<String> {
    let conn = state.db.lock().await;
    let context =
        crate::context::assemble_context(&conn, Some(ecky_thread_id.to_string()), None, None);
    Ok(format!(
        "THREAD SUMMARY\n{}\n\nRECENT DIALOGUE\n{}\n\nDESIGN DIGEST\n{}\n\nARTIFACT DIGEST\n{}",
        if context.summary.trim().is_empty() {
            "[none]"
        } else {
            &context.summary
        },
        if context.recent_dialogue.trim().is_empty() {
            "[none]"
        } else {
            &context.recent_dialogue
        },
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
    ))
}

async fn provider_project_cwd(
    app: &tauri::AppHandle,
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
                session_id: format!("codex-provider:{ecky_thread_id}"),
                client_kind: "provider".to_string(),
                host_label: "Ecky".to_string(),
                agent_label: "Codex".to_string(),
                llm_model_id: None,
                llm_model_label: None,
            },
        )
        .await?;
        return Ok(exported.folder);
    }

    // A blank Ecky thread has no source to mirror yet. Give Codex a stable,
    // isolated workspace; the first committed version will replace this with
    // the canonical thread-source binding on the next resume.
    let configured_root = state.config.lock().unwrap().projects_root.clone();
    let slug = crate::project_mirror::project_slug(title, ecky_thread_id);
    let path = crate::project_mirror::project_dir(app, configured_root.as_deref(), &slug)?;
    std::fs::create_dir_all(&path).map_err(|error| {
        AppError::persistence(format!(
            "Failed to create Ecky provider workspace '{}': {error}",
            path.display()
        ))
    })?;
    Ok(path.to_string_lossy().into_owned())
}

async fn binding_for(state: &AppState, ecky_thread_id: &str) -> AppResult<CodexTakeoverBinding> {
    let conn = state.db.lock().await;
    codex_takeover::get_binding(&conn, ecky_thread_id)?.ok_or_else(|| {
        AppError::not_found(format!(
            "Ecky thread {ecky_thread_id} has no owned Codex conversation."
        ))
    })
}

async fn snapshot_for(
    state: &AppState,
    binding: CodexTakeoverBinding,
    cursor: Option<String>,
) -> AppResult<CodexTakeoverSnapshot> {
    let runtime = state
        .codex_app_server
        .runtime(&binding.codex_thread_id)
        .await;
    let live_messages = state
        .codex_app_server
        .live_messages(&binding.codex_thread_id)
        .await;
    let turn_traces = state
        .codex_app_server
        .turn_traces(&binding.codex_thread_id)
        .await;
    let queue = {
        let conn = state.db.lock().await;
        let page = codex_takeover::provider_message_page(
            &conn,
            &binding.ecky_thread_id,
            codex_takeover::CODEX_PROVIDER_ID,
            cursor.as_deref(),
        )?;
        let title = crate::db::get_thread_title(&conn, &binding.ecky_thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .unwrap_or_else(|| binding.label.clone());
        let ecky_messages =
            crate::db::get_thread_messages_for_context(&conn, &binding.ecky_thread_id)
                .unwrap_or_default();
        let canonical = crate::context::build_thread_summary(&title, &ecky_messages);
        let handoff = codex_takeover::build_provider_handoff_summary(&canonical, &page.messages);
        crate::db::update_thread_summary(&conn, &binding.ecky_thread_id, &handoff)
            .map_err(|error| AppError::persistence(error.to_string()))?;
        (
            page,
            codex_takeover::list_queue(&conn, &binding.ecky_thread_id)?,
        )
    };
    let (page, queue) = queue;
    Ok(CodexTakeoverSnapshot {
        binding,
        messages: page.messages,
        live_messages,
        turn_traces,
        next_cursor: page.next_cursor,
        backwards_cursor: page.backwards_cursor,
        runtime,
        queue,
    })
}

async fn queued_snapshot_for(
    state: &AppState,
    binding: CodexTakeoverBinding,
) -> AppResult<CodexTakeoverSnapshot> {
    snapshot_for(state, binding, None).await
}

async fn persist_codex_messages(
    state: &AppState,
    binding: &CodexTakeoverBinding,
    messages: &[CodexDialogueMessage],
) -> AppResult<usize> {
    let conn = state.db.lock().await;
    let persisted = codex_takeover::persist_finished_provider_messages(
        &conn,
        &binding.ecky_thread_id,
        codex_takeover::CODEX_PROVIDER_ID,
        &binding.codex_thread_id,
        messages,
    )?;
    if persisted > 0 {
        let title = crate::db::get_thread_title(&conn, &binding.ecky_thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .unwrap_or_else(|| binding.label.clone());
        let ecky_messages =
            crate::db::get_thread_messages_for_context(&conn, &binding.ecky_thread_id)
                .unwrap_or_default();
        let canonical = crate::context::build_thread_summary(&title, &ecky_messages);
        let local = codex_takeover::list_provider_messages(
            &conn,
            &binding.ecky_thread_id,
            codex_takeover::CODEX_PROVIDER_ID,
            30,
        )?;
        let handoff = codex_takeover::build_provider_handoff_summary(&canonical, &local);
        crate::db::update_thread_summary(&conn, &binding.ecky_thread_id, &handoff)
            .map_err(|error| AppError::persistence(error.to_string()))?;
    }
    Ok(persisted)
}

async fn persist_latest_codex_history(
    state: &AppState,
    binding: &CodexTakeoverBinding,
) -> AppResult<usize> {
    let provider_page = state
        .codex_app_server
        .message_page(&binding.codex_thread_id, None, None)
        .await?;
    persist_codex_messages(state, binding, &provider_page.messages).await
}

async fn resume_binding(
    state: &AppState,
    binding: &CodexTakeoverBinding,
    force_resume_request: bool,
) -> AppResult<()> {
    resume_binding_with_policy(
        state,
        binding,
        force_resume_request,
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify),
    )
    .await
}

async fn resume_binding_with_policy(
    state: &AppState,
    binding: &CodexTakeoverBinding,
    force_resume_request: bool,
    policy: ProviderTurnPolicy,
) -> AppResult<()> {
    let active_runtime = state
        .codex_app_server
        .runtime(&binding.codex_thread_id)
        .await;
    if active_runtime.active_turn_id.is_some()
        && state
            .provider_turn_policy(&binding.ecky_thread_id)
            .await
            .is_some_and(|current| !current.is_prompt_based())
    {
        // Active routed turns keep their accepted model/tool boundary until
        // terminal completion, regardless of current Jev settings.
        return Ok(());
    }
    let endpoint = require_mcp_endpoint(state)?;
    let title = project_title(state, &binding.ecky_thread_id).await?;
    let handoff = canonical_handoff(state, &binding.ecky_thread_id).await?;
    let refresh_developer_instructions =
        binding.bootstrap_version < codex_takeover::CODEX_BOOTSTRAP_VERSION;
    state
        .codex_app_server
        .resume_thread_with_policy(
            binding,
            &title,
            &endpoint,
            &handoff,
            refresh_developer_instructions,
            force_resume_request,
            configured_codex_model(state).as_deref(),
            policy,
        )
        .await?;
    state
        .codex_app_server
        .name_thread(&binding.codex_thread_id, &title)
        .await?;
    if refresh_developer_instructions {
        let conn = state.db.lock().await;
        codex_takeover::record_bootstrap_version(
            &conn,
            &binding.ecky_thread_id,
            codex_takeover::CODEX_PROVIDER_ID,
            &binding.codex_thread_id,
            codex_takeover::CODEX_BOOTSTRAP_VERSION,
            now_seconds(),
        )?;
    }
    Ok(())
}

pub(crate) async fn activate_bound_writer(state: &AppState, ecky_thread_id: &str) -> AppResult<()> {
    let binding = {
        let conn = state.db.lock().await;
        codex_takeover::get_binding(&conn, ecky_thread_id)?
    };
    if let Some(binding) = binding {
        // Explicit activation may refresh this client's subscription. It never
        // claims, interrupts, or releases another client's writer.
        resume_binding(state, &binding, true).await?;
    }
    Ok(())
}

async fn refresh_binding_workspace(
    app: &tauri::AppHandle,
    state: &AppState,
    binding: CodexTakeoverBinding,
) -> AppResult<CodexTakeoverBinding> {
    let title = project_title(state, &binding.ecky_thread_id).await?;
    let cwd = provider_project_cwd(app, state, &binding.ecky_thread_id, &title).await?;
    if binding.cwd == cwd && binding.label == title {
        return Ok(binding);
    }
    let conn = state.db.lock().await;
    codex_takeover::refresh_binding_metadata(&conn, &binding, &title, &cwd, now_seconds())
}

async fn ensure_binding(
    app: &tauri::AppHandle,
    state: &AppState,
    ecky_thread_id: &str,
) -> AppResult<CodexTakeoverBinding> {
    if let Some(binding) = {
        let conn = state.db.lock().await;
        codex_takeover::get_binding(&conn, ecky_thread_id)?
    } {
        return refresh_binding_workspace(app, state, binding).await;
    }

    let _creation = CODEX_BINDING_CREATE_LOCK.lock().await;
    if let Some(binding) = {
        let conn = state.db.lock().await;
        codex_takeover::get_binding(&conn, ecky_thread_id)?
    } {
        return refresh_binding_workspace(app, state, binding).await;
    }

    let endpoint = require_mcp_endpoint(state)?;
    let title = project_title(state, ecky_thread_id).await?;
    let cwd = provider_project_cwd(app, state, ecky_thread_id, &title).await?;
    let handoff = canonical_handoff(state, ecky_thread_id).await?;
    let thread = state
        .codex_app_server
        .start_thread(
            ecky_thread_id,
            &title,
            &cwd,
            &endpoint,
            &handoff,
            configured_codex_model(state).as_deref(),
        )
        .await?;
    let binding = {
        let conn = state.db.lock().await;
        codex_takeover::bind_owned_thread(
            &conn,
            ecky_thread_id,
            &thread.id,
            &title,
            &cwd,
            now_seconds(),
        )
    };
    let binding = match binding {
        Ok(binding) => binding,
        Err(error) => {
            let cleanup = state.codex_app_server.delete_thread(&thread.id).await;
            return match cleanup {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(AppError::with_details(
                    error.code,
                    error.message,
                    format!(
                        "{}\nCreated Codex thread cleanup also failed: {}",
                        error.details.unwrap_or_default(),
                        codex_takeover::error_text(&cleanup_error),
                    ),
                )),
            };
        }
    };
    state
        .codex_app_server
        .name_thread(&binding.codex_thread_id, &title)
        .await?;
    Ok(binding)
}

async fn dispatch_queue_for(state: &AppState, binding: &CodexTakeoverBinding) -> AppResult<()> {
    dispatch_queue_for_impl(state, binding, None, true).await
}

async fn rotate_conflicted_codex_cursor(
    state: &AppState,
    binding: &CodexTakeoverBinding,
    queue_id: &str,
) -> AppResult<bool> {
    let already_rotated = {
        let conn = state.db.lock().await;
        codex_takeover::writer_conflict_rotated_for_queue(&conn, queue_id)?
    };
    if already_rotated {
        return Ok(false);
    }
    let endpoint = require_mcp_endpoint(state)?;
    let title = project_title(state, &binding.ecky_thread_id).await?;
    let handoff = canonical_handoff(state, &binding.ecky_thread_id).await?;
    let thread = state
        .codex_app_server
        .start_thread(
            &binding.ecky_thread_id,
            &title,
            &binding.cwd,
            &endpoint,
            &handoff,
            configured_codex_model(state).as_deref(),
        )
        .await?;
    let saved = async {
        state
            .codex_app_server
            .name_thread(&thread.id, &title)
            .await?;
        let conn = state.db.lock().await;
        codex_takeover::rotate_agent_binding(
            &conn,
            &binding.ecky_thread_id,
            codex_takeover::CODEX_PROVIDER_ID,
            &thread.id,
            &format!("writer-conflict:{queue_id}"),
            now_seconds(),
        )?;
        Ok::<(), AppError>(())
    }
    .await;
    if let Err(error) = saved {
        let _ = state.codex_app_server.delete_thread(&thread.id).await;
        return Err(error);
    }
    Ok(true)
}

fn codex_jev_eval_route(
    route: &crate::jev_classifier::AcceptedRoute,
    model_ceiling: Option<String>,
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
        model_ceiling,
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

async fn persist_codex_pre_dispatch_failure(
    state: &AppState,
    mut seed: crate::llm_eval::EvalRunSeed,
    diagnostic: &str,
    redaction_secret: Option<&str>,
    failure_stage: &str,
    accepted_route: Option<&crate::jev_classifier::AcceptedRoute>,
) {
    let occurred_at = now_seconds();
    let mut events = vec![crate::llm_eval::EvalEvent {
        sequence: 0,
        step_index: None,
        kind: crate::llm_eval::EvalEventKind::System,
        state: "admitted".into(),
        name: Some("request".into()),
        summary: Some("Queued Codex request admitted".into()),
        input: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
            "promptSha256": format!("sha256:{:x}", sha2::Sha256::digest(seed.prompt.as_bytes())),
            "promptChars": seed.prompt.chars().count(),
            "threadId": seed.thread_id,
            "externalThreadId": seed.external_thread_id,
            "startingVersionId": seed.starting_version_id,
            "startingInputDigest": seed.starting_input_digest,
        }))),
        output: None,
        error: None,
        occurred_at,
    }];
    if let Some(route) = accepted_route {
        seed.turn_intent = route.intent;
        seed.answer_first_required = route.answer_first;
        seed.jev_route = Some(codex_jev_eval_route(route, seed.model.clone()));
        events.push(crate::llm_eval::EvalEvent {
            sequence: 1,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::System,
            state: "failed".into(),
            name: Some("delivery".into()),
            summary: Some("Accepted Jev route failed before Codex turn/start".into()),
            input: None,
            output: None,
            error: Some(diagnostic.to_string()),
            occurred_at,
        });
    } else {
        events.push(crate::llm_eval::EvalEvent {
            sequence: 1,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::System,
            state: "error".into(),
            name: Some(
                if failure_stage == "classification" {
                    "jev"
                } else {
                    "jev.preflight"
                }
                .into(),
            ),
            summary: Some(format!(
                "Jev {failure_stage} failed before Codex turn/start"
            )),
            input: None,
            output: None,
            error: Some(diagnostic.to_string()),
            occurred_at,
        });
    }
    state
        .codex_app_server
        .record_pre_dispatch_eval_failure(seed, redaction_secret, diagnostic, events)
        .await;
}

#[cfg(test)]
async fn dispatch_queue_for_with_classifier<C: crate::jev_classifier::TurnClassifier>(
    state: &AppState,
    binding: &CodexTakeoverBinding,
    classifier: &C,
) -> AppResult<()> {
    dispatch_queue_for_impl(state, binding, Some(classifier), false).await
}

async fn dispatch_queue_for_impl(
    state: &AppState,
    binding: &CodexTakeoverBinding,
    classifier: Option<&dyn crate::jev_classifier::TurnClassifier>,
    use_configured_classifier: bool,
) -> AppResult<()> {
    let mut binding = binding.clone();
    let mut rotated_this_dispatch = false;
    loop {
        binding = binding_for(state, &binding.ecky_thread_id).await?;
        let mut runtime = state
            .codex_app_server
            .runtime(&binding.codex_thread_id)
            .await;
        if runtime.active_turn_id.is_some() {
            runtime = state
                .codex_app_server
                .reconcile_runtime(&binding.codex_thread_id)
                .await?;
        }
        if runtime.phase == "stopping" {
            return Ok(());
        }
        let queue = {
            let conn = state.db.lock().await;
            codex_takeover::list_queue(&conn, &binding.ecky_thread_id)?
        };
        let Some(head) = queue.first().cloned() else {
            return Ok(());
        };
        if head.status == "failed" || head.status == "sending" {
            return Ok(());
        }

        if runtime.active_turn_id.is_some() {
            return Ok(());
        }

        let claimed = {
            let conn = state.db.lock().await;
            codex_takeover::claim_queue_item(&conn, &head.id, now_seconds())?
        };
        if !claimed {
            return Ok(());
        }

        let config_snapshot = state.config.lock().unwrap().clone();
        let run_id = uuid::Uuid::new_v4().to_string();
        let eval_started_at = now_seconds();
        let ceiling_model = (!config_snapshot.provider_models.codex.trim().is_empty())
            .then(|| config_snapshot.provider_models.codex.trim().to_owned());
        let mut pre_dispatch_seed = crate::llm_eval::EvalRunSeed {
            run_id: run_id.clone(),
            thread_id: binding.ecky_thread_id.clone(),
            external_thread_id: binding.codex_thread_id.clone(),
            provider: codex_takeover::CODEX_PROVIDER_ID.into(),
            model: ceiling_model.clone(),
            effort: None,
            prompt_version: format!(
                "codex-app-server-v{}-{}",
                codex_takeover::CODEX_BOOTSTRAP_VERSION,
                crate::jev_classifier::CLASSIFIER_POLICY_VERSION
            ),
            prompt: head.prompt_text.clone(),
            starting_version_id: None,
            starting_input_digest: None,
            expected_red_rounds: 0,
            turn_intent: ProviderTurnIntent::Answer,
            answer_first_required: false,
            jev_route: None,
        };
        let configured_classifier =
            if use_configured_classifier && config_snapshot.jev_classifier.enabled {
                if let Err(error) = config_snapshot.jev_classifier.validate() {
                    persist_codex_pre_dispatch_failure(
                        state,
                        pre_dispatch_seed.clone(),
                        &codex_takeover::error_text(&error),
                        Some(config_snapshot.jev_classifier.api_key.as_str()),
                        "configuration",
                        None,
                    )
                    .await;
                    return fail_claimed_queue(state, &head.id, error).await;
                }
                match crate::jev_classifier::JevSdkClassifier::new(
                    &config_snapshot.jev_classifier.api_key,
                ) {
                    Ok(classifier) => Some(classifier),
                    Err(error) => {
                        persist_codex_pre_dispatch_failure(
                            state,
                            pre_dispatch_seed.clone(),
                            &codex_takeover::error_text(&error),
                            Some(config_snapshot.jev_classifier.api_key.as_str()),
                            "configuration",
                            None,
                        )
                        .await;
                        return fail_claimed_queue(state, &head.id, error).await;
                    }
                }
            } else {
                None
            };
        let classifier = classifier.or_else(|| {
            configured_classifier
                .as_ref()
                .map(|classifier| classifier as &dyn crate::jev_classifier::TurnClassifier)
        });

        let preflight_policy = if classifier.is_some() {
            ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer)
        } else {
            ProviderTurnPolicy::prompt_based()
        };
        state
            .set_provider_turn_policy(&binding.ecky_thread_id, preflight_policy)
            .await;

        if let Err(error) = persist_latest_codex_history(state, &binding).await {
            state.push_log(format!(
                "[CODEX] read-only history backfill failed before delivery for {}: {}",
                binding.ecky_thread_id,
                codex_takeover::error_text(&error)
            ));
        }
        if let Err(error) =
            resume_binding_with_policy(state, &binding, true, preflight_policy).await
        {
            let writer_conflict =
                codex_takeover::is_active_writer_error(&codex_takeover::error_text(&error));
            if classifier.is_some() || writer_conflict {
                persist_codex_pre_dispatch_failure(
                    state,
                    pre_dispatch_seed.clone(),
                    &codex_takeover::error_text(&error),
                    config_snapshot
                        .jev_classifier
                        .enabled
                        .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                    "adapter_setup",
                    None,
                )
                .await;
            }
            if writer_conflict && !rotated_this_dispatch {
                match rotate_conflicted_codex_cursor(state, &binding, &head.id).await {
                    Ok(true) => {
                        rotated_this_dispatch = true;
                        let conn = state.db.lock().await;
                        codex_takeover::defer_queue_item(
                            &conn,
                            &head.id,
                            &codex_takeover::error_text(&error),
                            now_seconds(),
                        )?;
                        continue;
                    }
                    Ok(false) => {}
                    Err(rotation_error) => {
                        return fail_claimed_queue(state, &head.id, rotation_error).await;
                    }
                }
            }
            return fail_claimed_queue(state, &head.id, error).await;
        }
        let reconciled = state
            .codex_app_server
            .runtime(&binding.codex_thread_id)
            .await;
        if reconciled.active_turn_id.is_some() {
            let error = AppError::conflict(
                "Codex became active while the queued route was being prepared.",
            );
            if classifier.is_some() {
                persist_codex_pre_dispatch_failure(
                    state,
                    pre_dispatch_seed.clone(),
                    &codex_takeover::error_text(&error),
                    config_snapshot
                        .jev_classifier
                        .enabled
                        .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                    "adapter_setup",
                    None,
                )
                .await;
            }
            return fail_claimed_queue(state, &head.id, error).await;
        }

        let starting_identity = {
            let conn = state.db.lock().await;
            match crate::llm_eval::latest_version_identity(&conn, &binding.ecky_thread_id) {
                Ok(identity) => identity,
                Err(error) => {
                    let app_error = AppError::persistence(error);
                    if classifier.is_some() {
                        persist_codex_pre_dispatch_failure(
                            state,
                            pre_dispatch_seed.clone(),
                            &codex_takeover::error_text(&app_error),
                            config_snapshot
                                .jev_classifier
                                .enabled
                                .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                            "request_setup",
                            None,
                        )
                        .await;
                    }
                    record_queue_delivery_error(&conn, &head.id, &app_error)?;
                    return Err(app_error);
                }
            }
        };
        let (starting_version_id, starting_input_digest) =
            starting_identity.map_or((None, None), |(id, digest)| (Some(id), digest));
        pre_dispatch_seed.starting_version_id = starting_version_id.clone();
        pre_dispatch_seed.starting_input_digest = starting_input_digest.clone();
        let route = if let Some(classifier) = classifier {
            let discovered_models = if ceiling_model.is_some() {
                match state.codex_app_server.list_models().await {
                    Ok(models) => models,
                    Err(error) => {
                        persist_codex_pre_dispatch_failure(
                            state,
                            pre_dispatch_seed.clone(),
                            &codex_takeover::error_text(&error),
                            config_snapshot
                                .jev_classifier
                                .enabled
                                .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                            "model_catalog",
                            None,
                        )
                        .await;
                        let conn = state.db.lock().await;
                        record_queue_delivery_error(&conn, &head.id, &error)?;
                        return Err(error);
                    }
                }
            } else {
                Vec::new()
            };
            let has_image = head
                .attachments
                .iter()
                .any(|attachment| attachment.kind == crate::contracts::AttachmentKind::Image);
            let account_type = state.codex_app_server.account_type().await.ok();
            let model_provider = state
                .codex_app_server
                .thread_model_provider(&binding.codex_thread_id)
                .await;
            let api_cost_basis_confirmed = account_type.as_deref() == Some("apiKey")
                && model_provider.as_deref() == Some("openai");
            let candidates = match crate::jev_classifier::eligible_model_candidates_at(
                &discovered_models,
                ceiling_model.as_deref(),
                has_image,
                api_cost_basis_confirmed,
                chrono::Utc::now().date_naive(),
            ) {
                Ok(candidates) => candidates,
                Err(error) => {
                    persist_codex_pre_dispatch_failure(
                        state,
                        pre_dispatch_seed.clone(),
                        &codex_takeover::error_text(&error),
                        config_snapshot
                            .jev_classifier
                            .enabled
                            .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                        "model_selection",
                        None,
                    )
                    .await;
                    return fail_claimed_queue(state, &head.id, error).await;
                }
            };
            let context_result = {
                let conn = state.db.lock().await;
                let recent = codex_takeover::list_provider_messages(
                    &conn,
                    &binding.ecky_thread_id,
                    codex_takeover::CODEX_PROVIDER_ID,
                    crate::jev_classifier::MAX_RECENT_MESSAGES,
                );
                let summary = crate::db::get_thread_summary(&conn, &binding.ecky_thread_id)
                    .map_err(|error| AppError::persistence(error.to_string()));
                recent
                    .and_then(|recent| summary.map(|summary| (recent, summary.unwrap_or_default())))
            };
            let (recent, summary) = match context_result {
                Ok(context) => context,
                Err(error) => {
                    persist_codex_pre_dispatch_failure(
                        state,
                        pre_dispatch_seed.clone(),
                        &codex_takeover::error_text(&error),
                        config_snapshot
                            .jev_classifier
                            .enabled
                            .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                        "context_assembly",
                        None,
                    )
                    .await;
                    return fail_claimed_queue(state, &head.id, error).await;
                }
            };
            let request = crate::jev_classifier::ClassifierRequest::bounded(
                &head.prompt_text,
                recent
                    .into_iter()
                    .map(|message| crate::jev_classifier::RecentMessage {
                        role: message.role,
                        content: message.content,
                    }),
                &summary,
                &format!(
                    "queued provider turn; starting version {}; digest {}",
                    starting_version_id.as_deref().unwrap_or("none"),
                    starting_input_digest.as_deref().unwrap_or("unknown")
                ),
                head.attachments
                    .iter()
                    .map(|attachment| crate::jev_classifier::AttachmentModality {
                        kind: attachment.kind.as_str().to_owned(),
                        explanation: attachment.explanation.clone(),
                    })
                    .collect(),
                candidates.clone(),
            );
            let request_event = crate::llm_eval::EvalEvent {
                sequence: 0,
                step_index: None,
                kind: crate::llm_eval::EvalEventKind::System,
                state: "admitted".into(),
                name: Some("request".into()),
                summary: Some("Queued Codex request admitted for Jev routing".into()),
                input: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
                    "promptSha256": format!("sha256:{:x}", sha2::Sha256::digest(head.prompt_text.as_bytes())),
                    "promptChars": head.prompt_text.chars().count(),
                    "threadId": binding.ecky_thread_id,
                    "externalThreadId": binding.codex_thread_id,
                    "startingVersionId": starting_version_id,
                    "startingInputDigest": starting_input_digest,
                }))),
                output: None,
                error: None,
                occurred_at: eval_started_at,
            };
            let jev_input = serde_json::json!({
                "policyVersion": crate::jev_classifier::CLASSIFIER_POLICY_VERSION,
                "promptSha256": format!("sha256:{:x}", sha2::Sha256::digest(request.current_prompt.as_bytes())),
                "promptChars": request.current_prompt.chars().count(),
                "promptTruncated": request.current_prompt_truncated,
                "contextTruncated": request.context_truncated,
                "recentMessageCount": request.recent_dialogue.len(),
                "eligibleModels": request.eligible_models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
                "attachmentCount": request.attachments.len(),
            });
            let route = match classifier.classify(request).await {
                Ok(route) => route,
                Err(error) => {
                    let diagnostic = codex_takeover::error_text(&error);
                    let events = vec![
                        request_event,
                        crate::llm_eval::EvalEvent {
                            sequence: 1,
                            step_index: None,
                            kind: crate::llm_eval::EvalEventKind::System,
                            state: "error".into(),
                            name: Some("jev".into()),
                            summary: Some(
                                "Jev classification failed before provider dispatch".into(),
                            ),
                            input: Some(crate::llm_eval::EvalPayload::new(jev_input)),
                            output: None,
                            error: Some(diagnostic.clone()),
                            occurred_at: now_seconds(),
                        },
                    ];
                    state
                        .codex_app_server
                        .record_pre_dispatch_eval_failure(
                            pre_dispatch_seed.clone(),
                            config_snapshot
                                .jev_classifier
                                .enabled
                                .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                            &diagnostic,
                            events,
                        )
                        .await;
                    let conn = state.db.lock().await;
                    codex_takeover::fail_classifier_queue_item(
                        &conn,
                        &head.id,
                        &diagnostic,
                        now_seconds(),
                    )?;
                    return Err(error);
                }
            };
            let route_valid = route.model.as_ref().is_none_or(|model| {
                candidates.iter().any(|candidate| candidate.id == *model)
                    && discovered_models
                        .iter()
                        .any(|discovered| discovered == model)
            });
            if !route_valid {
                let error =
                    AppError::provider("Jev selected a model outside the verified eligible set.");
                persist_codex_pre_dispatch_failure(
                    state,
                    pre_dispatch_seed.clone(),
                    &codex_takeover::error_text(&error),
                    config_snapshot
                        .jev_classifier
                        .enabled
                        .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                    "model_validation",
                    Some(&route),
                )
                .await;
                let conn = state.db.lock().await;
                record_queue_delivery_error(&conn, &head.id, &error)?;
                return Err(error);
            }
            let current_config = state.config.lock().unwrap().clone();
            let current_binding = match binding_for(state, &binding.ecky_thread_id).await {
                Ok(binding) => binding,
                Err(error) => {
                    persist_codex_pre_dispatch_failure(
                        state,
                        pre_dispatch_seed.clone(),
                        &codex_takeover::error_text(&error),
                        config_snapshot
                            .jev_classifier
                            .enabled
                            .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                        "stale_route_check",
                        Some(&route),
                    )
                    .await;
                    return fail_claimed_queue(state, &head.id, error).await;
                }
            };
            let current_head = {
                let conn = state.db.lock().await;
                codex_takeover::list_queue(&conn, &binding.ecky_thread_id)
            };
            let current_head = match current_head {
                Ok(queue) => queue.into_iter().next(),
                Err(error) => {
                    persist_codex_pre_dispatch_failure(
                        state,
                        pre_dispatch_seed.clone(),
                        &codex_takeover::error_text(&AppError::persistence(error.to_string())),
                        config_snapshot
                            .jev_classifier
                            .enabled
                            .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                        "stale_route_check",
                        Some(&route),
                    )
                    .await;
                    return fail_claimed_queue(state, &head.id, error).await;
                }
            };
            if current_head.is_none() {
                return Ok(());
            }
            let current_identity = {
                let conn = state.db.lock().await;
                crate::llm_eval::latest_version_identity(&conn, &binding.ecky_thread_id)
            };
            let current_identity = match current_identity {
                Ok(identity) => identity,
                Err(error) => {
                    let app_error = AppError::persistence(error);
                    persist_codex_pre_dispatch_failure(
                        state,
                        pre_dispatch_seed.clone(),
                        &codex_takeover::error_text(&app_error),
                        config_snapshot
                            .jev_classifier
                            .enabled
                            .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                        "stale_route_check",
                        Some(&route),
                    )
                    .await;
                    return fail_claimed_queue(state, &head.id, app_error).await;
                }
            };
            let current_identity = current_identity
                .map(|(id, digest)| (Some(id), digest))
                .unwrap_or((None, None));
            let current_runtime = state
                .codex_app_server
                .runtime(&binding.codex_thread_id)
                .await;
            if current_config.jev_classifier.enabled != config_snapshot.jev_classifier.enabled
                || current_config.jev_classifier.api_key != config_snapshot.jev_classifier.api_key
                || current_config.provider_models.codex != config_snapshot.provider_models.codex
                || current_binding.codex_thread_id != binding.codex_thread_id
                || current_identity != (starting_version_id.clone(), starting_input_digest.clone())
                || current_runtime.phase == "stopping"
                || current_runtime.active_turn_id.is_some()
                || current_head
                    .as_ref()
                    .is_none_or(|item| item.id != head.id || item.status != "sending")
            {
                let error = AppError::conflict(
                    "Jev route became stale before dispatch; retry this queued turn.",
                );
                persist_codex_pre_dispatch_failure(
                    state,
                    pre_dispatch_seed.clone(),
                    &codex_takeover::error_text(&error),
                    config_snapshot
                        .jev_classifier
                        .enabled
                        .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                    "stale_route_check",
                    Some(&route),
                )
                .await;
                let conn = state.db.lock().await;
                record_queue_delivery_error(&conn, &head.id, &error)?;
                return Err(error);
            }
            Some(route)
        } else {
            None
        };
        let (policy, effective_model, jev_route) = if let Some(route) = route {
            let policy = ProviderTurnPolicy::routed(route.intent, route.answer_first);
            let model = route.model.clone().or_else(|| ceiling_model.clone());
            let evidence = codex_jev_eval_route(&route, ceiling_model.clone());
            (policy, model, Some(evidence))
        } else {
            let policy = ProviderTurnPolicy::prompt_based();
            (policy, ceiling_model.clone(), None)
        };
        let jev_route_accepted = jev_route.is_some();
        let eval_seed = crate::llm_eval::EvalRunSeed {
            run_id,
            thread_id: binding.ecky_thread_id.clone(),
            external_thread_id: binding.codex_thread_id.clone(),
            provider: codex_takeover::CODEX_PROVIDER_ID.into(),
            model: effective_model.clone(),
            effort: None,
            prompt_version: format!(
                "codex-app-server-v{}-{}",
                codex_takeover::CODEX_BOOTSTRAP_VERSION,
                jev_route
                    .as_ref()
                    .map(|route| route.policy_version.as_str())
                    .unwrap_or("prompt")
            ),
            prompt: head.prompt_text.clone(),
            starting_version_id,
            starting_input_digest,
            expected_red_rounds: 0,
            turn_intent: policy.intent(),
            answer_first_required: policy.requires_answer_first(),
            jev_route,
        };
        let delivery_started = {
            let conn = state.db.lock().await;
            codex_takeover::begin_queue_delivery(&conn, &head.id, now_seconds())
        };
        match delivery_started {
            Ok(true) => {
                if jev_route_accepted {
                    let conn = state.db.lock().await;
                    crate::services::jev_classifications::save_accepted(
                        &conn,
                        &binding.ecky_thread_id,
                        codex_takeover::CODEX_PROVIDER_ID,
                        &head.id,
                        None,
                        policy.intent(),
                        &eval_seed
                            .jev_route
                            .as_ref()
                            .expect("accepted Jev route")
                            .intent_probabilities,
                        now_seconds(),
                    )?;
                    if let Some(app) = state.app_handle.lock().unwrap().clone() {
                        let _ = app.emit(
                            "jev-classification-accepted",
                            serde_json::json!({"threadId": binding.ecky_thread_id}),
                        );
                    }
                }
                state
                    .set_provider_turn_policy(&binding.ecky_thread_id, policy)
                    .await;
                if jev_route_accepted {
                    state
                        .codex_app_server
                        .mark_routed_delivery_pending(&binding.codex_thread_id)
                        .await;
                }
            }
            Ok(false) => return Ok(()),
            Err(error) => return fail_claimed_queue(state, &head.id, error).await,
        }
        match state
            .codex_app_server
            .start_turn_with_eval_seed_and_secret(
                &binding.codex_thread_id,
                &head.prompt_text,
                effective_model.as_deref(),
                &head.attachments,
                policy,
                eval_seed,
                (config_snapshot.jev_classifier.enabled)
                    .then_some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await
        {
            Ok(turn_id) => {
                let conn = state.db.lock().await;
                codex_takeover::persist_finished_provider_messages(
                    &conn,
                    &binding.ecky_thread_id,
                    codex_takeover::CODEX_PROVIDER_ID,
                    &binding.codex_thread_id,
                    &[CodexDialogueMessage {
                        id: format!("codex:{}:{}:user:0", binding.codex_thread_id, turn_id),
                        role: "user".to_string(),
                        content: head.prompt_text.clone(),
                        status: "success".to_string(),
                        timestamp: head.created_at,
                        attachments: head.attachments.clone(),
                        provider_event_kind: None,
                    }],
                )?;
                if jev_route_accepted {
                    crate::services::jev_classifications::bind_message(
                        &conn,
                        &binding.ecky_thread_id,
                        codex_takeover::CODEX_PROVIDER_ID,
                        &head.id,
                        &format!("codex:{}:{}:user:0", binding.codex_thread_id, turn_id),
                    )?;
                    if let Some(app) = state.app_handle.lock().unwrap().clone() {
                        let _ = app.emit(
                            "jev-classification-accepted",
                            serde_json::json!({"threadId": binding.ecky_thread_id}),
                        );
                    }
                }
                codex_takeover::complete_queue_item(&conn, &head.id)?;
            }
            Err(error) => {
                if !rotated_this_dispatch
                    && codex_takeover::is_active_writer_error(&codex_takeover::error_text(&error))
                {
                    match rotate_conflicted_codex_cursor(state, &binding, &head.id).await {
                        Ok(true) => {
                            rotated_this_dispatch = true;
                            let conn = state.db.lock().await;
                            codex_takeover::defer_queue_item(
                                &conn,
                                &head.id,
                                &codex_takeover::error_text(&error),
                                now_seconds(),
                            )?;
                            continue;
                        }
                        Ok(false) => {}
                        Err(rotation_error) => {
                            return fail_claimed_queue(state, &head.id, rotation_error).await;
                        }
                    }
                }
                let conn = state.db.lock().await;
                record_queue_delivery_error(&conn, &head.id, &error)?;
                return Err(error);
            }
        }
        let started = state
            .codex_app_server
            .runtime(&binding.codex_thread_id)
            .await;
        if started.active_turn_id.is_some() {
            return Ok(());
        }
    }
}

pub fn initialize_codex_queue_supervisor(state: AppState, app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(CODEX_QUEUE_POLL_INTERVAL) => {}
                _ = codex_takeover::wait_for_queue_supervisor() => {}
            }
            persist_completed_codex_eval_runs(&state, &app).await;
            if state.config.lock().unwrap().connection_type.as_deref() != Some("provider:codex") {
                continue;
            }
            let bindings = {
                let conn = state.db.lock().await;
                let now = now_seconds();
                if let Err(error) = codex_takeover::recover_retryable_failures(&conn, now) {
                    state.push_log(format!(
                        "[CODEX] prompt queue recovery failed: {}",
                        codex_takeover::error_text(&error)
                    ));
                    continue;
                }
                match codex_takeover::pending_queue_bindings(&conn, now) {
                    Ok(bindings) => bindings,
                    Err(error) => {
                        state.push_log(format!(
                            "[CODEX] prompt queue scan failed: {}",
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
                    let result = dispatch_queue_for(&dispatch_state, &binding).await;
                    if let Err(error) = &result {
                        let error_text = codex_takeover::error_text(error);
                        if !codex_takeover::is_retryable_delivery_error(&error_text) {
                            dispatch_state.push_log(format!(
                                "[CODEX] prompt queue dispatch failed for {}: {}",
                                binding.ecky_thread_id, error_text
                            ));
                        }
                    }
                    let _ = dispatch_app.emit(
                        "codex-provider-updated",
                        serde_json::json!({
                            "threadId": binding.codex_thread_id,
                            "method": if result.is_ok() { "queue/dispatched" } else { "queue/failed" },
                        }),
                    );
                });
            }
        }
    });
}

#[tauri::command]
#[specta::specta]
pub async fn get_codex_takeover(
    ecky_thread_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<CodexTakeoverSnapshot>> {
    let binding = {
        let conn = state.db.lock().await;
        codex_takeover::get_binding(&conn, &ecky_thread_id)?
    };
    match binding {
        Some(binding) => {
            let snapshot = snapshot_for(&state, binding.clone(), None).await?;
            let backfill_state = state.inner().clone();
            tauri::async_runtime::spawn(async move {
                let Ok(_backfill) = CODEX_HISTORY_BACKFILL_LOCK.try_lock() else {
                    return;
                };
                let mut cursor = None;
                loop {
                    let page = match backfill_state
                        .codex_app_server
                        .message_page(&binding.codex_thread_id, cursor.clone(), None)
                        .await
                    {
                        Ok(page) => page,
                        Err(error) => {
                            backfill_state.push_log(format!(
                                "[CODEX] background history backfill failed for {}: {}",
                                binding.ecky_thread_id,
                                codex_takeover::error_text(&error)
                            ));
                            break;
                        }
                    };
                    let next_cursor = page.next_cursor.clone();
                    match persist_codex_messages(&backfill_state, &binding, &page.messages).await {
                        Ok(changed) if changed > 0 => {
                            let _ = app.emit(
                                "codex-provider-updated",
                                serde_json::json!({
                                    "threadId": binding.codex_thread_id,
                                    "method": "history/persisted",
                                }),
                            );
                        }
                        Ok(_) => {}
                        Err(error) => {
                            backfill_state.push_log(format!(
                                "[CODEX] background history persistence failed for {}: {}",
                                binding.ecky_thread_id,
                                codex_takeover::error_text(&error)
                            ));
                            break;
                        }
                    }
                    if next_cursor.is_none() || next_cursor == cursor {
                        break;
                    }
                    cursor = next_cursor;
                }
            });
            Ok(Some(snapshot))
        }
        None => Ok(None),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_codex_takeover_messages(
    input: CodexMessagePageInput,
    state: State<'_, AppState>,
) -> AppResult<CodexMessagePage> {
    let conn = state.db.lock().await;
    codex_takeover::provider_message_page(
        &conn,
        &input.ecky_thread_id,
        codex_takeover::CODEX_PROVIDER_ID,
        input.cursor.as_deref(),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn send_codex_takeover_prompt(
    input: CodexPromptInput,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CodexTakeoverSnapshot> {
    require_codex_provider_mode(&state)?;
    let existing_binding = {
        let conn = state.db.lock().await;
        codex_takeover::get_binding(&conn, &input.ecky_thread_id)?
    };
    let binding = match existing_binding {
        Some(binding) => binding,
        None => ensure_binding(&app, &state, &input.ecky_thread_id).await?,
    };
    {
        let conn = state.db.lock().await;
        codex_takeover::enqueue_prompt_with_attachments(
            &conn,
            &input.ecky_thread_id,
            &input.prompt_text,
            &input.attachments,
            now_seconds(),
        )?;
    }
    codex_takeover::notify_queue_supervisor();
    let snapshot = queued_snapshot_for(&state, binding.clone()).await?;
    let delivery_state = state.inner().clone();
    let delivery_app = app.clone();
    let codex_thread_id = binding.codex_thread_id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = dispatch_queue_for(&delivery_state, &binding).await;
        let _ = delivery_app.emit(
            "codex-provider-updated",
            serde_json::json!({
                "threadId": codex_thread_id,
                "method": "queue/dispatched",
            }),
        );
    });
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub async fn dispatch_codex_prompt_queue(
    ecky_thread_id: String,
    state: State<'_, AppState>,
) -> AppResult<CodexTakeoverSnapshot> {
    let binding = binding_for(&state, &ecky_thread_id).await?;
    dispatch_queue_for(&state, &binding).await?;
    snapshot_for(&state, binding_for(&state, &ecky_thread_id).await?, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn steer_codex_takeover(
    input: CodexSteerInput,
    state: State<'_, AppState>,
) -> AppResult<CodexTakeoverSnapshot> {
    let binding = binding_for(&state, &input.ecky_thread_id).await?;
    steer_bound_codex_turn(&state, &binding, &input).await
}

async fn steer_bound_codex_turn(
    state: &AppState,
    binding: &CodexTakeoverBinding,
    input: &CodexSteerInput,
) -> AppResult<CodexTakeoverSnapshot> {
    steer_bound_codex_turn_with_classifier(state, binding, input, None).await
}

async fn steer_bound_codex_turn_with_classifier(
    state: &AppState,
    binding: &CodexTakeoverBinding,
    input: &CodexSteerInput,
    classifier_override: Option<&dyn crate::jev_classifier::TurnClassifier>,
) -> AppResult<CodexTakeoverSnapshot> {
    let steer_lock = {
        let mut locks = state.codex_steer_locks.lock().await;
        locks
            .entry(binding.ecky_thread_id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _steer_guard = steer_lock.lock_owned().await;
    resume_binding(state, binding, false).await?;
    let runtime = state
        .codex_app_server
        .runtime(&binding.codex_thread_id)
        .await;
    if runtime.active_turn_id.as_deref() != Some(&input.expected_turn_id) {
        return Err(AppError::conflict(format!(
            "Codex active turn changed; expected {}, current {}.",
            input.expected_turn_id,
            runtime.active_turn_id.as_deref().unwrap_or("none")
        )));
    }
    if runtime.phase == "stopping" {
        return Err(AppError::conflict(
            "Codex active turn is stopping; STEER was not delivered.",
        ));
    }
    let active_model = state
        .codex_app_server
        .model_for_turn(&binding.codex_thread_id, &input.expected_turn_id)
        .await;
    let config_snapshot = state.config.lock().unwrap().clone();
    let request_id = uuid::Uuid::new_v4().to_string();
    let steer_started_at = now_seconds();
    let (starting_version_id, starting_input_digest) = {
        let conn = state.db.lock().await;
        crate::llm_eval::latest_version_identity(&conn, &binding.ecky_thread_id)
            .map_err(AppError::persistence)?
            .map_or((None, None), |(id, digest)| (Some(id), digest))
    };
    let mut steer_events = vec![crate::llm_eval::EvalEvent {
        sequence: 0,
        step_index: None,
        kind: crate::llm_eval::EvalEventKind::System,
        state: "admitted".into(),
        name: Some("steer.request".into()),
        summary: Some("Exact-turn Codex STEER admitted".into()),
        input: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
            "requestId": request_id,
            "turnId": input.expected_turn_id,
            "prompt": input.prompt_text,
            "attachments": input.attachments.iter().map(|attachment| serde_json::json!({
                "kind": attachment.kind.as_str(),
                "name": attachment.name,
                "explanation": attachment.explanation,
                "payloadSha256": format!("sha256:{:x}", sha2::Sha256::digest(
                    attachment.data_url.as_deref().unwrap_or(&attachment.path).as_bytes()
                )),
            })).collect::<Vec<_>>(),
            "startingVersionId": starting_version_id,
            "startingInputDigest": starting_input_digest,
        }))),
        output: None,
        error: None,
        occurred_at: steer_started_at,
    }];
    let captured_policy = state
        .provider_turn_policy_snapshot(&binding.ecky_thread_id)
        .await;
    let prior_policy = captured_policy
        .map(|snapshot| snapshot.policy)
        .unwrap_or_else(ProviderTurnPolicy::prompt_based);
    let (policy, accepted_route) = if config_snapshot.jev_classifier.enabled {
        let classify = async {
            config_snapshot.jev_classifier.validate()?;
            let (persisted_messages, summary) = {
                let conn = state.db.lock().await;
                let recent = codex_takeover::list_provider_messages(
                    &conn,
                    &binding.ecky_thread_id,
                    codex_takeover::CODEX_PROVIDER_ID,
                    crate::jev_classifier::MAX_RECENT_MESSAGES,
                )?;
                let summary = crate::db::get_thread_summary(&conn, &binding.ecky_thread_id)
                    .map_err(|error| AppError::persistence(error.to_string()))?
                    .unwrap_or_default();
                (recent, summary)
            };
            let live_messages = state
                .codex_app_server
                .live_messages(&binding.codex_thread_id)
                .await;
            let mut messages = Vec::new();
            let mut message_indexes = std::collections::HashMap::new();
            for message in persisted_messages {
                if message.role == "user"
                    || (message.role == "assistant"
                        && message.status == "success"
                        && message.provider_event_kind
                            != Some(crate::contracts::ProviderEventKind::Activity))
                {
                    if let Some(index) = message_indexes.get(&message.id).copied() {
                        messages[index] = message;
                    } else {
                        message_indexes.insert(message.id.clone(), messages.len());
                        messages.push(message);
                    }
                }
            }
            for mut message in live_messages {
                let is_public_user = message.role == "user";
                let is_public_assistant = message.role == "assistant"
                    && message.provider_event_kind
                        == Some(crate::contracts::ProviderEventKind::Assistant);
                if is_public_user || is_public_assistant {
                    if is_public_user {
                        message.content = crate::provider_turn::unwrap_user_message(&message.content)
                            .unwrap_or(message.content);
                    }
                    if let Some(index) = message_indexes.get(&message.id).copied() {
                        messages[index] = message;
                    } else {
                        message_indexes.insert(message.id.clone(), messages.len());
                        messages.push(message);
                    }
                }
            }
            messages.sort_by_key(|message| message.timestamp);
            let recent = messages
                .into_iter()
                .rev()
                .take(crate::jev_classifier::MAX_RECENT_MESSAGES)
                .rev()
                .map(|message| crate::jev_classifier::RecentMessage {
                    role: message.role,
                    content: message.content,
                })
                .collect::<Vec<_>>();
            let request = crate::jev_classifier::ClassifierRequest::bounded(
                &input.prompt_text,
                recent,
                &summary,
                &format!(
                    "exact active Codex turn {}; starting version {}; input digest {}; model remains fixed for active turn",
                    input.expected_turn_id,
                    starting_version_id.as_deref().unwrap_or("none"),
                    starting_input_digest.as_deref().unwrap_or("unknown"),
                ),
                input.attachments.iter().map(|attachment| crate::jev_classifier::AttachmentModality {
                    kind: attachment.kind.as_str().to_owned(),
                    explanation: attachment.explanation.clone(),
                }).collect(),
                Vec::new(),
            );
            let route = match classifier_override {
                Some(classifier) => classifier.classify(request).await?,
                None => crate::jev_classifier::classify_configured_request(
                    &config_snapshot.jev_classifier,
                    request,
                ).await?,
            };
            if route.model.is_some() {
                return Err(AppError::provider("Jev cannot change the model of an active Codex turn."));
            }
            Ok::<_, AppError>(route)
        }.await;
        match classify {
            Ok(route) => {
                let route_policy = ProviderTurnPolicy::routed(route.intent, route.answer_first);
                steer_events.push(crate::llm_eval::EvalEvent {
                    sequence: 1,
                    step_index: None,
                    kind: crate::llm_eval::EvalEventKind::System,
                    state: "accepted".into(),
                    name: Some("jev".into()),
                    summary: Some(format!("Jev accepted {} intent for exact active turn", route.intent.as_str())),
                    input: None,
                    output: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
                        "requestId": request_id,
                        "turnId": input.expected_turn_id,
                        "intent": route.intent.as_str(),
                        "actionProbabilities": route.action_probabilities,
                        "actionConfidence": route.action_confidence,
                        "answerFirst": route.answer_first,
                        "model": active_model,
                        "modelReason": if active_model.is_some() { "actual active turn model retained" } else { "actual active turn model unknown; provider turn was not reconfigured" },
                        "classifierModel": route.classifier_model,
                        "classifierInputTokens": route.classifier_input_tokens,
                        "classifierOutputTokens": route.classifier_output_tokens,
                        "classifierLatencyMs": route.classifier_latency_ms,
                        "policyVersion": route.policy_version,
                    }))),
                    error: None,
                    occurred_at: now_seconds(),
                });
                (route_policy, Some(route))
            }
            Err(error) => {
                let diagnostic = redact_jev_diagnostic(
                    codex_takeover::error_text(&error),
                    &config_snapshot.jev_classifier.api_key,
                );
                steer_events.push(crate::llm_eval::EvalEvent {
                    sequence: 1,
                    step_index: None,
                    kind: crate::llm_eval::EvalEventKind::System,
                    state: "error".into(),
                    name: Some("jev".into()),
                    summary: Some("Jev rejected Codex STEER before delivery".into()),
                    input: None,
                    output: None,
                    error: Some(diagnostic.clone()),
                    occurred_at: now_seconds(),
                });
                state
                    .codex_app_server
                    .append_steer_eval_events(
                        &binding.ecky_thread_id,
                        &binding.codex_thread_id,
                        &input.expected_turn_id,
                        steer_events,
                        Some(&config_snapshot.jev_classifier.api_key),
                    )
                    .await;
                return Err(redact_jev_app_error(
                    error,
                    &config_snapshot.jev_classifier.api_key,
                ));
            }
        }
    } else {
        (prior_policy, None)
    };

    let current_config = state.config.lock().unwrap().clone();
    let current_binding = binding_for(state, &binding.ecky_thread_id).await?;
    let current_runtime = state
        .codex_app_server
        .runtime(&binding.codex_thread_id)
        .await;
    let current_identity = {
        let conn = state.db.lock().await;
        crate::llm_eval::latest_version_identity(&conn, &binding.ecky_thread_id)
            .map_err(AppError::persistence)?
            .map(|(id, digest)| (Some(id), digest))
            .unwrap_or((None, None))
    };
    if current_config.jev_classifier != config_snapshot.jev_classifier
        || current_config.connection_type != config_snapshot.connection_type
        || current_config.provider_models.codex != config_snapshot.provider_models.codex
        || current_binding != *binding
        || current_runtime.active_turn_id.as_deref() != Some(&input.expected_turn_id)
        || current_runtime.phase == "stopping"
        || current_identity != (starting_version_id, starting_input_digest)
    {
        let error = AppError::conflict(
            "Jev route or active Codex turn became stale before STEER delivery.",
        );
        steer_events.push(crate::llm_eval::EvalEvent {
            sequence: steer_events.len() as u64,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::System,
            state: "error".into(),
            name: Some("steer.delivery".into()),
            summary: Some("Stale route prevented STEER delivery".into()),
            input: None,
            output: None,
            error: Some(codex_takeover::error_text(&error)),
            occurred_at: now_seconds(),
        });
        state
            .codex_app_server
            .append_steer_eval_events(
                &binding.ecky_thread_id,
                &binding.codex_thread_id,
                &input.expected_turn_id,
                steer_events,
                config_snapshot
                    .jev_classifier
                    .enabled
                    .then_some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await;
        return Err(redact_jev_app_error(
            error,
            &config_snapshot.jev_classifier.api_key,
        ));
    }
    // Exclude any old-turn answer that arrives while the classifier is in flight.
    let answer_first_baseline = state
        .codex_app_server
        .live_messages(&binding.codex_thread_id)
        .await
        .into_iter()
        .filter(|message| {
            message.provider_event_kind == Some(crate::contracts::ProviderEventKind::Assistant)
                && !message.content.trim().is_empty()
        })
        .map(|message| message.id)
        .collect::<HashSet<_>>();
    let mut policy_lease = if accepted_route.is_some() {
        match state
            .install_provider_turn_policy_with_answer_baseline(
                &binding.ecky_thread_id,
                captured_policy,
                policy,
                answer_first_baseline,
            )
            .await
        {
            Ok(lease) => Some(lease),
            Err(error) => {
                steer_events.push(crate::llm_eval::EvalEvent {
                    sequence: steer_events.len() as u64,
                    step_index: None,
                    kind: crate::llm_eval::EvalEventKind::System,
                    state: "error".into(),
                    name: Some("steer.delivery".into()),
                    summary: Some("Policy ownership changed before STEER delivery".into()),
                    input: None,
                    output: None,
                    error: Some(codex_takeover::error_text(&error)),
                    occurred_at: now_seconds(),
                });
                state
                    .codex_app_server
                    .append_steer_eval_events(
                        &binding.ecky_thread_id,
                        &binding.codex_thread_id,
                        &input.expected_turn_id,
                        steer_events,
                        config_snapshot
                            .jev_classifier
                            .enabled
                            .then_some(config_snapshot.jev_classifier.api_key.as_str()),
                    )
                    .await;
                return Err(error);
            }
        }
    } else {
        None
    };
    if let Err(error) = state
        .codex_app_server
        .steer_turn_with_attachments_and_policy(
            &binding.codex_thread_id,
            &input.expected_turn_id,
            &input.prompt_text,
            &input.attachments,
            policy,
        )
        .await
    {
        if let Some(lease) = policy_lease.take() {
            state
                .rollback_provider_turn_policy(&binding.ecky_thread_id, lease)
                .await;
        }
        steer_events.push(crate::llm_eval::EvalEvent {
            sequence: steer_events.len() as u64,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::System,
            state: "error".into(),
            name: Some("steer.delivery".into()),
            summary: Some("Codex rejected exact-turn STEER".into()),
            input: None,
            output: None,
            error: Some(redact_jev_diagnostic(
                codex_takeover::error_text(&error),
                &config_snapshot.jev_classifier.api_key,
            )),
            occurred_at: now_seconds(),
        });
        state
            .codex_app_server
            .append_steer_eval_events(
                &binding.ecky_thread_id,
                &binding.codex_thread_id,
                &input.expected_turn_id,
                steer_events,
                config_snapshot
                    .jev_classifier
                    .enabled
                    .then_some(config_snapshot.jev_classifier.api_key.as_str()),
            )
            .await;
        return Err(redact_jev_app_error(
            error,
            &config_snapshot.jev_classifier.api_key,
        ));
    }
    let post_delivery_runtime = state
        .codex_app_server
        .runtime(&binding.codex_thread_id)
        .await;
    if post_delivery_runtime.active_turn_id.as_deref() != Some(&input.expected_turn_id)
        || post_delivery_runtime.phase == "stopping"
    {
        if let Some(lease) = policy_lease.take() {
            state
                .rollback_provider_turn_policy(&binding.ecky_thread_id, lease)
                .await;
        }
    }
    let message = {
        let conn = state.db.lock().await;
        let message = codex_takeover::persist_provider_turn_user_input(
            &conn,
            &binding.ecky_thread_id,
            codex_takeover::CODEX_PROVIDER_ID,
            &binding.codex_thread_id,
            &input.expected_turn_id,
            &input.prompt_text,
            &input.attachments,
            now_seconds(),
        )?;
        if let Some(route) = accepted_route.as_ref() {
            crate::services::jev_classifications::save_accepted(
                &conn,
                &binding.ecky_thread_id,
                codex_takeover::CODEX_PROVIDER_ID,
                &request_id,
                Some(&message.id),
                route.intent,
                &route.action_probabilities,
                now_seconds(),
            )?;
            if let Some(app) = state.app_handle.lock().unwrap().clone() {
                let _ = app.emit(
                    "jev-classification-accepted",
                    serde_json::json!({"threadId": binding.ecky_thread_id}),
                );
            }
        }
        message
    };
    steer_events.push(crate::llm_eval::EvalEvent {
        sequence: steer_events.len() as u64,
        step_index: None,
        kind: crate::llm_eval::EvalEventKind::System,
        state: "sent".into(),
        name: Some("steer.delivery".into()),
        summary: Some("STEER delivered to exact active Codex turn".into()),
        input: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
            "requestId": request_id,
            "messageId": message.id,
            "turnId": input.expected_turn_id,
        }))),
        output: None,
        error: None,
        occurred_at: now_seconds(),
    });
    state
        .codex_app_server
        .append_steer_eval_events(
            &binding.ecky_thread_id,
            &binding.codex_thread_id,
            &input.expected_turn_id,
            steer_events,
            config_snapshot
                .jev_classifier
                .enabled
                .then_some(config_snapshot.jev_classifier.api_key.as_str()),
        )
        .await;
    snapshot_for(state, binding.clone(), None).await
}

fn redact_jev_diagnostic(mut diagnostic: String, api_key: &str) -> String {
    if !api_key.is_empty() {
        diagnostic = diagnostic.replace(api_key, "[REDACTED]");
    }
    diagnostic
}

fn redact_jev_app_error(mut error: AppError, api_key: &str) -> AppError {
    error.message = redact_jev_diagnostic(error.message, api_key);
    error.details = error
        .details
        .map(|details| redact_jev_diagnostic(details, api_key));
    error
}

#[tauri::command]
#[specta::specta]
pub async fn stop_codex_takeover(
    input: CodexStopInput,
    state: State<'_, AppState>,
) -> AppResult<CodexTakeoverSnapshot> {
    let binding = binding_for(&state, &input.ecky_thread_id).await?;
    let runtime = state
        .codex_app_server
        .runtime(&binding.codex_thread_id)
        .await;
    if runtime.active_turn_id.as_deref() != Some(&input.turn_id) {
        return Err(AppError::conflict(format!(
            "Codex active turn changed; expected {}, current {}.",
            input.turn_id,
            runtime.active_turn_id.as_deref().unwrap_or("none")
        )));
    }
    state
        .codex_app_server
        .interrupt_turn(&binding.codex_thread_id, &input.turn_id)
        .await?;
    snapshot_for(&state, binding, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_codex_queued_prompt(
    ecky_thread_id: String,
    queue_id: String,
    state: State<'_, AppState>,
) -> AppResult<CodexTakeoverSnapshot> {
    let binding = binding_for(&state, &ecky_thread_id).await?;
    {
        let conn = state.db.lock().await;
        codex_takeover::retry_queue_item(&conn, &ecky_thread_id, &queue_id, now_seconds())?;
    }
    dispatch_queue_for(&state, &binding).await?;
    snapshot_for(&state, binding_for(&state, &ecky_thread_id).await?, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_codex_queued_prompt(
    ecky_thread_id: String,
    queue_id: String,
    state: State<'_, AppState>,
) -> AppResult<CodexTakeoverSnapshot> {
    let binding = binding_for(&state, &ecky_thread_id).await?;
    {
        let conn = state.db.lock().await;
        codex_takeover::remove_queue_item(&conn, &ecky_thread_id, &queue_id)?;
        crate::services::jev_classifications::discard_unbound(
            &conn,
            &ecky_thread_id,
            codex_takeover::CODEX_PROVIDER_ID,
            &queue_id,
        )?;
    }
    snapshot_for(&state, binding, None).await
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::contracts::{Config, EngineKind, GeometryBackend, McpConfig, SourceLanguage};
    use crate::services::codex_takeover::{
        bind_owned_thread, enqueue_prompt, ensure_schema, get_binding, list_provider_messages,
        list_queue,
    };
    use std::os::unix::fs::PermissionsExt;

    static CODEX_TEST_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct WaitingJevClassifier {
        started: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        observed_request: Arc<std::sync::Mutex<Option<crate::jev_classifier::ClassifierRequest>>>,
        release: tokio::sync::Mutex<
            Option<tokio::sync::oneshot::Receiver<crate::jev_classifier::AcceptedRoute>>,
        >,
    }

    impl crate::jev_classifier::TurnClassifier for WaitingJevClassifier {
        fn classify<'a>(
            &'a self,
            request: crate::jev_classifier::ClassifierRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = AppResult<crate::jev_classifier::AcceptedRoute>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                *self.observed_request.lock().unwrap() = Some(request);
                if let Some(started) = self.started.lock().unwrap().take() {
                    let _ = started.send(());
                }
                self.release
                    .lock()
                    .await
                    .take()
                    .expect("release receiver is present")
                    .await
                    .map_err(|_| AppError::provider("test classifier was cancelled"))
            })
        }
    }

    struct FailingJevClassifier;

    impl crate::jev_classifier::TurnClassifier for FailingJevClassifier {
        fn classify<'a>(
            &'a self,
            _request: crate::jev_classifier::ClassifierRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = AppResult<crate::jev_classifier::AcceptedRoute>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                Err(AppError::provider(
                    "test Jev unavailable: client-disconnected; already has an active writer",
                ))
            })
        }
    }

    struct VersionMutatingJevClassifier {
        state: AppState,
        route: crate::jev_classifier::AcceptedRoute,
    }

    impl crate::jev_classifier::TurnClassifier for VersionMutatingJevClassifier {
        fn classify<'a>(
            &'a self,
            _request: crate::jev_classifier::ClassifierRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = AppResult<crate::jev_classifier::AcceptedRoute>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                let conn = self.state.db.lock().await;
                conn.execute(
                    "INSERT INTO messages (id, thread_id, role, content, status, output, timestamp, version_input_digest)
                     VALUES (?1, 'ecky-1', 'assistant', 'Newer artifact during Jev', 'success', '{}', ?2, 'sha256:newer')",
                    rusqlite::params![uuid::Uuid::new_v4().to_string(), now_seconds() + 1_000],
                )
                .map_err(|error| AppError::persistence(error.to_string()))?;
                Ok(self.route.clone())
            })
        }
    }

    struct EnvRestore {
        codex_bin: Option<std::ffi::OsString>,
        requests: Option<std::ffi::OsString>,
        hook_status: Option<std::ffi::OsString>,
        resume_config: Option<std::ffi::OsString>,
        hold_turn_start: Option<std::ffi::OsString>,
        allow_turn_start: Option<std::ffi::OsString>,
        allow_turn_terminal: Option<std::ffi::OsString>,
        turn_start_entered: Option<std::ffi::OsString>,
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.codex_bin {
                Some(value) => std::env::set_var("ECKY_CODEX_BIN", value),
                None => std::env::remove_var("ECKY_CODEX_BIN"),
            }
            match &self.requests {
                Some(value) => std::env::set_var("ECKY_CODEX_TEST_REQUESTS", value),
                None => std::env::remove_var("ECKY_CODEX_TEST_REQUESTS"),
            }
            match &self.hook_status {
                Some(value) => std::env::set_var("ECKY_CODEX_TEST_HOOK_STATUS", value),
                None => std::env::remove_var("ECKY_CODEX_TEST_HOOK_STATUS"),
            }
            match &self.resume_config {
                Some(value) => std::env::set_var("ECKY_CODEX_TEST_RESUME_CONFIG", value),
                None => std::env::remove_var("ECKY_CODEX_TEST_RESUME_CONFIG"),
            }
            for (name, value) in [
                ("ECKY_CODEX_TEST_HOLD_TURN_START", &self.hold_turn_start),
                ("ECKY_CODEX_TEST_ALLOW_TURN_START", &self.allow_turn_start),
                (
                    "ECKY_CODEX_TEST_ALLOW_TURN_TERMINAL",
                    &self.allow_turn_terminal,
                ),
                (
                    "ECKY_CODEX_TEST_TURN_START_ENTERED",
                    &self.turn_start_entered,
                ),
            ] {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    fn test_config() -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::contracts::VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: Some("provider:codex".to_string()),
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: crate::contracts::JevClassifierConfig::default(),
            default_engine_kind: EngineKind::Freecad,
            default_geometry_backend: GeometryBackend::Freecad,
            default_source_language: SourceLanguage::LegacyPython,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_codex_turn_uses_mocked_jev_route_and_persists_eval_with_linked_versions() {
        let _environment = CODEX_TEST_ENV.lock().await;
        let test_id = uuid::Uuid::new_v4().to_string();
        let directory = std::env::temp_dir().join(format!("ecky-codex-eval-dispatch-{test_id}"));
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("fake-codex");
        std::fs::write(
            &executable,
            r##"#!/usr/bin/env python3
import json, os, re, sys, time
turn_number = 0
hook_command = "unused-hook-command"
for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if os.environ.get("ECKY_CODEX_TEST_REQUESTS"):
        with open(os.environ["ECKY_CODEX_TEST_REQUESTS"], "a", encoding="utf-8") as log:
            log.write(method + "\\n")
    if method == "initialize":
        result = {}
    elif method == "config/read":
        result = {"config": {"features": {"shell_tool": True, "unified_exec": True, "multi_agent": True, "apps": True}, "tools": {"view_image": True}, "web_search": "live"}, "origins": {}}
    elif method == "experimentalFeature/list":
        result = {"data": [{"name": name, "enabled": True, "defaultEnabled": True, "stage": "stable"} for name in ["shell_tool", "unified_exec", "multi_agent", "apps", "view_image"]], "nextCursor": None}
    elif method == "thread/turns/list":
        result = {"data": []}
    elif method == "thread/resume":
        if os.environ.get("ECKY_CODEX_TEST_RESUME_CONFIG"):
            with open(os.environ["ECKY_CODEX_TEST_RESUME_CONFIG"], "a", encoding="utf-8") as log:
                log.write(json.dumps(message["params"].get("config", {})) + "\n")
        result = {"thread": {"id": "codex-7", "preview": "Dryer", "cwd": "/tmp/dryer", "createdAt": 1, "updatedAt": 2, "modelProvider": "openai", "status": {"type": "idle"}}, "initialTurnsPage": {"data": []}}
    elif method == "hooks/list":
        with open(os.environ["ECKY_CODEX_TEST_HOOK_STATUS"], encoding="utf-8") as status_file:
            trust_status = status_file.read().strip()
        result = {"data": [{"cwd": "/tmp/dryer", "warnings": [], "errors": [], "hooks": [{"eventName": "preToolUse", "enabled": True, "source": "sessionFlags", "trustStatus": trust_status, "currentHash": "sha256:fixture", "matcher": ".*", "async": False, "timeoutSec": 5, "handlerType": "command", "command": hook_command}]}]}
    elif method == "thread/name/set":
        result = {}
    elif method == "model/list":
        result = {"data": [{"model": "gpt-5.6-sol"}, {"model": "gpt-5.6-luna"}]}
    elif method == "account/read":
        result = {"account": {"type": "apiKey"}}
    elif method == "turn/steer":
        with open(os.environ["ECKY_CODEX_TEST_REQUESTS"] + ".steers.jsonl", "a", encoding="utf-8") as log:
            log.write(json.dumps(message["params"]) + "\n")
        if any("simulate steer transport failure" in block.get("text", "") for block in message["params"].get("input", [])):
            print(json.dumps({"id": message["id"], "error": {"message": "steer transport fixture failed raw"}}), flush=True)
            continue
        result = {}
    elif method == "turn/start":
        turn_number += 1
        params = message["params"]
        with open(os.environ["ECKY_CODEX_TEST_REQUESTS"] + ".turns.jsonl", "a", encoding="utf-8") as log:
            log.write(json.dumps(params) + "\n")
        tid = f"turn-eval-{turn_number}"
        def notify(name, params):
            print(json.dumps({"method": name, "params": params}), flush=True)
        hold = os.environ.get("ECKY_CODEX_TEST_HOLD_TURN_START")
        if hold and os.path.exists(hold):
            with open(os.environ["ECKY_CODEX_TEST_TURN_START_ENTERED"], "w", encoding="utf-8") as marker:
                marker.write("entered")
            while not os.path.exists(os.environ["ECKY_CODEX_TEST_ALLOW_TURN_START"]): time.sleep(0.01)
            notify("turn/started", {"threadId": params["threadId"], "turn": {"id": tid}})
            print(json.dumps({"id": message["id"], "result": {"turn": {"id": tid}}}), flush=True)
            while not os.path.exists(os.environ["ECKY_CODEX_TEST_ALLOW_TURN_TERMINAL"]): time.sleep(0.01)
            notify("item/started", {"threadId": params["threadId"], "turnId": tid, "item": {"id": "tool-item", "type": "mcpToolCall", "serverName": "ecky", "toolName": "render", "arguments": {"quality": "high"}}})
            notify("item/completed", {"threadId": params["threadId"], "turnId": tid, "item": {"id": "tool-item", "type": "mcpToolCall", "serverName": "ecky", "toolName": "render", "result": {"status": "success"}}})
            notify("item/agentMessage/delta", {"threadId": params["threadId"], "turnId": tid, "itemId": "answer", "delta": "done"})
            notify("turn/completed", {"threadId": params["threadId"], "turn": {"id": tid, "status": "completed"}})
            continue
        notify("turn/started", {"threadId": params["threadId"], "turn": {"id": tid}})
        notify("item/started", {"threadId": params["threadId"], "turnId": tid, "item": {"id": "tool-item", "type": "mcpToolCall", "serverName": "ecky", "toolName": "render", "arguments": {"quality": "high"}}})
        notify("item/completed", {"threadId": params["threadId"], "turnId": tid, "item": {"id": "tool-item", "type": "mcpToolCall", "serverName": "ecky", "toolName": "render", "result": {"status": "success"}}})
        notify("item/agentMessage/delta", {"threadId": params["threadId"], "turnId": tid, "itemId": "answer", "delta": "done"})
        notify("turn/completed", {"threadId": params["threadId"], "turn": {"id": tid, "status": "completed"}})
        result = {"turn": {"id": tid}}
    else:
        result = {}
    if "id" in message:
        print(json.dumps({"id": message["id"], "result": result}), flush=True)
"##,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let _env_restore = EnvRestore {
            codex_bin: std::env::var_os("ECKY_CODEX_BIN"),
            requests: std::env::var_os("ECKY_CODEX_TEST_REQUESTS"),
            hook_status: std::env::var_os("ECKY_CODEX_TEST_HOOK_STATUS"),
            resume_config: std::env::var_os("ECKY_CODEX_TEST_RESUME_CONFIG"),
            hold_turn_start: std::env::var_os("ECKY_CODEX_TEST_HOLD_TURN_START"),
            allow_turn_start: std::env::var_os("ECKY_CODEX_TEST_ALLOW_TURN_START"),
            allow_turn_terminal: std::env::var_os("ECKY_CODEX_TEST_ALLOW_TURN_TERMINAL"),
            turn_start_entered: std::env::var_os("ECKY_CODEX_TEST_TURN_START_ENTERED"),
        };
        std::env::set_var("ECKY_CODEX_BIN", &executable);
        let requests_log = directory.join("requests.log");
        let resume_config_log = directory.join("resume-config.jsonl");
        let hook_status = directory.join("hook-trust.txt");
        let hold_turn_start = directory.join("hold-turn-start");
        let allow_turn_start = directory.join("allow-turn-start");
        let allow_turn_terminal = directory.join("allow-turn-terminal");
        let turn_start_entered = directory.join("turn-start-entered");
        std::fs::write(&hook_status, "untrusted").unwrap();
        std::env::set_var("ECKY_CODEX_TEST_REQUESTS", &requests_log);
        std::env::set_var("ECKY_CODEX_TEST_HOOK_STATUS", &hook_status);
        std::env::set_var("ECKY_CODEX_TEST_RESUME_CONFIG", &resume_config_log);
        std::env::set_var("ECKY_CODEX_TEST_HOLD_TURN_START", &hold_turn_start);
        std::env::set_var("ECKY_CODEX_TEST_ALLOW_TURN_START", &allow_turn_start);
        std::env::set_var("ECKY_CODEX_TEST_ALLOW_TURN_TERMINAL", &allow_turn_terminal);
        std::env::set_var("ECKY_CODEX_TEST_TURN_START_ENTERED", &turn_start_entered);

        let db_path = directory.join("ecky.sqlite");
        let conn = crate::db::init_db(&db_path).unwrap();
        conn.execute(
            "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params!["ecky-1", "Dryer", 1i64],
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        bind_owned_thread(&conn, "ecky-1", "codex-7", "Dryer", "/tmp/dryer", 1).unwrap();
        let mut config = test_config();
        config.provider_models.codex = "gpt-5.6-sol".into();
        let state = AppState::new(config, None, conn);
        let hook_descriptor = directory.join("codex-hook-runtime.json");
        crate::services::codex_pre_tool_hook::write_runtime_descriptor_at(
            &hook_descriptor,
            "http://127.0.0.1:39249/codex-pre-tool-hook/test-token",
            "test-token",
        )
        .unwrap();
        state
            .codex_app_server
            .set_hook_descriptor_path_for_test(hook_descriptor);
        state.set_mcp_status(true, None);
        let binding = {
            let conn = state.db.lock().await;
            get_binding(&conn, "ecky-1").unwrap().unwrap()
        };

        {
            let conn = state.db.lock().await;
            enqueue_prompt(&conn, "ecky-1", "untrusted hook must not block Jev", 2).unwrap();
        }
        let classifier = crate::jev_classifier::MockJevClassifier::fixed(
            crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Plan,
                Some("gpt-5.6-luna".into()),
                false,
            ),
        );
        dispatch_queue_for_with_classifier(&state, &binding, &classifier)
            .await
            .unwrap();
        let untrusted_after = {
            let conn = state.db.lock().await;
            list_queue(&conn, "ecky-1").unwrap()
        };
        assert!(untrusted_after.iter().all(|item| item.status != "failed"));
        let classifications = {
            let conn = state.db.lock().await;
            crate::services::jev_classifications::list_for_thread(&conn, "ecky-1").unwrap()
        };
        assert_eq!(classifications.len(), 1);
        assert_eq!(classifications[0].intent, "plan");
        assert!(classifications[0]
            .message_id
            .as_deref()
            .is_some_and(|id| id.contains(":user:0")));
        let request_log = std::fs::read_to_string(&requests_log).unwrap();
        assert!(request_log.contains("turn/start"));
        assert!(!request_log.contains("hooks/list"));
        assert!(!request_log.contains("hooks.PreToolUse"));
        std::fs::write(&hook_status, "trusted").unwrap();
        {
            let conn = state.db.lock().await;
            enqueue_prompt(&conn, "ecky-1", "Original queued prompt", 3).unwrap();
        }
        std::fs::write(&hold_turn_start, "hold").unwrap();
        let dispatch_state = state.clone();
        let dispatch_binding = binding.clone();
        let dispatcher = tokio::spawn(async move {
            dispatch_queue_for_with_classifier(&dispatch_state, &dispatch_binding, &classifier)
                .await
        });
        for _ in 0..200 {
            if turn_start_entered.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            turn_start_entered.exists(),
            "turn/start fixture did not pause"
        );
        let resume_count_before_activation = std::fs::read_to_string(&requests_log)
            .unwrap()
            .lines()
            .filter(|method| *method == "thread/resume")
            .count();
        let endpoint = require_mcp_endpoint(&state).unwrap();
        state
            .codex_app_server
            .resume_thread_with_policy(
                &binding,
                "Dryer",
                &endpoint,
                "handoff",
                false,
                true,
                Some("gpt-5.6-sol"),
                ProviderTurnPolicy::prompt_based(),
            )
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&requests_log)
                .unwrap()
                .lines()
                .filter(|method| *method == "thread/resume")
                .count(),
            resume_count_before_activation,
            "activation restored config before turn/start response"
        );

        std::fs::write(&allow_turn_start, "allow").unwrap();
        for _ in 0..200 {
            if state
                .codex_app_server
                .runtime(&binding.codex_thread_id)
                .await
                .active_turn_id
                .is_some()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(state
            .codex_app_server
            .runtime(&binding.codex_thread_id)
            .await
            .active_turn_id
            .is_some());
        state
            .codex_app_server
            .resume_thread_with_policy(
                &binding,
                "Dryer",
                &endpoint,
                "handoff",
                false,
                true,
                Some("gpt-5.6-sol"),
                ProviderTurnPolicy::prompt_based(),
            )
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&requests_log)
                .unwrap()
                .lines()
                .filter(|method| *method == "thread/resume")
                .count(),
            resume_count_before_activation,
            "active routed turn allowed native-tool restoration"
        );
        std::fs::write(&allow_turn_terminal, "allow").unwrap();
        dispatcher.await.unwrap().unwrap();
        for _ in 0..50 {
            if state.codex_app_server.completed_eval_runs().await.len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let captured = state.codex_app_server.completed_eval_runs().await;
        assert_eq!(captured.len(), 2);
        let initial_eval_count = captured.len();
        assert!(captured.iter().all(|run| run.status == "success"));
        let routed_run = captured
            .iter()
            .find(|run| run.prompt == "Original queued prompt")
            .unwrap();

        // Active routed policy remains stable through activation; each steer gets
        // its own accepted policy on the same provider turn.
        state
            .codex_app_server
            .set_runtime_for_test(
                &binding.codex_thread_id,
                crate::contracts::CodexTakeoverRuntime {
                    phase: "active".into(),
                    active_turn_id: Some("turn-active-routed".into()),
                    error: None,
                },
            )
            .await;
        let answer_policy = ProviderTurnPolicy::routed(ProviderTurnIntent::Answer, false);
        state
            .set_provider_turn_policy(&binding.ecky_thread_id, answer_policy)
            .await;
        let old_answer = CodexDialogueMessage {
            id: "codex:codex-7:answer-before-steer".into(),
            role: "assistant".into(),
            content: "Earlier answer from this still-active turn".into(),
            status: "success".into(),
            timestamp: now_seconds(),
            attachments: Vec::new(),
            provider_event_kind: Some(crate::contracts::ProviderEventKind::Assistant),
        };
        let private_activity = CodexDialogueMessage {
            id: "codex:codex-7:private-reasoning".into(),
            role: "assistant".into(),
            content: "SECRET hidden reasoning".into(),
            status: "success".into(),
            timestamp: now_seconds(),
            attachments: Vec::new(),
            provider_event_kind: Some(crate::contracts::ProviderEventKind::Activity),
        };
        let same_second_later_id = CodexDialogueMessage {
            id: "codex:codex-7:zz-live-first".into(),
            role: "assistant".into(),
            content: "First live public assistant item".into(),
            status: "success".into(),
            timestamp: old_answer.timestamp,
            attachments: Vec::new(),
            provider_event_kind: Some(crate::contracts::ProviderEventKind::Assistant),
        };
        let same_second_earlier_id = CodexDialogueMessage {
            id: "codex:codex-7:aa-live-second".into(),
            role: "assistant".into(),
            content: "Second live public assistant item".into(),
            status: "success".into(),
            timestamp: old_answer.timestamp,
            attachments: Vec::new(),
            provider_event_kind: Some(crate::contracts::ProviderEventKind::Assistant),
        };
        let live_user_message = CodexDialogueMessage {
            id: "codex:codex-7:user-before-steer".into(),
            role: "user".into(),
            content: ProviderTurnPolicy::prompt_based().wrap_user_message_for_turn(
                "Earlier request from the user",
                &uuid::Uuid::new_v4().to_string(),
            ),
            status: "success".into(),
            timestamp: now_seconds().saturating_sub(1),
            attachments: Vec::new(),
            provider_event_kind: None,
        };
        state
            .codex_app_server
            .set_live_messages_for_test(
                &binding.codex_thread_id,
                vec![
                    old_answer.clone(),
                    same_second_later_id,
                    same_second_earlier_id,
                    private_activity,
                    live_user_message,
                ],
            )
            .await;
        let resume_count_before = std::fs::read_to_string(&requests_log)
            .unwrap()
            .lines()
            .filter(|method| *method == "thread/resume")
            .count();
        resume_binding_with_policy(&state, &binding, true, ProviderTurnPolicy::prompt_based())
            .await
            .unwrap();
        let resume_count_after = std::fs::read_to_string(&requests_log)
            .unwrap()
            .lines()
            .filter(|method| *method == "thread/resume")
            .count();
        assert_eq!(resume_count_after, resume_count_before);
        assert_eq!(
            state
                .provider_turn_policy(&binding.ecky_thread_id)
                .await
                .unwrap()
                .intent(),
            ProviderTurnIntent::Answer
        );
        let mut steer_config = state.config.lock().unwrap().clone();
        steer_config.jev_classifier.enabled = true;
        steer_config.jev_classifier.api_key = "test-jev-key".into();
        *state.config.lock().unwrap() = steer_config;
        let (classifier_started_tx, classifier_started_rx) = tokio::sync::oneshot::channel();
        let (classifier_release_tx, classifier_release_rx) = tokio::sync::oneshot::channel();
        let steer_classifier = Arc::new(WaitingJevClassifier {
            started: std::sync::Mutex::new(Some(classifier_started_tx)),
            observed_request: Arc::new(std::sync::Mutex::new(None)),
            release: tokio::sync::Mutex::new(Some(classifier_release_rx)),
        });
        let classifications_before_steer = {
            let conn = state.db.lock().await;
            crate::services::jev_classifications::list_for_thread(&conn, &binding.ecky_thread_id)
                .unwrap()
                .len()
        };
        let steer_state = state.clone();
        let steer_binding = binding.clone();
        let steer_classifier_ref = Arc::clone(&steer_classifier);
        let steer_task = tokio::spawn(async move {
            steer_bound_codex_turn_with_classifier(
                &steer_state,
                &steer_binding,
                &CodexSteerInput {
                    ecky_thread_id: steer_binding.ecky_thread_id.clone(),
                    prompt_text: "steer routed answer".into(),
                    expected_turn_id: "turn-active-routed".into(),
                    attachments: vec![crate::contracts::Attachment {
                        path: String::new(),
                        name: "reference.png".into(),
                        explanation: "shape reference".into(),
                        data_url: Some("data:image/png;base64,aGVsbG8=".into()),
                        kind: crate::contracts::AttachmentKind::Image,
                    }],
                },
                Some(steer_classifier_ref.as_ref()),
            )
            .await
        });
        classifier_started_rx.await.unwrap();
        let classified_request = steer_classifier
            .observed_request
            .lock()
            .unwrap()
            .clone()
            .expect("classifier captured request");
        assert!(classified_request
            .recent_dialogue
            .iter()
            .any(|message| message
                .content
                .contains("Earlier answer from this still-active turn")));
        assert!(classified_request.recent_dialogue.iter().any(|message| {
            message.role == "user" && message.content == "Earlier request from the user"
        }));
        assert!(!classified_request
            .recent_dialogue
            .iter()
            .any(|message| message.content.contains("SECRET")));
        let first_live_index = classified_request
            .recent_dialogue
            .iter()
            .position(|message| message.content == "First live public assistant item")
            .expect("first same-second live item included");
        let second_live_index = classified_request
            .recent_dialogue
            .iter()
            .position(|message| message.content == "Second live public assistant item")
            .expect("second same-second live item included");
        assert!(first_live_index < second_live_index);
        let answer_while_classifying = CodexDialogueMessage {
            id: "codex:codex-7:answer-during-jev".into(),
            role: "assistant".into(),
            content: "Old-turn answer arriving before STEER delivery".into(),
            status: "success".into(),
            timestamp: now_seconds(),
            attachments: Vec::new(),
            provider_event_kind: Some(crate::contracts::ProviderEventKind::Assistant),
        };
        state
            .codex_app_server
            .set_live_messages_for_test(
                &binding.codex_thread_id,
                vec![old_answer.clone(), answer_while_classifying.clone()],
            )
            .await;
        classifier_release_tx
            .send(crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Inspect,
                None,
                true,
            ))
            .unwrap();
        steer_task.await.unwrap().unwrap();
        assert!(std::fs::read_to_string(&requests_log)
            .unwrap()
            .contains("turn/steer"));
        {
            let conn = state.db.lock().await;
            let classifications = crate::services::jev_classifications::list_for_thread(
                &conn,
                &binding.ecky_thread_id,
            )
            .unwrap();
            assert_eq!(
                classifications.len(),
                classifications_before_steer + 1,
                "STEER gets a fresh Jev classification"
            );
            assert!(classifications.iter().any(|classification| {
                classification
                    .message_id
                    .as_deref()
                    .is_some_and(|id| id.starts_with("codex:codex-7:turn-active-routed:user:"))
            }));
        }
        let steer_params: serde_json::Value = serde_json::from_str(
            std::fs::read_to_string(format!("{}.steers.jsonl", requests_log.display()))
                .unwrap()
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(steer_params["expectedTurnId"], "turn-active-routed");
        assert!(
            steer_params["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|block| {
                    block["type"] == "image" && block["url"] == "data:image/png;base64,aGVsbG8="
                }),
            "STEER forwards image as native Codex image input"
        );
        assert!(
            !state
                .provider_answer_first_satisfied(
                    &binding.ecky_thread_id,
                    &std::collections::HashSet::from([
                        old_answer.id.clone(),
                        answer_while_classifying.id.clone(),
                    ]),
                )
                .await
        );
        let new_answer = CodexDialogueMessage {
            id: "codex:codex-7:answer-after-steer".into(),
            role: "assistant".into(),
            content: "Answer to the latest steer".into(),
            status: "success".into(),
            timestamp: now_seconds(),
            attachments: Vec::new(),
            provider_event_kind: Some(crate::contracts::ProviderEventKind::Assistant),
        };
        state
            .codex_app_server
            .set_live_messages_for_test(
                &binding.codex_thread_id,
                vec![
                    old_answer.clone(),
                    answer_while_classifying.clone(),
                    new_answer.clone(),
                ],
            )
            .await;
        assert!(
            state
                .provider_answer_first_satisfied(
                    &binding.ecky_thread_id,
                    &std::collections::HashSet::from([
                        old_answer.id.clone(),
                        answer_while_classifying.id.clone(),
                        new_answer.id.clone()
                    ]),
                )
                .await
        );
        let accepted_policy = state
            .provider_turn_policy(&binding.ecky_thread_id)
            .await
            .unwrap();
        let persisted_before_transport_failure = {
            let conn = state.db.lock().await;
            crate::services::jev_classifications::list_for_thread(&conn, &binding.ecky_thread_id)
                .unwrap()
                .len()
        };
        let rejected_route = crate::jev_classifier::MockJevClassifier::fixed(
            crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Modify,
                None,
                false,
            ),
        );
        let steer_rejection = steer_bound_codex_turn_with_classifier(
            &state,
            &binding,
            &CodexSteerInput {
                ecky_thread_id: binding.ecky_thread_id.clone(),
                prompt_text: "simulate steer transport failure".into(),
                expected_turn_id: "turn-active-routed".into(),
                attachments: Vec::new(),
            },
            Some(&rejected_route),
        )
        .await
        .unwrap_err();
        assert!(steer_rejection
            .message
            .contains("steer transport fixture failed raw"));
        assert_eq!(
            state.provider_turn_policy(&binding.ecky_thread_id).await,
            Some(accepted_policy),
            "provider rejection rolls back only the rejected route"
        );
        {
            let conn = state.db.lock().await;
            assert_eq!(
                crate::services::jev_classifications::list_for_thread(
                    &conn,
                    &binding.ecky_thread_id,
                )
                .unwrap()
                .len(),
                persisted_before_transport_failure,
                "rejected STEER does not retain accepted classification"
            );
            assert!(!list_provider_messages(
                &conn,
                &binding.ecky_thread_id,
                codex_takeover::CODEX_PROVIDER_ID,
                100,
            )
            .unwrap()
            .iter()
            .any(|message| message.content == "simulate steer transport failure"));
        }
        let failed_steer_trace = state
            .codex_app_server
            .completed_eval_runs()
            .await
            .into_iter()
            .find(|run| run.turn_id == "turn-active-routed")
            .unwrap();
        assert!(failed_steer_trace.events.iter().any(|event| {
            event.name.as_deref() == Some("steer.delivery")
                && event.state == "error"
                && event
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("steer transport fixture failed raw"))
        }));
        let steer_count_before_failure = std::fs::read_to_string(&requests_log)
            .unwrap()
            .lines()
            .filter(|method| *method == "turn/steer")
            .count();
        let failure = steer_bound_codex_turn_with_classifier(
            &state,
            &binding,
            &CodexSteerInput {
                ecky_thread_id: binding.ecky_thread_id.clone(),
                prompt_text: "must not reach Codex".into(),
                expected_turn_id: "turn-active-routed".into(),
                attachments: Vec::new(),
            },
            Some(&FailingJevClassifier),
        )
        .await
        .unwrap_err();
        assert!(failure.message.contains("test Jev unavailable"));
        assert_eq!(
            state.provider_turn_policy(&binding.ecky_thread_id).await,
            Some(accepted_policy),
            "classifier failure keeps the last accepted MCP policy"
        );
        assert_eq!(
            std::fs::read_to_string(&requests_log)
                .unwrap()
                .lines()
                .filter(|method| *method == "turn/steer")
                .count(),
            steer_count_before_failure,
            "classifier failure prevents provider delivery"
        );
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let waiting_classifier = Arc::new(WaitingJevClassifier {
            started: std::sync::Mutex::new(Some(started_tx)),
            observed_request: Arc::new(std::sync::Mutex::new(None)),
            release: tokio::sync::Mutex::new(Some(release_rx)),
        });
        let stale_binding = binding.clone();
        let stale_state = state.clone();
        let stale_classifier = Arc::clone(&waiting_classifier);
        state
            .codex_app_server
            .set_runtime_for_test(
                &binding.codex_thread_id,
                crate::contracts::CodexTakeoverRuntime {
                    phase: "active".into(),
                    active_turn_id: Some("turn-active-routed".into()),
                    error: None,
                },
            )
            .await;
        let stale_task = tokio::spawn(async move {
            steer_bound_codex_turn_with_classifier(
                &stale_state,
                &stale_binding,
                &CodexSteerInput {
                    ecky_thread_id: stale_binding.ecky_thread_id.clone(),
                    prompt_text: "stale steer".into(),
                    expected_turn_id: "turn-active-routed".into(),
                    attachments: Vec::new(),
                },
                Some(stale_classifier.as_ref()),
            )
            .await
        });
        started_rx.await.unwrap();
        state
            .codex_app_server
            .set_runtime_for_test(
                &binding.codex_thread_id,
                crate::contracts::CodexTakeoverRuntime {
                    phase: "active".into(),
                    active_turn_id: Some("turn-next".into()),
                    error: None,
                },
            )
            .await;
        release_tx
            .send(crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Modify,
                None,
                false,
            ))
            .unwrap();
        assert!(stale_task.await.unwrap().is_err());
        assert_eq!(
            std::fs::read_to_string(&requests_log)
                .unwrap()
                .lines()
                .filter(|method| *method == "turn/steer")
                .count(),
            steer_count_before_failure,
            "a replaced turn cannot receive late STEER"
        );
        let (config_started_tx, config_started_rx) = tokio::sync::oneshot::channel();
        let (config_release_tx, config_release_rx) = tokio::sync::oneshot::channel();
        let config_classifier = Arc::new(WaitingJevClassifier {
            started: std::sync::Mutex::new(Some(config_started_tx)),
            observed_request: Arc::new(std::sync::Mutex::new(None)),
            release: tokio::sync::Mutex::new(Some(config_release_rx)),
        });
        state
            .codex_app_server
            .set_runtime_for_test(
                &binding.codex_thread_id,
                crate::contracts::CodexTakeoverRuntime {
                    phase: "active".into(),
                    active_turn_id: Some("turn-active-routed".into()),
                    error: None,
                },
            )
            .await;
        let config_before_classification = state.config.lock().unwrap().clone();
        let config_state = state.clone();
        let config_binding = binding.clone();
        let config_classifier_ref = Arc::clone(&config_classifier);
        let config_task = tokio::spawn(async move {
            steer_bound_codex_turn_with_classifier(
                &config_state,
                &config_binding,
                &CodexSteerInput {
                    ecky_thread_id: config_binding.ecky_thread_id.clone(),
                    prompt_text: "stale config steer".into(),
                    expected_turn_id: "turn-active-routed".into(),
                    attachments: Vec::new(),
                },
                Some(config_classifier_ref.as_ref()),
            )
            .await
        });
        config_started_rx.await.unwrap();
        {
            let mut changed_config = state.config.lock().unwrap().clone();
            changed_config.jev_classifier.api_key = "replacement-test-key".into();
            *state.config.lock().unwrap() = changed_config;
        }
        config_release_tx
            .send(crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Modify,
                None,
                false,
            ))
            .unwrap();
        assert!(config_task.await.unwrap().is_err());
        *state.config.lock().unwrap() = config_before_classification;
        assert_eq!(
            std::fs::read_to_string(&requests_log)
                .unwrap()
                .lines()
                .filter(|method| *method == "turn/steer")
                .count(),
            steer_count_before_failure,
            "configuration change while Jev waits prevents STEER delivery"
        );
        assert_eq!(
            state.provider_turn_policy(&binding.ecky_thread_id).await,
            Some(accepted_policy),
            "stale route keeps last accepted MCP policy"
        );
        {
            let conn = state.db.lock().await;
            assert!(list_queue(&conn, &binding.ecky_thread_id)
                .unwrap()
                .is_empty());
        }
        state
            .codex_app_server
            .set_runtime_for_test(&binding.codex_thread_id, Default::default())
            .await;

        // Remove queued work while the production dispatcher awaits Jev. The
        // atomic begin-delivery marker must lose, and preflight Answer policy
        // must remain installed; no second Codex turn may be started.
        let cancelled = {
            let conn = state.db.lock().await;
            enqueue_prompt(&conn, "ecky-1", "cancel during route", 3).unwrap()
        };
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let blocking_classifier = WaitingJevClassifier {
            started: std::sync::Mutex::new(Some(started_tx)),
            observed_request: Arc::new(std::sync::Mutex::new(None)),
            release: tokio::sync::Mutex::new(Some(release_rx)),
        };
        let dispatch_state = state.clone();
        let dispatch_binding = binding.clone();
        let dispatcher = tokio::spawn(async move {
            dispatch_queue_for_with_classifier(
                &dispatch_state,
                &dispatch_binding,
                &blocking_classifier,
            )
            .await
        });
        started_rx.await.unwrap();
        {
            let conn = state.db.lock().await;
            codex_takeover::remove_queue_item(&conn, "ecky-1", &cancelled.id).unwrap();
        }
        release_tx
            .send(crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Modify,
                Some("gpt-5.6-luna".into()),
                true,
            ))
            .unwrap();
        dispatcher.await.unwrap().unwrap();
        let remaining_queue = {
            let conn = state.db.lock().await;
            list_queue(&conn, "ecky-1").unwrap()
        };
        assert!(remaining_queue.is_empty());
        assert_eq!(
            state.codex_app_server.completed_eval_runs().await.len(),
            initial_eval_count + 1,
            "STEER keeps one exact-turn route trace"
        );
        let cancelled_policy = state.provider_turn_policy("ecky-1").await.unwrap();
        assert!(!cancelled_policy.allows_project_writes());

        // Default-off production path must deliver normally without invoking
        // the classifier or attaching Jev route evidence.
        {
            let mut config = state.config.lock().unwrap().clone();
            config.jev_classifier.enabled = false;
            *state.config.lock().unwrap() = config;
        }
        {
            let conn = state.db.lock().await;
            enqueue_prompt(&conn, "ecky-1", "normal prompt while Jev disabled", 4).unwrap();
        }
        dispatch_queue_for(&state, &binding).await.unwrap();
        for _ in 0..50 {
            if state.codex_app_server.completed_eval_runs().await.len() >= initial_eval_count + 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let runs = state.codex_app_server.completed_eval_runs().await;
        assert_eq!(runs.len(), initial_eval_count + 2);
        let resume_configs = std::fs::read_to_string(&resume_config_log).unwrap();
        let resumed_configs = resume_configs
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(
            resumed_configs.iter().all(|config| {
                !config.get("features").is_some() && !config.get("web_search").is_some()
            }),
            "Jev must not modify native feature settings: {resume_configs}"
        );
        let disabled_run = runs
            .iter()
            .find(|run| run.prompt == "normal prompt while Jev disabled")
            .unwrap();
        assert!(disabled_run.route.jev.is_none());
        let turn_params_path =
            std::path::PathBuf::from(format!("{}.turns.jsonl", requests_log.display()));
        let turn_params = std::fs::read_to_string(&turn_params_path)
            .expect("Codex fixture records actual turn/start settings")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(turn_params.len(), 3);
        assert_eq!(turn_params[0]["model"], "gpt-5.6-luna");
        assert_eq!(turn_params[1]["model"], "gpt-5.6-luna");
        assert_eq!(turn_params[2]["model"], "gpt-5.6-sol");
        assert!(turn_params
            .iter()
            .all(|params| params.get("config").is_none()));
        assert!(resumed_configs.iter().all(|config| {
            config.get("features").is_none() && config.get("web_search").is_none()
        }));

        let mut invalid_jev_config = state.config.lock().unwrap().clone();
        invalid_jev_config.jev_classifier.enabled = true;
        invalid_jev_config.jev_classifier.api_key.clear();
        *state.config.lock().unwrap() = invalid_jev_config;
        let setup_queue = {
            let conn = state.db.lock().await;
            enqueue_prompt(&conn, "ecky-1", "Jev setup failure prompt", 5).unwrap()
        };
        assert!(dispatch_queue_for(&state, &binding).await.is_err());
        let setup_failure = state
            .codex_app_server
            .completed_eval_runs()
            .await
            .into_iter()
            .find(|run| run.prompt == "Jev setup failure prompt")
            .expect("Jev setup failure must persist request preflight trace");
        assert!(setup_failure.turn_id.is_empty());
        assert_eq!(setup_failure.status, "failed_pre_dispatch");
        assert!(setup_failure.events.iter().any(|event| {
            event.name.as_deref() == Some("request") && event.state == "admitted"
        }));
        assert!(setup_failure.events.iter().any(|event| {
            event.name.as_deref() == Some("jev.preflight") && event.state == "error"
        }));
        {
            let conn = state.db.lock().await;
            codex_takeover::remove_queue_item(&conn, "ecky-1", &setup_queue.id).unwrap();
        }

        let failed_queue = {
            let conn = state.db.lock().await;
            enqueue_prompt(&conn, "ecky-1", "classifier failure prompt", 6).unwrap()
        };
        assert!(
            dispatch_queue_for_with_classifier(&state, &binding, &FailingJevClassifier,)
                .await
                .is_err()
        );
        let after_classifier_failure = {
            let conn = state.db.lock().await;
            list_queue(&conn, "ecky-1").unwrap()
        };
        assert_eq!(after_classifier_failure[0].id, failed_queue.id);
        assert_eq!(after_classifier_failure[0].status, "failed");
        assert!(after_classifier_failure[0]
            .error
            .as_deref()
            .unwrap()
            .contains(codex_takeover::JEV_CLASSIFIER_ERROR_PREFIX));
        let failed_eval = state
            .codex_app_server
            .completed_eval_runs()
            .await
            .into_iter()
            .find(|run| run.prompt == "classifier failure prompt")
            .expect("classifier failure must persist a pre-dispatch eval trace");
        assert!(failed_eval.turn_id.is_empty());
        assert_eq!(failed_eval.status, "failed_pre_dispatch");
        assert!(failed_eval.events.iter().any(|event| {
            event.name.as_deref() == Some("request") && event.state == "admitted"
        }));
        assert!(failed_eval.events.iter().any(|event| {
            event.name.as_deref() == Some("jev")
                && event.state == "error"
                && event
                    .error
                    .as_deref()
                    .unwrap_or_default()
                    .contains("client-disconnected")
        }));
        {
            let conn = state.db.lock().await;
            assert_eq!(
                codex_takeover::recover_retryable_failures(&conn, now_seconds()).unwrap(),
                0
            );
            assert_eq!(list_queue(&conn, "ecky-1").unwrap()[0].status, "failed");
        }
        assert_eq!(
            state.codex_app_server.completed_eval_runs().await.len(),
            initial_eval_count + 4
        );
        {
            let conn = state.db.lock().await;
            codex_takeover::remove_queue_item(&conn, "ecky-1", &failed_queue.id).unwrap();
        }
        let blocked_root = directory.join("not-a-directory");
        std::fs::write(&blocked_root, "occupied").unwrap();
        persist_completed_codex_eval_runs_to(&state, &blocked_root).await;
        assert_eq!(
            state.codex_app_server.completed_eval_runs().await.len(),
            initial_eval_count + 4
        );
        {
            let conn = state.db.lock().await;
            conn.execute(
                "INSERT INTO messages (id, thread_id, role, content, status, output, timestamp, version_input_digest)
                 VALUES ('version-codex-result', 'ecky-1', 'assistant', 'Draft', 'success', '{}', ?1, 'sha256:changed')",
                rusqlite::params![routed_run.completed_at],
            )
            .unwrap();
        }
        let empty_turn_run_id = uuid::Uuid::new_v4().to_string();
        let mut delivery_failed = routed_run.clone();
        delivery_failed.run_id = empty_turn_run_id.clone();
        delivery_failed.status = "error".into();
        delivery_failed.turn_id.clear();
        delivery_failed.prompt = "turn/start transport failure".into();
        delivery_failed.started_at = routed_run.completed_at;
        delivery_failed.completed_at = routed_run.completed_at;
        delivery_failed.raw_error =
            Some("turn/start failed before provider returned turn id".into());
        delivery_failed.events.clear();
        delivery_failed.versions.clear();
        state
            .codex_app_server
            .push_completed_eval_run_for_test(delivery_failed)
            .await;
        persist_completed_codex_eval_runs_to(&state, &directory).await;
        let run_dir = directory.join("evals/runs");
        let run = std::fs::read_dir(run_dir)
            .unwrap()
            .map(|entry| crate::llm_eval::read_run(&entry.unwrap().path()).unwrap())
            .find(|run| run.prompt == "Original queued prompt")
            .unwrap();
        assert_eq!(run.status, "success");
        assert_eq!(run.prompt, "Original queued prompt");
        assert_eq!(run.route.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(run.route.provider, "codex");
        assert_eq!(
            run.turn_policy.as_ref().unwrap().intent,
            ProviderTurnIntent::Plan
        );
        assert!(run
            .events
            .iter()
            .any(|event| event.name.as_deref() == Some("ecky/render")));
        assert_eq!(run.versions.len(), 1);
        assert_eq!(run.versions[0].version_id, "version-codex-result");
        assert_eq!(
            run.versions[0].input_digest.as_deref(),
            Some("sha256:changed")
        );
        let delivery_failure = std::fs::read_dir(directory.join("evals/runs"))
            .unwrap()
            .map(|entry| crate::llm_eval::read_run(&entry.unwrap().path()).unwrap())
            .find(|run| run.run_id == empty_turn_run_id)
            .unwrap();
        assert!(delivery_failure.turn_id.is_empty());
        assert_eq!(delivery_failure.status, "error");
        assert!(delivery_failure.versions.is_empty());
        let unpersisted = state.codex_app_server.completed_eval_runs().await;
        assert!(
            unpersisted.is_empty(),
            "unpersisted eval runs: {:?}",
            unpersisted
                .iter()
                .map(|run| (&run.prompt, &run.status, &run.turn_id))
                .collect::<Vec<_>>()
        );

        // Artifact changes during Jev classification invalidate the accepted
        // route before turn/start.
        let stale_queue = {
            let conn = state.db.lock().await;
            enqueue_prompt(&conn, "ecky-1", "artifact staleness prompt", now_seconds()).unwrap()
        };
        let mut stale_config = state.config.lock().unwrap().clone();
        stale_config.jev_classifier.enabled = true;
        stale_config.jev_classifier.api_key = "test-key".into();
        *state.config.lock().unwrap() = stale_config;
        let stale_classifier = VersionMutatingJevClassifier {
            state: state.clone(),
            route: crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Modify,
                Some("gpt-5.6-luna".into()),
                true,
            ),
        };
        assert!(
            dispatch_queue_for_with_classifier(&state, &binding, &stale_classifier)
                .await
                .is_err()
        );
        let stale_after = {
            let conn = state.db.lock().await;
            list_queue(&conn, "ecky-1").unwrap()
        };
        assert_eq!(stale_after[0].id, stale_queue.id);
        assert_eq!(stale_after[0].status, "failed");
        assert!(stale_after[0]
            .error
            .as_deref()
            .unwrap()
            .contains("stale before dispatch"));
        let stale_eval = state
            .codex_app_server
            .completed_eval_runs()
            .await
            .into_iter()
            .find(|run| run.prompt == "artifact staleness prompt")
            .expect("accepted stale route must persist a failed pre-dispatch trace");
        assert!(stale_eval.turn_id.is_empty());
        assert_eq!(stale_eval.status, "failed_pre_dispatch");
        assert!(stale_eval.route.jev.is_some());
        assert!(stale_eval
            .events
            .iter()
            .any(|event| { event.name.as_deref() == Some("delivery") && event.state == "failed" }));
        {
            let conn = state.db.lock().await;
            codex_takeover::remove_queue_item(&conn, "ecky-1", &stale_queue.id).unwrap();
        }

        let invalid_model_queue = {
            let conn = state.db.lock().await;
            enqueue_prompt(&conn, "ecky-1", "invalid model route prompt", now_seconds()).unwrap()
        };
        let invalid_model_classifier = crate::jev_classifier::MockJevClassifier::fixed(
            crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Plan,
                Some("not-in-verified-catalog".into()),
                false,
            ),
        );
        assert!(
            dispatch_queue_for_with_classifier(&state, &binding, &invalid_model_classifier,)
                .await
                .is_err()
        );
        let invalid_model_after = {
            let conn = state.db.lock().await;
            list_queue(&conn, "ecky-1").unwrap()
        };
        assert_eq!(invalid_model_after[0].id, invalid_model_queue.id);
        assert_eq!(invalid_model_after[0].status, "failed");
        let invalid_model_eval = state
            .codex_app_server
            .completed_eval_runs()
            .await
            .into_iter()
            .find(|run| run.prompt == "invalid model route prompt")
            .expect("invalid model route must persist an accepted-route failure trace");
        assert!(invalid_model_eval.turn_id.is_empty());
        assert_eq!(invalid_model_eval.status, "failed_pre_dispatch");
        assert!(invalid_model_eval.route.jev.is_some());
        assert!(invalid_model_eval
            .events
            .iter()
            .any(|event| { event.name.as_deref() == Some("jev") && event.state == "accepted" }));
        assert!(invalid_model_eval
            .events
            .iter()
            .any(|event| { event.name.as_deref() == Some("delivery") && event.state == "failed" }));
        drop(state);
        std::env::remove_var("ECKY_CODEX_BIN");
        let _ = std::fs::remove_dir_all(directory);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatch_writer_conflict_rotates_cursor_and_delivers_queued_prompt() {
        let _environment = CODEX_TEST_ENV.lock().await;
        run_writer_conflict_case("resume").await;
        run_writer_conflict_case("turn").await;
    }

    async fn run_writer_conflict_case(conflict_phase: &str) {
        let test_id = uuid::Uuid::new_v4().to_string();
        let directory = std::env::temp_dir().join(format!("ecky-codex-dispatch-{test_id}"));
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("fake-codex");
        let requests = directory.join("requests");
        let fake_codex = r#"#!/usr/bin/env python3
import json, os, sys
requests = open(os.environ["ECKY_CODEX_TEST_REQUESTS"], "a", encoding="utf-8")
for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method:
        requests.write(method + " " + str(message.get("params", {}).get("threadId", "")) + "\n")
        requests.flush()
    if method == "initialize":
        print(json.dumps({"id": message["id"], "result": {}}), flush=True)
    elif method == "thread/turns/list":
        print(json.dumps({"id": message["id"], "result": {"data": [{"id": "turn-external", "startedAt": 1, "completedAt": 2, "status": "completed", "items": [{"id": "user-1", "type": "userMessage", "content": [{"type": "text", "text": "Desktop prompt"}]}, {"id": "assistant-1", "type": "agentMessage", "text": "Desktop reply"}]}]}}), flush=True)
    elif method == "thread/resume":
        thread_id = message.get("params", {}).get("threadId")
        if thread_id == "codex-7" and "CONFLICT_PHASE" == "resume":
            print(json.dumps({"id": message["id"], "error": {"message": "thread codex-7 already has an active writer", "data": {"owner": "foreign-client"}}}), flush=True)
        else:
            print(json.dumps({"id": message["id"], "result": {"thread": {"id": thread_id, "preview": "Dryer", "cwd": "/tmp/dryer", "createdAt": 1, "updatedAt": 2, "modelProvider": "openai", "status": {"type": "idle"}}, "initialTurnsPage": {"data": []}}}), flush=True)
    elif method == "thread/start":
        print(json.dumps({"id": message["id"], "result": {"thread": {"id": "codex-8", "preview": "Dryer", "cwd": "/tmp/dryer", "createdAt": 3, "updatedAt": 3, "modelProvider": "openai", "status": {"type": "idle"}}}}), flush=True)
    elif method == "thread/name/set":
        print(json.dumps({"id": message["id"], "result": {}}), flush=True)
    elif method == "turn/start":
        if message.get("params", {}).get("threadId") == "codex-7" and "CONFLICT_PHASE" == "turn":
            print(json.dumps({"id": message["id"], "error": {"message": "thread codex-7 already has an active writer", "data": {"owner": "foreign-client"}}}), flush=True)
        else:
            print(json.dumps({"id": message["id"], "result": {"turn": {"id": "turn-2"}}}), flush=True)
"#;
        std::fs::write(
            &executable,
            fake_codex.replace("CONFLICT_PHASE", conflict_phase),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let _env_restore = EnvRestore {
            codex_bin: std::env::var_os("ECKY_CODEX_BIN"),
            requests: std::env::var_os("ECKY_CODEX_TEST_REQUESTS"),
            hook_status: std::env::var_os("ECKY_CODEX_TEST_HOOK_STATUS"),
            resume_config: std::env::var_os("ECKY_CODEX_TEST_RESUME_CONFIG"),
            hold_turn_start: std::env::var_os("ECKY_CODEX_TEST_HOLD_TURN_START"),
            allow_turn_start: std::env::var_os("ECKY_CODEX_TEST_ALLOW_TURN_START"),
            allow_turn_terminal: std::env::var_os("ECKY_CODEX_TEST_ALLOW_TURN_TERMINAL"),
            turn_start_entered: std::env::var_os("ECKY_CODEX_TEST_TURN_START_ENTERED"),
        };
        std::env::set_var("ECKY_CODEX_BIN", &executable);
        std::env::set_var("ECKY_CODEX_TEST_REQUESTS", &requests);

        let db_path = directory.join("ecky.sqlite");
        let conn = crate::db::init_db(&db_path).unwrap();
        conn.execute(
            "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params!["ecky-1", "Dryer", 1i64],
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        bind_owned_thread(&conn, "ecky-1", "codex-7", "Dryer", "/tmp/dryer", 1).unwrap();
        let queued = enqueue_prompt(&conn, "ecky-1", "Continue", 2).unwrap();
        let state = AppState::new(test_config(), None, conn);
        state.set_mcp_status(true, None);

        let binding = {
            let conn = state.db.lock().await;
            get_binding(&conn, "ecky-1").unwrap().unwrap()
        };
        dispatch_queue_for(&state, &binding).await.unwrap();
        let external = {
            let conn = state.db.lock().await;
            list_provider_messages(&conn, "ecky-1", codex_takeover::CODEX_PROVIDER_ID, 30).unwrap()
        };
        assert!(external
            .iter()
            .any(|message| message.content == "Desktop reply"));
        assert_eq!(
            {
                let conn = state.db.lock().await;
                get_binding(&conn, "ecky-1")
                    .unwrap()
                    .unwrap()
                    .codex_thread_id
            },
            "codex-8"
        );

        {
            let conn = state.db.lock().await;
            let queue = list_queue(&conn, "ecky-1").unwrap();
            assert!(queue.iter().all(|item| item.id != queued.id));
            assert!(codex_takeover::writer_conflict_rotated_for_queue(&conn, &queued.id).unwrap());
            let lineage = codex_takeover::list_binding_lineage(
                &conn,
                "ecky-1",
                codex_takeover::CODEX_PROVIDER_ID,
            )
            .unwrap();
            assert_eq!(lineage.len(), 2);
            assert_eq!(lineage[0].external_thread_id, "codex-7");
            assert_eq!(lineage[1].external_thread_id, "codex-8");
        }
        let log = std::fs::read_to_string(&requests).unwrap();
        assert_eq!(log.matches("thread/start").count(), 1, "{log}");
        assert!(
            log.contains("thread/resume codex-7"),
            "same binding resume: {log}"
        );
        assert!(
            log.contains("turn/start codex-8"),
            "replacement continuation: {log}"
        );

        let _ = std::fs::remove_dir_all(directory);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn start_codex_voice(
    input: crate::contracts::CodexVoiceStartInput,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::contracts::CodexVoiceConnection> {
    require_codex_provider_mode(&state)?;
    if input.session_id.trim().is_empty() || input.sdp.trim().is_empty() {
        return Err(AppError::validation(
            "Codex voice requires a session identity and an audio SDP offer.",
        ));
    }
    if state.config.lock().unwrap().jev_classifier.enabled {
        return Err(AppError::validation("Codex native realtime delegation does not expose pre-turn Jev routing. Disable experimental Jev routing before starting voice."));
    }
    let binding = ensure_binding(&app, &state, &input.ecky_thread_id).await?;
    let runtime = state
        .codex_app_server
        .runtime(&binding.codex_thread_id)
        .await;
    if runtime.active_turn_id.is_some() {
        return Err(AppError::validation(
            "Wait for the current Codex turn before starting voice.",
        ));
    }
    let policy = ProviderTurnPolicy::prompt_based();
    state
        .set_provider_turn_policy(&binding.ecky_thread_id, policy)
        .await;
    resume_binding_with_policy(&state, &binding, true, policy).await?;
    let sdp = state
        .codex_app_server
        .start_realtime(&binding.codex_thread_id, &input.session_id, &input.sdp)
        .await?;
    if state
        .codex_app_server
        .realtime_session_canceled(&input.session_id)
        .await
    {
        let _ = state
            .codex_app_server
            .stop_realtime(&binding.codex_thread_id, &input.session_id)
            .await;
        return Err(AppError::provider("Codex voice startup was canceled."));
    }
    // A mode switch during network negotiation must not leave a live microphone session.
    if let Err(error) = require_codex_provider_mode(&state) {
        let _ = state
            .codex_app_server
            .stop_realtime(&binding.codex_thread_id, &input.session_id)
            .await;
        return Err(error);
    }
    Ok(crate::contracts::CodexVoiceConnection {
        thread_id: binding.codex_thread_id,
        session_id: input.session_id,
        sdp,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn stop_codex_voice(
    input: crate::contracts::CodexVoiceStopInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    // Stop remains available after a mode switch; identity still binds exact owned session.
    state
        .codex_app_server
        .cancel_realtime_session(&input.session_id)
        .await;
    let binding = {
        let conn = state.db.lock().await;
        codex_takeover::get_binding(&conn, &input.ecky_thread_id)?
    };
    match binding {
        Some(binding) => {
            state
                .codex_app_server
                .stop_realtime(&binding.codex_thread_id, &input.session_id)
                .await
        }
        None => Ok(()), // Cancellation may arrive before lazy binding creation.
    }
}

pub(crate) async fn persist_realtime_message(
    app: &tauri::AppHandle,
    thread_id: &str,
    message: CodexDialogueMessage,
) -> AppResult<()> {
    let state = app.state::<AppState>();
    let binding = {
        let conn = state.db.lock().await;
        codex_takeover::get_binding_by_codex_thread_id(&conn, thread_id)?
    }
    .ok_or_else(|| AppError::not_found("Codex voice transcript has no owned Ecky binding."))?;
    persist_codex_messages(&state, &binding, &[message]).await?;
    app.emit(
        "codex-provider-updated",
        serde_json::json!({"threadId": thread_id, "method": "history/persisted"}),
    )
    .map_err(|error| AppError::internal(error.to_string()))?;
    Ok(())
}
