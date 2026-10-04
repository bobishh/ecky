use crate::contracts::{
    AgyMessagePage, AgyProviderBinding, AppError, AppResult, Attachment, CodexDialogueMessage,
    CodexQueuedPrompt, CodexTakeoverRuntime, ProviderEventKind, ProviderTurnTrace,
};
use crate::provider_turn::{ProviderTurnIntent, ProviderTurnPolicy};
use crate::services::provider_executable::resolve_provider_executable;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

pub const AGY_PROVIDER_ID: &str = "agy";
pub const AGY_BOOTSTRAP_VERSION: u32 = 3;
pub const MINIMUM_AGY_VERSION: (u32, u32, u32) = (1, 1, 15);
const AGY_INIT_TIMEOUT: Duration = Duration::from_secs(20);
const AGY_STOP_GRACE: Duration = Duration::from_secs(3);
const STDERR_TAIL_LINES: usize = 80;
const LIVE_MESSAGE_LIMIT: usize = 256;
const LIVE_MESSAGE_CHAR_LIMIT: usize = 16_384;
const TURN_TRACE_LIMIT: usize = 24;

fn normalize_model(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgyTurnResult {
    pub conversation_id: String,
    pub turn_id: String,
    pub status: String,
    pub response: String,
    pub error: Option<String>,
    pub eval_events: Vec<crate::llm_eval::EvalEvent>,
    pub started_at: i64,
    pub completed_at: i64,
}

pub struct AgyTurnStarted {
    pub conversation_id: String,
    pub turn_id: String,
    pub process: AgyProcessIdentity,
    pub result: oneshot::Receiver<AppResult<AgyTurnResult>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgyProcessIdentity {
    pub run_id: String,
    pub pid: u32,
    pub process_group_id: Option<i32>,
    pub executable: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleAgyDelivery {
    pub queue_id: String,
    pub conversation_id: String,
    pub process: Option<AgyProcessIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgyProcessReapOutcome {
    StoppedOwnedProcessGroup(i32),
    StoppedLegacyProcess(u32),
    AlreadyStopped,
    IdentityMismatch(String),
    Unverified(String),
}

pub trait AgyProcessReaper {
    fn reap(&self, delivery: &StaleAgyDelivery) -> AgyProcessReapOutcome;
}

pub struct SystemAgyProcessReaper;

#[derive(Clone)]
pub struct AgyProviderSupervisor {
    inner: Arc<AgySupervisorInner>,
}

struct AgySupervisorInner {
    state: Mutex<AgySupervisorState>,
    startup: Mutex<()>,
    activation: Mutex<()>,
}

struct AgySupervisorState {
    sessions: HashMap<String, AgySession>,
    turn_traces: HashMap<String, Vec<ProviderTurnTrace>>,
    failed_eval_turns: HashMap<(String, String), AgyTurnResult>,
    stderr_tail: VecDeque<String>,
    app_handle: Option<tauri::AppHandle>,
}

struct AgySession {
    conversation_id: Option<String>,
    model: Option<String>,
    endpoint_identity: Option<String>,
    policy: ProviderTurnPolicy,
    stdin: Arc<Mutex<ChildStdin>>,
    pid: u32,
    process: AgyProcessIdentity,
    kill: mpsc::Sender<()>,
    runtime: CodexTakeoverRuntime,
    live_messages: Vec<CodexDialogueMessage>,
    eval_events: Vec<crate::llm_eval::EvalEvent>,
    eval_started_at: Option<i64>,
    init_waiter: Option<oneshot::Sender<AppResult<String>>>,
    active_result: Option<oneshot::Sender<AppResult<AgyTurnResult>>>,
}

impl Default for AgyProviderSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl AgyProviderSupervisor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AgySupervisorInner {
                state: Mutex::new(AgySupervisorState {
                    sessions: HashMap::new(),
                    turn_traces: HashMap::new(),
                    failed_eval_turns: HashMap::new(),
                    stderr_tail: VecDeque::new(),
                    app_handle: None,
                }),
                startup: Mutex::new(()),
                activation: Mutex::new(()),
            }),
        }
    }

    pub async fn set_app_handle(&self, app_handle: tauri::AppHandle) {
        self.inner.state.lock().await.app_handle = Some(app_handle);
    }

    pub async fn runtime(&self, conversation_id: &str) -> CodexTakeoverRuntime {
        self.find_session(conversation_id)
            .await
            .map(|(_, runtime, _, _, _, _)| runtime)
            .unwrap_or_default()
    }

    pub async fn live_messages(&self, conversation_id: &str) -> Vec<CodexDialogueMessage> {
        self.find_session(conversation_id)
            .await
            .map(|(_, _, messages, _, _, _)| messages)
            .unwrap_or_default()
    }

    pub async fn turn_traces(&self, conversation_id: &str) -> Vec<ProviderTurnTrace> {
        self.inner
            .state
            .lock()
            .await
            .turn_traces
            .get(conversation_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn take_failed_turn_result(
        &self,
        conversation_id: &str,
        turn_id: &str,
    ) -> Option<AgyTurnResult> {
        self.inner
            .state
            .lock()
            .await
            .failed_eval_turns
            .remove(&(conversation_id.to_string(), turn_id.to_string()))
    }

    pub async fn has_compatible_session(
        &self,
        conversation_id: &str,
        model: Option<&str>,
        endpoint_identity: Option<&str>,
    ) -> bool {
        self.has_compatible_session_with_policy(
            conversation_id,
            model,
            endpoint_identity,
            ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify),
        )
        .await
    }

    pub async fn has_compatible_session_with_policy(
        &self,
        conversation_id: &str,
        model: Option<&str>,
        endpoint_identity: Option<&str>,
        policy: ProviderTurnPolicy,
    ) -> bool {
        let requested_model = normalize_model(model);
        let requested_endpoint = normalize_model(endpoint_identity);
        self.find_session(conversation_id).await.is_some_and(
            |(_, _, _, session_model, session_endpoint, session_policy)| {
                session_model == requested_model
                    && session_endpoint == requested_endpoint
                    && session_policy == policy
            },
        )
    }

    pub async fn activate_conversation(
        &self,
        conversation_id: &str,
        cwd: &str,
        model: Option<&str>,
    ) -> AppResult<()> {
        let _activation = self.inner.activation.lock().await;
        let requested_model = normalize_model(model);
        if let Some((key, runtime, _, session_model, _, _)) =
            self.find_session(conversation_id).await
        {
            if runtime.active_turn_id.is_some() || session_model == requested_model {
                return Ok(());
            }
            self.discard_session(&key).await;
        }
        let (session_key, init) = self
            .spawn_session(
                cwd,
                Some(conversation_id.to_string()),
                requested_model,
                None,
                ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify),
            )
            .await?;
        match tokio::time::timeout(AGY_INIT_TIMEOUT, init).await {
            Ok(Ok(Ok(resumed))) if resumed == conversation_id => Ok(()),
            Ok(Ok(Ok(resumed))) => {
                self.discard_session(&session_key).await;
                Err(AppError::provider(format!(
                    "Agy activated unexpected conversation {resumed}; expected {conversation_id}."
                )))
            }
            Ok(Ok(Err(error))) => {
                self.discard_session(&session_key).await;
                Err(error)
            }
            Ok(Err(_)) => {
                self.discard_session(&session_key).await;
                Err(AppError::provider(
                    "Agy activation stream closed before init.",
                ))
            }
            Err(_) => {
                self.discard_session(&session_key).await;
                Err(AppError::provider(format!(
                    "Agy activation did not emit init within {} seconds. {}",
                    AGY_INIT_TIMEOUT.as_secs(),
                    self.stderr_detail().await
                )))
            }
        }
    }

    async fn find_session(
        &self,
        conversation_id: &str,
    ) -> Option<(
        String,
        CodexTakeoverRuntime,
        Vec<CodexDialogueMessage>,
        Option<String>,
        Option<String>,
        ProviderTurnPolicy,
    )> {
        let state = self.inner.state.lock().await;
        state.sessions.iter().find_map(|(key, session)| {
            (session.conversation_id.as_deref() == Some(conversation_id)).then(|| {
                (
                    key.clone(),
                    session.runtime.clone(),
                    session.live_messages.clone(),
                    session.model.clone(),
                    session.endpoint_identity.clone(),
                    session.policy,
                )
            })
        })
    }

    pub async fn start_new_turn(
        &self,
        cwd: &str,
        prompt: &str,
        model: Option<&str>,
        endpoint_identity: Option<&str>,
    ) -> AppResult<AgyTurnStarted> {
        self.start_new_turn_with_policy(
            cwd,
            prompt,
            model,
            endpoint_identity,
            ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify),
        )
        .await
    }

    pub async fn start_new_turn_with_policy(
        &self,
        cwd: &str,
        prompt: &str,
        model: Option<&str>,
        endpoint_identity: Option<&str>,
        policy: ProviderTurnPolicy,
    ) -> AppResult<AgyTurnStarted> {
        let (session_key, init) = self
            .spawn_session(
                cwd,
                None,
                normalize_model(model),
                normalize_model(endpoint_identity),
                policy,
            )
            .await?;
        let (turn_id, process, result) = self.write_prompt(&session_key, prompt).await?;
        let conversation_id = match tokio::time::timeout(AGY_INIT_TIMEOUT, init).await {
            Ok(Ok(Ok(id))) => id,
            Ok(Ok(Err(error))) => return Err(error),
            Ok(Err(_)) => {
                return Err(AppError::provider(
                    "Agy stream closed before the init event was delivered.",
                ))
            }
            Err(_) => {
                self.kill_session(&session_key).await;
                return Err(AppError::provider(format!(
                    "Agy did not emit init within {} seconds. {}",
                    AGY_INIT_TIMEOUT.as_secs(),
                    self.stderr_detail().await
                )));
            }
        };
        Ok(AgyTurnStarted {
            conversation_id,
            turn_id,
            process,
            result,
        })
    }

    pub async fn start_turn(
        &self,
        conversation_id: &str,
        cwd: &str,
        prompt: &str,
        model: Option<&str>,
        endpoint_identity: Option<&str>,
    ) -> AppResult<AgyTurnStarted> {
        self.start_turn_with_policy(
            conversation_id,
            cwd,
            prompt,
            model,
            endpoint_identity,
            ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify),
        )
        .await
    }

    pub async fn start_turn_with_policy(
        &self,
        conversation_id: &str,
        cwd: &str,
        prompt: &str,
        model: Option<&str>,
        endpoint_identity: Option<&str>,
        policy: ProviderTurnPolicy,
    ) -> AppResult<AgyTurnStarted> {
        let _activation = self.inner.activation.lock().await;
        let requested_model = normalize_model(model);
        let requested_endpoint = normalize_model(endpoint_identity);
        let session_key = match self.find_session(conversation_id).await {
            Some((key, runtime, _, session_model, session_endpoint, session_policy))
                if runtime.active_turn_id.is_none() =>
            {
                if runtime.phase == "idle"
                    && session_model == requested_model
                    && session_endpoint == requested_endpoint
                    && session_policy == policy
                {
                    key
                } else {
                    self.discard_session(&key).await;
                    let (key, init) = self
                        .spawn_session(
                            cwd,
                            Some(conversation_id.to_string()),
                            requested_model.clone(),
                            requested_endpoint.clone(),
                            policy,
                        )
                        .await?;
                    let (turn_id, process, result) = self.write_prompt(&key, prompt).await?;
                    match tokio::time::timeout(AGY_INIT_TIMEOUT, init).await {
                        Ok(Ok(Ok(resumed))) if resumed == conversation_id => {
                            return Ok(AgyTurnStarted {
                                conversation_id: conversation_id.to_string(),
                                turn_id,
                                process,
                                result,
                            });
                        }
                        Ok(Ok(Ok(resumed))) => {
                            self.kill_session(&key).await;
                            return Err(AppError::provider(format!(
                                "Agy resumed unexpected conversation {resumed}; expected {conversation_id}."
                            )));
                        }
                        Ok(Ok(Err(error))) => return Err(error),
                        Ok(Err(_)) => {
                            return Err(AppError::provider("Agy resume stream closed before init."))
                        }
                        Err(_) => {
                            self.kill_session(&key).await;
                            return Err(AppError::provider(format!(
                                "Agy resume did not emit init within {} seconds. {}",
                                AGY_INIT_TIMEOUT.as_secs(),
                                self.stderr_detail().await
                            )));
                        }
                    }
                }
            }
            Some((_, runtime, _, _, _, _)) => {
                return Err(AppError::conflict(format!(
                    "Agy conversation {conversation_id} already has active turn {}.",
                    runtime.active_turn_id.as_deref().unwrap_or("unknown")
                )))
            }
            None => {
                let (key, init) = self
                    .spawn_session(
                        cwd,
                        Some(conversation_id.to_string()),
                        requested_model.clone(),
                        requested_endpoint.clone(),
                        policy,
                    )
                    .await?;
                let (turn_id, process, result) = self.write_prompt(&key, prompt).await?;
                match tokio::time::timeout(AGY_INIT_TIMEOUT, init).await {
                    Ok(Ok(Ok(resumed))) if resumed == conversation_id => {
                        return Ok(AgyTurnStarted {
                            conversation_id: conversation_id.to_string(),
                            turn_id,
                            process,
                            result,
                        });
                    }
                    Ok(Ok(Ok(resumed))) => {
                        self.kill_session(&key).await;
                        return Err(AppError::provider(format!(
                            "Agy resumed unexpected conversation {resumed}; expected {conversation_id}."
                        )));
                    }
                    Ok(Ok(Err(error))) => return Err(error),
                    Ok(Err(_)) => {
                        return Err(AppError::provider("Agy resume stream closed before init."))
                    }
                    Err(_) => {
                        self.kill_session(&key).await;
                        return Err(AppError::provider(format!(
                            "Agy resume did not emit init within {} seconds. {}",
                            AGY_INIT_TIMEOUT.as_secs(),
                            self.stderr_detail().await
                        )));
                    }
                }
            }
        };
        let (turn_id, process, result) = self.write_prompt(&session_key, prompt).await?;
        Ok(AgyTurnStarted {
            conversation_id: conversation_id.to_string(),
            turn_id,
            process,
            result,
        })
    }

    pub async fn stop_turn(&self, conversation_id: &str, turn_id: &str) -> AppResult<()> {
        let (session_key, pid) =
            {
                let mut state = self.inner.state.lock().await;
                let Some((key, session)) = state.sessions.iter_mut().find(|(_, session)| {
                    session.conversation_id.as_deref() == Some(conversation_id)
                }) else {
                    return Err(AppError::not_found(format!(
                        "Agy conversation {conversation_id} has no live process."
                    )));
                };
                if session.runtime.active_turn_id.as_deref() != Some(turn_id) {
                    return Err(AppError::conflict(format!(
                        "Agy active turn changed; expected {turn_id}, current {}.",
                        session.runtime.active_turn_id.as_deref().unwrap_or("none")
                    )));
                }
                session.runtime.phase = "stopping".to_string();
                (key.clone(), session.pid)
            };

        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as i32), libc::SIGINT);
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
        }
        let supervisor = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(AGY_STOP_GRACE).await;
            let still_active = supervisor
                .inner
                .state
                .lock()
                .await
                .sessions
                .get(&session_key)
                .is_some_and(|session| session.runtime.active_turn_id.is_some());
            if still_active {
                supervisor.kill_session(&session_key).await;
            }
        });
        Ok(())
    }

    async fn spawn_session(
        &self,
        cwd: &str,
        resume_conversation_id: Option<String>,
        model: Option<String>,
        endpoint_identity: Option<String>,
        policy: ProviderTurnPolicy,
    ) -> AppResult<(String, oneshot::Receiver<AppResult<String>>)> {
        let _startup = self.inner.startup.lock().await;
        let resolved = resolve_provider_executable("agy", "ECKY_AGY_BIN", "Antigravity CLI")?;
        let version_output = Command::new(&resolved.path)
            .arg("--version")
            .env("PATH", &resolved.spawn_path)
            .output()
            .await
            .map_err(|error| {
                AppError::provider(format!(
                    "Failed to run Antigravity CLI '{} --version': {error}",
                    resolved.path.display()
                ))
            })?;
        let version_text = format!(
            "{}{}",
            String::from_utf8_lossy(&version_output.stdout),
            String::from_utf8_lossy(&version_output.stderr)
        );
        if !version_output.status.success() {
            return Err(AppError::provider(format!(
                "Antigravity CLI version probe failed: {}",
                version_text.trim()
            )));
        }
        ensure_supported_agy_version(&version_text)?;

        let run_id = uuid::Uuid::new_v4().to_string();
        let mut command = Command::new(&resolved.path);
        command
            .arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--print-timeout")
            .arg("30m")
            .arg("--sandbox")
            .arg("--mode")
            .arg(policy.execution_mode())
            .env("PATH", &resolved.spawn_path)
            .env("ECKY_PROVIDER_RUN_ID", &run_id)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .arg("--dangerously-skip-permissions");
        #[cfg(unix)]
        command.process_group(0);
        if let Some(conversation_id) = &resume_conversation_id {
            command.arg("--conversation").arg(conversation_id);
        }
        if let Some(model) = &model {
            command.arg("--model").arg(model);
        }
        let mut child = command.spawn().map_err(|error| {
            AppError::provider(format!(
                "Failed to start Antigravity CLI in '{cwd}': {error}"
            ))
        })?;
        let pid = child
            .id()
            .ok_or_else(|| AppError::provider("Agy child process has no process id."))?;
        let process = AgyProcessIdentity {
            run_id,
            pid,
            process_group_id: if cfg!(unix) { Some(pid as i32) } else { None },
            executable: resolved.path.to_string_lossy().into_owned(),
        };
        let stdin =
            Arc::new(Mutex::new(child.stdin.take().ok_or_else(|| {
                AppError::provider("Agy did not expose stdin.")
            })?));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::provider("Agy did not expose stdout."))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::provider("Agy did not expose stderr."))?;
        let (init_tx, init_rx) = oneshot::channel();
        let (kill, mut kill_rx) = mpsc::channel(1);
        let session_key = uuid::Uuid::new_v4().to_string();
        self.inner.state.lock().await.sessions.insert(
            session_key.clone(),
            AgySession {
                conversation_id: resume_conversation_id,
                model,
                endpoint_identity,
                policy,
                stdin,
                pid,
                process,
                kill,
                runtime: CodexTakeoverRuntime::default(),
                live_messages: Vec::new(),
                eval_events: Vec::new(),
                eval_started_at: None,
                init_waiter: Some(init_tx),
                active_result: None,
            },
        );

        let reader = self.clone();
        let reader_key = session_key.clone();
        tokio::spawn(async move {
            reader.read_stdout(reader_key, stdout).await;
        });
        let stderr_reader = self.clone();
        tokio::spawn(async move {
            stderr_reader.read_stderr(stderr).await;
        });
        let waiter = self.clone();
        let waiter_key = session_key.clone();
        tokio::spawn(async move {
            let status = tokio::select! {
                status = child.wait() => status,
                _ = kill_rx.recv() => {
                    let _ = child.kill().await;
                    child.wait().await
                }
            };
            waiter.handle_exit(&waiter_key, status).await;
        });
        Ok((session_key, init_rx))
    }

    async fn write_prompt(
        &self,
        session_key: &str,
        prompt: &str,
    ) -> AppResult<(
        String,
        AgyProcessIdentity,
        oneshot::Receiver<AppResult<AgyTurnResult>>,
    )> {
        if prompt.trim().is_empty() {
            return Err(AppError::validation("Agy prompt must not be empty."));
        }
        let (stdin, turn_id, process, result_rx) = {
            let mut state = self.inner.state.lock().await;
            let session = state
                .sessions
                .get_mut(session_key)
                .ok_or_else(|| AppError::provider("Agy process exited before prompt delivery."))?;
            if session.runtime.active_turn_id.is_some() {
                return Err(AppError::conflict("Agy turn is already active."));
            }
            let turn_id = uuid::Uuid::new_v4().to_string();
            let (result_tx, result_rx) = oneshot::channel();
            session.runtime = CodexTakeoverRuntime {
                phase: "active".to_string(),
                active_turn_id: Some(turn_id.clone()),
                error: None,
            };
            session.live_messages.clear();
            session.eval_events.clear();
            session.eval_started_at = Some(now_seconds());
            session.active_result = Some(result_tx);
            (
                session.stdin.clone(),
                turn_id,
                session.process.clone(),
                result_rx,
            )
        };
        let line = serde_json::to_string(&serde_json::json!({
            "event": "user",
            "message": { "content": prompt }
        }))
        .map_err(|error| AppError::provider(format!("Failed to encode Agy prompt: {error}")))?;
        let write_result = async {
            let mut stdin = stdin.lock().await;
            stdin.write_all(line.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        }
        .await;
        if let Err(error) = write_result {
            self.fail_session(
                session_key,
                AppError::provider(format!("Failed to write Agy prompt: {error}")),
            )
            .await;
            return Err(AppError::provider(format!(
                "Failed to write Agy prompt: {error}"
            )));
        }
        self.emit_update(session_key, "turn/started").await;
        Ok((turn_id, process, result_rx))
    }

    async fn read_stdout<R>(&self, session_key: String, stdout: R)
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) if line.trim().is_empty() => continue,
                Ok(Some(line)) => match serde_json::from_str::<Value>(&line) {
                    Ok(value) => {
                        if let Some(event) =
                            crate::llm_eval::project_agy_eval_event(&value, now_seconds())
                        {
                            self.append_eval_event(&session_key, event).await;
                        }
                        match project_stream_event(&value) {
                            Ok(Some(event)) => self.apply_event(&session_key, event).await,
                            Ok(None) => {}
                            Err(error) => {
                                self.fail_session(&session_key, error).await;
                                self.kill_session(&session_key).await;
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        self.fail_session(
                            &session_key,
                            AppError::provider(format!(
                                "Agy emitted invalid stream-json: {error}; line: {line}"
                            )),
                        )
                        .await;
                        self.kill_session(&session_key).await;
                        break;
                    }
                },
                Ok(None) => break,
                Err(error) => {
                    self.fail_session(
                        &session_key,
                        AppError::provider(format!("Failed reading Agy stdout: {error}")),
                    )
                    .await;
                    break;
                }
            }
        }
    }

    async fn append_eval_event(&self, session_key: &str, mut event: crate::llm_eval::EvalEvent) {
        let mut state = self.inner.state.lock().await;
        let Some(session) = state.sessions.get_mut(session_key) else {
            return;
        };
        event.sequence = session.eval_events.len() as u64 + 1;
        session.eval_events.push(event);
    }

    async fn read_stderr<R>(&self, stderr: R)
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut state = self.inner.state.lock().await;
            state.stderr_tail.push_back(line);
            while state.stderr_tail.len() > STDERR_TAIL_LINES {
                state.stderr_tail.pop_front();
            }
        }
    }

    async fn apply_event(&self, session_key: &str, event: AgyProjectedEvent) {
        let method = match &event {
            AgyProjectedEvent::Init { .. } => "session/init",
            AgyProjectedEvent::Result { .. } => "turn/result",
            _ => "turn/progress",
        };
        let mut state = self.inner.state.lock().await;
        let Some(session) = state.sessions.get_mut(session_key) else {
            return;
        };
        let mut terminal_trace: Option<(String, ProviderTurnTrace)> = None;
        match event {
            AgyProjectedEvent::Init { conversation_id } => {
                session.conversation_id = Some(conversation_id.clone());
                if let Some(waiter) = session.init_waiter.take() {
                    let _ = waiter.send(Ok(conversation_id));
                }
            }
            AgyProjectedEvent::AssistantDelta {
                step_index, text, ..
            } => append_live_delta(session, step_index, &text),
            AgyProjectedEvent::Working {
                step_index, text, ..
            } => upsert_live_working(session, step_index, &text),
            AgyProjectedEvent::Result {
                conversation_id,
                mut status,
                mut response,
                mut error,
            } => {
                let completed_at = now_seconds();
                let was_stopping = session.runtime.phase == "stopping";
                if was_stopping {
                    status = "INTERRUPTED".to_string();
                    response.clear();
                    error = None;
                }
                let turn_id = session.runtime.active_turn_id.take().unwrap_or_default();
                let trace_status = agy_trace_status(&status);
                session.runtime.phase = if was_stopping { "stopped" } else { "idle" }.to_string();
                session.runtime.error =
                    if matches!(status.as_str(), "SUCCESS" | "CANCELED" | "INTERRUPTED") {
                        None
                    } else {
                        Some(error.clone().unwrap_or_else(|| status.clone()))
                    };
                let message_status = match trace_status {
                    "success" => "success",
                    "interrupted" => "discarded",
                    _ => "error",
                };
                for message in &mut session.live_messages {
                    message.status = message_status.to_string();
                }
                let trace_messages = std::mem::take(&mut session.live_messages);
                if !trace_messages.is_empty() {
                    terminal_trace = Some((
                        conversation_id.clone(),
                        ProviderTurnTrace {
                            turn_id: turn_id.clone(),
                            status: trace_status.to_string(),
                            messages: trace_messages,
                            completed_at: now_seconds(),
                        },
                    ));
                }
                if let Some(waiter) = session.active_result.take() {
                    let _ = waiter.send(Ok(AgyTurnResult {
                        conversation_id,
                        turn_id,
                        status,
                        response,
                        error,
                        eval_events: std::mem::take(&mut session.eval_events),
                        started_at: session.eval_started_at.take().unwrap_or(completed_at),
                        completed_at,
                    }));
                }
            }
        }
        if let Some((conversation_id, trace)) = terminal_trace {
            push_turn_trace(&mut state.turn_traces, &conversation_id, trace);
        }
        drop(state);
        self.emit_update(session_key, method).await;
    }

    async fn fail_session(&self, session_key: &str, error: AppError) {
        let mut state = self.inner.state.lock().await;
        let Some(session) = state.sessions.get_mut(session_key) else {
            return;
        };
        let turn_id = session.runtime.active_turn_id.clone().unwrap_or_default();
        let conversation_id = session.conversation_id.clone();
        if let Some(waiter) = session.init_waiter.take() {
            let _ = waiter.send(Err(error.clone()));
        }
        let completed_at = now_seconds();
        let failed_eval = session.active_result.is_some().then(|| AgyTurnResult {
            conversation_id: conversation_id
                .clone()
                .unwrap_or_else(|| session_key.to_string()),
            turn_id: turn_id.clone(),
            status: "ERROR".into(),
            response: String::new(),
            error: Some(super::codex_takeover::error_text(&error)),
            eval_events: std::mem::take(&mut session.eval_events),
            started_at: session.eval_started_at.take().unwrap_or(completed_at),
            completed_at,
        });
        if let Some(waiter) = session.active_result.take() {
            let _ = waiter.send(Err(error.clone()));
        }
        session.runtime.phase = "idle".to_string();
        session.runtime.active_turn_id = None;
        session.runtime.error = Some(super::codex_takeover::error_text(&error));
        for message in &mut session.live_messages {
            message.status = "error".to_string();
        }
        let messages = std::mem::take(&mut session.live_messages);
        if let Some(failed_eval) = failed_eval {
            if state.failed_eval_turns.len() >= TURN_TRACE_LIMIT * 4 {
                if let Some(key) = state.failed_eval_turns.keys().next().cloned() {
                    state.failed_eval_turns.remove(&key);
                }
            }
            state.failed_eval_turns.insert(
                (
                    failed_eval.conversation_id.clone(),
                    failed_eval.turn_id.clone(),
                ),
                failed_eval,
            );
        }
        if let Some(conversation_id) = conversation_id.filter(|_| !messages.is_empty()) {
            push_turn_trace(
                &mut state.turn_traces,
                &conversation_id,
                ProviderTurnTrace {
                    turn_id,
                    status: "error".to_string(),
                    messages,
                    completed_at: now_seconds(),
                },
            );
        }
        drop(state);
        self.emit_update(session_key, "turn/terminal").await;
    }

    async fn handle_exit(
        &self,
        session_key: &str,
        status: std::io::Result<std::process::ExitStatus>,
    ) {
        let (active, stopping_conversation) = self
            .inner
            .state
            .lock()
            .await
            .sessions
            .get(session_key)
            .map(|session| {
                (
                    session.runtime.active_turn_id.is_some() || session.init_waiter.is_some(),
                    (session.runtime.phase == "stopping")
                        .then(|| session.conversation_id.clone())
                        .flatten(),
                )
            })
            .unwrap_or((false, None));
        if !active {
            self.inner.state.lock().await.sessions.remove(session_key);
            return;
        }
        if let Some(conversation_id) = stopping_conversation {
            self.apply_event(
                session_key,
                AgyProjectedEvent::Result {
                    conversation_id,
                    status: "INTERRUPTED".to_string(),
                    response: String::new(),
                    error: None,
                },
            )
            .await;
            self.inner.state.lock().await.sessions.remove(session_key);
            return;
        }
        let detail = self.stderr_detail().await;
        let status_text = status
            .map(|status| status.to_string())
            .unwrap_or_else(|error| error.to_string());
        self.fail_session(
            session_key,
            AppError::provider(format!(
                "Agy process exited ({status_text}) before the active turn completed. {detail}"
            )),
        )
        .await;
        self.inner.state.lock().await.sessions.remove(session_key);
    }

    async fn kill_session(&self, session_key: &str) {
        let process = self
            .inner
            .state
            .lock()
            .await
            .sessions
            .get(session_key)
            .map(|session| (session.pid, session.kill.clone()));
        if let Some((pid, kill)) = process {
            kill_owned_process_group(pid);
            let _ = kill.send(()).await;
        }
    }

    async fn discard_session(&self, session_key: &str) {
        let process = self
            .inner
            .state
            .lock()
            .await
            .sessions
            .remove(session_key)
            .map(|session| (session.pid, session.kill));
        if let Some((pid, kill)) = process {
            kill_owned_process_group(pid);
            let _ = kill.send(()).await;
        }
    }

    pub async fn discard_conversation_session(&self, conversation_id: &str) {
        let session_key = {
            let state = self.inner.state.lock().await;
            state.sessions.iter().find_map(|(key, session)| {
                if session.conversation_id.as_deref() == Some(conversation_id) {
                    Some(key.clone())
                } else {
                    None
                }
            })
        };
        if let Some(key) = session_key {
            self.discard_session(&key).await;
        }
    }

    pub async fn shutdown_all(&self) {
        let sessions = {
            let state = self.inner.state.lock().await;
            state
                .sessions
                .iter()
                .map(|(key, session)| (key.clone(), session.pid, session.kill.clone()))
                .collect::<Vec<_>>()
        };
        for (key, _, _) in &sessions {
            self.fail_session(
                key,
                AppError::provider(
                    "Ecky shutdown stopped the active Agy process; the turn was not replayed.",
                ),
            )
            .await;
        }
        for (_, pid, kill) in sessions {
            kill_owned_process_group(pid);
            let _ = kill.send(()).await;
        }
    }

    async fn stderr_detail(&self) -> String {
        let state = self.inner.state.lock().await;
        if state.stderr_tail.is_empty() {
            "Agy stderr was empty.".to_string()
        } else {
            format!(
                "Agy stderr:\n{}",
                state
                    .stderr_tail
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
    }

    async fn emit_update(&self, session_key: &str, method: &str) {
        let (app_handle, conversation_id, runtime, live_messages, turn_traces) = {
            let state = self.inner.state.lock().await;
            let Some(session) = state.sessions.get(session_key) else {
                return;
            };
            let conversation_id = session.conversation_id.clone();
            let turn_traces = conversation_id
                .as_deref()
                .and_then(|id| state.turn_traces.get(id))
                .cloned()
                .unwrap_or_default();
            (
                state.app_handle.clone(),
                conversation_id,
                session.runtime.clone(),
                session.live_messages.clone(),
                turn_traces,
            )
        };
        let Some(app_handle) = app_handle else { return };
        let _ = app_handle.emit(
            "agy-provider-updated",
            serde_json::json!({
                "conversationId": conversation_id,
                "method": method,
                "runtime": runtime,
                "liveMessages": live_messages,
                "turnTraces": turn_traces,
            }),
        );
    }
}

#[cfg(unix)]
fn kill_owned_process_group(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_owned_process_group(_pid: u32) {}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn append_live_delta(session: &mut AgySession, step_index: i64, delta: &str) {
    let id = format!("agy:live:answer:{step_index}");
    if let Some(message) = session
        .live_messages
        .iter_mut()
        .find(|message| message.id == id)
    {
        message.content.push_str(delta);
        if message.content.chars().count() > LIVE_MESSAGE_CHAR_LIMIT {
            message.content = message
                .content
                .chars()
                .take(LIVE_MESSAGE_CHAR_LIMIT.saturating_sub(1))
                .collect::<String>()
                + "…";
        }
    } else {
        session.live_messages.push(CodexDialogueMessage {
            id,
            role: "assistant".to_string(),
            content: delta.chars().take(LIVE_MESSAGE_CHAR_LIMIT).collect(),
            status: "working".to_string(),
            timestamp: now_seconds(),
            attachments: Vec::new(),
            provider_event_kind: Some(ProviderEventKind::Assistant),
        });
    }
    trim_live_messages(session);
}

fn upsert_live_working(session: &mut AgySession, step_index: i64, text: &str) {
    let id = format!("agy:live:working:{step_index}");
    if let Some(message) = session
        .live_messages
        .iter_mut()
        .find(|message| message.id == id)
    {
        message.content = text.chars().take(LIVE_MESSAGE_CHAR_LIMIT).collect();
    } else {
        session.live_messages.push(CodexDialogueMessage {
            id,
            role: "assistant".to_string(),
            content: text.chars().take(LIVE_MESSAGE_CHAR_LIMIT).collect(),
            status: "working".to_string(),
            timestamp: now_seconds(),
            attachments: Vec::new(),
            provider_event_kind: Some(ProviderEventKind::Activity),
        });
    }
    trim_live_messages(session);
}

fn trim_live_messages(session: &mut AgySession) {
    if session.live_messages.len() > LIVE_MESSAGE_LIMIT {
        session
            .live_messages
            .drain(..session.live_messages.len() - LIVE_MESSAGE_LIMIT);
    }
}

fn agy_trace_status(status: &str) -> &'static str {
    match status {
        "SUCCESS" => "success",
        "CANCELED" | "INTERRUPTED" => "interrupted",
        _ => "error",
    }
}

fn push_turn_trace(
    traces: &mut HashMap<String, Vec<ProviderTurnTrace>>,
    conversation_id: &str,
    trace: ProviderTurnTrace,
) {
    let conversation_traces = traces.entry(conversation_id.to_string()).or_default();
    conversation_traces.push(trace);
    if conversation_traces.len() > TURN_TRACE_LIMIT {
        conversation_traces.drain(..conversation_traces.len() - TURN_TRACE_LIMIT);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgyProjectedEvent {
    Init {
        conversation_id: String,
    },
    AssistantDelta {
        conversation_id: String,
        step_index: i64,
        text: String,
    },
    Working {
        conversation_id: String,
        step_index: i64,
        text: String,
    },
    Result {
        conversation_id: String,
        status: String,
        response: String,
        error: Option<String>,
    },
}

pub fn parse_agy_version(output: &str) -> AppResult<(u32, u32, u32)> {
    for token in output.split_whitespace() {
        let normalized =
            token.trim_matches(|character: char| !character.is_ascii_digit() && character != '.');
        let mut parts = normalized.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if let (Ok(major), Ok(minor), Ok(patch)) = (major.parse(), minor.parse(), patch.parse()) {
            return Ok((major, minor, patch));
        }
    }
    Err(AppError::provider(format!(
        "Could not parse Antigravity CLI version from: {}",
        output.trim()
    )))
}

pub fn ensure_supported_agy_version(output: &str) -> AppResult<(u32, u32, u32)> {
    let version = parse_agy_version(output)?;
    if version < MINIMUM_AGY_VERSION {
        return Err(AppError::provider(format!(
            "Antigravity CLI {}.{}.{} does not support bidirectional stream-json; Ecky requires >=1.1.15. Run `agy update`.",
            version.0, version.1, version.2
        )));
    }
    Ok(version)
}

fn mcp_field<'a>(object: &'a serde_json::Map<String, Value>, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| object.get(*name))
}

fn find_nested_mcp_call(value: &Value) -> Option<(String, String)> {
    match value {
        Value::Object(object) => {
            let server = mcp_field(object, &["ServerName", "serverName"])
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|value| !value.is_empty());
            let tool = mcp_field(object, &["ToolName", "toolName"])
                .and_then(Value::as_str)
                .map(|s| s.trim_matches('"').trim_matches('\'').trim())
                .filter(|value| !value.is_empty());
            if let (Some(server), Some(tool)) = (server, tool) {
                return Some((server.to_string(), tool.to_string()));
            }

            [
                "parameters",
                "params",
                "input",
                "toolInput",
                "tool_input",
                "arguments",
                "args",
            ]
            .iter()
            .filter_map(|key| object.get(*key))
            .find_map(find_nested_mcp_call)
            .or_else(|| object.values().find_map(find_nested_mcp_call))
        }
        Value::Array(values) => values.iter().find_map(find_nested_mcp_call),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .as_ref()
            .and_then(find_nested_mcp_call),
        _ => None,
    }
}

fn normalize_public_tool_text(text: &str) -> Option<String> {
    let unquoted = text.trim_matches('"').trim_matches('\'');
    let text = unquoted.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

fn public_tool_text_from_value(value: &Value, names: &[&str]) -> Option<String> {
    match value {
        Value::Object(object) => mcp_field(object, names)
            .and_then(Value::as_str)
            .and_then(normalize_public_tool_text),
        Value::String(encoded) => serde_json::from_str::<Value>(encoded)
            .ok()
            .as_ref()
            .and_then(|decoded| public_tool_text_from_value(decoded, names)),
        Value::Array(values) => values
            .iter()
            .find_map(|value| public_tool_text_from_value(value, names)),
        _ => None,
    }
}

fn public_tool_text(update: &serde_json::Map<String, Value>, names: &[&str]) -> Option<String> {
    let direct = mcp_field(update, names)
        .and_then(Value::as_str)
        .and_then(normalize_public_tool_text);
    let nested = update
        .get("tool_info")
        .and_then(Value::as_object)
        .and_then(|tool_info| {
            [
                "parameters",
                "params",
                "input",
                "toolInput",
                "tool_input",
                "arguments",
                "args",
            ]
            .iter()
            .filter_map(|key| tool_info.get(*key))
            .find_map(|input| public_tool_text_from_value(input, names))
        });
    direct.or(nested)
}

fn extract_tool_params(update: &serde_json::Map<String, Value>) -> Option<&Value> {
    if let Some(tool_info) = update.get("tool_info").and_then(Value::as_object) {
        for key in [
            "parameters",
            "params",
            "input",
            "toolInput",
            "tool_input",
            "arguments",
            "args",
        ] {
            if let Some(val) = tool_info.get(key) {
                return Some(val);
            }
        }
    }
    for key in [
        "parameters",
        "params",
        "input",
        "toolInput",
        "tool_input",
        "arguments",
        "args",
    ] {
        if let Some(val) = update.get(key) {
            return Some(val);
        }
    }
    None
}

fn project_tool_activity(update: &serde_json::Map<String, Value>) -> String {
    let wrapper_name = update
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let resolved_tool = if wrapper_name == "call_mcp_tool" {
        find_nested_mcp_call(&Value::Object(update.clone()))
            .map(|(server, tool)| format!("{server}/{tool}"))
            .unwrap_or_else(|| wrapper_name.to_string())
    } else {
        wrapper_name.to_string()
    };

    let is_error = update
        .get("state")
        .and_then(Value::as_str)
        .map(|s| s.eq_ignore_ascii_case("ERROR"))
        .unwrap_or(false);
    let error_text = update
        .get("tool_info")
        .and_then(Value::as_object)
        .and_then(|info| info.get("error"))
        .or_else(|| update.get("error"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());

    if is_error {
        if let Some(err) = error_text {
            return format!("FAILED · {resolved_tool}: {err}");
        }
    }

    let action = public_tool_text(update, &["toolAction", "tool_action"]);
    let summary = public_tool_text(update, &["toolSummary", "tool_summary"]);

    if let Some(action) = action.filter(|a| !crate::llm_eval::is_generic_tool_action(a)) {
        return format!("WORKING · {action}");
    }
    if let Some(summary) = summary.filter(|s| !crate::llm_eval::is_generic_tool_action(s)) {
        return format!("WORKING · {summary}");
    }

    let params = extract_tool_params(update);
    if let Some(detail) = crate::llm_eval::format_tool_call_details(&resolved_tool, params) {
        return detail;
    }

    format!("USING TOOL · {resolved_tool}")
}

pub fn project_stream_event(value: &Value) -> AppResult<Option<AgyProjectedEvent>> {
    match value.get("event").and_then(Value::as_str) {
        Some("init") => {
            let conversation_id = value
                .get("conversation_id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| AppError::provider("Agy init event omitted conversation_id."))?;
            Ok(Some(AgyProjectedEvent::Init {
                conversation_id: conversation_id.to_string(),
            }))
        }
        Some("step_update") => {
            let update = value
                .get("step_update")
                .and_then(Value::as_object)
                .ok_or_else(|| AppError::provider("Agy step_update event omitted payload."))?;
            let conversation_id = update
                .get("conversation_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let step_index = update
                .get("step_index")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let step_type = update
                .get("step_type")
                .and_then(Value::as_str)
                .unwrap_or("progress");
            if step_type == "user_input" {
                return Ok(None);
            }
            if step_type == "agent_response" {
                let text = update
                    .get("text_delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if text.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(AgyProjectedEvent::AssistantDelta {
                    conversation_id,
                    step_index,
                    text: text.to_string(),
                }));
            }
            if matches!(
                step_type,
                "system_message" | "unknown" | "progress" | "generic" | "checkpoint"
            ) {
                return Ok(None);
            }
            let text = if step_type == "tool" {
                project_tool_activity(update)
            } else {
                let label = step_type.replace('_', " ").to_uppercase();
                format!("WORKING · {label}")
            };
            Ok(Some(AgyProjectedEvent::Working {
                conversation_id,
                step_index,
                text,
            }))
        }
        Some("result") => {
            let result = value
                .get("result")
                .and_then(Value::as_object)
                .ok_or_else(|| AppError::provider("Agy result event omitted payload."))?;
            Ok(Some(AgyProjectedEvent::Result {
                conversation_id: result
                    .get("conversation_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                status: result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("ERROR")
                    .to_string(),
                response: result
                    .get("response")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                error: result
                    .get("error")
                    .and_then(Value::as_str)
                    .filter(|error| !error.trim().is_empty())
                    .map(str::to_string),
            }))
        }
        _ => Ok(None),
    }
}

pub fn record_process_lease(
    conn: &Connection,
    queue_id: &str,
    conversation_id: &str,
    process: &AgyProcessIdentity,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO agent_provider_process_leases (
            queue_id, provider, external_thread_id, run_id, process_id,
            process_group_id, executable, started_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(queue_id) DO UPDATE SET
            external_thread_id = excluded.external_thread_id,
            run_id = excluded.run_id,
            process_id = excluded.process_id,
            process_group_id = excluded.process_group_id,
            executable = excluded.executable,
            started_at = excluded.started_at",
        params![
            queue_id,
            AGY_PROVIDER_ID,
            conversation_id,
            process.run_id,
            process.pid,
            process.process_group_id,
            process.executable,
            now,
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(())
}

pub fn stale_deliveries(conn: &Connection) -> AppResult<Vec<StaleAgyDelivery>> {
    let mut statement = conn
        .prepare(
            "SELECT q.id, b.external_thread_id, l.run_id, l.process_id,
                    l.process_group_id, l.executable
             FROM agent_prompt_queue q
             JOIN agent_thread_bindings b
               ON b.ecky_thread_id = q.ecky_thread_id AND b.provider = q.provider
             LEFT JOIN agent_provider_process_leases l ON l.queue_id = q.id
             WHERE q.provider = ?1 AND q.status = 'sending'
             ORDER BY q.created_at ASC, q.id ASC",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = statement
        .query_map([AGY_PROVIDER_ID], |row| {
            let run_id = row.get::<_, Option<String>>(2)?;
            let process_id = row.get::<_, Option<u32>>(3)?;
            let process_group_id = row.get::<_, Option<i32>>(4)?;
            let executable = row.get::<_, Option<String>>(5)?;
            let process = match (run_id, process_id, executable) {
                (Some(run_id), Some(pid), Some(executable)) => Some(AgyProcessIdentity {
                    run_id,
                    pid,
                    process_group_id,
                    executable,
                }),
                _ => None,
            };
            Ok(StaleAgyDelivery {
                queue_id: row.get(0)?,
                conversation_id: row.get(1)?,
                process,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn reconcile_stale_deliveries(
    conn: &Connection,
    reaper: &dyn AgyProcessReaper,
    now: i64,
) -> AppResult<usize> {
    let deliveries = stale_deliveries(conn)?;
    for delivery in &deliveries {
        let outcome = reaper.reap(delivery);
        let detail = match outcome {
            AgyProcessReapOutcome::StoppedOwnedProcessGroup(process_group_id) => format!(
                "Ecky stopped orphaned Agy process group {process_group_id}."
            ),
            AgyProcessReapOutcome::StoppedLegacyProcess(pid) => {
                format!("Ecky stopped orphaned legacy Agy process {pid}.")
            }
            AgyProcessReapOutcome::AlreadyStopped => {
                "The recorded Agy process was already stopped.".to_string()
            }
            AgyProcessReapOutcome::IdentityMismatch(detail) => format!(
                "Recorded Agy process identity no longer matched; Ecky left that process untouched. {detail}"
            ),
            AgyProcessReapOutcome::Unverified(detail) => format!(
                "Ecky could not verify or stop the recorded Agy process; it may still be running. {detail}"
            ),
        };
        let error = format!(
            "Previous Ecky process exited while Agy delivery was active. {detail} Automatic replay disabled to prevent duplicate model work."
        );
        super::codex_takeover::fail_queue_item(conn, &delivery.queue_id, &error, now)?;
    }
    Ok(deliveries.len())
}

impl AgyProcessReaper for SystemAgyProcessReaper {
    fn reap(&self, delivery: &StaleAgyDelivery) -> AgyProcessReapOutcome {
        system_reap_stale_delivery(delivery)
    }
}

#[cfg(unix)]
fn system_reap_stale_delivery(delivery: &StaleAgyDelivery) -> AgyProcessReapOutcome {
    if let Some(process) = &delivery.process {
        return reap_recorded_process(process);
    }
    let Some((pid, process_group_id)) = find_legacy_owned_process(&delivery.conversation_id) else {
        return AgyProcessReapOutcome::AlreadyStopped;
    };
    let signal_target = if process_group_id == pid as i32 {
        -process_group_id
    } else {
        pid as i32
    };
    let killed = unsafe { libc::kill(signal_target, libc::SIGKILL) };
    if killed == 0 {
        AgyProcessReapOutcome::StoppedLegacyProcess(pid)
    } else {
        AgyProcessReapOutcome::Unverified(format!(
            "Failed to stop legacy PID {pid}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(unix))]
fn system_reap_stale_delivery(_delivery: &StaleAgyDelivery) -> AgyProcessReapOutcome {
    AgyProcessReapOutcome::Unverified(
        "Provider process reconciliation is not implemented on this platform.".to_string(),
    )
}

#[cfg(unix)]
fn reap_recorded_process(process: &AgyProcessIdentity) -> AgyProcessReapOutcome {
    let output = match ProcessCommand::new("ps")
        .args(["-p", &process.pid.to_string(), "-o", "pgid=,command="])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(_) => return AgyProcessReapOutcome::AlreadyStopped,
        Err(error) => return AgyProcessReapOutcome::Unverified(error.to_string()),
    };
    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return AgyProcessReapOutcome::AlreadyStopped;
    }
    let mut fields = trimmed.split_whitespace();
    let Some(actual_group_id) = fields.next().and_then(|value| value.parse::<i32>().ok()) else {
        return AgyProcessReapOutcome::Unverified(format!(
            "Could not parse process group for PID {}.",
            process.pid
        ));
    };
    let command = fields.collect::<Vec<_>>().join(" ");
    let executable_name = std::path::Path::new(&process.executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("agy");
    if process.process_group_id != Some(actual_group_id)
        || !command.contains(executable_name)
        || !looks_like_ecky_agy_command(&command)
    {
        return AgyProcessReapOutcome::IdentityMismatch(format!(
            "PID {} now reports process group {actual_group_id} and command '{command}'.",
            process.pid
        ));
    }
    if unsafe { libc::kill(-actual_group_id, libc::SIGKILL) } == 0 {
        AgyProcessReapOutcome::StoppedOwnedProcessGroup(actual_group_id)
    } else {
        AgyProcessReapOutcome::Unverified(format!(
            "Failed to stop process group {actual_group_id}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(unix)]
fn find_legacy_owned_process(conversation_id: &str) -> Option<(u32, i32)> {
    let output = ProcessCommand::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let conversation_flag = format!("--conversation {conversation_id}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let parent_id = fields.next()?.parse::<u32>().ok()?;
            let process_group_id = fields.next()?.parse::<i32>().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (parent_id == 1
                && command.contains(&conversation_flag)
                && looks_like_ecky_agy_command(&command))
            .then_some((pid, process_group_id))
        })
}

#[cfg(unix)]
fn looks_like_ecky_agy_command(command: &str) -> bool {
    command.contains("agy")
        && command.contains("--input-format stream-json")
        && command.contains("--output-format stream-json")
}

pub fn get_binding(
    conn: &Connection,
    ecky_thread_id: &str,
) -> AppResult<Option<AgyProviderBinding>> {
    Ok(super::codex_takeover::get_agent_binding_for_provider(
        conn,
        ecky_thread_id,
        AGY_PROVIDER_ID,
    )?
    .map(|binding| AgyProviderBinding {
        ecky_thread_id: binding.ecky_thread_id,
        agy_conversation_id: binding.external_thread_id,
        label: binding.external_title,
        cwd: binding.external_cwd,
        bootstrap_version: binding.bootstrap_version,
        created_at: binding.created_at,
        updated_at: binding.updated_at,
    }))
}

pub fn bind_owned_conversation(
    conn: &Connection,
    ecky_thread_id: &str,
    conversation_id: &str,
    label: &str,
    cwd: &str,
    now: i64,
) -> AppResult<AgyProviderBinding> {
    let saved = super::codex_takeover::upsert_agent_binding(
        conn,
        &super::codex_takeover::AgentThreadBindingRecord {
            ecky_thread_id: ecky_thread_id.to_string(),
            provider: AGY_PROVIDER_ID.to_string(),
            external_thread_id: conversation_id.to_string(),
            external_title: label.to_string(),
            external_cwd: cwd.to_string(),
            bootstrap_version: AGY_BOOTSTRAP_VERSION,
            created_at: now,
            updated_at: now,
        },
    )?;
    Ok(AgyProviderBinding {
        ecky_thread_id: saved.ecky_thread_id,
        agy_conversation_id: saved.external_thread_id,
        label: saved.external_title,
        cwd: saved.external_cwd,
        bootstrap_version: saved.bootstrap_version,
        created_at: saved.created_at,
        updated_at: saved.updated_at,
    })
}

pub fn rotate_binding(
    conn: &Connection,
    ecky_thread_id: &str,
    new_conversation_id: &str,
    reason: &str,
    now: i64,
) -> AppResult<AgyProviderBinding> {
    let saved = super::codex_takeover::rotate_agent_binding(
        conn,
        ecky_thread_id,
        AGY_PROVIDER_ID,
        new_conversation_id,
        reason,
        now,
    )?;
    Ok(AgyProviderBinding {
        ecky_thread_id: saved.ecky_thread_id,
        agy_conversation_id: saved.external_thread_id,
        label: saved.external_title,
        cwd: saved.external_cwd,
        bootstrap_version: saved.bootstrap_version,
        created_at: saved.created_at,
        updated_at: saved.updated_at,
    })
}

pub fn count_conversation_messages(
    conn: &Connection,
    ecky_thread_id: &str,
    conversation_id: &str,
) -> AppResult<usize> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_provider_messages
             WHERE ecky_thread_id = ?1 AND provider = ?2 AND external_thread_id = ?3",
            params![ecky_thread_id, AGY_PROVIDER_ID, conversation_id],
            |row| row.get(0),
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(count as usize)
}

pub fn enqueue_prompt(
    conn: &Connection,
    ecky_thread_id: &str,
    prompt_text: &str,
    now: i64,
) -> AppResult<CodexQueuedPrompt> {
    enqueue_prompt_with_attachments(conn, ecky_thread_id, prompt_text, &[], now)
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
            "Agy queued prompt must include text or attachments.",
        ));
    }
    if get_binding(conn, ecky_thread_id)?.is_none() {
        return Err(AppError::not_found(format!(
            "Ecky thread {ecky_thread_id} has no owned Agy conversation."
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
            AGY_PROVIDER_ID,
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
            "SELECT id, ecky_thread_id, prompt_text, attachments_json, status, error, created_at, updated_at
             FROM agent_prompt_queue
             WHERE ecky_thread_id = ?1 AND provider = ?2
             ORDER BY created_at ASC, id ASC",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map(params![ecky_thread_id, AGY_PROVIDER_ID], |row| {
            Ok(CodexQueuedPrompt {
                id: row.get(0)?,
                ecky_thread_id: row.get(1)?,
                prompt_text: row.get(2)?,
                attachments: decode_queue_attachments(row.get(3)?)?,
                status: row.get(4)?,
                can_cancel: row.get::<_, String>(4)? != "sending",
                error: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn pending_queue_bindings(conn: &Connection) -> AppResult<Vec<AgyProviderBinding>> {
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
        .query_map([AGY_PROVIDER_ID], |row| {
            Ok(AgyProviderBinding {
                ecky_thread_id: row.get(0)?,
                agy_conversation_id: row.get(1)?,
                label: row.get(2)?,
                cwd: row.get(3)?,
                bootstrap_version: row.get::<_, i64>(4)? as u32,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn queue_head(conn: &Connection, ecky_thread_id: &str) -> AppResult<Option<CodexQueuedPrompt>> {
    conn.query_row(
        "SELECT id, ecky_thread_id, prompt_text, attachments_json, status, error, created_at, updated_at
         FROM agent_prompt_queue
         WHERE ecky_thread_id = ?1 AND provider = ?2
         ORDER BY created_at ASC, id ASC LIMIT 1",
        params![ecky_thread_id, AGY_PROVIDER_ID],
        |row| {
            Ok(CodexQueuedPrompt {
                id: row.get(0)?,
                ecky_thread_id: row.get(1)?,
                prompt_text: row.get(2)?,
                attachments: decode_queue_attachments(row.get(3)?)?,
                status: row.get(4)?,
                can_cancel: row.get::<_, String>(4)? != "sending",
                error: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .optional()
    .map_err(|error| AppError::persistence(error.to_string()))
}

fn decode_queue_attachments(value: String) -> rusqlite::Result<Vec<Attachment>> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

pub fn retry_queue_item(
    conn: &Connection,
    ecky_thread_id: &str,
    id: &str,
    now: i64,
) -> AppResult<()> {
    let head = queue_head(conn, ecky_thread_id)?
        .ok_or_else(|| AppError::not_found(format!("Agy queue item {id} was not found.")))?;
    if head.id != id {
        return Err(AppError::conflict(format!(
            "Agy queue item {id} cannot overtake queue head {}.",
            head.id
        )));
    }
    let changed = conn
        .execute(
            "UPDATE agent_prompt_queue SET status = 'queued', error = NULL, updated_at = ?3
             WHERE id = ?1 AND ecky_thread_id = ?2 AND provider = 'agy' AND status = 'failed'",
            params![id, ecky_thread_id, now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    if changed == 1 {
        Ok(())
    } else {
        Err(AppError::conflict(format!(
            "Agy queue item {id} is not failed."
        )))
    }
}

pub fn remove_queue_item(conn: &Connection, ecky_thread_id: &str, id: &str) -> AppResult<()> {
    let status: Option<(String, bool)> = conn
        .query_row(
            "SELECT status, dispatch_started FROM agent_prompt_queue WHERE id = ?1 AND ecky_thread_id = ?2 AND provider = ?3",
            params![id, ecky_thread_id, AGY_PROVIDER_ID],
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
                    params![id, ecky_thread_id, AGY_PROVIDER_ID],
                )
                .map_err(|error| AppError::persistence(error.to_string()))?;
            if removed == 1 {
                Ok(())
            } else {
                Err(AppError::conflict(format!(
                    "Agy queue item {id} crossed the provider delivery boundary. Use STOP."
                )))
            }
        }
        Some((status, true)) if status == "sending" => Err(AppError::conflict(format!(
            "Agy queue item {id} crossed the provider delivery boundary. Use STOP."
        ))),
        Some(_) => {
            conn.execute(
                "DELETE FROM agent_prompt_queue WHERE id = ?1 AND ecky_thread_id = ?2 AND provider = ?3",
                params![id, ecky_thread_id, AGY_PROVIDER_ID],
            )
            .map_err(|error| AppError::persistence(error.to_string()))?;
            Ok(())
        }
        None => Err(AppError::not_found(format!(
            "Agy queue item {id} was not found."
        ))),
    }
}

pub fn insert_message(
    conn: &Connection,
    ecky_thread_id: &str,
    conversation_id: &str,
    role: &str,
    content: &str,
    status: &str,
    now: i64,
) -> AppResult<CodexDialogueMessage> {
    insert_message_with_id(
        conn,
        &format!("agy:{}", uuid::Uuid::new_v4()),
        ecky_thread_id,
        conversation_id,
        role,
        content,
        status,
        now,
    )
}

/// Keep any provider-produced answer even when its terminal transport status is
/// an error. The message status remains truthful so answer content cannot mask
/// quota, authorization, or transport diagnostics.
pub fn persist_terminal_answer(
    conn: &Connection,
    ecky_thread_id: &str,
    result: &AgyTurnResult,
) -> AppResult<Option<CodexDialogueMessage>> {
    if result.response.trim().is_empty()
        || matches!(result.status.as_str(), "CANCELED" | "INTERRUPTED")
    {
        return Ok(None);
    }
    let status = if result.status == "SUCCESS" {
        "success"
    } else {
        "error"
    };
    insert_message(
        conn,
        ecky_thread_id,
        &result.conversation_id,
        "assistant",
        &result.response,
        status,
        result.completed_at,
    )
    .map(Some)
}

pub fn insert_message_with_id(
    conn: &Connection,
    id: &str,
    ecky_thread_id: &str,
    conversation_id: &str,
    role: &str,
    content: &str,
    status: &str,
    now: i64,
) -> AppResult<CodexDialogueMessage> {
    insert_message_with_id_and_attachments(
        conn,
        id,
        ecky_thread_id,
        conversation_id,
        role,
        content,
        &[],
        status,
        now,
    )
}

pub fn insert_message_with_id_and_attachments(
    conn: &Connection,
    id: &str,
    ecky_thread_id: &str,
    conversation_id: &str,
    role: &str,
    content: &str,
    attachments: &[Attachment],
    status: &str,
    now: i64,
) -> AppResult<CodexDialogueMessage> {
    conn.execute(
        "INSERT OR IGNORE INTO agent_provider_messages
            (id, ecky_thread_id, provider, external_thread_id, role, content, attachments_json, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            ecky_thread_id,
            AGY_PROVIDER_ID,
            conversation_id,
            role,
            content,
            serde_json::to_string(attachments)
                .map_err(|error| AppError::persistence(error.to_string()))?,
            status,
            now
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(CodexDialogueMessage {
        id: id.to_string(),
        role: role.to_string(),
        content: content.to_string(),
        status: status.to_string(),
        timestamp: now,
        attachments: attachments.to_vec(),
        provider_event_kind: None,
    })
}

fn encode_cursor(timestamp: i64, id: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{timestamp}\n{id}"))
}

fn decode_cursor(cursor: &str) -> AppResult<(i64, String)> {
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|error| AppError::validation(format!("Invalid Agy message cursor: {error}")))?;
    let decoded = String::from_utf8(decoded)
        .map_err(|error| AppError::validation(format!("Invalid Agy message cursor: {error}")))?;
    let (timestamp, id) = decoded
        .split_once('\n')
        .ok_or_else(|| AppError::validation("Invalid Agy message cursor payload."))?;
    Ok((
        timestamp.parse().map_err(|error| {
            AppError::validation(format!("Invalid Agy message cursor timestamp: {error}"))
        })?,
        id.to_string(),
    ))
}

fn decode_message_attachments(value: String) -> rusqlite::Result<Vec<Attachment>> {
    serde_json::from_str(&value)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}

pub fn message_page(
    conn: &Connection,
    ecky_thread_id: &str,
    cursor: Option<&str>,
) -> AppResult<AgyMessagePage> {
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
    let boundary_timestamp = boundary.as_ref().map(|(timestamp, _)| *timestamp);
    let boundary_id = boundary.as_ref().map(|(_, id)| id.as_str());
    let rows = stmt
        .query_map(
            params![
                ecky_thread_id,
                AGY_PROVIDER_ID,
                boundary_timestamp,
                boundary_id
            ],
            |row| {
                Ok(CodexDialogueMessage {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    attachments: decode_message_attachments(row.get(3)?)?,
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
    Ok(AgyMessagePage {
        messages,
        next_cursor,
        backwards_cursor: None,
    })
}
