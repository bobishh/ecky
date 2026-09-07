use crate::contracts::{
    AgentActivityCatchUp, AgentActivityEvent, AgentActivityEventInput, AgentTerminalSnapshot,
    AgentWorkingVersionEvent, AppError, AppLogEntry, AppResult, CaptureClientCapabilities,
    CaptureFrameManifestEntry, CaptureSessionInfo, CaptureSessionState, Config, LastDesignSnapshot,
    McpServerStatus, ResolveAgentPromptInput, ViewportCameraState,
};
#[cfg(unix)]
use libc;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

/// Pending user-prompt requests keyed by requestId.
type PromptChannels = Arc<
    tokio::sync::Mutex<HashMap<String, oneshot::Sender<Result<ResolveAgentPromptInput, String>>>>,
>;

static HISTORY_REVISION: AtomicU64 = AtomicU64::new(0);

pub fn next_history_changed_event(
    thread_id: Option<String>,
    message_id: Option<String>,
    kind: impl Into<String>,
) -> crate::contracts::HistoryChangedEvent {
    let event = crate::contracts::HistoryChangedEvent {
        thread_id,
        message_id,
        revision: HISTORY_REVISION.fetch_add(1, Ordering::Relaxed) + 1,
        kind: kind.into(),
    };
    debug_assert!(
        crate::transport_budget::serialized_size(&event)
            .is_ok_and(|bytes| bytes <= crate::transport_budget::ACTIVITY_EVENT_MAX_BYTES),
        "history invalidation event must remain payload-free and under the event budget",
    );
    event
}

pub trait PathResolver: Send + Sync {
    fn app_config_dir(&self) -> PathBuf;
    fn try_app_config_dir(&self) -> AppResult<PathBuf> {
        Ok(self.app_config_dir())
    }
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
    fn try_app_config_dir(&self) -> AppResult<PathBuf> {
        if let Some(path) = env_path_override("ECKY_APP_CONFIG_DIR") {
            return Ok(path);
        }
        self.path()
            .app_config_dir()
            .map_err(|_| AppError::persistence("config path resolution failed: app-config-dir"))
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
    fn try_app_config_dir(&self) -> AppResult<PathBuf> {
        (**self).try_app_config_dir()
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

/// All ephemeral state owned by one MCP session.  Keep lifecycle cleanup here:
/// disconnect must remove every per-session projection together.
#[derive(Clone)]
pub struct McpSessionRegistry {
    sessions: Arc<tokio::sync::Mutex<HashMap<String, McpSessionState>>>,
    read_resources: Arc<tokio::sync::Mutex<HashMap<String, HashSet<String>>>>,
    enabled_groups: Arc<tokio::sync::Mutex<HashMap<String, HashSet<String>>>>,
    pending_notifications: Arc<tokio::sync::Mutex<HashMap<String, Vec<serde_json::Value>>>>,
}

impl McpSessionRegistry {
    pub fn with_sessions(&self) -> &Arc<tokio::sync::Mutex<HashMap<String, McpSessionState>>> {
        &self.sessions
    }
    pub async fn insert(&self, session_id: String, session: McpSessionState) {
        self.sessions.lock().await.insert(session_id, session);
    }

    pub async fn get(&self, session_id: &str) -> Option<McpSessionState> {
        self.sessions.lock().await.get(session_id).cloned()
    }

    pub async fn update<F>(&self, session_id: &str, mutate: F) -> bool
    where
        F: FnOnce(&mut McpSessionState),
    {
        let mut sessions = self.sessions.lock().await;
        let Some(session) = sessions.get_mut(session_id) else {
            return false;
        };
        mutate(session);
        true
    }

    pub async fn contains(&self, session_id: &str) -> bool {
        self.sessions.lock().await.contains_key(session_id)
    }

    pub async fn list(&self) -> Vec<(String, McpSessionState)> {
        self.sessions
            .lock()
            .await
            .iter()
            .map(|(id, session)| (id.clone(), session.clone()))
            .collect()
    }

    pub async fn mark_resource_read(&self, session_id: &str, uri: String) {
        self.read_resources
            .lock()
            .await
            .entry(session_id.to_string())
            .or_default()
            .insert(uri);
    }

    pub async fn read_resources(&self, session_id: &str) -> HashSet<String> {
        self.read_resources
            .lock()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn enabled_groups(&self, session_id: &str) -> HashSet<String> {
        self.enabled_groups
            .lock()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn enable_group(&self, session_id: &str, group: &str) -> bool {
        let mut groups = self.enabled_groups.lock().await;
        groups
            .entry(session_id.to_string())
            .or_default()
            .insert(group.to_string())
    }

    pub async fn enqueue_notification(&self, session_id: &str, notification: serde_json::Value) {
        self.pending_notifications
            .lock()
            .await
            .entry(session_id.to_string())
            .or_default()
            .push(notification);
    }

    pub async fn drain_notifications(&self, session_id: &str) -> Vec<serde_json::Value> {
        self.pending_notifications
            .lock()
            .await
            .remove(session_id)
            .unwrap_or_default()
    }

    pub async fn remove(&self, session_id: &str) -> Option<McpSessionState> {
        let removed = self.sessions.lock().await.remove(session_id);
        self.read_resources.lock().await.remove(session_id);
        self.enabled_groups.lock().await.remove(session_id);
        self.pending_notifications.lock().await.remove(session_id);
        removed
    }
}

#[cfg(test)]
mod mcp_session_registry_tests {
    use super::*;

    #[tokio::test]
    async fn registry_crud_uses_owned_map() {
        let registry = McpSessionRegistry {
            sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            read_resources: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            enabled_groups: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            pending_notifications: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        };
        registry
            .insert(
                "s1".to_string(),
                McpSessionState::new("client".into(), "host".into()),
            )
            .await;
        assert!(registry.contains("s1").await);
        assert_eq!(registry.get("s1").await.unwrap().client_kind, "client");
        assert!(
            registry
                .update("s1", |session| session.phase = Some("active".into()))
                .await
        );
        assert_eq!(
            registry.get("s1").await.unwrap().phase.as_deref(),
            Some("active")
        );
        assert_eq!(registry.list().await.len(), 1);
        assert!(registry.remove("s1").await.is_some());
        assert!(!registry.contains("s1").await);
    }

    #[tokio::test]
    async fn remove_clears_all_session_state_and_new_session_starts_empty() {
        let registry = McpSessionRegistry {
            sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            read_resources: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            enabled_groups: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            pending_notifications: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        };
        let session_id = "session-1";
        registry.sessions.lock().await.insert(
            session_id.to_string(),
            McpSessionState::new("test".to_string(), "host".to_string()),
        );
        registry.read_resources.lock().await.insert(
            session_id.to_string(),
            HashSet::from(["guide://one".to_string()]),
        );
        registry.enabled_groups.lock().await.insert(
            session_id.to_string(),
            HashSet::from(["target-reads".to_string()]),
        );
        registry.pending_notifications.lock().await.insert(
            session_id.to_string(),
            vec![serde_json::json!({"method": "notifications/tools/list_changed"})],
        );

        assert!(registry.remove(session_id).await.is_some());
        assert!(!registry.sessions.lock().await.contains_key(session_id));
        assert!(!registry
            .read_resources
            .lock()
            .await
            .contains_key(session_id));
        assert!(!registry
            .enabled_groups
            .lock()
            .await
            .contains_key(session_id));
        assert!(!registry
            .pending_notifications
            .lock()
            .await
            .contains_key(session_id));

        registry.sessions.lock().await.insert(
            session_id.to_string(),
            McpSessionState::new("new".to_string(), "new-host".to_string()),
        );
        assert!(!registry
            .read_resources
            .lock()
            .await
            .contains_key(session_id));
        assert!(!registry
            .enabled_groups
            .lock()
            .await
            .contains_key(session_id));
        assert!(!registry
            .pending_notifications
            .lock()
            .await
            .contains_key(session_id));
    }
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

#[derive(Debug, Clone)]
pub struct CaptureSessionRecord {
    pub info: CaptureSessionInfo,
    pub frames: Vec<CaptureFrameManifestEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigPersistenceStatus {
    pub cleanup_pending: bool,
    pub warnings: Vec<String>,
}

pub struct GeometryRenderGuard {
    _lock: tokio::sync::OwnedMutexGuard<()>,
    _activity: Option<GeometryRenderActivityGuard>,
}

struct GeometryRenderActivityGuard {
    active_count: Arc<AtomicUsize>,
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
}

fn emit_geometry_render_activity(
    app_handle: &Arc<Mutex<Option<tauri::AppHandle>>>,
    active_count: usize,
) {
    let handle = app_handle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(handle) = handle {
        let _ = handle.emit(
            "geometry-render-activity",
            serde_json::json!({ "activeCount": active_count }),
        );
    }
}

impl GeometryRenderActivityGuard {
    fn activate(
        active_count: Arc<AtomicUsize>,
        app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    ) -> Self {
        let next_active_count = active_count.fetch_add(1, Ordering::AcqRel) + 1;
        emit_geometry_render_activity(&app_handle, next_active_count);
        Self {
            active_count,
            app_handle,
        }
    }
}

impl Drop for GeometryRenderActivityGuard {
    fn drop(&mut self) {
        let previous = self.active_count.fetch_sub(1, Ordering::AcqRel);
        let active_count = previous.saturating_sub(1);
        emit_geometry_render_activity(&self.app_handle, active_count);
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub config_persistence_status: Arc<Mutex<ConfigPersistenceStatus>>,
    pub last_snapshot: Arc<Mutex<Option<LastDesignSnapshot>>>,
    pub db: Arc<tokio::sync::Mutex<rusqlite::Connection>>,
    pub db_read: Option<Arc<tokio::sync::Mutex<rusqlite::Connection>>>,
    pub render_lock: Arc<tokio::sync::Mutex<()>>,
    geometry_render_active_count: Arc<AtomicUsize>,
    /// Active project-folder renders keyed by project slug. Snapshot source
    /// for clients attaching after the watcher's one-shot detected event.
    pub project_folder_render_activity: Arc<
        tokio::sync::Mutex<BTreeMap<String, crate::project_mirror::ProjectFolderRenderActivity>>,
    >,
    pub mcp_status: Arc<Mutex<McpServerStatus>>,
    pub codex_app_server: Arc<crate::services::codex_app_server::CodexAppServerSupervisor>,
    pub agy_provider: Arc<crate::services::agy_provider::AgyProviderSupervisor>,
    pub mcp_session_registry: McpSessionRegistry,
    /// Pending user-confirmation requests keyed by requestId.
    pub confirm_channels: Arc<tokio::sync::Mutex<HashMap<String, oneshot::Sender<String>>>>,
    /// Pending user-prompt requests keyed by requestId (agent waits for text/attachments from UI).
    pub prompt_channels: PromptChannels,
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
    /// Capture sessions currently available to local capture clients.
    pub capture_sessions: Arc<tokio::sync::Mutex<HashMap<String, CaptureSessionRecord>>>,
    /// Running capture reconstructions, keyed by hashed pairing token.
    pub capture_reconstructions: Arc<
        tokio::sync::Mutex<
            HashMap<String, crate::capture_reconstruction::ReconstructionCancellation>,
        >,
    >,
    /// Explicit FEM job cancellation flags. Jobs exist only after an explicit
    /// validate/mesh/run request; ordinary geometry preview never inserts one.
    pub fem_cancellations: Arc<tokio::sync::Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// Public base URL of the dedicated LAN capture listener.
    pub capture_server_url: Arc<Mutex<Option<String>>>,
    /// HTTP bootstrap URL for installing Ecky's persistent local capture CA.
    pub capture_trust_url: Arc<Mutex<Option<String>>>,
    /// AppState-scoped authoring actor registry. Each `AppState` owns its own
    /// registry so independent instances never share or invalidate each
    /// other's authoring actor state (even with identical `session_id`/
    /// `thread_id` strings), while a UI mutation still invalidates every
    /// session's actor for that thread within this `AppState`.
    pub authoring_actor_registry: Arc<crate::mcp::handlers::AuthoringActorRegistry>,
    /// Sole admission/cancellation authority for API and controller builds.
    pub exploration_run_registry: Arc<crate::exploration_run_registry::ExplorationRunRegistry>,
    /// Latest-wins admission authority for sketch preview submissions.
    pub sketch_preview_run_registry:
        Arc<crate::services::sketch_preview_submission::SketchPreviewRunRegistry>,
    /// App-global journal of typed agent activity events.
    pub agent_activity: Arc<Mutex<crate::services::agent_activity::AgentActivityJournal>>,
    /// App handle for emitting runtime PTY events back into the frontend.
    pub app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
}

impl AppState {
    /// Threads currently selected by an authoring surface.
    ///
    /// UI selection and live MCP ownership are peer projections. Neither
    /// requires a rendered version, so blank bound threads remain eligible for
    /// their first watcher append.
    pub async fn active_authoring_thread_ids(&self) -> HashSet<String> {
        let ui_thread_id = self
            .last_snapshot
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|snapshot| snapshot.thread_id.clone());
        let sessions = self.mcp_session_registry.with_sessions().lock().await;
        ui_thread_id
            .into_iter()
            .chain(sessions.values().flat_map(|session| {
                [
                    session.bound_thread_id.as_ref(),
                    session.current_turn_thread_id.as_ref(),
                ]
                .into_iter()
                .flatten()
                .cloned()
            }))
            .collect()
    }

    pub fn new(
        config: Config,
        last_snapshot: Option<LastDesignSnapshot>,
        conn: rusqlite::Connection,
    ) -> Self {
        Self::new_with_read_connection(config, last_snapshot, conn, None)
    }

    pub fn new_with_read_connection(
        config: Config,
        last_snapshot: Option<LastDesignSnapshot>,
        conn: rusqlite::Connection,
        read_conn: Option<rusqlite::Connection>,
    ) -> Self {
        let mcp_session_map = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let mcp_session_read_resources = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let mcp_session_enabled_groups = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let mcp_session_pending_notifications = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        Self {
            config: Arc::new(Mutex::new(config)),
            config_persistence_status: Arc::new(Mutex::new(ConfigPersistenceStatus::default())),
            last_snapshot: Arc::new(Mutex::new(last_snapshot)),
            db: Arc::new(tokio::sync::Mutex::new(conn)),
            db_read: read_conn.map(|conn| Arc::new(tokio::sync::Mutex::new(conn))),
            render_lock: Arc::new(tokio::sync::Mutex::new(())),
            geometry_render_active_count: Arc::new(AtomicUsize::new(0)),
            project_folder_render_activity: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
            mcp_status: Arc::new(Mutex::new(McpServerStatus {
                running: false,
                endpoint_url: "http://127.0.0.1:39249/mcp".to_string(),
                last_startup_error: None,
            })),
            codex_app_server: Arc::new(
                crate::services::codex_app_server::CodexAppServerSupervisor::new(),
            ),
            agy_provider: Arc::new(crate::services::agy_provider::AgyProviderSupervisor::new()),
            mcp_session_registry: McpSessionRegistry {
                sessions: mcp_session_map,
                read_resources: Arc::clone(&mcp_session_read_resources),
                enabled_groups: Arc::clone(&mcp_session_enabled_groups),
                pending_notifications: Arc::clone(&mcp_session_pending_notifications),
            },
            confirm_channels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            prompt_channels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            auto_agent_runtime: Arc::new(Mutex::new(
                crate::mcp::runtime::AutoAgentRuntimeRegistry::default(),
            )),
            prompt_waits: Arc::new(Mutex::new(HashMap::new())),
            viewport_screenshot_channels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            app_logs: Arc::new(Mutex::new(VecDeque::new())),
            agent_terminals: Arc::new(Mutex::new(HashMap::new())),
            capture_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            capture_reconstructions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            fem_cancellations: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            capture_server_url: Arc::new(Mutex::new(None)),
            capture_trust_url: Arc::new(Mutex::new(None)),
            authoring_actor_registry: Arc::new(
                crate::mcp::handlers::AuthoringActorRegistry::default(),
            ),
            exploration_run_registry: Arc::new(
                crate::exploration_run_registry::ExplorationRunRegistry::default(),
            ),
            sketch_preview_run_registry: Arc::new(
                crate::services::sketch_preview_submission::SketchPreviewRunRegistry::default(),
            ),
            agent_activity: Arc::new(Mutex::new(
                crate::services::agent_activity::AgentActivityJournal::default(),
            )),
            app_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn geometry_render_active_count(&self) -> usize {
        self.geometry_render_active_count.load(Ordering::Acquire)
    }

    pub async fn acquire_geometry_render(&self) -> GeometryRenderGuard {
        let activity = GeometryRenderActivityGuard::activate(
            self.geometry_render_active_count.clone(),
            self.app_handle.clone(),
        );
        let lock = self.render_lock.clone().lock_owned().await;
        GeometryRenderGuard {
            _lock: lock,
            _activity: Some(activity),
        }
    }

    pub async fn acquire_geometry_render_silent(&self) -> GeometryRenderGuard {
        let lock = self.render_lock.clone().lock_owned().await;
        GeometryRenderGuard {
            _lock: lock,
            _activity: None,
        }
    }

    fn make_capture_token() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn capture_token_hash(token: &str) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(token.as_bytes()))
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    async fn cleanup_capture_sessions(&self) {
        let now = Self::now_secs();
        let mut sessions = self.capture_sessions.lock().await;
        sessions.retain(|_, session| session.info.expires_at > now);
    }

    pub async fn start_capture_session(
        &self,
        ttl_seconds: u64,
        target_thread_id: String,
        target_message_id: Option<String>,
    ) -> AppResult<CaptureSessionInfo> {
        if target_thread_id.trim().is_empty() {
            return Err(AppError::validation(
                "Capture target thread id is required.",
            ));
        }
        self.cleanup_capture_sessions().await;
        let now = Self::now_secs();
        let session_id = Self::make_capture_token();
        let pairing_token = Self::make_capture_token();
        let expires_at = now.saturating_add(ttl_seconds);
        let pairing_url = self
            .capture_server_url
            .lock()
            .unwrap()
            .as_ref()
            .map(|base| format!("{base}/capture/{pairing_token}"))
            .ok_or_else(|| AppError::internal("LAN capture service is not ready."))?;
        let trust_url = self
            .capture_trust_url
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AppError::internal("LAN capture trust service is not ready."))?;
        let session = CaptureSessionInfo {
            session_id: session_id.clone(),
            target_thread_id,
            target_message_id,
            target_title: String::new(),
            target_source: String::new(),
            target_source_language: String::new(),
            started_from_empty: false,
            pairing_token: pairing_token.clone(),
            pairing_url,
            trust_url,
            protocol_version: 1,
            client_capabilities: CaptureClientCapabilities::default(),
            state: CaptureSessionState::Pairing,
            created_at: now,
            expires_at,
            accepted_frame_count: 0,
            coverage_percent: 0,
            guidance: "Pair phone".into(),
            raw_error: None,
            reconstruction_progress: None,
            mesh_preview: None,
        };
        self.capture_sessions.lock().await.insert(
            Self::capture_token_hash(&session.pairing_token),
            CaptureSessionRecord {
                info: session.clone(),
                frames: Vec::new(),
            },
        );
        Ok(session)
    }

    pub async fn reopen_capture_session(
        &self,
        run: &crate::contracts::CaptureRun,
        frames: Vec<CaptureFrameManifestEntry>,
        ttl_seconds: u64,
    ) -> AppResult<CaptureSessionInfo> {
        let now = Self::now_secs();
        let pairing_token = Self::make_capture_token();
        let expires_at = now.saturating_add(ttl_seconds);
        let pairing_url = self
            .capture_server_url
            .lock()
            .unwrap()
            .as_ref()
            .map(|base| format!("{base}/capture/{pairing_token}"))
            .ok_or_else(|| AppError::internal("LAN capture service is not ready."))?;
        let trust_url = self
            .capture_trust_url
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AppError::internal("LAN capture trust service is not ready."))?;
        let state = if run.mesh_preview.is_some() {
            CaptureSessionState::Preview
        } else if run.state == CaptureSessionState::Failed {
            CaptureSessionState::Failed
        } else {
            CaptureSessionState::Capturing
        };
        let session = CaptureSessionInfo {
            session_id: run.id.clone(),
            target_thread_id: run.target_thread_id.clone(),
            target_message_id: run.target_message_id.clone(),
            target_title: run.title.clone(),
            target_source: run.target_source.clone(),
            target_source_language: run.target_source_language.clone(),
            started_from_empty: run.started_from_empty,
            pairing_token: pairing_token.clone(),
            pairing_url,
            trust_url,
            protocol_version: 1,
            client_capabilities: CaptureClientCapabilities::default(),
            state,
            created_at: run.created_at,
            expires_at,
            accepted_frame_count: frames.len().max(run.accepted_frame_count as usize) as u32,
            coverage_percent: 0,
            guidance: "Reopened durable capture".into(),
            raw_error: run.raw_error.clone(),
            reconstruction_progress: run.mesh_preview.as_ref().map(|_| 1.0),
            mesh_preview: run.mesh_preview.clone(),
        };
        self.capture_sessions.lock().await.insert(
            Self::capture_token_hash(&pairing_token),
            CaptureSessionRecord {
                info: session.clone(),
                frames,
            },
        );
        Ok(session)
    }

    async fn persist_capture_session(&self, session: &CaptureSessionInfo) -> AppResult<()> {
        let db = self.db.lock().await;
        crate::capture_runs::update_from_session(&db, session)
            .map_err(|error| AppError::persistence(error.to_string()))
    }

    pub async fn get_capture_session(&self, token: &str) -> Option<CaptureSessionInfo> {
        self.cleanup_capture_sessions().await;
        self.capture_sessions
            .lock()
            .await
            .get(&Self::capture_token_hash(token))
            .map(|record| record.info.clone())
    }

    pub async fn set_capture_session_state(
        &self,
        token: &str,
        state: CaptureSessionState,
    ) -> AppResult<CaptureSessionInfo> {
        self.cleanup_capture_sessions().await;
        let info = {
            let mut sessions = self.capture_sessions.lock().await;
            let record = sessions
                .get_mut(&Self::capture_token_hash(token))
                .ok_or_else(|| {
                    AppError::not_found(format!("Capture session `{}` not found.", token))
                })?;
            record.info.state = state;
            record.info.clone()
        };
        self.persist_capture_session(&info).await?;
        Ok(info)
    }

    pub async fn resume_capture_session(&self, token: &str) -> AppResult<CaptureSessionInfo> {
        self.cleanup_capture_sessions().await;
        let info = {
            let mut sessions = self.capture_sessions.lock().await;
            let record = sessions
                .get_mut(&Self::capture_token_hash(token))
                .ok_or_else(|| AppError::not_found("Capture session not found or expired."))?;
            if !matches!(
                record.info.state,
                CaptureSessionState::Preview | CaptureSessionState::Failed
            ) {
                return Err(AppError::conflict(
                    "Capture can add photos only after preview or failure.",
                ));
            }
            record.info.state = CaptureSessionState::Capturing;
            record.info.raw_error = None;
            record.info.reconstruction_progress = None;
            record.info.mesh_preview = None;
            record.info.guidance = "Add photos, then reconstruct again".into();
            record.info.clone()
        };
        self.persist_capture_session(&info).await?;
        Ok(info)
    }

    pub async fn pair_capture_session(
        &self,
        token: &str,
        protocol_version: u16,
        capabilities: CaptureClientCapabilities,
    ) -> AppResult<CaptureSessionInfo> {
        let info = {
            let mut sessions = self.capture_sessions.lock().await;
            let record = sessions
                .get_mut(&Self::capture_token_hash(token))
                .ok_or_else(|| AppError::not_found("Capture session not found or expired."))?;
            if protocol_version != 0 && protocol_version != record.info.protocol_version {
                return Err(AppError::validation(format!(
                    "Capture protocol version {} is unsupported; expected {}.",
                    protocol_version, record.info.protocol_version
                )));
            }
            record.info.client_capabilities = capabilities;
            record.info.state = CaptureSessionState::Capturing;
            record.info.clone()
        };
        self.persist_capture_session(&info).await?;
        Ok(info)
    }

    pub async fn set_capture_reconstruction_progress(&self, token: &str, progress: f32) {
        if let Some(record) = self
            .capture_sessions
            .lock()
            .await
            .get_mut(&Self::capture_token_hash(token))
        {
            record.info.reconstruction_progress = Some(progress.clamp(0.0, 1.0));
        }
    }

    pub async fn complete_capture_reconstruction(
        &self,
        token: &str,
        preview: crate::contracts::CaptureMeshPreview,
    ) -> AppResult<CaptureSessionInfo> {
        let info = {
            let mut sessions = self.capture_sessions.lock().await;
            let record = sessions
                .get_mut(&Self::capture_token_hash(token))
                .ok_or_else(|| AppError::not_found("Capture session not found or expired."))?;
            record.info.state = CaptureSessionState::Preview;
            record.info.reconstruction_progress = Some(1.0);
            record.info.raw_error = None;
            record.info.guidance = "Inspect reconstructed mesh".into();
            record.info.mesh_preview = Some(preview);
            record.info.clone()
        };
        self.persist_capture_session(&info).await?;
        Ok(info)
    }

    pub async fn fail_capture_reconstruction(&self, token: &str, error: String) -> AppResult<()> {
        let info = {
            let mut sessions = self.capture_sessions.lock().await;
            sessions
                .get_mut(&Self::capture_token_hash(token))
                .map(|record| {
                    record.info.state = CaptureSessionState::Failed;
                    record.info.raw_error = Some(error);
                    record.info.guidance = "Reconstruction failed".into();
                    record.info.clone()
                })
        };
        if let Some(info) = info {
            self.persist_capture_session(&info).await?;
        }
        Ok(())
    }

    pub async fn register_capture_reconstruction(
        &self,
        token: &str,
        cancellation: crate::capture_reconstruction::ReconstructionCancellation,
    ) {
        self.capture_reconstructions
            .lock()
            .await
            .insert(Self::capture_token_hash(token), cancellation);
    }

    pub async fn clear_capture_reconstruction(&self, token: &str) {
        self.capture_reconstructions
            .lock()
            .await
            .remove(&Self::capture_token_hash(token));
    }

    pub async fn cancel_capture_session(&self, token: &str) -> AppResult<CaptureSessionInfo> {
        self.cleanup_capture_sessions().await;
        if let Some(cancellation) = self
            .capture_reconstructions
            .lock()
            .await
            .remove(&Self::capture_token_hash(token))
        {
            cancellation.cancel();
        }
        let info = {
            let mut sessions = self.capture_sessions.lock().await;
            let mut record = sessions
                .remove(&Self::capture_token_hash(token))
                .ok_or_else(|| AppError::not_found("Capture session not found or expired."))?;
            record.info.state = CaptureSessionState::Cancelled;
            record.info
        };
        self.persist_capture_session(&info).await?;
        Ok(info)
    }

    pub async fn capture_manifest(&self, token: &str) -> AppResult<Vec<CaptureFrameManifestEntry>> {
        self.cleanup_capture_sessions().await;
        self.capture_sessions
            .lock()
            .await
            .get(&Self::capture_token_hash(token))
            .map(|record| record.frames.clone())
            .ok_or_else(|| AppError::not_found("Capture session not found or expired."))
    }

    pub async fn add_capture_frame(
        &self,
        token: &str,
        frame: CaptureFrameManifestEntry,
    ) -> AppResult<(CaptureFrameManifestEntry, bool)> {
        self.cleanup_capture_sessions().await;
        let (result, info) = {
            let mut sessions = self.capture_sessions.lock().await;
            let record = sessions
                .get_mut(&Self::capture_token_hash(token))
                .ok_or_else(|| AppError::not_found("Capture session not found or expired."))?;
            if matches!(record.info.state, CaptureSessionState::Cancelled) {
                return Err(AppError::validation("Capture session is cancelled."));
            }
            if let Some(existing) = record.frames.iter().find(|existing| {
                existing.frame_id == frame.frame_id
                    || existing.content_digest == frame.content_digest
            }) {
                if existing.content_digest != frame.content_digest {
                    return Err(AppError::validation(format!(
                        "Frame `{}` already exists with a different digest.",
                        frame.frame_id
                    )));
                }
                return Ok((existing.clone(), false));
            }
            record.frames.push(frame.clone());
            record.info.accepted_frame_count = record.frames.len() as u32;
            record.info.coverage_percent = frame.server_assessment.coverage_percent;
            record.info.guidance = frame.server_assessment.guidance.clone();
            record.info.state = CaptureSessionState::Capturing;
            ((frame, true), record.info.clone())
        };
        self.persist_capture_session(&info).await?;
        Ok(result)
    }

    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.lock().unwrap() = Some(handle.clone());
        let codex_app_server = self.codex_app_server.clone();
        let agy_provider = self.agy_provider.clone();
        tauri::async_runtime::spawn(async move {
            codex_app_server.set_app_handle(handle.clone()).await;
            agy_provider.set_app_handle(handle).await;
        });
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

    /// Record content-free context-budget telemetry to the in-app profiler
    /// (OpenSpec `agent-context-budgeting`, section 4.2). The record is derived
    /// from the shape-only [`ContextEnvelope`] plus optional numeric provider
    /// usage, then serialized to one profiler line. Because the telemetry is
    /// content-free by construction, no source, references, prompt, image bytes,
    /// API keys, authorization headers, or full paths can leak into the log.
    /// This reuses the existing session-activity/profiler ring buffer — it adds
    /// no status bar and no new UI surface.
    pub fn record_context_telemetry(
        &self,
        envelope: &crate::context_envelope::ContextEnvelope,
        usage: Option<&crate::contracts::UsageSummary>,
    ) {
        let telemetry_usage = usage.map(|u| crate::context_envelope::TelemetryUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            total_tokens: u.total_tokens,
            cached_input_tokens: u.cached_input_tokens,
            reasoning_tokens: u.reasoning_tokens,
        });
        let telemetry = crate::context_envelope::envelope_telemetry(envelope, telemetry_usage);
        let json = serde_json::to_string(&telemetry).unwrap_or_else(|_| "{}".to_string());
        self.push_log(format!("[CONTEXT] {json}"));
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

    pub fn record_agent_activity_event(
        &self,
        input: AgentActivityEventInput,
    ) -> AgentActivityEvent {
        self.agent_activity.lock().unwrap().record(input)
    }

    pub fn get_agent_activity(&self, after_cursor: Option<u64>) -> AgentActivityCatchUp {
        self.agent_activity.lock().unwrap().catch_up(after_cursor)
    }

    pub fn emit_agent_activity_event(&self, event: &AgentActivityEvent) {
        let handle = self.app_handle.lock().unwrap().clone();
        if let Some(handle) = handle {
            match crate::transport_budget::require_serialized_budget(
                "agentActivityEvent",
                event,
                crate::transport_budget::ACTIVITY_EVENT_MAX_BYTES,
                "get_agent_activity cursor catch-up",
            ) {
                Ok(_) => {
                    let _ = handle.emit("agent-activity-event", event);
                }
                Err(error) => self.push_log(format!("[IPC] {}", error.message)),
            }
        }
    }

    pub fn emit_agent_draft_preview_changed(
        &self,
        event: &crate::contracts::AgentDraftPreviewChangedEvent,
    ) {
        let handle = self.app_handle.lock().unwrap().clone();
        if let Some(handle) = handle {
            match crate::transport_budget::require_serialized_budget(
                "agentDraftPreviewChanged",
                event,
                crate::transport_budget::ACTIVITY_EVENT_MAX_BYTES,
                "get_agent_draft_preview",
            ) {
                Ok(_) => {
                    let _ = handle.emit("agent-draft-preview-changed", event);
                }
                Err(error) => self.push_log(format!("[IPC] {}", error.message)),
            }
        }
    }

    pub fn emit_history_updated(&self) {
        self.emit_history_changed(None, None, "unknown");
    }

    pub fn emit_history_changed(
        &self,
        thread_id: Option<String>,
        message_id: Option<String>,
        kind: impl Into<String>,
    ) {
        let event = next_history_changed_event(thread_id, message_id, kind);
        let handle = self.app_handle.lock().unwrap().clone();
        if let Some(handle) = handle {
            let _ = handle.emit("history-updated", event);
        }
    }

    /// Close a single pending prompt: SIGCONT any frozen process, send Err to unblock the handler,
    /// clear waiting_on_prompt, and emit agent-prompt-closed to the frontend.
    pub async fn close_single_prompt(
        &self,
        request_id: &str,
        session_id: &str,
        thread_id: Option<String>,
        reason: &str,
    ) {
        let pgid = {
            let mut waits = self.prompt_waits.lock().unwrap();
            waits.remove(request_id).and_then(|ctrl| ctrl.pgid)
        };
        #[cfg(unix)]
        if let Some(pgid) = pgid {
            unsafe {
                libc::kill(-pgid, libc::SIGCONT);
            }
        }
        {
            let mut channels = self.prompt_channels.lock().await;
            if let Some(tx) = channels.remove(request_id) {
                let _ = tx.send(Err(reason.to_string()));
            }
        }
        {
            let mut sessions = self.mcp_session_registry.with_sessions().lock().await;
            if let Some(session) = sessions.get_mut(session_id) {
                session.waiting_on_prompt = false;
            }
        }
        let handle = self.app_handle.lock().unwrap().clone();
        if let Some(handle) = handle {
            let _ = handle.emit(
                "agent-prompt-closed",
                serde_json::json!({
                    "requestId": request_id,
                    "sessionId": session_id,
                    "threadId": thread_id,
                    "reason": reason,
                }),
            );
        }
    }

    /// Close all pending prompts for a session (e.g. on disconnect or logout).
    pub async fn close_prompts_for_session(&self, session_id: &str, reason: &str) {
        let targets: Vec<(String, Option<String>)> = {
            let waits = self.prompt_waits.lock().unwrap();
            waits
                .iter()
                .filter(|(_, ctrl)| ctrl.session_id == session_id)
                .map(|(req_id, ctrl)| (req_id.clone(), ctrl.thread_id.clone()))
                .collect()
        };
        for (request_id, thread_id) in targets {
            self.close_single_prompt(&request_id, session_id, thread_id, reason)
                .await;
        }
    }

    /// Close all pending prompts for an agent label (e.g. when the agent process is stopped).
    pub async fn close_prompts_for_agent_label(&self, agent_label: &str, reason: &str) {
        let targets: Vec<(String, String, Option<String>)> = {
            let waits = self.prompt_waits.lock().unwrap();
            waits
                .iter()
                .filter(|(_, ctrl)| ctrl.agent_label == agent_label)
                .map(|(req_id, ctrl)| {
                    (
                        req_id.clone(),
                        ctrl.session_id.clone(),
                        ctrl.thread_id.clone(),
                    )
                })
                .collect()
        };
        for (request_id, session_id, thread_id) in targets {
            self.close_single_prompt(&request_id, &session_id, thread_id, reason)
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_envelope::{
        assemble_envelope, envelope_telemetry, section_id, ContextSection, EnvelopeStage,
        SectionPriority, Sensitivity, TelemetryUsage,
    };
    use crate::contracts::{
        AppErrorCode, Config, Engine, EngineKind, GeometryBackend, SourceLanguage, UsageSummary,
    };

    fn minimal_config() -> Config {
        Config {
            engines: vec![Engine {
                id: "test".to_string(),
                name: "Test".to_string(),
                provider: "gemini".to_string(),
                api_key: String::new(),
                model: "gemini-2.5-flash".to_string(),
                light_model: "gemini-2.5-flash-lite".to_string(),
                base_url: String::new(),
                enabled: true,
                vision_overrides: std::collections::HashMap::new(),
            }],
            selected_engine_id: "test".to_string(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: vec![],
            microwave: None,
            voice: crate::contracts::VoiceConfig::default(),
            mcp: crate::contracts::McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: false,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            default_engine_kind: EngineKind::Freecad,
            default_source_language: SourceLanguage::LegacyPython,
            default_geometry_backend: GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 2,
            projects_root: None,
        }
    }

    fn state() -> AppState {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        crate::capture_runs::ensure_schema(&conn).expect("capture schema");
        AppState::new(minimal_config(), None, conn)
    }

    #[tokio::test]
    async fn geometry_render_activity_follows_the_backend_render_lock() {
        let state = state();
        assert_eq!(state.geometry_render_active_count(), 0);

        let guard = state.acquire_geometry_render().await;
        assert_eq!(state.geometry_render_active_count(), 1);

        drop(guard);
        assert_eq!(state.geometry_render_active_count(), 0);
    }

    #[tokio::test]
    async fn geometry_render_activity_includes_renders_waiting_for_the_backend_lock() {
        let state = state();
        let first_guard = state.acquire_geometry_render().await;
        let waiting_state = state.clone();
        let waiting_render =
            tokio::spawn(async move { waiting_state.acquire_geometry_render().await });

        tokio::task::yield_now().await;
        assert_eq!(state.geometry_render_active_count(), 2);

        drop(first_guard);
        let second_guard = waiting_render.await.expect("waiting render guard");
        assert_eq!(state.geometry_render_active_count(), 1);
        drop(second_guard);
        assert_eq!(state.geometry_render_active_count(), 0);
    }

    #[tokio::test]
    async fn cancelled_waiting_render_clears_its_activity() {
        let state = state();
        let first_guard = state.acquire_geometry_render().await;
        let waiting_state = state.clone();
        let waiting_render =
            tokio::spawn(async move { waiting_state.acquire_geometry_render().await });

        tokio::task::yield_now().await;
        assert_eq!(state.geometry_render_active_count(), 2);

        waiting_render.abort();
        let _ = waiting_render.await;
        assert_eq!(state.geometry_render_active_count(), 1);
        drop(first_guard);
        assert_eq!(state.geometry_render_active_count(), 0);
    }

    /// 4.2: `record_context_telemetry` emits the envelope shape plus provider
    /// cache/input/output usage through the existing profiler/session-activity
    /// path (the `app_logs` ring buffer surfaced by `get_app_logs`). The
    /// recorded line is content-free: even though the budgeted sections carry
    /// secret source/prompt/API-key/path content, none of it reaches the log.
    #[test]
    fn record_context_telemetry_emits_content_free_shape_line() {
        let state = state();
        let request = ContextSection::new(
            section_id::REQUEST,
            SectionPriority::Mandatory,
            Sensitivity::Sensitive,
            "PROMPT-SECRET-DO-NOT-LEAK",
        );
        let source = ContextSection::new(
            section_id::CURRENT_SOURCE,
            SectionPriority::Authoritative,
            Sensitivity::Sensitive,
            "SOURCE-SECRET sk-leak-AAAAAAAA Bearer zzz /Users/secret/x.ecky",
        );
        let envelope = assemble_envelope(EnvelopeStage::Generation, vec![request, source])
            .expect("envelope assembles");
        let usage = UsageSummary {
            input_tokens: 100,
            output_tokens: 40,
            total_tokens: 140,
            cached_input_tokens: 60,
            reasoning_tokens: 0,
            estimated_cost_usd: None,
            segments: Vec::new(),
        };

        state.record_context_telemetry(&envelope, Some(&usage));

        let logs = state.app_logs.lock().unwrap();
        assert_eq!(logs.len(), 1, "exactly one profiler line recorded");
        let line = &logs[0].message;
        assert!(line.starts_with("[CONTEXT] "), "tagged as a context line");
        let json = line.trim_start_matches("[CONTEXT] ");
        let parsed: serde_json::Value =
            serde_json::from_str(json).expect("recorded payload is valid JSON");

        // Shape + provider usage are present.
        assert_eq!(parsed["stage"], "generation");
        assert_eq!(parsed["ceilingChars"], 64_000);
        assert_eq!(parsed["usage"]["inputTokens"], 100);
        assert_eq!(parsed["usage"]["cachedInputTokens"], 60);
        assert_eq!(parsed["usage"]["outputTokens"], 40);
        assert!(parsed["sections"].is_array());
        // Inclusion decisions are carried.
        let decisions: Vec<&str> = parsed["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["decision"].as_str().unwrap())
            .collect();
        assert!(decisions.contains(&"included"));

        // No sensitive content leaks into the profiler line.
        for needle in [
            "PROMPT-SECRET-DO-NOT-LEAK",
            "SOURCE-SECRET",
            "sk-leak-AAAAAAAA",
            "Bearer zzz",
            "/Users/secret/x.ecky",
        ] {
            assert!(!line.contains(needle), "profiler line leaked: {needle}");
        }
    }

    /// 4.2: recording without provider usage still emits a valid, content-free
    /// shape line (the `usage` field is omitted entirely).
    #[test]
    fn record_context_telemetry_without_usage_omits_usage_field() {
        let state = state();
        let request = ContextSection::new(
            section_id::REQUEST,
            SectionPriority::Mandatory,
            Sensitivity::Sensitive,
            "NEVER-LEAK-THIS-PROMPT",
        );
        let envelope = assemble_envelope(EnvelopeStage::Classifier, vec![request]).unwrap();
        state.record_context_telemetry(&envelope, None);
        let line = &state.app_logs.lock().unwrap()[0].message;
        let json = line.trim_start_matches("[CONTEXT] ");
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        assert!(parsed.get("usage").is_none() || parsed["usage"].is_null());
        assert!(!line.contains("NEVER-LEAK-THIS-PROMPT"));
    }

    /// Guards the derivation contract used by the emission path: the recorded
    /// JSON carries exactly the shape fields (stage, ceiling, totals, sections,
    /// usage) and deserializes as a well-formed object.
    #[test]
    fn recorded_telemetry_is_a_well_formed_shape_object() {
        let envelope = assemble_envelope(
            EnvelopeStage::Generation,
            vec![ContextSection::new(
                section_id::REQUEST,
                SectionPriority::Mandatory,
                Sensitivity::Sensitive,
                "secret",
            )],
        )
        .unwrap();
        let telemetry = envelope_telemetry(
            &envelope,
            Some(TelemetryUsage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: 3,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
            }),
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&telemetry).unwrap()).unwrap();
        assert_eq!(parsed["stage"], "generation");
        assert_eq!(parsed["ceilingChars"], 64_000);
        assert_eq!(parsed["sections"][0]["id"], "request");
        assert_eq!(parsed["sections"][0]["decision"], "included");
        assert_eq!(parsed["usage"]["inputTokens"], 1);
        // camelCase boundary, no snake_case leaks.
        let json = parsed.to_string();
        assert!(!json.contains("observed_chars"));
        assert!(!json.contains("input_tokens"));
    }

    #[tokio::test]
    async fn start_capture_session_creates_retrievable_pairing_session() {
        let state = state();
        *state.capture_server_url.lock().unwrap() = Some("http://192.0.2.1:44000".into());
        *state.capture_trust_url.lock().unwrap() = Some("http://192.0.2.1:44001/trust".into());
        let session = state
            .start_capture_session(3600, "thread-a".into(), Some("message-a".into()))
            .await
            .expect("start capture session");

        assert_eq!(session.state, CaptureSessionState::Pairing);
        assert!(!session.session_id.is_empty(), "session_id is set");
        assert_ne!(session.session_id, session.pairing_token);
        assert_eq!(
            session.pairing_url,
            format!("http://192.0.2.1:44000/capture/{}", session.pairing_token)
        );
        assert_eq!(session.trust_url, "http://192.0.2.1:44001/trust");
        assert_eq!(session.target_thread_id, "thread-a");
        assert_eq!(session.target_message_id.as_deref(), Some("message-a"));

        let fetched = state
            .get_capture_session(&session.pairing_token)
            .await
            .expect("capturing session exists");
        assert_eq!(fetched, session);
    }

    #[tokio::test]
    async fn set_capture_session_state_updates_active_session() {
        let state = state();
        *state.capture_server_url.lock().unwrap() = Some("http://192.0.2.1:44000".into());
        *state.capture_trust_url.lock().unwrap() = Some("http://192.0.2.1:44001/trust".into());
        let session = state
            .start_capture_session(3600, "thread-test".into(), None)
            .await
            .expect("start capture session");

        let updated = state
            .set_capture_session_state(&session.pairing_token, CaptureSessionState::Capturing)
            .await
            .expect("session state updates");
        assert_eq!(updated.state, CaptureSessionState::Capturing);

        let after = state
            .get_capture_session(&session.pairing_token)
            .await
            .expect("session still present");
        assert_eq!(after.state, CaptureSessionState::Capturing);
    }

    #[tokio::test]
    async fn set_capture_session_state_fails_for_unknown_token() {
        let state = state();
        let error = state
            .set_capture_session_state("missing-token", CaptureSessionState::Cancelled)
            .await
            .expect_err("unknown token fails");
        assert_eq!(error.code, AppErrorCode::NotFound);
    }

    #[tokio::test]
    async fn pairing_records_client_capabilities_and_rejects_wrong_protocol() {
        let state = state();
        *state.capture_server_url.lock().unwrap() = Some("http://192.0.2.1:44000".into());
        *state.capture_trust_url.lock().unwrap() = Some("http://192.0.2.1:44001/trust".into());
        let session = state
            .start_capture_session(3600, "thread-test".into(), None)
            .await
            .unwrap();
        let capabilities = CaptureClientCapabilities {
            metric_depth: true,
            camera_intrinsics: true,
            camera_pose: true,
            depth_sidecars: true,
        };
        let paired = state
            .pair_capture_session(&session.pairing_token, 1, capabilities.clone())
            .await
            .unwrap();
        assert_eq!(paired.client_capabilities, capabilities);
        assert_eq!(paired.state, CaptureSessionState::Capturing);
        let error = state
            .pair_capture_session(&session.pairing_token, 99, Default::default())
            .await
            .unwrap_err();
        assert!(error.message.contains("unsupported"));
    }

    #[tokio::test]
    async fn reconstruction_failure_preserves_session_frames_and_raw_error() {
        let state = state();
        *state.capture_server_url.lock().unwrap() = Some("http://192.0.2.1:44000".into());
        *state.capture_trust_url.lock().unwrap() = Some("http://192.0.2.1:44001/trust".into());
        let session = state
            .start_capture_session(3600, "thread-test".into(), None)
            .await
            .unwrap();
        let frame = CaptureFrameManifestEntry {
            frame_id: "frame-1".into(),
            content_digest: "digest".into(),
            captured_at: 1,
            mime_type: "image/jpeg".into(),
            width: 2,
            height: 2,
            image_path: "source/digest.jpg".into(),
            client_metrics: None,
            camera_intrinsics: None,
            camera_transform: None,
            depth_digest: None,
            visual_signature: vec![1],
            server_assessment: Default::default(),
        };
        state
            .add_capture_frame(&session.pairing_token, frame.clone())
            .await
            .unwrap();

        state
            .fail_capture_reconstruction(&session.pairing_token, "provider failed raw".into())
            .await
            .unwrap();

        let failed = state
            .get_capture_session(&session.pairing_token)
            .await
            .unwrap();
        assert_eq!(failed.state, CaptureSessionState::Failed);
        assert_eq!(failed.raw_error.as_deref(), Some("provider failed raw"));
        assert_eq!(
            state
                .capture_manifest(&session.pairing_token)
                .await
                .unwrap(),
            vec![frame]
        );
        state
            .set_capture_session_state(&session.pairing_token, CaptureSessionState::Preview)
            .await
            .unwrap();
        let resumed = state
            .resume_capture_session(&session.pairing_token)
            .await
            .unwrap();
        assert_eq!(resumed.state, CaptureSessionState::Capturing);
        assert_eq!(
            state
                .capture_manifest(&session.pairing_token)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn cancel_capture_session_revokes_pairing_token() {
        let state = state();
        *state.capture_server_url.lock().unwrap() = Some("http://192.0.2.1:44000".into());
        *state.capture_trust_url.lock().unwrap() = Some("http://192.0.2.1:44001/trust".into());
        let session = state
            .start_capture_session(3600, "thread-test".into(), None)
            .await
            .expect("start capture session");

        let cancelled = state
            .cancel_capture_session(&session.pairing_token)
            .await
            .expect("cancel capture session");

        assert_eq!(cancelled.state, CaptureSessionState::Cancelled);
        assert!(state
            .get_capture_session(&session.pairing_token)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn expired_capture_sessions_are_cleaned_up() {
        let state = state();
        *state.capture_server_url.lock().unwrap() = Some("http://192.0.2.1:44000".into());
        *state.capture_trust_url.lock().unwrap() = Some("http://192.0.2.1:44001/trust".into());
        let session = state
            .start_capture_session(3600, "thread-test".into(), None)
            .await
            .expect("start capture session");

        {
            let mut sessions = state.capture_sessions.lock().await;
            let stale_session = sessions
                .get_mut(&AppState::capture_token_hash(&session.pairing_token))
                .expect("session exists");
            stale_session.info.expires_at = AppState::now_secs().saturating_sub(1);
        }

        assert!(state
            .get_capture_session(&session.pairing_token)
            .await
            .is_none());
    }
}
