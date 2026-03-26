use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

pub use crate::contracts::*;

pub trait PathResolver: Send + Sync {
    fn app_config_dir(&self) -> PathBuf;
    fn app_data_dir(&self) -> PathBuf;
    fn resource_path(&self, path: &str) -> Option<PathBuf>;
}

fn env_path_override(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

impl PathResolver for tauri::AppHandle {
    fn app_config_dir(&self) -> PathBuf {
        env_path_override("ECKY_APP_CONFIG_DIR")
            .unwrap_or_else(|| self.path().app_config_dir().unwrap())
    }
    fn app_data_dir(&self) -> PathBuf {
        env_path_override("ECKY_APP_DATA_DIR")
            .or_else(|| env_path_override("ECKY_APP_CONFIG_DIR"))
            .unwrap_or_else(|| self.path().app_data_dir().unwrap())
    }
    fn resource_path(&self, path: &str) -> Option<PathBuf> {
        self.path()
            .resolve(path, tauri::path::BaseDirectory::Resource)
            .ok()
    }
}

impl<T: PathResolver + ?Sized> PathResolver for std::sync::Arc<T> {
    fn app_config_dir(&self) -> PathBuf {
        (**self).app_config_dir()
    }
    fn app_data_dir(&self) -> PathBuf {
        (**self).app_data_dir()
    }
    fn resource_path(&self, path: &str) -> Option<PathBuf> {
        (**self).resource_path(path)
    }
}

#[derive(Debug, Clone)]
pub struct McpTargetRef {
    pub thread_id: String,
    pub message_id: String,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct McpSessionState {
    pub client_kind: String,
    pub host_label: String,
    pub agent_label: String,
    pub llm_model_id: Option<String>,
    pub llm_model_label: Option<String>,
    pub bound_thread_id: Option<String>,
    pub last_target: Option<McpTargetRef>,
    pub phase: Option<String>,
    pub status_text: Option<String>,
    pub busy: bool,
    pub activity_label: Option<String>,
    pub activity_started_at: Option<u64>,
    pub attention_kind: Option<String>,
    pub waiting_on_prompt: bool,
    pub current_turn_id: Option<String>,
    pub current_turn_thread_id: Option<String>,
    pub current_turn_working_message_ids: Vec<String>,
    pub current_turn_working_version_message_id: Option<String>,
    pub updated_at: u64,
}

impl McpSessionState {
    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn new(client_kind: String, host_label: String) -> Self {
        Self {
            client_kind,
            agent_label: host_label.clone(),
            host_label,
            llm_model_id: None,
            llm_model_label: None,
            bound_thread_id: None,
            last_target: None,
            phase: None,
            status_text: None,
            busy: false,
            activity_label: None,
            activity_started_at: None,
            attention_kind: None,
            waiting_on_prompt: false,
            current_turn_id: None,
            current_turn_thread_id: None,
            current_turn_working_message_ids: Vec::new(),
            current_turn_working_version_message_id: None,
            updated_at: Self::now_secs(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptResumeState {
    pub pgid: Option<i32>,
    pub agent_label: String,
    pub session_id: String,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ViewportScreenshotCapture {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub camera: ViewportCameraState,
    pub source: String,
    pub thread_id: String,
    pub message_id: String,
    pub model_id: Option<String>,
    pub include_overlays: bool,
}

pub type ViewportScreenshotSender = oneshot::Sender<Result<ViewportScreenshotCapture, String>>;
pub type PendingViewportScreenshotChannels =
    Arc<tokio::sync::Mutex<HashMap<String, ViewportScreenshotSender>>>;
pub type AgentTerminalWriter = Arc<Mutex<Box<dyn Write + Send>>>;
pub type AgentTerminalPty = Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>;

pub struct AgentTerminalRuntime {
    pub snapshot: AgentTerminalSnapshot,
    pub writer: AgentTerminalWriter,
    pub pty: AgentTerminalPty,
    pub pending_utf8: Vec<u8>,
    pub pending_escape: String,
    pub last_emitted_at: Option<Instant>,
}

pub type PendingAgentTerminalSessions = Arc<Mutex<HashMap<String, AgentTerminalRuntime>>>;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub last_snapshot: Arc<Mutex<Option<LastDesignSnapshot>>>,
    pub db: Arc<tokio::sync::Mutex<rusqlite::Connection>>,
    pub render_lock: Arc<tokio::sync::Mutex<()>>,
    pub mcp_status: Arc<Mutex<McpServerStatus>>,
    pub mcp_sessions: Arc<tokio::sync::Mutex<HashMap<String, McpSessionState>>>,
    /// Pending user-confirmation requests keyed by requestId.
    pub confirm_channels: Arc<tokio::sync::Mutex<HashMap<String, oneshot::Sender<String>>>>,
    /// Pending user-prompt requests keyed by requestId (agent waits for text/attachments from UI).
    pub prompt_channels:
        Arc<tokio::sync::Mutex<HashMap<String, oneshot::Sender<ResolveAgentPromptInput>>>>,
    /// Runtime state machine for active-mode MCP agents.
    pub auto_agent_runtime: Arc<Mutex<crate::mcp::runtime::AutoAgentRuntimeRegistry>>,
    /// Maps prompt request_id → process control for agents SIGSTOP'd while waiting on the user.
    pub prompt_waits: Arc<Mutex<HashMap<String, PromptResumeState>>>,
    /// Pending viewport screenshot requests keyed by requestId.
    pub viewport_screenshot_channels: PendingViewportScreenshotChannels,
    /// Ring buffer of in-app log entries (latest 200 entries).
    pub app_logs: Arc<Mutex<VecDeque<AppLogEntry>>>,
    /// Active PTY-backed terminal bridges for interactive auto-agents.
    pub agent_terminals: PendingAgentTerminalSessions,
    /// App handle for emitting runtime PTY events back into the frontend.
    pub app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
}

impl AppState {
    pub fn new(
        config: Config,
        last_snapshot: Option<LastDesignSnapshot>,
        conn: rusqlite::Connection,
    ) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            last_snapshot: Arc::new(Mutex::new(last_snapshot)),
            db: Arc::new(tokio::sync::Mutex::new(conn)),
            render_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_status: Arc::new(Mutex::new(McpServerStatus {
                running: false,
                endpoint_url: "http://127.0.0.1:39249/mcp".to_string(),
                last_startup_error: None,
            })),
            mcp_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            confirm_channels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            prompt_channels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            auto_agent_runtime: Arc::new(Mutex::new(
                crate::mcp::runtime::AutoAgentRuntimeRegistry::default(),
            )),
            prompt_waits: Arc::new(Mutex::new(HashMap::new())),
            viewport_screenshot_channels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            app_logs: Arc::new(Mutex::new(VecDeque::new())),
            agent_terminals: Arc::new(Mutex::new(HashMap::new())),
            app_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.lock().unwrap() = Some(handle);
    }

    pub fn push_log(&self, message: String) {
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let entry = AppLogEntry { ts_ms, message };
        let mut logs = self.app_logs.lock().unwrap();
        if logs.len() >= 200 {
            logs.pop_front();
        }
        logs.push_back(entry);
    }

    pub fn set_mcp_status(&self, running: bool, last_startup_error: Option<String>) {
        let mut status = self.mcp_status.lock().unwrap();
        status.running = running;
        status.last_startup_error = last_startup_error;
    }

    pub fn mcp_status(&self) -> McpServerStatus {
        self.mcp_status.lock().unwrap().clone()
    }

    pub fn emit_agent_terminal_update(&self, snapshot: &AgentTerminalSnapshot) {
        let handle = self.app_handle.lock().unwrap().clone();
        if let Some(handle) = handle {
            let _ = handle.emit("agent-terminal-updated", snapshot);
        }
    }

    pub fn emit_agent_working_version_created(&self, event: &AgentWorkingVersionEvent) {
        let handle = self.app_handle.lock().unwrap().clone();
        if let Some(handle) = handle {
            let _ = handle.emit("agent-working-version-created", event);
        }
    }

    pub fn emit_history_updated(&self) {
        let handle = self.app_handle.lock().unwrap().clone();
        if let Some(handle) = handle {
            let _ = handle.emit("history-updated", ());
        }
    }
}
