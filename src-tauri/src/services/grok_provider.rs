use crate::contracts::{
    AppError, AppResult, Attachment, CodexDialogueMessage, CodexQueuedPrompt, CodexTakeoverRuntime,
    GrokMessagePage, GrokProviderBinding, ProviderEventKind, ProviderTurnTrace,
};
use crate::services::provider_executable::resolve_provider_executable;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

pub const GROK_PROVIDER_ID: &str = "grok";
pub const GROK_BOOTSTRAP_VERSION: u32 = 1;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_GRACE: Duration = Duration::from_secs(3);
const STDERR_TAIL_LINES: usize = 80;
const LIVE_MESSAGE_CHAR_LIMIT: usize = 16_384;
const TURN_TRACE_LIMIT: usize = 24;
const LOGIN_HINT: &str = "Grok is not signed in. Run `grok login` in a terminal, then send again.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokTurnResult {
    pub session_id: String,
    pub turn_id: String,
    pub status: String,
    pub response: String,
    pub error: Option<String>,
}

pub struct GrokTurnStarted {
    pub session_id: String,
    pub turn_id: String,
    pub result: oneshot::Receiver<AppResult<GrokTurnResult>>,
}

#[derive(Debug, Clone)]
pub struct GrokSessionRequest {
    pub cwd: String,
    pub model: Option<String>,
    pub mcp_url: String,
    pub resume_session_id: Option<String>,
}

#[derive(Clone)]
pub struct GrokProviderSupervisor {
    inner: Arc<GrokSupervisorInner>,
}

struct GrokSupervisorInner {
    state: Mutex<GrokSupervisorState>,
    startup: Mutex<()>,
}

struct GrokSupervisorState {
    processes: HashMap<String, GrokProcess>,
    session_index: HashMap<String, String>,
    turn_traces: HashMap<String, Vec<ProviderTurnTrace>>,
    app_handle: Option<tauri::AppHandle>,
}

struct GrokProcess {
    session_id: Option<String>,
    identity: String,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: HashMap<u64, oneshot::Sender<AppResult<Value>>>,
    next_id: u64,
    active: Option<ActivePrompt>,
    assistant_text: String,
    thought_buffer: String,
    thought_index: u32,
    runtime: CodexTakeoverRuntime,
    live_messages: Vec<CodexDialogueMessage>,
    stderr_tail: VecDeque<String>,
    kill: mpsc::Sender<()>,
    pid: u32,
}

struct ActivePrompt {
    request_id: u64,
    turn_id: String,
    result_tx: Option<oneshot::Sender<AppResult<GrokTurnResult>>>,
}

#[derive(Debug, PartialEq, Eq)]
enum AuthChoice {
    Skip,
    Method(&'static str),
    NeedsLogin,
}

impl Default for GrokProviderSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl GrokProviderSupervisor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(GrokSupervisorInner {
                state: Mutex::new(GrokSupervisorState {
                    processes: HashMap::new(),
                    session_index: HashMap::new(),
                    turn_traces: HashMap::new(),
                    app_handle: None,
                }),
                startup: Mutex::new(()),
            }),
        }
    }

    pub async fn set_app_handle(&self, app_handle: tauri::AppHandle) {
        self.inner.state.lock().await.app_handle = Some(app_handle);
    }

    pub async fn ensure_session(&self, request: GrokSessionRequest) -> AppResult<String> {
        let _startup = self.inner.startup.lock().await;
        let model = normalize_model(request.model.as_deref());
        let identity = session_identity(model.as_deref(), &request.mcp_url, &request.cwd);
        if let Some(resume) = request.resume_session_id.as_deref() {
            if let Some(key) = self.process_key(resume).await {
                if self.process_identity(&key).await.as_deref() == Some(identity.as_str()) {
                    return Ok(resume.to_string());
                }
                self.shutdown_process(&key).await;
            }
        }
        let resolved = resolve_provider_executable("grok", "ECKY_GROK_BIN", "Grok CLI")?;
        probe_grok_version(&resolved.path, &resolved.spawn_path).await?;
        let key = uuid::Uuid::new_v4().to_string();
        self.spawn_process(
            key.clone(),
            &resolved.path,
            &resolved.spawn_path,
            &request.cwd,
            model.as_deref(),
            identity,
        )
        .await?;
        match self.handshake(&key, &request).await {
            Ok(session_id) => {
                self.publish_session(&key, &session_id).await;
                Ok(session_id)
            }
            Err(error) => {
                self.shutdown_process(&key).await;
                Err(error)
            }
        }
    }

    pub async fn prompt(&self, session_id: &str, prompt: &str) -> AppResult<GrokTurnStarted> {
        if prompt.trim().is_empty() {
            return Err(AppError::validation("Grok prompt must not be empty."));
        }
        let (key, request_id, stdin, turn_id, result) = {
            let mut state = self.inner.state.lock().await;
            let key = state
                .session_index
                .get(session_id)
                .cloned()
                .ok_or_else(|| {
                    AppError::not_found(format!("Grok session {session_id} has no live process."))
                })?;
            let process = state.processes.get_mut(&key).ok_or_else(|| {
                AppError::not_found(format!("Grok session {session_id} has no live process."))
            })?;
            if process.active.is_some() {
                return Err(AppError::conflict("Grok turn is already active."));
            }
            let request_id = process.next_id;
            process.next_id += 1;
            let turn_id = uuid::Uuid::new_v4().to_string();
            let (tx, rx) = oneshot::channel();
            process.assistant_text.clear();
            process.thought_buffer.clear();
            process.thought_index = 0;
            process.live_messages.clear();
            process.runtime = CodexTakeoverRuntime {
                phase: "active".to_string(),
                active_turn_id: Some(turn_id.clone()),
                error: None,
            };
            process.active = Some(ActivePrompt {
                request_id,
                turn_id: turn_id.clone(),
                result_tx: Some(tx),
            });
            (key, request_id, process.stdin.clone(), turn_id, rx)
        };
        if let Err(error) = write_line(
            &stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "session/prompt",
                "params": {
                    "sessionId": session_id,
                    "prompt": [{ "type": "text", "text": prompt }],
                }
            }),
        )
        .await
        {
            self.clear_active(&key).await;
            return Err(error);
        }
        self.emit(&key, "turn/started").await;
        Ok(GrokTurnStarted {
            session_id: session_id.to_string(),
            turn_id,
            result,
        })
    }

    pub async fn stop_turn(&self, session_id: &str, turn_id: &str) -> AppResult<()> {
        let (key, stdin, pid) = {
            let mut state = self.inner.state.lock().await;
            let key = state
                .session_index
                .get(session_id)
                .cloned()
                .ok_or_else(|| {
                    AppError::not_found(format!("Grok session {session_id} has no live process."))
                })?;
            let process = state.processes.get_mut(&key).ok_or_else(|| {
                AppError::not_found(format!("Grok session {session_id} has no live process."))
            })?;
            if process.runtime.active_turn_id.as_deref() != Some(turn_id) {
                return Err(AppError::conflict(format!(
                    "Grok active turn changed; expected {turn_id}, current {}.",
                    process.runtime.active_turn_id.as_deref().unwrap_or("none")
                )));
            }
            process.runtime.phase = "stopping".to_string();
            (key.clone(), process.stdin.clone(), process.pid)
        };
        write_line(
            &stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": session_id }
            }),
        )
        .await?;
        let supervisor = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(STOP_GRACE).await;
            let still_active = supervisor
                .inner
                .state
                .lock()
                .await
                .processes
                .get(&key)
                .is_some_and(|process| process.runtime.active_turn_id.is_some());
            if still_active {
                signal_group(pid, libc_sigint());
            }
        });
        Ok(())
    }

    pub async fn runtime(&self, session_id: &str) -> CodexTakeoverRuntime {
        let state = self.inner.state.lock().await;
        state
            .session_index
            .get(session_id)
            .and_then(|key| state.processes.get(key))
            .map(|process| process.runtime.clone())
            .unwrap_or_default()
    }

    pub async fn live_messages(&self, session_id: &str) -> Vec<CodexDialogueMessage> {
        let state = self.inner.state.lock().await;
        state
            .session_index
            .get(session_id)
            .and_then(|key| state.processes.get(key))
            .map(|process| process.live_messages.clone())
            .unwrap_or_default()
    }

    pub async fn turn_traces(&self, session_id: &str) -> Vec<ProviderTurnTrace> {
        self.inner
            .state
            .lock()
            .await
            .turn_traces
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn shutdown_all(&self) {
        let keys: Vec<String> = self
            .inner
            .state
            .lock()
            .await
            .processes
            .keys()
            .cloned()
            .collect();
        for key in keys {
            self.shutdown_process(&key).await;
        }
    }

    async fn spawn_process(
        &self,
        key: String,
        executable: &std::path::Path,
        spawn_path: &std::ffi::OsStr,
        cwd: &str,
        model: Option<&str>,
        identity: String,
    ) -> AppResult<()> {
        let mut command = Command::new(executable);
        command
            .arg("agent")
            .arg("--always-approve")
            .env("PATH", spawn_path)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(model) = model {
            command.arg("--model").arg(model);
        }
        command.arg("stdio");
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| {
            AppError::provider(format!(
                "Failed to start Grok agent in '{cwd}' using '{}': {error}",
                executable.display()
            ))
        })?;
        let pid = child
            .id()
            .ok_or_else(|| AppError::provider("Grok child process has no process id."))?;
        let stdin =
            Arc::new(Mutex::new(child.stdin.take().ok_or_else(|| {
                AppError::provider("Grok did not expose stdin.")
            })?));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::provider("Grok did not expose stdout."))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::provider("Grok did not expose stderr."))?;
        let (kill, mut kill_rx) = mpsc::channel(1);
        self.inner.state.lock().await.processes.insert(
            key.clone(),
            GrokProcess {
                session_id: None,
                identity,
                stdin,
                pending: HashMap::new(),
                next_id: 1,
                active: None,
                assistant_text: String::new(),
                thought_buffer: String::new(),
                thought_index: 0,
                runtime: CodexTakeoverRuntime::default(),
                live_messages: Vec::new(),
                stderr_tail: VecDeque::new(),
                kill,
                pid,
            },
        );
        let reader = self.clone();
        let reader_key = key.clone();
        tokio::spawn(async move {
            reader.read_stdout(reader_key, stdout).await;
        });
        let stderr_reader = self.clone();
        let stderr_key = key.clone();
        tokio::spawn(async move {
            stderr_reader.read_stderr(stderr_key, stderr).await;
        });
        let waiter = self.clone();
        tokio::spawn(async move {
            let status = tokio::select! {
                status = child.wait() => status,
                _ = kill_rx.recv() => {
                    let _ = child.kill().await;
                    child.wait().await
                }
            };
            waiter.handle_exit(&key, status).await;
        });
        Ok(())
    }

    async fn handshake(&self, key: &str, request: &GrokSessionRequest) -> AppResult<String> {
        let initialized = self
            .call(
                key,
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": { "readTextFile": false, "writeTextFile": false },
                        "terminal": false
                    }
                }),
            )
            .await?;
        match choose_auth(
            &auth_method_ids(&initialized),
            grok_saved_login(),
            api_key_present(),
        ) {
            AuthChoice::Skip => {}
            AuthChoice::Method(method_id) => {
                self.call(key, "authenticate", json!({ "methodId": method_id }))
                    .await?;
            }
            AuthChoice::NeedsLogin => return Err(AppError::provider(LOGIN_HINT.to_string())),
        }
        let params = session_params(&request.cwd, &request.mcp_url);
        let resume = request
            .resume_session_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let (opened, resumed) = if let Some(resume) = resume {
            let mut load_params = params.clone();
            load_params["sessionId"] = json!(resume);
            match self.call(key, "session/load", load_params).await {
                Ok(loaded) => (loaded, Some(resume)),
                Err(_) => (self.call(key, "session/new", params).await?, None),
            }
        } else {
            (self.call(key, "session/new", params).await?, None)
        };
        opened_session_id(&opened, resumed).ok_or_else(|| {
            AppError::provider("Grok session start did not return a session id.".to_string())
        })
    }

    async fn call(&self, key: &str, method: &str, params: Value) -> AppResult<Value> {
        let (request_id, stdin, result) = {
            let mut state = self.inner.state.lock().await;
            let process = state.processes.get_mut(key).ok_or_else(|| {
                AppError::provider(format!("Grok process exited before {method}."))
            })?;
            let request_id = process.next_id;
            process.next_id += 1;
            let (tx, rx) = oneshot::channel();
            process.pending.insert(request_id, tx);
            (request_id, process.stdin.clone(), rx)
        };
        if let Err(error) = write_line(
            &stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }),
        )
        .await
        {
            self.take_pending(key, request_id).await;
            return Err(error);
        }
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, result).await {
            Ok(Ok(value)) => value,
            Ok(Err(_)) => Err(AppError::provider(format!("Grok exited during {method}."))),
            Err(_) => {
                self.take_pending(key, request_id).await;
                Err(AppError::provider(format!(
                    "Grok {method} timed out after {} seconds.",
                    HANDSHAKE_TIMEOUT.as_secs()
                )))
            }
        }
    }

    async fn read_stdout<R>(&self, key: String, stdout: R)
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) if line.trim().is_empty() => continue,
                Ok(Some(line)) => match serde_json::from_str::<Value>(&line) {
                    Ok(value) => self.handle_message(&key, value).await,
                    Err(_) => self.push_stderr(&key, line.trim()).await,
                },
                Ok(None) => break,
                Err(error) => {
                    self.fail_process(&key, &format!("Failed reading Grok stdout: {error}"))
                        .await;
                    break;
                }
            }
        }
    }

    async fn read_stderr<R>(&self, key: String, stderr: R)
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            self.push_stderr(&key, &line).await;
        }
    }

    async fn handle_message(&self, key: &str, value: Value) {
        if value.get("method").is_some() && value.get("id").is_some() {
            self.answer_client_request(key, value).await;
            return;
        }
        if value.get("method").and_then(Value::as_str) == Some("session/update") {
            self.apply_update(key, &value).await;
            return;
        }
        if let Some(request_id) = value.get("id").and_then(json_id) {
            self.complete_response(key, request_id, value).await;
        }
    }

    async fn apply_update(&self, key: &str, message: &Value) {
        let update = message
            .pointer("/params/update")
            .cloned()
            .unwrap_or(Value::Null);
        let kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or("");
        {
            let mut state = self.inner.state.lock().await;
            let Some(process) = state.processes.get_mut(key) else {
                return;
            };
            let Some(turn_id) = process.active.as_ref().map(|active| active.turn_id.clone()) else {
                return;
            };
            let publish = match kind {
                "agent_message_chunk" => {
                    let flushed = flush_thought_phrase(process, &turn_id);
                    let text = content_text(&update);
                    process.assistant_text.push_str(&text);
                    upsert_assistant(process, &turn_id, &text);
                    flushed || !text.is_empty()
                }
                "agent_thought_chunk" => {
                    absorb_thought_chunk(process, &turn_id, &content_text(&update))
                }
                "tool_call" | "tool_call_update" => {
                    flush_thought_phrase(process, &turn_id);
                    upsert_activity(
                        process,
                        &turn_id,
                        &format!("USING TOOL · {}", tool_label(&update)),
                    );
                    true
                }
                _ => false,
            };
            if !publish {
                return;
            }
        }
        self.emit(key, "turn/progress").await;
    }

    async fn complete_response(&self, key: &str, request_id: u64, value: Value) {
        let active_turn = {
            let state = self.inner.state.lock().await;
            state
                .processes
                .get(key)
                .and_then(|process| process.active.as_ref())
                .is_some_and(|active| active.request_id == request_id)
        };
        if active_turn {
            self.finish_turn(key, value).await;
            return;
        }
        let outcome = response_value(&value);
        let sender = {
            self.inner
                .state
                .lock()
                .await
                .processes
                .get_mut(key)
                .and_then(|process| process.pending.remove(&request_id))
        };
        if let Some(sender) = sender {
            let _ = sender.send(outcome);
        }
    }

    async fn finish_turn(&self, key: &str, value: Value) {
        let terminal = {
            let mut state = self.inner.state.lock().await;
            let Some(process) = state.processes.get_mut(key) else {
                return;
            };
            let Some(active) = process.active.take() else {
                return;
            };
            flush_thought_phrase(process, &active.turn_id);
            let stopping = process.runtime.phase == "stopping";
            let (status, response, error) = turn_outcome(&value, &process.assistant_text, stopping);
            let trace_status = grok_trace_status(&status);
            process.runtime.phase = if stopping { "stopped" } else { "idle" }.to_string();
            process.runtime.active_turn_id = None;
            process.runtime.error = error
                .clone()
                .filter(|_| !matches!(status.as_str(), "SUCCESS" | "INTERRUPTED" | "CANCELED"));
            let message_status = match trace_status {
                "success" => "success",
                "interrupted" => "discarded",
                _ => "error",
            };
            for message in &mut process.live_messages {
                message.status = message_status.to_string();
            }
            let messages = std::mem::take(&mut process.live_messages);
            process.assistant_text.clear();
            let session_id = process.session_id.clone().unwrap_or_default();
            if !messages.is_empty() && !session_id.is_empty() {
                let traces = state.turn_traces.entry(session_id.clone()).or_default();
                traces.push(ProviderTurnTrace {
                    turn_id: active.turn_id.clone(),
                    status: trace_status.to_string(),
                    messages,
                    completed_at: now_seconds(),
                });
                if traces.len() > TURN_TRACE_LIMIT {
                    traces.drain(..traces.len() - TURN_TRACE_LIMIT);
                }
            }
            active.result_tx.map(|sender| {
                (
                    sender,
                    GrokTurnResult {
                        session_id,
                        turn_id: active.turn_id,
                        status,
                        response,
                        error,
                    },
                )
            })
        };
        if let Some((sender, result)) = terminal {
            let _ = sender.send(Ok(result));
        }
        self.emit(key, "turn/result").await;
    }

    async fn answer_client_request(&self, key: &str, value: Value) {
        let request_id = value.get("id").cloned().unwrap_or(Value::Null);
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let response = if method == "session/request_permission" {
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": permission_result(value.get("params").unwrap_or(&Value::Null)),
            })
        } else {
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {
                    "code": -32601,
                    "message": format!("Ecky does not implement Grok client method {method}."),
                }
            })
        };
        let stdin = {
            let state = self.inner.state.lock().await;
            state
                .processes
                .get(key)
                .map(|process| process.stdin.clone())
        };
        if let Some(stdin) = stdin {
            let _ = write_line(&stdin, &response).await;
        }
    }

    async fn publish_session(&self, key: &str, session_id: &str) {
        let mut state = self.inner.state.lock().await;
        if let Some(process) = state.processes.get_mut(key) {
            process.session_id = Some(session_id.to_string());
        }
        state
            .session_index
            .insert(session_id.to_string(), key.to_string());
    }

    async fn process_key(&self, session_id: &str) -> Option<String> {
        self.inner
            .state
            .lock()
            .await
            .session_index
            .get(session_id)
            .cloned()
    }

    async fn process_identity(&self, key: &str) -> Option<String> {
        self.inner
            .state
            .lock()
            .await
            .processes
            .get(key)
            .map(|process| process.identity.clone())
    }

    async fn clear_active(&self, key: &str) {
        if let Some(process) = self.inner.state.lock().await.processes.get_mut(key) {
            process.active = None;
            process.runtime = CodexTakeoverRuntime::default();
        }
    }

    async fn take_pending(&self, key: &str, request_id: u64) {
        if let Some(process) = self.inner.state.lock().await.processes.get_mut(key) {
            process.pending.remove(&request_id);
        }
    }

    async fn push_stderr(&self, key: &str, line: &str) {
        if let Some(process) = self.inner.state.lock().await.processes.get_mut(key) {
            process.stderr_tail.push_back(line.to_string());
            while process.stderr_tail.len() > STDERR_TAIL_LINES {
                process.stderr_tail.pop_front();
            }
        }
    }

    async fn stderr_detail(&self, key: &str) -> String {
        let state = self.inner.state.lock().await;
        let Some(process) = state.processes.get(key) else {
            return "Grok stderr was empty.".to_string();
        };
        if process.stderr_tail.is_empty() {
            "Grok stderr was empty.".to_string()
        } else {
            format!(
                "Grok stderr:\n{}",
                process
                    .stderr_tail
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
    }

    async fn handle_exit(
        &self,
        key: &str,
        status: Result<std::process::ExitStatus, std::io::Error>,
    ) {
        let detail = match status {
            Ok(status) => format!("Grok process exited ({status})."),
            Err(error) => format!("Grok process exited: {error}."),
        };
        let stderr = self.stderr_detail(key).await;
        self.fail_process(key, &format!("{detail} {stderr}")).await;
    }

    async fn fail_process(&self, key: &str, detail: &str) {
        let (pending, active, session_id) = {
            let mut state = self.inner.state.lock().await;
            let Some(mut process) = state.processes.remove(key) else {
                return;
            };
            if let Some(session_id) = process.session_id.as_deref() {
                if state.session_index.get(session_id).map(String::as_str) == Some(key) {
                    state.session_index.remove(session_id);
                }
            }
            (process.pending, process.active.take(), process.session_id)
        };
        for (_, sender) in pending {
            let _ = sender.send(Err(AppError::provider(detail.to_string())));
        }
        if let Some(active) = active {
            if let Some(sender) = active.result_tx {
                let _ = sender.send(Ok(GrokTurnResult {
                    session_id: session_id.unwrap_or_default(),
                    turn_id: active.turn_id,
                    status: "FAILED".to_string(),
                    response: String::new(),
                    error: Some(detail.to_string()),
                }));
            }
        }
    }

    async fn shutdown_process(&self, key: &str) {
        let taken = {
            let mut state = self.inner.state.lock().await;
            let Some(process) = state.processes.remove(key) else {
                return;
            };
            if let Some(session_id) = process.session_id.clone() {
                if state.session_index.get(&session_id).map(String::as_str) == Some(key) {
                    state.session_index.remove(&session_id);
                }
            }
            process
        };
        signal_group(taken.pid, libc_sigkill());
        let _ = taken.kill.send(()).await;
        for (_, sender) in taken.pending {
            let _ = sender.send(Err(AppError::provider(
                "Grok process stopped before the request finished.".to_string(),
            )));
        }
        if let Some(active) = taken.active {
            if let Some(sender) = active.result_tx {
                let _ = sender.send(Err(AppError::provider(
                    "Grok process stopped before the turn finished.".to_string(),
                )));
            }
        }
    }

    async fn emit(&self, key: &str, method: &str) {
        let event = {
            let state = self.inner.state.lock().await;
            let Some(process) = state.processes.get(key) else {
                return;
            };
            let session_id = process.session_id.clone();
            let traces = session_id
                .as_deref()
                .and_then(|id| state.turn_traces.get(id))
                .cloned()
                .unwrap_or_default();
            (
                state.app_handle.clone(),
                session_id,
                process.runtime.clone(),
                process.live_messages.clone(),
                traces,
            )
        };
        let (Some(app_handle), session_id, runtime, live_messages, turn_traces) = event else {
            return;
        };
        let _ = app_handle.emit(
            "grok-provider-updated",
            json!({
                "conversationId": session_id,
                "method": method,
                "runtime": runtime,
                "liveMessages": live_messages,
                "turnTraces": turn_traces,
            }),
        );
    }
}

fn normalize_model(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
}

fn session_identity(model: Option<&str>, mcp_url: &str, cwd: &str) -> String {
    format!("{}\n{mcp_url}\n{cwd}", model.unwrap_or(""))
}

fn session_params(cwd: &str, mcp_url: &str) -> Value {
    let mcp_servers = if mcp_url.trim().is_empty() {
        Vec::new()
    } else {
        vec![json!({
            "type": "http",
            "name": "ecky_mcp",
            "url": mcp_url,
            "headers": [],
        })]
    };
    json!({
        "cwd": cwd,
        "mcpServers": mcp_servers,
        "_meta": { "yoloMode": true },
    })
}

fn opened_session_id(opened: &Value, resumed_session_id: Option<&str>) -> Option<String> {
    opened
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            resumed_session_id
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        })
}

fn auth_method_ids(initialized: &Value) -> Vec<String> {
    initialized
        .get("authMethods")
        .and_then(Value::as_array)
        .map(|methods| {
            methods
                .iter()
                .filter_map(|method| method.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn choose_auth(method_ids: &[String], saved_login: bool, api_key: bool) -> AuthChoice {
    if method_ids.is_empty() {
        return AuthChoice::Skip;
    }
    if method_ids.iter().any(|id| id == "cached_token") {
        return AuthChoice::Method("cached_token");
    }
    if api_key && method_ids.iter().any(|id| id == "xai.api_key") {
        return AuthChoice::Method("xai.api_key");
    }
    if saved_login && method_ids.iter().any(|id| id == "grok.com") {
        return AuthChoice::Method("grok.com");
    }
    AuthChoice::NeedsLogin
}

fn grok_saved_login() -> bool {
    env_present("GROK_DEPLOYMENT_KEY")
        || std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".grok/auth.json").is_file())
            .unwrap_or(false)
}

fn api_key_present() -> bool {
    env_present("XAI_API_KEY")
}

fn env_present(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn response_value(value: &Value) -> AppResult<Value> {
    if let Some(message) = rpc_failure(value) {
        Err(AppError::provider(friendly_grok_error(&message)))
    } else {
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn turn_outcome(
    value: &Value,
    assistant_text: &str,
    stopping: bool,
) -> (String, String, Option<String>) {
    if let Some(message) = rpc_failure(value) {
        let message = friendly_grok_error(&message);
        return (
            "FAILED".to_string(),
            assistant_text.to_string(),
            Some(message),
        );
    }
    let result = value.get("result").cloned().unwrap_or(Value::Null);
    let stop_reason = result
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let status = turn_status(stop_reason, stopping).to_string();
    let mut response = assistant_text.to_string();
    if response.trim().is_empty() {
        if let Some(text) = result.get("text").and_then(Value::as_str) {
            response = text.to_string();
        }
    }
    let error = if matches!(status.as_str(), "SUCCESS" | "INTERRUPTED" | "CANCELED") {
        None
    } else if stop_reason.is_empty() {
        Some(status.clone())
    } else {
        Some(stop_reason.to_string())
    };
    (status, response, error)
}

fn turn_status(stop_reason: &str, stopping: bool) -> &'static str {
    if stopping || matches!(stop_reason, "cancelled" | "canceled" | "cancelled_by_user") {
        "INTERRUPTED"
    } else if matches!(stop_reason, "end_turn" | "end" | "stop" | "") {
        "SUCCESS"
    } else {
        "FAILED"
    }
}

fn grok_trace_status(status: &str) -> &'static str {
    match status {
        "SUCCESS" => "success",
        "CANCELED" | "INTERRUPTED" => "interrupted",
        _ => "error",
    }
}

fn friendly_grok_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("authentication required")
        || lower.contains("not authenticated")
        || lower.contains("no auth")
    {
        LOGIN_HINT.to_string()
    } else {
        message.to_string()
    }
}

fn rpc_failure(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Grok request failed");
    let data = error
        .get("data")
        .and_then(Value::as_str)
        .filter(|data| !data.is_empty());
    Some(match data {
        Some(data) => format!("{message}: {data}"),
        None => message.to_string(),
    })
}

fn permission_result(params: &Value) -> Value {
    let option_id = params
        .get("options")
        .and_then(Value::as_array)
        .and_then(|options| {
            options
                .iter()
                .find(|option| {
                    option
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| kind.starts_with("allow"))
                })
                .or_else(|| options.first())
                .and_then(|option| option.get("optionId").and_then(Value::as_str))
                .map(str::to_string)
        });
    match option_id {
        Some(option_id) => json!({ "outcome": { "outcome": "selected", "optionId": option_id } }),
        None => json!({ "outcome": { "outcome": "cancelled" } }),
    }
}

fn content_text(update: &Value) -> String {
    update
        .get("content")
        .and_then(|content| {
            content
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| content.as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

fn tool_label(update: &Value) -> String {
    update
        .get("title")
        .or_else(|| update.get("tool"))
        .or_else(|| update.get("name"))
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .unwrap_or("tool")
        .to_string()
}

fn json_id(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|id| u64::try_from(id).ok()))
}

fn upsert_assistant(process: &mut GrokProcess, turn_id: &str, extra: &str) {
    let id = format!("{turn_id}:assistant");
    if let Some(message) = process
        .live_messages
        .iter_mut()
        .find(|message| message.id == id)
    {
        message.content.push_str(extra);
        message.content = truncate_chars(&message.content, LIVE_MESSAGE_CHAR_LIMIT);
        return;
    }
    process.live_messages.push(CodexDialogueMessage {
        id,
        role: "assistant".to_string(),
        content: truncate_chars(extra, LIVE_MESSAGE_CHAR_LIMIT),
        status: "working".to_string(),
        timestamp: now_seconds(),
        attachments: Vec::new(),
        provider_event_kind: Some(ProviderEventKind::Assistant),
    });
}

const THOUGHT_PHRASE_LIMIT: usize = 240;

fn absorb_thought_chunk(process: &mut GrokProcess, turn_id: &str, chunk: &str) -> bool {
    if chunk.is_empty() {
        return false;
    }
    process.thought_buffer.push_str(chunk);
    let (phrases, rest) = take_complete_phrases(&process.thought_buffer);
    process.thought_buffer = rest;
    let mut published = false;
    for phrase in phrases {
        push_thought_phrase(process, turn_id, &phrase);
        published = true;
    }
    published
}

fn flush_thought_phrase(process: &mut GrokProcess, turn_id: &str) -> bool {
    let phrase = normalize_phrase(&process.thought_buffer);
    process.thought_buffer.clear();
    if phrase.is_empty() {
        return false;
    }
    push_thought_phrase(process, turn_id, &phrase);
    true
}

fn push_thought_phrase(process: &mut GrokProcess, turn_id: &str, phrase: &str) {
    let id = format!("{turn_id}:thought:{}", process.thought_index);
    process.thought_index = process.thought_index.saturating_add(1);
    process.live_messages.push(CodexDialogueMessage {
        id,
        role: "assistant".to_string(),
        content: truncate_chars(&format!("THINKING · {phrase}"), LIVE_MESSAGE_CHAR_LIMIT),
        status: "working".to_string(),
        timestamp: now_seconds(),
        attachments: Vec::new(),
        provider_event_kind: Some(ProviderEventKind::Activity),
    });
}

fn take_complete_phrases(buffer: &str) -> (Vec<String>, String) {
    let chars: Vec<char> = buffer.chars().collect();
    let mut phrases = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < chars.len() {
        let boundary = phrase_boundary(&chars, index);
        if boundary && phrase_is_long_enough(&chars[start..=index]) {
            let phrase = normalize_phrase(&chars[start..=index].iter().collect::<String>());
            if !phrase.is_empty() {
                phrases.push(phrase);
            }
            index += 1;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            start = index;
            continue;
        }
        index += 1;
    }
    let mut rest: String = chars[start..].iter().collect();
    while rest.chars().count() > THOUGHT_PHRASE_LIMIT {
        let split_at = rest
            .char_indices()
            .take(THOUGHT_PHRASE_LIMIT)
            .filter(|(_, ch)| ch.is_whitespace())
            .map(|(offset, _)| offset)
            .last()
            .unwrap_or_else(|| {
                rest.char_indices()
                    .nth(THOUGHT_PHRASE_LIMIT)
                    .map(|(offset, _)| offset)
                    .unwrap_or(rest.len())
            });
        let phrase = normalize_phrase(&rest[..split_at]);
        rest = rest[split_at..].trim_start().to_string();
        if !phrase.is_empty() {
            phrases.push(phrase);
        }
    }
    (phrases, rest)
}

fn phrase_boundary(chars: &[char], index: usize) -> bool {
    let ch = chars[index];
    if ch == '\n' || ch == '\r' {
        return true;
    }
    if !matches!(ch, '.' | '!' | '?' | '…' | '。' | '！' | '？') {
        return false;
    }
    chars
        .get(index + 1)
        .copied()
        .is_some_and(|next| next.is_whitespace())
}

fn phrase_is_long_enough(chars: &[char]) -> bool {
    chars.iter().any(|ch| *ch == '\n' || *ch == '\r')
        || chars.iter().filter(|ch| !ch.is_whitespace()).count() >= 12
}

fn normalize_phrase(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn upsert_activity(process: &mut GrokProcess, turn_id: &str, text: &str) {
    let id = format!("{turn_id}:activity");
    let content = truncate_chars(text, LIVE_MESSAGE_CHAR_LIMIT);
    if let Some(message) = process
        .live_messages
        .iter_mut()
        .find(|message| message.id == id)
    {
        message.content = content;
        return;
    }
    process.live_messages.push(CodexDialogueMessage {
        id,
        role: "assistant".to_string(),
        content,
        status: "working".to_string(),
        timestamp: now_seconds(),
        attachments: Vec::new(),
        provider_event_kind: Some(ProviderEventKind::Activity),
    });
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        text.chars().take(limit).collect()
    }
}

async fn write_line(stdin: &Arc<Mutex<ChildStdin>>, value: &Value) -> AppResult<()> {
    let mut line = serde_json::to_string(value)
        .map_err(|error| AppError::provider(format!("Failed to encode Grok request: {error}")))?;
    line.push('\n');
    let mut stdin = stdin.lock().await;
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|error| AppError::provider(format!("Failed writing Grok stdin: {error}")))?;
    stdin
        .flush()
        .await
        .map_err(|error| AppError::provider(format!("Failed flushing Grok stdin: {error}")))?;
    Ok(())
}

async fn probe_grok_version(path: &std::path::Path, spawn_path: &std::ffi::OsStr) -> AppResult<()> {
    let output = Command::new(path)
        .arg("version")
        .env("PATH", spawn_path)
        .output()
        .await
        .map_err(|error| {
            AppError::provider(format!(
                "Failed to run Grok CLI '{} version': {error}",
                path.display()
            ))
        })?;
    if output.status.success() {
        return Ok(());
    }
    let detail = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Err(AppError::provider(format!(
        "Grok CLI version probe failed: {}",
        detail.trim()
    )))
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    unsafe {
        libc::kill(-(pid as i32), signal);
    }
}

#[cfg(not(unix))]
fn signal_group(_pid: u32, _signal: i32) {}

#[cfg(unix)]
fn libc_sigint() -> i32 {
    libc::SIGINT
}

#[cfg(not(unix))]
fn libc_sigint() -> i32 {
    2
}

#[cfg(unix)]
fn libc_sigkill() -> i32 {
    libc::SIGKILL
}

#[cfg(not(unix))]
fn libc_sigkill() -> i32 {
    9
}

pub fn parse_grok_models(output: &str) -> Vec<String> {
    let trimmed = output.trim();
    if let Ok(values) = serde_json::from_str::<Vec<Value>>(trimmed) {
        let models = values
            .into_iter()
            .filter_map(|value| {
                value.as_str().map(str::to_string).or_else(|| {
                    value
                        .get("id")
                        .or_else(|| value.get("modelId"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
            })
            .filter(|model| !model.is_empty())
            .collect::<Vec<_>>();
        if !models.is_empty() {
            return models;
        }
    }
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line
                .strip_prefix("* ")
                .or_else(|| line.strip_prefix("- "))?;
            let model = rest.split_whitespace().next()?.trim();
            (!model.is_empty()).then(|| model.to_string())
        })
        .collect()
}

pub fn get_binding(
    conn: &Connection,
    ecky_thread_id: &str,
) -> AppResult<Option<GrokProviderBinding>> {
    Ok(super::codex_takeover::get_agent_binding_for_provider(
        conn,
        ecky_thread_id,
        GROK_PROVIDER_ID,
    )?
    .map(binding_from_record))
}

pub fn bind_owned_session(
    conn: &Connection,
    ecky_thread_id: &str,
    session_id: &str,
    label: &str,
    cwd: &str,
    now: i64,
) -> AppResult<GrokProviderBinding> {
    let saved = super::codex_takeover::upsert_agent_binding(
        conn,
        &super::codex_takeover::AgentThreadBindingRecord {
            ecky_thread_id: ecky_thread_id.to_string(),
            provider: GROK_PROVIDER_ID.to_string(),
            external_thread_id: session_id.to_string(),
            external_title: label.to_string(),
            external_cwd: cwd.to_string(),
            bootstrap_version: GROK_BOOTSTRAP_VERSION,
            created_at: now,
            updated_at: now,
        },
    )?;
    Ok(binding_from_record(saved))
}

pub fn rotate_binding(
    conn: &Connection,
    ecky_thread_id: &str,
    new_session_id: &str,
    reason: &str,
    now: i64,
) -> AppResult<GrokProviderBinding> {
    let saved = super::codex_takeover::rotate_agent_binding(
        conn,
        ecky_thread_id,
        GROK_PROVIDER_ID,
        new_session_id,
        reason,
        now,
    )?;
    Ok(binding_from_record(saved))
}

fn binding_from_record(
    binding: super::codex_takeover::AgentThreadBindingRecord,
) -> GrokProviderBinding {
    GrokProviderBinding {
        ecky_thread_id: binding.ecky_thread_id,
        grok_session_id: binding.external_thread_id,
        label: binding.external_title,
        cwd: binding.external_cwd,
        bootstrap_version: binding.bootstrap_version,
        created_at: binding.created_at,
        updated_at: binding.updated_at,
    }
}

pub fn count_session_messages(
    conn: &Connection,
    ecky_thread_id: &str,
    session_id: &str,
) -> AppResult<usize> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_provider_messages
             WHERE ecky_thread_id = ?1 AND provider = ?2 AND external_thread_id = ?3",
            params![ecky_thread_id, GROK_PROVIDER_ID, session_id],
            |row| row.get(0),
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(count as usize)
}

pub fn enqueue_prompt_with_attachments(
    conn: &Connection,
    ecky_thread_id: &str,
    prompt_text: &str,
    attachments: &[Attachment],
    now: i64,
) -> AppResult<CodexQueuedPrompt> {
    if prompt_text.trim().is_empty() && attachments.is_empty() {
        return Err(AppError::validation(
            "Grok queued prompt must include text or attachments.",
        ));
    }
    if get_binding(conn, ecky_thread_id)?.is_none() {
        return Err(AppError::not_found(format!(
            "Ecky thread {ecky_thread_id} has no owned Grok session."
        )));
    }
    let item = CodexQueuedPrompt {
        id: uuid::Uuid::new_v4().to_string(),
        ecky_thread_id: ecky_thread_id.to_string(),
        prompt_text: prompt_text.to_string(),
        attachments: attachments.to_vec(),
        status: "queued".to_string(),
        can_cancel: true,
        error: None,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO agent_prompt_queue
            (id, ecky_thread_id, provider, prompt_text, attachments_json, status, error, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'queued', NULL, ?6, ?6)",
        params![
            item.id,
            item.ecky_thread_id,
            GROK_PROVIDER_ID,
            item.prompt_text,
            serde_json::to_string(&item.attachments)
                .map_err(|error| AppError::persistence(error.to_string()))?,
            now
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(item)
}

pub fn list_queue(conn: &Connection, ecky_thread_id: &str) -> AppResult<Vec<CodexQueuedPrompt>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, ecky_thread_id, prompt_text, attachments_json, status, error, created_at, updated_at, dispatch_started
             FROM agent_prompt_queue
             WHERE ecky_thread_id = ?1 AND provider = ?2
             ORDER BY created_at ASC, id ASC",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map(params![ecky_thread_id, GROK_PROVIDER_ID], |row| {
            let status: String = row.get(4)?;
            let dispatch_started: i64 = row.get(8)?;
            Ok(CodexQueuedPrompt {
                id: row.get(0)?,
                ecky_thread_id: row.get(1)?,
                prompt_text: row.get(2)?,
                attachments: decode_attachments(row.get(3)?)?,
                status: status.clone(),
                error: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                can_cancel: status != "sending" || dispatch_started == 0,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn queue_head(conn: &Connection, ecky_thread_id: &str) -> AppResult<Option<CodexQueuedPrompt>> {
    Ok(list_queue(conn, ecky_thread_id)?.into_iter().next())
}

pub fn pending_queue_bindings(conn: &Connection) -> AppResult<Vec<GrokProviderBinding>> {
    let mut stmt = conn
        .prepare(
            "SELECT b.ecky_thread_id, b.external_thread_id, b.external_title,
                    b.external_cwd, b.bootstrap_version, b.created_at, b.updated_at
             FROM agent_thread_bindings b
             JOIN agent_prompt_queue q ON q.id = (
                 SELECT head.id FROM agent_prompt_queue head
                 WHERE head.ecky_thread_id = b.ecky_thread_id AND head.provider = b.provider
                 ORDER BY head.created_at ASC, head.id ASC LIMIT 1
             )
             WHERE b.provider = ?1 AND q.status = 'queued'
             ORDER BY q.created_at ASC, q.id ASC",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map([GROK_PROVIDER_ID], |row| {
            Ok(GrokProviderBinding {
                ecky_thread_id: row.get(0)?,
                grok_session_id: row.get(1)?,
                label: row.get(2)?,
                cwd: row.get(3)?,
                bootstrap_version: row.get::<_, i64>(4)? as u32,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn retry_queue_item(
    conn: &Connection,
    ecky_thread_id: &str,
    id: &str,
    now: i64,
) -> AppResult<()> {
    let head = queue_head(conn, ecky_thread_id)?
        .ok_or_else(|| AppError::not_found(format!("Grok queue item {id} was not found.")))?;
    if head.id != id {
        return Err(AppError::conflict(format!(
            "Grok queue item {id} cannot overtake queue head {}.",
            head.id
        )));
    }
    let changed = conn
        .execute(
            "UPDATE agent_prompt_queue SET status = 'queued', error = NULL, updated_at = ?3
             WHERE id = ?1 AND ecky_thread_id = ?2 AND provider = 'grok' AND status = 'failed'",
            params![id, ecky_thread_id, now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    if changed == 1 {
        Ok(())
    } else {
        Err(AppError::conflict(format!(
            "Grok queue item {id} is not failed."
        )))
    }
}

pub fn remove_queue_item(conn: &Connection, ecky_thread_id: &str, id: &str) -> AppResult<()> {
    let status: Option<(String, bool)> = conn
        .query_row(
            "SELECT status, dispatch_started FROM agent_prompt_queue WHERE id = ?1 AND ecky_thread_id = ?2 AND provider = ?3",
            params![id, ecky_thread_id, GROK_PROVIDER_ID],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .optional()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    match status {
        Some((status, false)) if status == "sending" => {
            let removed = conn
                .execute(
                    "DELETE FROM agent_prompt_queue
                     WHERE id = ?1 AND ecky_thread_id = ?2 AND provider = ?3
                       AND status = 'sending' AND dispatch_started = 0",
                    params![id, ecky_thread_id, GROK_PROVIDER_ID],
                )
                .map_err(|error| AppError::persistence(error.to_string()))?;
            if removed == 1 {
                Ok(())
            } else {
                Err(AppError::conflict(format!(
                    "Grok queue item {id} crossed the provider delivery boundary. Use STOP."
                )))
            }
        }
        Some((status, true)) if status == "sending" => Err(AppError::conflict(format!(
            "Grok queue item {id} crossed the provider delivery boundary. Use STOP."
        ))),
        Some(_) => {
            conn.execute(
                "DELETE FROM agent_prompt_queue WHERE id = ?1 AND ecky_thread_id = ?2 AND provider = ?3",
                params![id, ecky_thread_id, GROK_PROVIDER_ID],
            )
            .map_err(|error| AppError::persistence(error.to_string()))?;
            Ok(())
        }
        None => Err(AppError::not_found(format!(
            "Grok queue item {id} was not found."
        ))),
    }
}

pub fn message_page(
    conn: &Connection,
    ecky_thread_id: &str,
    cursor: Option<&str>,
) -> AppResult<GrokMessagePage> {
    const PAGE_SIZE: usize = 30;
    let boundary = cursor.map(decode_cursor).transpose()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, role, content, attachments_json, status, created_at
             FROM agent_provider_messages
             WHERE ecky_thread_id = ?1 AND provider = ?2
               AND (?3 IS NULL OR created_at < ?3 OR (created_at = ?3 AND id < ?4))
             ORDER BY created_at DESC, id DESC LIMIT 31",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map(
            params![
                ecky_thread_id,
                GROK_PROVIDER_ID,
                boundary.as_ref().map(|(timestamp, _)| *timestamp),
                boundary.as_ref().map(|(_, id)| id.as_str()),
            ],
            |row| {
                Ok(CodexDialogueMessage {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    attachments: decode_attachments(row.get(3)?)?,
                    status: row.get(4)?,
                    timestamp: row.get(5)?,
                    provider_event_kind: None,
                })
            },
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let mut messages = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let has_more = messages.len() > PAGE_SIZE;
    messages.truncate(PAGE_SIZE);
    let next_cursor = has_more
        .then(|| {
            messages
                .last()
                .map(|message| encode_cursor(message.timestamp, &message.id))
        })
        .flatten();
    messages.reverse();
    Ok(GrokMessagePage {
        messages,
        next_cursor,
        backwards_cursor: None,
    })
}

pub fn fail_inflight_queue(conn: &Connection, now: i64) -> AppResult<usize> {
    conn.execute(
        "UPDATE agent_prompt_queue
         SET status = 'failed', error = ?2, failure_phase = 'provider-restart', updated_at = ?3
         WHERE provider = ?1 AND status = 'sending'",
        params![
            GROK_PROVIDER_ID,
            "Grok restarted before this turn finished.",
            now
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))
}

fn decode_attachments(value: String) -> rusqlite::Result<Vec<Attachment>> {
    serde_json::from_str(&value)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}

fn encode_cursor(timestamp: i64, id: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{timestamp}\n{id}"))
}

fn decode_cursor(cursor: &str) -> AppResult<(i64, String)> {
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|error| AppError::validation(format!("Invalid Grok message cursor: {error}")))?;
    let decoded = String::from_utf8(decoded)
        .map_err(|error| AppError::validation(format!("Invalid Grok message cursor: {error}")))?;
    let (timestamp, id) = decoded
        .split_once('\n')
        .ok_or_else(|| AppError::validation("Invalid Grok message cursor payload."))?;
    Ok((
        timestamp.parse().map_err(|error| {
            AppError::validation(format!("Invalid Grok message cursor timestamp: {error}"))
        })?,
        id.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        choose_auth, friendly_grok_error, opened_session_id, parse_grok_models, permission_result,
        take_complete_phrases, turn_status, AuthChoice,
    };
    use serde_json::json;

    #[test]
    fn model_list_reads_grok_models_markers() {
        let output = "You are not authenticated.\n\nDefault model: grok-4.6\n\nAvailable models:\n  * grok-4.6 (default)\n  - grok-4.5\n";
        assert_eq!(
            parse_grok_models(output),
            vec!["grok-4.6".to_string(), "grok-4.5".to_string()]
        );
    }

    #[test]
    fn unsigned_grok_login_is_reported_before_a_browser_prompt() {
        assert_eq!(
            choose_auth(&["grok.com".to_string()], false, false),
            AuthChoice::NeedsLogin
        );
        assert_eq!(
            choose_auth(&["grok.com".to_string()], true, false),
            AuthChoice::Method("grok.com")
        );
        assert_eq!(choose_auth(&[], false, false), AuthChoice::Skip);
        assert_eq!(
            friendly_grok_error("Authentication required: no auth method id provided"),
            super::LOGIN_HINT
        );
    }

    #[test]
    fn permission_request_selects_an_allow_option() {
        let result = permission_result(&json!({
            "options": [
                { "optionId": "reject", "kind": "reject_once" },
                { "optionId": "allow", "kind": "allow_once" }
            ]
        }));
        assert_eq!(result["outcome"]["optionId"], "allow");
    }

    #[test]
    fn loaded_session_keeps_the_requested_id_when_grok_omits_it() {
        let loaded = json!({
            "models": { "currentModelId": "grok-4.7", "availableModels": [] },
            "configOptions": [],
        });
        assert_eq!(
            opened_session_id(&loaded, Some("sess-resume")),
            Some("sess-resume".to_string())
        );
        let created = json!({"sessionId": "sess-new", "models": {}});
        assert_eq!(
            opened_session_id(&created, None),
            Some("sess-new".to_string())
        );
        assert_eq!(opened_session_id(&loaded, None), None);
    }

    #[test]
    fn thought_stream_waits_for_a_whole_phrase() {
        let mut buffer = String::new();
        let mut published = Vec::new();
        for chunk in [
            "заменю ",
            "отверстие ",
            "на ",
            "резьбу. ",
            "Потом проверю посадку",
        ] {
            buffer.push_str(chunk);
            let (phrases, rest) = take_complete_phrases(&buffer);
            buffer = rest;
            published.extend(phrases);
        }
        assert_eq!(published, vec!["заменю отверстие на резьбу.".to_string()]);
        assert_eq!(buffer, "Потом проверю посадку");

        let (phrases, rest) = take_complete_phrases("т.е. дальше без точки");
        assert!(phrases.is_empty());
        assert_eq!(rest, "т.е. дальше без точки");

        let (phrases, rest) =
            take_complete_phrases("Сначала смотрю модель.\nПотом правлю отверстие");
        assert_eq!(phrases, vec!["Сначала смотрю модель.".to_string()]);
        assert_eq!(rest, "Потом правлю отверстие");
    }

    #[test]
    fn cancelled_stop_reason_interrupts_the_turn() {
        assert_eq!(turn_status("end_turn", false), "SUCCESS");
        assert_eq!(turn_status("cancelled", false), "INTERRUPTED");
        assert_eq!(turn_status("end_turn", true), "INTERRUPTED");
        assert_eq!(turn_status("refusal", false), "FAILED");
    }
}
