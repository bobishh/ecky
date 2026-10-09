use crate::contracts::{
    AppError, AppResult, Attachment, AttachmentKind, CodexDialogueMessage, CodexMessagePage,
    CodexTakeoverRuntime, CodexThreadSummary, ProviderTurnTrace,
};
use crate::services::provider_executable::resolve_codex_executable;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

const DEFAULT_CODEX_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CODEX_TRANSCRIPT_PAGE_SIZE: u32 = 30;
const STDERR_TAIL_LINES: usize = 80;
const TERMINAL_TURN_MEMORY: usize = 256;
const CODEX_EVAL_EVENT_LIMIT: usize = 4_096;
const LIVE_MESSAGE_LIMIT: usize = 256;
const LIVE_MESSAGE_CHAR_LIMIT: usize = 16_384;
const TURN_TRACE_LIMIT: usize = 24;

fn codex_now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
#[derive(Clone)]
pub struct CodexAppServerSupervisor {
    inner: Arc<SupervisorInner>,
}

struct SupervisorInner {
    state: Mutex<SupervisorState>,
    startup: Mutex<()>,
    resume: Mutex<()>,
    hook_descriptor_path: std::sync::Mutex<std::path::PathBuf>,
}

struct RealtimeSession {
    session_id: String,
    negotiation: Option<oneshot::Sender<AppResult<String>>>,
}

struct SupervisorState {
    realtime_sessions: HashMap<String, RealtimeSession>,
    canceled_realtime_sessions: VecDeque<String>,
    process: Option<SupervisorProcess>,
    codex_executable_path: Option<std::path::PathBuf>,
    generation: u64,
    next_request_id: u64,
    pending: HashMap<u64, oneshot::Sender<AppResult<Value>>>,
    stderr_tail: VecDeque<String>,
    runtimes: HashMap<String, CodexTakeoverRuntime>,
    live_messages: HashMap<String, Vec<CodexDialogueMessage>>,
    turn_traces: HashMap<String, Vec<ProviderTurnTrace>>,
    resumed_threads: HashMap<String, u64>,
    thread_model_providers: HashMap<String, String>,
    routed_config_baselines: HashMap<String, Value>,
    routed_delivery_pending: HashSet<String>,
    terminal_turns: HashSet<String>,
    terminal_turn_order: VecDeque<String>,
    pending_eval_seeds: HashMap<String, PendingCodexEval>,
    active_eval_runs: HashMap<String, ActiveCodexEval>,
    completed_eval_runs: VecDeque<crate::llm_eval::EvalRun>,
    reported_eval_persistence_errors: HashSet<String>,
    app_handle: Option<tauri::AppHandle>,
}

struct PendingCodexEval {
    seed: crate::llm_eval::EvalRunSeed,
    started_at: i64,
    redaction_secret: Option<String>,
}

struct ActiveCodexEval {
    seed: crate::llm_eval::EvalRunSeed,
    redaction_secret: Option<String>,
    turn_id: String,
    started_at: i64,
    events: Vec<crate::llm_eval::EvalEvent>,
    invocation_steps: HashMap<String, i64>,
    seen_invocation_states: HashSet<String>,
    assistant_message_items: HashSet<String>,
    next_step_index: i64,
    omitted_reasons: HashMap<&'static str, u64>,
}

#[derive(Clone)]
struct SupervisorProcess {
    generation: u64,
    stdin: Arc<Mutex<ChildStdin>>,
    kill: mpsc::Sender<()>,
    initialized: bool,
}

impl Default for CodexAppServerSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexAppServerSupervisor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SupervisorInner {
                state: Mutex::new(SupervisorState {
                    realtime_sessions: HashMap::new(),
                    canceled_realtime_sessions: VecDeque::new(),
                    process: None,
                    codex_executable_path: None,
                    generation: 0,
                    next_request_id: 1,
                    pending: HashMap::new(),
                    stderr_tail: VecDeque::new(),
                    runtimes: HashMap::new(),
                    live_messages: HashMap::new(),
                    turn_traces: HashMap::new(),
                    resumed_threads: HashMap::new(),
                    thread_model_providers: HashMap::new(),
                    routed_config_baselines: HashMap::new(),
                    routed_delivery_pending: HashSet::new(),
                    terminal_turns: HashSet::new(),
                    terminal_turn_order: VecDeque::new(),
                    pending_eval_seeds: HashMap::new(),
                    active_eval_runs: HashMap::new(),
                    completed_eval_runs: VecDeque::new(),
                    reported_eval_persistence_errors: HashSet::new(),
                    app_handle: None,
                }),
                startup: Mutex::new(()),
                resume: Mutex::new(()),
                hook_descriptor_path: std::sync::Mutex::new(
                    crate::services::codex_pre_tool_hook::runtime_descriptor_path(),
                ),
            }),
        }
    }

    fn hook_descriptor_path(&self) -> std::path::PathBuf {
        self.inner
            .hook_descriptor_path
            .lock()
            .map(|path| path.clone())
            .unwrap_or_else(|_| crate::services::codex_pre_tool_hook::runtime_descriptor_path())
    }

    #[cfg(test)]
    pub fn set_hook_descriptor_path_for_test(&self, path: std::path::PathBuf) {
        *self
            .inner
            .hook_descriptor_path
            .lock()
            .expect("hook descriptor lock") = path;
    }

    pub async fn set_app_handle(&self, app_handle: tauri::AppHandle) {
        self.inner.state.lock().await.app_handle = Some(app_handle);
    }

    pub async fn runtime(&self, thread_id: &str) -> CodexTakeoverRuntime {
        self.inner
            .state
            .lock()
            .await
            .runtimes
            .get(thread_id)
            .cloned()
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub async fn set_runtime_for_test(&self, thread_id: &str, runtime: CodexTakeoverRuntime) {
        self.inner
            .state
            .lock()
            .await
            .runtimes
            .insert(thread_id.to_string(), runtime);
    }

    #[cfg(test)]
    pub async fn set_live_messages_for_test(
        &self,
        thread_id: &str,
        messages: Vec<CodexDialogueMessage>,
    ) {
        self.inner
            .state
            .lock()
            .await
            .live_messages
            .insert(thread_id.to_string(), messages);
    }

    pub async fn live_messages(&self, thread_id: &str) -> Vec<CodexDialogueMessage> {
        self.inner
            .state
            .lock()
            .await
            .live_messages
            .get(thread_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn thread_model_provider(&self, thread_id: &str) -> Option<String> {
        self.inner
            .state
            .lock()
            .await
            .thread_model_providers
            .get(thread_id)
            .cloned()
    }

    pub async fn account_type(&self) -> AppResult<String> {
        let result = self.request("account/read", json!({})).await?;
        result
            .get("account")
            .and_then(|account| account.get("type"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| AppError::parse("Codex account/read result missed account.type."))
    }

    pub async fn turn_traces(&self, thread_id: &str) -> Vec<ProviderTurnTrace> {
        self.inner
            .state
            .lock()
            .await
            .turn_traces
            .get(thread_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn take_completed_eval_runs(&self) -> Vec<crate::llm_eval::EvalRun> {
        self.inner
            .state
            .lock()
            .await
            .completed_eval_runs
            .drain(..)
            .collect()
    }

    pub async fn completed_eval_runs(&self) -> Vec<crate::llm_eval::EvalRun> {
        self.inner
            .state
            .lock()
            .await
            .completed_eval_runs
            .iter()
            .cloned()
            .collect()
    }

    pub async fn model_for_turn(&self, thread_id: &str, turn_id: &str) -> Option<String> {
        let state = self.inner.state.lock().await;
        let key = terminal_turn_key(thread_id, turn_id);
        state
            .active_eval_runs
            .get(&key)
            .and_then(|capture| capture.seed.model.clone())
            .or_else(|| {
                state
                    .completed_eval_runs
                    .iter()
                    .find(|run| run.external_thread_id == thread_id && run.turn_id == turn_id)
                    .and_then(|run| run.route.model.clone())
            })
    }

    #[cfg(test)]
    pub async fn push_completed_eval_run_for_test(&self, run: crate::llm_eval::EvalRun) {
        self.inner
            .state
            .lock()
            .await
            .completed_eval_runs
            .push_back(run);
    }

    pub async fn record_pre_dispatch_eval_failure(
        &self,
        seed: crate::llm_eval::EvalRunSeed,
        redaction_secret: Option<&str>,
        diagnostic: &str,
        events: Vec<crate::llm_eval::EvalEvent>,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let mut capture = ActiveCodexEval {
            seed,
            redaction_secret: None,
            turn_id: String::new(),
            started_at: now,
            events,
            invocation_steps: HashMap::new(),
            seen_invocation_states: HashSet::new(),
            assistant_message_items: HashSet::new(),
            next_step_index: 0,
            omitted_reasons: HashMap::new(),
        };
        let mut raw_error = Some(diagnostic.to_string());
        let mut terminal_output = None;
        if let Some(secret) = redaction_secret {
            redact_active_eval_secret(&mut capture, &mut raw_error, &mut terminal_output, secret);
        }
        let run = crate::llm_eval::build_codex_eval_run(
            capture.seed,
            "",
            now,
            now,
            "failed_pre_dispatch",
            None,
            raw_error,
            capture.events,
            Vec::new(),
        );
        self.inner
            .state
            .lock()
            .await
            .completed_eval_runs
            .push_back(run);
    }

    pub async fn append_steer_eval_events(
        &self,
        ecky_thread_id: &str,
        thread_id: &str,
        turn_id: &str,
        mut events: Vec<crate::llm_eval::EvalEvent>,
        secret: Option<&str>,
    ) {
        let now = codex_now_seconds();
        let mut state = self.inner.state.lock().await;
        let key = terminal_turn_key(thread_id, turn_id);
        if let Some(capture) = state.active_eval_runs.get_mut(&key) {
            for mut event in events.drain(..) {
                if let Some(secret) = secret.filter(|secret| !secret.is_empty()) {
                    redact_steer_event(&mut event, secret);
                }
                append_codex_eval_event(capture, event);
            }
            return;
        }
        if let Some(run) = state
            .completed_eval_runs
            .iter_mut()
            .find(|run| run.external_thread_id == thread_id && run.turn_id == turn_id)
        {
            for mut event in events.drain(..) {
                if let Some(secret) = secret.filter(|secret| !secret.is_empty()) {
                    redact_steer_event(&mut event, secret);
                }
                event.sequence = run.events.len() as u64 + 1;
                run.events.push(event);
            }
            run.completed_at = now;
            return;
        }
        // Keep an exact-turn trace if the provider capture was unavailable.
        let mut seed = crate::llm_eval::EvalRunSeed {
            run_id: uuid::Uuid::new_v4().to_string(),
            thread_id: ecky_thread_id.to_string(),
            external_thread_id: thread_id.to_string(),
            provider: crate::services::codex_takeover::CODEX_PROVIDER_ID.into(),
            model: None,
            effort: None,
            prompt_version: format!(
                "codex-steer-{}",
                crate::jev_classifier::CLASSIFIER_POLICY_VERSION
            ),
            prompt: String::new(),
            starting_version_id: None,
            starting_input_digest: None,
            expected_red_rounds: 0,
            turn_intent: crate::provider_turn::ProviderTurnIntent::Answer,
            answer_first_required: false,
            jev_route: None,
        };
        let mut capture = ActiveCodexEval {
            seed: seed.clone(),
            redaction_secret: None,
            turn_id: turn_id.to_string(),
            started_at: now,
            events,
            invocation_steps: HashMap::new(),
            seen_invocation_states: HashSet::new(),
            assistant_message_items: HashSet::new(),
            next_step_index: 0,
            omitted_reasons: HashMap::new(),
        };
        let mut raw_error = None;
        let mut terminal_output = None;
        if let Some(secret) = secret {
            redact_active_eval_secret(&mut capture, &mut raw_error, &mut terminal_output, secret);
        }
        seed = capture.seed;
        let run = crate::llm_eval::build_codex_eval_run(
            seed,
            turn_id,
            now,
            now,
            "steer_trace",
            None,
            None,
            capture.events,
            Vec::new(),
        );
        state.completed_eval_runs.push_back(run);
    }

    pub async fn acknowledge_eval_run(&self, run_id: &str) {
        let mut state = self.inner.state.lock().await;
        state.completed_eval_runs.retain(|run| run.run_id != run_id);
        state.reported_eval_persistence_errors.remove(run_id);
    }

    pub async fn should_report_eval_persistence_error(&self, run_id: &str) -> bool {
        self.inner
            .state
            .lock()
            .await
            .reported_eval_persistence_errors
            .insert(run_id.to_string())
    }

    pub async fn record_external_turn_started(&self, thread_id: &str, turn_id: &str) {
        let mut state = self.inner.state.lock().await;
        let already_terminal = state
            .terminal_turns
            .contains(&terminal_turn_key(thread_id, turn_id));
        let runtime = state.runtimes.entry(thread_id.to_string()).or_default();
        apply_start_response(runtime, turn_id, already_terminal);
        if !already_terminal {
            state.live_messages.remove(thread_id);
        }
    }

    pub async fn reconcile_runtime(&self, thread_id: &str) -> AppResult<CodexTakeoverRuntime> {
        let result = self
            .request(
                "thread/turns/list",
                json!({
                    "threadId": thread_id,
                    "limit": 1,
                    "sortDirection": "desc",
                    "itemsView": "notLoaded"
                }),
            )
            .await?;
        let runtime = runtime_from_turn_page(&result)?;
        self.inner
            .state
            .lock()
            .await
            .runtimes
            .insert(thread_id.to_string(), runtime.clone());
        Ok(runtime)
    }

    async fn ensure_started(&self) -> AppResult<()> {
        let _startup = self.inner.startup.lock().await;
        {
            let state = self.inner.state.lock().await;
            if state
                .process
                .as_ref()
                .is_some_and(|process| process.initialized)
            {
                return Ok(());
            }
        }

        let resolved = resolve_codex_executable()?;
        let mut command = Command::new(&resolved.path);
        command.arg("app-server").arg("--stdio");
        let mut child = command
            .env("PATH", &resolved.spawn_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                AppError::provider(format!(
                    "Failed to start Codex app-server using '{}': {error}",
                    resolved.path.display()
                ))
            })?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or_else(|| {
            AppError::provider("Codex app-server did not expose stdin.")
        })?));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::provider("Codex app-server did not expose stdout."))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::provider("Codex app-server did not expose stderr."))?;

        let (kill, mut kill_request) = mpsc::channel(1);
        let generation = {
            let mut state = self.inner.state.lock().await;
            state.generation = state.generation.wrapping_add(1).max(1);
            state.stderr_tail.clear();
            let generation = state.generation;
            state.process = Some(SupervisorProcess {
                generation,
                stdin: stdin.clone(),
                kill,
                initialized: false,
            });
            state.codex_executable_path = Some(resolved.path.clone());
            generation
        };

        let reader_supervisor = self.clone();
        tokio::spawn(async move {
            reader_supervisor.read_stdout(stdout, generation).await;
        });
        let stderr_supervisor = self.clone();
        tokio::spawn(async move {
            stderr_supervisor.read_stderr(stderr, generation).await;
        });
        let wait_supervisor = self.clone();
        tokio::spawn(async move {
            let status = tokio::select! {
                status = child.wait() => status,
                _ = kill_request.recv() => {
                    let _ = child.kill().await;
                    child.wait().await
                }
            };
            wait_supervisor
                .handle_process_exit(generation, status)
                .await;
        });

        let initialize = self
            .request_started(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": "ecky",
                        "title": "Ecky CAD",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": { "experimentalApi": true }
                }),
            )
            .await;
        if let Err(error) = initialize {
            self.invalidate_process(generation, error.clone()).await;
            return Err(error);
        }
        self.notify_started("initialized", json!({})).await?;
        let mut state = self.inner.state.lock().await;
        if let Some(process) = state
            .process
            .as_mut()
            .filter(|process| process.generation == generation)
        {
            process.initialized = true;
            return Ok(());
        }
        Err(AppError::provider(
            "Codex app-server exited during initialization.",
        ))
    }

    pub async fn request(&self, method: &str, params: Value) -> AppResult<Value> {
        self.ensure_started().await?;
        self.request_started(method, params).await
    }

    async fn request_started(&self, method: &str, params: Value) -> AppResult<Value> {
        let request_timeout = codex_request_timeout();
        let (id, receiver, stdin, generation) = {
            let mut state = self.inner.state.lock().await;
            let process = state
                .process
                .as_ref()
                .ok_or_else(|| AppError::provider("Codex app-server is not running."))?;
            let stdin = process.stdin.clone();
            let generation = process.generation;
            let id = state.next_request_id;
            state.next_request_id = state.next_request_id.wrapping_add(1).max(1);
            let (sender, receiver) = oneshot::channel();
            state.pending.insert(id, sender);
            (id, receiver, stdin, generation)
        };
        let payload = json!({ "method": method, "id": id, "params": params });
        if let Err(error) = write_json_line(&stdin, &payload).await {
            self.inner.state.lock().await.pending.remove(&id);
            self.invalidate_process(generation, error.clone()).await;
            return Err(error);
        }
        match tokio::time::timeout(request_timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                let error = AppError::provider(format!(
                    "Codex app-server dropped response channel for {method}."
                ));
                self.invalidate_process(generation, error.clone()).await;
                Err(error)
            }
            Err(_) => {
                self.inner.state.lock().await.pending.remove(&id);
                let error = AppError::provider(format!(
                    "Codex app-server request {method} timed out after {} seconds.",
                    request_timeout.as_secs_f64()
                ));
                self.invalidate_process(generation, error.clone()).await;
                Err(error)
            }
        }
    }

    async fn notify_started(&self, method: &str, params: Value) -> AppResult<()> {
        let stdin = self
            .inner
            .state
            .lock()
            .await
            .process
            .as_ref()
            .map(|process| process.stdin.clone())
            .ok_or_else(|| AppError::provider("Codex app-server is not running."))?;
        write_json_line(&stdin, &json!({ "method": method, "params": params })).await
    }

    async fn read_stdout(&self, stdout: tokio::process::ChildStdout, generation: u64) {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => self.handle_stdout_line(generation, &line).await,
                Ok(None) => break,
                Err(error) => {
                    self.invalidate_process(
                        generation,
                        AppError::provider(format!(
                            "Failed reading Codex app-server stdout: {error}"
                        )),
                    )
                    .await;
                    break;
                }
            }
        }
    }

    async fn read_stderr(&self, stderr: tokio::process::ChildStderr, generation: u64) {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut state = self.inner.state.lock().await;
            if state
                .process
                .as_ref()
                .is_none_or(|process| process.generation != generation)
            {
                break;
            }
            state.stderr_tail.push_back(line);
            while state.stderr_tail.len() > STDERR_TAIL_LINES {
                state.stderr_tail.pop_front();
            }
        }
    }

    async fn handle_stdout_line(&self, generation: u64, line: &str) {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(error) => {
                self.invalidate_process(
                    generation,
                    AppError::provider(format!(
                        "Codex app-server returned malformed JSON: {error}. Raw line: {line}"
                    )),
                )
                .await;
                return;
            }
        };
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            if message.get("method").is_some() {
                let stdin = self
                    .inner
                    .state
                    .lock()
                    .await
                    .process
                    .as_ref()
                    .map(|process| process.stdin.clone());
                if let Some(stdin) = stdin {
                    let _ = write_json_line(
                        &stdin,
                        &json!({
                            "id": id,
                            "error": {
                                "code": -32601,
                                "message": "Ecky provider integration does not support interactive app-server requests."
                            }
                        }),
                    )
                    .await;
                }
                return;
            }
            let sender = self.inner.state.lock().await.pending.remove(&id);
            if let Some(sender) = sender {
                let result = response_result_for_id(line, id).and_then(|result| {
                    result.ok_or_else(|| AppError::provider("Missing response result."))
                });
                let _ = sender.send(result);
            }
            return;
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return;
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let thread_id = params
            .get("threadId")
            .and_then(Value::as_str)
            .map(str::to_string);
        if method.starts_with("thread/realtime/") {
            let (app_handle, session_id) = {
                let mut state = self.inner.state.lock().await;
                if state
                    .process
                    .as_ref()
                    .is_none_or(|process| process.generation != generation)
                {
                    return;
                }
                if let Some(thread_id) = thread_id.as_deref() {
                    if let Some(session) = state.realtime_sessions.get_mut(thread_id) {
                        let negotiated = match method {
                            "thread/realtime/sdp" => params
                                .get("sdp")
                                .and_then(Value::as_str)
                                .map(|sdp| Ok(sdp.to_string())),
                            "thread/realtime/error" => Some(Err(AppError::provider(
                                params
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("Codex realtime error")
                                    .to_string(),
                            ))),
                            "thread/realtime/closed" => Some(Err(AppError::provider(
                                params
                                    .get("reason")
                                    .and_then(Value::as_str)
                                    .unwrap_or("Codex realtime closed before audio connected")
                                    .to_string(),
                            ))),
                            _ => None,
                        };
                        if let Some(result) = negotiated {
                            if let Some(sender) = session.negotiation.take() {
                                let _ = sender.send(result);
                            }
                        }
                    }
                }
                let session_id = thread_id
                    .as_deref()
                    .and_then(|id| state.realtime_sessions.get(id))
                    .map(|session| session.session_id.clone());
                (state.app_handle.clone(), session_id)
            };
            if let (Some(app), Some(thread_id)) = (app_handle, thread_id.as_deref()) {
                if method == "thread/realtime/item/completed" {
                    if let Some(message) = params.get("item").and_then(|item| {
                        project_realtime_transcript(thread_id, item, codex_now_seconds())
                    }) {
                        if let Err(error) =
                            crate::commands::codex_takeover::persist_realtime_message(
                                &app, thread_id, message,
                            )
                            .await
                        {
                            let _ = app.emit("codex-voice-event", json!({"threadId": thread_id, "sessionId": session_id, "method": "thread/realtime/error", "params": {"message": error.message}}));
                        }
                    }
                }
                let _ = app.emit(
                    "codex-voice-event",
                    json!({"threadId": thread_id, "sessionId": session_id, "method": method, "params": params}),
                );
            }
            return;
        }
        let (app_handle, live_messages, turn_traces, runtime_snapshot) = {
            let mut state = self.inner.state.lock().await;
            if state
                .process
                .as_ref()
                .is_none_or(|process| process.generation != generation)
            {
                return;
            }
            let (live_messages, turn_traces, runtime_snapshot) = if let Some(thread_id) =
                thread_id.as_deref()
            {
                if method == "turn/completed" {
                    if let Some(turn_id) = params
                        .get("turn")
                        .and_then(|turn| turn.get("id"))
                        .and_then(Value::as_str)
                    {
                        remember_terminal_turn(&mut state, thread_id, turn_id);
                    }
                }
                let runtime = state.runtimes.entry(thread_id.to_string()).or_default();
                apply_runtime_notification(runtime, method, &params);
                let runtime_snapshot = runtime.clone();
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let terminal_trace = {
                    let live_messages = state
                        .live_messages
                        .entry(thread_id.to_string())
                        .or_default();
                    apply_live_notification(live_messages, thread_id, method, &params, now);
                    if method == "turn/completed" {
                        let turn = params.get("turn").unwrap_or(&params);
                        turn.get("id").and_then(Value::as_str).and_then(|turn_id| {
                            take_terminal_trace(
                                live_messages,
                                turn_id,
                                codex_turn_trace_status(&params),
                                now,
                            )
                        })
                    } else {
                        None
                    }
                };
                let terminal_eval_run =
                    project_codex_eval_notification(&mut state, thread_id, method, &params, now);
                if let Some(run) = terminal_eval_run {
                    state.completed_eval_runs.push_back(run);
                }
                if let Some(trace) = terminal_trace {
                    push_turn_trace(&mut state.turn_traces, thread_id, trace);
                }
                let live_messages = state
                    .live_messages
                    .get(thread_id)
                    .cloned()
                    .unwrap_or_default();
                let turn_traces = state
                    .turn_traces
                    .get(thread_id)
                    .cloned()
                    .unwrap_or_default();
                (live_messages, turn_traces, Some(runtime_snapshot))
            } else {
                (Vec::new(), Vec::new(), None)
            };
            (
                state.app_handle.clone(),
                live_messages,
                turn_traces,
                runtime_snapshot,
            )
        };
        if let (Some(app_handle), Some(thread_id)) = (app_handle, thread_id) {
            let _ = app_handle.emit(
                "codex-provider-updated",
                json!({
                    "threadId": thread_id,
                    "method": method,
                    "liveMessages": live_messages,
                    "turnTraces": turn_traces,
                    "runtime": runtime_snapshot,
                }),
            );
        }
        if method == "turn/completed"
            || method == "thread/status/changed"
                && params
                    .get("status")
                    .and_then(|status| status.get("type"))
                    .and_then(Value::as_str)
                    == Some("idle")
        {
            crate::services::codex_takeover::notify_queue_supervisor();
        }
    }

    async fn handle_process_exit(
        &self,
        generation: u64,
        status: std::io::Result<std::process::ExitStatus>,
    ) {
        let (tail, code) = {
            let state = self.inner.state.lock().await;
            (
                state
                    .stderr_tail
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
                status
                    .as_ref()
                    .ok()
                    .and_then(std::process::ExitStatus::code),
            )
        };
        let mut message = format!(
            "Codex app-server exited{}.",
            code.map(|code| format!(" with status {code}"))
                .unwrap_or_default()
        );
        if let Err(error) = status {
            message.push_str(&format!(" Wait error: {error}"));
        }
        if !tail.trim().is_empty() {
            message.push_str(&format!("\n{tail}"));
        }
        self.invalidate_process(generation, AppError::provider(message))
            .await;
    }

    async fn invalidate_process(&self, generation: u64, error: AppError) {
        let (pending, app_handle, thread_ids, voice_thread_ids, kill) = {
            let mut state = self.inner.state.lock().await;
            if state
                .process
                .as_ref()
                .is_none_or(|process| process.generation != generation)
            {
                return;
            }
            let kill = state.process.take().map(|process| process.kill);
            state.resumed_threads.clear();
            state.terminal_turns.clear();
            state.terminal_turn_order.clear();
            // Process loss ends any in-flight delivery window. Keep baselines
            // so the next normal resume can restore native settings.
            state.routed_delivery_pending.clear();
            let terminal_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let active_evals = std::mem::take(&mut state.active_eval_runs);
            for (_, capture) in active_evals {
                state
                    .completed_eval_runs
                    .push_back(finish_codex_eval_capture(
                        capture,
                        terminal_at,
                        "error",
                        Some(error.message.clone()),
                        None,
                    ));
            }
            let pending_evals = std::mem::take(&mut state.pending_eval_seeds);
            for (thread_id, pending) in pending_evals {
                let turn_id = state
                    .runtimes
                    .get(&thread_id)
                    .and_then(|runtime| runtime.active_turn_id.clone())
                    .unwrap_or_default();
                let capture = active_eval_from_pending(pending, turn_id);
                state
                    .completed_eval_runs
                    .push_back(finish_codex_eval_capture(
                        capture,
                        terminal_at,
                        "error",
                        Some(error.message.clone()),
                        None,
                    ));
            }
            let interrupted_live = std::mem::take(&mut state.live_messages);
            for (thread_id, mut messages) in interrupted_live {
                if messages.is_empty() {
                    continue;
                }
                mark_live_messages_terminal(&mut messages, "error");
                let turn_id = state
                    .runtimes
                    .get(&thread_id)
                    .and_then(|runtime| runtime.active_turn_id.clone())
                    .unwrap_or_default();
                push_turn_trace(
                    &mut state.turn_traces,
                    &thread_id,
                    ProviderTurnTrace {
                        turn_id,
                        status: "error".to_string(),
                        messages,
                        completed_at: terminal_at,
                    },
                );
            }
            let voice_thread_ids = state
                .realtime_sessions
                .iter()
                .map(|(id, session)| (id.clone(), session.session_id.clone()))
                .collect::<Vec<_>>();
            for (_, mut session) in state.realtime_sessions.drain() {
                if let Some(sender) = session.negotiation.take() {
                    let _ = sender.send(Err(error.clone()));
                }
            }
            let pending = state
                .pending
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>();
            let thread_ids = state.runtimes.keys().cloned().collect::<Vec<_>>();
            for runtime in state.runtimes.values_mut() {
                runtime.phase = "disconnected".to_string();
                runtime.active_turn_id = None;
                runtime.error = Some(error.message.clone());
            }
            (
                pending,
                state.app_handle.clone(),
                thread_ids,
                voice_thread_ids,
                kill,
            )
        };
        if let Some(kill) = kill {
            let _ = kill.send(()).await;
        }
        for sender in pending {
            let _ = sender.send(Err(error.clone()));
        }
        if let Some(app_handle) = app_handle {
            for (thread_id, session_id) in voice_thread_ids {
                let _ = app_handle.emit("codex-voice-event", json!({"threadId": thread_id, "sessionId": session_id, "method": "thread/realtime/error", "params": {"message": error.message}}));
            }
            for thread_id in thread_ids {
                let _ = app_handle.emit(
                    "codex-provider-updated",
                    json!({ "threadId": thread_id, "method": "process/exited" }),
                );
            }
        }
    }

    pub async fn start_thread(
        &self,
        ecky_thread_id: &str,
        project_title: &str,
        cwd: &str,
        mcp_endpoint: &str,
        handoff_context: &str,
        model: Option<&str>,
    ) -> AppResult<CodexThreadSummary> {
        let result = self
            .request(
                "thread/start",
                start_params(
                    ecky_thread_id,
                    project_title,
                    cwd,
                    mcp_endpoint,
                    handoff_context,
                    model,
                ),
            )
            .await?;
        let thread = result
            .get("thread")
            .ok_or_else(|| AppError::parse("Codex thread/start result is missing thread."))?;
        let summary = parse_thread_summary(thread)?;
        let generation = self
            .inner
            .state
            .lock()
            .await
            .process
            .as_ref()
            .map(|process| process.generation)
            .ok_or_else(|| AppError::provider("Codex app-server exited after thread/start."))?;
        let mut state = self.inner.state.lock().await;
        state
            .runtimes
            .insert(summary.id.clone(), CodexTakeoverRuntime::default());
        state
            .thread_model_providers
            .insert(summary.id.clone(), summary.model_provider.clone());
        state.live_messages.remove(&summary.id);
        state.resumed_threads.insert(summary.id.clone(), generation);
        Ok(summary)
    }

    pub async fn name_thread(&self, thread_id: &str, name: &str) -> AppResult<()> {
        self.request(
            "thread/name/set",
            json!({ "threadId": thread_id, "name": name }),
        )
        .await?;
        Ok(())
    }

    pub async fn cancel_realtime_session(&self, session_id: &str) {
        let mut state = self.inner.state.lock().await;
        if !state
            .canceled_realtime_sessions
            .iter()
            .any(|id| id == session_id)
        {
            state
                .canceled_realtime_sessions
                .push_back(session_id.to_string());
            while state.canceled_realtime_sessions.len() > TERMINAL_TURN_MEMORY {
                state.canceled_realtime_sessions.pop_front();
            }
        }
    }

    pub async fn realtime_session_canceled(&self, session_id: &str) -> bool {
        self.inner
            .state
            .lock()
            .await
            .canceled_realtime_sessions
            .iter()
            .any(|id| id == session_id)
    }

    pub async fn start_realtime(
        &self,
        thread_id: &str,
        session_id: &str,
        sdp: &str,
    ) -> AppResult<String> {
        if self.realtime_session_canceled(session_id).await {
            return Err(AppError::provider("Codex voice startup was canceled."));
        }
        self.ensure_started().await?;
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.inner.state.lock().await;
            if state
                .canceled_realtime_sessions
                .iter()
                .any(|id| id == session_id)
            {
                return Err(AppError::provider("Codex voice startup was canceled."));
            }
            if state.realtime_sessions.contains_key(thread_id) {
                return Err(AppError::validation(
                    "Codex voice conversation is already active or connecting.",
                ));
            }
            state.realtime_sessions.insert(
                thread_id.to_string(),
                RealtimeSession {
                    session_id: session_id.to_string(),
                    negotiation: Some(sender),
                },
            );
        }
        let result = async {
            self.request("thread/realtime/start", json!({
                "threadId": thread_id,
                "realtimeSessionId": session_id,
                "outputModality": "audio",
                // Native v3 selects FramelessBidi and OpenAI-Alpha: quicksilver=v2.
                // Omitting this defaults WebRTC to v1, which AVAS now rejects.
                "version": "v3",
                "transport": { "type": "webrtc", "sdp": sdp },
                "flushTranscriptTailOnSessionEnd": false,
                "realtimeStartInstructions": crate::provider_turn::ProviderTurnPolicy::unified_prompt_contract(),
            })).await?;
            tokio::time::timeout(Duration::from_secs(45), receiver).await
                .map_err(|_| AppError::provider("Codex realtime did not return its SDP answer within 45 seconds."))?
                .map_err(|_| AppError::provider("Codex voice negotiation was canceled."))?
        }.await;
        if result.is_err() {
            // Stop only this owned session. A stale startup cannot stop its successor.
            let _ = self.stop_realtime(thread_id, session_id).await;
        }
        result
    }

    pub async fn stop_realtime(&self, thread_id: &str, session_id: &str) -> AppResult<()> {
        {
            let mut state = self.inner.state.lock().await;
            let Some(session) = state.realtime_sessions.get_mut(thread_id) else {
                return Ok(());
            };
            if session.session_id != session_id {
                return Err(AppError::validation(
                    "Codex voice stop targets a stale session.",
                ));
            }
            if let Some(sender) = session.negotiation.take() {
                let _ = sender.send(Err(AppError::provider(
                    "Codex voice negotiation was canceled.",
                )));
            }
        }
        let result = self
            .request("thread/realtime/stop", json!({"threadId": thread_id}))
            .await
            .map(|_| ());
        let mut state = self.inner.state.lock().await;
        if state
            .realtime_sessions
            .get(thread_id)
            .is_some_and(|session| session.session_id == session_id)
        {
            state.realtime_sessions.remove(thread_id);
        }
        result
    }

    pub async fn list_models(&self) -> AppResult<Vec<String>> {
        let mut cursor = None;
        let mut models = Vec::new();
        let mut seen = HashSet::new();
        loop {
            let result = self
                .request(
                    "model/list",
                    json!({
                        "cursor": cursor,
                        "limit": 100,
                        "includeHidden": false,
                    }),
                )
                .await?;
            let (page, next_cursor) = parse_model_list_page(&result)?;
            for model in page {
                if seen.insert(model.clone()) {
                    models.push(model);
                }
            }
            let Some(next_cursor) = next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }
        if models.is_empty() {
            return Err(AppError::provider(
                "Codex app-server model/list returned no selectable models.",
            ));
        }
        Ok(models)
    }

    /// Read the effective native Codex hooks for the bound project directory.
    /// Jev dispatch uses this as a fail-closed trust check; it never writes
    /// trust state or bypasses Codex's review flow.
    pub async fn list_hooks(&self, cwd: &str) -> AppResult<Value> {
        self.request("hooks/list", json!({ "cwds": [cwd] })).await
    }

    async fn codex_feature_enablement(&self, thread_id: &str) -> AppResult<Value> {
        let required = [
            "shell_tool",
            "unified_exec",
            "multi_agent",
            "apps",
            "view_image",
        ];
        let mut cursor = None::<String>;
        let mut seen_cursors = HashSet::new();
        let mut values = serde_json::Map::new();
        loop {
            let page = self
                .request(
                    "experimentalFeature/list",
                    json!({"threadId": thread_id, "cursor": cursor, "limit": 100}),
                )
                .await?;
            for feature in page
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(name) = feature.get("name").and_then(Value::as_str) else {
                    continue;
                };
                if required.contains(&name) {
                    if let Some(enabled) = feature.get("enabled").and_then(Value::as_bool) {
                        values.insert(name.to_string(), Value::Bool(enabled));
                    }
                }
            }
            cursor = next_feature_cursor(&page, &mut seen_cursors)?;
            if cursor.is_none() {
                break;
            }
        }
        let missing = required
            .iter()
            .filter(|name| !values.contains_key(**name))
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(AppError::provider(format!(
                "Codex could not confirm current native-tool feature settings: {}. Restart Codex and retry.",
                missing.join(", ")
            )));
        }
        Ok(Value::Object(values))
    }

    pub async fn resume_with_routed_hook(
        &self,
        binding: &crate::contracts::CodexTakeoverBinding,
        project_title: &str,
        mcp_endpoint: &str,
        handoff_context: &str,
        model: Option<&str>,
        policy: crate::provider_turn::ProviderTurnPolicy,
        command: &str,
    ) -> AppResult<String> {
        let _resume = self.inner.resume.lock().await;
        let descriptor = crate::services::codex_pre_tool_hook::runtime_descriptor_at(
            &self.hook_descriptor_path(),
        )
        .map_err(|_| {
            AppError::provider(
                "Ecky routed turns require the loopback PreToolUse callback. Restart Ecky and retry.",
            )
        })?;
        let callback = descriptor
            .get("endpoint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !callback.starts_with("http://127.0.0.1:")
            || !callback.contains("/codex-pre-tool-hook/")
            || descriptor.get("token").and_then(Value::as_str).is_none()
        {
            return Err(AppError::provider(
                "Ecky routed turns require a valid loopback PreToolUse callback. Restart Ecky and retry.",
            ));
        }
        let params = resume_params_with_routed_hook(
            binding,
            project_title,
            mcp_endpoint,
            handoff_context,
            model,
            policy,
            command,
        );
        let selected_executable = self
            .inner
            .state
            .lock()
            .await
            .codex_executable_path
            .clone()
            .ok_or_else(|| AppError::provider("Codex executable selection is unavailable."))?;
        let review_diagnostic =
            codex_hook_review_diagnostic(&selected_executable, &binding.cwd, command)?;
        let effective = self
            .request(
                "config/read",
                json!({"cwd": binding.cwd, "includeLayers": false}),
            )
            .await?;
        let config = effective
            .get("config")
            .ok_or_else(|| AppError::parse("Codex config/read result is missing config."))?;
        let baseline_features = self
            .codex_feature_enablement(&binding.codex_thread_id)
            .await?;
        let baseline = native_config_baseline(config, &baseline_features)?;
        self.inner
            .state
            .lock()
            .await
            .routed_config_baselines
            .entry(binding.codex_thread_id.clone())
            .or_insert(baseline);
        let result = self.request_started("thread/resume", params).await?;
        let hooks = self.list_hooks(&binding.cwd).await?;
        let hash = verify_owned_pre_tool_hook(&hooks, &binding.cwd, command)
            .map_err(|_| AppError::provider(review_diagnostic.clone()))?;
        // Trust can change between registration and the queued turn. Read the
        // effective hook again immediately before returning control to dispatch.
        let hooks = self.list_hooks(&binding.cwd).await?;
        let current_hash = verify_owned_pre_tool_hook(&hooks, &binding.cwd, command)
            .map_err(|_| AppError::provider(review_diagnostic.clone()))?;
        if current_hash != hash {
            return Err(AppError::provider(review_diagnostic));
        }
        let thread = result
            .get("thread")
            .ok_or_else(|| AppError::parse("Codex thread/resume result is missing thread."))?;
        let provider = parse_thread_summary(thread)?.model_provider;
        let mut state = self.inner.state.lock().await;
        let generation = state
            .process
            .as_ref()
            .map(|process| process.generation)
            .ok_or_else(|| {
                AppError::provider("Codex app-server exited after hook registration.")
            })?;
        state
            .thread_model_providers
            .insert(binding.codex_thread_id.clone(), provider);
        state
            .resumed_threads
            .insert(binding.codex_thread_id.clone(), generation);
        state
            .routed_delivery_pending
            .insert(binding.codex_thread_id.clone());
        Ok(hash)
    }

    pub async fn delete_thread(&self, thread_id: &str) -> AppResult<()> {
        self.request("thread/delete", json!({ "threadId": thread_id }))
            .await?;
        let mut state = self.inner.state.lock().await;
        state.runtimes.remove(thread_id);
        state.resumed_threads.remove(thread_id);
        state.routed_delivery_pending.remove(thread_id);
        state.routed_config_baselines.remove(thread_id);
        Ok(())
    }

    pub async fn mark_routed_delivery_pending(&self, thread_id: &str) {
        self.inner
            .state
            .lock()
            .await
            .routed_delivery_pending
            .insert(thread_id.to_string());
    }

    pub async fn resume_thread(
        &self,
        binding: &crate::contracts::CodexTakeoverBinding,
        project_title: &str,
        mcp_endpoint: &str,
        handoff_context: &str,
        refresh_developer_instructions: bool,
        force_resume_request: bool,
        model: Option<&str>,
    ) -> AppResult<()> {
        self.resume_thread_with_policy(
            binding,
            project_title,
            mcp_endpoint,
            handoff_context,
            refresh_developer_instructions,
            force_resume_request,
            model,
            crate::provider_turn::ProviderTurnPolicy::for_intent(
                crate::provider_turn::ProviderTurnIntent::Modify,
            ),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn resume_thread_with_policy(
        &self,
        binding: &crate::contracts::CodexTakeoverBinding,
        project_title: &str,
        mcp_endpoint: &str,
        handoff_context: &str,
        refresh_developer_instructions: bool,
        force_resume_request: bool,
        model: Option<&str>,
        policy: crate::provider_turn::ProviderTurnPolicy,
    ) -> AppResult<()> {
        let _resume = self.inner.resume.lock().await;
        self.ensure_started().await?;
        let generation = {
            let state = self.inner.state.lock().await;
            let generation = state
                .process
                .as_ref()
                .map(|process| process.generation)
                .ok_or_else(|| AppError::provider("Codex app-server is not running."))?;
            if state
                .runtimes
                .get(&binding.codex_thread_id)
                .is_some_and(|runtime| runtime.active_turn_id.is_some())
                || routed_delivery_blocks_normal_resume(&state, &binding.codex_thread_id)
            {
                return Ok(());
            }
            if should_skip_resume(
                state.resumed_threads.get(&binding.codex_thread_id).copied(),
                generation,
                refresh_developer_instructions,
                force_resume_request
                    || state
                        .routed_config_baselines
                        .contains_key(&binding.codex_thread_id),
            ) {
                return Ok(());
            }
            generation
        };
        let restore_config = self
            .inner
            .state
            .lock()
            .await
            .routed_config_baselines
            .get(&binding.codex_thread_id)
            .cloned();
        let mut params = resume_params_with_policy(
            binding,
            project_title,
            mcp_endpoint,
            handoff_context,
            model,
            policy,
        );
        if let Some(config) = restore_config.as_ref() {
            merge_config_overrides(&mut params, config);
        }
        let result = self.request_started("thread/resume", params).await?;
        let thread = result
            .get("thread")
            .ok_or_else(|| AppError::parse("Codex thread/resume result is missing thread."))?;
        let model_provider = parse_thread_summary(thread)?.model_provider;
        let mut runtime = CodexTakeoverRuntime::default();
        if let Some(turn) = result
            .get("initialTurnsPage")
            .and_then(|page| page.get("data"))
            .and_then(Value::as_array)
            .and_then(|turns| {
                turns
                    .iter()
                    .rev()
                    .find(|turn| turn.get("status").and_then(Value::as_str) == Some("inProgress"))
            })
        {
            runtime.phase = "active".to_string();
            runtime.active_turn_id = turn.get("id").and_then(Value::as_str).map(str::to_string);
        }
        self.inner
            .state
            .lock()
            .await
            .runtimes
            .insert(binding.codex_thread_id.clone(), runtime);
        self.inner
            .state
            .lock()
            .await
            .thread_model_providers
            .insert(binding.codex_thread_id.clone(), model_provider);
        let mut state = self.inner.state.lock().await;
        if restore_config.is_some() {
            state
                .routed_config_baselines
                .remove(&binding.codex_thread_id);
        }
        if state
            .process
            .as_ref()
            .is_some_and(|process| process.generation == generation)
        {
            state
                .resumed_threads
                .insert(binding.codex_thread_id.clone(), generation);
        }
        Ok(())
    }

    pub async fn message_page(
        &self,
        thread_id: &str,
        cursor: Option<String>,
        direction: Option<&str>,
    ) -> AppResult<CodexMessagePage> {
        let sort_direction = if direction == Some("newer") {
            "asc"
        } else {
            "desc"
        };
        let result = self
            .request(
                "thread/turns/list",
                message_page_params(thread_id, cursor, direction),
            )
            .await?;
        let mut turns = result
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| AppError::parse("Codex thread/turns/list result is missing data."))?;
        if sort_direction == "desc" {
            turns.reverse();
        }
        Ok(CodexMessagePage {
            messages: project_turn_messages(thread_id, &turns),
            next_cursor: result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string),
            backwards_cursor: result
                .get("backwardsCursor")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    pub async fn start_turn(
        &self,
        thread_id: &str,
        prompt: &str,
        model: Option<&str>,
    ) -> AppResult<String> {
        self.start_turn_with_attachments(thread_id, prompt, model, &[])
            .await
    }

    pub async fn start_turn_with_attachments(
        &self,
        thread_id: &str,
        prompt: &str,
        model: Option<&str>,
        attachments: &[Attachment],
    ) -> AppResult<String> {
        self.start_turn_with_attachments_policy(
            thread_id,
            prompt,
            model,
            attachments,
            crate::provider_turn::ProviderTurnPolicy::for_intent(
                crate::provider_turn::ProviderTurnIntent::Modify,
            ),
        )
        .await
    }

    pub async fn start_turn_with_attachments_policy(
        &self,
        thread_id: &str,
        prompt: &str,
        model: Option<&str>,
        attachments: &[Attachment],
        policy: crate::provider_turn::ProviderTurnPolicy,
    ) -> AppResult<String> {
        self.start_turn_with_attachments_policy_and_eval(
            thread_id,
            prompt,
            model,
            attachments,
            policy,
            None,
            None,
        )
        .await
    }

    pub async fn start_turn_with_eval_seed(
        &self,
        thread_id: &str,
        prompt: &str,
        model: Option<&str>,
        attachments: &[Attachment],
        policy: crate::provider_turn::ProviderTurnPolicy,
        seed: crate::llm_eval::EvalRunSeed,
    ) -> AppResult<String> {
        self.start_turn_with_eval_seed_and_secret(
            thread_id,
            prompt,
            model,
            attachments,
            policy,
            seed,
            None,
        )
        .await
    }

    pub async fn start_turn_with_eval_seed_and_secret(
        &self,
        thread_id: &str,
        prompt: &str,
        model: Option<&str>,
        attachments: &[Attachment],
        policy: crate::provider_turn::ProviderTurnPolicy,
        seed: crate::llm_eval::EvalRunSeed,
        redaction_secret: Option<&str>,
    ) -> AppResult<String> {
        self.start_turn_with_attachments_policy_and_eval(
            thread_id,
            prompt,
            model,
            attachments,
            policy,
            Some(seed),
            redaction_secret,
        )
        .await
    }

    async fn start_turn_with_attachments_policy_and_eval(
        &self,
        thread_id: &str,
        prompt: &str,
        model: Option<&str>,
        attachments: &[Attachment],
        policy: crate::provider_turn::ProviderTurnPolicy,
        eval_seed: Option<crate::llm_eval::EvalRunSeed>,
        redaction_secret: Option<&str>,
    ) -> AppResult<String> {
        let params = turn_start_params(thread_id, prompt, model, attachments, policy);
        if let Some(seed) = eval_seed {
            self.inner.state.lock().await.pending_eval_seeds.insert(
                thread_id.to_string(),
                PendingCodexEval {
                    seed,
                    started_at: codex_now_seconds(),
                    redaction_secret: redaction_secret
                        .filter(|secret| !secret.is_empty())
                        .map(str::to_owned),
                },
            );
        }
        let result = match self.request("turn/start", params).await {
            Ok(result) => result,
            Err(error) => {
                let mut state = self.inner.state.lock().await;
                state.routed_delivery_pending.remove(thread_id);
                if let Some(pending) = state.pending_eval_seeds.remove(thread_id) {
                    let completed_at = codex_now_seconds();
                    state
                        .completed_eval_runs
                        .push_back(finish_pending_codex_eval(
                            pending,
                            String::new(),
                            completed_at,
                            "error",
                            Some(error.message.clone()),
                        ));
                }
                return Err(error);
            }
        };
        let Some(turn_id) = result
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            let error = AppError::parse("Codex turn/start result is missing turn id.");
            let mut state = self.inner.state.lock().await;
            state.routed_delivery_pending.remove(thread_id);
            if let Some(pending) = state.pending_eval_seeds.remove(thread_id) {
                let completed_at = codex_now_seconds();
                state
                    .completed_eval_runs
                    .push_back(finish_pending_codex_eval(
                        pending,
                        String::new(),
                        completed_at,
                        "error",
                        Some(error.message.clone()),
                    ));
            }
            return Err(error);
        };
        let mut state = self.inner.state.lock().await;
        state.routed_delivery_pending.remove(thread_id);
        ensure_codex_eval_capture(&mut state, thread_id, &turn_id);
        if let Some(turn) = result.get("turn") {
            apply_codex_reported_route(&mut state, thread_id, &turn_id, turn);
        }
        let already_terminal = state
            .terminal_turns
            .contains(&terminal_turn_key(thread_id, &turn_id));
        let runtime = state.runtimes.entry(thread_id.to_string()).or_default();
        apply_start_response(runtime, &turn_id, already_terminal);
        if !already_terminal {
            state.live_messages.remove(thread_id);
        }
        Ok(turn_id)
    }

    pub async fn steer_turn(
        &self,
        thread_id: &str,
        expected_turn_id: &str,
        prompt: &str,
    ) -> AppResult<()> {
        self.steer_turn_with_attachments(thread_id, expected_turn_id, prompt, &[])
            .await
    }

    pub async fn steer_turn_with_attachments(
        &self,
        thread_id: &str,
        expected_turn_id: &str,
        prompt: &str,
        attachments: &[Attachment],
    ) -> AppResult<()> {
        self.steer_turn_with_attachments_and_policy(
            thread_id,
            expected_turn_id,
            prompt,
            attachments,
            crate::provider_turn::ProviderTurnPolicy::prompt_based(),
        )
        .await
    }

    pub async fn steer_turn_with_attachments_and_policy(
        &self,
        thread_id: &str,
        expected_turn_id: &str,
        prompt: &str,
        attachments: &[Attachment],
        policy: crate::provider_turn::ProviderTurnPolicy,
    ) -> AppResult<()> {
        let wrapped = policy.wrap_user_message_for_turn(prompt, &uuid::Uuid::new_v4().to_string());
        self.request(
            "turn/steer",
            json!({
                "threadId": thread_id,
                "expectedTurnId": expected_turn_id,
                "input": build_user_input(&wrapped, attachments)
            }),
        )
        .await?;
        Ok(())
    }

    pub async fn interrupt_turn(&self, thread_id: &str, turn_id: &str) -> AppResult<()> {
        self.request(
            "turn/interrupt",
            json!({ "threadId": thread_id, "turnId": turn_id }),
        )
        .await?;
        let mut state = self.inner.state.lock().await;
        if let Some(runtime) = state.runtimes.get_mut(thread_id) {
            if runtime.active_turn_id.as_deref() == Some(turn_id) {
                runtime.phase = "stopping".to_string();
            }
        }
        Ok(())
    }
}

fn redact_steer_event(event: &mut crate::llm_eval::EvalEvent, secret: &str) {
    replace_known_secret(&mut event.state, secret);
    for text in [&mut event.name, &mut event.summary].into_iter().flatten() {
        replace_known_secret(text, secret);
    }
    if let Some(text) = &mut event.error {
        replace_known_secret(text, secret);
    }
    if let Some(payload) = &mut event.input {
        redact_eval_payload_secret(payload, secret);
    }
    if let Some(payload) = &mut event.output {
        redact_eval_payload_secret(payload, secret);
    }
}

fn next_feature_cursor(page: &Value, seen: &mut HashSet<String>) -> AppResult<Option<String>> {
    let cursor = page
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::to_string);
    if cursor
        .as_ref()
        .is_some_and(|cursor| !seen.insert(cursor.clone()))
    {
        return Err(AppError::parse(
            "Codex experimentalFeature/list repeated a pagination cursor.",
        ));
    }
    Ok(cursor)
}

fn routed_delivery_blocks_normal_resume(state: &SupervisorState, thread_id: &str) -> bool {
    state.routed_delivery_pending.contains(thread_id)
        || (state.routed_config_baselines.contains_key(thread_id)
            && state
                .runtimes
                .get(thread_id)
                .is_some_and(|runtime| runtime.active_turn_id.is_some()))
}

/// Translate Ecky attachments into Codex app-server UserInput blocks. CAD files
/// remain explicit path context because app-server only accepts native image
/// blocks for image attachments.
pub fn build_user_input(prompt: &str, attachments: &[Attachment]) -> Vec<Value> {
    let mut text = prompt.trim().to_string();
    let attachment_notes = attachments
        .iter()
        .filter_map(|attachment| {
            let explanation = attachment.explanation.trim();
            if explanation.is_empty() {
                return None;
            }
            let label = if attachment.name.trim().is_empty() {
                attachment.path.trim()
            } else {
                attachment.name.trim()
            };
            Some(format!("- {label}: {explanation}"))
        })
        .collect::<Vec<_>>();
    if !attachment_notes.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str("[ATTACHMENT NOTES]\n");
        text.push_str(&attachment_notes.join("\n"));
    }
    let cad_context = attachments
        .iter()
        .filter(|attachment| attachment.kind == AttachmentKind::Cad)
        .filter_map(|attachment| {
            let path = attachment.path.trim();
            (!path.is_empty()).then(|| {
                let name = attachment.name.trim();
                if name.is_empty() {
                    format!("- {path}")
                } else {
                    format!("- {name}: {path}")
                }
            })
        })
        .collect::<Vec<_>>();
    if !cad_context.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str("[CAD ATTACHMENTS]\n");
        text.push_str(&cad_context.join("\n"));
    }

    let mut input = Vec::new();
    if !text.is_empty() {
        input.push(json!({ "type": "text", "text": text }));
    }
    for attachment in attachments
        .iter()
        .filter(|attachment| attachment.kind == AttachmentKind::Image)
    {
        if let Some(data_url) = attachment
            .data_url
            .as_deref()
            .filter(|value| value.trim_start().starts_with("data:image/"))
        {
            input.push(json!({ "type": "image", "url": data_url }));
        } else if !attachment.path.trim().is_empty() {
            input.push(json!({ "type": "localImage", "path": attachment.path }));
        }
    }
    input
}

fn should_skip_resume(
    resumed_generation: Option<u64>,
    current_generation: u64,
    refresh_developer_instructions: bool,
    force_resume_request: bool,
) -> bool {
    !refresh_developer_instructions
        && !force_resume_request
        && resumed_generation == Some(current_generation)
}

pub fn resume_params(
    binding: &crate::contracts::CodexTakeoverBinding,
    project_title: &str,
    mcp_endpoint: &str,
    handoff_context: &str,
    model: Option<&str>,
) -> Value {
    resume_params_with_policy(
        binding,
        project_title,
        mcp_endpoint,
        handoff_context,
        model,
        crate::provider_turn::ProviderTurnPolicy::for_intent(
            crate::provider_turn::ProviderTurnIntent::Modify,
        ),
    )
}

pub fn resume_params_with_policy(
    binding: &crate::contracts::CodexTakeoverBinding,
    project_title: &str,
    mcp_endpoint: &str,
    handoff_context: &str,
    model: Option<&str>,
    policy: crate::provider_turn::ProviderTurnPolicy,
) -> Value {
    let mcp_endpoint = crate::mcp::server::provider_bound_endpoint_with_policy(
        mcp_endpoint,
        &binding.ecky_thread_id,
        policy,
    );
    let mut params = json!({
        "threadId": binding.codex_thread_id,
        "cwd": binding.cwd,
        "approvalPolicy": "on-request",
        "approvalsReviewer": "auto_review",
        "sandbox": "workspace-write",
        "developerInstructions": bootstrap_instructions(
            &binding.ecky_thread_id,
            project_title,
            &binding.cwd,
            handoff_context,
        ),
        "excludeTurns": true,
        "initialTurnsPage": {
            "limit": 1,
            "sortDirection": "desc",
            "itemsView": "notLoaded"
        },
        "config": {
            "mcp_servers.ecky_provider_mcp.url": mcp_endpoint,
            "mcp_servers.ecky_provider_mcp.required": true,
            "mcp_servers.ecky_provider_mcp.default_tools_approval_mode": "approve",
        }
    });
    if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
        params["model"] = Value::String(model.trim().to_string());
    }
    params
}

pub fn resume_params_with_routed_hook(
    binding: &crate::contracts::CodexTakeoverBinding,
    project_title: &str,
    mcp_endpoint: &str,
    handoff_context: &str,
    model: Option<&str>,
    policy: crate::provider_turn::ProviderTurnPolicy,
    _command: &str,
) -> Value {
    let mut params = resume_params_with_policy(
        binding,
        project_title,
        mcp_endpoint,
        handoff_context,
        model,
        policy,
    );
    params["config"]["features.shell_tool"] = Value::Bool(false);
    params["config"]["features.unified_exec"] = Value::Bool(false);
    params["config"]["features.multi_agent"] = Value::Bool(false);
    params["config"]["features.apps"] = Value::Bool(false);
    params["config"]["features.view_image"] = Value::Bool(false);
    params["config"]["web_search"] = Value::String("disabled".to_string());
    params
}

fn native_config_baseline(config: &Value, feature_enablement: &Value) -> AppResult<Value> {
    let features = feature_enablement
        .as_object()
        .ok_or_else(|| AppError::parse("Codex experimentalFeature/list result is invalid."))?;
    let mut baseline = serde_json::Map::new();
    for name in [
        "shell_tool",
        "unified_exec",
        "multi_agent",
        "apps",
        "view_image",
    ] {
        let enabled = features
            .get(name)
            .and_then(Value::as_bool)
            .ok_or_else(|| AppError::parse(format!("Codex feature state '{name}' is unknown.")))?;
        baseline.insert(format!("features.{name}"), Value::Bool(enabled));
    }
    // Ecky's resumed turns use workspace-write sandbox. Official local-chat
    // defaults in that sandbox to cached search when config omits this setting.
    let web_search = config
        .get("web_search")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| Value::String("cached".into()));
    baseline.insert("web_search".into(), web_search);
    Ok(Value::Object(baseline))
}

fn merge_config_overrides(params: &mut Value, overrides: &Value) {
    let Some(target) = params.get_mut("config").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(values) = overrides.as_object() {
        target.extend(
            values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
}

pub fn codex_pre_tool_hook_command() -> AppResult<String> {
    let executable = std::env::current_exe().map_err(|error| {
        AppError::internal(format!("Cannot locate Ecky hook executable: {error}"))
    })?;
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    Ok(format!(
        "{} --ecky-codex-pre-tool-hook || exit 2",
        quote(&executable.to_string_lossy())
    ))
}

fn codex_startup_hook_override(command: &str) -> AppResult<String> {
    let command = serde_json::to_string(command).map_err(|error| {
        AppError::internal(format!("Cannot encode Codex hook command: {error}"))
    })?;
    Ok(format!(
        "hooks.PreToolUse=[{{matcher=\".*\",hooks=[{{type=\"command\",command={command},timeout=5}}]}}]"
    ))
}

fn codex_hook_review_diagnostic(
    executable: &std::path::Path,
    cwd: &str,
    hook_command: &str,
) -> AppResult<String> {
    let startup_override = codex_startup_hook_override(hook_command)?;
    let shell_quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let command = format!(
        "{} -C {} -c {}",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(cwd),
        shell_quote(&startup_override)
    );
    Ok(format!(
        "Ecky routed turns require its exact trusted startup PreToolUse hook. Run {command}, then /hooks; trust only this exact hook, then retry."
    ))
}

#[cfg(test)]
fn codex_startup_hook_override_if_available(path: &std::path::Path) -> AppResult<Option<String>> {
    if crate::services::codex_pre_tool_hook::runtime_descriptor_at(&path.to_path_buf()).is_err() {
        return Ok(None);
    }
    let command = codex_pre_tool_hook_command()?;
    codex_startup_hook_override(&command).map(Some)
}

fn verify_owned_pre_tool_hook(
    hooks: &Value,
    expected_cwd: &str,
    expected_command: &str,
) -> AppResult<String> {
    let found = hooks
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("cwd").and_then(Value::as_str) == Some(expected_cwd))
        .flat_map(|entry| {
            entry
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find(|hook| {
            hook.get("eventName").and_then(Value::as_str) == Some("preToolUse")
                && hook.get("enabled").and_then(Value::as_bool) == Some(true)
                && hook.get("source").and_then(Value::as_str) == Some("sessionFlags")
                && matches!(
                    hook.get("trustStatus").and_then(Value::as_str),
                    Some("trusted" | "managed")
                )
                && hook.get("matcher").and_then(Value::as_str) == Some(".*")
                && hook.get("async").and_then(Value::as_bool) == Some(false)
                && hook.get("timeoutSec").and_then(Value::as_u64) == Some(5)
                && hook
                    .get("currentHash")
                    .and_then(Value::as_str)
                    .is_some_and(|hash| !hash.is_empty())
                && hook.get("command").and_then(Value::as_str) == Some(expected_command)
                && hook.get("handlerType").and_then(Value::as_str) == Some("command")
        });
    found
        .and_then(|hook| hook.get("currentHash").and_then(Value::as_str))
        .map(str::to_string)
        .ok_or_else(|| AppError::provider(
            "Ecky routed turns require the exact trusted startup PreToolUse hook. Restart Ecky to show the exact review command, then run /hooks and trust only that hook.",
        ))
}

pub fn start_params(
    ecky_thread_id: &str,
    project_title: &str,
    cwd: &str,
    mcp_endpoint: &str,
    handoff_context: &str,
    model: Option<&str>,
) -> Value {
    let mcp_endpoint = crate::mcp::server::provider_bound_endpoint(mcp_endpoint, ecky_thread_id);
    let mut params = json!({
        "cwd": cwd,
        "approvalPolicy": "on-request",
        "approvalsReviewer": "auto_review",
        "sandbox": "workspace-write",
        "developerInstructions": bootstrap_instructions(
            ecky_thread_id,
            project_title,
            cwd,
            handoff_context,
        ),
        "ephemeral": false,
        "serviceName": "ecky",
        "config": {
            "mcp_servers.ecky_provider_mcp.url": mcp_endpoint,
            "mcp_servers.ecky_provider_mcp.required": true,
            "mcp_servers.ecky_provider_mcp.default_tools_approval_mode": "approve",
        }
    });
    if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
        params["model"] = Value::String(model.trim().to_string());
    }
    params
}

pub fn turn_start_params(
    thread_id: &str,
    prompt: &str,
    model: Option<&str>,
    attachments: &[Attachment],
    policy: crate::provider_turn::ProviderTurnPolicy,
) -> Value {
    let turn_nonce = uuid::Uuid::new_v4().to_string();
    let mut params = json!({
        "threadId": thread_id,
        "input": build_user_input(&policy.wrap_user_message_for_turn(prompt, &turn_nonce), attachments),
        "approvalPolicy": if policy.allows_project_writes() { "on-request" } else { "never" },
        "sandboxPolicy": if policy.allows_project_writes() {
            json!({ "type": "workspaceWrite" })
        } else {
            json!({ "type": "readOnly", "networkAccess": false })
        }
    });
    if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
        params["model"] = Value::String(model.trim().to_string());
    }
    params
}

pub fn message_page_params(
    thread_id: &str,
    cursor: Option<String>,
    direction: Option<&str>,
) -> Value {
    json!({
        "threadId": thread_id,
        "cursor": cursor,
        "limit": CODEX_TRANSCRIPT_PAGE_SIZE,
        "sortDirection": if direction == Some("newer") { "asc" } else { "desc" },
        "itemsView": "full"
    })
}

pub fn parse_model_list_page(result: &Value) -> AppResult<(Vec<String>, Option<String>)> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::parse("Codex model/list result is missing data."))?;
    let mut seen = HashSet::new();
    let models = data
        .iter()
        .filter(|entry| {
            !entry
                .get("hidden")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|entry| entry.get("model").and_then(Value::as_str))
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .filter(|model| seen.insert((*model).to_string()))
        .map(str::to_string)
        .collect();
    let next_cursor = result
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok((models, next_cursor))
}

fn codex_request_timeout() -> Duration {
    std::env::var("ECKY_CODEX_REQUEST_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_CODEX_REQUEST_TIMEOUT)
}

fn terminal_turn_key(thread_id: &str, turn_id: &str) -> String {
    format!("{thread_id}\0{turn_id}")
}

fn remember_terminal_turn(state: &mut SupervisorState, thread_id: &str, turn_id: &str) {
    let key = terminal_turn_key(thread_id, turn_id);
    if state.terminal_turns.insert(key.clone()) {
        state.terminal_turn_order.push_back(key);
    }
    while state.terminal_turn_order.len() > TERMINAL_TURN_MEMORY {
        if let Some(expired) = state.terminal_turn_order.pop_front() {
            state.terminal_turns.remove(&expired);
        }
    }
}

async fn write_json_line(stdin: &Arc<Mutex<ChildStdin>>, payload: &Value) -> AppResult<()> {
    let mut bytes = serde_json::to_vec(payload).map_err(|error| {
        AppError::internal(format!("Failed to encode app-server request: {error}"))
    })?;
    bytes.push(b'\n');
    let mut stdin = stdin.lock().await;
    stdin.write_all(&bytes).await.map_err(|error| {
        AppError::provider(format!("Failed writing Codex app-server stdin: {error}"))
    })?;
    stdin.flush().await.map_err(|error| {
        AppError::provider(format!("Failed flushing Codex app-server stdin: {error}"))
    })
}

fn parse_thread_summary(thread: &Value) -> AppResult<CodexThreadSummary> {
    let required_string = |key: &str| {
        thread
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| AppError::parse(format!("Codex thread is missing {key}.")))
    };
    Ok(CodexThreadSummary {
        id: required_string("id")?,
        name: thread
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        preview: required_string("preview")?,
        cwd: required_string("cwd")?,
        created_at: thread
            .get("createdAt")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        updated_at: thread
            .get("updatedAt")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        model_provider: required_string("modelProvider")?,
        status: thread
            .get("status")
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
            .or_else(|| thread.get("status").and_then(Value::as_str))
            .unwrap_or("unknown")
            .to_string(),
    })
}

pub fn bootstrap_instructions(
    ecky_thread_id: &str,
    title: &str,
    cwd: &str,
    handoff_context: &str,
) -> String {
    crate::mcp::authoring::codex_provider_bootstrap_text(
        crate::services::codex_takeover::CODEX_BOOTSTRAP_VERSION,
        ecky_thread_id,
        title,
        cwd,
        handoff_context,
    )
}

fn live_message_id(thread_id: &str, item_id: &str) -> String {
    format!("codex:{thread_id}:{item_id}")
}

fn append_live_delta(
    messages: &mut Vec<CodexDialogueMessage>,
    id: String,
    prefix: &str,
    delta: &str,
    now: i64,
    provider_event_kind: crate::contracts::ProviderEventKind,
) {
    if delta.is_empty() {
        return;
    }
    if let Some(message) = messages.iter_mut().find(|message| message.id == id) {
        message.content.push_str(delta);
        message.timestamp = now;
        return;
    }
    messages.push(CodexDialogueMessage {
        id,
        role: "assistant".to_string(),
        content: format!("{prefix}{delta}"),
        status: "working".to_string(),
        timestamp: now,
        attachments: Vec::new(),
        provider_event_kind: Some(provider_event_kind),
    });
}

fn replace_live_message(
    messages: &mut Vec<CodexDialogueMessage>,
    id: String,
    content: String,
    now: i64,
    provider_event_kind: crate::contracts::ProviderEventKind,
) {
    if content.trim().is_empty() {
        return;
    }
    if let Some(message) = messages.iter_mut().find(|message| message.id == id) {
        message.content = content;
        message.status = "working".to_string();
        message.timestamp = now;
        return;
    }
    messages.push(CodexDialogueMessage {
        id,
        role: "assistant".to_string(),
        content,
        status: "working".to_string(),
        timestamp: now,
        attachments: Vec::new(),
        provider_event_kind: Some(provider_event_kind),
    });
}

fn codex_turn_trace_status(params: &Value) -> &'static str {
    match params
        .get("turn")
        .unwrap_or(params)
        .get("status")
        .and_then(Value::as_str)
    {
        Some("completed") => "success",
        Some("interrupted" | "canceled" | "cancelled") => "interrupted",
        _ => "error",
    }
}

fn ensure_codex_eval_capture(state: &mut SupervisorState, thread_id: &str, turn_id: &str) {
    let key = terminal_turn_key(thread_id, turn_id);
    if state.active_eval_runs.contains_key(&key) {
        return;
    }
    if let Some(pending) = state.pending_eval_seeds.remove(thread_id) {
        state
            .active_eval_runs
            .insert(key, active_eval_from_pending(pending, turn_id.to_string()));
    }
}

fn active_eval_from_pending(pending: PendingCodexEval, turn_id: String) -> ActiveCodexEval {
    ActiveCodexEval {
        seed: pending.seed,
        redaction_secret: pending.redaction_secret,
        turn_id,
        started_at: pending.started_at,
        events: Vec::new(),
        invocation_steps: HashMap::new(),
        seen_invocation_states: HashSet::new(),
        assistant_message_items: HashSet::new(),
        next_step_index: 0,
        omitted_reasons: HashMap::new(),
    }
}

fn finish_pending_codex_eval(
    pending: PendingCodexEval,
    turn_id: String,
    completed_at: i64,
    status: &str,
    raw_error: Option<String>,
) -> crate::llm_eval::EvalRun {
    finish_codex_eval_capture(
        active_eval_from_pending(pending, turn_id),
        completed_at,
        status,
        raw_error,
        None,
    )
}

fn apply_codex_reported_route(
    state: &mut SupervisorState,
    thread_id: &str,
    turn_id: &str,
    turn: &Value,
) {
    let model = turn
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
    let effort = turn
        .get("reasoningEffort")
        .or_else(|| turn.get("effort"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let key = terminal_turn_key(thread_id, turn_id);
    if let Some(capture) = state.active_eval_runs.get_mut(&key) {
        if model.is_some() {
            capture.seed.model = model.clone();
        }
        if effort.is_some() {
            capture.seed.effort = effort.clone();
        }
    }
    for run in &mut state.completed_eval_runs {
        if run.turn_id == turn_id && run.external_thread_id == thread_id {
            if model.is_some() {
                run.route.model = model.clone();
            }
            if effort.is_some() {
                run.route.effort = effort.clone();
            }
        }
    }
}

fn append_codex_eval_event(capture: &mut ActiveCodexEval, mut event: crate::llm_eval::EvalEvent) {
    if capture.events.len() >= CODEX_EVAL_EVENT_LIMIT.saturating_sub(2) {
        note_codex_eval_omission(capture, "event-limit");
        return;
    }
    event.sequence = capture.events.len() as u64 + 1;
    capture.events.push(event);
}

fn note_codex_eval_omission(capture: &mut ActiveCodexEval, reason: &'static str) {
    let count = capture.omitted_reasons.entry(reason).or_default();
    *count = count.saturating_add(1);
}

fn append_codex_eval_terminal_event(
    capture: &mut ActiveCodexEval,
    mut event: crate::llm_eval::EvalEvent,
) {
    if capture.events.len() >= CODEX_EVAL_EVENT_LIMIT {
        return;
    }
    event.sequence = capture.events.len() as u64 + 1;
    capture.events.push(event);
}

fn replace_known_secret(value: &mut String, secret: &str) {
    if !secret.is_empty() {
        *value = value.replace(secret, "[REDACTED]");
    }
}

fn redact_json_secret(value: &mut Value, secret: &str) {
    match value {
        Value::String(text) => replace_known_secret(text, secret),
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| redact_json_secret(value, secret)),
        Value::Object(fields) => {
            let previous = std::mem::take(fields);
            for (mut key, mut child) in previous {
                replace_known_secret(&mut key, secret);
                redact_json_secret(&mut child, secret);
                let base = key.clone();
                let mut suffix = 1usize;
                while fields.contains_key(&key) {
                    key = format!("{base}#{suffix}");
                    suffix += 1;
                }
                fields.insert(key, child);
            }
        }
        _ => {}
    }
}

fn redact_eval_payload_secret(payload: &mut crate::llm_eval::EvalPayload, secret: &str) {
    fn redact_typed(value: &mut crate::llm_eval::EvalValue, secret: &str) {
        match value {
            crate::llm_eval::EvalValue::String(text) => replace_known_secret(text, secret),
            crate::llm_eval::EvalValue::List(values) => {
                for value in values {
                    redact_typed(value, secret);
                }
            }
            crate::llm_eval::EvalValue::Map(fields) => {
                let mut seen = HashSet::new();
                for field in fields {
                    replace_known_secret(&mut field.key, secret);
                    let base = field.key.clone();
                    let mut suffix = 1usize;
                    while !seen.insert(field.key.clone()) {
                        field.key = format!("{base}#{suffix}");
                        suffix += 1;
                    }
                    redact_typed(&mut field.value, secret);
                }
            }
            _ => {}
        }
    }
    redact_typed(&mut payload.value, secret);
}

fn redact_probability_keys(values: &mut std::collections::BTreeMap<String, f64>, secret: &str) {
    *values = std::mem::take(values)
        .into_iter()
        .map(|(mut key, probability)| {
            replace_known_secret(&mut key, secret);
            (key, probability)
        })
        .collect();
}

fn redact_active_eval_secret(
    capture: &mut ActiveCodexEval,
    raw_error: &mut Option<String>,
    terminal_output: &mut Option<Value>,
    secret: &str,
) {
    replace_known_secret(&mut capture.seed.prompt, secret);
    replace_known_secret(&mut capture.seed.provider, secret);
    replace_known_secret(&mut capture.seed.prompt_version, secret);
    if let Some(effort) = &mut capture.seed.effort {
        replace_known_secret(effort, secret);
    }
    if let Some(model) = &mut capture.seed.model {
        replace_known_secret(model, secret);
    }
    if let Some(evidence) = &mut capture.seed.jev_route {
        replace_known_secret(&mut evidence.policy_version, secret);
        if let Some(model) = &mut evidence.model_ceiling {
            replace_known_secret(model, secret);
        }
        replace_known_secret(&mut evidence.model_reason, secret);
        if let Some(model) = &mut evidence.classifier_model {
            replace_known_secret(model, secret);
        }
        if let Some(version) = &mut evidence.model_catalog_version {
            replace_known_secret(version, secret);
        }
        if let Some(expiry) = &mut evidence.model_catalog_valid_until {
            replace_known_secret(expiry, secret);
        }
        redact_probability_keys(&mut evidence.intent_probabilities, secret);
        redact_probability_keys(&mut evidence.model_probabilities, secret);
    }
    for event in &mut capture.events {
        replace_known_secret(&mut event.state, secret);
        if let Some(value) = &mut event.name {
            replace_known_secret(value, secret);
        }
        if let Some(value) = &mut event.summary {
            replace_known_secret(value, secret);
        }
        if let Some(value) = &mut event.error {
            replace_known_secret(value, secret);
        }
        if let Some(payload) = &mut event.input {
            redact_eval_payload_secret(payload, secret);
        }
        if let Some(payload) = &mut event.output {
            redact_eval_payload_secret(payload, secret);
        }
    }
    if let Some(error) = raw_error {
        replace_known_secret(error, secret);
    }
    if let Some(output) = terminal_output {
        redact_json_secret(output, secret);
    }
}

fn remove_null_object_fields(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .collect(),
        ),
        value => value,
    }
}

fn finish_codex_eval_capture(
    mut capture: ActiveCodexEval,
    completed_at: i64,
    status: &str,
    mut raw_error: Option<String>,
    mut terminal_output: Option<Value>,
) -> crate::llm_eval::EvalRun {
    if let Some(secret) = capture.redaction_secret.take() {
        redact_active_eval_secret(&mut capture, &mut raw_error, &mut terminal_output, &secret);
    }
    if !capture.omitted_reasons.is_empty() {
        if capture.events.len() >= CODEX_EVAL_EVENT_LIMIT {
            capture.events.pop();
        }
        let mut reasons = capture.omitted_reasons.iter().collect::<Vec<_>>();
        reasons.sort_by_key(|(reason, _)| **reason);
        let summary = reasons
            .into_iter()
            .map(|(reason, count)| format!("{reason}={count}"))
            .collect::<Vec<_>>()
            .join(", ");
        append_codex_eval_terminal_event(
            &mut capture,
            crate::llm_eval::EvalEvent {
                sequence: 0,
                step_index: None,
                kind: crate::llm_eval::EvalEventKind::System,
                state: "capture-incomplete".into(),
                name: Some("capture-incomplete".into()),
                summary: Some(summary),
                input: None,
                output: None,
                error: None,
                occurred_at: completed_at,
            },
        );
    }
    if !capture.turn_id.is_empty() {
        append_codex_eval_terminal_event(
            &mut capture,
            crate::llm_eval::EvalEvent {
                sequence: 0,
                step_index: None,
                kind: crate::llm_eval::EvalEventKind::System,
                state: status.into(),
                name: Some("turn/terminal".into()),
                summary: None,
                input: None,
                output: terminal_output.map(crate::llm_eval::EvalPayload::new),
                error: raw_error.clone(),
                occurred_at: completed_at,
            },
        );
    }
    let response = capture
        .events
        .iter()
        .filter(|event| event.kind == crate::llm_eval::EvalEventKind::Assistant)
        .filter_map(|event| event.output.as_ref())
        .filter_map(|payload| match &payload.value {
            crate::llm_eval::EvalValue::String(value) => Some(value.as_str()),
            _ => None,
        })
        .collect::<String>();
    crate::llm_eval::build_codex_eval_run(
        capture.seed,
        capture.turn_id,
        capture.started_at,
        completed_at,
        status,
        (!response.trim().is_empty()).then_some(response),
        raw_error,
        capture.events,
        Vec::new(),
    )
}

fn project_codex_eval_notification(
    state: &mut SupervisorState,
    thread_id: &str,
    method: &str,
    params: &Value,
    now: i64,
) -> Option<crate::llm_eval::EvalRun> {
    let turn = params.get("turn").unwrap_or(params);
    let turn_id = turn
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| params.get("turnId").and_then(Value::as_str))
        .or_else(|| {
            state
                .runtimes
                .get(thread_id)
                .and_then(|runtime| runtime.active_turn_id.as_deref())
        })
        .map(str::to_string);
    if method == "turn/started" {
        if let Some(turn_id) = turn_id.as_deref() {
            ensure_codex_eval_capture(state, thread_id, turn_id);
        }
    }
    let turn_id = turn_id.as_deref()?;
    let key = terminal_turn_key(thread_id, turn_id);
    if method == "turn/completed" {
        apply_codex_reported_route(state, thread_id, turn_id, turn);
    }
    let capture = state.active_eval_runs.get_mut(&key)?;
    if matches!(method, "item/started" | "item/completed") {
        let item = params.get("item").unwrap_or(params);
        let item_type = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if item_type == "agentMessage" {
            let Some(item_id) = item.get("id").and_then(Value::as_str) else {
                note_codex_eval_omission(capture, "assistant-missing-item-id");
                return None;
            };
            if method == "item/completed" {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !capture.assistant_message_items.contains(item_id) {
                        if capture.assistant_message_items.len() >= CODEX_EVAL_EVENT_LIMIT / 2 {
                            note_codex_eval_omission(capture, "assistant-item-limit");
                        } else {
                            capture.assistant_message_items.insert(item_id.to_string());
                            append_codex_eval_event(
                                capture,
                                crate::llm_eval::EvalEvent {
                                    sequence: 0,
                                    step_index: None,
                                    kind: crate::llm_eval::EvalEventKind::Assistant,
                                    state: "completed".into(),
                                    name: Some("agentMessage".into()),
                                    summary: None,
                                    input: None,
                                    output: Some(crate::llm_eval::EvalPayload::new(Value::String(
                                        text.into(),
                                    ))),
                                    error: None,
                                    occurred_at: now,
                                },
                            );
                        }
                    }
                }
            }
            return None;
        }
        let Some(item_id) = item.get("id").and_then(Value::as_str) else {
            note_codex_eval_omission(capture, "missing-item-id");
            return None;
        };
        if matches!(
            item_type,
            "userMessage" | "reasoning" | "plan" | "compaction"
        ) {
            return None;
        }
        if !matches!(
            item_type,
            "commandExecution" | "mcpToolCall" | "fileChange" | "webSearch" | "collabToolCall"
        ) {
            note_codex_eval_omission(capture, "unsupported-item-type");
            return None;
        }
        let state_identity = format!("{item_id}:{method}");
        if capture.seen_invocation_states.contains(&state_identity) {
            return None;
        }
        let existing_step = capture.invocation_steps.get(item_id).copied();
        let orphan_completion = method == "item/completed" && existing_step.is_none();
        if orphan_completion {
            note_codex_eval_omission(capture, "completed-without-start");
        }
        if existing_step.is_none() && capture.invocation_steps.len() >= CODEX_EVAL_EVENT_LIMIT / 2 {
            note_codex_eval_omission(capture, "invocation-limit");
            return None;
        }
        capture.seen_invocation_states.insert(state_identity);
        let invocation_step = if let Some(step) = existing_step {
            step
        } else {
            let step = capture.next_step_index;
            capture.next_step_index += 1;
            capture.invocation_steps.insert(item_id.to_string(), step);
            step
        };
        let identity = serde_json::json!({
            "threadId": thread_id,
            "turnId": turn_id,
            "itemId": item_id,
            "invocationId": item_id,
        });
        let input = (method == "item/started").then(|| {
            let args = item
                .get("arguments")
                .or_else(|| item.get("params"))
                .or_else(|| item.get("input"));
            crate::llm_eval::EvalPayload::new(remove_null_object_fields(serde_json::json!({
                "identity": identity,
                "itemType": item_type,
                "toolName": item.get("toolName").or_else(|| item.get("tool")),
                "serverName": item.get("serverName").or_else(|| item.get("server")),
                "arguments": args,
                "command": item.get("command"),
                "changes": item.get("changes"),
                "query": item.get("query"),
            })))
        });
        let has_result = [
            "result",
            "output",
            "aggregatedOutput",
            "exitCode",
            "status",
            "error",
        ]
        .iter()
        .any(|field| item.get(*field).is_some());
        let output = (method == "item/completed" && has_result).then(|| {
            crate::llm_eval::EvalPayload::new(remove_null_object_fields(serde_json::json!({
                "identity": identity,
                "itemType": item_type,
                "toolName": item.get("toolName").or_else(|| item.get("tool")),
                "serverName": item.get("serverName").or_else(|| item.get("server")),
                "result": item.get("result"),
                "output": item.get("output"),
                "aggregatedOutput": item.get("aggregatedOutput"),
                "exitCode": item.get("exitCode"),
                "status": item.get("status"),
                "error": item.get("error"),
            })))
        });
        append_codex_eval_event(
            capture,
            crate::llm_eval::EvalEvent {
                sequence: 0,
                step_index: Some(invocation_step),
                kind: if method == "item/started" {
                    crate::llm_eval::EvalEventKind::Tool
                } else {
                    crate::llm_eval::EvalEventKind::Result
                },
                state: if method == "item/started" {
                    "started"
                } else if orphan_completion {
                    "completed-without-start"
                } else if has_result {
                    "completed"
                } else {
                    "completed-result-missing"
                }
                .into(),
                name: Some(if item_type == "mcpToolCall" {
                    let server = item
                        .get("serverName")
                        .or_else(|| item.get("server"))
                        .and_then(Value::as_str)
                        .unwrap_or("mcp");
                    let tool = item
                        .get("toolName")
                        .or_else(|| item.get("tool"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool");
                    format!("{server}/{tool}")
                } else {
                    item_type.to_string()
                }),
                summary: Some(format!("{thread_id}/{turn_id}/{item_id}")),
                input,
                output,
                error: item
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                occurred_at: now,
            },
        );
    } else if method == "item/agentMessage/delta" {
        if let Some(delta) = params.get("delta").and_then(Value::as_str) {
            if let Some(item_id) = params.get("itemId").and_then(Value::as_str) {
                if capture.assistant_message_items.len() >= CODEX_EVAL_EVENT_LIMIT / 2
                    && !capture.assistant_message_items.contains(item_id)
                {
                    note_codex_eval_omission(capture, "assistant-item-limit");
                    return None;
                }
                capture.assistant_message_items.insert(item_id.to_string());
            } else {
                note_codex_eval_omission(capture, "assistant-missing-item-id");
            }
            append_codex_eval_event(
                capture,
                crate::llm_eval::EvalEvent {
                    sequence: 0,
                    step_index: None,
                    kind: crate::llm_eval::EvalEventKind::Assistant,
                    state: "delta".into(),
                    name: Some("agentMessage".into()),
                    summary: None,
                    input: None,
                    output: Some(crate::llm_eval::EvalPayload::new(Value::String(
                        delta.into(),
                    ))),
                    error: None,
                    occurred_at: now,
                },
            );
        }
    }
    if method != "turn/completed" {
        return None;
    }
    let status = codex_turn_trace_status(params);
    let raw_error = turn
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let capture = state.active_eval_runs.remove(&key)?;
    Some(finish_codex_eval_capture(
        capture,
        now,
        status,
        raw_error,
        Some(serde_json::json!({
            "threadId": thread_id,
            "turnId": turn_id,
            "status": turn.get("status"),
            "error": turn.get("error"),
        })),
    ))
}

fn mark_live_messages_terminal(messages: &mut [CodexDialogueMessage], status: &str) {
    let message_status = match status {
        "success" => "success",
        "interrupted" => "discarded",
        _ => "error",
    };
    for message in messages {
        message.status = message_status.to_string();
    }
}

fn push_turn_trace(
    traces: &mut HashMap<String, Vec<ProviderTurnTrace>>,
    thread_id: &str,
    trace: ProviderTurnTrace,
) {
    let thread_traces = traces.entry(thread_id.to_string()).or_default();
    thread_traces.push(trace);
    if thread_traces.len() > TURN_TRACE_LIMIT {
        thread_traces.drain(..thread_traces.len() - TURN_TRACE_LIMIT);
    }
}

pub fn take_terminal_trace(
    messages: &mut Vec<CodexDialogueMessage>,
    turn_id: &str,
    status: &str,
    completed_at: i64,
) -> Option<ProviderTurnTrace> {
    (!messages.is_empty()).then(|| ProviderTurnTrace {
        turn_id: turn_id.to_string(),
        status: status.to_string(),
        messages: std::mem::take(messages),
        completed_at,
    })
}

fn enforce_live_bounds(messages: &mut Vec<CodexDialogueMessage>) {
    for message in messages.iter_mut() {
        if let Some((byte_index, _)) = message.content.char_indices().nth(LIVE_MESSAGE_CHAR_LIMIT) {
            message.content.truncate(byte_index);
        }
    }
    if messages.len() > LIVE_MESSAGE_LIMIT {
        messages.drain(..messages.len() - LIVE_MESSAGE_LIMIT);
    }
}

fn string_field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
}

fn tool_activity(item: &Value) -> Option<String> {
    match item.get("type").and_then(Value::as_str) {
        Some("commandExecution") => {
            let command = item
                .get("command")
                .and_then(|command| match command {
                    Value::String(command) => Some(command.clone()),
                    Value::Array(parts) => Some(
                        parts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => None,
                })
                .unwrap_or_else(|| "shell command".to_string());
            Some(format!("RUNNING · {command}"))
        }
        Some("mcpToolCall") => {
            let server = string_field(item, &["server", "serverName"]).unwrap_or("mcp");
            let tool = string_field(item, &["tool", "toolName", "name"]).unwrap_or("tool");
            let full_name = format!("{server}/{tool}");
            let params = item
                .get("arguments")
                .or_else(|| item.get("params"))
                .or_else(|| item.get("input"));
            if let Some(detail) = crate::llm_eval::format_tool_call_details(&full_name, params) {
                Some(detail)
            } else {
                Some(format!("USING TOOL · {full_name}"))
            }
        }
        Some("fileChange") => {
            let paths = item
                .get("changes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|change| string_field(change, &["path", "filePath"]))
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!(
                "EDITING · {}",
                if paths.is_empty() {
                    "project files"
                } else {
                    &paths
                }
            ))
        }
        Some("webSearch") => Some(format!(
            "SEARCHING · {}",
            string_field(item, &["query"]).unwrap_or("web")
        )),
        Some("collabToolCall") => Some(format!(
            "DELEGATING · {}",
            string_field(item, &["tool", "toolName", "name"]).unwrap_or("agent task")
        )),
        _ => None,
    }
}

/// Projects public app-server progress into transient dialogue bubbles. Readable
/// reasoning summaries are allowed; raw reasoning text is intentionally ignored.
pub fn apply_live_notification(
    messages: &mut Vec<CodexDialogueMessage>,
    thread_id: &str,
    method: &str,
    params: &Value,
    now: i64,
) {
    if method == "turn/started" {
        messages.clear();
        return;
    }
    if method == "turn/completed" {
        mark_live_messages_terminal(messages, codex_turn_trace_status(params));
        return;
    }

    let item_id = params.get("itemId").and_then(Value::as_str).or_else(|| {
        params
            .get("item")
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
    });
    match method {
        "item/agentMessage/delta" => {
            if let (Some(item_id), Some(delta)) =
                (item_id, params.get("delta").and_then(Value::as_str))
            {
                append_live_delta(
                    messages,
                    live_message_id(thread_id, item_id),
                    "",
                    delta,
                    now,
                    crate::contracts::ProviderEventKind::Assistant,
                );
            }
        }
        "item/reasoning/summaryTextDelta" => {
            if let (Some(item_id), Some(delta)) =
                (item_id, params.get("delta").and_then(Value::as_str))
            {
                append_live_delta(
                    messages,
                    live_message_id(thread_id, item_id),
                    "THINKING · ",
                    delta,
                    now,
                    crate::contracts::ProviderEventKind::Activity,
                );
            }
        }
        "item/plan/delta" => {
            if let (Some(item_id), Some(delta)) =
                (item_id, params.get("delta").and_then(Value::as_str))
            {
                append_live_delta(
                    messages,
                    live_message_id(thread_id, item_id),
                    "PLAN · ",
                    delta,
                    now,
                    crate::contracts::ProviderEventKind::Activity,
                );
            }
        }
        "item/started" | "item/completed" => {
            let item = params.get("item").unwrap_or(params);
            let Some(item_id) = item.get("id").and_then(Value::as_str) else {
                return;
            };
            if item.get("type").and_then(Value::as_str) == Some("agentMessage") {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    replace_live_message(
                        messages,
                        live_message_id(thread_id, item_id),
                        text.to_string(),
                        now,
                        crate::contracts::ProviderEventKind::Assistant,
                    );
                }
            } else if let Some(activity) = tool_activity(item) {
                replace_live_message(
                    messages,
                    live_message_id(thread_id, item_id),
                    activity,
                    now,
                    crate::contracts::ProviderEventKind::Activity,
                );
            }
        }
        _ => {}
    }
    enforce_live_bounds(messages);
}

pub fn apply_runtime_notification(
    runtime: &mut crate::contracts::CodexTakeoverRuntime,
    method: &str,
    params: &Value,
) {
    let turn = params.get("turn").unwrap_or(params);
    match method {
        "thread/status/changed" => match params
            .get("status")
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
        {
            Some("idle") => {
                runtime.phase = "idle".to_string();
                runtime.active_turn_id = None;
                runtime.error = None;
            }
            Some("systemError") => {
                runtime.phase = "error".to_string();
                runtime.active_turn_id = None;
                runtime.error = Some("Codex thread entered systemError state.".to_string());
            }
            Some("notLoaded") => {
                runtime.phase = "disconnected".to_string();
                runtime.active_turn_id = None;
            }
            _ => {}
        },
        "turn/started" => {
            if let Some(turn_id) = turn.get("id").and_then(Value::as_str) {
                runtime.phase = "active".to_string();
                runtime.active_turn_id = Some(turn_id.to_string());
                runtime.error = None;
            }
        }
        "turn/completed" => {
            let Some(turn_id) = turn.get("id").and_then(Value::as_str) else {
                return;
            };
            if runtime.active_turn_id.as_deref() != Some(turn_id) {
                return;
            }
            if turn.get("status").and_then(Value::as_str) == Some("failed") {
                runtime.error = turn
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            runtime.phase = "idle".to_string();
            runtime.active_turn_id = None;
        }
        _ => {
            // Item deltas, reasoning, and compaction are progress. They never
            // manufacture terminal state or release the FIFO dispatcher.
        }
    }
}

pub fn runtime_from_turn_page(result: &Value) -> AppResult<CodexTakeoverRuntime> {
    let turns = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::parse("Codex thread/turns/list result is missing data."))?;
    let Some(turn) = turns.first() else {
        return Ok(CodexTakeoverRuntime::default());
    };
    if turn.get("status").and_then(Value::as_str) != Some("inProgress") {
        return Ok(CodexTakeoverRuntime::default());
    }
    let turn_id = turn
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::parse("Codex in-progress turn is missing id."))?;
    Ok(CodexTakeoverRuntime {
        phase: "active".to_string(),
        active_turn_id: Some(turn_id.to_string()),
        error: None,
    })
}

pub fn apply_start_response(
    runtime: &mut crate::contracts::CodexTakeoverRuntime,
    turn_id: &str,
    already_terminal: bool,
) {
    if already_terminal {
        *runtime = CodexTakeoverRuntime::default();
    } else {
        runtime.phase = "active".to_string();
        runtime.active_turn_id = Some(turn_id.to_string());
        runtime.error = None;
    }
}

pub fn response_result_for_id(line: &str, expected_id: u64) -> AppResult<Option<Value>> {
    let message: Value = serde_json::from_str(line).map_err(|error| {
        AppError::provider(format!(
            "Codex app-server returned malformed JSON: {error}. Raw line: {line}"
        ))
    })?;
    let Some(id) = message.get("id").and_then(Value::as_u64) else {
        return Ok(None);
    };
    if id != expected_id {
        return Ok(None);
    }
    if let Some(error) = message.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex app-server request failed");
        let details = error
            .get("data")
            .map(Value::to_string)
            .filter(|details| details != "null");
        return Err(match details {
            Some(details) => {
                AppError::with_details(crate::contracts::AppErrorCode::Provider, message, details)
            }
            None => AppError::provider(message),
        });
    }
    message.get("result").cloned().map(Some).ok_or_else(|| {
        AppError::provider(format!(
            "Codex app-server response {expected_id} has neither result nor error: {line}"
        ))
    })
}

pub fn project_thread_messages(thread: &Value) -> AppResult<Vec<CodexDialogueMessage>> {
    let thread_id = thread
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::parse("Codex thread payload is missing id."))?;
    let turns = thread
        .get("turns")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::parse("Codex thread payload is missing turns."))?;
    Ok(project_turn_messages(thread_id, turns))
}

fn generated_image_attachment(item: &Value) -> Option<Attachment> {
    if item.get("status").and_then(Value::as_str) != Some("completed") {
        return None;
    }
    let result = item.get("result").and_then(Value::as_str)?.trim();
    if result.is_empty() {
        return None;
    }
    let path = item
        .get("savedPath")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mime = match std::path::Path::new(&path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => "jpeg",
        Some("webp") => "webp",
        _ => "png",
    };
    let data_url = if result.starts_with("data:image/") {
        result.to_string()
    } else {
        format!("data:image/{mime};base64,{result}")
    };
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("generated-concept.png")
        .to_string();
    Some(Attachment {
        path,
        name,
        explanation: "Generated concept sketch.".to_string(),
        data_url: Some(data_url),
        kind: AttachmentKind::Image,
    })
}

fn completed_ecky_sketch_tool_result(item: &Value) -> Option<(String, Vec<Attachment>)> {
    if item.get("status").and_then(Value::as_str) != Some("completed") {
        return None;
    }
    let server = string_field(item, &["server", "serverName"])?;
    if !server.contains("ecky") {
        return None;
    }
    let tool = string_field(item, &["tool", "toolName", "name"])?;
    if !tool.contains("sketch") {
        return None;
    }
    let result = item.get("result")?;
    let structured = result.get("structuredContent").unwrap_or(result);
    let payload = structured.get("data").unwrap_or(structured);
    let document = payload
        .get("sketchDocument")
        .or_else(|| payload.get("document"))?;
    let document_id = string_field(document, &["documentId", "id"])?;
    if document_id.trim().is_empty() {
        return None;
    }
    let sketch_count = document
        .get("sketches")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let noun = if sketch_count == 1 {
        "sketch"
    } else {
        "sketches"
    };
    let attachments = result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("image"))
        .filter_map(|content| {
            let mime_type = content.get("mimeType").and_then(Value::as_str)?;
            let data = content.get("data").and_then(Value::as_str)?.trim();
            (!data.is_empty()).then(|| Attachment {
                path: String::new(),
                name: "ecky-sketch-preview".to_string(),
                explanation: "Ecky sketch preview.".to_string(),
                data_url: Some(format!("data:{mime_type};base64,{data}")),
                kind: AttachmentKind::Image,
            })
        })
        .collect();
    Some((
        format!("Ecky sketch draft created · {document_id} · {sketch_count} {noun}."),
        attachments,
    ))
}

pub fn project_turn_messages(thread_id: &str, turns: &[Value]) -> Vec<CodexDialogueMessage> {
    let mut messages = Vec::new();
    let mut ordered_turns = turns.iter().collect::<Vec<_>>();
    ordered_turns.sort_by(|left, right| {
        let left_started = left
            .get("startedAt")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let right_started = right
            .get("startedAt")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        left_started.cmp(&right_started).then_with(|| {
            left.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .cmp(right.get("id").and_then(Value::as_str).unwrap_or_default())
        })
    });
    for turn in ordered_turns {
        let turn_id = turn
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unknown-turn");
        let started_at = turn
            .get("startedAt")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let completed_at = turn
            .get("completedAt")
            .and_then(Value::as_i64)
            .unwrap_or(started_at);
        let status = match turn.get("status").and_then(Value::as_str) {
            Some("completed") => "success",
            Some("inProgress") => "pending",
            Some("failed" | "interrupted") => "error",
            _ => "pending",
        };
        let Some(items) = turn.get("items").and_then(Value::as_array) else {
            continue;
        };
        let mut user_ordinal = 0usize;
        let mut user_messages = Vec::new();
        let mut assistant_messages = Vec::new();
        for item in items {
            let Some(item_id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            match item.get("type").and_then(Value::as_str) {
                Some("userMessage") => {
                    let item_content = item.get("content").and_then(Value::as_array);
                    let content = item_content
                        .into_iter()
                        .flatten()
                        .filter(|input| input.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|input| input.get("text").and_then(Value::as_str))
                        .filter(|text| !text.trim().is_empty())
                        .map(|text| {
                            crate::provider_turn::unwrap_user_message(text)
                                .unwrap_or_else(|| text.to_string())
                        })
                        .filter(|text| {
                            !crate::services::codex_takeover::is_realtime_delegation(text)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let attachments = item_content
                        .into_iter()
                        .flatten()
                        .filter_map(|input| match input.get("type").and_then(Value::as_str) {
                            Some("image") => input
                                .get("url")
                                .and_then(Value::as_str)
                                .filter(|url| !url.trim().is_empty())
                                .map(|url| Attachment {
                                    path: String::new(),
                                    name: "image".to_string(),
                                    explanation: String::new(),
                                    data_url: Some(url.to_string()),
                                    kind: AttachmentKind::Image,
                                }),
                            Some("localImage") => input
                                .get("path")
                                .and_then(Value::as_str)
                                .filter(|path| !path.trim().is_empty())
                                .map(|path| Attachment {
                                    path: path.to_string(),
                                    name: std::path::Path::new(path)
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("image")
                                        .to_string(),
                                    explanation: String::new(),
                                    data_url: None,
                                    kind: AttachmentKind::Image,
                                }),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    if !content.is_empty() || !attachments.is_empty() {
                        user_messages.push(CodexDialogueMessage {
                            id: format!("codex:{thread_id}:{turn_id}:user:{user_ordinal}"),
                            role: "user".to_string(),
                            content,
                            status: "success".to_string(),
                            timestamp: started_at,
                            attachments,
                            provider_event_kind: None,
                        });
                        user_ordinal += 1;
                    }
                }
                Some("agentMessage") => {
                    let text = item.get("text").and_then(Value::as_str).unwrap_or_default();
                    if !text.trim().is_empty() {
                        assistant_messages.push(CodexDialogueMessage {
                            id: format!("codex:{thread_id}:{turn_id}:assistant:{item_id}"),
                            role: "assistant".to_string(),
                            content: text.to_string(),
                            status: status.to_string(),
                            timestamp: completed_at,
                            attachments: Vec::new(),
                            provider_event_kind: None,
                        });
                    }
                }
                Some("imageGeneration") => {
                    if let Some(attachment) = generated_image_attachment(item) {
                        assistant_messages.push(CodexDialogueMessage {
                            id: format!("codex:{thread_id}:{turn_id}:image:{item_id}"),
                            role: "assistant".to_string(),
                            content: "Generated concept sketch.".to_string(),
                            status: status.to_string(),
                            timestamp: completed_at,
                            attachments: vec![attachment],
                            provider_event_kind: None,
                        });
                    }
                }
                Some("mcpToolCall") => {
                    if let Some((content, attachments)) = completed_ecky_sketch_tool_result(item) {
                        assistant_messages.push(CodexDialogueMessage {
                            id: format!("codex:{thread_id}:{turn_id}:mcp:{item_id}"),
                            role: "assistant".to_string(),
                            content,
                            status: status.to_string(),
                            timestamp: completed_at,
                            attachments,
                            provider_event_kind: None,
                        });
                    }
                }
                _ => {}
            }
        }
        messages.extend(user_messages);
        messages.extend(assistant_messages);
    }
    messages
}

pub fn project_realtime_transcript(
    thread_id: &str,
    item: &Value,
    timestamp: i64,
) -> Option<CodexDialogueMessage> {
    if item.get("type")?.as_str()? != "transcriptSegment" {
        return None;
    }
    let role = item.get("role")?.as_str()?;
    if !matches!(role, "user" | "assistant") {
        return None;
    }
    let text = item.get("text")?.as_str()?;
    if text.trim().is_empty() {
        return None;
    }
    Some(CodexDialogueMessage {
        id: format!("codex:{thread_id}:realtime:{}", item.get("id")?.as_str()?),
        role: role.to_string(),
        content: text.to_string(),
        status: "success".to_string(),
        timestamp,
        attachments: Vec::new(),
        provider_event_kind: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn canceled_voice_start_cannot_launch_a_late_provider_session() {
        let supervisor = CodexAppServerSupervisor::new();
        supervisor.cancel_realtime_session("canceled-session").await;
        let error = supervisor
            .start_realtime("thread", "canceled-session", "offer")
            .await
            .unwrap_err();
        assert!(error.message.contains("canceled"));
        assert!(supervisor.inner.state.lock().await.process.is_none());
    }

    #[test]
    fn voice_transcript_projection_keeps_native_item_identity_and_roles() {
        let item = json!({"id":"spoken-1", "realtimeSessionId":"session", "type":"transcriptSegment", "role":"assistant", "text":"Привет"});
        let message = project_realtime_transcript("thread", &item, 42).unwrap();
        assert_eq!(message.id, "codex:thread:realtime:spoken-1");
        assert_eq!(message.role, "assistant");
        assert_eq!(message.content, "Привет");
        assert_eq!(message.status, "success");
        assert!(project_realtime_transcript(
            "thread",
            &json!({"id":"started", "type":"realtimeSessionStarted"}),
            42
        )
        .is_none());
    }

    #[test]
    fn codex_eval_capture_reserves_incomplete_and_terminal_events_at_limit() {
        let seed = crate::llm_eval::EvalRunSeed {
            run_id: "run-limit".into(),
            thread_id: "ecky-thread".into(),
            external_thread_id: "codex-thread".into(),
            provider: "codex".into(),
            model: None,
            effort: None,
            prompt_version: "codex-v1".into(),
            prompt: "prompt".into(),
            starting_version_id: None,
            starting_input_digest: None,
            expected_red_rounds: 0,
            turn_intent: crate::provider_turn::ProviderTurnIntent::Modify,
            answer_first_required: false,
            jev_route: None,
        };
        let mut capture = ActiveCodexEval {
            seed,
            redaction_secret: None,
            turn_id: "turn-1".into(),
            started_at: 1,
            events: Vec::new(),
            invocation_steps: HashMap::new(),
            seen_invocation_states: HashSet::new(),
            assistant_message_items: HashSet::new(),
            next_step_index: 0,
            omitted_reasons: HashMap::new(),
        };
        for _ in 0..(CODEX_EVAL_EVENT_LIMIT + 10) {
            append_codex_eval_event(
                &mut capture,
                crate::llm_eval::EvalEvent {
                    sequence: 0,
                    step_index: None,
                    kind: crate::llm_eval::EvalEventKind::Assistant,
                    state: "delta".into(),
                    name: Some("agentMessage".into()),
                    summary: None,
                    input: None,
                    output: None,
                    error: None,
                    occurred_at: 2,
                },
            );
        }
        let run = finish_codex_eval_capture(
            capture,
            3,
            "success",
            None,
            Some(serde_json::json!({"status": "completed"})),
        );
        assert_eq!(run.events.len(), CODEX_EVAL_EVENT_LIMIT);
        assert_eq!(
            run.events.last().unwrap().name.as_deref(),
            Some("turn/terminal")
        );
        assert!(run
            .events
            .iter()
            .any(|event| event.name.as_deref() == Some("capture-incomplete")));
        assert!(run
            .events
            .iter()
            .zip(run.events.iter().skip(1))
            .all(|(a, b)| a.sequence < b.sequence));
    }

    #[test]
    fn codex_eval_capture_redacts_configured_key_before_edn_and_markdown_export() {
        let secret = "exact-jev-token-123";
        let seed = crate::llm_eval::EvalRunSeed {
            run_id: "run-redaction".into(),
            thread_id: "ecky-thread".into(),
            external_thread_id: "codex-thread".into(),
            provider: "codex".into(),
            model: None,
            effort: None,
            prompt_version: "codex-v1".into(),
            prompt: format!("user asked {secret}"),
            starting_version_id: None,
            starting_input_digest: None,
            expected_red_rounds: 0,
            turn_intent: crate::provider_turn::ProviderTurnIntent::Answer,
            answer_first_required: false,
            jev_route: Some(crate::llm_eval::EvalJevRoute {
                policy_version: "route-v1".into(),
                intent_confidence_threshold: 0.65,
                intent_margin_threshold: 0.15,
                model_confidence_threshold: 0.65,
                model_margin_threshold: 0.15,
                intent_confidence: 0.9,
                intent_probabilities: std::collections::BTreeMap::new(),
                answer_requested: true,
                answer_first: false,
                model_ceiling: None,
                model_confidence: None,
                model_probabilities: std::collections::BTreeMap::new(),
                model_reason: format!("reason {secret}"),
                context_truncated: false,
                current_prompt_truncated: false,
                classifier_input_tokens: Some(1),
                classifier_output_tokens: Some(1),
                classifier_model: Some(format!("model {secret}")),
                classifier_latency_ms: Some(1),
                model_catalog_version: None,
                model_catalog_valid_until: None,
                native_tool_coverage: crate::llm_eval::unknown_native_tool_coverage(),
            }),
        };
        let mut object = serde_json::Map::new();
        object.insert(secret.to_string(), Value::String(secret.into()));
        let mut response_payload =
            crate::llm_eval::EvalPayload::new(Value::String(format!("answer {secret}")));
        response_payload.truncated = true;
        let capture = ActiveCodexEval {
            seed: seed.clone(),
            redaction_secret: Some(secret.into()),
            turn_id: "turn-1".into(),
            started_at: 1,
            events: vec![
                crate::llm_eval::EvalEvent {
                    sequence: 1,
                    step_index: None,
                    kind: crate::llm_eval::EvalEventKind::Tool,
                    state: "completed".into(),
                    name: Some("tool".into()),
                    summary: Some(format!("summary {secret}")),
                    input: Some(crate::llm_eval::EvalPayload::new(Value::Object(object))),
                    output: None,
                    error: Some(format!("error {secret}")),
                    occurred_at: 2,
                },
                crate::llm_eval::EvalEvent {
                    sequence: 2,
                    step_index: None,
                    kind: crate::llm_eval::EvalEventKind::Assistant,
                    state: "completed".into(),
                    name: Some("agentMessage".into()),
                    summary: None,
                    input: None,
                    output: Some(response_payload),
                    error: None,
                    occurred_at: 2,
                },
            ],
            invocation_steps: HashMap::new(),
            seen_invocation_states: HashSet::new(),
            assistant_message_items: HashSet::new(),
            next_step_index: 0,
            omitted_reasons: HashMap::new(),
        };
        let run = finish_codex_eval_capture(
            capture,
            3,
            "success",
            Some(format!("terminal error {secret}")),
            Some(serde_json::json!({"echo": secret})),
        );
        assert_eq!(run.thread_id, "ecky-thread");
        assert_eq!(run.response.as_deref(), Some("answer [REDACTED]"));
        let output = run.events[1].output.as_ref().unwrap();
        assert!(output.truncated);
        assert!(matches!(
            output.value,
            crate::llm_eval::EvalValue::String(_)
        ));
        let serialized = serde_json::to_string(&run).unwrap();
        assert!(!serialized.contains(secret));
        let failed = finish_pending_codex_eval(
            PendingCodexEval {
                seed,
                started_at: 1,
                redaction_secret: Some(secret.into()),
            },
            String::new(),
            4,
            "error",
            Some(format!("start failed {secret}")),
        );
        assert!(
            failed.turn_id.is_empty(),
            "failed turn/start has no provider turn ID"
        );
        assert_eq!(failed.status, "error");
        assert!(failed.events.is_empty());
        assert!(!serde_json::to_string(&failed).unwrap().contains(secret));

        let root =
            std::env::temp_dir().join(format!("ecky-eval-redaction-{}", uuid::Uuid::new_v4()));
        let files = crate::llm_eval::persist_run(&root, &run).unwrap();
        for path in [files.run_edn, files.trajectory_edn, files.report_md] {
            let content = std::fs::read_to_string(path).unwrap();
            assert!(!content.contains(secret));
        }
        let failed_files = crate::llm_eval::persist_run(&root, &failed).unwrap();
        let persisted_failed = crate::llm_eval::read_run(&failed_files.run_dir).unwrap();
        assert!(persisted_failed.turn_id.is_empty());
        assert_eq!(persisted_failed.status, "error");
        assert!(persisted_failed.events.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn binding(bootstrap_version: u32) -> crate::contracts::CodexTakeoverBinding {
        crate::contracts::CodexTakeoverBinding {
            ecky_thread_id: "ecky-thread".to_string(),
            codex_thread_id: "codex-thread".to_string(),
            label: "Dryer".to_string(),
            cwd: "/tmp/dryer".to_string(),
            bootstrap_version,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn start_and_resume_use_same_shared_ecky_policy() {
        let start = start_params(
            "ecky-thread",
            "Dryer",
            "/tmp/dryer",
            "http://127.0.0.1:1234/mcp",
            "handoff",
            None,
        );
        let resume = resume_params(
            &binding(crate::services::codex_takeover::CODEX_BOOTSTRAP_VERSION),
            "Dryer",
            "http://127.0.0.1:1234/mcp",
            "handoff",
            None,
        );
        let start_instructions = start["developerInstructions"].as_str().unwrap();
        let resume_instructions = resume["developerInstructions"].as_str().unwrap();

        assert_eq!(start_instructions, resume_instructions);
        assert!(start_instructions.contains(crate::mcp::authoring::authoring_card_text()));
        assert!(start_instructions.contains("Do not call `thread_borrow` for this thread"));
        assert!(start_instructions.contains("edit that exact file"));
        assert!(!start_instructions.contains("preview -> commit"));
        // Baseline thread creation/resume must preserve Codex's prior tool
        // configuration. Jev must prove a scoped native boundary before turn
        // dispatch; global config overrides would silently change disabled mode.
        for params in [&start, &resume] {
            assert!(params["config"]["features.shell_tool"].is_null());
            assert!(params["config"]["features.unified_exec"].is_null());
            assert!(params["config"]["features.multi_agent"].is_null());
            assert!(params["config"]["features.apps"].is_null());
            assert!(params["config"]["tools.view_image"].is_null());
            assert!(params["config"]["web_search"].is_null());
        }
    }

    #[test]
    fn routed_resume_scopes_tool_flags_without_mutating_normal_config() {
        let policy = crate::provider_turn::ProviderTurnPolicy::routed(
            crate::provider_turn::ProviderTurnIntent::Answer,
            false,
        );
        let params = resume_params_with_routed_hook(
            &binding(crate::services::codex_takeover::CODEX_BOOTSTRAP_VERSION),
            "Dryer",
            "http://127.0.0.1:39249/mcp",
            "handoff",
            None,
            policy,
            "/Applications/Ecky.app/Contents/MacOS/ecky --ecky-codex-pre-tool-hook http://127.0.0.1:39249/codex-pre-tool-hook || exit 2",
        );
        assert!(params["config"]["hooks"].is_null());
        assert_eq!(params["config"]["features.shell_tool"], false);
        assert_eq!(params["config"]["features.view_image"], false);
        assert!(params["config"]["tools.view_image"].is_null());
        assert_eq!(params["config"]["web_search"], "disabled");
    }

    #[test]
    fn routed_config_baseline_uses_effective_features_and_web_search_defaults() {
        let features = json!({
            "shell_tool": true,
            "unified_exec": false,
            "multi_agent": true,
            "apps": false,
            "view_image": true
        });
        let defaults = native_config_baseline(&json!({}), &features).unwrap();
        assert_eq!(defaults["features.shell_tool"], true);
        assert_eq!(defaults["features.unified_exec"], false);
        assert_eq!(defaults["features.view_image"], true);
        assert_eq!(defaults["web_search"], "cached");
        let explicit = native_config_baseline(&json!({"web_search": "live"}), &features).unwrap();
        assert_eq!(explicit["web_search"], "live");
    }

    #[tokio::test]
    async fn accepted_routed_resume_blocks_activation_restore_before_turn_start() {
        let supervisor = CodexAppServerSupervisor::new();
        let thread_id = "codex-thread-routed-pending";
        let original_baseline = json!({
            "features.shell_tool": true,
            "features.unified_exec": true,
            "features.view_image": true,
            "web_search": "live"
        });
        {
            let mut state = supervisor.inner.state.lock().await;
            state
                .routed_config_baselines
                .insert(thread_id.to_string(), original_baseline.clone());
            state.routed_delivery_pending.insert(thread_id.to_string());
            state
                .runtimes
                .insert(thread_id.to_string(), CodexTakeoverRuntime::default());
            assert!(state.runtimes[thread_id].active_turn_id.is_none());
            assert!(routed_delivery_blocks_normal_resume(&state, thread_id));
            // Activation takes the same resume mutex as routed dispatch, sees
            // this marker before reading/removing the restore baseline.
            assert_eq!(state.routed_config_baselines[thread_id], original_baseline);

            // Turn/start response transitions atomically from pending to active.
            state.routed_delivery_pending.remove(thread_id);
            state.runtimes.get_mut(thread_id).unwrap().active_turn_id = Some("turn-1".into());
            assert!(routed_delivery_blocks_normal_resume(&state, thread_id));
            assert_eq!(state.routed_config_baselines[thread_id], original_baseline);

            // Only terminal completion makes baseline restoration eligible.
            state.runtimes.get_mut(thread_id).unwrap().active_turn_id = None;
            assert!(!routed_delivery_blocks_normal_resume(&state, thread_id));
        }
    }

    #[tokio::test]
    async fn process_exit_before_turn_start_persists_failure_without_provider_turn_id() {
        let supervisor = CodexAppServerSupervisor::new();
        let thread_id = "ecky-process-exit";
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = Arc::new(Mutex::new(child.stdin.take().unwrap()));
        let (kill, mut kill_request) = mpsc::channel(1);
        let generation = 9;
        {
            let mut state = supervisor.inner.state.lock().await;
            state.process = Some(SupervisorProcess {
                generation,
                stdin,
                kill,
                initialized: true,
            });
            state.pending_eval_seeds.insert(
                thread_id.into(),
                PendingCodexEval {
                    seed: crate::llm_eval::EvalRunSeed {
                        run_id: "run-process-exit".into(),
                        thread_id: thread_id.into(),
                        external_thread_id: "codex-process-exit".into(),
                        provider: "codex".into(),
                        model: None,
                        effort: None,
                        prompt_version: "codex-v1".into(),
                        prompt: "process exits before turn response".into(),
                        starting_version_id: None,
                        starting_input_digest: None,
                        expected_red_rounds: 0,
                        turn_intent: crate::provider_turn::ProviderTurnIntent::Answer,
                        answer_first_required: false,
                        jev_route: None,
                    },
                    started_at: 10,
                    redaction_secret: None,
                },
            );
            state
                .runtimes
                .insert(thread_id.into(), CodexTakeoverRuntime::default());
        }

        supervisor
            .invalidate_process(generation, AppError::provider("fake Codex child exited"))
            .await;
        assert!(kill_request.try_recv().is_ok());
        child.kill().await.unwrap();
        let failed = supervisor.completed_eval_runs().await.pop().unwrap();
        assert!(
            failed.turn_id.is_empty(),
            "unknown provider turn must stay unknown"
        );
        assert_eq!(failed.status, "error");
        assert!(failed.events.is_empty());
        assert!(failed.raw_error.is_some());
        let root = std::env::temp_dir().join(format!("ecky-process-exit-{}", uuid::Uuid::new_v4()));
        let files = crate::llm_eval::persist_run(&root, &failed).unwrap();
        let persisted = crate::llm_eval::read_run(&files.run_dir).unwrap();
        assert!(persisted.turn_id.is_empty());
        assert_eq!(persisted.status, "error");
        assert!(persisted.events.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn experimental_feature_pagination_rejects_repeated_cursor() {
        let mut seen = HashSet::new();
        assert_eq!(
            next_feature_cursor(&json!({"nextCursor": "page-2"}), &mut seen).unwrap(),
            Some("page-2".into())
        );
        assert!(next_feature_cursor(&json!({"nextCursor": "page-2"}), &mut seen).is_err());
        assert_eq!(
            next_feature_cursor(&json!({"nextCursor": null}), &mut seen).unwrap(),
            None
        );
    }

    #[test]
    fn startup_hook_override_is_stable_and_matches_all_tools() {
        let command = codex_pre_tool_hook_command().unwrap();
        let config = codex_startup_hook_override(&command).unwrap();
        assert!(config.contains("hooks.PreToolUse"));
        assert!(config.contains("matcher=\".*\""));
        assert!(config.contains("timeout=5"));
        assert_eq!(command, codex_pre_tool_hook_command().unwrap());
    }

    #[test]
    fn review_command_quotes_executable_cwd_and_exact_startup_override() {
        let executable =
            std::path::Path::new("/Applications/Codex O'Neil.app/Contents/MacOS/codex");
        let cwd = "/Users/test/Project O'Neil";
        let hook_command = "/Applications/Ecky O'Neil.app/Contents/MacOS/ecky --ecky-codex-pre-tool-hook || exit 2";
        let diagnostic = codex_hook_review_diagnostic(executable, cwd, hook_command).unwrap();
        let exact_override = codex_startup_hook_override(hook_command).unwrap();
        let shell_quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let expected = format!(
            "{} -C {} -c {}",
            shell_quote(executable.to_str().unwrap()),
            shell_quote(cwd),
            shell_quote(&exact_override)
        );
        assert!(diagnostic.contains(&expected), "{diagnostic}");
        assert!(diagnostic.contains("/hooks"), "{diagnostic}");
        assert!(
            diagnostic.contains("trust only this exact hook"),
            "{diagnostic}"
        );
    }

    #[test]
    fn missing_callback_descriptor_keeps_disabled_startup_unchanged() {
        let directory =
            std::env::temp_dir().join(format!("ecky-hook-start-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let descriptor = directory.join("missing.json");
        assert_eq!(
            codex_startup_hook_override_if_available(&descriptor).unwrap(),
            None
        );
        crate::services::codex_pre_tool_hook::write_runtime_descriptor_at(
            &descriptor,
            "http://127.0.0.1:39249/codex-pre-tool-hook/test-token",
            "test-token",
        )
        .unwrap();
        assert!(codex_startup_hook_override_if_available(&descriptor)
            .unwrap()
            .is_some());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn native_hook_gate_requires_exact_enabled_trusted_hash() {
        let command =
            "ecky --ecky-codex-pre-tool-hook http://127.0.0.1:39249/codex-pre-tool-hook || exit 2";
        let base_hook = json!({
            "eventName": "preToolUse",
            "enabled": true,
            "source": "sessionFlags",
            "matcher": ".*",
            "async": false,
            "timeoutSec": 5,
            "trustStatus": "trusted",
            "currentHash": "sha256:abc",
            "handlerType": "command",
            "command": command
        });
        let response = json!({"data":[{"cwd":"/tmp/dryer","hooks":[base_hook]}]});
        assert_eq!(
            verify_owned_pre_tool_hook(&response, "/tmp/dryer", command).unwrap(),
            "sha256:abc"
        );

        for (field, value) in [
            ("trustStatus", json!("untrusted")),
            ("enabled", json!(false)),
            ("currentHash", json!("")),
            ("source", json!("user")),
            ("command", json!("other command")),
            ("matcher", json!("^Bash$")),
            ("async", json!(true)),
            ("timeoutSec", json!(30)),
        ] {
            let mut bad = base_hook.clone();
            bad[field] = value;
            assert!(verify_owned_pre_tool_hook(
                &json!({"data":[{"cwd":"/tmp/dryer","hooks":[bad]}]}),
                "/tmp/dryer",
                command
            )
            .is_err());
        }
        assert!(verify_owned_pre_tool_hook(&response, "/tmp/other", command).is_err());
    }

    #[test]
    fn bootstrap_refresh_or_new_process_generation_resumes_once() {
        assert!(should_skip_resume(Some(9), 9, false, false));
        assert!(!should_skip_resume(Some(9), 9, true, false));
        assert!(!should_skip_resume(Some(9), 9, false, true));
        assert!(!should_skip_resume(Some(8), 9, false, false));
    }

    #[test]
    fn codex_turn_policy_is_prompted_and_enforced_by_sandbox() {
        let answer_policy = crate::provider_turn::ProviderTurnPolicy::for_intent(
            crate::provider_turn::ProviderTurnIntent::Answer,
        );
        let answer = turn_start_params("codex-thread", "что происходит?", None, &[], answer_policy);
        assert_eq!(answer["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(answer["approvalPolicy"], "never");
        assert!(answer["input"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Intent: ANSWER"));
        let resumed = resume_params_with_policy(
            &binding(crate::services::codex_takeover::CODEX_BOOTSTRAP_VERSION),
            "Dryer",
            "http://127.0.0.1:1234/mcp",
            "handoff",
            None,
            answer_policy,
        );
        assert!(resumed["config"]["mcp_servers.ecky_provider_mcp.url"]
            .as_str()
            .unwrap()
            .contains("providerTurnIntent=answer"));

        let modify = turn_start_params(
            "codex-thread",
            "измени размер",
            None,
            &[],
            crate::provider_turn::ProviderTurnPolicy::for_intent(
                crate::provider_turn::ProviderTurnIntent::Modify,
            ),
        );
        assert_eq!(modify["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(modify["approvalPolicy"], "on-request");
    }

    #[test]
    fn user_input_uses_native_inline_and_local_image_blocks_and_cad_context() {
        let input = build_user_input(
            "Review these files.",
            &[
                crate::contracts::Attachment {
                    path: String::new(),
                    name: "inline.png".to_string(),
                    explanation: "Match the bearing shoulder.".to_string(),
                    data_url: Some("data:image/png;base64,abc".to_string()),
                    kind: crate::contracts::AttachmentKind::Image,
                },
                crate::contracts::Attachment {
                    path: "/tmp/local.png".to_string(),
                    name: "local.png".to_string(),
                    explanation: String::new(),
                    data_url: None,
                    kind: crate::contracts::AttachmentKind::Image,
                },
                crate::contracts::Attachment {
                    path: "/tmp/model.step".to_string(),
                    name: "model.step".to_string(),
                    explanation: String::new(),
                    data_url: None,
                    kind: crate::contracts::AttachmentKind::Cad,
                },
            ],
        );

        assert_eq!(input[0]["type"], "text");
        assert!(input[0]["text"]
            .as_str()
            .unwrap()
            .contains("/tmp/model.step"));
        assert!(input[0]["text"]
            .as_str()
            .unwrap()
            .contains("inline.png: Match the bearing shoulder."));
        assert_eq!(
            input[1],
            json!({"type": "image", "url": "data:image/png;base64,abc"})
        );
        assert_eq!(
            input[2],
            json!({"type": "localImage", "path": "/tmp/local.png"})
        );
    }

    #[test]
    fn provider_thread_projection_keeps_inline_and_local_user_images() {
        let messages = project_turn_messages(
            "codex-thread",
            &[json!({
                "id": "turn-1",
                "status": "completed",
                "startedAt": 10,
                "completedAt": 11,
                "items": [{
                    "id": "user-item",
                    "type": "userMessage",
                    "content": [
                        {"type": "text", "text": "Use these."},
                        {"type": "image", "url": "data:image/png;base64,abc"},
                        {"type": "localImage", "path": "/tmp/reference.png"}
                    ]
                }]
            })],
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].attachments.len(), 2);
        assert_eq!(
            messages[0].attachments[0].data_url.as_deref(),
            Some("data:image/png;base64,abc")
        );
        assert_eq!(messages[0].attachments[1].name, "reference.png");
    }

    #[test]
    fn provider_thread_projection_hides_internal_turn_policy_but_keeps_user_text() {
        let original = "Покажи, что изменилось после проверки.";
        let wrapped = crate::provider_turn::ProviderTurnPolicy::prompt_based()
            .wrap_user_message_for_turn(original, "00000000-0000-4000-8000-000000000001");
        let provider_input = build_user_input(
            &wrapped,
            &[crate::contracts::Attachment {
                path: String::new(),
                name: "reference.png".to_string(),
                explanation: "Keep the rim visible.".to_string(),
                data_url: Some("data:image/png;base64,abc".to_string()),
                kind: crate::contracts::AttachmentKind::Image,
            }],
        );
        let messages = project_turn_messages(
            "codex-thread",
            &[json!({
                "id": "turn-1",
                "status": "completed",
                "startedAt": 10,
                "completedAt": 11,
                "items": [{
                    "id": "user-item",
                    "type": "userMessage",
                    "content": [{"type": "text", "text": provider_input[0]["text"]}]
                }]
            })],
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, original);
        assert!(!messages[0].content.contains("ATTACHMENT NOTES"));
    }

    #[test]
    fn provider_thread_projection_keeps_generated_concept_sketches() {
        let messages = project_turn_messages(
            "codex-thread",
            &[json!({
                "id": "turn-1",
                "status": "completed",
                "startedAt": 10,
                "completedAt": 11,
                "items": [{
                    "id": "generated-sketch",
                    "type": "imageGeneration",
                    "status": "completed",
                    "savedPath": "/tmp/engine-sketch.png",
                    "result": "iVBORw0KGgo="
                }]
            })],
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        assert_eq!(messages[0].content, "Generated concept sketch.");
        assert_eq!(
            messages[0].attachments[0].data_url.as_deref(),
            Some("data:image/png;base64,iVBORw0KGgo=")
        );
    }

    #[test]
    fn provider_thread_projection_omits_incomplete_generated_images() {
        let messages = project_turn_messages(
            "codex-thread",
            &[json!({
                "id": "turn-1",
                "status": "inProgress",
                "startedAt": 10,
                "items": [{
                    "id": "generated-sketch",
                    "type": "imageGeneration",
                    "status": "inProgress",
                    "result": "iVBORw0KGgo="
                }]
            })],
        );

        assert!(messages.is_empty());
    }

    #[test]
    fn provider_thread_projection_keeps_ecky_sketch_tool_results() {
        let messages = project_turn_messages(
            "codex-thread",
            &[json!({
                "id": "turn-1",
                "status": "completed",
                "startedAt": 10,
                "completedAt": 11,
                "items": [{
                    "id": "sketch-result",
                    "type": "mcpToolCall",
                    "server": "ecky_provider_mcp",
                    "tool": "sketch_preview_create",
                    "status": "completed",
                    "result": {
                        "structuredContent": {
                            "sketchDocument": {
                                "documentId": "engine-layout",
                                "sketches": [{"sketchId": "block-profile"}]
                            },
                            "draftSource": {"source": "(model (part engine-block ...))"},
                            "artifactBundle": {"modelStlPath": "/tmp/engine-layout.stl"}
                        }
                    }
                }]
            })],
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        assert_eq!(
            messages[0].content,
            "Ecky sketch draft created · engine-layout · 1 sketch."
        );
        assert!(messages[0].content.contains("engine-layout"));
    }

    #[test]
    fn provider_thread_projection_omits_incomplete_ecky_sketch_tool_results() {
        let messages = project_turn_messages(
            "codex-thread",
            &[json!({
                "id": "turn-1",
                "status": "inProgress",
                "startedAt": 10,
                "items": [{
                    "id": "sketch-result",
                    "type": "mcpToolCall",
                    "server": "ecky_provider_mcp",
                    "tool": "sketch_preview_create",
                    "status": "inProgress",
                    "result": {
                        "structuredContent": {
                            "sketchDocument": {
                                "documentId": "engine-layout",
                                "sketches": [{"sketchId": "block-profile"}]
                            }
                        }
                    }
                }]
            })],
        );

        assert!(messages.is_empty());
    }
}
