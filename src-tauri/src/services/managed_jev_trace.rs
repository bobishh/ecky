use serde_json::json;
use std::path::Path;

use crate::contracts::{AppError, AppResult};
use crate::jev_classifier::{AcceptedRoute, ClassifierRequest};
use crate::llm_eval::{
    EvalCase, EvalEvent, EvalEventKind, EvalJevRoute, EvalPayload, EvalRoute, EvalRun, EvalRunSeed,
};
use crate::models::{AppState, PathResolver};

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn system_event(
    sequence: u64,
    name: &str,
    state: &str,
    summary: &str,
    input: Option<serde_json::Value>,
    output: Option<serde_json::Value>,
    error: Option<String>,
) -> EvalEvent {
    EvalEvent {
        sequence,
        step_index: None,
        kind: EvalEventKind::System,
        state: state.to_string(),
        name: Some(name.to_string()),
        summary: Some(summary.to_string()),
        input: input.map(EvalPayload::new),
        output: output.map(EvalPayload::new),
        error,
        occurred_at: now_seconds(),
    }
}

pub fn route_evidence(route: &AcceptedRoute) -> EvalJevRoute {
    EvalJevRoute {
        policy_version: route.policy_version.to_string(),
        intent_confidence_threshold: crate::jev_classifier::ACTION_CONFIDENCE_MIN,
        intent_margin_threshold: crate::jev_classifier::ACTION_MARGIN_MIN,
        model_confidence_threshold: crate::jev_classifier::ACTION_CONFIDENCE_MIN,
        model_margin_threshold: crate::jev_classifier::ACTION_MARGIN_MIN,
        intent_confidence: route.action_confidence,
        intent_probabilities: route.action_probabilities.clone(),
        answer_requested: route.answer_requested,
        answer_first: route.answer_first,
        model_ceiling: None,
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

pub fn accepted_run(
    thread_id: &str,
    session_id: &str,
    prompt: &str,
    request: &ClassifierRequest,
    route: &AcceptedRoute,
    starting_version: Option<(String, Option<String>)>,
) -> EvalRun {
    let (starting_version_id, starting_input_digest) =
        starting_version.map_or((None, None), |(id, digest)| (Some(id), digest));
    let started_at = now_seconds();
    let seed = EvalRunSeed {
        run_id: uuid::Uuid::new_v4().to_string(),
        thread_id: thread_id.to_string(),
        external_thread_id: session_id.to_string(),
        provider: "managed-mcp".into(),
        model: None,
        effort: None,
        prompt_version: "managed-mcp-jev-v1".into(),
        prompt: prompt.to_string(),
        starting_version_id,
        starting_input_digest,
        expected_red_rounds: 0,
        turn_intent: route.intent,
        answer_first_required: route.answer_first,
        jev_route: Some(route_evidence(route)),
    };
    let mut run = crate::llm_eval::build_codex_eval_run(
        seed,
        "",
        started_at,
        started_at,
        "running",
        None,
        None,
        Vec::new(),
        Vec::new(),
    );
    run.events = vec![
        system_event(
            1,
            "request",
            "admitted",
            "Managed request admitted",
            Some(json!({"prompt": prompt, "threadId": thread_id, "sessionId": session_id})),
            None,
            None,
        ),
        system_event(
            2,
            "jev",
            "accepted",
            "Jev selected managed request contract",
            serde_json::to_value(request).ok(),
            Some(json!({
                "intent": route.intent.as_str(),
                "answerFirst": route.answer_first,
                "actionConfidence": route.action_confidence,
                "actionProbabilities": route.action_probabilities,
                "modelReason": route.model_reason,
                "classifierInputTokens": route.classifier_input_tokens,
                "classifierOutputTokens": route.classifier_output_tokens,
            })),
            None,
        ),
        system_event(
            3,
            "delivery",
            "pending",
            "Managed prompt awaiting bound agent channel delivery",
            None,
            Some(json!({"sessionId": session_id})),
            None,
        ),
    ];
    run
}

pub fn failed_run(thread_id: &str, session_id: &str, prompt: &str, error: &AppError) -> EvalRun {
    let now = now_seconds();
    EvalRun {
        schema_version: crate::llm_eval::EVAL_SCHEMA_VERSION,
        run_id: uuid::Uuid::new_v4().to_string(),
        case: EvalCase {
            case_id: crate::llm_eval::case_id(prompt, None),
            objective: prompt.to_string(),
            acceptance_criteria: vec!["Jev routes before managed agent delivery".into()],
            starting_version_id: None,
            starting_input_digest: None,
            expected_red_rounds: 0,
        },
        thread_id: thread_id.to_string(),
        external_thread_id: session_id.to_string(),
        turn_id: String::new(),
        route: EvalRoute {
            provider: "managed-mcp".into(),
            model: None,
            effort: None,
            prompt_version: "managed-mcp-jev-v1".into(),
            jev: None,
        },
        prompt: prompt.to_string(),
        started_at: now,
        completed_at: now,
        status: "failed_pre_dispatch".into(),
        response: None,
        raw_error: Some(error.message.clone()),
        turn_policy: None,
        policy_violations: Vec::new(),
        events: vec![
            system_event(
                1,
                "request",
                "admitted",
                "Managed request admitted",
                Some(json!({"prompt": prompt, "threadId": thread_id, "sessionId": session_id})),
                None,
                None,
            ),
            system_event(
                2,
                "jev",
                "error",
                "Jev classification failed before managed delivery",
                None,
                None,
                Some(error.message.clone()),
            ),
        ],
        versions: Vec::new(),
        usage: None,
    }
}

pub fn failed_delivery(mut run: EvalRun, error: &AppError) -> EvalRun {
    run.completed_at = now_seconds().max(run.started_at);
    run.status = "error".into();
    run.response = None;
    run.raw_error = Some(error.message.clone());
    if let Some(delivery) = run
        .events
        .iter_mut()
        .find(|event| event.name.as_deref() == Some("delivery"))
    {
        delivery.state = "error".into();
        delivery.summary = Some("Managed prompt was not delivered".into());
        delivery.error = Some(error.message.clone());
        delivery.occurred_at = now_seconds();
    }
    run.events.push(system_event(
        run.events.len() as u64 + 1,
        "terminal",
        "error",
        "Managed request failed before agent delivery",
        None,
        None,
        Some(error.message.clone()),
    ));
    run.policy_violations = crate::llm_eval::evaluate_policy_violations(&run);
    run
}

pub fn persist_to(root: &Path, run: &EvalRun) -> AppResult<()> {
    crate::llm_eval::persist_run(root, run)
        .map(|_| ())
        .map_err(AppError::persistence)
}

pub fn persist_if_app_handle(state: &AppState, run: &EvalRun) -> AppResult<()> {
    let handle = state.app_handle.lock().unwrap().clone();
    if let Some(handle) = handle {
        persist_to(&PathResolver::app_data_dir(&handle), run)?;
    }
    Ok(())
}

pub async fn mark_delivered(state: &AppState, session_id: &str) -> AppResult<()> {
    let mut runs = state.managed_jev_runs.lock().await;
    let Some(run) = runs.get_mut(session_id) else {
        return Ok(());
    };
    if let Some(delivery) = run
        .events
        .iter_mut()
        .find(|event| event.name.as_deref() == Some("delivery"))
    {
        delivery.state = "done".into();
        delivery.summary = Some("Managed prompt delivered to bound agent channel".into());
        delivery.occurred_at = now_seconds();
    }
    persist_if_app_handle(state, run)
}

pub async fn tool_started(
    state: &AppState,
    session_id: &str,
    tool_name: &str,
    arguments: Option<serde_json::Value>,
) -> Option<String> {
    let mut runs = state.managed_jev_runs.lock().await;
    let run = runs.get_mut(session_id)?;
    let run_id = run.run_id.clone();
    run.events.push(EvalEvent {
        sequence: run.events.len() as u64 + 1,
        step_index: None,
        kind: EvalEventKind::Tool,
        state: "started".into(),
        name: Some(tool_name.into()),
        summary: Some(format!("Managed MCP tool {tool_name} started")),
        input: arguments.map(EvalPayload::new),
        output: None,
        error: None,
        occurred_at: now_seconds(),
    });
    if let Err(error) = persist_if_app_handle(state, run) {
        state.push_log(format!(
            "[MCP] Failed to persist managed Jev tool start: {}",
            error.message
        ));
    }
    Some(run_id)
}

pub async fn tool_finished(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    tool_name: &str,
    result: &Result<serde_json::Value, AppError>,
) {
    let mut runs = state.managed_jev_runs.lock().await;
    let Some(run) = runs.get_mut(session_id) else {
        return;
    };
    if run.run_id != run_id {
        return;
    }
    run.events.push(EvalEvent {
        sequence: run.events.len() as u64 + 1,
        step_index: None,
        kind: EvalEventKind::Result,
        state: if result.is_ok() { "done" } else { "error" }.into(),
        name: Some(tool_name.into()),
        summary: Some(format!("Managed MCP tool {tool_name} finished")),
        input: None,
        output: result.as_ref().ok().cloned().map(EvalPayload::new),
        error: result.as_ref().err().map(|error| error.message.clone()),
        occurred_at: now_seconds(),
    });
    if let Err(error) = persist_if_app_handle(state, run) {
        state.push_log(format!(
            "[MCP] Failed to persist managed Jev tool result: {}",
            error.message
        ));
    }
}

pub async fn finish_run(
    state: &AppState,
    session_id: &str,
    response: &str,
    fatal: bool,
) -> AppResult<Option<EvalRun>> {
    let mut run = match state.managed_jev_runs.lock().await.remove(session_id) {
        Some(run) => run,
        None => return Ok(None),
    };
    run.completed_at = now_seconds().max(run.started_at);
    run.status = if fatal { "error" } else { "success" }.into();
    run.response = (!fatal).then(|| response.to_string());
    run.raw_error = fatal.then(|| response.to_string());
    run.events.push(system_event(
        run.events.len() as u64 + 1,
        "terminal",
        &run.status,
        if fatal {
            "Managed request ended without a successful reply"
        } else {
            "Managed agent returned a user-facing reply"
        },
        None,
        (!fatal).then(|| json!({"response": response})),
        run.raw_error.clone(),
    ));
    run.policy_violations = crate::llm_eval::evaluate_policy_violations(&run);
    if let Err(error) = persist_if_app_handle(state, &run) {
        state
            .managed_jev_runs
            .lock()
            .await
            .insert(session_id.to_string(), run);
        return Err(error);
    }
    Ok(Some(run))
}
