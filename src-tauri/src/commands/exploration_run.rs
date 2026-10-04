//! Rust-owned generation runner.
//!
//! The UI submits intent. This actor owns admission, retries, append-before-
//! render, verification, and terminal persistence. Lifecycle facts never come
//! in from the caller.

use crate::commands::generation::{
    finalize_generation_core, generate_design_core, init_generation_core,
    persist_generation_draft_in_db, persist_structural_verification_core, ClassifyIntentCoreInput,
    FinalizeGenerationCoreInput, GenerateDesignCoreInput, InitGenerationCoreInput,
};
use crate::contracts::exploration_cycle::{
    CycleNextAction, CycleNextInput, PlanAction, PlanProposal, StartCycleInput,
};
use crate::contracts::exploration_run::{
    ExplorationRunKind, ExplorationRunOutput, ExplorationRunPhase, ExplorationRunProgress,
    ExplorationRunProjection, StartExplorationRunInput, StopExplorationRunInput,
};
use crate::contracts::{AppError, AppResult, StructuralVerificationResult};
use crate::db;
use crate::models::{AppState, PathResolver};
use crate::services::render_snapshot::{build_render_snapshot, RenderSnapshotInput};
use crate::{build_queue::BuildKind, exploration_run_registry::AdmissionError};
use sha2::Digest;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
#[specta::specta]
pub async fn start_exploration_run(
    input: StartExplorationRunInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<ExplorationRunProjection> {
    let run = start_exploration_run_core(input, state.inner(), &app).await?;
    let snapshot_id = match (&run.design, &run.artifact_bundle, &run.model_manifest) {
        (Some(design), Some(artifact_bundle), Some(model_manifest)) => Some(
            build_render_snapshot(RenderSnapshotInput {
                design,
                effective_params: &design.initial_params,
                artifact_bundle,
                model_manifest,
            })?
            .snapshot_id,
        ),
        _ => None,
    };
    let message = {
        let conn = state.db.lock().await;
        db::get_thread_message_version(&conn, &run.thread_id, &run.message_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    };
    Ok(ExplorationRunProjection {
        run,
        message,
        snapshot_id,
    })
}

pub async fn start_exploration_run_core(
    input: StartExplorationRunInput,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ExplorationRunOutput> {
    validate_input(&input)?;
    let kind = match input.kind {
        ExplorationRunKind::Interactive => BuildKind::Interactive,
        ExplorationRunKind::Controller => BuildKind::Controller,
    };
    let key = input.thread_id.clone();
    let admitted = state
        .exploration_run_registry
        .admit(
            key.clone(),
            input.request_id.clone(),
            kind,
            input.base_version_id.clone().unwrap_or_default(),
            input.prompt.clone(),
        )
        .await;
    if let Err(error) = admitted {
        return Err(match error {
            AdmissionError::Superseded => {
                AppError::validation("Exploration run superseded by newer input.")
            }
            AdmissionError::Cancelled => AppError::validation("Exploration run cancelled."),
        });
    }

    let result = run_admitted(input.clone(), state, app).await;
    if let Err(error) = &result {
        terminate_active_cycle_on_error(&input.thread_id, error, state).await;
    }
    state
        .exploration_run_registry
        .finish(key, &input.request_id)
        .await;
    result
}

/// Cancel backend-owned work for one request and stop its durable cycle, if any.
///
/// Callers provide cancellation intent only. The registry and cycle reducer own
/// the actual lifecycle transitions.
#[tauri::command]
#[specta::specta]
pub async fn stop_exploration_run(
    input: StopExplorationRunInput,
    state: State<'_, AppState>,
) -> AppResult<Option<crate::contracts::exploration_cycle::CyclePacket>> {
    stop_exploration_run_core(input, state.inner()).await
}

pub async fn stop_exploration_run_core(
    input: StopExplorationRunInput,
    state: &AppState,
) -> AppResult<Option<crate::contracts::exploration_cycle::CyclePacket>> {
    if input.request_id.trim().is_empty() {
        return Err(AppError::validation("requestId cannot be empty."));
    }
    if input.thread_id.trim().is_empty() {
        return Err(AppError::validation("threadId cannot be empty."));
    }

    let running = state
        .exploration_run_registry
        .is_running(input.thread_id.clone(), &input.request_id)
        .await;
    state
        .exploration_run_registry
        .cancel(input.thread_id.clone(), input.request_id)
        .await;

    if !running {
        return Ok(None);
    }

    let active = crate::commands::exploration_cycle::get_active_exploration_cycle_core(
        input.thread_id,
        state,
    )
    .await?;
    match active {
        Some(packet) => crate::commands::exploration_cycle::stop_exploration_cycle_core(
            packet.state.cycle_id,
            state,
        )
        .await
        .map(Some),
        None => Ok(None),
    }
}

async fn run_admitted(
    input: StartExplorationRunInput,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ExplorationRunOutput> {
    // A version-bound run is the exploration controller path. It must let the
    // authoring provider choose the typed PLAN action; caller-side intent
    // routing must not manufacture a BUILD or bypass ASK/STOP.
    let awaiting_input = crate::commands::exploration_cycle::get_active_exploration_cycle_core(
        input.thread_id.clone(),
        state,
    )
    .await?
    .is_some_and(|packet| {
        packet.state.phase == crate::contracts::exploration_cycle::CyclePhase::AwaitingInput
    });
    let version_bound_run = input.base_version_id.is_some() || awaiting_input;
    let jev_enabled = state.config.lock().unwrap().jev_classifier.enabled;
    if version_bound_run && !jev_enabled {
        return run_cycle_admitted(input, state, app).await;
    }
    let max_attempts = state
        .config
        .lock()
        .map_err(|_| AppError::persistence("Config lock poisoned."))?
        .max_generation_attempts
        .max(1);
    let started_at = chrono::Utc::now().timestamp();
    let starting = {
        let db = state.db.lock().await;
        crate::llm_eval::latest_version_identity(&db, &input.thread_id)
            .map_err(AppError::persistence)?
    };
    let engine = {
        let config = state
            .config
            .lock()
            .map_err(|_| AppError::persistence("Config lock poisoned."))?;
        config
            .engines
            .iter()
            .find(|candidate| candidate.id == config.selected_engine_id)
            .cloned()
    };
    let eval_run_id = uuid::Uuid::new_v4().to_string();
    let classification = crate::commands::generation::classify_intent_with_route_core(
        ClassifyIntentCoreInput {
            prompt: input.prompt.clone(),
            thread_id: Some(input.thread_id.clone()),
            context: None,
            image_data: input.image_data.clone(),
            attachments: Some(input.attachments.clone()),
        },
        state,
    )
    .await;
    let (intent, route) = match classification {
        Ok(classification) => classification,
        Err(error) => {
            if jev_enabled {
                persist_api_classification_failure(
                    &input,
                    app,
                    &eval_run_id,
                    engine.as_ref(),
                    starting,
                    started_at,
                    &error,
                )
                .await;
            }
            return Err(error);
        }
    };
    if let Some(route) = route.as_ref() {
        let conn = state.db.lock().await;
        crate::services::jev_classifications::save_accepted(
            &conn,
            &input.thread_id,
            "api",
            &input.request_id,
            None,
            route.intent,
            &route.action_probabilities,
            chrono::Utc::now().timestamp(),
        )?;
        if let Some(app) = state.app_handle.lock().unwrap().clone() {
            use tauri::Emitter;
            let _ = app.emit(
                "jev-classification-accepted",
                serde_json::json!({"threadId": input.thread_id}),
            );
        }
    }
    let result = run_admitted_routed(
        input.clone(),
        state,
        app,
        max_attempts,
        version_bound_run,
        intent,
    )
    .await;
    if let Some(route) = route {
        persist_api_route_trace(
            &input,
            state,
            engine.as_ref(),
            &eval_run_id,
            starting,
            started_at,
            &route,
            &result,
            app,
        )
        .await;
    }
    result
}

async fn persist_api_classification_failure(
    input: &StartExplorationRunInput,
    app: &dyn PathResolver,
    run_id: &str,
    engine: Option<&crate::contracts::Engine>,
    starting: Option<(String, Option<String>)>,
    started_at: i64,
    error: &AppError,
) {
    let completed_at = chrono::Utc::now().timestamp();
    let (starting_version_id, starting_input_digest) =
        starting.map_or((None, None), |(id, digest)| (Some(id), digest));
    let seed = crate::llm_eval::EvalRunSeed {
        run_id: run_id.to_string(),
        thread_id: input.thread_id.clone(),
        external_thread_id: String::new(),
        provider: engine
            .map(|value| value.provider.clone())
            .unwrap_or_else(|| "api".into()),
        model: engine.map(|value| value.model.clone()),
        effort: None,
        prompt_version: "api-generation-v1".into(),
        prompt: input.prompt.clone(),
        starting_version_id,
        starting_input_digest,
        expected_red_rounds: 0,
        turn_intent: crate::provider_turn::ProviderTurnIntent::Clarify,
        answer_first_required: false,
        jev_route: None,
    };
    let raw_error = error
        .details
        .clone()
        .unwrap_or_else(|| error.message.clone());
    let events = vec![
        crate::llm_eval::EvalEvent {
            sequence: 0,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::System,
            state: "admitted".into(),
            name: Some("request".into()),
            summary: Some("API generation request admitted".into()),
            input: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
                "requestId": input.request_id,
                "promptSha256": format!("sha256:{:x}", sha2::Sha256::digest(input.prompt.as_bytes())),
                "promptChars": input.prompt.chars().count(),
            }))),
            output: None,
            error: None,
            occurred_at: started_at,
        },
        crate::llm_eval::EvalEvent {
            sequence: 1,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::System,
            state: "error".into(),
            name: Some("jev.classification".into()),
            summary: Some("Jev classification failed before provider dispatch".into()),
            input: None,
            output: None,
            error: Some(raw_error.clone()),
            occurred_at: completed_at,
        },
    ];
    let run = crate::llm_eval::build_api_eval_run(
        seed,
        "",
        started_at,
        completed_at,
        "failed_pre_dispatch",
        None,
        Some(raw_error),
        events,
        Vec::new(),
        None,
    );
    if let Err(persist_error) = crate::llm_eval::persist_run(&app.app_data_dir(), &run) {
        eprintln!("Unable to persist API Jev classifier failure trace: {persist_error}");
    }
}

async fn persist_api_route_trace(
    input: &StartExplorationRunInput,
    state: &AppState,
    engine: Option<&crate::contracts::Engine>,
    run_id: &str,
    starting: Option<(String, Option<String>)>,
    started_at: i64,
    route: &crate::jev_classifier::AcceptedRoute,
    result: &AppResult<ExplorationRunOutput>,
    app: &dyn PathResolver,
) {
    let completed_at = chrono::Utc::now().timestamp();
    let (versions, outcome) = {
        let db = state.db.lock().await;
        let starting_id = starting.as_ref().map(|(id, _)| id.as_str());
        let versions = crate::llm_eval::version_outcomes_for_window(
            &db,
            &input.thread_id,
            starting_id,
            started_at,
            completed_at,
        )
        .unwrap_or_default();
        let response = result
            .as_ref()
            .ok()
            .and_then(|output| output.response_text.clone());
        (versions, response)
    };
    let raw_error = result
        .as_ref()
        .err()
        .map(|error| {
            error
                .details
                .clone()
                .unwrap_or_else(|| error.message.clone())
        })
        .or_else(|| {
            result
                .as_ref()
                .ok()
                .and_then(|output| output.raw_error.clone())
        });
    let usage = result
        .as_ref()
        .ok()
        .and_then(|output| output.usage.as_ref())
        .map(|usage| crate::llm_eval::EvalUsage {
            input_tokens: Some(usage.input_tokens),
            output_tokens: Some(usage.output_tokens),
            estimated_cost_usd: usage.estimated_cost_usd,
        });
    let mut events = vec![crate::llm_eval::EvalEvent {
        sequence: 0,
        step_index: None,
        kind: crate::llm_eval::EvalEventKind::System,
        state: "accepted".into(),
        name: Some("jev.route".into()),
        summary: Some(format!(
            "Jev selected {} for API execution",
            route.intent.as_str()
        )),
        input: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
            "requestId": input.request_id,
            "promptSha256": format!("sha256:{:x}", sha2::Sha256::digest(input.prompt.as_bytes())),
            "promptChars": input.prompt.chars().count(),
            "contextTruncated": route.context_truncated,
            "currentPromptTruncated": route.current_prompt_truncated,
            "classifierModel": route.classifier_model,
            "classifierInputTokens": route.classifier_input_tokens,
            "classifierOutputTokens": route.classifier_output_tokens,
        }))),
        output: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
            "intent": route.intent.as_str(),
            "intentConfidence": route.action_confidence,
            "intentProbabilities": route.action_probabilities,
            "answerRequested": route.answer_requested,
            "answerFirst": route.answer_first,
            "model": route.model,
            "modelReason": route.model_reason,
            "classifierLatencyMs": route.classifier_latency_ms,
        }))),
        error: None,
        occurred_at: started_at,
    }];
    events.push(crate::llm_eval::EvalEvent {
        sequence: 1,
        step_index: None,
        kind: crate::llm_eval::EvalEventKind::System,
        state: "attempted".into(),
        name: Some("delivery".into()),
        summary: Some(
            "Accepted route handed to API adapter; provider dispatch not independently observed"
                .into(),
        ),
        input: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
            "provider": engine.map(|engine| engine.provider.as_str()),
            "model": engine.map(|engine| engine.model.as_str()),
            "routeModel": route.model,
        }))),
        output: None,
        error: None,
        occurred_at: started_at,
    });
    if let Some(response) = outcome.as_ref() {
        events.push(crate::llm_eval::EvalEvent {
            sequence: events.len() as u64,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::Assistant,
            state: "completed".into(),
            name: Some("api.provider_response".into()),
            summary: Some("API provider returned response".into()),
            input: None,
            output: Some(crate::llm_eval::EvalPayload::new(
                serde_json::json!({"response": response, "providerTurnId": null}),
            )),
            error: None,
            occurred_at: completed_at,
        });
    }
    if let Ok(output) = result.as_ref() {
        events.push(crate::llm_eval::EvalEvent {
            sequence: events.len() as u64,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::System,
            state: "terminal".into(),
            name: Some("api.outcome".into()),
            summary: Some(format!("API exploration reached {:?}", output.phase)),
            input: None,
            output: Some(crate::llm_eval::EvalPayload::new(serde_json::json!({
                "phase": format!("{:?}", output.phase),
                "messageId": output.message_id,
                "cycleId": output.cycle_id,
                "publicationAllowed": output.publication_allowed,
                "responsePresent": output.response_text.is_some(),
                "verificationPassed": output.structural_verification.as_ref().map(|result| result.passed),
                "versionsCreated": versions.iter().map(|version| version.version_id.as_str()).collect::<Vec<_>>(),
            }))),
            error: None,
            occurred_at: completed_at,
        });
    }
    if let Some(error) = raw_error.as_ref() {
        events.push(crate::llm_eval::EvalEvent {
            sequence: events.len() as u64,
            step_index: None,
            kind: crate::llm_eval::EvalEventKind::System,
            state: "error".into(),
            name: Some("api.execution_error".into()),
            summary: Some(
                "API route ended with an error; provider dispatch not independently observed"
                    .into(),
            ),
            input: None,
            output: None,
            error: Some(error.clone()),
            occurred_at: completed_at,
        });
    }
    let (starting_version_id, starting_input_digest) =
        starting.map_or((None, None), |(id, digest)| (Some(id), digest));
    let eval_route = crate::llm_eval::EvalJevRoute {
        policy_version: route.policy_version.to_string(),
        intent_confidence_threshold: crate::jev_classifier::ACTION_CONFIDENCE_MIN,
        intent_margin_threshold: crate::jev_classifier::ACTION_MARGIN_MIN,
        model_confidence_threshold: crate::jev_classifier::ACTION_CONFIDENCE_MIN,
        model_margin_threshold: crate::jev_classifier::ACTION_MARGIN_MIN,
        intent_confidence: route.action_confidence,
        intent_probabilities: route.action_probabilities.clone(),
        answer_requested: route.answer_requested,
        answer_first: route.answer_first,
        model_ceiling: engine.map(|engine| engine.model.clone()),
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
    };
    let seed = crate::llm_eval::EvalRunSeed {
        run_id: run_id.to_string(),
        thread_id: input.thread_id.clone(),
        external_thread_id: String::new(),
        provider: engine
            .map(|engine| engine.provider.clone())
            .unwrap_or_else(|| "api".into()),
        model: engine.map(|engine| engine.model.clone()),
        effort: None,
        prompt_version: "api-generation-v1".into(),
        prompt: input.prompt.clone(),
        starting_version_id,
        starting_input_digest,
        expected_red_rounds: 0,
        turn_intent: route.intent,
        answer_first_required: route.answer_first,
        jev_route: Some(eval_route),
    };
    let status = if raw_error.is_some() {
        "error"
    } else if result.as_ref().ok().is_some_and(|output| {
        matches!(
            output.phase,
            ExplorationRunPhase::Stopped
                | ExplorationRunPhase::Interrupted
                | ExplorationRunPhase::Superseded
        )
    }) {
        "interrupted"
    } else {
        "success"
    };
    let run = crate::llm_eval::build_api_eval_run(
        seed,
        String::new(),
        started_at,
        completed_at,
        status,
        outcome,
        raw_error,
        events,
        versions,
        usage,
    );
    if let Err(error) = crate::llm_eval::persist_run(&app.app_data_dir(), &run) {
        eprintln!("Unable to persist API Jev eval trace: {error}");
    }
}

async fn run_admitted_routed(
    input: StartExplorationRunInput,
    state: &AppState,
    app: &dyn PathResolver,
    max_attempts: u32,
    version_bound_run: bool,
    intent: crate::contracts::IntentDecision,
) -> AppResult<ExplorationRunOutput> {
    if intent.intent_mode.eq_ignore_ascii_case("question") {
        return run_question_only(input, state, app, max_attempts).await;
    }
    if version_bound_run {
        return run_cycle_admitted(input, state, app).await;
    }
    let message_id = {
        let configured_root = state.config.lock().unwrap().projects_root.clone();
        let _ = configured_root;
        init_generation_core(
            InitGenerationCoreInput {
                thread_id: input.thread_id.clone(),
                prompt: input.prompt.clone(),
                attachments: Some(input.attachments.clone()),
                image_data: input.image_data.clone(),
                classification_request_id: Some(input.request_id.clone()),
            },
            state,
            app,
        )
        .await?
    };
    emit_progress(
        state,
        ExplorationRunProgress {
            request_id: input.request_id.clone(),
            thread_id: input.thread_id.clone(),
            cycle_id: None,
            phase: ExplorationRunPhase::Planning,
            attempt: 0,
            max_attempts,
            running_builds: 1,
            pending_builds: 0,
            current_version_id: input.base_version_id.clone(),
            summary: "Planning one bounded authoring step.".into(),
            raw_error: None,
        },
    )
    .await;

    let mut current_design = input.working_design.clone();
    let mut last_error: Option<String> = None;
    let mut last_verification: Option<StructuralVerificationResult> = None;
    let mut last_bundle = None;
    let mut last_manifest = None;
    let mut last_usage = None;
    let mut current_version_id = input.base_version_id.clone();
    let mut latest_version_id: Option<String> = None;
    let cycle_id: Option<String> = None;

    for attempt in 1..=max_attempts {
        if state
            .exploration_run_registry
            .is_cancelled(&input.request_id)
            .await
        {
            return finish_stopped(
                &input,
                message_id,
                "Exploration run cancelled.".into(),
                state,
                app,
                max_attempts,
                attempt,
                current_version_id,
                cycle_id.clone(),
                latest_version_id.clone(),
                current_design.clone(),
                last_bundle.clone(),
                last_manifest.clone(),
                last_usage.clone(),
                last_verification.clone(),
            )
            .await;
        }
        emit_progress(
            state,
            ExplorationRunProgress {
                request_id: input.request_id.clone(),
                thread_id: input.thread_id.clone(),
                cycle_id: cycle_id.clone(),
                phase: ExplorationRunPhase::Building,
                attempt,
                max_attempts,
                running_builds: 1,
                pending_builds: 0,
                current_version_id: current_version_id.clone(),
                summary: if attempt == 1 {
                    "Building bounded draft."
                } else {
                    "Repairing exact verification failures."
                }
                .into(),
                raw_error: last_error.clone(),
            },
        )
        .await;
        let prompt = if let Some(error) = &last_error {
            format!(
                "{}\n\nREPAIR EXACTLY THESE FAILURES:\n{}\nMake smallest bounded repair.",
                input.prompt, error
            )
        } else {
            input.prompt.clone()
        };
        let generated = generate_design_core(
            GenerateDesignCoreInput {
                prompt,
                thread_id: Some(input.thread_id.clone()),
                parent_macro_code: current_design
                    .as_ref()
                    .map(|d| d.macro_code.clone())
                    .or(input.parent_macro_code.clone()),
                working_design: current_design.clone(),
                is_retry: attempt > 1,
                image_data: input.image_data.clone(),
                attachments: Some(input.attachments.clone()),
                options: Some(input.options.clone()),
            },
            state,
            app,
        )
        .await;
        let generated = match generated {
            Ok(value) => value,
            Err(error) => {
                last_error = Some(error.details.clone().unwrap_or(error.message.clone()));
                if attempt == max_attempts {
                    return finish_error(
                        &input,
                        message_id,
                        last_error.clone().unwrap(),
                        state,
                        app,
                        max_attempts,
                        attempt,
                        current_version_id,
                        cycle_id.clone(),
                        latest_version_id.clone(),
                        current_design.clone(),
                        last_bundle.clone(),
                        last_manifest.clone(),
                        last_usage.clone(),
                        last_verification.clone(),
                    )
                    .await;
                }
                continue;
            }
        };
        if state
            .exploration_run_registry
            .is_cancelled(&input.request_id)
            .await
        {
            return finish_stopped(
                &input,
                message_id,
                "Exploration run cancelled.".into(),
                state,
                app,
                max_attempts,
                attempt,
                current_version_id,
                cycle_id.clone(),
                latest_version_id.clone(),
                current_design.clone(),
                last_bundle.clone(),
                last_manifest.clone(),
                last_usage.clone(),
                last_verification.clone(),
            )
            .await;
        }
        current_design = Some(generated.design.clone());
        last_usage = generated.usage.clone();
        let version_id = {
            let db = state.db.lock().await;
            persist_generation_draft_in_db(
                &db,
                message_id.clone(),
                generated.design.clone(),
                generated.usage.clone(),
            )?
        };
        current_version_id = Some(version_id.clone());
        latest_version_id = Some(version_id.clone());
        // Evidence belongs to one exact version. A newly appended draft has
        // no render/verification until this iteration produces it.
        last_bundle = None;
        last_manifest = None;
        last_verification = None;
        emit_progress(
            state,
            ExplorationRunProgress {
                request_id: input.request_id.clone(),
                thread_id: input.thread_id.clone(),
                cycle_id: cycle_id.clone(),
                phase: ExplorationRunPhase::Verifying,
                attempt,
                max_attempts,
                running_builds: 1,
                pending_builds: 0,
                current_version_id: current_version_id.clone(),
                summary: "Draft appended; verifying exact version.".into(),
                raw_error: None,
            },
        )
        .await;

        let rendered = crate::services::render::render_model_with_previous_manifest(
            &generated.design.macro_code,
            &generated.design.initial_params,
            Some(generated.design.macro_dialect.clone()),
            Some(generated.design.geometry_backend),
            generated.design.post_processing.as_ref(),
            last_manifest.as_ref(),
            state,
            app,
        )
        .await;
        let bundle = match rendered {
            Ok(value) => value,
            Err(error) => {
                last_error = Some(error.details.clone().unwrap_or(error.message.clone()));
                if attempt == max_attempts {
                    return finish_error(
                        &input,
                        message_id,
                        last_error.clone().unwrap(),
                        state,
                        app,
                        max_attempts,
                        attempt,
                        current_version_id,
                        cycle_id.clone(),
                        latest_version_id.clone(),
                        current_design.clone(),
                        last_bundle.clone(),
                        last_manifest.clone(),
                        last_usage.clone(),
                        last_verification.clone(),
                    )
                    .await;
                }
                finalize_generation_core(
                    FinalizeGenerationCoreInput {
                        message_id: version_id.clone(),
                        status: crate::contracts::FinalizeStatus::Error,
                        design: Some(generated.design.clone()),
                        usage: generated.usage.clone(),
                        artifact_bundle: None,
                        model_manifest: None,
                        error_message: last_error.clone(),
                        response_text: None,
                    },
                    state,
                    app,
                )
                .await?;
                state.emit_history_updated();
                continue;
            }
        };
        if state
            .exploration_run_registry
            .is_cancelled(&input.request_id)
            .await
        {
            return finish_stopped(
                &input,
                message_id,
                "Exploration run cancelled.".into(),
                state,
                app,
                max_attempts,
                attempt,
                current_version_id,
                cycle_id.clone(),
                latest_version_id.clone(),
                current_design.clone(),
                last_bundle.clone(),
                last_manifest.clone(),
                last_usage.clone(),
                last_verification.clone(),
            )
            .await;
        }
        let manifest = crate::model_runtime::read_model_manifest(app, &bundle.model_id)?;
        let _snapshot = build_render_snapshot(RenderSnapshotInput {
            design: &generated.design,
            effective_params: &generated.design.initial_params,
            artifact_bundle: &bundle,
            model_manifest: &manifest,
        })?;
        let verification = crate::services::author_verification_foundation::verify_structure_with_author_verification_and_params(
            &bundle,
            &manifest,
            &generated.design.initial_params,
        );
        let raw_verification = if verification.passed {
            None
        } else {
            Some(format_issues(&verification))
        };
        {
            let db = state.db.lock().await;
            persist_structural_verification_core(&db, &version_id, &verification)?;
        }
        last_verification = Some(verification.clone());
        last_bundle = Some(bundle.clone());
        last_manifest = Some(manifest.clone());
        if state
            .exploration_run_registry
            .is_cancelled(&input.request_id)
            .await
        {
            return finish_stopped(
                &input,
                message_id,
                "Exploration run cancelled.".into(),
                state,
                app,
                max_attempts,
                attempt,
                current_version_id,
                cycle_id.clone(),
                latest_version_id.clone(),
                current_design.clone(),
                last_bundle.clone(),
                last_manifest.clone(),
                last_usage.clone(),
                last_verification.clone(),
            )
            .await;
        }
        if verification.passed {
            finalize_generation_core(
                FinalizeGenerationCoreInput {
                    message_id: version_id.clone(),
                    status: crate::contracts::FinalizeStatus::Success,
                    design: Some(generated.design.clone()),
                    usage: generated.usage.clone(),
                    artifact_bundle: Some(bundle.clone()),
                    model_manifest: Some(manifest.clone()),
                    error_message: None,
                    response_text: Some(generated.design.response.clone()),
                },
                state,
                app,
            )
            .await?;
            state.emit_history_updated();
            let publication_allowed = state
                .exploration_run_registry
                .publication_allowed(input.thread_id.clone(), &input.request_id)
                .await;
            return Ok(ExplorationRunOutput {
                request_id: input.request_id,
                thread_id: input.thread_id,
                cycle_id,
                phase: ExplorationRunPhase::Completed,
                message_id: version_id,
                design: Some(generated.design),
                artifact_bundle: Some(bundle),
                model_manifest: Some(manifest),
                structural_verification: Some(verification),
                usage: last_usage,
                response_text: current_design.as_ref().map(|d| d.response.clone()),
                raw_error: None,
                publication_allowed,
            });
        }
        last_error = raw_verification;
        finalize_generation_core(
            FinalizeGenerationCoreInput {
                message_id: version_id.clone(),
                status: crate::contracts::FinalizeStatus::Error,
                design: Some(generated.design.clone()),
                usage: generated.usage.clone(),
                artifact_bundle: Some(bundle.clone()),
                model_manifest: Some(manifest.clone()),
                error_message: last_error.clone(),
                response_text: None,
            },
            state,
            app,
        )
        .await?;
        {
            let db = state.db.lock().await;
            persist_structural_verification_core(&db, &version_id, &verification)?;
        }
        state.emit_history_updated();
        if attempt == max_attempts {
            break;
        }
    }
    finish_error(
        &input,
        message_id,
        last_error.unwrap_or_else(|| "Exploration verification failed.".into()),
        state,
        app,
        max_attempts,
        max_attempts,
        current_version_id,
        cycle_id,
        latest_version_id,
        current_design,
        last_bundle,
        last_manifest,
        last_usage,
        last_verification,
    )
    .await
}

/// Run one adaptive PLAN -> BUILD -> VERIFY -> DECIDE cycle.
///
/// PLAN and BUILD are intentionally one provider turn. The provider returns a
/// transient `next_action`; the reducer validates it against the exact current
/// version before any source append or render. ASK and STOP finish the pending
/// assistant message without persisting the provider's design as a version.
async fn run_cycle_admitted(
    input: StartExplorationRunInput,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ExplorationRunOutput> {
    let max_attempts = state
        .config
        .lock()
        .map_err(|_| AppError::persistence("Config lock poisoned."))?
        .max_generation_attempts
        .max(1);
    let active_cycle = crate::commands::exploration_cycle::get_active_exploration_cycle_core(
        input.thread_id.clone(),
        state,
    )
    .await?;
    let awaiting_cycle = active_cycle.as_ref().filter(|packet| {
        packet.state.phase == crate::contracts::exploration_cycle::CyclePhase::AwaitingInput
    });

    // Keep the pending assistant message as the run's conversation projection.
    // It becomes a version only when BUILD actually appends changed source.
    let message_id = init_generation_core(
        InitGenerationCoreInput {
            thread_id: input.thread_id.clone(),
            prompt: input.prompt.clone(),
            attachments: Some(input.attachments.clone()),
            image_data: input.image_data.clone(),
            classification_request_id: Some(input.request_id.clone()),
        },
        state,
        app,
    )
    .await?;
    let (cycle_id, initial_version_id) = if let Some(packet) = awaiting_cycle {
        // A new run submitted while ASK is pending is the answer action. Bind
        // it to the durable question before the next provider turn; never
        // create a second cycle or infer the decision from conversation text.
        let answered = crate::commands::exploration_cycle::answer_exploration_cycle_core(
            packet.state.cycle_id.clone(),
            input.prompt.clone(),
            state,
        )
        .await?;
        (answered.state.cycle_id, answered.state.current_version_id)
    } else {
        let base_version_id = input
            .base_version_id
            .clone()
            .ok_or_else(|| AppError::validation("Exploration requires an exact base version."))?;
        let criteria = if input.acceptance_criteria.is_empty() {
            vec!["deterministic structural verification is green".to_string()]
        } else {
            input.acceptance_criteria.clone()
        };
        let packet = crate::commands::exploration_cycle::start_exploration_cycle_core(
            StartCycleInput {
                thread_id: input.thread_id.clone(),
                base_version_id: base_version_id.clone(),
                objective: input.prompt.clone(),
                acceptance_criteria: criteria,
                hard_constraints: input.hard_constraints.clone(),
                soft_preferences: input.soft_preferences.clone(),
                budget: max_attempts,
            },
            state,
        )
        .await?;
        (packet.state.cycle_id, base_version_id)
    };

    let mut current_version_id = Some(initial_version_id);
    let mut draft_message_id = message_id.clone();
    let mut latest_version_id = None;
    let mut last_error: Option<String> = None;
    let mut last_bundle = None;
    let mut last_manifest = None;
    let mut last_usage = None;
    let mut last_verification = None;

    // The persisted version is the only source authority for a version-bound
    // run. Caller-provided workingDesign/parentMacroCode can be stale or point
    // at another version, so never use either to seed the provider context.
    let mut current_design = {
        let db = state.db.lock().await;
        db::get_message_output_and_thread(&db, current_version_id.as_deref().unwrap())
            .map_err(|error| AppError::persistence(error.to_string()))?
            .filter(|(_, thread_id)| thread_id == &input.thread_id)
            .map(|(design, _)| design)
            .ok_or_else(|| {
                AppError::validation(
                    "Exploration currentVersionId has no persisted source in this thread.",
                )
            })?
    };

    emit_progress(
        state,
        ExplorationRunProgress {
            request_id: input.request_id.clone(),
            thread_id: input.thread_id.clone(),
            cycle_id: Some(cycle_id.clone()),
            phase: ExplorationRunPhase::Planning,
            attempt: 0,
            max_attempts,
            running_builds: 1,
            pending_builds: 0,
            current_version_id: current_version_id.clone(),
            summary: "Planning from the exact current version.".into(),
            raw_error: None,
        },
    )
    .await;

    for attempt in 1..=max_attempts {
        if state
            .exploration_run_registry
            .is_cancelled(&input.request_id)
            .await
        {
            return finish_stopped(
                &input,
                message_id,
                "Exploration run cancelled.".into(),
                state,
                app,
                max_attempts,
                attempt,
                current_version_id,
                Some(cycle_id),
                latest_version_id,
                Some(current_design),
                last_bundle,
                last_manifest,
                last_usage,
                last_verification,
            )
            .await;
        }

        emit_progress(
            state,
            ExplorationRunProgress {
                request_id: input.request_id.clone(),
                thread_id: input.thread_id.clone(),
                cycle_id: Some(cycle_id.clone()),
                phase: ExplorationRunPhase::Planning,
                attempt,
                max_attempts,
                running_builds: 1,
                pending_builds: 0,
                current_version_id: current_version_id.clone(),
                summary: if attempt == 1 {
                    "Authoring one typed next step from current context."
                } else {
                    "Replanning from exact verification evidence."
                }
                .into(),
                raw_error: last_error.clone(),
            },
        )
        .await;

        let provider_prompt = cycle_provider_prompt(&input.prompt, last_error.as_deref());
        let generated = generate_design_core(
            GenerateDesignCoreInput {
                prompt: provider_prompt,
                thread_id: Some(input.thread_id.clone()),
                parent_macro_code: Some(current_design.macro_code.clone()),
                working_design: Some(current_design.clone()),
                is_retry: attempt > 1,
                image_data: input.image_data.clone(),
                attachments: Some(input.attachments.clone()),
                options: Some(input.options.clone()),
            },
            state,
            app,
        )
        .await;
        let generated = match generated {
            Ok(value) => value,
            Err(error) => {
                let raw_error = error.details.clone().unwrap_or(error.message.clone());
                last_error = Some(raw_error.clone());
                // Provider parse/validation failures must be durable cycle
                // events and return to PLAN. Transport failures are already
                // recorded by generation_core; helper avoids a duplicate.
                record_cycle_failure(&cycle_id, &raw_error, state).await?;
                if attempt == max_attempts {
                    return finish_error(
                        &input,
                        message_id,
                        raw_error,
                        state,
                        app,
                        max_attempts,
                        attempt,
                        current_version_id,
                        Some(cycle_id),
                        latest_version_id,
                        Some(current_design),
                        last_bundle,
                        last_manifest,
                        last_usage,
                        last_verification,
                    )
                    .await;
                }
                continue;
            }
        };
        last_usage = generated.usage.clone();

        // This is the only PLAN authority. The provider's action carries all
        // hypothesis/scope/evidence fields; no generic fallback is allowed.
        let plan = generated.next_action.clone().ok_or_else(|| {
            AppError::with_details(
                crate::contracts::AppErrorCode::Validation,
                "Active exploration cycle returned no typed PLAN action.",
                "Provider response omitted nextAction after cycle validation.".to_string(),
            )
        })?;
        let plan_action = plan.action;
        let plan_question = plan.question.clone();
        let source_version_id = plan.source_version_id.clone();
        if let Err(error) = accept_provider_plan(&cycle_id, plan, state).await {
            let raw_error = error.details.clone().unwrap_or(error.message.clone());
            last_error = Some(raw_error.clone());
            record_cycle_failure(&cycle_id, &raw_error, state).await?;
            if attempt == max_attempts {
                return finish_error(
                    &input,
                    message_id,
                    raw_error,
                    state,
                    app,
                    max_attempts,
                    attempt,
                    current_version_id,
                    Some(cycle_id),
                    latest_version_id,
                    Some(current_design),
                    last_bundle,
                    last_manifest,
                    last_usage,
                    last_verification,
                )
                .await;
            }
            continue;
        }

        match plan_action {
            PlanAction::Ask => {
                let response = plan_question
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| {
                        if generated.design.response.trim().is_empty() {
                            "Please choose how to continue this design.".to_string()
                        } else {
                            generated.design.response.clone()
                        }
                    });
                finalize_generation_core(
                    FinalizeGenerationCoreInput {
                        message_id: message_id.clone(),
                        status: crate::contracts::FinalizeStatus::Success,
                        design: None,
                        usage: generated.usage.clone(),
                        artifact_bundle: None,
                        model_manifest: None,
                        error_message: None,
                        response_text: Some(response.clone()),
                    },
                    state,
                    app,
                )
                .await?;
                state.emit_history_updated();
                emit_progress(
                    state,
                    ExplorationRunProgress {
                        request_id: input.request_id.clone(),
                        thread_id: input.thread_id.clone(),
                        cycle_id: Some(cycle_id.clone()),
                        phase: ExplorationRunPhase::AwaitingInput,
                        attempt,
                        max_attempts,
                        running_builds: 0,
                        pending_builds: 0,
                        current_version_id: current_version_id.clone(),
                        summary: "Waiting for the answer to the persisted design decision.".into(),
                        raw_error: None,
                    },
                )
                .await;
                return Ok(ExplorationRunOutput {
                    request_id: input.request_id,
                    thread_id: input.thread_id,
                    cycle_id: Some(cycle_id),
                    phase: ExplorationRunPhase::AwaitingInput,
                    message_id,
                    design: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    structural_verification: None,
                    usage: last_usage,
                    response_text: Some(response),
                    raw_error: None,
                    publication_allowed: false,
                });
            }
            PlanAction::Stop => {
                let response = if generated.design.response.trim().is_empty() {
                    "Exploration stopped by the plan.".to_string()
                } else {
                    generated.design.response.clone()
                };
                finalize_generation_core(
                    FinalizeGenerationCoreInput {
                        message_id: message_id.clone(),
                        status: crate::contracts::FinalizeStatus::Success,
                        design: None,
                        usage: generated.usage.clone(),
                        artifact_bundle: None,
                        model_manifest: None,
                        error_message: None,
                        response_text: Some(response.clone()),
                    },
                    state,
                    app,
                )
                .await?;
                state.emit_history_updated();
                return Ok(ExplorationRunOutput {
                    request_id: input.request_id,
                    thread_id: input.thread_id,
                    cycle_id: Some(cycle_id),
                    phase: ExplorationRunPhase::Stopped,
                    message_id,
                    design: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    structural_verification: None,
                    usage: last_usage,
                    response_text: Some(response),
                    raw_error: None,
                    publication_allowed: false,
                });
            }
            PlanAction::Build => {}
        }

        crate::commands::exploration_cycle::next_exploration_cycle_core(
            CycleNextInput {
                cycle_id: cycle_id.clone(),
                action: CycleNextAction::BuildStarted { source_version_id },
                route: None,
            },
            state,
        )
        .await?;
        emit_progress(
            state,
            ExplorationRunProgress {
                request_id: input.request_id.clone(),
                thread_id: input.thread_id.clone(),
                cycle_id: Some(cycle_id.clone()),
                phase: ExplorationRunPhase::Building,
                attempt,
                max_attempts,
                running_builds: 1,
                pending_builds: 0,
                current_version_id: current_version_id.clone(),
                summary: "Applying the provider's bounded BUILD step.".into(),
                raw_error: last_error.clone(),
            },
        )
        .await;

        current_design = generated.design.clone();
        let version_id = {
            let db = state.db.lock().await;
            persist_generation_draft_in_db(
                &db,
                draft_message_id.clone(),
                generated.design.clone(),
                generated.usage.clone(),
            )?
        };
        // The append service may return the existing current version for an
        // identical first draft and discard the pending placeholder. Future
        // repairs must target that real version, never revive the placeholder.
        draft_message_id = version_id.clone();
        current_version_id = Some(version_id.clone());
        latest_version_id = Some(version_id.clone());
        last_bundle = None;
        last_manifest = None;
        last_verification = None;
        crate::commands::exploration_cycle::next_exploration_cycle_core(
            CycleNextInput {
                cycle_id: cycle_id.clone(),
                action: CycleNextAction::VersionAppended {
                    result_version_id: version_id.clone(),
                },
                route: None,
            },
            state,
        )
        .await?;

        emit_progress(
            state,
            ExplorationRunProgress {
                request_id: input.request_id.clone(),
                thread_id: input.thread_id.clone(),
                cycle_id: Some(cycle_id.clone()),
                phase: ExplorationRunPhase::Verifying,
                attempt,
                max_attempts,
                running_builds: 1,
                pending_builds: 0,
                current_version_id: current_version_id.clone(),
                summary: "Draft appended; verifying that exact version.".into(),
                raw_error: None,
            },
        )
        .await;

        let rendered = crate::services::render::render_model_with_previous_manifest(
            &generated.design.macro_code,
            &generated.design.initial_params,
            Some(generated.design.macro_dialect.clone()),
            Some(generated.design.geometry_backend),
            generated.design.post_processing.as_ref(),
            last_manifest.as_ref(),
            state,
            app,
        )
        .await;
        let bundle = match rendered {
            Ok(value) => value,
            Err(error) => {
                let raw_error = error.details.clone().unwrap_or(error.message.clone());
                last_error = Some(raw_error.clone());
                finalize_generation_core(
                    FinalizeGenerationCoreInput {
                        message_id: version_id.clone(),
                        status: crate::contracts::FinalizeStatus::Error,
                        design: Some(generated.design.clone()),
                        usage: generated.usage.clone(),
                        artifact_bundle: None,
                        model_manifest: None,
                        error_message: Some(raw_error.clone()),
                        response_text: None,
                    },
                    state,
                    app,
                )
                .await?;
                state.emit_history_updated();
                record_cycle_failure(&cycle_id, &raw_error, state).await?;
                if attempt == max_attempts {
                    return finish_error(
                        &input,
                        message_id,
                        raw_error,
                        state,
                        app,
                        max_attempts,
                        attempt,
                        current_version_id,
                        Some(cycle_id),
                        latest_version_id,
                        Some(current_design),
                        last_bundle,
                        last_manifest,
                        last_usage,
                        last_verification,
                    )
                    .await;
                }
                continue;
            }
        };
        let manifest = crate::model_runtime::read_model_manifest(app, &bundle.model_id)?;
        let snapshot = build_render_snapshot(RenderSnapshotInput {
            design: &generated.design,
            effective_params: &generated.design.initial_params,
            artifact_bundle: &bundle,
            model_manifest: &manifest,
        })?;
        let verification =
            crate::services::author_verification_foundation::verify_structure_with_author_verification_and_params(
                &bundle,
                &manifest,
                &generated.design.initial_params,
            );
        let raw_verification = (!verification.passed).then(|| format_issues(&verification));
        {
            let db = state.db.lock().await;
            persist_structural_verification_core(&db, &version_id, &verification)?;
        }
        last_verification = Some(verification.clone());
        last_bundle = Some(bundle.clone());
        last_manifest = Some(manifest.clone());
        if let Some(cycle) = Some(cycle_id.clone()) {
            let digest = crate::services::render_snapshot::canonical_version_input_digest(
                &generated.design,
                &generated.design.initial_params,
            )?;
            crate::commands::exploration_cycle::next_exploration_cycle_core(
                CycleNextInput {
                    cycle_id: cycle.clone(),
                    action: CycleNextAction::Verify {
                        verification: crate::contracts::exploration_cycle::Verification {
                            version_id: version_id.clone(),
                            input_digest: digest,
                            evidence_ref: snapshot.snapshot_id.clone(),
                            deterministic: if verification.passed {
                                crate::contracts::exploration_cycle::VerificationVerdict::Green
                            } else {
                                crate::contracts::exploration_cycle::VerificationVerdict::Red
                            },
                            vision: None,
                        },
                        raw_error: raw_verification.clone(),
                        render_snapshot_id: Some(snapshot.snapshot_id.clone()),
                        artifact_digest: Some(snapshot.artifact_digest.clone()),
                    },
                    route: None,
                },
                state,
            )
            .await?;
            let decision = if verification.passed {
                crate::contracts::exploration_cycle::Decision::Complete
            } else {
                crate::contracts::exploration_cycle::Decision::Replan
            };
            crate::commands::exploration_cycle::next_exploration_cycle_core(
                CycleNextInput {
                    cycle_id: cycle,
                    action: CycleNextAction::Decide { decision },
                    route: None,
                },
                state,
            )
            .await?;
        }

        if verification.passed {
            finalize_generation_core(
                FinalizeGenerationCoreInput {
                    message_id: version_id.clone(),
                    status: crate::contracts::FinalizeStatus::Success,
                    design: Some(generated.design.clone()),
                    usage: generated.usage.clone(),
                    artifact_bundle: Some(bundle.clone()),
                    model_manifest: Some(manifest.clone()),
                    error_message: None,
                    response_text: Some(generated.design.response.clone()),
                },
                state,
                app,
            )
            .await?;
            state.emit_history_updated();
            let publication_allowed = state
                .exploration_run_registry
                .publication_allowed(input.thread_id.clone(), &input.request_id)
                .await;
            return Ok(ExplorationRunOutput {
                request_id: input.request_id,
                thread_id: input.thread_id,
                cycle_id: Some(cycle_id),
                phase: ExplorationRunPhase::Completed,
                message_id: version_id,
                design: Some(generated.design),
                artifact_bundle: Some(bundle),
                model_manifest: Some(manifest),
                structural_verification: Some(verification),
                usage: last_usage,
                response_text: Some(current_design.response.clone()),
                raw_error: None,
                publication_allowed,
            });
        }
        last_error = raw_verification;
        finalize_generation_core(
            FinalizeGenerationCoreInput {
                message_id: version_id.clone(),
                status: crate::contracts::FinalizeStatus::Error,
                design: Some(generated.design.clone()),
                usage: generated.usage.clone(),
                artifact_bundle: Some(bundle.clone()),
                model_manifest: Some(manifest.clone()),
                error_message: last_error.clone(),
                response_text: None,
            },
            state,
            app,
        )
        .await?;
        state.emit_history_updated();
        if attempt == max_attempts {
            break;
        }
    }

    finish_error(
        &input,
        message_id,
        last_error.unwrap_or_else(|| "Exploration verification failed.".into()),
        state,
        app,
        max_attempts,
        max_attempts,
        current_version_id,
        Some(cycle_id),
        latest_version_id,
        Some(current_design),
        last_bundle,
        last_manifest,
        last_usage,
        last_verification,
    )
    .await
}

async fn run_question_only(
    input: StartExplorationRunInput,
    state: &AppState,
    app: &dyn PathResolver,
    max_attempts: u32,
) -> AppResult<ExplorationRunOutput> {
    let message_id = init_generation_core(
        InitGenerationCoreInput {
            thread_id: input.thread_id.clone(),
            prompt: input.prompt.clone(),
            attachments: Some(input.attachments.clone()),
            image_data: input.image_data.clone(),
            classification_request_id: Some(input.request_id.clone()),
        },
        state,
        app,
    )
    .await?;
    emit_progress(
        state,
        ExplorationRunProgress {
            request_id: input.request_id.clone(),
            thread_id: input.thread_id.clone(),
            cycle_id: None,
            phase: ExplorationRunPhase::Planning,
            attempt: 0,
            max_attempts,
            running_builds: 1,
            pending_builds: 0,
            current_version_id: input.base_version_id.clone(),
            summary: "Answering without changing geometry.".into(),
            raw_error: None,
        },
    )
    .await;
    if state
        .exploration_run_registry
        .is_cancelled(&input.request_id)
        .await
    {
        return finish_question_stopped(&input, message_id, state, app, max_attempts).await;
    }
    let mut options = input.options.clone();
    options.question_mode = Some(true);
    let generated = generate_design_core(
        GenerateDesignCoreInput {
            prompt: input.prompt.clone(),
            thread_id: Some(input.thread_id.clone()),
            parent_macro_code: input.parent_macro_code.clone(),
            working_design: input.working_design.clone(),
            is_retry: false,
            image_data: input.image_data.clone(),
            attachments: Some(input.attachments.clone()),
            options: Some(options),
        },
        state,
        app,
    )
    .await;
    if state
        .exploration_run_registry
        .is_cancelled(&input.request_id)
        .await
    {
        return finish_question_stopped(&input, message_id, state, app, max_attempts).await;
    }
    let generated = match generated {
        Ok(value) => value,
        Err(error) => {
            let raw_error = error.details.clone().unwrap_or(error.message.clone());
            finalize_generation_core(
                FinalizeGenerationCoreInput {
                    message_id: message_id.clone(),
                    status: crate::contracts::FinalizeStatus::Error,
                    design: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    error_message: Some(raw_error.clone()),
                    response_text: None,
                },
                state,
                app,
            )
            .await?;
            state.emit_history_updated();
            return Ok(ExplorationRunOutput {
                request_id: input.request_id,
                thread_id: input.thread_id,
                cycle_id: None,
                phase: ExplorationRunPhase::Failed,
                message_id,
                design: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                usage: None,
                response_text: None,
                raw_error: Some(raw_error),
                publication_allowed: false,
            });
        }
    };
    let response_text = if generated.design.response.trim().is_empty() {
        "Question answered. Geometry unchanged.".to_string()
    } else {
        generated.design.response.clone()
    };
    finalize_generation_core(
        FinalizeGenerationCoreInput {
            message_id: message_id.clone(),
            status: crate::contracts::FinalizeStatus::Success,
            design: None,
            usage: generated.usage.clone(),
            artifact_bundle: None,
            model_manifest: None,
            error_message: None,
            response_text: Some(response_text.clone()),
        },
        state,
        app,
    )
    .await?;
    state.emit_history_updated();
    Ok(ExplorationRunOutput {
        request_id: input.request_id,
        thread_id: input.thread_id,
        cycle_id: None,
        phase: ExplorationRunPhase::Completed,
        message_id,
        design: None,
        artifact_bundle: None,
        model_manifest: None,
        structural_verification: None,
        usage: generated.usage,
        response_text: Some(response_text),
        raw_error: None,
        publication_allowed: false,
    })
}

async fn finish_question_stopped(
    input: &StartExplorationRunInput,
    message_id: String,
    state: &AppState,
    app: &dyn PathResolver,
    max_attempts: u32,
) -> AppResult<ExplorationRunOutput> {
    let raw_error = "Exploration run cancelled.".to_string();
    finalize_generation_core(
        FinalizeGenerationCoreInput {
            message_id: message_id.clone(),
            status: crate::contracts::FinalizeStatus::Error,
            design: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            error_message: Some(raw_error.clone()),
            response_text: None,
        },
        state,
        app,
    )
    .await?;
    state.emit_history_updated();
    emit_progress(
        state,
        ExplorationRunProgress {
            request_id: input.request_id.clone(),
            thread_id: input.thread_id.clone(),
            cycle_id: None,
            phase: ExplorationRunPhase::Stopped,
            attempt: 1,
            max_attempts,
            running_builds: 1,
            pending_builds: 0,
            current_version_id: input.base_version_id.clone(),
            summary: "Exploration stopped by user.".into(),
            raw_error: Some(raw_error.clone()),
        },
    )
    .await;
    Ok(ExplorationRunOutput {
        request_id: input.request_id.clone(),
        thread_id: input.thread_id.clone(),
        cycle_id: None,
        phase: ExplorationRunPhase::Stopped,
        message_id,
        design: None,
        artifact_bundle: None,
        model_manifest: None,
        structural_verification: None,
        usage: None,
        response_text: None,
        raw_error: Some(raw_error),
        publication_allowed: false,
    })
}

async fn finish_error(
    input: &StartExplorationRunInput,
    message_id: String,
    error: String,
    state: &AppState,
    app: &dyn PathResolver,
    max_attempts: u32,
    attempt: u32,
    current_version_id: Option<String>,
    cycle_id: Option<String>,
    latest_version_id: Option<String>,
    latest_design: Option<crate::contracts::DesignOutput>,
    latest_bundle: Option<crate::contracts::ArtifactBundle>,
    latest_manifest: Option<crate::contracts::ModelManifest>,
    latest_usage: Option<crate::contracts::UsageSummary>,
    latest_verification: Option<StructuralVerificationResult>,
) -> AppResult<ExplorationRunOutput> {
    let terminal_version_id = latest_version_id
        .clone()
        .unwrap_or_else(|| message_id.clone());
    finalize_generation_core(
        FinalizeGenerationCoreInput {
            message_id: terminal_version_id.clone(),
            status: crate::contracts::FinalizeStatus::Error,
            design: latest_design.clone(),
            usage: latest_usage.clone(),
            artifact_bundle: latest_bundle.clone(),
            model_manifest: latest_manifest.clone(),
            error_message: Some(error.clone()),
            response_text: None,
        },
        state,
        app,
    )
    .await?;
    if latest_version_id.is_some() {
        if let Some(verification) = latest_verification.as_ref() {
            let db = state.db.lock().await;
            persist_structural_verification_core(&db, &terminal_version_id, verification)?;
        }
    }
    state.emit_history_updated();
    finish_cycle(cycle_id.as_deref(), &error, state).await;
    emit_progress(
        state,
        ExplorationRunProgress {
            request_id: input.request_id.clone(),
            thread_id: input.thread_id.clone(),
            cycle_id: cycle_id.clone(),
            phase: ExplorationRunPhase::Failed,
            attempt,
            max_attempts,
            running_builds: 1,
            pending_builds: 0,
            current_version_id: current_version_id.clone(),
            summary: "Exploration stopped with raw failure.".into(),
            raw_error: Some(error.clone()),
        },
    )
    .await;
    Ok(ExplorationRunOutput {
        request_id: input.request_id.clone(),
        thread_id: input.thread_id.clone(),
        cycle_id,
        phase: ExplorationRunPhase::Failed,
        message_id: terminal_version_id,
        response_text: latest_design.as_ref().map(|design| design.response.clone()),
        design: latest_design,
        artifact_bundle: latest_bundle,
        model_manifest: latest_manifest,
        structural_verification: latest_verification,
        usage: latest_usage,
        raw_error: Some(error),
        publication_allowed: false,
    })
}

async fn finish_stopped(
    input: &StartExplorationRunInput,
    message_id: String,
    error: String,
    state: &AppState,
    app: &dyn PathResolver,
    max_attempts: u32,
    attempt: u32,
    current_version_id: Option<String>,
    cycle_id: Option<String>,
    latest_version_id: Option<String>,
    latest_design: Option<crate::contracts::DesignOutput>,
    latest_bundle: Option<crate::contracts::ArtifactBundle>,
    latest_manifest: Option<crate::contracts::ModelManifest>,
    latest_usage: Option<crate::contracts::UsageSummary>,
    latest_verification: Option<StructuralVerificationResult>,
) -> AppResult<ExplorationRunOutput> {
    let terminal_version_id = latest_version_id
        .clone()
        .unwrap_or_else(|| message_id.clone());
    // Cancellation is a cycle transition, not a version outcome. Preserve an
    // already-appended immutable version exactly as written. Only the initial
    // pending placeholder needs a terminal status when no version exists.
    if latest_version_id.is_none() {
        finalize_generation_core(
            FinalizeGenerationCoreInput {
                message_id: terminal_version_id.clone(),
                status: crate::contracts::FinalizeStatus::Error,
                design: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                error_message: Some(error.clone()),
                response_text: None,
            },
            state,
            app,
        )
        .await?;
    }
    state.emit_history_updated();
    finish_cycle_stopped(cycle_id.as_deref(), state).await;
    emit_progress(
        state,
        ExplorationRunProgress {
            request_id: input.request_id.clone(),
            thread_id: input.thread_id.clone(),
            cycle_id: cycle_id.clone(),
            phase: ExplorationRunPhase::Stopped,
            attempt,
            max_attempts,
            running_builds: 1,
            pending_builds: 0,
            current_version_id,
            summary: "Exploration stopped by user.".into(),
            raw_error: Some(error.clone()),
        },
    )
    .await;
    Ok(ExplorationRunOutput {
        request_id: input.request_id.clone(),
        thread_id: input.thread_id.clone(),
        cycle_id,
        phase: ExplorationRunPhase::Stopped,
        message_id: terminal_version_id,
        design: latest_design,
        artifact_bundle: latest_bundle,
        model_manifest: latest_manifest,
        structural_verification: latest_verification,
        usage: latest_usage,
        response_text: None,
        raw_error: Some(error),
        publication_allowed: false,
    })
}

async fn finish_cycle(cycle_id: Option<&str>, error: &str, state: &AppState) {
    let Some(cycle_id) = cycle_id else {
        return;
    };
    let _ = crate::commands::exploration_cycle::next_exploration_cycle_core(
        CycleNextInput {
            cycle_id: cycle_id.to_string(),
            action: CycleNextAction::ProviderFailed {
                raw_error: error.to_string(),
            },
            route: None,
        },
        state,
    )
    .await;
    let _ = crate::commands::exploration_cycle::stop_exploration_cycle_core(
        cycle_id.to_string(),
        state,
    )
    .await;
}

async fn finish_cycle_stopped(cycle_id: Option<&str>, state: &AppState) {
    let Some(cycle_id) = cycle_id else {
        return;
    };
    let _ = crate::commands::exploration_cycle::stop_exploration_cycle_core(
        cycle_id.to_string(),
        state,
    )
    .await;
}

fn cycle_provider_prompt(prompt: &str, exact_error: Option<&str>) -> String {
    match exact_error.map(str::trim).filter(|value| !value.is_empty()) {
        Some(error) => format!(
            "{prompt}\n\nEXACT PRIOR VERIFICATION EVIDENCE (AUTHORITATIVE):\n{error}\n\nPlan the smallest bounded repair that addresses these exact diagnostics. Preserve existing verification intent.",
        ),
        None => prompt.to_string(),
    }
}

async fn accept_provider_plan(
    cycle_id: &str,
    proposal: PlanProposal,
    state: &AppState,
) -> AppResult<()> {
    crate::commands::exploration_cycle::next_exploration_cycle_core(
        CycleNextInput {
            cycle_id: cycle_id.to_string(),
            action: CycleNextAction::Plan { proposal },
            route: None,
        },
        state,
    )
    .await
    .map(|_| ())
}

async fn record_cycle_failure(cycle_id: &str, error: &str, state: &AppState) -> AppResult<()> {
    if error.trim().is_empty() {
        return Ok(());
    }
    let packet =
        crate::commands::exploration_cycle::get_exploration_cycle_core(cycle_id.to_string(), state)
            .await?;
    // Transport/parse failures from `generate_design_core` already append the
    // ProviderFailed event while recording the model route. Do not duplicate
    // that event when the runner receives the propagated AppError.
    let after_sequence = packet.event_count.saturating_sub(1);
    if let Some(last) = crate::commands::exploration_cycle::get_exploration_cycle_events_core(
        cycle_id.to_string(),
        Some(after_sequence),
        Some(1),
        state,
    )
    .await?
    .last()
    {
        if last.event_type == crate::contracts::exploration_cycle::CycleEventType::ProviderFailed
            && last.raw_error.as_deref() == Some(error)
        {
            return Ok(());
        }
    }
    if packet.state.status != crate::contracts::exploration_cycle::CycleStatus::Active {
        return Ok(());
    }
    crate::commands::exploration_cycle::next_exploration_cycle_core(
        CycleNextInput {
            cycle_id: cycle_id.to_string(),
            action: CycleNextAction::ProviderFailed {
                raw_error: error.to_string(),
            },
            route: None,
        },
        state,
    )
    .await
    .map(|_| ())
}

async fn terminate_active_cycle_on_error(thread_id: &str, error: &AppError, state: &AppState) {
    let Ok(Some(packet)) = crate::commands::exploration_cycle::get_active_exploration_cycle_core(
        thread_id.to_string(),
        state,
    )
    .await
    else {
        return;
    };
    finish_cycle(
        Some(packet.state.cycle_id.as_str()),
        &error.to_string(),
        state,
    )
    .await;
}

fn validate_input(input: &StartExplorationRunInput) -> AppResult<()> {
    if input.request_id.trim().is_empty() {
        return Err(AppError::validation("requestId cannot be empty."));
    }
    if input.thread_id.trim().is_empty() {
        return Err(AppError::validation("threadId cannot be empty."));
    }
    if input.prompt.trim().is_empty() {
        return Err(AppError::validation("prompt cannot be empty."));
    }
    Ok(())
}

fn format_issues(result: &StructuralVerificationResult) -> String {
    result
        .issues
        .iter()
        .map(|issue| format!("{}: {}", issue.code, issue.message))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn emit_progress(state: &AppState, mut progress: ExplorationRunProgress) {
    let counts = state
        .exploration_run_registry
        .counts(progress.thread_id.clone())
        .await;
    progress.running_builds = counts.running;
    progress.pending_builds = counts.pending;
    if let Some(handle) = state.app_handle.lock().unwrap().clone() {
        let _ = handle.emit("exploration-run-progress", &progress);
    }
}

#[cfg(test)]
mod tests {
    use super::{format_issues, validate_input};
    use crate::contracts::exploration_run::{ExplorationRunKind, StartExplorationRunInput};
    use crate::contracts::{
        StructuralIssue, StructuralMetrics, StructuralVerificationResult, VerifierStatus,
    };

    fn input() -> StartExplorationRunInput {
        StartExplorationRunInput {
            request_id: "request-1".into(),
            thread_id: "thread-1".into(),
            prompt: "make a bracket".into(),
            attachments: vec![],
            image_data: None,
            parent_macro_code: None,
            working_design: None,
            base_version_id: None,
            kind: ExplorationRunKind::Interactive,
            options: Default::default(),
            acceptance_criteria: vec![],
            hard_constraints: vec![],
            soft_preferences: vec![],
        }
    }

    #[test]
    fn validate_input_rejects_missing_identity_and_prompt() {
        let mut value = input();
        value.request_id.clear();
        assert_eq!(
            validate_input(&value).unwrap_err().message,
            "requestId cannot be empty."
        );

        let mut value = input();
        value.thread_id.clear();
        assert_eq!(
            validate_input(&value).unwrap_err().message,
            "threadId cannot be empty."
        );

        let mut value = input();
        value.prompt.clear();
        assert_eq!(
            validate_input(&value).unwrap_err().message,
            "prompt cannot be empty."
        );
    }

    #[test]
    fn format_issues_preserves_machine_code_and_message() {
        let result = StructuralVerificationResult {
            passed: false,
            summary: "red".into(),
            issues: vec![
                StructuralIssue {
                    code: "OVERLAP".into(),
                    message: "parts overlap".into(),
                    part_id: None,
                    numeric_payload: None,
                    diagnostic_context: None,
                },
                StructuralIssue {
                    code: "GAP".into(),
                    message: "connector gap".into(),
                    part_id: Some("joint".into()),
                    numeric_payload: None,
                    diagnostic_context: None,
                },
            ],
            authored_verify_checks: vec![],
            metrics: StructuralMetrics {
                part_count: 0,
                model_stl_size_bytes: None,
                model_stl_triangle_count: None,
                model_stl_component_count: None,
                model_stl_non_manifold_edge_count: None,
                model_stl_overhang_triangle_count: None,
                model_stl_overhang_ratio: None,
                total_volume: None,
                total_area: None,
                bbox: None,
            },
            verifier_status: VerifierStatus::OkRustOnly,
            verifier_source: None,
        };

        assert_eq!(
            format_issues(&result),
            "OVERLAP: parts overlap\nGAP: connector gap"
        );
    }

    #[test]
    fn cycle_provider_prompt_preserves_exact_repair_evidence() {
        let prompt = super::cycle_provider_prompt(
            "make a bracket",
            Some("MIN_WALL: wall is 0.6mm; sourceVersionId=version-b"),
        );
        assert!(prompt.contains("MIN_WALL: wall is 0.6mm; sourceVersionId=version-b"));
        assert!(prompt.contains("smallest bounded repair"));
        assert!(!prompt.contains("generic repair"));
    }
}
