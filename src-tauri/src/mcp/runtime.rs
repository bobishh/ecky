use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::Notify;
use tokio::time::sleep;

use crate::contracts::{
    AgentSession, AgentTerminalSnapshot, AppError, AppResult, AutoAgent, Config, McpMode,
    ThreadAgentState,
};
use crate::db;
use crate::models::{AppState, McpSessionState, McpTargetRef};

const MCP_READY_ATTEMPTS: usize = 75;
const MCP_READY_SLEEP_MS: u64 = 200;
const THREAD_STATE_STALE_THRESHOLD_SECS: u64 = 30;
const INITIAL_PROMPT_DELAY_MS: u64 = 700;
const INITIAL_PROMPT_RETRY_MS: u64 = 250;
const CHILD_OUTPUT_CHUNK_BYTES: usize = 1024;
const POST_AUTO_TRUST_PROMPT_DELAY_MS: u64 = 300;
const TERMINAL_DETECTOR_TAIL_LIMIT: usize = 4096;
const TERMINAL_SCREEN_TEXT_LIMIT: usize = 32_768;
const TERMINAL_VT_STREAM_LIMIT: usize = 65_536;
const TERMINAL_EMIT_INTERVAL_MS: u64 = 50;
const TERMINAL_ROWS: u16 = 32;
const TERMINAL_COLS: u16 = 120;
const AGENT_TEMP_WORK_DIR_PREFIX: &str = "ecky-agent-";
const COMMON_USER_BIN_PATH_SUFFIXES: &[&str] = &[
    ".asdf/shims",
    ".local/bin",
    "bin",
    ".volta/bin",
    ".npm/bin",
    ".bun/bin",
];
const COMMON_MACOS_BIN_DIRS: &[&str] = &["/opt/homebrew/bin", "/opt/homebrew/sbin"];
static TERMINAL_SESSION_NONCE: AtomicU64 = AtomicU64::new(1);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn next_terminal_session_nonce() -> u64 {
    TERMINAL_SESSION_NONCE.fetch_add(1, Ordering::Relaxed)
}

fn push_unique_path_entry(entries: &mut Vec<PathBuf>, candidate: PathBuf) {
    if candidate.as_os_str().is_empty() {
        return;
    }
    if entries.iter().any(|existing| existing == &candidate) {
        return;
    }
    entries.push(candidate);
}

fn join_path_entries(entries: &[PathBuf]) -> Option<OsString> {
    if entries.is_empty() {
        return None;
    }

    std::env::join_paths(entries.iter()).ok().or_else(|| {
        let separator = if cfg!(windows) { ";" } else { ":" };
        let joined = entries
            .iter()
            .map(|entry| entry.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(separator);
        Some(OsString::from(joined))
    })
}

fn build_spawn_path_from_sources(
    process_path: Option<&str>,
    login_shell_path: Option<&str>,
    home_dir: Option<&Path>,
) -> Option<OsString> {
    let mut merged = Vec::new();

    for source in [process_path, login_shell_path].into_iter().flatten() {
        for entry in std::env::split_paths(source) {
            push_unique_path_entry(&mut merged, entry);
        }
    }

    if let Some(home_dir) = home_dir {
        for suffix in COMMON_USER_BIN_PATH_SUFFIXES {
            push_unique_path_entry(&mut merged, home_dir.join(suffix));
        }
    }

    for directory in COMMON_MACOS_BIN_DIRS {
        push_unique_path_entry(&mut merged, PathBuf::from(directory));
    }

    join_path_entries(&merged)
}

#[cfg(target_os = "macos")]
fn read_login_shell_path(shell: &OsStr) -> Option<String> {
    let output = ProcessCommand::new(shell)
        .arg("-lc")
        .arg(r#"printf %s "$PATH""#)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

#[cfg(target_os = "macos")]
fn current_login_shell_path() -> Option<String> {
    let configured_shell = std::env::var_os("SHELL")
        .filter(|shell| !shell.is_empty())
        .and_then(|shell| read_login_shell_path(shell.as_os_str()));
    configured_shell.or_else(|| read_login_shell_path(OsStr::new("/bin/zsh")))
}

#[cfg(not(target_os = "macos"))]
fn current_login_shell_path() -> Option<String> {
    None
}

fn build_spawn_path() -> Option<OsString> {
    let process_path = std::env::var_os("PATH").map(|value| value.to_string_lossy().into_owned());
    let login_shell_path = current_login_shell_path();
    let home_dir = std::env::var_os("HOME").map(PathBuf::from);

    build_spawn_path_from_sources(
        process_path.as_deref(),
        login_shell_path.as_deref(),
        home_dir.as_deref(),
    )
}

fn is_bare_command_name(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    !trimmed.is_empty() && Path::new(trimmed).components().count() == 1
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(windows)]
fn windows_path_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.BAT;.CMD;.COM".to_string())
        .split(';')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.trim_start_matches('.').to_string())
        .collect()
}

fn resolve_bare_command_on_path(cmd: &str, path_value: &OsStr) -> Option<PathBuf> {
    for directory in std::env::split_paths(path_value) {
        let candidate = directory.join(cmd);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }

        #[cfg(windows)]
        if Path::new(cmd).extension().is_none() {
            for extension in windows_path_extensions() {
                let candidate = directory.join(cmd).with_extension(extension);
                if is_executable_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn format_agent_command_not_found_error(agent: &AutoAgent) -> String {
    let command = agent.cmd.trim();
    format!(
        "Auto-agent `{}` could not start because command `{}` was not found in the spawn PATH. \
         Release builds may start with a reduced environment. Configure an absolute path in the \
         agent command field to override PATH lookup.",
        agent.label, command
    )
}

fn ensure_agent_command_resolvable(agent: &AutoAgent, spawn_path: &OsStr) -> AppResult<()> {
    let command = agent.cmd.trim();
    if !is_bare_command_name(command) {
        return Ok(());
    }

    if resolve_bare_command_on_path(command, spawn_path).is_some() {
        return Ok(());
    }

    Err(AppError::internal(format_agent_command_not_found_error(
        agent,
    )))
}

fn format_spawn_failure(agent: &AutoAgent, err: &str) -> String {
    if is_bare_command_name(agent.cmd.trim())
        && (err.contains("No viable candidates found in PATH")
            || err.contains("Unable to resolve the PATH"))
    {
        return format!(
            "{}\nUnderlying spawn error:\n{}",
            format_agent_command_not_found_error(agent),
            err
        );
    }

    format!("spawn failed: {}", err)
}

fn format_agent_exit_error(
    agent: &AutoAgent,
    exit: &portable_pty::ExitStatus,
    elapsed_secs: u64,
    output_tail: &str,
) -> String {
    let trimmed_tail = output_tail.trim();
    if exit.exit_code() == 127 && trimmed_tail.contains("env: node: No such file or directory") {
        let mut err = format!(
            "{} could not start because `node` was not found in the auto-agent PATH. \
             Release builds may start with a reduced environment.\n\
             Process exited with status {} after {}s.",
            agent.label, exit, elapsed_secs
        );
        if !trimmed_tail.is_empty() {
            err.push_str(&format!("\nLast agent output:\n{}", trimmed_tail));
        }
        return err;
    }

    let mut err = format!(
        "{} exited with status {} after {}s.",
        agent.label, exit, elapsed_secs
    );
    if !trimmed_tail.is_empty() {
        err.push_str(&format!("\nLast agent output:\n{}", trimmed_tail));
    }
    err
}

#[cfg(test)]
fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupPromptMode {
    DeferredPty,
    Positional,
    Flag(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentTerminalProviderKind {
    Claude,
    Gemini,
    Codex,
    Amp,
    Opencode,
    Generic,
}

impl AgentTerminalProviderKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::Amp => "amp",
            Self::Opencode => "opencode",
            Self::Generic => "generic",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Codex => "Codex",
            Self::Amp => "Amp",
            Self::Opencode => "OpenCode",
            Self::Generic => "Agent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalAttentionObservation {
    kind: String,
    summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalActivityObservation {
    label: String,
    elapsed_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct TerminalBehaviorObservation {
    attention: Option<TerminalAttentionObservation>,
    activity: Option<TerminalActivityObservation>,
    sanitized_fallback: Option<String>,
}

impl TerminalBehaviorObservation {
    fn summary(&self) -> Option<String> {
        self.attention
            .as_ref()
            .map(|entry| entry.summary.clone())
            .or_else(|| self.activity.as_ref().map(|entry| entry.label.clone()))
            .or_else(|| self.sanitized_fallback.clone())
    }
}

trait AgentTerminalBehavior: Send + Sync {
    fn kind(&self) -> AgentTerminalProviderKind;

    fn detect_attention(&self, output: &str) -> Option<TerminalAttentionObservation> {
        detect_terminal_attention(self.kind(), output)
    }

    fn extract_activity(&self, output: &str) -> Option<TerminalActivityObservation> {
        output
            .lines()
            .rev()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .find_map(|line| parse_cancelable_activity_line(self.kind(), line))
    }

    fn extract_fallback(&self, output: &str) -> Option<String> {
        extract_sanitized_terminal_fallback(self.kind(), output)
    }

    fn observe(&self, output: &str) -> TerminalBehaviorObservation {
        TerminalBehaviorObservation {
            attention: self.detect_attention(output),
            activity: self.extract_activity(output),
            sanitized_fallback: self.extract_fallback(output),
        }
    }
}

struct GenericTerminalBehavior {
    kind: AgentTerminalProviderKind,
}

impl AgentTerminalBehavior for GenericTerminalBehavior {
    fn kind(&self) -> AgentTerminalProviderKind {
        self.kind
    }
}

struct GeminiTerminalBehavior;

impl AgentTerminalBehavior for GeminiTerminalBehavior {
    fn kind(&self) -> AgentTerminalProviderKind {
        AgentTerminalProviderKind::Gemini
    }

    fn extract_activity(&self, output: &str) -> Option<TerminalActivityObservation> {
        output
            .lines()
            .rev()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .find_map(|line| parse_cancelable_activity_line(self.kind(), line))
    }
}

struct ClaudeTerminalBehavior;

impl AgentTerminalBehavior for ClaudeTerminalBehavior {
    fn kind(&self) -> AgentTerminalProviderKind {
        AgentTerminalProviderKind::Claude
    }
}

static CLAUDE_TERMINAL_BEHAVIOR: ClaudeTerminalBehavior = ClaudeTerminalBehavior;
static GEMINI_TERMINAL_BEHAVIOR: GeminiTerminalBehavior = GeminiTerminalBehavior;
static CODEX_TERMINAL_BEHAVIOR: GenericTerminalBehavior = GenericTerminalBehavior {
    kind: AgentTerminalProviderKind::Codex,
};
static AMP_TERMINAL_BEHAVIOR: GenericTerminalBehavior = GenericTerminalBehavior {
    kind: AgentTerminalProviderKind::Amp,
};
static OPENCODE_TERMINAL_BEHAVIOR: GenericTerminalBehavior = GenericTerminalBehavior {
    kind: AgentTerminalProviderKind::Opencode,
};
static GENERIC_TERMINAL_BEHAVIOR: GenericTerminalBehavior = GenericTerminalBehavior {
    kind: AgentTerminalProviderKind::Generic,
};

fn normalized_agent_command_basename(cmd: &str) -> String {
    Path::new(cmd)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(cmd)
        .trim()
        .to_ascii_lowercase()
}

fn provider_kind_from_cmd(cmd: &str) -> AgentTerminalProviderKind {
    let normalized = normalized_agent_command_basename(cmd);
    if normalized.contains("claude") {
        AgentTerminalProviderKind::Claude
    } else if normalized.contains("gemini") {
        AgentTerminalProviderKind::Gemini
    } else if normalized.contains("codex") {
        AgentTerminalProviderKind::Codex
    } else if normalized.contains("opencode") {
        AgentTerminalProviderKind::Opencode
    } else if normalized.contains("amp") {
        AgentTerminalProviderKind::Amp
    } else {
        AgentTerminalProviderKind::Generic
    }
}

fn provider_kind_for_agent(agent: &AutoAgent) -> AgentTerminalProviderKind {
    provider_kind_from_cmd(&agent.cmd)
}

fn behavior_for_provider(
    provider: AgentTerminalProviderKind,
) -> &'static dyn AgentTerminalBehavior {
    match provider {
        AgentTerminalProviderKind::Claude => &CLAUDE_TERMINAL_BEHAVIOR,
        AgentTerminalProviderKind::Gemini => &GEMINI_TERMINAL_BEHAVIOR,
        AgentTerminalProviderKind::Codex => &CODEX_TERMINAL_BEHAVIOR,
        AgentTerminalProviderKind::Amp => &AMP_TERMINAL_BEHAVIOR,
        AgentTerminalProviderKind::Opencode => &OPENCODE_TERMINAL_BEHAVIOR,
        AgentTerminalProviderKind::Generic => &GENERIC_TERMINAL_BEHAVIOR,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoAgentRuntimePhase {
    Sleeping,
    Waking,
    Waiting,
    Active,
    Disconnected,
    Error,
}

impl AutoAgentRuntimePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sleeping => "sleeping",
            Self::Waking => "waking",
            Self::Waiting => "waiting",
            Self::Active => "active",
            Self::Disconnected => "disconnected",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AutoAgentRuntimeSnapshot {
    pub agent_id: String,
    pub agent_label: String,
    pub provider_kind: String,
    pub phase: AutoAgentRuntimePhase,
    pub has_spawned: bool,
    pub pid: Option<i32>,
    pub pending_thread_id: Option<String>,
    pub pending_message_id: Option<String>,
    pub pending_model_id: Option<String>,
    pub session_id: Option<String>,
    pub llm_model_label: Option<String>,
    pub busy: bool,
    pub activity_label: Option<String>,
    pub activity_started_at: Option<u64>,
    pub attention_kind: Option<String>,
    pub waiting_on_prompt: bool,
    pub status_text: Option<String>,
    pub last_error: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone)]
struct AutoAgentRuntimeEntry {
    agent: AutoAgent,
    notify: Arc<Notify>,
    provider_kind: String,
    phase: AutoAgentRuntimePhase,
    has_spawned: bool,
    pid: Option<i32>,
    pending_thread_id: Option<String>,
    pending_message_id: Option<String>,
    pending_model_id: Option<String>,
    session_id: Option<String>,
    llm_model_label: Option<String>,
    busy: bool,
    activity_label: Option<String>,
    activity_started_at: Option<u64>,
    attention_kind: Option<String>,
    waiting_on_prompt: bool,
    status_text: Option<String>,
    last_error: Option<String>,
    updated_at: u64,
}

impl AutoAgentRuntimeEntry {
    fn new(agent: AutoAgent) -> Self {
        Self {
            provider_kind: provider_kind_for_agent(&agent).as_str().to_string(),
            agent,
            notify: Arc::new(Notify::new()),
            phase: AutoAgentRuntimePhase::Sleeping,
            has_spawned: false,
            pid: None,
            pending_thread_id: None,
            pending_message_id: None,
            pending_model_id: None,
            session_id: None,
            llm_model_label: None,
            busy: false,
            activity_label: None,
            activity_started_at: None,
            attention_kind: None,
            waiting_on_prompt: false,
            status_text: None,
            last_error: None,
            updated_at: now_secs(),
        }
    }

    fn snapshot(&self) -> AutoAgentRuntimeSnapshot {
        AutoAgentRuntimeSnapshot {
            agent_id: self.agent.id.clone(),
            agent_label: self.agent.label.clone(),
            provider_kind: self.provider_kind.clone(),
            phase: self.phase.clone(),
            has_spawned: self.has_spawned,
            pid: self.pid,
            pending_thread_id: self.pending_thread_id.clone(),
            pending_message_id: self.pending_message_id.clone(),
            pending_model_id: self.pending_model_id.clone(),
            session_id: self.session_id.clone(),
            llm_model_label: self.llm_model_label.clone(),
            busy: self.busy,
            activity_label: self.activity_label.clone(),
            activity_started_at: self.activity_started_at,
            attention_kind: self.attention_kind.clone(),
            waiting_on_prompt: self.waiting_on_prompt,
            status_text: self.status_text.clone(),
            last_error: self.last_error.clone(),
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Default)]
pub struct AutoAgentRuntimeRegistry {
    by_id: HashMap<String, AutoAgentRuntimeEntry>,
}

impl AutoAgentRuntimeRegistry {
    pub fn register(&mut self, agent: AutoAgent) -> Arc<Notify> {
        if let Some(existing) = self.by_id.get(&agent.id) {
            return existing.notify.clone();
        }
        let entry = AutoAgentRuntimeEntry::new(agent);
        let notify = entry.notify.clone();
        self.by_id.insert(entry.agent.id.clone(), entry);
        notify
    }

    pub fn snapshot_by_id(&self, agent_id: &str) -> Option<AutoAgentRuntimeSnapshot> {
        self.by_id
            .get(agent_id)
            .map(AutoAgentRuntimeEntry::snapshot)
    }

    pub fn snapshot_by_label(&self, agent_label: &str) -> Option<AutoAgentRuntimeSnapshot> {
        self.by_id
            .values()
            .find(|entry| entry.agent.label == agent_label)
            .map(AutoAgentRuntimeEntry::snapshot)
    }

    pub fn find_by_session_id(&self, session_id: &str) -> Option<AutoAgentRuntimeSnapshot> {
        self.by_id
            .values()
            .find(|entry| entry.session_id.as_deref() == Some(session_id))
            .map(AutoAgentRuntimeEntry::snapshot)
    }

    fn update_by_id<F>(&mut self, agent_id: &str, f: F) -> bool
    where
        F: FnOnce(&mut AutoAgentRuntimeEntry),
    {
        let Some(entry) = self.by_id.get_mut(agent_id) else {
            return false;
        };
        f(entry);
        entry.updated_at = now_secs();
        true
    }

    fn update_by_label<F>(&mut self, agent_label: &str, f: F) -> bool
    where
        F: FnOnce(&mut AutoAgentRuntimeEntry),
    {
        let Some(entry) = self
            .by_id
            .values_mut()
            .find(|candidate| candidate.agent.label == agent_label)
        else {
            return false;
        };
        f(entry);
        entry.updated_at = now_secs();
        true
    }

    fn update_by_session_id<F>(&mut self, session_id: &str, f: F) -> bool
    where
        F: FnOnce(&mut AutoAgentRuntimeEntry),
    {
        let Some(entry) = self
            .by_id
            .values_mut()
            .find(|candidate| candidate.session_id.as_deref() == Some(session_id))
        else {
            return false;
        };
        f(entry);
        entry.updated_at = now_secs();
        true
    }

    fn agent_by_id(&self, agent_id: &str) -> Option<AutoAgent> {
        self.by_id.get(agent_id).map(|entry| entry.agent.clone())
    }
}

fn enabled_auto_agents(config: &Config) -> Vec<AutoAgent> {
    config
        .mcp
        .auto_agents
        .iter()
        .filter(|agent| agent.enabled)
        .cloned()
        .collect()
}

pub fn default_mcp_mode(config: &Config) -> McpMode {
    if config.mcp.auto_agents.is_empty() {
        McpMode::Passive
    } else {
        McpMode::Active
    }
}

pub fn ensure_primary_agent_id(config: &mut Config) -> bool {
    let enabled_agents = enabled_auto_agents(config);
    let next_primary = config
        .mcp
        .primary_agent_id
        .as_deref()
        .and_then(|current| {
            enabled_agents
                .iter()
                .find(|agent| agent.id == current)
                .map(|agent| agent.id.clone())
        })
        .or_else(|| enabled_agents.first().map(|agent| agent.id.clone()));

    if config.mcp.primary_agent_id != next_primary {
        config.mcp.primary_agent_id = next_primary;
        return true;
    }
    false
}

pub fn active_mode_enabled(config: &Config) -> bool {
    config.connection_type.as_deref() == Some("mcp") && config.mcp.mode == McpMode::Active
}

pub fn primary_auto_agent(config: &Config) -> Option<AutoAgent> {
    if !active_mode_enabled(config) {
        return None;
    }

    let enabled = enabled_auto_agents(config);
    if enabled.is_empty() {
        return None;
    }

    config
        .mcp
        .primary_agent_id
        .as_deref()
        .and_then(|primary| enabled.iter().find(|agent| agent.id == primary).cloned())
        .or_else(|| enabled.first().cloned())
}

fn runtime_registry(state: &AppState) -> std::sync::MutexGuard<'_, AutoAgentRuntimeRegistry> {
    state.auto_agent_runtime.lock().unwrap()
}

fn set_runtime_pending_target(
    entry: &mut AutoAgentRuntimeEntry,
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
) {
    entry.pending_thread_id = thread_id;
    if entry.pending_thread_id.is_some() {
        entry.pending_message_id = message_id;
        entry.pending_model_id = model_id;
    } else {
        entry.pending_message_id = None;
        entry.pending_model_id = None;
    }
}

fn update_runtime_pending_thread(entry: &mut AutoAgentRuntimeEntry, thread_id: Option<String>) {
    let Some(thread_id) = thread_id else {
        return;
    };
    if entry.pending_thread_id.as_deref() == Some(thread_id.as_str()) {
        return;
    }
    entry.pending_thread_id = Some(thread_id);
    entry.pending_message_id = None;
    entry.pending_model_id = None;
}

pub fn primary_runtime_snapshot(state: &AppState) -> Option<AutoAgentRuntimeSnapshot> {
    let config = state.config.lock().unwrap().clone();
    let primary = primary_auto_agent(&config)?;
    runtime_registry(state).snapshot_by_id(&primary.id)
}

pub fn runtime_snapshot_by_id(
    state: &AppState,
    agent_id: &str,
) -> Option<AutoAgentRuntimeSnapshot> {
    runtime_registry(state).snapshot_by_id(agent_id)
}

pub fn runtime_snapshot_by_label(
    state: &AppState,
    agent_label: &str,
) -> Option<AutoAgentRuntimeSnapshot> {
    runtime_registry(state).snapshot_by_label(agent_label)
}

pub fn runtime_snapshot_by_session_id(
    state: &AppState,
    session_id: &str,
) -> Option<AutoAgentRuntimeSnapshot> {
    runtime_registry(state).find_by_session_id(session_id)
}

pub fn runtime_snapshot_for_thread(
    state: &AppState,
    thread_id: &str,
    session_id: Option<&str>,
) -> Option<AutoAgentRuntimeSnapshot> {
    let runtime = runtime_registry(state);
    if let Some(session_id) = session_id {
        return runtime.find_by_session_id(session_id).filter(|snapshot| {
            snapshot.pending_thread_id.as_deref() == Some(thread_id)
                || snapshot.session_id.as_deref() == Some(session_id)
        });
    }

    runtime
        .by_id
        .values()
        .find(|entry| {
            entry.pending_thread_id.as_deref() == Some(thread_id) && entry.session_id.is_none()
        })
        .map(AutoAgentRuntimeEntry::snapshot)
}

fn ensure_supervisor_for_agent(state: &AppState, agent: AutoAgent) {
    let (notify, should_spawn) = {
        let mut runtime = runtime_registry(state);
        let already_registered = runtime.snapshot_by_id(&agent.id).is_some();
        let notify = runtime.register(agent.clone());
        (notify, !already_registered)
    };

    if !should_spawn {
        return;
    }

    let loop_state = state.clone();
    let agent_id = agent.id.clone();
    tauri::async_runtime::spawn(async move {
        supervisor_loop(loop_state, agent_id, notify).await;
    });
}

pub fn sync_auto_agent_supervisors(state: AppState) {
    let config = state.config.lock().unwrap().clone();
    if !active_mode_enabled(&config) {
        return;
    }

    for agent in enabled_auto_agents(&config) {
        ensure_supervisor_for_agent(&state, agent);
    }

    cleanup_stale_managed_sessions(&state);
    cleanup_orphaned_managed_processes(&state);
}

pub fn initialize_auto_agent_supervisors(state: AppState) {
    sync_auto_agent_supervisors(state);
}

async fn supervisor_loop(state: AppState, agent_id: String, notify: Arc<Notify>) {
    loop {
        notify.notified().await;
        let Some(agent) = runtime_registry(&state).agent_by_id(&agent_id) else {
            continue;
        };

        if let Err(err) = spawn_agent_once(&state, &agent).await {
            let msg = format!("[SUPERVISOR] Failed to spawn {}: {}", agent.label, err);
            state.push_log(msg.clone());
            let mut runtime = runtime_registry(&state);
            runtime.update_by_id(&agent.id, |entry| {
                entry.phase = AutoAgentRuntimePhase::Error;
                entry.pid = None;
                entry.busy = false;
                entry.activity_label = None;
                entry.activity_started_at = None;
                entry.attention_kind = None;
                entry.last_error = Some(err.to_string());
                entry.status_text = Some(msg.clone());
            });
        }
    }
}

async fn wait_for_mcp_endpoint(state: &AppState) -> AppResult<String> {
    for _ in 0..MCP_READY_ATTEMPTS {
        let status = state.mcp_status();
        if status.running {
            return Ok(status.endpoint_url);
        }
        sleep(Duration::from_millis(MCP_READY_SLEEP_MS)).await;
    }

    Err(AppError::internal(
        "MCP server not ready after 15 seconds; auto-agent wake aborted.",
    ))
}

fn managed_endpoint_url(endpoint_url: &str, agent: &AutoAgent) -> String {
    format!(
        "{endpoint_url}?managedAgentId={agent_id}",
        endpoint_url = endpoint_url,
        agent_id = agent.id
    )
}

fn write_agent_instructions(
    agent: &AutoAgent,
    endpoint_url: &str,
) -> AppResult<std::path::PathBuf> {
    let work_dir =
        std::env::temp_dir().join(format!("{}{}", AGENT_TEMP_WORK_DIR_PREFIX, agent.label));
    fs::create_dir_all(&work_dir).map_err(|err| AppError::internal(err.to_string()))?;

    let agents_md = format!(
        "# Ecky CAD Agent\n\n\
        ## Identity\n\
        - Your stable Ecky agent label is `{agent_label}`.\n\
        - On startup, call `agent_identity_set` with `agentLabel: \"{agent_label}\"`.\n\
        - Before `session_log_in`, choose the thread you will work on. Use `thread_list` / `thread_get` first unless Ecky already woke you from a selected thread.\n\
        - Then call `session_log_in` with the same `agentLabel` and an explicit `threadId` whenever you are choosing the thread yourself.\n\n\
        ## MCP Server\n\
        Connect to: `{endpoint_url}`\n\n\
        ## Startup sequence (token-efficient — follow exactly)\n\
        1. Call `agent_identity_set` with `agentLabel: \"{agent_label}\"`.\n\
        2. If Ecky already woke you from a selected thread, call `session_log_in` with `agentLabel: \"{agent_label}\"` to inherit that bound target. \
           Otherwise call `thread_list` / `thread_get`, choose a thread, then call `session_log_in` with `agentLabel: \"{agent_label}\"` and that `threadId`.\n\
        3. Call `request_user_prompt` with a SHORT friendly greeting only \
           (for example \"Hello! What would you like to design?\"). \
           The response may include local image/CAD attachments from the Ecky prompt panel, \
           plus `threadId` / `threadTitle` for the exact bound thread. Ecky will NOT infer a thread from whatever happens to be visible in the workspace. \
           Attachment paths are already absolute local files staged by Ecky, so open them \
           directly with your normal file/image tools instead of guessing or rewriting paths. \
           Do NOT call `bootstrap_ecky` or `workspace_overview` yet. \
           If `request_user_prompt` times out, treat that as NORMAL idle behavior, \
           not as a fatal error. Ecky uses the configured MCP prompt timeout when you omit `timeoutSecs`: \
           poll again later, or call `session_log_out` before exiting.\n\
        4. When the user sends the first queued message:\n\
           a. Call `bootstrap_ecky` to load system guidance.\n\
           b. Call `workspace_overview` to inspect the current thread state.\n\
           c. Use `workspace_overview.agentBrief.sourceLanguage` and `workspace_overview.agentBrief.geometryBackend` to choose guides before writing macro code: if sourceLanguage is `ecky`, read `ecky://guides/ecky-source` first, then read `ecky://guides/build123d` or `ecky://guides/freecad` to match the backend.\n\
           d. If `workspace_overview.defaultTarget.hasVersion` is true, call `target_meta_get`.\n\
           e. Use `target_macro_get` for macro reasoning and `target_detail_get(section=...)` \
              for exact chunks.\n\
           f. Use `semantic_manifest_get` only when semantic bindings matter.\n\
           g. Use `target_get` only as a last-resort full payload.\n\
           h. Use `measurement_annotation_save/delete` when you need to encode what a dimension \
              means in the manifest.\n\
           i. If a step will take more than a few seconds, call `session_activity_set`, and call \
              `session_activity_clear` when that step finishes.\n\
           j. Act on the request using `macro_replace_and_render` or `params_patch_and_render`.\n\
        5. When you finish a user-facing turn, call `session_reply_save` for the final reply \
           (or fatal error) if the user should see text in the thread history.\n\
        6. Immediately after the turn completes, call `request_user_prompt` again so Ecky can \
           hibernate you between user messages. Do NOT wait for terminal input and do NOT \
           restart or re-bootstrap between turns.\n\n\
        7. If you need to move to a different thread, call `session_log_out`, then `session_log_in` again for that thread. Do not silently hop threads inside `request_user_prompt`.\n\n\
        8. If you read queued thread messages via `thread_get` instead of receiving a live \
           `request_user_prompt`, call `mark_as_read` on any one pending user message from that \
           thread before you start. Ecky will drain the whole pending batch for that thread into \
           the current turn.\n\n\
        ## Scope\n\
        - This file is MCP runtime/process guidance only.\n\
        - Modeling policy lives in `bootstrap_ecky`, `workspace_overview`, and the `ecky://guides/*` resources.\n\
        - Do not treat this file as the CAD design policy source of truth.\n",
        agent_label = agent.label,
        endpoint_url = endpoint_url,
    );

    fs::write(work_dir.join("AGENTS.md"), agents_md)
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok(work_dir)
}

fn startup_prompt_mode(agent: &AutoAgent) -> StartupPromptMode {
    match provider_kind_for_agent(agent) {
        AgentTerminalProviderKind::Claude | AgentTerminalProviderKind::Codex => {
            StartupPromptMode::Positional
        }
        AgentTerminalProviderKind::Gemini => StartupPromptMode::Flag("--prompt-interactive"),
        AgentTerminalProviderKind::Opencode => StartupPromptMode::Flag("--prompt"),
        AgentTerminalProviderKind::Amp | AgentTerminalProviderKind::Generic => {
            StartupPromptMode::DeferredPty
        }
    }
}

#[cfg(test)]
fn build_command(agent: &AutoAgent, initial_prompt: Option<&str>) -> String {
    let mut parts = vec![shell_escape(&agent.cmd)];
    for arg in build_command_args(agent, initial_prompt) {
        parts.push(shell_escape(&arg));
    }
    parts.join(" ")
}

fn build_command_args(agent: &AutoAgent, initial_prompt: Option<&str>) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(model) = &agent.model {
        let trimmed = model.trim();
        if !trimmed.is_empty() && trimmed != "auto" {
            parts.push("--model".to_string());
            parts.push(trimmed.to_string());
        }
    }
    for arg in &agent.args {
        parts.push(arg.clone());
    }
    if let Some(prompt) = initial_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        match startup_prompt_mode(agent) {
            StartupPromptMode::Positional => {
                parts.push("--".to_string());
                parts.push(prompt.to_string());
            }
            StartupPromptMode::Flag(flag) => {
                parts.push(flag.to_string());
                parts.push(prompt.to_string());
            }
            StartupPromptMode::DeferredPty => {}
        }
    }
    parts
}

fn build_initial_prompt(agent: &AutoAgent, endpoint_url: &str) -> String {
    format!(
        "You are an Ecky CAD design assistant. \
        Read AGENTS.md in your current directory for MCP runtime instructions only. \
        The Ecky MCP server is at {endpoint_url}. \
        Start now: call `agent_identity_set` with `agentLabel: \"{agent_label}\"`, then \
        choose the thread you will work on via `thread_list` / `thread_get` unless Ecky already woke you from a selected thread, \
        then call `session_log_in` with the same `agentLabel` and that thread target, then call `request_user_prompt` \
        with a short friendly greeting. The response may include local image/CAD attachments \
        plus `threadId` / `threadTitle` for the exact bound thread. Ecky will not infer a thread from the current workspace view. Attachment paths are already \
        absolute local files staged by Ecky; open them directly with your file/image tools \
        instead of rewriting or guessing new paths. \
        Do NOT call `bootstrap_ecky` or `workspace_overview` until the user sends the first queued message. \
        After that, treat `bootstrap_ecky`, `workspace_overview`, and the `ecky://guides/*` resources as the modeling policy source of truth. Use `workspace_overview.agentBrief.sourceLanguage` and `workspace_overview.agentBrief.geometryBackend` to choose the matching guide. If the source language is `ecky`, read `ecky://guides/ecky-source` first, then the backend guide for `build123d` or `freecad`. If `workspace_overview` says the thread has no saved versions yet, \
        use the guides plus the queued thread context to create the first version instead of assuming `target_meta_get` exists. Otherwise prefer `target_meta_get`, `target_macro_get`, and `target_detail_get(section=...)` \
        before falling back to `target_get`. Use `session_activity_set` / `session_activity_clear` for \
        long steps instead of relying on terminal text. At the end of each turn, save any final user-facing \
        reply with `session_reply_save` and then immediately call `request_user_prompt` again.",
        endpoint_url = endpoint_url,
        agent_label = agent.label,
    )
}

#[derive(Debug, Default)]
struct AgentBootState {
    trust_prompt_auto_approved: bool,
    trust_prompt_visible: bool,
    attention_required: bool,
    initial_prompt_sent: bool,
    auto_approved_mcp_tool_prompts: Vec<String>,
}

fn is_managed_agent_workspace(work_dir: &Path) -> bool {
    let file_name = work_dir.file_name().and_then(|name| name.to_str());
    work_dir.starts_with(std::env::temp_dir())
        && file_name
            .map(|name| name.starts_with(AGENT_TEMP_WORK_DIR_PREFIX))
            .unwrap_or(false)
}

fn looks_like_workspace_trust_prompt(output: &str) -> bool {
    let normalized = normalize_terminal_output_for_detection(output);
    let has_trust = normalized.contains("trust this folder")
        || normalized.contains("project you created or one you trust")
        || (normalized.contains("quick safety check") && normalized.contains("trust"));
    let has_choice = normalized.contains("enter to confirm")
        || normalized.contains("[y/n]")
        || (normalized.contains("1. yes") && normalized.contains("2. no"));
    has_trust && has_choice
}

fn looks_like_terminal_confirmation_prompt(output: &str) -> bool {
    let normalized = normalize_terminal_output_for_detection(output);
    if normalized.is_empty() {
        return false;
    }

    normalized.contains("enter to confirm")
        || normalized.contains("[y/n]")
        || normalized.contains("(y/n)")
        || normalized.contains("press enter to continue")
        || normalized.contains("press any key to continue")
}

fn extract_safe_managed_mcp_tool_prompt(output: &str) -> Option<&'static str> {
    let normalized = normalize_terminal_output_for_detection(output);
    if !normalized.contains("do you want to proceed")
        || !normalized.contains("don't ask again")
        || !normalized.contains("ecky_mcp - ")
    {
        return None;
    }

    for tool_name in [
        "agent_identity_set",
        "session_log_in",
        "request_user_prompt",
    ] {
        let marker = format!("ecky_mcp - {}", tool_name);
        if normalized.contains(&marker) {
            return Some(tool_name);
        }
    }

    None
}

fn normalize_terminal_output_for_detection(output: &str) -> String {
    output
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn detect_terminal_attention(
    provider: AgentTerminalProviderKind,
    output: &str,
) -> Option<TerminalAttentionObservation> {
    if looks_like_workspace_trust_prompt(output) {
        return Some(TerminalAttentionObservation {
            kind: "workspace_trust".to_string(),
            summary: format!(
                "{} needs workspace trust confirmation.",
                provider.display_name()
            ),
        });
    }
    if let Some(tool_name) = extract_safe_managed_mcp_tool_prompt(output) {
        return Some(TerminalAttentionObservation {
            kind: "startup_tool_approval".to_string(),
            summary: format!(
                "{} is waiting for approval to call Ecky MCP startup tool `{}`.",
                provider.display_name(),
                tool_name
            ),
        });
    }
    if looks_like_terminal_confirmation_prompt(output) {
        return Some(TerminalAttentionObservation {
            kind: "terminal_confirmation".to_string(),
            summary: format!(
                "{} is waiting for terminal confirmation.",
                provider.display_name()
            ),
        });
    }
    None
}

fn normalize_terminal_summary_line(line: &str) -> String {
    line.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_start_matches([
            ':', '⋮', '•', '·', '…', '>', '|', '⠋', '⠙', '⠹', '⠸', '💬', '🟠',
        ])
        .trim()
        .to_string()
}

fn parse_terminal_elapsed_hint(fragment: &str) -> Option<u64> {
    let mut total = 0_u64;
    let mut matched = false;
    for token in fragment
        .replace(',', " ")
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| "()[]{}".contains(ch)))
    {
        if let Some(value) = token
            .strip_suffix('h')
            .and_then(|value| value.parse::<u64>().ok())
        {
            total = total.saturating_add(value.saturating_mul(3600));
            matched = true;
            continue;
        }
        if let Some(value) = token
            .strip_suffix('m')
            .and_then(|value| value.parse::<u64>().ok())
        {
            total = total.saturating_add(value.saturating_mul(60));
            matched = true;
            continue;
        }
        if let Some(value) = token
            .strip_suffix('s')
            .and_then(|value| value.parse::<u64>().ok())
        {
            total = total.saturating_add(value);
            matched = true;
        }
    }
    matched.then_some(total)
}

fn parse_cancelable_activity_line(
    provider: AgentTerminalProviderKind,
    line: &str,
) -> Option<TerminalActivityObservation> {
    let normalized_line = normalize_terminal_summary_line(line);
    let lower = normalized_line.to_ascii_lowercase();
    let marker = "(esc to cancel,";
    let start = lower.rfind(marker)?;
    let label = normalized_line[..start]
        .trim_end_matches(['-', '—', ':', '|', '·', ' '])
        .trim()
        .to_string();
    if label.is_empty() || is_terminal_noise_line(provider, &label) {
        return None;
    }
    let elapsed_fragment = normalized_line[start + marker.len()..]
        .trim()
        .trim_end_matches(')')
        .trim();
    Some(TerminalActivityObservation {
        label,
        elapsed_secs: parse_terminal_elapsed_hint(elapsed_fragment),
    })
}

fn looks_like_terminal_chrome(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && trimmed.chars().all(|ch| {
            ch.is_ascii_punctuation() || ch.is_whitespace() || "│─┌┐└┘┬┴├┤┼╭╮╯╰═".contains(ch)
        })
}

fn is_terminal_noise_line(provider: AgentTerminalProviderKind, line: &str) -> bool {
    let normalized = normalize_terminal_summary_line(line).to_ascii_lowercase();
    if normalized.is_empty() || looks_like_terminal_chrome(&normalized) {
        return true;
    }

    let generic_noise = [
        "shift+tab to accept edits",
        "press 'esc' for normal mode",
        "click terminal to type directly",
        "click the terminal to type directly",
        "last captured terminal output",
        "live pty",
    ];
    if generic_noise
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return true;
    }

    let provider_noise: &[&str] = match provider {
        AgentTerminalProviderKind::Claude => &[
            "welcome back",
            "tips for getting started",
            "recent activity",
            "no recent activity",
            "how can i help you today",
            "run /init",
        ],
        AgentTerminalProviderKind::Gemini => &[
            "installed via homebrew",
            "gemini cli update available",
            "/model ",
            "no sandbox (see /docs)",
        ],
        _ => &[],
    };

    provider_noise
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn extract_sanitized_terminal_fallback(
    provider: AgentTerminalProviderKind,
    output: &str,
) -> Option<String> {
    output
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(|line| {
            let candidate = normalize_terminal_summary_line(line);
            if candidate.is_empty() || is_terminal_noise_line(provider, &candidate) {
                return None;
            }
            Some(candidate)
        })
}

fn parse_csi_count(params: &str) -> usize {
    params
        .split(';')
        .next()
        .and_then(|value| {
            let trimmed = value.trim_matches('?');
            if trimmed.is_empty() {
                None
            } else {
                trimmed.parse::<usize>().ok()
            }
        })
        .unwrap_or(1)
}

fn line_start_for_cursor(output: &[char], cursor: usize) -> usize {
    output[..cursor.min(output.len())]
        .iter()
        .rposition(|ch| *ch == '\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn line_end_for_cursor(output: &[char], cursor: usize) -> usize {
    output[cursor.min(output.len())..]
        .iter()
        .position(|ch| *ch == '\n')
        .map(|offset| cursor.min(output.len()) + offset)
        .unwrap_or(output.len())
}

fn erase_to_line_end(output: &mut Vec<char>, cursor: usize) {
    let start = cursor.min(output.len());
    let end = line_end_for_cursor(output, start);
    output.drain(start..end);
}

fn erase_from_line_start(output: &mut Vec<char>, cursor: &mut usize) {
    let current = (*cursor).min(output.len());
    let start = line_start_for_cursor(output, current);
    output.drain(start..current);
    *cursor = start;
}

fn erase_entire_line(output: &mut Vec<char>, cursor: &mut usize) {
    let current = (*cursor).min(output.len());
    let start = line_start_for_cursor(output, current);
    let end = line_end_for_cursor(output, current);
    output.drain(start..end);
    *cursor = start;
}

fn write_terminal_char(output: &mut Vec<char>, cursor: &mut usize, ch: char) {
    if *cursor < output.len() {
        if output[*cursor] == '\n' {
            output.insert(*cursor, ch);
        } else {
            output[*cursor] = ch;
        }
    } else {
        while output.len() < *cursor {
            output.push(' ');
        }
        output.push(ch);
    }
    *cursor += 1;
}

fn apply_csi_sequence(output: &mut Vec<char>, cursor: &mut usize, params: &str, final_char: char) {
    let count = parse_csi_count(params);
    match final_char {
        'C' => {
            for _ in 0..count {
                if *cursor >= output.len() {
                    output.push(' ');
                }
                *cursor += 1;
            }
        }
        'D' => {
            *cursor = cursor.saturating_sub(count);
        }
        'G' => {
            let start = line_start_for_cursor(output, *cursor);
            *cursor = start + count.saturating_sub(1);
        }
        'K' => match params.trim() {
            "" | "0" => erase_to_line_end(output, *cursor),
            "1" => erase_from_line_start(output, cursor),
            "2" => erase_entire_line(output, cursor),
            _ => {}
        },
        'J' => match params.trim() {
            "2" => {
                output.clear();
                *cursor = 0;
            }
            "" | "0" => output.truncate((*cursor).min(output.len())),
            _ => {}
        },
        'm' | 'H' | 'f' | 'A' | 'B' | 'h' | 'l' | 'q' | 's' | 'u' => {}
        _ => {}
    }
}

fn decode_terminal_text(pending_utf8: &mut Vec<u8>, bytes: &[u8]) -> String {
    let mut raw = Vec::with_capacity(pending_utf8.len() + bytes.len());
    raw.extend_from_slice(pending_utf8);
    raw.extend_from_slice(bytes);
    pending_utf8.clear();

    match std::str::from_utf8(&raw) {
        Ok(text) => text.to_string(),
        Err(err) if err.error_len().is_none() => {
            let valid = String::from_utf8_lossy(&raw[..err.valid_up_to()]).into_owned();
            pending_utf8.extend_from_slice(&raw[err.valid_up_to()..]);
            valid
        }
        Err(_) => String::from_utf8_lossy(&raw).into_owned(),
    }
}

fn apply_terminal_text(output: &mut String, pending_escape: &mut String, raw: &str) {
    let combined = if pending_escape.is_empty() {
        raw.to_string()
    } else {
        let mut combined = std::mem::take(pending_escape);
        combined.push_str(raw);
        combined
    };
    let mut rendered: Vec<char> = output.chars().collect();
    let mut cursor = rendered.len();
    let mut chars = combined.chars().peekable();
    let mut saw_carriage_return = false;
    let mut saw_newline_after_carriage_return = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.peek().copied() {
                Some('[') => {
                    let mut sequence = String::from("\u{1b}[");
                    let _ = chars.next();
                    let mut params = String::new();
                    let mut complete = false;
                    for next in chars.by_ref() {
                        sequence.push(next);
                        if ('@'..='~').contains(&next) {
                            apply_csi_sequence(&mut rendered, &mut cursor, &params, next);
                            complete = true;
                            break;
                        }
                        params.push(next);
                    }
                    if !complete {
                        *pending_escape = sequence;
                        break;
                    }
                }
                Some(']') => {
                    let mut sequence = String::from("\u{1b}]");
                    let _ = chars.next();
                    let mut previous = '\0';
                    let mut complete = false;
                    for next in chars.by_ref() {
                        sequence.push(next);
                        if next == '\u{7}' {
                            complete = true;
                            break;
                        }
                        if previous == '\u{1b}' && next == '\\' {
                            complete = true;
                            break;
                        }
                        previous = next;
                    }
                    if !complete {
                        *pending_escape = sequence;
                        break;
                    }
                }
                Some(_) => {
                    let _ = chars.next();
                }
                None => {
                    pending_escape.push('\u{1b}');
                    break;
                }
            },
            '\r' => {
                cursor = line_start_for_cursor(&rendered, cursor);
                saw_carriage_return = true;
            }
            '\u{8}' => {
                cursor = cursor.saturating_sub(1);
                if cursor < rendered.len() && rendered[cursor] != '\n' {
                    rendered.remove(cursor);
                }
            }
            '\n' => {
                saw_newline_after_carriage_return |= saw_carriage_return;
                saw_carriage_return = false;
                if cursor < rendered.len() {
                    if rendered[cursor] != '\n' {
                        rendered.insert(cursor, '\n');
                    }
                } else {
                    rendered.push('\n');
                }
                cursor += 1;
            }
            '\t' => write_terminal_char(&mut rendered, &mut cursor, '\t'),
            _ if ch.is_control() => {}
            _ => {
                write_terminal_char(&mut rendered, &mut cursor, ch);
            }
        }
    }

    if saw_carriage_return && !saw_newline_after_carriage_return {
        erase_to_line_end(&mut rendered, cursor);
    }

    *output = rendered.into_iter().collect();
}

fn apply_terminal_bytes(output: &mut String, bytes: &[u8]) {
    let mut pending_utf8 = Vec::new();
    let mut pending_escape = String::new();
    let raw = decode_terminal_text(&mut pending_utf8, bytes);
    apply_terminal_text(output, &mut pending_escape, &raw);
}

fn replace_agent_terminal_session(
    state: &AppState,
    agent: &AutoAgent,
    pty: crate::models::AgentTerminalPty,
    writer: crate::models::AgentTerminalWriter,
) {
    let provider_kind = provider_kind_for_agent(agent);
    let snapshot = AgentTerminalSnapshot {
        agent_id: agent.id.clone(),
        agent_label: agent.label.clone(),
        session_id: None,
        provider_kind: Some(provider_kind.as_str().to_string()),
        session_nonce: next_terminal_session_nonce(),
        screen_text: String::new(),
        vt_stream: String::new(),
        vt_delta: None,
        attention_required: false,
        busy: false,
        activity_label: None,
        activity_started_at: None,
        attention_kind: None,
        summary: None,
        active: true,
        updated_at: now_secs(),
    };

    {
        let mut terminals = state.agent_terminals.lock().unwrap();
        terminals.insert(
            agent.id.clone(),
            crate::models::AgentTerminalRuntime {
                snapshot: snapshot.clone(),
                writer,
                pty,
                pending_utf8: Vec::new(),
                pending_escape: String::new(),
                last_emitted_at: None,
            },
        );
    }

    state.emit_agent_terminal_update(&snapshot);
}

#[cfg(unix)]
fn resume_prompt_wait_process(pgid: i32) {
    unsafe {
        libc::kill(-pgid, libc::SIGCONT);
    }
}

#[cfg(not(unix))]
fn resume_prompt_wait_process(_pgid: i32) {}

pub fn release_prompt_wait(
    state: &AppState,
    request_id: &str,
) -> Option<crate::models::PromptResumeState> {
    let control = state.prompt_waits.lock().unwrap().remove(request_id);
    if let Some(pgid) = control.as_ref().and_then(|control| control.pgid) {
        resume_prompt_wait_process(pgid);
    }
    control
}

fn close_prompts_for_agent_label_sync(state: &AppState, agent_label: &str, reason: &str) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let state = state.clone();
        let agent_label = agent_label.to_string();
        let reason = reason.to_string();
        handle.block_on(async move {
            state
                .close_prompts_for_agent_label(&agent_label, &reason)
                .await;
        });
    }
}

fn mark_agent_stopped(
    state: &AppState,
    agent: &AutoAgent,
    thread_id: Option<String>,
    status_text: String,
) {
    close_prompts_for_agent_label_sync(state, &agent.label, "agent_stopped");
    mark_agent_terminal_inactive(
        state,
        agent,
        Some(format!("{} terminal stopped by user.", agent.label)),
        false,
    );
    let mut runtime = runtime_registry(state);
    runtime.update_by_id(&agent.id, |entry| {
        entry.pid = None;
        entry.session_id = None;
        entry.phase = AutoAgentRuntimePhase::Sleeping;
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = false;
        update_runtime_pending_thread(entry, thread_id.clone());
        entry.status_text = Some(status_text.clone());
        entry.last_error = None;
        entry.llm_model_label = None;
    });
}

#[cfg(unix)]
fn kill_agent_process(pid: i32) -> AppResult<()> {
    unsafe {
        if libc::kill(-pid, libc::SIGKILL) == 0 {
            return Ok(());
        }
        if libc::kill(pid, libc::SIGKILL) == 0 {
            return Ok(());
        }
    }
    Err(AppError::internal(format!(
        "Failed to kill agent process {}: {}",
        pid,
        std::io::Error::last_os_error()
    )))
}

#[cfg(unix)]
fn kill_agent_process_group(pgid: i32) -> AppResult<()> {
    unsafe {
        if libc::kill(-pgid, libc::SIGKILL) == 0 {
            return Ok(());
        }
    }
    Err(AppError::internal(format!(
        "Failed to kill managed agent process group {}: {}",
        pgid,
        std::io::Error::last_os_error()
    )))
}

#[cfg(not(unix))]
fn kill_agent_process_group(pgid: i32) -> AppResult<()> {
    let _ = pgid;
    Err(AppError::internal(
        "Force-stopping auto-agent process groups is not implemented on this platform.",
    ))
}

fn cleanup_stale_managed_sessions(state: &AppState) {
    let live_session_ids = {
        let runtime = runtime_registry(state);
        runtime
            .by_id
            .values()
            .filter_map(|entry| entry.session_id.clone())
            .collect::<Vec<_>>()
    };

    let Ok(conn) = state.db.try_lock() else {
        return;
    };
    let stale_session_ids = match db::get_managed_agent_session_ids_not_in(&conn, &live_session_ids)
    {
        Ok(session_ids) => session_ids,
        Err(err) => {
            state.push_log(format!(
                "[SUPERVISOR] Failed to reconcile managed agent sessions: {}",
                err
            ));
            return;
        }
    };

    for session_id in stale_session_ids {
        if let Err(err) = db::delete_target_leases_for_session(&conn, &session_id) {
            state.push_log(format!(
                "[SUPERVISOR] Failed to delete target leases for stale session {}: {}",
                session_id, err
            ));
            continue;
        }
        if let Err(err) = db::delete_agent_session(&conn, &session_id) {
            state.push_log(format!(
                "[SUPERVISOR] Failed to delete stale managed session {}: {}",
                session_id, err
            ));
            continue;
        }
        state.push_log(format!(
            "[SUPERVISOR] Cleared stale managed MCP session {}.",
            session_id
        ));
    }
}

fn looks_like_managed_startup_command(command: &str) -> bool {
    let normalized = command.trim();
    normalized.contains("The Ecky MCP server is at http://127.0.0.1:")
        || normalized.contains("call `request_user_prompt`")
        || normalized.contains("Do NOT call `bootstrap_ecky`")
}

#[cfg(unix)]
fn cleanup_orphaned_managed_processes(state: &AppState) {
    let live_group_ids = {
        let runtime = runtime_registry(state);
        runtime
            .by_id
            .values()
            .filter_map(|entry| entry.pid)
            .collect::<Vec<_>>()
    };

    let Ok(output) = ProcessCommand::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,command="])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut orphaned_group_ids = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(_pid) = parts.next().and_then(|value| value.parse::<i32>().ok()) else {
            continue;
        };
        let Some(ppid) = parts.next().and_then(|value| value.parse::<i32>().ok()) else {
            continue;
        };
        let Some(pgid) = parts.next().and_then(|value| value.parse::<i32>().ok()) else {
            continue;
        };
        let command = parts.collect::<Vec<_>>().join(" ");
        if ppid != 1
            || !looks_like_managed_startup_command(&command)
            || live_group_ids.contains(&pgid)
            || orphaned_group_ids.contains(&pgid)
        {
            continue;
        }
        orphaned_group_ids.push(pgid);
    }

    for pgid in orphaned_group_ids {
        if let Err(err) = kill_agent_process_group(pgid) {
            state.push_log(format!(
                "[SUPERVISOR] Failed to reap orphaned managed agent group {}: {}",
                pgid, err
            ));
            continue;
        }
        state.push_log(format!(
            "[SUPERVISOR] Reaped orphaned managed agent group {}.",
            pgid
        ));
    }
}

#[cfg(not(unix))]
fn cleanup_orphaned_managed_processes(_state: &AppState) {}

#[cfg(not(unix))]
fn kill_agent_process(pid: i32) -> AppResult<()> {
    let _ = pid;
    Err(AppError::internal(
        "Force-stopping auto-agents is not implemented on this platform.",
    ))
}

fn should_apply_process_exit_update(
    state: &AppState,
    agent_id: &str,
    exited_pid: Option<i32>,
) -> bool {
    runtime_registry(state)
        .snapshot_by_id(agent_id)
        .is_some_and(|snapshot| snapshot.pid == exited_pid)
}

fn update_agent_terminal_snapshot<F>(state: &AppState, agent_id: &str, updater: F)
where
    F: FnOnce(&mut AgentTerminalSnapshot),
{
    let snapshot = {
        let mut terminals = state.agent_terminals.lock().unwrap();
        let Some(runtime) = terminals.get_mut(agent_id) else {
            return;
        };
        updater(&mut runtime.snapshot);
        runtime.snapshot.updated_at = now_secs();
        runtime.last_emitted_at = Some(Instant::now());
        runtime.snapshot.clone()
    };

    state.emit_agent_terminal_update(&snapshot);
}

fn sync_terminal_snapshot_from_runtime(state: &AppState, runtime: &AutoAgentRuntimeSnapshot) {
    update_agent_terminal_snapshot(state, &runtime.agent_id, |snapshot| {
        snapshot.session_id = runtime.session_id.clone();
        snapshot.provider_kind = Some(runtime.provider_kind.clone());
        snapshot.busy = runtime.busy;
        snapshot.activity_label = runtime.activity_label.clone();
        snapshot.activity_started_at = runtime.activity_started_at;
        snapshot.attention_kind = runtime.attention_kind.clone();
    });
}

fn append_agent_terminal_output(
    state: &AppState,
    agent_id: &str,
    bytes: &[u8],
    observation: TerminalBehaviorObservation,
) {
    let now_secs_value = now_secs();
    let runtime_snapshot = {
        let mut runtime = runtime_registry(state);
        runtime.update_by_id(agent_id, |entry| {
            let next_attention_kind = observation
                .attention
                .as_ref()
                .map(|attention| attention.kind.clone());
            entry.attention_kind = next_attention_kind;

            if entry.busy {
                if let Some(activity) = observation.activity.as_ref() {
                    entry.activity_label = Some(activity.label.clone());
                    entry.activity_started_at = activity
                        .elapsed_secs
                        .map(|elapsed| now_secs_value.saturating_sub(elapsed))
                        .or(entry.activity_started_at)
                        .or(Some(now_secs_value));
                } else if entry.activity_started_at.is_none() {
                    entry.activity_started_at = Some(now_secs_value);
                }
            } else {
                entry.activity_label = None;
                entry.activity_started_at = None;
            }
        });
        runtime.snapshot_by_id(agent_id)
    };

    let Some(runtime_snapshot) = runtime_snapshot else {
        return;
    };

    let activity_label = runtime_snapshot.activity_label.clone();
    let activity_started_at = runtime_snapshot.activity_started_at;
    let attention_kind = runtime_snapshot.attention_kind.clone();
    let busy = runtime_snapshot.busy;
    let provider_kind = runtime_snapshot.provider_kind.clone();
    let attention_required = observation.attention.is_some();
    let summary = observation.summary();
    let (snapshot, should_emit, _activity_changed, _attention_changed) = {
        let mut terminals = state.agent_terminals.lock().unwrap();
        let Some(runtime) = terminals.get_mut(agent_id) else {
            return;
        };
        let raw = decode_terminal_text(&mut runtime.pending_utf8, bytes);
        // `vt_stream` is the authoritative replay source for live xterm rendering.
        runtime.snapshot.vt_stream.push_str(&raw);
        trim_output_tail(&mut runtime.snapshot.vt_stream, TERMINAL_VT_STREAM_LIMIT);
        // `screen_text` remains as a degraded readable fallback for inactive snapshots
        // and terminal-attention copy; it is not a live TUI model.
        apply_terminal_text(
            &mut runtime.snapshot.screen_text,
            &mut runtime.pending_escape,
            &raw,
        );
        trim_output_tail(
            &mut runtime.snapshot.screen_text,
            TERMINAL_SCREEN_TEXT_LIMIT,
        );
        let next_attention_required = attention_required && runtime.snapshot.active;
        let summary_changed = runtime.snapshot.summary.as_deref() != summary.as_deref();
        let activity_changed = runtime.snapshot.activity_label.as_deref()
            != activity_label.as_deref()
            || runtime.snapshot.activity_started_at != activity_started_at
            || runtime.snapshot.busy != busy;
        let attention_kind_changed =
            runtime.snapshot.attention_kind.as_deref() != attention_kind.as_deref();
        let attention_changed = runtime.snapshot.attention_required != next_attention_required
            || attention_kind_changed;
        let now = Instant::now();
        let emit_due_to_time = runtime.last_emitted_at.is_none_or(|last_emit| {
            now.duration_since(last_emit) >= Duration::from_millis(TERMINAL_EMIT_INTERVAL_MS)
        });
        runtime.snapshot.provider_kind = Some(provider_kind);
        runtime.snapshot.attention_required = next_attention_required;
        runtime.snapshot.busy = busy;
        runtime.snapshot.activity_label = activity_label;
        runtime.snapshot.activity_started_at = activity_started_at;
        runtime.snapshot.attention_kind = attention_kind;
        runtime.snapshot.summary = summary;
        runtime.snapshot.updated_at = now_secs_value;
        let should_emit =
            summary_changed || attention_changed || activity_changed || emit_due_to_time;
        if should_emit {
            runtime.last_emitted_at = Some(now);
        }
        let mut emitted_snapshot = runtime.snapshot.clone();
        if should_emit {
            if emitted_snapshot.active {
                emitted_snapshot.screen_text.clear();
            }
            emitted_snapshot.vt_delta = Some(raw);
            emitted_snapshot.vt_stream.clear();
        }
        (
            emitted_snapshot,
            should_emit,
            activity_changed,
            attention_changed,
        )
    };

    if should_emit {
        state.emit_agent_terminal_update(&snapshot);
    }
}

fn mark_agent_terminal_inactive(
    state: &AppState,
    agent: &AutoAgent,
    summary: Option<String>,
    active: bool,
) {
    update_agent_terminal_snapshot(state, &agent.id, |snapshot| {
        snapshot.active = active;
        snapshot.attention_required = false;
        snapshot.busy = false;
        snapshot.activity_label = None;
        snapshot.activity_started_at = None;
        snapshot.attention_kind = None;
        snapshot.summary = summary;
    });
}

fn clear_live_session_for_disconnect(state: &AppState, session_id: &str, status_text: &str) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let state = state.clone();
        let session_id = session_id.to_string();
        let status_text = status_text.to_string();
        handle.block_on(async move {
            // Close pending prompts before removing from mcp_sessions.
            state
                .close_prompts_for_session(&session_id, "session_disconnected")
                .await;
            let disconnected_session = {
                let mut sessions = state.mcp_sessions.lock().await;
                sessions.remove(&session_id)
            };
            if let Some(session) = disconnected_session {
                let conn = state.db.lock().await;
                let _ = db::upsert_agent_session(
                    &conn,
                    &AgentSession {
                        session_id: session_id.clone(),
                        client_kind: session.client_kind,
                        host_label: session.host_label,
                        agent_label: session.agent_label,
                        llm_model_id: session.llm_model_id,
                        llm_model_label: session.llm_model_label,
                        thread_id: session.bound_thread_id,
                        message_id: session
                            .last_target
                            .as_ref()
                            .map(|target| target.message_id.clone()),
                        model_id: session.last_target.and_then(|target| target.model_id),
                        phase: "disconnected".to_string(),
                        status_text: status_text.clone(),
                        updated_at: now_secs(),
                    },
                );
                let _ = db::delete_target_leases_for_session(&conn, &session_id);
            }
        });
    }
}

fn write_to_terminal(writer: &crate::models::AgentTerminalWriter, text: &str) -> AppResult<()> {
    let mut locked = writer.lock().unwrap();
    locked
        .write_all(text.as_bytes())
        .map_err(|err| AppError::internal(err.to_string()))?;
    locked
        .flush()
        .map_err(|err| AppError::internal(err.to_string()))?;
    Ok(())
}

fn trim_output_tail(output: &mut String, max_chars: usize) {
    if output.len() <= max_chars {
        return;
    }
    let keep_from = output
        .char_indices()
        .nth_back(max_chars.saturating_sub(1))
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    output.drain(..keep_from);
}

fn send_initial_prompt_if_needed(
    writer: &crate::models::AgentTerminalWriter,
    boot_state: &Arc<Mutex<AgentBootState>>,
    initial_prompt: &str,
) -> AppResult<bool> {
    {
        let mut state = boot_state.lock().unwrap();
        if state.initial_prompt_sent {
            return Ok(false);
        }
        state.initial_prompt_sent = true;
    }

    if let Err(err) = write_to_terminal(writer, &format!("{}\n", initial_prompt)) {
        let mut state = boot_state.lock().unwrap();
        state.initial_prompt_sent = false;
        return Err(err);
    }

    Ok(true)
}

fn maybe_auto_approve_workspace_trust_prompt(
    writer: &crate::models::AgentTerminalWriter,
    boot_state: &Arc<Mutex<AgentBootState>>,
    work_dir: &Path,
    output: &str,
) -> AppResult<bool> {
    if !is_managed_agent_workspace(work_dir) || !looks_like_workspace_trust_prompt(output) {
        return Ok(false);
    }

    {
        let mut state = boot_state.lock().unwrap();
        if state.trust_prompt_auto_approved || state.initial_prompt_sent {
            return Ok(false);
        }
        state.trust_prompt_auto_approved = true;
        state.trust_prompt_visible = true;
        state.attention_required = false;
    }

    if let Err(err) = write_to_terminal(writer, "\r") {
        let mut state = boot_state.lock().unwrap();
        state.trust_prompt_auto_approved = false;
        return Err(err);
    }

    Ok(true)
}

fn maybe_auto_approve_managed_mcp_startup_tool_prompt(
    writer: &crate::models::AgentTerminalWriter,
    boot_state: &Arc<Mutex<AgentBootState>>,
    work_dir: &Path,
    output: &str,
) -> AppResult<bool> {
    if !is_managed_agent_workspace(work_dir) {
        return Ok(false);
    }
    let Some(tool_name) = extract_safe_managed_mcp_tool_prompt(output) else {
        return Ok(false);
    };

    {
        let mut state = boot_state.lock().unwrap();
        if state
            .auto_approved_mcp_tool_prompts
            .iter()
            .any(|existing| existing == tool_name)
        {
            return Ok(false);
        }
        state
            .auto_approved_mcp_tool_prompts
            .push(tool_name.to_string());
        state.attention_required = false;
    }

    if let Err(err) = write_to_terminal(writer, "2\r") {
        let mut state = boot_state.lock().unwrap();
        state
            .auto_approved_mcp_tool_prompts
            .retain(|existing| existing != tool_name);
        return Err(err);
    }

    Ok(true)
}

fn capture_child_output(
    mut reader: Box<dyn Read + Send>,
    state: AppState,
    agent: AutoAgent,
    writer: crate::models::AgentTerminalWriter,
    boot_state: Arc<Mutex<AgentBootState>>,
    work_dir: PathBuf,
    output_tail: Arc<Mutex<String>>,
) {
    let mut detector_tail = String::new();
    let mut buf = [0_u8; CHILD_OUTPUT_CHUNK_BYTES];
    let provider = provider_kind_for_agent(&agent);
    let behavior = behavior_for_provider(provider);

    loop {
        let read = match reader.read(&mut buf) {
            Ok(read) => read,
            Err(err) => {
                state.push_log(format!(
                    "[SUPERVISOR] Failed to read {} terminal output: {}",
                    agent.label, err
                ));
                break;
            }
        };
        if read == 0 {
            break;
        }

        apply_terminal_bytes(&mut detector_tail, &buf[..read]);
        trim_output_tail(&mut detector_tail, TERMINAL_DETECTOR_TAIL_LIMIT);

        let mut observation = behavior.observe(&detector_tail);
        let trust_prompt_visible = looks_like_workspace_trust_prompt(&detector_tail);
        {
            let mut state = boot_state.lock().unwrap();
            state.trust_prompt_visible = trust_prompt_visible;
            state.attention_required = observation.attention.is_some();
        }

        if trust_prompt_visible {
            match maybe_auto_approve_workspace_trust_prompt(
                &writer,
                &boot_state,
                &work_dir,
                &detector_tail,
            ) {
                Ok(true) => {
                    observation.attention = None;
                    thread::sleep(Duration::from_millis(POST_AUTO_TRUST_PROMPT_DELAY_MS));
                }
                Ok(false) => {}
                Err(err) => {
                    state.push_log(format!(
                        "[SUPERVISOR] Failed to auto-confirm {} workspace trust: {}",
                        agent.label, err
                    ));
                }
            }
        }

        match maybe_auto_approve_managed_mcp_startup_tool_prompt(
            &writer,
            &boot_state,
            &work_dir,
            &detector_tail,
        ) {
            Ok(true) => {
                observation.attention = None;
                thread::sleep(Duration::from_millis(POST_AUTO_TRUST_PROMPT_DELAY_MS));
            }
            Ok(false) => {}
            Err(err) => {
                state.push_log(format!(
                    "[SUPERVISOR] Failed to auto-approve {} startup MCP prompt: {}",
                    agent.label, err
                ));
            }
        }

        {
            let mut tail = output_tail.lock().unwrap();
            apply_terminal_bytes(&mut tail, &buf[..read]);
            trim_output_tail(&mut tail, TERMINAL_SCREEN_TEXT_LIMIT);
        }

        append_agent_terminal_output(&state, &agent.id, &buf[..read], observation);
    }
}

async fn spawn_agent_once(state: &AppState, agent: &AutoAgent) -> AppResult<()> {
    let endpoint_url = wait_for_mcp_endpoint(state).await?;
    let managed_endpoint_url = managed_endpoint_url(&endpoint_url, agent);
    let work_dir = write_agent_instructions(agent, &managed_endpoint_url)?;
    let initial_prompt = build_initial_prompt(agent, &managed_endpoint_url);
    let initial_prompt_is_inline =
        !matches!(startup_prompt_mode(agent), StartupPromptMode::DeferredPty);
    let cmd_args = build_command_args(
        agent,
        initial_prompt_is_inline.then_some(initial_prompt.as_str()),
    );

    state.push_log(format!("[SUPERVISOR] Spawning agent: {}", agent.label));

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: TERMINAL_ROWS,
            cols: TERMINAL_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| AppError::internal(format!("pty open failed: {}", err)))?;

    let mut cmd = CommandBuilder::new(&agent.cmd);
    cmd.args(&cmd_args);
    cmd.cwd(&work_dir);
    if let Some(spawn_path) = build_spawn_path() {
        ensure_agent_command_resolvable(agent, spawn_path.as_os_str())?;
        cmd.env("PATH", &spawn_path);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|err| AppError::internal(format_spawn_failure(agent, &err.to_string())))?;
    let pid = child.process_id().map(|process_id| process_id as i32);
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| AppError::internal(format!("pty reader unavailable: {}", err)))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| AppError::internal(format!("pty writer unavailable: {}", err)))?;
    let writer: crate::models::AgentTerminalWriter = Arc::new(Mutex::new(writer));
    let pty: crate::models::AgentTerminalPty = Arc::new(Mutex::new(pair.master));
    replace_agent_terminal_session(state, agent, pty, writer.clone());

    {
        let mut runtime = runtime_registry(state);
        runtime.update_by_id(&agent.id, |entry| {
            entry.has_spawned = true;
            entry.pid = pid;
            entry.phase = AutoAgentRuntimePhase::Waking;
            entry.busy = false;
            entry.activity_label = None;
            entry.activity_started_at = None;
            entry.attention_kind = None;
            entry.last_error = None;
            entry.status_text = Some(format!("Waking {}...", agent.label));
        });
    }

    let boot_state = Arc::new(Mutex::new(AgentBootState::default()));
    let output_tail = Arc::new(Mutex::new(String::new()));

    let output_state = state.clone();
    let output_agent = agent.clone();
    let output_writer = writer.clone();
    let output_boot_state = boot_state.clone();
    let output_work_dir = work_dir.clone();
    let output_tail_clone = output_tail.clone();
    let output_handle = thread::Builder::new()
        .name(format!("agent-terminal-{}", agent.label))
        .spawn(move || {
            capture_child_output(
                reader,
                output_state,
                output_agent,
                output_writer,
                output_boot_state,
                output_work_dir,
                output_tail_clone,
            );
        })
        .map_err(|err| AppError::internal(format!("reader thread failed: {}", err)))?;

    let initial_prompt_task = if initial_prompt_is_inline {
        None
    } else {
        let prompt_state = state.clone();
        let prompt_agent = agent.clone();
        let prompt_writer = writer.clone();
        let prompt_boot_state = boot_state.clone();
        let prompt_initial_prompt = initial_prompt.clone();
        Some(tokio::spawn(async move {
            sleep(Duration::from_millis(INITIAL_PROMPT_DELAY_MS)).await;
            loop {
                let should_wait = {
                    let state = prompt_boot_state.lock().unwrap();
                    if state.initial_prompt_sent {
                        return;
                    }
                    state.trust_prompt_visible || state.attention_required
                };
                if !should_wait {
                    break;
                }
                sleep(Duration::from_millis(INITIAL_PROMPT_RETRY_MS)).await;
            }

            if let Err(err) = send_initial_prompt_if_needed(
                &prompt_writer,
                &prompt_boot_state,
                &prompt_initial_prompt,
            ) {
                prompt_state.push_log(format!(
                    "[SUPERVISOR] Failed to deliver {} startup prompt: {}",
                    prompt_agent.label, err
                ));
            }
        }))
    };

    let started_at = Instant::now();
    let exit = tauri::async_runtime::spawn_blocking(move || child.wait())
        .await
        .map_err(|err| AppError::internal(format!("wait join failed: {}", err)))?
        .map_err(|err| AppError::internal(format!("wait failed: {}", err)))?;
    if let Some(initial_prompt_task) = initial_prompt_task {
        let _ = initial_prompt_task.await;
    }
    if let Err(err) = output_handle.join() {
        state.push_log(format!(
            "[SUPERVISOR] {} reader thread panicked: {:?}",
            agent.label, err
        ));
    }

    let output_tail = output_tail.lock().unwrap().clone();

    if !should_apply_process_exit_update(state, &agent.id, pid) {
        state.push_log(format!(
            "[SUPERVISOR] Ignoring stale exit update for {} (pid {:?}).",
            agent.label, pid
        ));
        return Ok(());
    }

    let previous_session_id = runtime_registry(state)
        .snapshot_by_id(&agent.id)
        .and_then(|snapshot| snapshot.session_id);

    let (phase, status_text, last_error) = if exit.success() {
        (
            AutoAgentRuntimePhase::Disconnected,
            format!(
                "{} disconnected after {}s.",
                agent.label,
                started_at.elapsed().as_secs()
            ),
            None,
        )
    } else {
        let err =
            format_agent_exit_error(agent, &exit, started_at.elapsed().as_secs(), &output_tail);
        (AutoAgentRuntimePhase::Error, err.clone(), Some(err))
    };

    mark_agent_terminal_inactive(
        state,
        agent,
        Some(if exit.success() {
            format!("{} terminal closed.", agent.label)
        } else {
            format!("{} terminal closed with an error.", agent.label)
        }),
        false,
    );

    state.push_log(format!("[SUPERVISOR] {}", status_text));
    let mut runtime = runtime_registry(state);
    runtime.update_by_id(&agent.id, |entry| {
        entry.pid = None;
        entry.session_id = None;
        entry.phase = phase;
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.status_text = Some(status_text.clone());
        entry.last_error = last_error.clone();
        entry.llm_model_label = None;
    });
    if let Some(session_id) = previous_session_id {
        clear_live_session_for_disconnect(state, &session_id, &status_text);
    }
    Ok(())
}

pub async fn stop_primary_auto_agent(
    state: &AppState,
    thread_id: Option<String>,
    _message_id: Option<String>,
    _model_id: Option<String>,
) -> AppResult<()> {
    let config = state.config.lock().unwrap().clone();
    let Some(primary) = primary_auto_agent(&config) else {
        return Err(AppError::validation(
            "No primary active MCP agent is configured.",
        ));
    };
    stop_auto_agent_by_id(state, &primary.id, thread_id)
}

pub async fn restart_primary_auto_agent(
    state: &AppState,
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
) -> AppResult<()> {
    let config = state.config.lock().unwrap().clone();
    let Some(primary) = primary_auto_agent(&config) else {
        return Err(AppError::validation(
            "No primary active MCP agent is configured.",
        ));
    };
    restart_auto_agent_by_id(state, &primary.id, thread_id, message_id, model_id)
}

fn stop_auto_agent_by_id(
    state: &AppState,
    agent_id: &str,
    thread_id: Option<String>,
) -> AppResult<()> {
    sync_auto_agent_supervisors(state.clone());
    let snapshot = runtime_registry(state)
        .snapshot_by_id(agent_id)
        .ok_or_else(|| AppError::not_found("Auto-agent runtime not initialized."))?;
    let agent = runtime_registry(state)
        .agent_by_id(agent_id)
        .ok_or_else(|| AppError::not_found("Auto-agent runtime not initialized."))?;

    if let Some(pid) = snapshot.pid {
        kill_agent_process(pid)?;
    }

    let target_thread_id = thread_id
        .or(snapshot.pending_thread_id.clone())
        .or_else(|| {
            db::get_active_agent_sessions(
                &state.db.blocking_lock(),
                THREAD_STATE_STALE_THRESHOLD_SECS,
            )
            .ok()
            .and_then(|sessions| {
                sessions
                    .into_iter()
                    .find(|session| session.agent_label == agent.label)
                    .and_then(|session| session.thread_id)
            })
        });

    let status_text = format!("{} stopped by user.", agent.label);
    mark_agent_stopped(state, &agent, target_thread_id, status_text.clone());
    state.push_log(format!("[SUPERVISOR] {}", status_text));
    Ok(())
}

fn restart_auto_agent_by_id(
    state: &AppState,
    agent_id: &str,
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
) -> AppResult<()> {
    stop_auto_agent_by_id(state, agent_id, thread_id.clone())?;
    wake_auto_agent_by_id(state, agent_id, thread_id, message_id, model_id)
}

pub async fn wake_primary_auto_agent(
    state: &AppState,
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
) -> AppResult<()> {
    let config = state.config.lock().unwrap().clone();
    let Some(primary) = primary_auto_agent(&config) else {
        return Err(AppError::validation(
            "No primary active MCP agent is configured.",
        ));
    };
    if let Some(target_thread_id) = thread_id.as_deref() {
        let conn = state.db.lock().await;
        let conflicting_session =
            db::get_active_agent_sessions(&conn, THREAD_STATE_STALE_THRESHOLD_SECS)
                .map_err(|err| AppError::persistence(err.to_string()))?
                .into_iter()
                .find(|session| {
                    session.thread_id.as_deref() == Some(target_thread_id)
                        && session.agent_label != primary.label
                        && session.phase != "idle"
                });
        drop(conn);

        if let Some(session) = conflicting_session {
            return Err(AppError::validation(format!(
                "Thread is currently occupied by external agent '{}' via {}.",
                session.agent_label, session.host_label
            )));
        }
    }
    wake_auto_agent_by_id(state, &primary.id, thread_id, message_id, model_id)
}

pub async fn wake_auto_agent_by_label(
    state: &AppState,
    label: &str,
    thread_id: Option<String>,
) -> AppResult<()> {
    let agent_id = runtime_registry(state)
        .snapshot_by_label(label)
        .map(|snapshot| snapshot.agent_id)
        .ok_or_else(|| AppError::not_found(format!("Auto-agent not found: {}", label)))?;
    wake_auto_agent_by_id(state, &agent_id, thread_id, None, None)
}

fn wake_auto_agent_by_id(
    state: &AppState,
    agent_id: &str,
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
) -> AppResult<()> {
    sync_auto_agent_supervisors(state.clone());
    let (notify, agent_label) = {
        let mut runtime = runtime_registry(state);
        let snapshot = runtime
            .snapshot_by_id(agent_id)
            .ok_or_else(|| AppError::not_found("Auto-agent runtime not initialized."))?;
        let stale_waking = matches!(snapshot.phase, AutoAgentRuntimePhase::Waking)
            && snapshot.pid.is_none()
            && snapshot.session_id.is_none();

        if snapshot.pid.is_some()
            || matches!(
                snapshot.phase,
                AutoAgentRuntimePhase::Waiting | AutoAgentRuntimePhase::Active
            )
            || (matches!(snapshot.phase, AutoAgentRuntimePhase::Waking) && !stale_waking)
        {
            runtime.update_by_id(agent_id, |entry| {
                if thread_id.is_some() {
                    set_runtime_pending_target(
                        entry,
                        thread_id.clone(),
                        message_id.clone(),
                        model_id.clone(),
                    );
                }
            });
            return Ok(());
        }

        let notify = runtime
            .by_id
            .get(agent_id)
            .map(|entry| entry.notify.clone())
            .ok_or_else(|| AppError::not_found("Auto-agent runtime not initialized."))?;
        let agent_label = snapshot.agent_label.clone();
        runtime.update_by_id(agent_id, |entry| {
            entry.phase = AutoAgentRuntimePhase::Waking;
            set_runtime_pending_target(
                entry,
                thread_id.clone(),
                message_id.clone(),
                model_id.clone(),
            );
            entry.busy = false;
            entry.activity_label = None;
            entry.activity_started_at = None;
            entry.attention_kind = None;
            entry.waiting_on_prompt = false;
            entry.status_text = Some(format!("Waking {}...", entry.agent.label));
            entry.last_error = None;
        });
        (notify, agent_label)
    };

    state.push_log(format!("[SUPERVISOR] Wake requested for {}", agent_label));
    notify.notify_one();
    Ok(())
}

pub fn mark_agent_waiting(
    state: &AppState,
    agent_label: &str,
    session_id: &str,
    llm_model_label: Option<String>,
    status_text: Option<String>,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_label(agent_label, |entry| {
        entry.phase = AutoAgentRuntimePhase::Waiting;
        entry.session_id = Some(session_id.to_string());
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = true;
        if llm_model_label.is_some() {
            entry.llm_model_label = llm_model_label.clone();
        }
        entry.status_text = status_text.clone();
    });
    if let Some(snapshot) = runtime.snapshot_by_label(agent_label) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn bind_managed_http_session(
    state: &AppState,
    agent_id: &str,
    session_id: &str,
    status_text: Option<String>,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_id(agent_id, |entry| {
        entry.session_id = Some(session_id.to_string());
        entry.phase = AutoAgentRuntimePhase::Active;
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = false;
        entry.status_text = status_text.clone();
        entry.last_error = None;
    });
    if let Some(snapshot) = runtime.snapshot_by_id(agent_id) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn mark_managed_session_waiting(
    state: &AppState,
    session_id: &str,
    llm_model_label: Option<String>,
    status_text: Option<String>,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_session_id(session_id, |entry| {
        entry.phase = AutoAgentRuntimePhase::Waiting;
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = true;
        if llm_model_label.is_some() {
            entry.llm_model_label = llm_model_label.clone();
        }
        entry.status_text = status_text.clone();
        entry.last_error = None;
    });
    if let Some(snapshot) = runtime.find_by_session_id(session_id) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn mark_managed_session_active(
    state: &AppState,
    session_id: &str,
    thread_id: Option<String>,
    llm_model_label: Option<String>,
    status_text: Option<String>,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_session_id(session_id, |entry| {
        entry.phase = AutoAgentRuntimePhase::Active;
        match thread_id.clone() {
            Some(thread_id) => update_runtime_pending_thread(entry, Some(thread_id)),
            None => set_runtime_pending_target(entry, None, None, None),
        }
        if llm_model_label.is_some() {
            entry.llm_model_label = llm_model_label.clone();
        }
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = false;
        entry.status_text = status_text.clone();
        entry.last_error = None;
    });
    if let Some(snapshot) = runtime.find_by_session_id(session_id) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn mark_managed_session_turn_busy(
    state: &AppState,
    session_id: &str,
    thread_id: Option<String>,
    llm_model_label: Option<String>,
    status_text: Option<String>,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_session_id(session_id, |entry| {
        entry.phase = AutoAgentRuntimePhase::Active;
        match thread_id.clone() {
            Some(thread_id) => update_runtime_pending_thread(entry, Some(thread_id)),
            None => set_runtime_pending_target(entry, None, None, None),
        }
        if llm_model_label.is_some() {
            entry.llm_model_label = llm_model_label.clone();
        }
        entry.busy = true;
        if entry.activity_started_at.is_none() {
            entry.activity_started_at = Some(now_secs());
        }
        entry.waiting_on_prompt = false;
        entry.status_text = status_text.clone();
        entry.last_error = None;
    });
    if let Some(snapshot) = runtime.find_by_session_id(session_id) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn mark_managed_session_error(
    state: &AppState,
    session_id: &str,
    thread_id: Option<String>,
    error_text: String,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_session_id(session_id, |entry| {
        entry.phase = AutoAgentRuntimePhase::Error;
        match thread_id.clone() {
            Some(thread_id) => update_runtime_pending_thread(entry, Some(thread_id)),
            None => set_runtime_pending_target(entry, None, None, None),
        }
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = false;
        entry.status_text = Some(error_text.clone());
        entry.last_error = Some(error_text.clone());
    });
    if let Some(snapshot) = runtime.find_by_session_id(session_id) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn mark_agent_active(
    state: &AppState,
    agent_label: &str,
    session_id: Option<String>,
    thread_id: Option<String>,
    llm_model_label: Option<String>,
    status_text: Option<String>,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_label(agent_label, |entry| {
        entry.phase = AutoAgentRuntimePhase::Active;
        if let Some(session_id) = session_id.clone() {
            entry.session_id = Some(session_id);
        }
        match thread_id.clone() {
            Some(thread_id) => update_runtime_pending_thread(entry, Some(thread_id)),
            None => set_runtime_pending_target(entry, None, None, None),
        }
        if llm_model_label.is_some() {
            entry.llm_model_label = llm_model_label.clone();
        }
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = false;
        entry.status_text = status_text.clone();
        entry.last_error = None;
    });
    if let Some(snapshot) = runtime.snapshot_by_label(agent_label) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn mark_agent_turn_busy(
    state: &AppState,
    agent_label: &str,
    session_id: Option<String>,
    thread_id: Option<String>,
    llm_model_label: Option<String>,
    status_text: Option<String>,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_label(agent_label, |entry| {
        entry.phase = AutoAgentRuntimePhase::Active;
        if let Some(session_id) = session_id.clone() {
            entry.session_id = Some(session_id);
        }
        match thread_id.clone() {
            Some(thread_id) => update_runtime_pending_thread(entry, Some(thread_id)),
            None => set_runtime_pending_target(entry, None, None, None),
        }
        if llm_model_label.is_some() {
            entry.llm_model_label = llm_model_label.clone();
        }
        entry.busy = true;
        if entry.activity_started_at.is_none() {
            entry.activity_started_at = Some(now_secs());
        }
        entry.waiting_on_prompt = false;
        entry.status_text = status_text.clone();
        entry.last_error = None;
    });
    if let Some(snapshot) = runtime.snapshot_by_label(agent_label) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn mark_agent_disconnected_for_session(
    state: &AppState,
    session_id: &str,
    status_text: Option<String>,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_session_id(session_id, |entry| {
        entry.phase = AutoAgentRuntimePhase::Disconnected;
        entry.session_id = None;
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = false;
        entry.status_text = status_text.clone();
    });
    if let Some(snapshot) = runtime.find_by_session_id(session_id) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn mark_agent_error(
    state: &AppState,
    agent_label: &str,
    thread_id: Option<String>,
    error_text: String,
) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_label(agent_label, |entry| {
        entry.phase = AutoAgentRuntimePhase::Error;
        update_runtime_pending_thread(entry, thread_id.clone());
        entry.busy = false;
        entry.activity_label = None;
        entry.activity_started_at = None;
        entry.attention_kind = None;
        entry.waiting_on_prompt = false;
        entry.status_text = Some(error_text.clone());
        entry.last_error = Some(error_text.clone());
    });
    if let Some(snapshot) = runtime.snapshot_by_label(agent_label) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

pub fn associate_session_target(state: &AppState, session_id: &str, target: Option<&McpTargetRef>) {
    let mut runtime = runtime_registry(state);
    runtime.update_by_session_id(session_id, |entry| {
        set_runtime_pending_target(
            entry,
            target.map(|target| target.thread_id.clone()),
            target.map(|target| target.message_id.clone()),
            target.and_then(|target| target.model_id.clone()),
        );
        if target.is_some() {
            entry.phase = AutoAgentRuntimePhase::Active;
        }
    });
    if let Some(snapshot) = runtime.find_by_session_id(session_id) {
        sync_terminal_snapshot_from_runtime(state, &snapshot);
    }
}

fn thread_matches_runtime(
    runtime: &AutoAgentRuntimeSnapshot,
    thread_id: &str,
    live_session: Option<&McpSessionState>,
    last_session: Option<&AgentSession>,
) -> bool {
    if runtime.pending_thread_id.as_deref() == Some(thread_id) {
        return true;
    }
    if last_session.and_then(|session| session.thread_id.as_deref()) == Some(thread_id) {
        return true;
    }
    if live_session
        .and_then(|session| session.last_target.as_ref())
        .map(|target| target.thread_id.as_str())
        == Some(thread_id)
    {
        return true;
    }
    if live_session.and_then(|session| session.bound_thread_id.as_deref()) == Some(thread_id) {
        return true;
    }
    matches!(runtime.phase, AutoAgentRuntimePhase::Sleeping) && runtime.pending_thread_id.is_none()
}

fn thread_matches_live_session(session: &McpSessionState, thread_id: &str) -> bool {
    session
        .last_target
        .as_ref()
        .map(|target| target.thread_id.as_str())
        == Some(thread_id)
        || session.bound_thread_id.as_deref() == Some(thread_id)
}

fn provider_kind_for_live_session(session: &McpSessionState) -> Option<String> {
    let direct = provider_kind_from_cmd(&session.agent_label);
    if direct != AgentTerminalProviderKind::Generic {
        return Some(direct.as_str().to_string());
    }
    let fallback = provider_kind_from_cmd(&session.host_label);
    (fallback != AgentTerminalProviderKind::Generic).then(|| fallback.as_str().to_string())
}

#[allow(clippy::too_many_arguments)]
fn build_thread_agent_state(
    connection_state: &str,
    runtime: Option<&AutoAgentRuntimeSnapshot>,
    live_session: Option<&McpSessionState>,
    agent_label: Option<String>,
    llm_model_label: Option<String>,
    session_id: Option<String>,
    phase: Option<String>,
    status_text: Option<String>,
    updated_at: Option<u64>,
) -> ThreadAgentState {
    ThreadAgentState {
        connection_state: connection_state.to_string(),
        agent_label,
        llm_model_label,
        provider_kind: runtime
            .map(|runtime| runtime.provider_kind.clone())
            .or_else(|| live_session.and_then(provider_kind_for_live_session)),
        session_id,
        phase: live_session
            .and_then(|session| session.phase.clone())
            .or(phase),
        status_text: live_session
            .and_then(|session| session.status_text.clone())
            .or(status_text),
        busy: live_session.map(|session| session.busy).unwrap_or(false),
        activity_label: live_session.and_then(|session| session.activity_label.clone()),
        activity_started_at: live_session.and_then(|session| session.activity_started_at),
        attention_kind: live_session.and_then(|session| session.attention_kind.clone()),
        waiting_on_prompt: live_session
            .map(|session| session.waiting_on_prompt)
            .unwrap_or(false),
        updated_at: live_session
            .map(|session| session.updated_at)
            .or(updated_at),
    }
}

pub fn derive_thread_agent_state(
    config: &Config,
    thread_id: &str,
    inputs: ThreadAgentStateInputs,
) -> ThreadAgentState {
    let ThreadAgentStateInputs {
        live_session_id,
        runtime,
        live_session,
        last_session,
        now,
    } = inputs;
    let stale_threshold = now.saturating_sub(THREAD_STATE_STALE_THRESHOLD_SECS);
    let fresh_last_session = last_session
        .clone()
        .filter(|session| session.updated_at >= stale_threshold);

    if let Some(session) = fresh_last_session.as_ref() {
        if session.phase == "error" {
            return build_thread_agent_state(
                "error",
                runtime.as_ref(),
                live_session.as_ref(),
                Some(session.agent_label.clone()),
                session.llm_model_label.clone(),
                Some(session.session_id.clone()),
                Some(session.phase.clone()),
                (!session.status_text.trim().is_empty()).then_some(session.status_text.clone()),
                Some(session.updated_at),
            );
        }
    }

    if let Some(primary) = primary_auto_agent(config) {
        if let Some(runtime) = runtime.as_ref() {
            let runtime_relevant = thread_matches_runtime(
                runtime,
                thread_id,
                live_session.as_ref(),
                fresh_last_session.as_ref(),
            );
            if runtime_relevant {
                let agent_label = Some(runtime.agent_label.clone());
                let llm_model_label = runtime.llm_model_label.clone().or_else(|| {
                    fresh_last_session
                        .as_ref()
                        .and_then(|session| session.llm_model_label.clone())
                });
                let updated_at = Some(runtime.updated_at);
                let status_text = runtime
                    .status_text
                    .clone()
                    .or_else(|| runtime.last_error.clone());
                match runtime.phase {
                    AutoAgentRuntimePhase::Sleeping => {
                        return build_thread_agent_state(
                            "sleeping",
                            Some(runtime),
                            live_session.as_ref(),
                            agent_label,
                            llm_model_label,
                            runtime.session_id.clone(),
                            None,
                            status_text.or_else(|| {
                                Some(format!(
                                    "{} is sleeping until a queued message arrives.",
                                    primary.label
                                ))
                            }),
                            updated_at,
                        );
                    }
                    AutoAgentRuntimePhase::Waking => {
                        return build_thread_agent_state(
                            "waking",
                            Some(runtime),
                            live_session.as_ref(),
                            agent_label,
                            llm_model_label,
                            runtime.session_id.clone(),
                            None,
                            status_text.or_else(|| Some(format!("Waking {}...", primary.label))),
                            updated_at,
                        );
                    }
                    AutoAgentRuntimePhase::Waiting => {
                        return build_thread_agent_state(
                            "waiting",
                            Some(runtime),
                            live_session.as_ref(),
                            agent_label,
                            llm_model_label,
                            runtime.session_id.clone(),
                            Some("waiting_for_user".to_string()),
                            status_text.or_else(|| {
                                Some("Waiting for your next queued message.".to_string())
                            }),
                            updated_at,
                        );
                    }
                    AutoAgentRuntimePhase::Active => {
                        if let Some(session) = fresh_last_session {
                            return build_thread_agent_state(
                                "active",
                                Some(runtime),
                                live_session.as_ref(),
                                Some(session.agent_label),
                                session.llm_model_label,
                                Some(session.session_id),
                                Some(session.phase),
                                (!session.status_text.trim().is_empty())
                                    .then_some(session.status_text),
                                Some(session.updated_at),
                            );
                        }
                        return build_thread_agent_state(
                            "active",
                            Some(runtime),
                            live_session.as_ref(),
                            agent_label,
                            llm_model_label,
                            runtime.session_id.clone(),
                            None,
                            status_text,
                            updated_at,
                        );
                    }
                    AutoAgentRuntimePhase::Disconnected => {
                        return build_thread_agent_state(
                            "disconnected",
                            Some(runtime),
                            live_session.as_ref(),
                            agent_label,
                            llm_model_label,
                            runtime.session_id.clone(),
                            None,
                            status_text
                                .or_else(|| Some(format!("{} disconnected.", primary.label))),
                            updated_at,
                        );
                    }
                    AutoAgentRuntimePhase::Error => {
                        return build_thread_agent_state(
                            "error",
                            Some(runtime),
                            live_session.as_ref(),
                            agent_label,
                            llm_model_label,
                            runtime.session_id.clone(),
                            Some("error".to_string()),
                            status_text
                                .or_else(|| Some(format!("{} failed to connect.", primary.label))),
                            updated_at,
                        );
                    }
                }
            }
        }
    }

    if let Some(session) = live_session
        .as_ref()
        .filter(|session| thread_matches_live_session(session, thread_id))
    {
        let connection_state = if session.waiting_on_prompt {
            "waiting"
        } else {
            "active"
        };
        return build_thread_agent_state(
            connection_state,
            None,
            Some(session),
            Some(session.agent_label.clone()),
            session.llm_model_label.clone(),
            live_session_id,
            session.phase.clone(),
            session.status_text.clone(),
            Some(session.updated_at),
        );
    }

    if let Some(session) = fresh_last_session {
        return build_thread_agent_state(
            if session.phase == "error" {
                "error"
            } else {
                "disconnected"
            },
            runtime.as_ref(),
            live_session.as_ref(),
            Some(session.agent_label),
            session.llm_model_label,
            Some(session.session_id),
            Some(session.phase),
            (!session.status_text.trim().is_empty()).then_some(session.status_text),
            Some(session.updated_at),
        );
    }

    build_thread_agent_state(
        "none",
        None,
        live_session.as_ref(),
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

pub struct ThreadAgentStateInputs {
    pub live_session_id: Option<String>,
    pub runtime: Option<AutoAgentRuntimeSnapshot>,
    pub live_session: Option<McpSessionState>,
    pub last_session: Option<AgentSession>,
    pub now: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use crate::contracts::McpConfig;

    fn test_config(primary_agent_id: Option<&str>) -> Config {
        Config {
            engines: vec![],
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            assets: vec![],
            microwave: None,
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig {
                port: None,
                max_sessions: None,
                mode: McpMode::Active,
                primary_agent_id: primary_agent_id.map(str::to_string),
                prompt_timeout_secs: 1800,
                auto_agents: vec![],
            },
            has_seen_onboarding: false,
            connection_type: Some("mcp".to_string()),
            default_engine_kind: crate::models::EngineKind::Freecad,
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::models::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
        }
    }

    fn build_agent(id: &str, label: &str, args: Vec<String>) -> AutoAgent {
        AutoAgent {
            id: id.to_string(),
            label: label.to_string(),
            cmd: "/bin/sh".to_string(),
            model: None,
            args,
            enabled: true,
            start_on_demand: false,
        }
    }

    fn build_agent_with_cmd(id: &str, label: &str, cmd: &str, args: Vec<String>) -> AutoAgent {
        AutoAgent {
            id: id.to_string(),
            label: label.to_string(),
            cmd: cmd.to_string(),
            model: None,
            args,
            enabled: true,
            start_on_demand: false,
        }
    }

    fn open_test_terminal_handles() -> (
        crate::models::AgentTerminalPty,
        crate::models::AgentTerminalWriter,
    ) {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: TERMINAL_ROWS,
                cols: TERMINAL_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("pty");
        let writer = pair.master.take_writer().expect("writer");
        (
            Arc::new(Mutex::new(pair.master)),
            Arc::new(Mutex::new(writer)),
        )
    }

    fn test_state(config: Config) -> AppState {
        let db_path = unique_file("runtime-test.sqlite");
        let conn = crate::db::init_db(&db_path).unwrap();
        AppState::new(config, None, conn)
    }

    fn unique_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{}-{}", name, uuid::Uuid::new_v4()))
    }

    #[cfg(unix)]
    fn mark_test_executable(path: &Path) {
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }

    #[cfg(not(unix))]
    fn mark_test_executable(_path: &Path) {}

    #[test]
    fn managed_agent_workspace_detection_is_limited_to_our_temp_dirs() {
        let managed = std::env::temp_dir().join("ecky-agent-claude");
        let foreign_temp = std::env::temp_dir().join("other-agent-claude");
        let repo_path = PathBuf::from("/tmp/not-the-right-prefix/ecky-agent-claude");

        assert!(is_managed_agent_workspace(&managed));
        assert!(!is_managed_agent_workspace(&foreign_temp));
        assert!(!is_managed_agent_workspace(&repo_path));
    }

    #[test]
    fn workspace_trust_prompt_detection_matches_expected_cli_copy() {
        assert!(looks_like_workspace_trust_prompt(
            "Quick safety check: Is this a project you created or one you trust?\n\
             ❯ 1. Yes, I trust this folder\n\
             2. No, exit\n\
             Enter to confirm · Esc to cancel"
        ));
        assert!(!looks_like_workspace_trust_prompt(
            "Waiting for your next queued message."
        ));
    }

    #[test]
    fn sanitize_terminal_output_makes_claude_trust_prompt_readable() {
        let raw = b"\x1b[1CAccessing\x1b[1Cworkspace:\n\n\
            \x1b[1CQuick\x1b[1Csafety\x1b[1Ccheck:\n\
            \x1b[1C\xE2\x9D\xAF\x1b[1C1.\x1b[1CYes,\x1b[1CI\x1b[1Ctrust\x1b[1Cthis\x1b[1Cfolder\n\
            \x1b[3C2.\x1b[1CNo,\x1b[1Cexit\n\
            \x1b[1CEnter\x1b[1Cto\x1b[1Cconfirm";

        let mut sanitized = String::new();
        apply_terminal_bytes(&mut sanitized, raw);
        assert!(sanitized.contains("Accessing workspace"));
        assert!(sanitized.contains("Quick safety check"));
        assert!(sanitized.contains("Yes, I trust this folder"));
        assert!(sanitized.contains("Enter to confirm"));
    }

    #[test]
    fn apply_terminal_bytes_overwrites_animated_status_line_in_place() {
        let mut output = String::new();
        apply_terminal_bytes(&mut output, b"Claude is combobulating...");
        apply_terminal_bytes(&mut output, b"\rClaude is still combobulating...");

        assert_eq!(output, "Claude is still combobulating...");
    }

    #[test]
    fn apply_terminal_bytes_clears_line_suffix_after_shorter_rewrite() {
        let mut output = String::new();
        apply_terminal_bytes(&mut output, b"Combobulating aggressively");
        apply_terminal_bytes(&mut output, b"\rReady");

        assert_eq!(output, "Ready");
    }

    #[test]
    fn terminal_parser_preserves_split_ansi_sequences_across_chunks() {
        let mut output = String::new();
        let mut pending_utf8 = Vec::new();
        let mut pending_escape = String::new();

        let raw = decode_terminal_text(&mut pending_utf8, b"\x1b[38;2;207;");
        apply_terminal_text(&mut output, &mut pending_escape, &raw);
        assert_eq!(output, "");
        assert!(!pending_escape.is_empty());

        let raw = decode_terminal_text(&mut pending_utf8, b"210;213mHello\x1b[0m");
        apply_terminal_text(&mut output, &mut pending_escape, &raw);

        assert_eq!(output, "Hello");
        assert!(pending_escape.is_empty());
    }

    #[test]
    fn terminal_parser_preserves_split_utf8_characters_across_chunks() {
        let mut output = String::new();
        let mut pending_utf8 = Vec::new();
        let mut pending_escape = String::new();

        let raw = decode_terminal_text(&mut pending_utf8, &[0xE2, 0x94]);
        apply_terminal_text(&mut output, &mut pending_escape, &raw);
        assert_eq!(output, "");
        assert_eq!(pending_utf8, vec![0xE2, 0x94]);

        let raw = decode_terminal_text(&mut pending_utf8, &[0x82, b'-', b'-']);
        apply_terminal_text(&mut output, &mut pending_escape, &raw);

        assert_eq!(output, "│--");
        assert!(pending_utf8.is_empty());
    }

    #[test]
    fn startup_prompt_modes_match_cli_capabilities() {
        let claude = build_agent_with_cmd(
            "a1",
            "claude",
            "/Users/bogdan/.asdf/shims/claude",
            vec!["--dangerously-skip-permissions".to_string()],
        );
        let codex = build_agent_with_cmd("a2", "codex", "codex", vec![]);
        let gemini = build_agent_with_cmd("a3", "gemini", "gemini", vec![]);
        let opencode = build_agent_with_cmd("a4", "opencode", "opencode", vec![]);
        let amp = build_agent_with_cmd("a5", "amp", "amp", vec![]);

        assert_eq!(startup_prompt_mode(&claude), StartupPromptMode::Positional);
        assert_eq!(startup_prompt_mode(&codex), StartupPromptMode::Positional);
        assert_eq!(
            startup_prompt_mode(&gemini),
            StartupPromptMode::Flag("--prompt-interactive")
        );
        assert_eq!(
            startup_prompt_mode(&opencode),
            StartupPromptMode::Flag("--prompt")
        );
        assert_eq!(startup_prompt_mode(&amp), StartupPromptMode::DeferredPty);

        let claude_command = build_command(&claude, Some("bootstrap now"));
        let codex_command = build_command(&codex, Some("bootstrap now"));
        let gemini_command = build_command(&gemini, Some("bootstrap now"));
        let opencode_command = build_command(&opencode, Some("bootstrap now"));
        let amp_command = build_command(&amp, Some("bootstrap now"));

        assert!(claude_command.contains("'bootstrap now'"));
        assert!(codex_command.contains("'bootstrap now'"));
        assert!(gemini_command.contains("--prompt-interactive"));
        assert!(gemini_command.contains("'bootstrap now'"));
        assert!(opencode_command.contains("--prompt"));
        assert!(opencode_command.contains("'bootstrap now'"));
        assert!(!amp_command.contains("'bootstrap now'"));
    }

    #[test]
    fn positional_startup_prompt_is_separated_from_variadic_args() {
        let claude = build_agent_with_cmd(
            "a1",
            "claude",
            "claude",
            vec![
                "--allowedTools".to_string(),
                "Read".to_string(),
                "LS".to_string(),
                "Glob".to_string(),
                "Grep".to_string(),
                "mcp__ecky_mcp__*".to_string(),
            ],
        );

        let args = build_command_args(&claude, Some("bootstrap now"));
        assert_eq!(
            args,
            vec![
                "--allowedTools".to_string(),
                "Read".to_string(),
                "LS".to_string(),
                "Glob".to_string(),
                "Grep".to_string(),
                "mcp__ecky_mcp__*".to_string(),
                "--".to_string(),
                "bootstrap now".to_string(),
            ]
        );
    }

    #[test]
    fn spawn_path_merges_login_shell_entries_for_release_like_environment() {
        let path = build_spawn_path_from_sources(
            Some("/usr/bin:/bin:/usr/sbin:/sbin"),
            Some("/Users/test/.asdf/shims:/opt/homebrew/bin:/usr/bin:/bin"),
            Some(Path::new("/Users/test")),
        )
        .expect("merged spawn path");

        let parts = std::env::split_paths(&path)
            .map(|entry| entry.display().to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            parts,
            vec![
                "/usr/bin".to_string(),
                "/bin".to_string(),
                "/usr/sbin".to_string(),
                "/sbin".to_string(),
                "/Users/test/.asdf/shims".to_string(),
                "/opt/homebrew/bin".to_string(),
                "/Users/test/.local/bin".to_string(),
                "/Users/test/bin".to_string(),
                "/Users/test/.volta/bin".to_string(),
                "/Users/test/.npm/bin".to_string(),
                "/Users/test/.bun/bin".to_string(),
                "/opt/homebrew/sbin".to_string(),
            ]
        );
    }

    #[test]
    fn spawn_preflight_accepts_bare_command_found_on_merged_path() {
        let fake_bin_dir = unique_file("spawn-path-bin");
        fs::create_dir_all(&fake_bin_dir).expect("mkdir");
        let fake_gemini = fake_bin_dir.join("gemini");
        fs::write(&fake_gemini, "#!/bin/sh\nexit 0\n").expect("write");
        mark_test_executable(&fake_gemini);

        let merged_path = build_spawn_path_from_sources(
            Some("/usr/bin:/bin:/usr/sbin:/sbin"),
            Some(fake_bin_dir.to_string_lossy().as_ref()),
            None,
        )
        .expect("merged path");
        let agent = build_agent_with_cmd("a3", "gemini", "gemini", vec![]);

        ensure_agent_command_resolvable(&agent, &merged_path).expect("preflight should pass");
    }

    #[test]
    fn spawn_preflight_returns_targeted_error_when_bare_command_is_missing() {
        let missing_command = format!("missing-agent-command-{}", uuid::Uuid::new_v4().simple());
        let merged_path = build_spawn_path_from_sources(
            Some("/usr/bin:/bin:/usr/sbin:/sbin"),
            None,
            Some(Path::new("/Users/test")),
        )
        .expect("merged path");
        let agent = build_agent_with_cmd("a3", "gemini", &missing_command, vec![]);

        let err =
            ensure_agent_command_resolvable(&agent, &merged_path).expect_err("missing command");
        assert!(err.message.contains(&format!("`{}`", missing_command)));
        assert!(err.message.contains("reduced environment"));
        assert!(err.message.contains("absolute path"));
    }

    #[test]
    fn exit_code_127_mentions_missing_node_dependency_and_preserves_tail() {
        let agent = build_agent_with_cmd("a3", "gemini", "gemini", vec![]);
        let message = format_agent_exit_error(
            &agent,
            &portable_pty::ExitStatus::with_exit_code(127),
            0,
            "env: node: No such file or directory",
        );

        assert!(message.contains("`node`"));
        assert!(message.contains("reduced environment"));
        assert!(message.contains("env: node: No such file or directory"));
        assert!(message.contains("Exited with code 127"));
    }

    #[test]
    fn detects_safe_managed_mcp_startup_tool_prompts() {
        let prompt = "Tool use\n\
            ecky_mcp - agent_identity_set(agentLabel: \"claude\", llmModelId: \"claude-opus-4-6\")\n\
            Do you want to proceed?\n\
            1. Yes\n\
            2. Yes, and don't ask again for ecky_mcp - agent_identity_set commands in /private/var/folders/.../ecky-agent-claude\n\
            3. No";

        assert_eq!(
            extract_safe_managed_mcp_tool_prompt(prompt),
            Some("agent_identity_set")
        );
        assert!(
            detect_terminal_attention(AgentTerminalProviderKind::Claude, prompt)
                .is_some_and(|observation| observation.summary.contains("agent_identity_set"))
        );
    }

    #[test]
    fn provider_kind_is_derived_from_agent_command() {
        assert_eq!(
            provider_kind_from_cmd("/opt/homebrew/bin/gemini"),
            AgentTerminalProviderKind::Gemini
        );
        assert_eq!(
            provider_kind_from_cmd("claude"),
            AgentTerminalProviderKind::Claude
        );
        assert_eq!(
            provider_kind_from_cmd("codex"),
            AgentTerminalProviderKind::Codex
        );
        assert_eq!(
            provider_kind_from_cmd("opencode"),
            AgentTerminalProviderKind::Opencode
        );
        assert_eq!(
            provider_kind_from_cmd("amp"),
            AgentTerminalProviderKind::Amp
        );
        assert_eq!(
            provider_kind_from_cmd("/tmp/custom-agent"),
            AgentTerminalProviderKind::Generic
        );
    }

    #[test]
    fn gemini_activity_parsing_strips_cancel_hints_and_preserves_elapsed_time() {
        assert_eq!(
            parse_cancelable_activity_line(
                AgentTerminalProviderKind::Gemini,
                "💬 Developing the next iteration (esc to cancel, 2m 45s)",
            ),
            Some(TerminalActivityObservation {
                label: "Developing the next iteration".to_string(),
                elapsed_secs: Some(165),
            })
        );
    }

    #[test]
    fn gemini_activity_parsing_ignores_footer_noise() {
        assert_eq!(
            extract_sanitized_terminal_fallback(
                AgentTerminalProviderKind::Gemini,
                "shift+tab to accept edits\nPress 'Esc' for NORMAL mode.\n",
            ),
            None
        );
    }

    #[test]
    fn claude_welcome_screen_does_not_become_bubble_activity() {
        let welcome = "Welcome back Bo!\nTips for getting started\nRecent activity\nHow can I help you today?\n";
        let observation = behavior_for_provider(AgentTerminalProviderKind::Claude).observe(welcome);
        assert_eq!(observation.activity, None);
        assert_eq!(observation.sanitized_fallback, None);
    }

    #[test]
    fn generic_provider_uses_sanitized_fallback_when_line_is_not_noise() {
        let observation = behavior_for_provider(AgentTerminalProviderKind::Generic)
            .observe("Running a custom MCP workflow\n");
        assert_eq!(
            observation.sanitized_fallback,
            Some("Running a custom MCP workflow".to_string())
        );
    }

    #[test]
    fn mark_agent_turn_busy_spans_the_whole_turn_without_tui_activity() {
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent("primary", "Primary", vec![])];
        let state = test_state(config);

        initialize_auto_agent_supervisors(state.clone());
        mark_agent_turn_busy(
            &state,
            "Primary",
            Some("session-1".to_string()),
            Some("thread-a".to_string()),
            Some("Gemini 3.1".to_string()),
            Some("Working through the queued message.".to_string()),
        );

        let snapshot = runtime_snapshot_by_session_id(&state, "session-1").expect("runtime");
        assert!(snapshot.busy);
        assert!(snapshot.activity_started_at.is_some());
        assert_eq!(snapshot.activity_label, None);
    }

    #[test]
    fn startup_guidance_prefers_split_target_reads() {
        let agent = build_agent("primary", "claude", vec![]);
        let prompt = build_initial_prompt(&agent, "http://127.0.0.1:39249/mcp");
        let instructions_dir =
            write_agent_instructions(&agent, "http://127.0.0.1:39249/mcp").expect("agents.md");
        let instructions =
            fs::read_to_string(instructions_dir.join("AGENTS.md")).expect("read AGENTS.md");

        assert!(prompt.contains("target_meta_get"));
        assert!(prompt.contains("target_macro_get"));
        assert!(prompt.contains("target_detail_get(section=...)"));
        assert!(prompt.contains("agentBrief.sourceLanguage"));
        assert!(prompt.contains("agentBrief.geometryBackend"));

        assert!(instructions.contains("call `target_meta_get`"));
        assert!(instructions.contains("Use `target_macro_get` for macro reasoning"));
        assert!(instructions.contains("Use `target_get` only as a last-resort full payload."));
        assert!(instructions.contains("agentBrief.sourceLanguage"));
        assert!(instructions.contains("agentBrief.geometryBackend"));
    }

    #[test]
    fn managed_startup_command_detection_matches_ecky_bootstrap_copy() {
        assert!(looks_like_managed_startup_command(
            "claude -- You are an Ecky CAD design assistant. The Ecky MCP server is at http://127.0.0.1:39249/mcp. Start now: call `agent_identity_set` ..."
        ));
        assert!(!looks_like_managed_startup_command(
            "gemini --approval-mode default"
        ));
    }

    #[test]
    fn replacing_agent_terminal_session_changes_session_nonce() {
        let state = test_state(test_config(Some("primary")));
        let agent = build_agent("primary", "Primary", vec![]);
        let (pty1, writer1) = open_test_terminal_handles();
        replace_agent_terminal_session(&state, &agent, pty1, writer1);
        let first_nonce = state
            .agent_terminals
            .lock()
            .unwrap()
            .get("primary")
            .map(|runtime| runtime.snapshot.session_nonce)
            .expect("first terminal");

        let (pty2, writer2) = open_test_terminal_handles();
        replace_agent_terminal_session(&state, &agent, pty2, writer2);
        let second_nonce = state
            .agent_terminals
            .lock()
            .unwrap()
            .get("primary")
            .map(|runtime| runtime.snapshot.session_nonce)
            .expect("second terminal");

        assert_ne!(first_nonce, second_nonce);
    }

    #[tokio::test]
    async fn active_mode_does_not_spawn_on_launch() {
        let touch_file = unique_file("lazy-launch");
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent(
            "primary",
            "Primary",
            vec![
                "-c".to_string(),
                format!("printf primary >> {}; sleep 1", touch_file.display()),
            ],
        )];
        let state = test_state(config);
        state.set_mcp_status(true, None);

        initialize_auto_agent_supervisors(state.clone());
        sleep(Duration::from_millis(250)).await;

        assert!(
            !touch_file.exists(),
            "primary agent should stay dormant until wake"
        );
    }

    #[test]
    fn initialize_auto_agent_supervisors_does_not_require_tokio_runtime_context() {
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent("primary", "Primary", vec![])];
        let state = test_state(config);
        state.set_mcp_status(true, None);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            initialize_auto_agent_supervisors(state.clone());
        }));

        assert!(
            result.is_ok(),
            "startup supervisor initialization should not panic outside Tokio runtime"
        );
    }

    #[tokio::test]
    async fn wake_primary_registers_runtime_on_demand_when_missing() {
        let touch_file = unique_file("lazy-register");
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent(
            "primary",
            "Primary",
            vec![
                "-c".to_string(),
                format!("printf primary >> {}; sleep 1", touch_file.display()),
            ],
        )];
        let state = test_state(config);
        state.set_mcp_status(true, None);

        wake_primary_auto_agent(&state, Some("thread-1".to_string()), None, None)
            .await
            .unwrap();

        let snapshot = primary_runtime_snapshot(&state).expect("primary runtime snapshot");
        assert_eq!(snapshot.phase, AutoAgentRuntimePhase::Waking);

        sleep(Duration::from_millis(350)).await;
        assert_eq!(
            fs::read_to_string(&touch_file).unwrap_or_default(),
            "primary"
        );
    }

    #[tokio::test]
    async fn waking_primary_creates_a_live_terminal_snapshot() {
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent(
            "primary",
            "Primary",
            vec![
                "-c".to_string(),
                "printf 'hello from agent\\n'; sleep 2".to_string(),
            ],
        )];
        let state = test_state(config);
        state.set_mcp_status(true, None);

        initialize_auto_agent_supervisors(state.clone());
        wake_primary_auto_agent(&state, Some("thread-1".to_string()), None, None)
            .await
            .unwrap();
        sleep(Duration::from_millis(600)).await;

        let snapshot = state
            .agent_terminals
            .lock()
            .unwrap()
            .get("primary")
            .map(|runtime| runtime.snapshot.clone())
            .expect("primary terminal snapshot");
        assert!(
            snapshot.active,
            "terminal snapshot should be live during wake"
        );
        assert!(
            snapshot.vt_stream.contains("hello from agent"),
            "expected live VT stream output in snapshot, got: {:?}",
            snapshot.vt_stream
        );
        assert!(
            snapshot.screen_text.contains("hello from agent"),
            "expected degraded readable fallback output in snapshot, got: {:?}",
            snapshot.screen_text
        );
    }

    #[tokio::test]
    async fn wake_primary_rearms_stale_waking_runtime_without_process() {
        let touch_file = unique_file("stale-waking");
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent(
            "primary",
            "Primary",
            vec![
                "-c".to_string(),
                format!("printf primary >> {}; sleep 1", touch_file.display()),
            ],
        )];
        let state = test_state(config);
        state.set_mcp_status(true, None);

        initialize_auto_agent_supervisors(state.clone());
        {
            let mut runtime = runtime_registry(&state);
            runtime.update_by_id("primary", |entry| {
                entry.phase = AutoAgentRuntimePhase::Waking;
                entry.has_spawned = true;
                entry.pid = None;
                entry.session_id = None;
            });
        }

        wake_primary_auto_agent(&state, Some("thread-1".to_string()), None, None)
            .await
            .unwrap();

        for _ in 0..10 {
            if fs::read_to_string(&touch_file).unwrap_or_default() == "primary" {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(
            fs::read_to_string(&touch_file).unwrap_or_default(),
            "primary"
        );
    }

    #[tokio::test]
    async fn waking_primary_spawns_only_primary_and_marks_runtime_waking() {
        let primary_file = unique_file("primary");
        let secondary_file = unique_file("secondary");
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![
            build_agent(
                "primary",
                "Primary",
                vec![
                    "-c".to_string(),
                    format!("printf primary >> {}; sleep 1", primary_file.display()),
                ],
            ),
            build_agent(
                "secondary",
                "Secondary",
                vec![
                    "-c".to_string(),
                    format!("printf secondary >> {}; sleep 1", secondary_file.display()),
                ],
            ),
        ];
        let state = test_state(config);
        state.set_mcp_status(true, None);

        initialize_auto_agent_supervisors(state.clone());
        wake_primary_auto_agent(&state, Some("thread-1".to_string()), None, None)
            .await
            .unwrap();

        let snapshot = primary_runtime_snapshot(&state).expect("primary runtime snapshot");
        assert_eq!(snapshot.phase, AutoAgentRuntimePhase::Waking);
        assert_eq!(snapshot.pending_thread_id.as_deref(), Some("thread-1"));

        sleep(Duration::from_millis(350)).await;

        assert_eq!(
            fs::read_to_string(&primary_file).unwrap_or_default(),
            "primary"
        );
        assert!(
            !secondary_file.exists(),
            "secondary agent must remain dormant when a primary is configured"
        );
    }

    #[tokio::test]
    async fn primary_process_is_not_respawned_while_it_is_still_running() {
        let touch_file = unique_file("single-spawn");
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent(
            "primary",
            "Primary",
            vec![
                "-c".to_string(),
                format!("printf x >> {}; sleep 2", touch_file.display()),
            ],
        )];
        let state = test_state(config);
        state.set_mcp_status(true, None);

        initialize_auto_agent_supervisors(state.clone());
        wake_primary_auto_agent(&state, Some("thread-1".to_string()), None, None)
            .await
            .unwrap();
        for _ in 0..10 {
            if fs::read_to_string(&touch_file).unwrap_or_default() == "x" {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        wake_primary_auto_agent(&state, Some("thread-1".to_string()), None, None)
            .await
            .unwrap();
        sleep(Duration::from_millis(200)).await;

        assert_eq!(fs::read_to_string(&touch_file).unwrap_or_default(), "x");
    }

    #[tokio::test]
    async fn wake_primary_rejects_threads_already_occupied_by_external_sessions() {
        let db_path = unique_file("occupied-thread.sqlite");
        let conn = crate::db::init_db(&db_path).unwrap();
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent("primary", "Primary", vec![])];
        let state = AppState::new(config, None, conn);
        state.set_mcp_status(true, None);

        {
            let conn = state.db.lock().await;
            crate::db::upsert_agent_session(
                &conn,
                &AgentSession {
                    session_id: "external-session".to_string(),
                    client_kind: "http".to_string(),
                    host_label: "Gemini CLI".to_string(),
                    agent_label: "External".to_string(),
                    llm_model_id: None,
                    llm_model_label: None,
                    thread_id: Some("thread-1".to_string()),
                    message_id: Some("message-1".to_string()),
                    model_id: None,
                    phase: "patching_macro".to_string(),
                    status_text: "Editing".to_string(),
                    updated_at: now_secs(),
                },
            )
            .unwrap();
        }

        initialize_auto_agent_supervisors(state.clone());
        let err = wake_primary_auto_agent(&state, Some("thread-1".to_string()), None, None)
            .await
            .expect_err("thread should be marked occupied");
        assert!(
            err.message.contains("occupied by external agent"),
            "unexpected error: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn sync_auto_agent_supervisors_clears_stale_managed_sessions_but_keeps_passive_rows() {
        let db_path = unique_file("stale-managed-sessions.sqlite");
        let conn = crate::db::init_db(&db_path).unwrap();
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent("primary", "Primary", vec![])];
        let state = AppState::new(config, None, conn);
        state.set_mcp_status(true, None);

        {
            let conn = state.db.lock().await;
            crate::db::upsert_agent_session(
                &conn,
                &AgentSession {
                    session_id: "managed-stale".to_string(),
                    client_kind: "managed-mcp-http".to_string(),
                    host_label: "Primary".to_string(),
                    agent_label: "Primary".to_string(),
                    llm_model_id: None,
                    llm_model_label: None,
                    thread_id: Some("thread-managed".to_string()),
                    message_id: Some("message-managed".to_string()),
                    model_id: None,
                    phase: "active".to_string(),
                    status_text: "Working".to_string(),
                    updated_at: now_secs(),
                },
            )
            .unwrap();
            crate::db::upsert_agent_session(
                &conn,
                &AgentSession {
                    session_id: "passive-still-here".to_string(),
                    client_kind: "mcp-http".to_string(),
                    host_label: "Gemini CLI".to_string(),
                    agent_label: "gemini".to_string(),
                    llm_model_id: None,
                    llm_model_label: None,
                    thread_id: Some("thread-passive".to_string()),
                    message_id: Some("message-passive".to_string()),
                    model_id: None,
                    phase: "active".to_string(),
                    status_text: "Working".to_string(),
                    updated_at: now_secs(),
                },
            )
            .unwrap();
        }

        sync_auto_agent_supervisors(state.clone());

        let conn = state.db.lock().await;
        assert!(
            crate::db::get_sessions_by_ids(&conn, &[String::from("managed-stale")])
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            crate::db::get_sessions_by_ids(&conn, &[String::from("passive-still-here")])
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn thread_agent_state_is_thread_specific_for_waiting_runtime() {
        let mut config = test_config(Some("primary"));
        config.mcp.auto_agents = vec![build_agent("primary", "Primary", vec![])];

        let runtime = AutoAgentRuntimeSnapshot {
            agent_id: "primary".to_string(),
            agent_label: "Primary".to_string(),
            provider_kind: "claude".to_string(),
            phase: AutoAgentRuntimePhase::Waiting,
            has_spawned: true,
            pid: Some(123),
            pending_thread_id: Some("thread-a".to_string()),
            pending_message_id: None,
            pending_model_id: None,
            session_id: Some("session-1".to_string()),
            llm_model_label: None,
            busy: false,
            activity_label: None,
            activity_started_at: None,
            attention_kind: None,
            waiting_on_prompt: true,
            status_text: Some("Waiting for your next queued message.".to_string()),
            last_error: None,
            updated_at: now_secs(),
        };

        let current = derive_thread_agent_state(
            &config,
            "thread-a",
            ThreadAgentStateInputs {
                runtime: Some(runtime.clone()),
                live_session_id: None,
                live_session: None,
                last_session: None,
                now: now_secs(),
            },
        );
        assert_eq!(current.connection_state, "waiting");

        let unrelated = derive_thread_agent_state(
            &config,
            "thread-b",
            ThreadAgentStateInputs {
                runtime: Some(runtime),
                live_session_id: None,
                live_session: None,
                last_session: None,
                now: now_secs(),
            },
        );
        assert_eq!(unrelated.connection_state, "none");
    }

    #[test]
    fn thread_agent_state_uses_live_session_activity_for_passive_mcp() {
        let config = test_config(None);
        let live_session = McpSessionState {
            client_kind: "mcp-http".to_string(),
            host_label: "Gemini CLI".to_string(),
            agent_label: "gemini".to_string(),
            llm_model_id: None,
            llm_model_label: Some("Gemini 3.1 Pro".to_string()),
            bound_thread_id: None,
            last_target: Some(McpTargetRef {
                thread_id: "thread-a".to_string(),
                message_id: "msg-a".to_string(),
                model_id: Some("model-a".to_string()),
            }),
            phase: Some("working".to_string()),
            status_text: Some("Extruding connector cutouts.".to_string()),
            busy: true,
            activity_label: Some("Extruding connector cutouts".to_string()),
            activity_started_at: Some(123),
            attention_kind: None,
            waiting_on_prompt: false,
            current_turn_id: None,
            current_turn_thread_id: None,
            current_turn_working_message_ids: Vec::new(),
            current_turn_working_version_message_id: None,
            updated_at: 456,
        };

        let current = derive_thread_agent_state(
            &config,
            "thread-a",
            ThreadAgentStateInputs {
                runtime: None,
                live_session_id: Some("session-a".to_string()),
                live_session: Some(live_session),
                last_session: None,
                now: now_secs(),
            },
        );

        assert_eq!(current.connection_state, "active");
        assert_eq!(current.provider_kind.as_deref(), Some("gemini"));
        assert_eq!(
            current.activity_label.as_deref(),
            Some("Extruding connector cutouts")
        );
        assert!(current.busy);
    }

    #[test]
    fn thread_agent_state_marks_persisted_only_sessions_as_disconnected() {
        let config = test_config(None);
        let current = derive_thread_agent_state(
            &config,
            "thread-a",
            ThreadAgentStateInputs {
                runtime: None,
                live_session_id: None,
                live_session: None,
                last_session: Some(AgentSession {
                    session_id: "session-a".to_string(),
                    client_kind: "mcp-http".to_string(),
                    host_label: "Gemini CLI".to_string(),
                    agent_label: "gemini".to_string(),
                    llm_model_id: None,
                    llm_model_label: Some("Gemini 3.1 Pro".to_string()),
                    thread_id: Some("thread-a".to_string()),
                    message_id: Some("msg-a".to_string()),
                    model_id: Some("model-a".to_string()),
                    phase: "idle".to_string(),
                    status_text: "Agent disconnected.".to_string(),
                    updated_at: now_secs(),
                }),
                now: now_secs(),
            },
        );

        assert_eq!(current.connection_state, "disconnected");
        assert!(!current.busy);
    }
}
