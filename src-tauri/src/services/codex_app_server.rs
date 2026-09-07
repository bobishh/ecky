use crate::contracts::{
    AppError, AppResult, Attachment, AttachmentKind, CodexDialogueMessage, CodexMessagePage,
    CodexTakeoverRuntime, CodexThreadSummary, ProviderTurnTrace,
};
use crate::services::provider_executable::resolve_provider_executable;
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
const LIVE_MESSAGE_LIMIT: usize = 256;
const LIVE_MESSAGE_CHAR_LIMIT: usize = 16_384;
const TURN_TRACE_LIMIT: usize = 24;
#[derive(Clone)]
pub struct CodexAppServerSupervisor {
    inner: Arc<SupervisorInner>,
}

struct SupervisorInner {
    state: Mutex<SupervisorState>,
    startup: Mutex<()>,
    resume: Mutex<()>,
}

struct SupervisorState {
    process: Option<SupervisorProcess>,
    generation: u64,
    next_request_id: u64,
    pending: HashMap<u64, oneshot::Sender<AppResult<Value>>>,
    stderr_tail: VecDeque<String>,
    runtimes: HashMap<String, CodexTakeoverRuntime>,
    live_messages: HashMap<String, Vec<CodexDialogueMessage>>,
    turn_traces: HashMap<String, Vec<ProviderTurnTrace>>,
    resumed_threads: HashMap<String, u64>,
    terminal_turns: HashSet<String>,
    terminal_turn_order: VecDeque<String>,
    app_handle: Option<tauri::AppHandle>,
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
                    process: None,
                    generation: 0,
                    next_request_id: 1,
                    pending: HashMap::new(),
                    stderr_tail: VecDeque::new(),
                    runtimes: HashMap::new(),
                    live_messages: HashMap::new(),
                    turn_traces: HashMap::new(),
                    resumed_threads: HashMap::new(),
                    terminal_turns: HashSet::new(),
                    terminal_turn_order: VecDeque::new(),
                    app_handle: None,
                }),
                startup: Mutex::new(()),
                resume: Mutex::new(()),
            }),
        }
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

        let resolved = resolve_provider_executable("codex", "ECKY_CODEX_BIN", "Codex CLI")?;
        let mut child = Command::new(&resolved.path)
            .arg("app-server")
            .arg("--stdio")
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
        let (app_handle, live_messages, turn_traces, runtime_snapshot) = {
            let mut state = self.inner.state.lock().await;
            if state
                .process
                .as_ref()
                .is_none_or(|process| process.generation != generation)
            {
                return;
            }
            let (live_messages, turn_traces, runtime_snapshot) =
                if let Some(thread_id) = thread_id.as_deref() {
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
        let (pending, app_handle, thread_ids, kill) = {
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
            let terminal_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
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
                    .unwrap_or_else(|| format!("process-error-{terminal_at}"));
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
            (pending, state.app_handle.clone(), thread_ids, kill)
        };
        if let Some(kill) = kill {
            let _ = kill.send(()).await;
        }
        for sender in pending {
            let _ = sender.send(Err(error.clone()));
        }
        if let Some(app_handle) = app_handle {
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

    pub async fn delete_thread(&self, thread_id: &str) -> AppResult<()> {
        self.request("thread/delete", json!({ "threadId": thread_id }))
            .await?;
        let mut state = self.inner.state.lock().await;
        state.runtimes.remove(thread_id);
        state.resumed_threads.remove(thread_id);
        Ok(())
    }

    pub async fn resume_thread(
        &self,
        binding: &crate::contracts::CodexTakeoverBinding,
        project_title: &str,
        mcp_endpoint: &str,
        handoff_context: &str,
        refresh_developer_instructions: bool,
        force_writer_activation: bool,
        model: Option<&str>,
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
            if should_skip_resume(
                state.resumed_threads.get(&binding.codex_thread_id).copied(),
                generation,
                refresh_developer_instructions,
                force_writer_activation,
            ) {
                return Ok(());
            }
            generation
        };
        let result = self
            .request_started(
                "thread/resume",
                resume_params(binding, project_title, mcp_endpoint, handoff_context, model),
            )
            .await?;
        result
            .get("thread")
            .ok_or_else(|| AppError::parse("Codex thread/resume result is missing thread."))?;
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
        let mut state = self.inner.state.lock().await;
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
        let mut params = json!({
            "threadId": thread_id,
            "input": build_user_input(prompt, attachments)
        });
        if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
            params["model"] = Value::String(model.trim().to_string());
        }
        let result = self.request("turn/start", params).await?;
        let turn_id = result
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::parse("Codex turn/start result is missing turn id."))?
            .to_string();
        let mut state = self.inner.state.lock().await;
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
        self.request(
            "turn/steer",
            json!({
                "threadId": thread_id,
                "expectedTurnId": expected_turn_id,
                "input": build_user_input(prompt, attachments)
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
    force_writer_activation: bool,
) -> bool {
    !refresh_developer_instructions
        && !force_writer_activation
        && resumed_generation == Some(current_generation)
}

pub fn resume_params(
    binding: &crate::contracts::CodexTakeoverBinding,
    project_title: &str,
    mcp_endpoint: &str,
    handoff_context: &str,
    model: Option<&str>,
) -> Value {
    let mcp_endpoint =
        crate::mcp::server::provider_bound_endpoint(mcp_endpoint, &binding.ecky_thread_id);
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
            "mcp_servers.ecky_provider_mcp.default_tools_approval_mode": "approve"
        }
    });
    if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
        params["model"] = Value::String(model.trim().to_string());
    }
    params
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
            "mcp_servers.ecky_provider_mcp.default_tools_approval_mode": "approve"
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
            Some(format!("USING TOOL · {server}/{tool}"))
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(start_instructions.contains("`thread_borrow` with threadId `ecky-thread`"));
        assert!(start_instructions.contains("edit that exact file"));
        assert!(!start_instructions.contains("preview -> commit"));
    }

    #[test]
    fn bootstrap_refresh_or_open_activation_forces_resume_in_same_process_generation() {
        assert!(should_skip_resume(Some(9), 9, false, false));
        assert!(!should_skip_resume(Some(9), 9, true, false));
        assert!(!should_skip_resume(Some(9), 9, false, true));
        assert!(!should_skip_resume(Some(8), 9, false, false));
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
