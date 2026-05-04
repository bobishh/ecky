use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, HashMap, HashSet};

pub type AppResult<T> = Result<T, AppError>;
pub type DesignParams = BTreeMap<String, ParamValue>;
pub const GENIE_TRAITS_VERSION: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrigin {
    pub host_label: String,
    pub client_kind: String,
    pub agent_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_model_label: Option<String>,
    pub session_id: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub session_id: String,
    pub client_kind: String,
    pub host_label: String,
    pub agent_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_model_label: Option<String>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
    pub phase: String,
    pub status_text: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetLeaseInfo {
    pub session_id: String,
    pub thread_id: String,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub host_label: String,
    pub agent_label: String,
    pub acquired_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAgentState {
    /// "none" | "sleeping" | "waking" | "waiting" | "active" | "disconnected" | "error"
    pub connection_state: String,
    pub agent_label: Option<String>,
    pub llm_model_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub phase: Option<String>,
    pub status_text: Option<String>,
    #[serde(default)]
    pub busy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_kind: Option<String>,
    #[serde(default)]
    pub waiting_on_prompt: bool,
    pub updated_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkingVersionEvent {
    pub session_id: String,
    pub thread_id: String,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTerminalSnapshot {
    pub agent_id: String,
    pub agent_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_kind: Option<String>,
    /// Stable per-PTY-session token. Changes whenever the backend creates
    /// a fresh terminal session for the agent.
    pub session_nonce: u64,
    /// Deprecated compatibility snapshot for inactive/last-session display.
    /// Live terminal rendering should use `vtStream`.
    pub screen_text: String,
    /// Authoritative raw VT replay stream for live terminal rendering.
    #[serde(default)]
    pub vt_stream: String,
    /// Optional incremental VT chunk for live updates. When present, the frontend
    /// should merge it into its local replay state instead of treating `vtStream`
    /// as a full snapshot replacement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vt_delta: Option<String>,
    pub attention_required: bool,
    #[serde(default)]
    pub busy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub active: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTerminalInput {
    pub agent_id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub meta: bool,
    #[serde(default)]
    pub submit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscribePromptAudioInput {
    pub base64_data: String,
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PromptTranscription {
    pub text: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportCameraState {
    pub position: [f64; 3],
    pub target: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fov: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveViewportScreenshotInput {
    pub request_id: String,
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub camera: ViewportCameraState,
    pub source: String,
    pub thread_id: String,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub include_overlays: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveAgentPromptInput {
    pub request_id: String,
    pub prompt_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreparePromptWorkspaceCaptureInput {
    pub data_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueueAgentPromptInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub prompt_text: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueuedAgentPrompt {
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RejectViewportScreenshotInput {
    pub request_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    pub running: bool,
    pub endpoint_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_startup_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AppErrorCode {
    Validation,
    NotFound,
    Conflict,
    Provider,
    Persistence,
    Render,
    Parse,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: AppErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl AppError {
    pub fn new(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(
        code: AppErrorCode,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details.into()),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Validation, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::NotFound, message)
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Provider, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Conflict, message)
    }

    pub fn persistence(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Persistence, message)
    }

    pub fn render(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Render, message)
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Parse, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Internal, message)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.details.as_deref() {
            Some(details) if !details.trim().is_empty() => {
                write!(f, "{}: {}", self.message, details)
            }
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for AppError {}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(alias = "api_key")]
    pub api_key: String,
    pub model: String,
    #[serde(default, alias = "light_model")]
    pub light_model: String,
    #[serde(alias = "base_url")]
    pub base_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MicrowaveConfig {
    #[serde(default, alias = "hum_id")]
    pub hum_id: Option<String>,
    #[serde(default, alias = "ding_id")]
    pub ding_id: Option<String>,
    #[serde(default)]
    pub muted: bool,
}

fn default_stt_language_code() -> String {
    "en-US".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfig {
    #[serde(default = "default_stt_language_code", alias = "stt_language_code")]
    pub stt_language_code: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            stt_language_code: default_stt_language_code(),
        }
    }
}

/// Whether Ecky runs the embedded MCP HTTP server.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoAgent {
    pub id: String,
    pub label: String,
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub args: Vec<String>,
    pub enabled: bool,
    /// Deprecated compatibility flag from the old eager-start implementation.
    /// Active-mode wake behavior now depends on `mcp.mode` and `mcp.primaryAgentId`.
    #[serde(default)]
    pub start_on_demand: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum McpMode {
    #[default]
    Passive,
    Active,
}

fn default_mcp_prompt_timeout_secs() -> u64 {
    1800
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppLogEntry {
    pub ts_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    /// HTTP port for the MCP server. Defaults to 39249.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Max concurrent agent sessions. None = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_sessions: Option<u8>,
    /// How Ecky exposes MCP: passive server-only or active server + lazy auto-agent wake.
    #[serde(default)]
    pub mode: McpMode,
    /// Which auto-agent should be woken when the user queues a message in active mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_agent_id: Option<String>,
    /// Default request_user_prompt timeout used when the agent does not pass timeoutSecs.
    #[serde(default = "default_mcp_prompt_timeout_secs")]
    pub prompt_timeout_secs: u64,
    /// External processes available to Ecky in active mode.
    #[serde(default)]
    pub auto_agents: Vec<AutoAgent>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            port: None,
            max_sessions: None,
            mode: McpMode::Passive,
            primary_agent_id: None,
            prompt_timeout_secs: default_mcp_prompt_timeout_secs(),
            auto_agents: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub engines: Vec<Engine>,
    #[serde(alias = "selected_engine_id")]
    pub selected_engine_id: String,
    #[serde(default, alias = "freecad_cmd")]
    pub freecad_cmd: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub microwave: Option<MicrowaveConfig>,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub has_seen_onboarding: bool,
    #[serde(default)]
    pub connection_type: Option<String>,
    #[serde(default = "default_engine_kind")]
    pub default_engine_kind: EngineKind,
    #[serde(default = "default_source_language")]
    pub default_source_language: SourceLanguage,
    #[serde(default = "default_geometry_backend")]
    pub default_geometry_backend: GeometryBackend,
    #[serde(default = "default_max_generation_attempts")]
    pub max_generation_attempts: u32,
    #[serde(default)]
    pub max_verify_attempts: u32,
}

fn default_max_generation_attempts() -> u32 {
    3
}

fn default_engine_kind() -> EngineKind {
    EngineKind::Freecad
}

fn default_source_language() -> SourceLanguage {
    SourceLanguage::LegacyPython
}

fn default_geometry_backend() -> GeometryBackend {
    GeometryBackend::Freecad
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum EngineKind {
    #[default]
    Freecad,
    #[serde(rename = "ecky", alias = "eckyIrV0", alias = "ecky_ir_v0")]
    #[specta(rename = "ecky")]
    EckyIrV0,
    #[serde(rename = "build123d")]
    #[specta(rename = "build123d")]
    Build123d,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SourceLanguage {
    #[default]
    LegacyPython,
    #[serde(rename = "ecky", alias = "eckyIrV0", alias = "ecky_ir_v0")]
    #[specta(rename = "ecky")]
    EckyIrV0,
    #[serde(rename = "build123d")]
    #[specta(rename = "build123d")]
    Build123d,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
pub enum GeometryBackend {
    #[default]
    #[serde(rename = "freecad")]
    Freecad,
    #[serde(rename = "build123d")]
    Build123d,
    #[serde(rename = "mesh", alias = "eckyRust", alias = "ecky_rust")]
    #[specta(rename = "mesh")]
    EckyRust,
}

impl EngineKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Freecad => "freecad",
            Self::EckyIrV0 => "ecky",
            Self::Build123d => "build123d",
        }
    }

    pub fn to_source_language(&self) -> SourceLanguage {
        match self {
            Self::Freecad => SourceLanguage::LegacyPython,
            Self::EckyIrV0 => SourceLanguage::EckyIrV0,
            Self::Build123d => SourceLanguage::Build123d,
        }
    }

    pub fn to_geometry_backend(&self) -> GeometryBackend {
        match self {
            Self::Freecad => GeometryBackend::Freecad,
            Self::EckyIrV0 => GeometryBackend::EckyRust,
            Self::Build123d => GeometryBackend::Build123d,
        }
    }
}

impl std::str::FromStr for EngineKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "freecad" => Ok(Self::Freecad),
            "ecky" | "eckyIrV0" | "ecky_ir_v0" => Ok(Self::EckyIrV0),
            "build123d" => Ok(Self::Build123d),
            _ => Err(()),
        }
    }
}

impl SourceLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LegacyPython => "legacyPython",
            Self::EckyIrV0 => "ecky",
            Self::Build123d => "build123d",
        }
    }

    pub fn to_engine_kind(&self) -> EngineKind {
        match self {
            Self::LegacyPython => EngineKind::Freecad,
            Self::EckyIrV0 => EngineKind::EckyIrV0,
            Self::Build123d => EngineKind::Build123d,
        }
    }
}

impl std::str::FromStr for SourceLanguage {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "legacyPython" | "legacy_python" => Ok(Self::LegacyPython),
            "ecky" | "eckyIrV0" | "ecky_ir_v0" => Ok(Self::EckyIrV0),
            "build123d" => Ok(Self::Build123d),
            _ => Err(()),
        }
    }
}

impl GeometryBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Freecad => "freecad",
            Self::Build123d => "build123d",
            Self::EckyRust => "mesh",
        }
    }
}

impl std::str::FromStr for GeometryBackend {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "freecad" => Ok(Self::Freecad),
            "build123d" => Ok(Self::Build123d),
            "mesh" | "eckyRust" | "ecky_rust" => Ok(Self::EckyRust),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBackendCapability {
    pub available: bool,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAuthoringContext {
    pub engine_kind: EngineKind,
    pub source_language: SourceLanguage,
    pub geometry_backend: GeometryBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub freecad: RuntimeBackendCapability,
    pub build123d: RuntimeBackendCapability,
    pub direct_occt: RuntimeBackendCapability,
    #[serde(rename = "mesh", alias = "eckyRust")]
    #[specta(rename = "mesh")]
    pub ecky_rust: RuntimeBackendCapability,
    pub recommended_authoring_context: RuntimeAuthoringContext,
}

impl ToSql for EngineKind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for EngineKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        Ok(raw.parse().unwrap_or_default())
    }
}

impl ToSql for SourceLanguage {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for SourceLanguage {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        Ok(raw.parse().unwrap_or_default())
    }
}

impl ToSql for GeometryBackend {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for GeometryBackend {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        Ok(raw.parse().unwrap_or_default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EyeStyle {
    Dot,
    Bar,
    Slant,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenieTraits {
    #[serde(default = "default_genie_traits_version")]
    pub version: u8,
    pub seed: u32,
    pub color_hue: f64,
    pub vertex_count: u8,
    pub radius_base: f64,
    pub stretch_y: f64,
    pub asymmetry: f64,
    pub chord_skip: u8,
    pub jitter_scale: f64,
    pub pulse_scale: f64,
    pub hover_scale: f64,
    pub warp_scale: f64,
    pub glow_hue_shift: f64,
    pub eye_style: EyeStyle,
    pub eye_spacing: f64,
    pub eye_size: f64,
    pub mouth_curve: f64,
    pub thinking_bias: f64,
    pub repair_bias: f64,
    pub render_bias: f64,
    pub expressiveness: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LegacyGenieTraitsV1 {
    #[serde(default)]
    seed: Option<u32>,
    #[serde(default)]
    color_hue: Option<f64>,
    #[serde(default)]
    vertex_count: Option<u8>,
    #[serde(default)]
    jitter_scale: Option<f64>,
    #[serde(default)]
    pulse_scale: Option<f64>,
}

#[derive(Debug, Clone)]
struct GeneRng {
    state: u64,
}

impl GeneRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) / ((1u64 << 53) as f64)
    }

    fn range_f64(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min) * self.unit()
    }

    fn range_u8(&mut self, min: u8, max_inclusive: u8) -> u8 {
        min + (self.next_u64() % (u64::from(max_inclusive - min) + 1)) as u8
    }

    fn eye_style(&mut self) -> EyeStyle {
        match self.range_u8(0, 2) {
            0 => EyeStyle::Dot,
            1 => EyeStyle::Bar,
            _ => EyeStyle::Slant,
        }
    }
}

fn default_genie_traits_version() -> u8 {
    GENIE_TRAITS_VERSION
}

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

fn normalize_hue(value: f64) -> f64 {
    value.rem_euclid(360.0)
}

pub fn derive_thread_seed(thread_id: &str) -> u32 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in thread_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    let seed = ((hash >> 32) ^ hash) as u32;
    if seed == 0 {
        1
    } else {
        seed
    }
}

impl GenieTraits {
    pub fn from_seed(seed: u32) -> Self {
        let mut rng = GeneRng::new(u64::from(seed));
        Self {
            version: GENIE_TRAITS_VERSION,
            seed,
            color_hue: rng.range_f64(0.0, 360.0),
            vertex_count: rng.range_u8(10, 24),
            radius_base: rng.range_f64(25.0, 34.0),
            stretch_y: rng.range_f64(0.90, 1.06),
            asymmetry: rng.range_f64(0.88, 1.14),
            chord_skip: rng.range_u8(2, 6),
            jitter_scale: rng.range_f64(0.70, 1.45),
            pulse_scale: rng.range_f64(0.70, 1.35),
            hover_scale: rng.range_f64(0.80, 1.60),
            warp_scale: rng.range_f64(0.35, 1.25),
            glow_hue_shift: rng.range_f64(-32.0, 32.0),
            eye_style: rng.eye_style(),
            eye_spacing: rng.range_f64(15.0, 22.5),
            eye_size: rng.range_f64(2.00, 3.60),
            mouth_curve: rng.range_f64(0.60, 2.60),
            thinking_bias: rng.range_f64(0.20, 1.00),
            repair_bias: rng.range_f64(0.20, 1.00),
            render_bias: rng.range_f64(0.20, 1.00),
            expressiveness: rng.range_f64(0.35, 1.00),
        }
    }

    pub fn normalized(mut self) -> Self {
        if self.version == 0 {
            self.version = GENIE_TRAITS_VERSION;
        }
        if self.seed == 0 {
            self.seed = 1;
        }
        self.color_hue = normalize_hue(self.color_hue);
        self.vertex_count = self.vertex_count.clamp(10, 24);
        self.radius_base = clamp_f64(self.radius_base, 25.0, 34.0);
        self.stretch_y = clamp_f64(self.stretch_y, 0.90, 1.06);
        self.asymmetry = clamp_f64(self.asymmetry, 0.88, 1.14);
        self.chord_skip = self.chord_skip.clamp(2, 6);
        self.jitter_scale = clamp_f64(self.jitter_scale, 0.70, 1.45);
        self.pulse_scale = clamp_f64(self.pulse_scale, 0.70, 1.35);
        self.hover_scale = clamp_f64(self.hover_scale, 0.80, 1.60);
        self.warp_scale = clamp_f64(self.warp_scale, 0.35, 1.25);
        self.glow_hue_shift = clamp_f64(self.glow_hue_shift, -32.0, 32.0);
        self.eye_spacing = clamp_f64(self.eye_spacing, 15.0, 22.5);
        self.eye_size = clamp_f64(self.eye_size, 2.00, 3.60);
        self.mouth_curve = clamp_f64(self.mouth_curve, 0.60, 2.60);
        self.thinking_bias = clamp_f64(self.thinking_bias, 0.20, 1.00);
        self.repair_bias = clamp_f64(self.repair_bias, 0.20, 1.00);
        self.render_bias = clamp_f64(self.render_bias, 0.20, 1.00);
        self.expressiveness = clamp_f64(self.expressiveness, 0.35, 1.00);
        self
    }

    fn from_legacy(legacy: LegacyGenieTraitsV1, thread_id: Option<&str>) -> Self {
        let seed = legacy
            .seed
            .filter(|value| *value != 0)
            .unwrap_or_else(|| derive_thread_seed(thread_id.unwrap_or("legacy-thread")));
        let mut traits = Self::from_seed(seed);
        if let Some(color_hue) = legacy.color_hue {
            traits.color_hue = normalize_hue(color_hue);
        }
        if let Some(vertex_count) = legacy.vertex_count {
            traits.vertex_count = vertex_count.clamp(10, 24);
        }
        if let Some(jitter_scale) = legacy.jitter_scale {
            traits.jitter_scale = clamp_f64(jitter_scale, 0.70, 1.45);
        }
        if let Some(pulse_scale) = legacy.pulse_scale {
            traits.pulse_scale = clamp_f64(pulse_scale, 0.70, 1.35);
        }
        traits.normalized()
    }
}

pub fn decode_genie_traits_json(raw: &str, thread_id: Option<&str>) -> Option<GenieTraits> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if value.get("version").is_some() {
        serde_json::from_value::<GenieTraits>(value)
            .ok()
            .map(GenieTraits::normalized)
    } else {
        serde_json::from_value::<LegacyGenieTraitsV1>(value)
            .ok()
            .map(|legacy| GenieTraits::from_legacy(legacy, thread_id))
    }
}

pub fn upgraded_or_default_genie_traits(thread_id: &str, raw: Option<&str>) -> GenieTraits {
    raw.and_then(|json| decode_genie_traits_json(json, Some(thread_id)))
        .unwrap_or_else(|| GenieTraits::from_seed(derive_thread_seed(thread_id)))
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiSpec {
    #[serde(default)]
    pub fields: Vec<UiField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", untagged)]
pub enum ParamValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
}

impl ParamValue {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::Number(_) => "number",
            Self::Boolean(_) => "boolean",
            Self::Null => "null",
        }
    }

    pub fn matches_select_value(&self, value: &SelectValue) -> bool {
        match (self, value) {
            (Self::String(left), SelectValue::String(right)) => left == right,
            (Self::Number(left), SelectValue::Number(right)) => left == right,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", untagged)]
pub enum SelectValue {
    String(String),
    Number(f64),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectOption {
    pub label: String,
    pub value: SelectValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UiField {
    Range {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        step: Option<f64>,
        #[serde(default, rename = "minFrom", alias = "min_from")]
        min_from: Option<String>,
        #[serde(default, rename = "maxFrom", alias = "max_from")]
        max_from: Option<String>,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
    Number {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        step: Option<f64>,
        #[serde(default, rename = "minFrom", alias = "min_from")]
        min_from: Option<String>,
        #[serde(default, rename = "maxFrom", alias = "max_from")]
        max_from: Option<String>,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
    Select {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        options: Vec<SelectOption>,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
    Checkbox {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
    Image {
        key: String,
        #[serde(default)]
        label: String,
        #[serde(default, alias = "freezed")]
        frozen: bool,
    },
}

impl UiField {
    pub fn key(&self) -> &str {
        match self {
            Self::Range { key, .. }
            | Self::Number { key, .. }
            | Self::Select { key, .. }
            | Self::Checkbox { key, .. }
            | Self::Image { key, .. } => key,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Range { label, .. }
            | Self::Number { label, .. }
            | Self::Select { label, .. }
            | Self::Checkbox { label, .. }
            | Self::Image { label, .. } => label,
        }
    }

    pub fn frozen(&self) -> bool {
        match self {
            Self::Range { frozen, .. }
            | Self::Number { frozen, .. }
            | Self::Select { frozen, .. }
            | Self::Checkbox { frozen, .. }
            | Self::Image { frozen, .. } => *frozen,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Range { .. } | Self::Number { .. })
    }

    pub fn validate_value(&self, value: &ParamValue) -> AppResult<()> {
        match self {
            Self::Range { key, .. } | Self::Number { key, .. } => match value {
                ParamValue::Number(_) => Ok(()),
                other => Err(AppError::validation(format!(
                    "Parameter '{}' must be a number, received {}.",
                    key,
                    other.kind()
                ))),
            },
            Self::Checkbox { key, .. } => match value {
                ParamValue::Boolean(_) => Ok(()),
                other => Err(AppError::validation(format!(
                    "Parameter '{}' must be a boolean, received {}.",
                    key,
                    other.kind()
                ))),
            },
            Self::Image { key, .. } => match value {
                ParamValue::String(_) => Ok(()),
                other => Err(AppError::validation(format!(
                    "Parameter '{}' must be a string (file path), received {}.",
                    key,
                    other.kind()
                ))),
            },
            Self::Select { key, options, .. } => match value {
                ParamValue::String(_) | ParamValue::Number(_) => {
                    if options.is_empty()
                        || options
                            .iter()
                            .any(|option| value.matches_select_value(&option.value))
                    {
                        Ok(())
                    } else {
                        Err(AppError::validation(format!(
                            "Parameter '{}' must match one of the declared select options.",
                            key
                        )))
                    }
                }
                other => Err(AppError::validation(format!(
                    "Parameter '{}' must be a string or number for a select field, received {}.",
                    key,
                    other.kind()
                ))),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InteractionMode {
    Design,
    Question,
    Tune,
}

impl InteractionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Design => "design",
            Self::Question => "question",
            Self::Tune => "tune",
        }
    }
}

impl std::str::FromStr for InteractionMode {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "design" => Ok(Self::Design),
            "question" => Ok(Self::Question),
            "tune" => Ok(Self::Tune),
            other => Err(AppError::validation(format!(
                "Unknown interaction mode '{}'.",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MacroDialect {
    Legacy,
    CadFrameworkV1,
    #[serde(rename = "ecky", alias = "eckyIrV0", alias = "ecky_ir_v0")]
    #[specta(rename = "ecky")]
    EckyIrV0,
    #[serde(rename = "build123d")]
    #[specta(rename = "build123d")]
    Build123d,
}

impl MacroDialect {
    pub fn is_framework(&self) -> bool {
        matches!(self, Self::CadFrameworkV1)
    }
}

fn default_macro_dialect() -> MacroDialect {
    MacroDialect::Legacy
}

pub fn infer_macro_dialect_from_code(macro_code: &str) -> MacroDialect {
    let trimmed = macro_code.trim();
    if trimmed.starts_with("(model") || trimmed.starts_with("(scene") {
        MacroDialect::EckyIrV0
    } else if trimmed.contains("build123d") {
        MacroDialect::Build123d
    } else if trimmed.contains("cad_sdk") || trimmed.contains("CONTROLS") {
        MacroDialect::CadFrameworkV1
    } else {
        MacroDialect::Legacy
    }
}

pub fn normalize_design_output(mut output: DesignOutput) -> DesignOutput {
    let inferred = infer_macro_dialect_from_code(&output.macro_code);
    if inferred.is_framework() || inferred == MacroDialect::EckyIrV0 {
        output.macro_dialect = inferred;
    }
    if output.engine_kind == EngineKind::Freecad && output.macro_dialect == MacroDialect::EckyIrV0 {
        output.engine_kind = EngineKind::EckyIrV0;
    }
    output.post_processing = normalize_post_processing_spec(output.post_processing.take());
    output
}

fn slugify_attachment_id(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut prev_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "lithophane".to_string()
    } else {
        trimmed.to_string()
    }
}

fn legacy_displacement_attachment_id(displacement: &DisplacementSpec) -> String {
    format!(
        "legacy-{}",
        slugify_attachment_id(displacement.image_param.trim())
    )
}

pub fn normalize_post_processing_spec(
    post_processing: Option<PostProcessingSpec>,
) -> Option<PostProcessingSpec> {
    let mut post = post_processing?;
    let mut attachments = post.lithophane_attachments;

    if let Some(displacement) = post.displacement.as_ref() {
        let legacy_id = legacy_displacement_attachment_id(displacement);
        if !attachments
            .iter()
            .any(|attachment| attachment.id == legacy_id)
        {
            attachments.insert(
                0,
                LithophaneAttachment {
                    id: legacy_id,
                    enabled: true,
                    source: LithophaneAttachmentSource::Param {
                        image_param: displacement.image_param.clone(),
                    },
                    target_part_id: String::new(),
                    placement: LithophanePlacement {
                        projection: displacement.projection,
                        ..default_lithophane_placement()
                    },
                    relief: LithophaneRelief {
                        depth_mm: displacement.depth_mm,
                        invert: displacement.invert,
                    },
                    color: default_lithophane_color(),
                },
            );
        }
    }

    post.lithophane_attachments = attachments
        .into_iter()
        .filter_map(|mut attachment| {
            if attachment.id.trim().is_empty() {
                let inferred = attachment
                    .source
                    .image_param()
                    .or_else(|| attachment.source.image_path())
                    .unwrap_or("lithophane");
                attachment.id = format!("litho-{}", slugify_attachment_id(inferred));
            }
            if attachment.relief.depth_mm <= 0.0 {
                attachment.relief.depth_mm = default_lithophane_depth_mm();
            }
            if attachment.color.channel_thickness_mm <= 0.0 {
                attachment.color.channel_thickness_mm = default_channel_thickness_mm();
            }
            if attachment.placement.bleed_margin_mm < 0.0 {
                attachment.placement.bleed_margin_mm = 0.0;
            }
            if attachment.placement.width_mm < 0.0 {
                attachment.placement.width_mm = 0.0;
            }
            if attachment.placement.height_mm < 0.0 {
                attachment.placement.height_mm = 0.0;
            }

            match &attachment.source {
                LithophaneAttachmentSource::File { image_path } if image_path.trim().is_empty() => {
                    Some(attachment)
                }
                LithophaneAttachmentSource::Param { image_param }
                    if image_param.trim().is_empty() =>
                {
                    None
                }
                _ => Some(attachment),
            }
        })
        .collect();

    if post.displacement.is_none() && post.lithophane_attachments.is_empty() {
        None
    } else {
        Some(post)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelList {
    pub models: Vec<String>,
    pub is_live: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProjectionType {
    Planar,
    Auto,
    Cylindrical,
    Spherical,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DisplacementSpec {
    pub image_param: String,
    pub projection: ProjectionType,
    pub depth_mm: f64,
    #[serde(default)]
    pub invert: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LithophanePlacementMode {
    PartSidePatch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LithophaneSide {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OverflowMode {
    Contain,
    Cover,
    Clamp,
    Bleed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LithophaneColorMode {
    Mono,
    Cmyk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LithophaneAttachmentSource {
    File {
        #[serde(rename = "imagePath", alias = "image_path")]
        image_path: String,
    },
    Param {
        #[serde(rename = "imageParam", alias = "image_param")]
        image_param: String,
    },
}

impl LithophaneAttachmentSource {
    pub fn image_path(&self) -> Option<&str> {
        match self {
            Self::File { image_path } => Some(image_path.as_str()),
            Self::Param { .. } => None,
        }
    }

    pub fn image_param(&self) -> Option<&str> {
        match self {
            Self::Param { image_param } => Some(image_param.as_str()),
            Self::File { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LithophanePlacement {
    #[serde(default = "default_lithophane_placement_mode")]
    pub mode: LithophanePlacementMode,
    #[serde(default = "default_lithophane_side")]
    pub side: LithophaneSide,
    #[serde(default = "default_lithophane_projection")]
    pub projection: ProjectionType,
    #[serde(default)]
    pub width_mm: f64,
    #[serde(default)]
    pub height_mm: f64,
    #[serde(default)]
    pub offset_x_mm: f64,
    #[serde(default)]
    pub offset_y_mm: f64,
    #[serde(default)]
    pub rotation_deg: f64,
    #[serde(default = "default_overflow_mode")]
    pub overflow_mode: OverflowMode,
    #[serde(default)]
    pub bleed_margin_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LithophaneRelief {
    #[serde(default = "default_lithophane_depth_mm")]
    pub depth_mm: f64,
    #[serde(default)]
    pub invert: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LithophaneColor {
    #[serde(default = "default_lithophane_color_mode")]
    pub mode: LithophaneColorMode,
    #[serde(default = "default_channel_thickness_mm")]
    pub channel_thickness_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LithophaneAttachment {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub source: LithophaneAttachmentSource,
    #[serde(default)]
    pub target_part_id: String,
    #[serde(default = "default_lithophane_placement")]
    pub placement: LithophanePlacement,
    #[serde(default = "default_lithophane_relief")]
    pub relief: LithophaneRelief,
    #[serde(default = "default_lithophane_color")]
    pub color: LithophaneColor,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PostProcessingSpec {
    #[serde(default)]
    pub displacement: Option<DisplacementSpec>,
    #[serde(default)]
    pub lithophane_attachments: Vec<LithophaneAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesignOutput {
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default = "default_version_name", alias = "version_name")]
    pub version_name: String,
    #[serde(default)]
    pub response: String,
    #[serde(default = "default_interaction_mode", alias = "interaction_mode")]
    pub interaction_mode: InteractionMode,
    #[serde(alias = "macro_code")]
    pub macro_code: String,
    #[serde(default = "default_macro_dialect", alias = "macro_dialect")]
    pub macro_dialect: MacroDialect,
    #[serde(default = "default_engine_kind", alias = "engine_kind")]
    pub engine_kind: EngineKind,
    #[serde(default = "default_source_language", alias = "source_language")]
    pub source_language: SourceLanguage,
    #[serde(default = "default_geometry_backend", alias = "geometry_backend")]
    pub geometry_backend: GeometryBackend,
    #[serde(default, alias = "ui_spec")]
    pub ui_spec: UiSpec,
    #[serde(default, alias = "initial_params")]
    pub initial_params: DesignParams,
    #[serde(default, alias = "post_processing")]
    pub post_processing: Option<PostProcessingSpec>,
}

fn default_title() -> String {
    "Untitled Design".to_string()
}

fn default_version_name() -> String {
    "V1".to_string()
}

fn default_interaction_mode() -> InteractionMode {
    InteractionMode::Design
}

fn default_lithophane_placement_mode() -> LithophanePlacementMode {
    LithophanePlacementMode::PartSidePatch
}

fn default_lithophane_side() -> LithophaneSide {
    LithophaneSide::Front
}

fn default_lithophane_projection() -> ProjectionType {
    ProjectionType::Auto
}

fn default_overflow_mode() -> OverflowMode {
    OverflowMode::Contain
}

fn default_lithophane_depth_mm() -> f64 {
    2.0
}

fn default_channel_thickness_mm() -> f64 {
    0.4
}

fn default_lithophane_color_mode() -> LithophaneColorMode {
    LithophaneColorMode::Mono
}

fn default_lithophane_placement() -> LithophanePlacement {
    LithophanePlacement {
        mode: default_lithophane_placement_mode(),
        side: default_lithophane_side(),
        projection: default_lithophane_projection(),
        width_mm: 0.0,
        height_mm: 0.0,
        offset_x_mm: 0.0,
        offset_y_mm: 0.0,
        rotation_deg: 0.0,
        overflow_mode: default_overflow_mode(),
        bleed_margin_mm: 0.0,
    }
}

fn default_lithophane_relief() -> LithophaneRelief {
    LithophaneRelief {
        depth_mm: default_lithophane_depth_mm(),
        invert: false,
    }
}

fn default_lithophane_color() -> LithophaneColor {
    LithophaneColor {
        mode: default_lithophane_color_mode(),
        channel_thickness_mm: default_channel_thickness_mm(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

impl std::str::FromStr for MessageRole {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            other => Err(AppError::validation(format!(
                "Unknown message role '{}'.",
                other
            ))),
        }
    }
}

impl ToSql for MessageRole {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for MessageRole {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        raw.parse()
            .map_err(|err: AppError| FromSqlError::Other(Box::new(err)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Pending,
    Working,
    Success,
    Error,
    Discarded,
}

impl MessageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Working => "working",
            Self::Success => "success",
            Self::Error => "error",
            Self::Discarded => "discarded",
        }
    }
}

impl std::str::FromStr for MessageStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "working" => Ok(Self::Working),
            "success" => Ok(Self::Success),
            "error" => Ok(Self::Error),
            "discarded" => Ok(Self::Discarded),
            other => Err(AppError::validation(format!(
                "Unknown message status '{}'.",
                other
            ))),
        }
    }
}

impl ToSql for MessageStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for MessageStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        raw.parse()
            .map_err(|err: AppError| FromSqlError::Other(Box::new(err)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MessageVisualKind {
    ConceptPreview,
}

impl MessageVisualKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConceptPreview => "conceptPreview",
        }
    }
}

impl std::str::FromStr for MessageVisualKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "conceptPreview" => Ok(Self::ConceptPreview),
            other => Err(AppError::validation(format!(
                "Unknown message visual kind '{}'.",
                other
            ))),
        }
    }
}

impl ToSql for MessageVisualKind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for MessageVisualKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        raw.parse()
            .map_err(|err: AppError| FromSqlError::Other(Box::new(err)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    #[serde(default)]
    pub output: Option<DesignOutput>,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
    #[serde(default)]
    pub artifact_bundle: Option<ArtifactBundle>,
    #[serde(default)]
    pub model_manifest: Option<ModelManifest>,
    #[serde(default)]
    pub agent_origin: Option<AgentOrigin>,
    #[serde(default, alias = "image_data")]
    pub image_data: Option<String>,
    #[serde(default, alias = "visual_kind")]
    pub visual_kind: Option<MessageVisualKind>,
    #[serde(default, alias = "attachment_images")]
    pub attachment_images: Vec<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThreadStatus {
    #[default]
    Active,
    Finalized,
}

impl ThreadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Finalized => "finalized",
        }
    }
}

impl std::str::FromStr for ThreadStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "finalized" => Ok(Self::Finalized),
            _ => Ok(Self::Active),
        }
    }
}

impl ToSql for ThreadStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for ThreadStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw = value.as_str()?;
        Ok(raw.parse().unwrap_or(ThreadStatus::Active))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub messages: Vec<Message>,
    #[serde(alias = "updated_at")]
    pub updated_at: u64,
    #[serde(default, alias = "genie_traits")]
    pub genie_traits: Option<GenieTraits>,
    #[serde(default, alias = "version_count")]
    pub version_count: usize,
    #[serde(default, alias = "pending_count")]
    pub pending_count: usize,
    #[serde(default, alias = "queued_count")]
    pub queued_count: usize,
    #[serde(default, alias = "error_count")]
    pub error_count: usize,
    #[serde(default, alias = "thread_status")]
    pub status: ThreadStatus,
    #[serde(default, alias = "finalized_at")]
    pub finalized_at: Option<u64>,
    #[serde(default, alias = "pending_confirm")]
    pub pending_confirm: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessagesPage {
    pub messages: Vec<Message>,
    pub next_before: Option<u64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadWindowState {
    pub visible: bool,
    #[serde(default)]
    pub minimized: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub z: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadWindowLayout {
    #[serde(default = "default_thread_window_layout_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_thread_window_layout_remember_layout")]
    pub remember_layout: bool,
    pub windows: HashMap<String, ThreadWindowState>,
}

fn default_thread_window_layout_schema_version() -> u32 {
    1
}

fn default_thread_window_layout_remember_layout() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReference {
    pub id: String,
    #[serde(alias = "thread_id")]
    pub thread_id: String,
    #[serde(default, alias = "source_message_id")]
    pub source_message_id: Option<String>,
    pub ordinal: i64,
    pub kind: String,
    pub name: String,
    pub content: String,
    pub summary: String,
    pub pinned: bool,
    #[serde(alias = "created_at")]
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    Cad,
}

impl AttachmentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Cad => "cad",
        }
    }
}

impl std::str::FromStr for AttachmentKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "image" => Ok(Self::Image),
            "cad" => Ok(Self::Cad),
            other => Err(AppError::validation(format!(
                "Unknown attachment kind '{}'.",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub path: String,
    pub name: String,
    pub explanation: String,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "data_url")]
    pub data_url: Option<String>,
    #[serde(alias = "type")]
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateOutput {
    pub design: DesignOutput,
    pub thread_id: String,
    pub message_id: String,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDesignOptions {
    #[serde(default)]
    pub question_mode: Option<bool>,
    #[serde(default)]
    pub follow_up_question: Option<String>,
    #[serde(default)]
    pub engine_kind: Option<EngineKind>,
    #[serde(default)]
    pub source_language: Option<SourceLanguage>,
    #[serde(default)]
    pub geometry_backend: Option<GeometryBackend>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitOutput {
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntentDecision {
    #[serde(alias = "intent_mode")]
    pub intent_mode: String,
    pub confidence: f32,
    pub response: String,
    #[serde(default)]
    pub final_response: Option<String>,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderVerification {
    /// true = model matches the prompt, no action needed
    pub passed: bool,
    /// human-readable description of what's wrong (empty when passed)
    pub issues: String,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
}

// ── Structural verification ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructuralVerificationResult {
    pub passed: bool,
    pub summary: String,
    pub issues: Vec<StructuralIssue>,
    pub metrics: StructuralMetrics,
    pub verifier_status: VerifierStatus,
    #[serde(default)]
    pub verifier_source: Option<VerifierSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifierSource {
    RustStructural,
    RustPlusBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructuralIssue {
    pub code: String,
    pub message: String,
    /// ID of the affected part, when the issue is part-specific.
    #[serde(default)]
    pub part_id: Option<String>,
    #[serde(default)]
    pub numeric_payload: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructuralMetrics {
    pub part_count: u32,
    #[serde(default)]
    pub preview_stl_size_bytes: Option<u64>,
    #[serde(default)]
    pub preview_stl_triangle_count: Option<u32>,
    #[serde(default)]
    pub preview_stl_component_count: Option<u32>,
    #[serde(default)]
    pub preview_stl_non_manifold_edge_count: Option<u32>,
    #[serde(default)]
    pub preview_stl_overhang_triangle_count: Option<u32>,
    #[serde(default)]
    pub preview_stl_overhang_ratio: Option<f64>,
    #[serde(default)]
    pub total_volume: Option<f64>,
    #[serde(default)]
    pub total_area: Option<f64>,
    #[serde(default)]
    pub bbox: Option<ManifestBounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifierStatus {
    Ok,
    OkRustOnly,
    OkWithBackend,
    SkippedUnavailable,
    SkippedBackendUnavailable,
}

// ── Visual (screenshot) verification ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VisualVerificationResult {
    pub passed: bool,
    pub summary: String,
    pub issues: Vec<VisualIssue>,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VisualIssue {
    pub category: VisualIssueCategory,
    pub description: String,
    #[serde(default)]
    pub part_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualIssueCategory {
    MissingPart,
    FloatingPart,
    ConnectorBroken,
    ReferenceMismatch,
    TopologyBroken,
    Other,
}

impl VisualIssueCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MissingPart => "missing_part",
            Self::FloatingPart => "floating_part",
            Self::ConnectorBroken => "connector_broken",
            Self::ReferenceMismatch => "reference_mismatch",
            Self::TopologyBroken => "topology_broken",
            Self::Other => "other",
        }
    }
}

// ── End structural verification ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSegment {
    pub stage: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
    #[serde(default)]
    pub segments: Vec<UsageSegment>,
}

impl UsageSummary {
    pub fn empty() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cached_input_tokens: 0,
            reasoning_tokens: 0,
            estimated_cost_usd: None,
            segments: Vec::new(),
        }
    }

    pub fn from_segment(segment: UsageSegment) -> Self {
        Self {
            input_tokens: segment.input_tokens,
            output_tokens: segment.output_tokens,
            total_tokens: segment.total_tokens,
            cached_input_tokens: segment.cached_input_tokens,
            reasoning_tokens: segment.reasoning_tokens,
            estimated_cost_usd: segment.estimated_cost_usd,
            segments: vec![segment],
        }
    }

    pub fn merge(&self, other: &Self) -> Self {
        let estimated_cost_usd = match (self.estimated_cost_usd, other.estimated_cost_usd) {
            (Some(left), Some(right)) => Some(left + right),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };

        let mut segments = self.segments.clone();
        segments.extend(other.segments.clone());

        Self {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            total_tokens: self.total_tokens + other.total_tokens,
            cached_input_tokens: self.cached_input_tokens + other.cached_input_tokens,
            reasoning_tokens: self.reasoning_tokens + other.reasoning_tokens,
            estimated_cost_usd,
            segments,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReply {
    pub thread_id: String,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LastDesignSnapshot {
    #[serde(default)]
    pub design: Option<DesignOutput>,
    #[serde(default, alias = "thread_id")]
    pub thread_id: Option<String>,
    #[serde(default, alias = "message_id")]
    pub message_id: Option<String>,
    #[serde(default, alias = "artifact_bundle")]
    pub artifact_bundle: Option<ArtifactBundle>,
    #[serde(default, alias = "model_manifest")]
    pub model_manifest: Option<ModelManifest>,
    #[serde(default, alias = "selected_part_id")]
    pub selected_part_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeletedMessage {
    pub id: String,
    #[serde(alias = "thread_id")]
    pub thread_id: String,
    pub thread_title: String,
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub output: Option<DesignOutput>,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
    #[serde(default)]
    pub artifact_bundle: Option<ArtifactBundle>,
    #[serde(default)]
    pub model_manifest: Option<ModelManifest>,
    #[serde(default)]
    pub agent_origin: Option<AgentOrigin>,
    pub timestamp: u64,
    #[serde(default, alias = "image_data")]
    pub image_data: Option<String>,
    #[serde(default, alias = "visual_kind")]
    pub visual_kind: Option<MessageVisualKind>,
    #[serde(default, alias = "attachment_images")]
    pub attachment_images: Vec<String>,
    pub deleted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedParamsResult {
    pub fields: Vec<UiField>,
    pub params: DesignParams,
}

pub const MODEL_RUNTIME_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelSourceKind {
    Generated,
    ImportedFcstd,
    ImportedStep,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ViewerAssetFormat {
    Stl,
    Gltf,
    Glb,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectionTargetKind {
    Part,
    Object,
    Group,
    Edge,
    Face,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum EnrichmentStatus {
    #[default]
    None,
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlPrimitiveKind {
    Number,
    Toggle,
    Choice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlRelationMode {
    Mirror,
    Scale,
    Offset,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlViewScope {
    Global,
    Part,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlViewSource {
    Generated,
    Inherited,
    Llm,
    Manual,
}

fn default_control_source() -> ControlViewSource {
    ControlViewSource::Generated
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MeasurementBasis {
    Outer,
    Inner,
    Wall,
    Clearance,
    Centerline,
    Pitch,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MeasurementAxis {
    X,
    Y,
    Z,
    Radial,
    Normal,
    Path,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MeasurementAnnotationSource {
    Generated,
    Llm,
    Manual,
    Api,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MeasurementGuideKind {
    Linear,
    Radial,
    Clearance,
    Pitch,
    Leader,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AdvisorySeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum AdvisoryCondition {
    #[default]
    Always,
    Below,
    Above,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewerAsset {
    pub part_id: String,
    pub node_id: String,
    pub object_name: String,
    pub label: String,
    pub path: String,
    pub format: ViewerAssetFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewerEdgePoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewerEdgeTarget {
    pub target_id: String,
    pub part_id: String,
    pub viewer_node_id: String,
    pub label: String,
    pub editable: bool,
    pub start: ViewerEdgePoint,
    pub end: ViewerEdgePoint,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewerFaceTarget {
    pub target_id: String,
    pub part_id: String,
    pub viewer_node_id: String,
    pub label: String,
    pub editable: bool,
    pub center: ViewerEdgePoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalloutAnchor {
    pub anchor_id: String,
    pub position: [f64; 3],
    #[serde(default)]
    pub normal: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementGuide {
    pub guide_id: String,
    pub kind: MeasurementGuideKind,
    #[serde(default)]
    pub anchor_ids: Vec<String>,
    #[serde(default)]
    pub label_anchor_id: Option<String>,
    #[serde(default)]
    pub target_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBundle {
    #[serde(default = "default_model_runtime_schema_version")]
    pub schema_version: u32,
    pub model_id: String,
    pub source_kind: ModelSourceKind,
    #[serde(default = "default_engine_kind")]
    pub engine_kind: EngineKind,
    #[serde(default = "default_source_language")]
    pub source_language: SourceLanguage,
    #[serde(default = "default_geometry_backend")]
    pub geometry_backend: GeometryBackend,
    pub content_hash: String,
    #[serde(default = "default_artifact_version")]
    pub artifact_version: u32,
    pub fcstd_path: String,
    pub manifest_path: String,
    #[serde(default)]
    pub macro_path: Option<String>,
    pub preview_stl_path: String,
    #[serde(default)]
    pub viewer_assets: Vec<ViewerAsset>,
    #[serde(default)]
    pub edge_targets: Vec<ViewerEdgeTarget>,
    #[serde(default)]
    pub face_targets: Vec<ViewerFaceTarget>,
    #[serde(default)]
    pub callout_anchors: Vec<CalloutAnchor>,
    #[serde(default)]
    pub measurement_guides: Vec<MeasurementGuide>,
    #[serde(default)]
    pub export_artifacts: Vec<ExportArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportArtifact {
    pub label: String,
    pub format: String,
    pub path: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportPartInput {
    pub label: String,
    pub path: String,
    #[serde(default)]
    pub object_name: Option<String>,
    #[serde(default)]
    pub part_id: Option<String>,
    #[serde(default)]
    pub display_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement_frame: Option<PortFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestBounds {
    pub x_min: f64,
    pub y_min: f64,
    pub z_min: f64,
    pub x_max: f64,
    pub y_max: f64,
    pub z_max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    pub document_name: String,
    pub document_label: String,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub object_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PartBinding {
    pub part_id: String,
    pub freecad_object_name: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub semantic_role: Option<String>,
    #[serde(default)]
    pub viewer_asset_path: Option<String>,
    #[serde(default)]
    pub viewer_node_ids: Vec<String>,
    #[serde(default)]
    pub parameter_keys: Vec<String>,
    pub editable: bool,
    #[serde(default)]
    pub bounds: Option<ManifestBounds>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub area: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParameterGroup {
    pub group_id: String,
    pub label: String,
    #[serde(default)]
    pub parameter_keys: Vec<String>,
    #[serde(default)]
    pub part_ids: Vec<String>,
    pub editable: bool,
    #[serde(default)]
    pub presentation: Option<String>,
    #[serde(default)]
    pub order: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectionTarget {
    #[serde(default)]
    pub target_id: Option<String>,
    pub part_id: String,
    pub viewer_node_id: String,
    pub label: String,
    pub kind: SelectionTargetKind,
    pub editable: bool,
    #[serde(default)]
    pub parameter_keys: Vec<String>,
    #[serde(default)]
    pub primitive_ids: Vec<String>,
    #[serde(default)]
    pub view_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementAnnotation {
    pub annotation_id: String,
    pub label: String,
    pub basis: MeasurementBasis,
    pub axis: MeasurementAxis,
    #[serde(default)]
    pub parameter_keys: Vec<String>,
    #[serde(default)]
    pub primitive_ids: Vec<String>,
    #[serde(default)]
    pub target_ids: Vec<String>,
    #[serde(default)]
    pub guide_id: Option<String>,
    #[serde(default)]
    pub explanation: Option<String>,
    #[serde(default)]
    pub formula_hint: Option<String>,
    pub source: MeasurementAnnotationSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentProposal {
    pub proposal_id: String,
    pub label: String,
    #[serde(default)]
    pub part_ids: Vec<String>,
    #[serde(default)]
    pub parameter_keys: Vec<String>,
    pub confidence: f32,
    pub status: EnrichmentStatus,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrimitiveBinding {
    pub parameter_key: String,
    #[serde(default = "default_primitive_binding_scale")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlPrimitive {
    pub primitive_id: String,
    pub label: String,
    pub kind: ControlPrimitiveKind,
    #[serde(default = "default_control_source")]
    pub source: ControlViewSource,
    #[serde(default)]
    pub part_ids: Vec<String>,
    #[serde(default)]
    pub bindings: Vec<PrimitiveBinding>,
    pub editable: bool,
    #[serde(default)]
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlRelation {
    pub relation_id: String,
    pub source_primitive_id: String,
    pub target_primitive_id: String,
    pub mode: ControlRelationMode,
    #[serde(default = "default_primitive_binding_scale")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlViewSection {
    pub section_id: String,
    pub label: String,
    #[serde(default)]
    pub primitive_ids: Vec<String>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlView {
    pub view_id: String,
    pub label: String,
    pub scope: ControlViewScope,
    #[serde(default)]
    pub part_ids: Vec<String>,
    #[serde(default)]
    pub primitive_ids: Vec<String>,
    #[serde(default)]
    pub sections: Vec<ControlViewSection>,
    #[serde(default, rename = "default")]
    pub is_default: bool,
    #[serde(default = "default_control_source")]
    pub source: ControlViewSource,
    #[serde(default)]
    pub status: EnrichmentStatus,
    #[serde(default)]
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Advisory {
    pub advisory_id: String,
    pub label: String,
    pub severity: AdvisorySeverity,
    #[serde(default)]
    pub primitive_ids: Vec<String>,
    #[serde(default)]
    pub view_ids: Vec<String>,
    pub message: String,
    #[serde(default)]
    pub condition: AdvisoryCondition,
    #[serde(default)]
    pub threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEnrichmentState {
    pub status: EnrichmentStatus,
    #[serde(default)]
    pub proposals: Vec<EnrichmentProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    #[serde(default = "default_model_runtime_schema_version")]
    pub schema_version: u32,
    pub model_id: String,
    pub source_kind: ModelSourceKind,
    #[serde(default = "default_engine_kind")]
    pub engine_kind: EngineKind,
    #[serde(default = "default_source_language")]
    pub source_language: SourceLanguage,
    #[serde(default = "default_geometry_backend")]
    pub geometry_backend: GeometryBackend,
    pub document: DocumentMetadata,
    #[serde(default)]
    pub parts: Vec<PartBinding>,
    #[serde(default)]
    pub parameter_groups: Vec<ParameterGroup>,
    #[serde(default)]
    pub control_primitives: Vec<ControlPrimitive>,
    #[serde(default)]
    pub control_relations: Vec<ControlRelation>,
    #[serde(default)]
    pub control_views: Vec<ControlView>,
    #[serde(default)]
    pub advisories: Vec<Advisory>,
    #[serde(default)]
    pub selection_targets: Vec<SelectionTarget>,
    #[serde(default)]
    pub measurement_annotations: Vec<MeasurementAnnotation>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default = "default_manifest_enrichment_state")]
    pub enrichment_state: ManifestEnrichmentState,
}

pub const COMPONENT_PACKAGE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PackageVisibility {
    Source,
    Compiled,
    Locked,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComponentParamKind {
    Number,
    Text,
    Boolean,
    Choice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Place,
    Mate,
    Join,
    Cut,
    Fuse,
    Mold,
    Blend,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum KeepoutVolumeKind {
    Box,
    Cylinder,
    Sphere,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", untagged)]
pub enum ComponentInterfaceValue {
    Number(f64),
    Text(String),
    Boolean(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PortFrame {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub z_axis: [f64; 3],
}

impl PortFrame {
    pub fn identity() -> Self {
        Self {
            origin: [0.0, 0.0, 0.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            z_axis: [0.0, 0.0, 1.0],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentParam {
    pub key: String,
    pub label: String,
    pub kind: ComponentParamKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentPort {
    pub port_id: String,
    pub type_id: String,
    #[serde(default)]
    pub target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<PortFrame>,
    #[serde(default)]
    pub params: BTreeMap<String, ComponentInterfaceValue>,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub compatible_with: Vec<String>,
    #[serde(default)]
    pub allowed_ops: Vec<OperationKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PortTypeDefinition {
    pub type_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub compatible_with: Vec<String>,
    #[serde(default)]
    pub allowed_ops: Vec<OperationKind>,
    #[serde(default)]
    pub params: Vec<ComponentParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct MatePortTypePair {
    pub a_type_id: String,
    pub b_type_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MateTypeDefinition {
    pub type_id: String,
    pub display_name: String,
    #[serde(default)]
    pub allowed_port_type_pairs: Vec<MatePortTypePair>,
    #[serde(default)]
    pub params: Vec<ComponentParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SketchView {
    Front,
    Side,
    Top,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SketchPrimitiveKind {
    Point,
    Line,
    Polyline,
    Spline,
    Arc,
    Circle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SketchConstraintKind {
    Closed,
    Horizontal,
    Vertical,
    Tangent,
    Equal,
    Symmetric,
    Dimension,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchPrimitive {
    pub primitive_id: String,
    pub kind: SketchPrimitiveKind,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchConstraint {
    pub constraint_id: String,
    pub kind: SketchConstraintKind,
    #[serde(default)]
    pub target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchDefinition {
    pub sketch_id: String,
    pub view: SketchView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<PortFrame>,
    #[serde(default)]
    pub primitives: Vec<SketchPrimitive>,
    #[serde(default)]
    pub constraints: Vec<SketchConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchDocument {
    pub document_id: String,
    #[serde(default)]
    pub sketches: Vec<SketchDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_sketch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SketchDraftOperationKind {
    Extrude,
    Revolve,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchDraftRequest {
    pub part_id: String,
    pub sketch: SketchDefinition,
    pub operation: SketchDraftOperationKind,
    pub amount: f64,
    #[serde(default)]
    pub symmetric: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchPreviewHullRequest {
    pub part_id: String,
    pub document: SketchDocument,
    pub fallback_depth: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateRequest {
    pub document: SketchDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateAcceptRequest {
    pub part_id: String,
    pub document: SketchDocument,
    pub solution_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchAcceptedBrepComponentPackageRequest {
    pub package_id: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub component_id: String,
    pub component_version: String,
    pub component_display_name: String,
    pub source_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_bundle: Option<ArtifactBundle>,
    pub document: SketchDocument,
    pub solution_id: String,
    #[serde(default)]
    pub port_types: Vec<PortTypeDefinition>,
    #[serde(default)]
    pub params: Vec<ComponentParam>,
    #[serde(default, alias = "ui_spec")]
    pub ui_spec: UiSpec,
    #[serde(default, alias = "initial_params")]
    pub initial_params: DesignParams,
    #[serde(default)]
    pub ports: Vec<ComponentPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBundleComponentPackageRequest {
    pub package_id: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub component_id: String,
    pub component_version: String,
    pub component_display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    pub artifact_bundle: ArtifactBundle,
    #[serde(default)]
    pub port_types: Vec<PortTypeDefinition>,
    #[serde(default)]
    pub params: Vec<ComponentParam>,
    #[serde(default, alias = "ui_spec")]
    pub ui_spec: UiSpec,
    #[serde(default, alias = "initial_params")]
    pub initial_params: DesignParams,
    #[serde(default)]
    pub ports: Vec<ComponentPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateVertex {
    pub vertex_id: String,
    pub point: [f64; 3],
    #[serde(default)]
    pub evidence_views: Vec<SketchView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateEdge {
    pub edge_id: String,
    pub a: String,
    pub b: String,
    #[serde(default)]
    pub support_views: Vec<SketchView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateGraph {
    #[serde(default)]
    pub vertices: Vec<SketchBrepCandidateVertex>,
    #[serde(default)]
    pub edges: Vec<SketchBrepCandidateEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateCell {
    pub cell_id: String,
    pub min: [f64; 3],
    pub max: [f64; 3],
    #[serde(default)]
    pub support_views: Vec<SketchView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SketchBrepCandidateSourceStrategy {
    CellUnion,
    FrontProfilePrism,
}

impl Default for SketchBrepCandidateSourceStrategy {
    fn default() -> Self {
        Self::CellUnion
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateSolution {
    pub solution_id: String,
    #[serde(default)]
    pub cell_ids: Vec<String>,
    pub score: f64,
    #[serde(default)]
    pub source_strategy: SketchBrepCandidateSourceStrategy,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateSearch {
    #[serde(default)]
    pub cells: Vec<SketchBrepCandidateCell>,
    #[serde(default)]
    pub rejected_cell_count: usize,
    #[serde(default)]
    pub solutions: Vec<SketchBrepCandidateSolution>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepProjectionValidation {
    pub passed: bool,
    #[serde(default)]
    pub issues: Vec<SketchValidationIssue>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateResponse {
    pub graph: SketchBrepCandidateGraph,
    pub search: SketchBrepCandidateSearch,
    pub validation: SketchBrepProjectionValidation,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchAcceptedBrepCandidateSource {
    pub draft_source: SketchDraftSource,
    pub candidate_response: SketchBrepCandidateResponse,
    pub accepted_solution: SketchBrepCandidateSolution,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchBrepCandidateAcceptResponse {
    pub draft_source: SketchDraftSource,
    pub artifact_bundle: ArtifactBundle,
    pub hidden_line_response: BrepHiddenLineProjectionResponse,
    pub candidate_response: SketchBrepCandidateResponse,
    pub accepted_solution: SketchBrepCandidateSolution,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrepHiddenLineProjectionRequest {
    pub artifact_bundle: ArtifactBundle,
    #[serde(default)]
    pub views: Vec<SketchView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch_document: Option<SketchDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrepProjectedEdge2d {
    pub edge_id: String,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    pub source_class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum BrepProjectedLoopRole {
    Outer,
    Hole,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrepProjectedLoop2d {
    pub loop_id: String,
    #[serde(default)]
    pub edge_ids: Vec<String>,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub role: BrepProjectedLoopRole,
    pub source_class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrepHiddenLineProjectionView {
    pub view: SketchView,
    pub direction: [f64; 3],
    #[serde(default)]
    pub visible_edges: Vec<BrepProjectedEdge2d>,
    #[serde(default)]
    pub hidden_edges: Vec<BrepProjectedEdge2d>,
    #[serde(default)]
    pub loops: Vec<BrepProjectedLoop2d>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrepHiddenLineProjectionResponse {
    pub model_id: String,
    pub source_artifact_path: String,
    #[serde(default)]
    pub views: Vec<BrepHiddenLineProjectionView>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<SketchBrepProjectionValidation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchDraftSource {
    pub source_language: SourceLanguage,
    pub geometry_backend: GeometryBackend,
    pub macro_dialect: MacroDialect,
    pub source: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchFeatureSuggestion {
    pub suggestion_id: String,
    pub sketch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primitive_id: Option<String>,
    pub part_id: String,
    pub operation: SketchDraftOperationKind,
    pub amount: f64,
    #[serde(default)]
    pub symmetric: bool,
    pub confidence: f64,
    pub reason: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchSuggestionRequest {
    pub document: SketchDocument,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchSuggestionResponse {
    #[serde(default)]
    pub suggestions: Vec<SketchFeatureSuggestion>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SketchValidationSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SketchValidationIssue {
    pub sketch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primitive_id: Option<String>,
    pub severity: SketchValidationSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SketchValidationResult {
    pub valid: bool,
    #[serde(default)]
    pub issues: Vec<SketchValidationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentKeepoutVolume {
    pub keepout_id: String,
    pub label: String,
    pub kind: KeepoutVolumeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<PortFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentFusionZone {
    pub zone_id: String,
    pub surface_ref: String,
    #[serde(default)]
    pub allowed_ops: Vec<OperationKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_blend_radius: Option<f64>,
    #[serde(default)]
    pub keepout_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDefinition {
    pub component_id: String,
    pub version: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_language: Option<SourceLanguage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_backend: Option<GeometryBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_dialect: Option<MacroDialect>,
    #[serde(default)]
    pub sketches: Vec<SketchDefinition>,
    #[serde(default)]
    pub keepouts: Vec<ComponentKeepoutVolume>,
    #[serde(default)]
    pub fusion_zones: Vec<ComponentFusionZone>,
    #[serde(default)]
    pub params: Vec<ComponentParam>,
    #[serde(default, alias = "ui_spec")]
    pub ui_spec: UiSpec,
    #[serde(default, alias = "initial_params")]
    pub initial_params: DesignParams,
    #[serde(default)]
    pub ports: Vec<ComponentPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyComponentRef {
    pub instance_id: String,
    pub component_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PortReference {
    pub instance_id: String,
    pub port_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyMate {
    pub mate_id: String,
    pub type_id: String,
    pub a: PortReference,
    pub b: PortReference,
    #[serde(default)]
    pub params: BTreeMap<String, ComponentInterfaceValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyOperation {
    pub operation_id: String,
    pub kind: OperationKind,
    #[serde(default)]
    pub target_instance_ids: Vec<String>,
    #[serde(default)]
    pub port_refs: Vec<PortReference>,
    #[serde(default)]
    pub params: BTreeMap<String, ComponentInterfaceValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AssemblyOutputMode {
    SeparateParts,
    JoinedAssembly,
    FusedSolid,
    MoldedSolid,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyOutput {
    pub mode: AssemblyOutputMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyDefinition {
    pub assembly_id: String,
    pub display_name: String,
    #[serde(default)]
    pub components: Vec<AssemblyComponentRef>,
    #[serde(default)]
    pub mates: Vec<AssemblyMate>,
    #[serde(default)]
    pub operations: Vec<AssemblyOperation>,
    pub output: AssemblyOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentPackage {
    #[serde(default = "default_component_package_schema_version")]
    pub schema_version: u32,
    pub package_id: String,
    pub version: String,
    pub display_name: String,
    pub visibility: PackageVisibility,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub port_types: Vec<PortTypeDefinition>,
    #[serde(default)]
    pub mate_types: Vec<MateTypeDefinition>,
    #[serde(default)]
    pub components: Vec<ComponentDefinition>,
    #[serde(default)]
    pub assemblies: Vec<AssemblyDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentPackageHeader {
    pub schema_version: u32,
    pub package_id: String,
    pub version: String,
    pub display_name: String,
    pub visibility: PackageVisibility,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub port_types: Vec<PortTypeDefinition>,
    #[serde(default)]
    pub mate_types: Vec<MateTypeDefinition>,
    #[serde(default)]
    pub components: Vec<ComponentHeader>,
    #[serde(default)]
    pub assemblies: Vec<AssemblyHeader>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledComponentPackage {
    pub header: ComponentPackageHeader,
    pub package_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledComponentSource {
    pub package_id: String,
    pub version: String,
    pub package_display_name: String,
    pub package_dir: String,
    pub component: ComponentDefinition,
    #[serde(default)]
    pub port_types: Vec<PortTypeDefinition>,
    #[serde(default)]
    pub mate_types: Vec<MateTypeDefinition>,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledComponentRuntime {
    pub installed_source: InstalledComponentSource,
    pub parameters: DesignParams,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledComponentControls {
    pub installed_source: InstalledComponentSource,
    pub parameters: DesignParams,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblyComponentControls {
    pub instance_id: String,
    pub component_id: String,
    pub parameters: DesignParams,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement_frame: Option<PortFrame>,
    pub installed_source: InstalledComponentSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblyControls {
    pub package_id: String,
    pub version: String,
    pub package_display_name: String,
    pub package_dir: String,
    pub assembly: AssemblyDefinition,
    #[serde(default)]
    pub port_types: Vec<PortTypeDefinition>,
    #[serde(default)]
    pub mate_types: Vec<MateTypeDefinition>,
    #[serde(default)]
    pub components: Vec<InstalledAssemblyComponentControls>,
    #[serde(default)]
    pub mate_results: Vec<InstalledAssemblyMateResult>,
    #[serde(default)]
    pub mates_solved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblyComponentSource {
    pub instance_id: String,
    pub component_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement_frame: Option<PortFrame>,
    pub installed_source: InstalledComponentSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblySource {
    pub package_id: String,
    pub version: String,
    pub package_display_name: String,
    pub package_dir: String,
    pub assembly: AssemblyDefinition,
    #[serde(default)]
    pub port_types: Vec<PortTypeDefinition>,
    #[serde(default)]
    pub mate_types: Vec<MateTypeDefinition>,
    #[serde(default)]
    pub components: Vec<InstalledAssemblyComponentSource>,
    #[serde(default)]
    pub mate_results: Vec<InstalledAssemblyMateResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblyMateResult {
    pub mate_id: String,
    pub solved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_clearance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_clearance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblyOperationResult {
    pub operation_id: String,
    pub applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default)]
    pub fusion_zone_ids_by_instance: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblyOutputRuntime {
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblyComponentRuntime {
    pub instance_id: String,
    pub component_id: String,
    pub parameters: DesignParams,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement_frame: Option<PortFrame>,
    pub runtime: InstalledComponentRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAssemblyRuntime {
    pub package_id: String,
    pub version: String,
    pub package_display_name: String,
    pub package_dir: String,
    pub assembly: AssemblyDefinition,
    #[serde(default)]
    pub port_types: Vec<PortTypeDefinition>,
    #[serde(default)]
    pub mate_types: Vec<MateTypeDefinition>,
    #[serde(default)]
    pub components: Vec<InstalledAssemblyComponentRuntime>,
    #[serde(default)]
    pub mate_results: Vec<InstalledAssemblyMateResult>,
    #[serde(default)]
    pub mates_solved: bool,
    #[serde(default)]
    pub operation_results: Vec<InstalledAssemblyOperationResult>,
    #[serde(default)]
    pub operations_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_runtime: Option<InstalledAssemblyOutputRuntime>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentHeader {
    pub component_id: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub params: Vec<ComponentParam>,
    #[serde(default, alias = "ui_spec")]
    pub ui_spec: UiSpec,
    #[serde(default, alias = "initial_params")]
    pub initial_params: DesignParams,
    #[serde(default)]
    pub ports: Vec<ComponentPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyHeader {
    pub assembly_id: String,
    pub display_name: String,
    pub component_count: usize,
    pub mate_count: usize,
    pub operation_count: usize,
    pub output: AssemblyOutput,
}

fn default_component_package_schema_version() -> u32 {
    COMPONENT_PACKAGE_SCHEMA_VERSION
}

fn default_model_runtime_schema_version() -> u32 {
    MODEL_RUNTIME_SCHEMA_VERSION
}

fn default_artifact_version() -> u32 {
    1
}

fn default_manifest_enrichment_state() -> ManifestEnrichmentState {
    ManifestEnrichmentState {
        status: EnrichmentStatus::None,
        proposals: Vec::new(),
    }
}

fn default_primitive_binding_scale() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FinalizeStatus {
    Success,
    Error,
    Discarded,
}

impl FinalizeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::Discarded => "discarded",
        }
    }
}

impl std::str::FromStr for FinalizeStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "success" => Ok(Self::Success),
            "error" => Ok(Self::Error),
            "discarded" => Ok(Self::Discarded),
            other => Err(AppError::validation(format!(
                "Unknown finalize status '{}'.",
                other
            ))),
        }
    }
}

pub fn validate_ui_spec(ui_spec: &UiSpec) -> AppResult<()> {
    let mut keys = HashSet::new();

    for field in &ui_spec.fields {
        let key = field.key().trim();
        if key.is_empty() {
            return Err(AppError::validation(
                "uiSpec fields must have a non-empty key.",
            ));
        }
        if !keys.insert(key.to_string()) {
            return Err(AppError::validation(format!(
                "uiSpec contains duplicate field key '{}'.",
                key
            )));
        }
        if field.label().trim().is_empty() {
            return Err(AppError::validation(format!(
                "uiSpec field '{}' must have a non-empty label.",
                key
            )));
        }

        match field {
            UiField::Range { min, max, step, .. } | UiField::Number { min, max, step, .. } => {
                if let (Some(min), Some(max)) = (min, max) {
                    if min > max {
                        return Err(AppError::validation(format!(
                            "uiSpec field '{}' has min greater than max.",
                            key
                        )));
                    }
                }
                if let Some(step) = step {
                    if *step <= 0.0 {
                        return Err(AppError::validation(format!(
                            "uiSpec field '{}' must have a positive step value.",
                            key
                        )));
                    }
                }
            }
            UiField::Select { options, .. } => {
                if options.is_empty() {
                    return Err(AppError::validation(format!(
                        "uiSpec select field '{}' must define at least one option.",
                        key
                    )));
                }
            }
            UiField::Checkbox { .. } | UiField::Image { .. } => {}
        }
    }

    let field_map: HashMap<&str, &UiField> = ui_spec
        .fields
        .iter()
        .map(|field| (field.key(), field))
        .collect();

    for field in &ui_spec.fields {
        match field {
            UiField::Range {
                key,
                min_from,
                max_from,
                ..
            }
            | UiField::Number {
                key,
                min_from,
                max_from,
                ..
            } => {
                for dependency in [min_from.as_deref(), max_from.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    let Some(target) = field_map.get(dependency) else {
                        return Err(AppError::validation(format!(
                            "uiSpec field '{}' references unknown dependency '{}'.",
                            key, dependency
                        )));
                    };
                    if !target.is_numeric() {
                        return Err(AppError::validation(format!(
                            "uiSpec field '{}' can only depend on numeric fields, but '{}' is not numeric.",
                            key, dependency
                        )));
                    }
                }
            }
            UiField::Select { .. } | UiField::Checkbox { .. } | UiField::Image { .. } => {}
        }
    }

    Ok(())
}

pub fn validate_design_params(params: &DesignParams, ui_spec: &UiSpec) -> AppResult<()> {
    let fields: HashMap<&str, &UiField> = ui_spec
        .fields
        .iter()
        .map(|field| (field.key(), field))
        .collect();

    for field in &ui_spec.fields {
        let Some(value) = params.get(field.key()) else {
            if matches!(field, UiField::Image { .. }) {
                continue;
            }
            return Err(AppError::validation(format!(
                "initialParams is missing '{}'.",
                field.key()
            )));
        };
        field.validate_value(value)?;
    }

    for key in params.keys() {
        if !fields.contains_key(key.as_str()) {
            return Err(AppError::validation(format!(
                "initialParams contains undeclared key '{}'.",
                key
            )));
        }
    }

    Ok(())
}

fn fallback_field_label(key: &str) -> String {
    let label = key.replace(['_', '-'], " ").trim().to_string();
    if label.is_empty() {
        key.to_string()
    } else {
        label
    }
}

pub fn reconcile_post_processing_controls(
    ui_spec: &UiSpec,
    params: &DesignParams,
    post_processing: Option<&PostProcessingSpec>,
) -> (UiSpec, DesignParams) {
    let Some(normalized) = normalize_post_processing_spec(post_processing.cloned()) else {
        return (ui_spec.clone(), params.clone());
    };

    let mut next_ui_spec = ui_spec.clone();
    let mut next_params = params.clone();

    let mut insert_missing_field = |image_key: &str| {
        if !next_ui_spec
            .fields
            .iter()
            .any(|field| field.key() == image_key)
        {
            next_ui_spec.fields.insert(
                0,
                UiField::Image {
                    key: image_key.to_string(),
                    label: fallback_field_label(image_key),
                    frozen: false,
                },
            );
        }
        next_params
            .entry(image_key.to_string())
            .or_insert_with(|| ParamValue::String(String::new()));
    };

    if let Some(displacement) = normalized.displacement.as_ref() {
        let image_key = displacement.image_param.trim();
        if !image_key.is_empty() {
            insert_missing_field(image_key);
        }
    }

    for attachment in &normalized.lithophane_attachments {
        if let Some(image_key) = attachment.source.image_param() {
            let image_key = image_key.trim();
            if !image_key.is_empty() {
                insert_missing_field(image_key);
            }
        }
    }

    (next_ui_spec, next_params)
}

fn validate_post_processing_controls(
    ui_spec: &UiSpec,
    post_processing: Option<&PostProcessingSpec>,
) -> AppResult<()> {
    let Some(normalized) = normalize_post_processing_spec(post_processing.cloned()) else {
        return Ok(());
    };

    if let Some(displacement) = normalized.displacement.as_ref() {
        validate_post_processing_image_field(
            ui_spec,
            displacement.image_param.as_str(),
            "displacement",
        )?;
    }

    let mut seen_ids = HashSet::new();
    for attachment in &normalized.lithophane_attachments {
        let attachment_id = attachment.id.trim();
        if attachment_id.is_empty() {
            return Err(AppError::validation(
                "postProcessing lithophaneAttachments must include a non-empty id.",
            ));
        }
        if !seen_ids.insert(attachment_id.to_string()) {
            return Err(AppError::validation(format!(
                "postProcessing lithophane attachment '{}' is duplicated.",
                attachment_id
            )));
        }
        if attachment.relief.depth_mm <= 0.0 {
            return Err(AppError::validation(format!(
                "postProcessing lithophane attachment '{}' must have depthMm > 0.",
                attachment_id
            )));
        }
        if attachment.color.channel_thickness_mm <= 0.0 {
            return Err(AppError::validation(format!(
                "postProcessing lithophane attachment '{}' must have channelThicknessMm > 0.",
                attachment_id
            )));
        }
        if matches!(attachment.color.mode, LithophaneColorMode::Cmyk)
            && !matches!(attachment.placement.projection, ProjectionType::Planar)
        {
            return Err(AppError::validation(format!(
                "postProcessing lithophane attachment '{}' only supports CMYK with planar projection.",
                attachment_id
            )));
        }
        if let Some(image_key) = attachment.source.image_param() {
            validate_post_processing_image_field(ui_spec, image_key, "lithophane attachment")?;
        }
    }

    Ok(())
}

fn validate_post_processing_image_field(
    ui_spec: &UiSpec,
    image_key: &str,
    label: &str,
) -> AppResult<()> {
    let image_key = image_key.trim();
    if image_key.is_empty() {
        return Err(AppError::validation(format!(
            "postProcessing {} must reference a non-empty imageParam.",
            label
        )));
    }
    let Some(field) = ui_spec.fields.iter().find(|field| field.key() == image_key) else {
        return Err(AppError::validation(format!(
            "postProcessing {} imageParam '{}' must reference a uiSpec field.",
            label, image_key
        )));
    };
    if !matches!(field, UiField::Image { .. }) {
        return Err(AppError::validation(format!(
            "postProcessing {} imageParam '{}' must reference a uiSpec image field.",
            label, image_key
        )));
    }
    Ok(())
}

pub fn validate_design_output(output: &DesignOutput) -> AppResult<()> {
    validate_ui_spec(&output.ui_spec)?;
    validate_post_processing_controls(&output.ui_spec, output.post_processing.as_ref())?;
    validate_design_params(&output.initial_params, &output.ui_spec)?;
    Ok(())
}

pub fn validate_artifact_bundle(bundle: &ArtifactBundle) -> AppResult<()> {
    let mut anchor_ids = HashSet::new();
    for anchor in &bundle.callout_anchors {
        if anchor.anchor_id.trim().is_empty() {
            return Err(AppError::validation(
                "callout anchors must include a non-empty anchorId.",
            ));
        }
        if !anchor_ids.insert(anchor.anchor_id.as_str()) {
            return Err(AppError::validation(format!(
                "callout anchor '{}' is duplicated.",
                anchor.anchor_id
            )));
        }
    }

    let mut guide_ids = HashSet::new();
    for guide in &bundle.measurement_guides {
        if guide.guide_id.trim().is_empty() {
            return Err(AppError::validation(
                "measurement guides must include a non-empty guideId.",
            ));
        }
        if !guide_ids.insert(guide.guide_id.as_str()) {
            return Err(AppError::validation(format!(
                "measurement guide '{}' is duplicated.",
                guide.guide_id
            )));
        }
        if guide.anchor_ids.is_empty() {
            return Err(AppError::validation(format!(
                "measurement guide '{}' must include at least one anchorId.",
                guide.guide_id
            )));
        }
        for anchor_id in &guide.anchor_ids {
            if !anchor_ids.contains(anchor_id.as_str()) {
                return Err(AppError::validation(format!(
                    "measurement guide '{}' references unknown anchorId '{}'.",
                    guide.guide_id, anchor_id
                )));
            }
        }
        if let Some(label_anchor_id) = guide.label_anchor_id.as_deref() {
            if !anchor_ids.contains(label_anchor_id) {
                return Err(AppError::validation(format!(
                    "measurement guide '{}' references unknown labelAnchorId '{}'.",
                    guide.guide_id, label_anchor_id
                )));
            }
        }
    }

    for export_artifact in &bundle.export_artifacts {
        if export_artifact.label.trim().is_empty() {
            return Err(AppError::validation(
                "export artifacts must include a non-empty label.",
            ));
        }
        if export_artifact.format.trim().is_empty() {
            return Err(AppError::validation(
                "export artifacts must include a non-empty format.",
            ));
        }
        if export_artifact.path.trim().is_empty() {
            return Err(AppError::validation(
                "export artifacts must include a non-empty path.",
            ));
        }
        if export_artifact.role.trim().is_empty() {
            return Err(AppError::validation(
                "export artifacts must include a non-empty role.",
            ));
        }
    }

    Ok(())
}

pub fn validate_model_manifest(manifest: &ModelManifest) -> AppResult<()> {
    if manifest.schema_version == 0 {
        return Err(AppError::validation(
            "model manifest schemaVersion must be greater than 0.",
        ));
    }

    if manifest.model_id.trim().is_empty() {
        return Err(AppError::validation(
            "model manifest must include a non-empty modelId.",
        ));
    }

    let mut part_ids = HashSet::new();
    let mut viewer_node_ids = HashSet::new();

    for part in &manifest.parts {
        if part.part_id.trim().is_empty() {
            return Err(AppError::validation(
                "model manifest partIds must be non-empty.",
            ));
        }
        if !part_ids.insert(part.part_id.as_str()) {
            return Err(AppError::validation(format!(
                "model manifest contains duplicate partId '{}'.",
                part.part_id
            )));
        }
        if part.freecad_object_name.trim().is_empty() {
            return Err(AppError::validation(format!(
                "part '{}' is missing freecadObjectName.",
                part.part_id
            )));
        }
        for node_id in &part.viewer_node_ids {
            if node_id.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "part '{}' contains an empty viewer node id.",
                    part.part_id
                )));
            }
            viewer_node_ids.insert(node_id.as_str());
        }
    }

    for group in &manifest.parameter_groups {
        if group.group_id.trim().is_empty() {
            return Err(AppError::validation(
                "model manifest parameterGroups must include non-empty groupId values.",
            ));
        }
        if let Some(presentation) = group.presentation.as_deref() {
            if !matches!(presentation, "primary" | "advanced") {
                return Err(AppError::validation(format!(
                    "parameter group '{}' has unsupported presentation '{}'.",
                    group.group_id, presentation
                )));
            }
        }
        for part_id in &group.part_ids {
            if !part_ids.contains(part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "parameter group '{}' references unknown partId '{}'.",
                    group.group_id, part_id
                )));
            }
        }
    }

    let mut known_parameter_keys = HashSet::new();
    for part in &manifest.parts {
        for key in &part.parameter_keys {
            known_parameter_keys.insert(key.as_str());
        }
    }
    for group in &manifest.parameter_groups {
        for key in &group.parameter_keys {
            known_parameter_keys.insert(key.as_str());
        }
    }

    let mut primitive_ids = HashSet::new();
    let mut view_ids = HashSet::new();
    let mut relation_ids = HashSet::new();

    for primitive in &manifest.control_primitives {
        if primitive.primitive_id.trim().is_empty() {
            return Err(AppError::validation(
                "control primitives must include a non-empty primitiveId.",
            ));
        }
        if !primitive_ids.insert(primitive.primitive_id.as_str()) {
            return Err(AppError::validation(format!(
                "control primitive '{}' is duplicated.",
                primitive.primitive_id
            )));
        }
        if primitive.label.trim().is_empty() {
            return Err(AppError::validation(format!(
                "control primitive '{}' must include a non-empty label.",
                primitive.primitive_id
            )));
        }
        if primitive.bindings.is_empty() {
            return Err(AppError::validation(format!(
                "control primitive '{}' must include at least one binding.",
                primitive.primitive_id
            )));
        }
        for part_id in &primitive.part_ids {
            if !part_ids.contains(part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "control primitive '{}' references unknown partId '{}'.",
                    primitive.primitive_id, part_id
                )));
            }
        }
        for binding in &primitive.bindings {
            if binding.parameter_key.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "control primitive '{}' contains a binding with an empty parameterKey.",
                    primitive.primitive_id
                )));
            }
            known_parameter_keys.insert(binding.parameter_key.as_str());
        }
    }

    for view in &manifest.control_views {
        if view.view_id.trim().is_empty() {
            return Err(AppError::validation(
                "control views must include a non-empty viewId.",
            ));
        }
        if !view_ids.insert(view.view_id.as_str()) {
            return Err(AppError::validation(format!(
                "control view '{}' is duplicated.",
                view.view_id
            )));
        }
        if view.label.trim().is_empty() {
            return Err(AppError::validation(format!(
                "control view '{}' must include a non-empty label.",
                view.view_id
            )));
        }
        for part_id in &view.part_ids {
            if !part_ids.contains(part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "control view '{}' references unknown partId '{}'.",
                    view.view_id, part_id
                )));
            }
        }
        for primitive_id in &view.primitive_ids {
            if !primitive_ids.contains(primitive_id.as_str()) {
                return Err(AppError::validation(format!(
                    "control view '{}' references unknown primitiveId '{}'.",
                    view.view_id, primitive_id
                )));
            }
        }
        for section in &view.sections {
            if section.section_id.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "control view '{}' contains a section with an empty sectionId.",
                    view.view_id
                )));
            }
            for primitive_id in &section.primitive_ids {
                if !primitive_ids.contains(primitive_id.as_str()) {
                    return Err(AppError::validation(format!(
                        "control view '{}' section '{}' references unknown primitiveId '{}'.",
                        view.view_id, section.section_id, primitive_id
                    )));
                }
            }
        }
    }

    for relation in &manifest.control_relations {
        if relation.relation_id.trim().is_empty() {
            return Err(AppError::validation(
                "control relations must include a non-empty relationId.",
            ));
        }
        if !relation_ids.insert(relation.relation_id.as_str()) {
            return Err(AppError::validation(format!(
                "control relation '{}' is duplicated.",
                relation.relation_id
            )));
        }
        if !primitive_ids.contains(relation.source_primitive_id.as_str()) {
            return Err(AppError::validation(format!(
                "control relation '{}' references unknown source primitive '{}'.",
                relation.relation_id, relation.source_primitive_id
            )));
        }
        if !primitive_ids.contains(relation.target_primitive_id.as_str()) {
            return Err(AppError::validation(format!(
                "control relation '{}' references unknown target primitive '{}'.",
                relation.relation_id, relation.target_primitive_id
            )));
        }
        if relation.source_primitive_id == relation.target_primitive_id {
            return Err(AppError::validation(format!(
                "control relation '{}' cannot target the same primitive as its source.",
                relation.relation_id
            )));
        }
    }

    let mut selection_target_ids = HashSet::new();
    for target in &manifest.selection_targets {
        if let Some(target_id) = target.target_id.as_deref() {
            if target_id.trim().is_empty() {
                return Err(AppError::validation(
                    "selection targets with targetId must use a non-empty value.",
                ));
            }
            if !selection_target_ids.insert(target_id) {
                return Err(AppError::validation(format!(
                    "selection target '{}' is duplicated.",
                    target_id
                )));
            }
        }
        if !part_ids.contains(target.part_id.as_str()) {
            return Err(AppError::validation(format!(
                "selection target '{}' references unknown partId '{}'.",
                target.viewer_node_id, target.part_id
            )));
        }
        if !viewer_node_ids.contains(target.viewer_node_id.as_str()) {
            return Err(AppError::validation(format!(
                "selection target '{}' references an unknown viewer node id.",
                target.viewer_node_id
            )));
        }
        for parameter_key in &target.parameter_keys {
            if !known_parameter_keys.contains(parameter_key.as_str()) {
                return Err(AppError::validation(format!(
                    "selection target '{}' references unknown parameterKey '{}'.",
                    target
                        .target_id
                        .as_deref()
                        .unwrap_or(target.viewer_node_id.as_str()),
                    parameter_key
                )));
            }
        }
        for primitive_id in &target.primitive_ids {
            if !primitive_ids.contains(primitive_id.as_str()) {
                return Err(AppError::validation(format!(
                    "selection target '{}' references unknown primitiveId '{}'.",
                    target
                        .target_id
                        .as_deref()
                        .unwrap_or(target.viewer_node_id.as_str()),
                    primitive_id
                )));
            }
        }
        for view_id in &target.view_ids {
            if !view_ids.contains(view_id.as_str()) {
                return Err(AppError::validation(format!(
                    "selection target '{}' references unknown viewId '{}'.",
                    target
                        .target_id
                        .as_deref()
                        .unwrap_or(target.viewer_node_id.as_str()),
                    view_id
                )));
            }
        }
    }

    let mut measurement_annotation_ids = HashSet::new();
    for annotation in &manifest.measurement_annotations {
        if annotation.annotation_id.trim().is_empty() {
            return Err(AppError::validation(
                "measurement annotations must include a non-empty annotationId.",
            ));
        }
        if !measurement_annotation_ids.insert(annotation.annotation_id.as_str()) {
            return Err(AppError::validation(format!(
                "measurement annotation '{}' is duplicated.",
                annotation.annotation_id
            )));
        }
        if annotation.label.trim().is_empty() {
            return Err(AppError::validation(format!(
                "measurement annotation '{}' must include a non-empty label.",
                annotation.annotation_id
            )));
        }
        if annotation.parameter_keys.is_empty()
            && annotation.primitive_ids.is_empty()
            && annotation.target_ids.is_empty()
        {
            return Err(AppError::validation(format!(
                "measurement annotation '{}' must reference at least one parameterKey, primitiveId, or targetId.",
                annotation.annotation_id
            )));
        }
        for parameter_key in &annotation.parameter_keys {
            if !known_parameter_keys.contains(parameter_key.as_str()) {
                return Err(AppError::validation(format!(
                    "measurement annotation '{}' references unknown parameterKey '{}'.",
                    annotation.annotation_id, parameter_key
                )));
            }
        }
        for primitive_id in &annotation.primitive_ids {
            if !primitive_ids.contains(primitive_id.as_str()) {
                return Err(AppError::validation(format!(
                    "measurement annotation '{}' references unknown primitiveId '{}'.",
                    annotation.annotation_id, primitive_id
                )));
            }
        }
        for target_id in &annotation.target_ids {
            if !selection_target_ids.contains(target_id.as_str()) {
                return Err(AppError::validation(format!(
                    "measurement annotation '{}' references unknown targetId '{}'.",
                    annotation.annotation_id, target_id
                )));
            }
        }
    }

    for proposal in &manifest.enrichment_state.proposals {
        if proposal.proposal_id.trim().is_empty() {
            return Err(AppError::validation(
                "enrichment proposals must include a non-empty proposalId.",
            ));
        }
        for part_id in &proposal.part_ids {
            if !part_ids.contains(part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "enrichment proposal '{}' references unknown partId '{}'.",
                    proposal.proposal_id, part_id
                )));
            }
        }
    }

    for advisory in &manifest.advisories {
        if advisory.advisory_id.trim().is_empty() {
            return Err(AppError::validation(
                "advisories must include a non-empty advisoryId.",
            ));
        }
        if advisory.label.trim().is_empty() {
            return Err(AppError::validation(format!(
                "advisory '{}' must include a non-empty label.",
                advisory.advisory_id
            )));
        }
        if advisory.message.trim().is_empty() {
            return Err(AppError::validation(format!(
                "advisory '{}' must include a non-empty message.",
                advisory.advisory_id
            )));
        }
        for primitive_id in &advisory.primitive_ids {
            if !primitive_ids.contains(primitive_id.as_str()) {
                return Err(AppError::validation(format!(
                    "advisory '{}' references unknown primitiveId '{}'.",
                    advisory.advisory_id, primitive_id
                )));
            }
        }
        for view_id in &advisory.view_ids {
            if !view_ids.contains(view_id.as_str()) {
                return Err(AppError::validation(format!(
                    "advisory '{}' references unknown viewId '{}'.",
                    advisory.advisory_id, view_id
                )));
            }
        }
    }

    Ok(())
}

pub fn validate_model_runtime_bundle(
    manifest: &ModelManifest,
    bundle: &ArtifactBundle,
) -> AppResult<()> {
    validate_model_manifest(manifest)?;
    validate_artifact_bundle(bundle)?;

    if bundle.model_id != manifest.model_id {
        return Err(AppError::validation(
            "Model manifest does not match artifact bundle model id.",
        ));
    }

    let selection_target_ids = manifest
        .selection_targets
        .iter()
        .filter_map(|target| target.target_id.as_deref())
        .collect::<HashSet<_>>();
    let guide_ids = bundle
        .measurement_guides
        .iter()
        .map(|guide| guide.guide_id.as_str())
        .collect::<HashSet<_>>();

    for guide in &bundle.measurement_guides {
        for target_id in &guide.target_ids {
            if !selection_target_ids.contains(target_id.as_str()) {
                return Err(AppError::validation(format!(
                    "measurement guide '{}' references unknown targetId '{}'.",
                    guide.guide_id, target_id
                )));
            }
        }
    }

    for edge_target in &bundle.edge_targets {
        if !selection_target_ids.contains(edge_target.target_id.as_str()) {
            return Err(AppError::validation(format!(
                "edge target '{}' references unknown targetId '{}'.",
                edge_target.label, edge_target.target_id
            )));
        }
    }

    for face_target in &bundle.face_targets {
        if !selection_target_ids.contains(face_target.target_id.as_str()) {
            return Err(AppError::validation(format!(
                "face target '{}' references unknown targetId '{}'.",
                face_target.label, face_target.target_id
            )));
        }
    }

    for annotation in &manifest.measurement_annotations {
        if let Some(guide_id) = annotation.guide_id.as_deref() {
            if !guide_ids.contains(guide_id) {
                return Err(AppError::validation(format!(
                    "measurement annotation '{}' references unknown guideId '{}'.",
                    annotation.annotation_id, guide_id
                )));
            }
        }
    }

    Ok(())
}

pub fn validate_component_package(package: &ComponentPackage) -> AppResult<()> {
    if package.schema_version == 0 {
        return Err(AppError::validation(
            "component package schemaVersion must be greater than 0.",
        ));
    }
    require_non_empty(
        &package.package_id,
        "component package must include a non-empty packageId.",
    )?;
    require_non_empty(
        &package.version,
        "component package must include a non-empty version.",
    )?;
    require_non_empty(
        &package.display_name,
        "component package must include a non-empty displayName.",
    )?;

    for tag in &package.tags {
        if tag.trim().is_empty() {
            return Err(AppError::validation(
                "component package tags must be non-empty.",
            ));
        }
    }

    let mut port_type_ids = HashSet::new();
    for port_type in &package.port_types {
        validate_port_type_definition(port_type)?;
        if !port_type_ids.insert(port_type.type_id.as_str()) {
            return Err(AppError::validation(format!(
                "component package contains duplicate port typeId '{}'.",
                port_type.type_id
            )));
        }
    }

    let mut mate_type_ids = HashSet::new();
    let mut mate_types_by_id = HashMap::new();
    for mate_type in &package.mate_types {
        validate_mate_type_definition(mate_type)?;
        if !mate_type_ids.insert(mate_type.type_id.as_str()) {
            return Err(AppError::validation(format!(
                "component package contains duplicate mate typeId '{}'.",
                mate_type.type_id
            )));
        }
        mate_types_by_id.insert(mate_type.type_id.as_str(), mate_type);
    }

    if package.components.is_empty() {
        return Err(AppError::validation(
            "component package must include at least one component.",
        ));
    }

    let mut component_ids = HashSet::new();
    let mut components_by_id = HashMap::new();
    for component in &package.components {
        validate_component_definition(component)?;
        if !component_ids.insert(component.component_id.as_str()) {
            return Err(AppError::validation(format!(
                "component package contains duplicate componentId '{}'.",
                component.component_id
            )));
        }
        components_by_id.insert(component.component_id.as_str(), component);
    }

    let mut assembly_ids = HashSet::new();
    for assembly in &package.assemblies {
        validate_assembly_definition(assembly, &components_by_id, &mate_types_by_id)?;
        if !assembly_ids.insert(assembly.assembly_id.as_str()) {
            return Err(AppError::validation(format!(
                "component package contains duplicate assemblyId '{}'.",
                assembly.assembly_id
            )));
        }
    }

    Ok(())
}

pub fn component_package_header(package: &ComponentPackage) -> AppResult<ComponentPackageHeader> {
    validate_component_package(package)?;
    Ok(ComponentPackageHeader {
        schema_version: package.schema_version,
        package_id: package.package_id.clone(),
        version: package.version.clone(),
        display_name: package.display_name.clone(),
        visibility: package.visibility.clone(),
        tags: package.tags.clone(),
        port_types: package.port_types.clone(),
        mate_types: package.mate_types.clone(),
        components: package
            .components
            .iter()
            .map(|component| ComponentHeader {
                component_id: component.component_id.clone(),
                version: component.version.clone(),
                display_name: component.display_name.clone(),
                params: component.params.clone(),
                ui_spec: component.ui_spec.clone(),
                initial_params: component.initial_params.clone(),
                ports: component.ports.clone(),
            })
            .collect(),
        assemblies: package
            .assemblies
            .iter()
            .map(|assembly| AssemblyHeader {
                assembly_id: assembly.assembly_id.clone(),
                display_name: assembly.display_name.clone(),
                component_count: assembly.components.len(),
                mate_count: assembly.mates.len(),
                operation_count: assembly.operations.len(),
                output: assembly.output.clone(),
            })
            .collect(),
    })
}

pub fn validate_component_package_header(header: &ComponentPackageHeader) -> AppResult<()> {
    if header.schema_version == 0 {
        return Err(AppError::validation(
            "component package header schemaVersion must be greater than 0.",
        ));
    }
    require_non_empty(
        &header.package_id,
        "component package header must include a non-empty packageId.",
    )?;
    require_non_empty(
        &header.version,
        "component package header must include a non-empty version.",
    )?;
    require_non_empty(
        &header.display_name,
        "component package header must include a non-empty displayName.",
    )?;

    let mut port_type_ids = HashSet::new();
    for port_type in &header.port_types {
        validate_port_type_definition(port_type)?;
        if !port_type_ids.insert(port_type.type_id.as_str()) {
            return Err(AppError::validation(format!(
                "component package header contains duplicate port typeId '{}'.",
                port_type.type_id
            )));
        }
    }

    let mut mate_type_ids = HashSet::new();
    for mate_type in &header.mate_types {
        validate_mate_type_definition(mate_type)?;
        if !mate_type_ids.insert(mate_type.type_id.as_str()) {
            return Err(AppError::validation(format!(
                "component package header contains duplicate mate typeId '{}'.",
                mate_type.type_id
            )));
        }
    }

    let mut component_ids = HashSet::new();
    for component in &header.components {
        require_non_empty(
            &component.component_id,
            "component package header components must include non-empty componentId values.",
        )?;
        if !component_ids.insert(component.component_id.as_str()) {
            return Err(AppError::validation(format!(
                "component package header contains duplicate componentId '{}'.",
                component.component_id
            )));
        }
        require_non_empty(
            &component.version,
            &format!(
                "component package header component '{}' must include a non-empty version.",
                component.component_id
            ),
        )?;
        require_non_empty(
            &component.display_name,
            &format!(
                "component package header component '{}' must include a non-empty displayName.",
                component.component_id
            ),
        )?;
        let mut param_keys = HashSet::new();
        for param in &component.params {
            require_non_empty(
                &param.key,
                &format!(
                    "component package header component '{}' params must include non-empty keys.",
                    component.component_id
                ),
            )?;
            if !param_keys.insert(param.key.as_str()) {
                return Err(AppError::validation(format!(
                    "component package header component '{}' contains duplicate param key '{}'.",
                    component.component_id, param.key
                )));
            }
        }
        validate_ui_spec(&component.ui_spec)?;
        validate_design_params(&component.initial_params, &component.ui_spec)?;
        let mut port_ids = HashSet::new();
        for port in &component.ports {
            validate_component_port(&component.component_id, port)?;
            if !port_ids.insert(port.port_id.as_str()) {
                return Err(AppError::validation(format!(
                    "component package header component '{}' contains duplicate portId '{}'.",
                    component.component_id, port.port_id
                )));
            }
        }
    }

    let mut assembly_ids = HashSet::new();
    for assembly in &header.assemblies {
        require_non_empty(
            &assembly.assembly_id,
            "component package header assemblies must include non-empty assemblyId values.",
        )?;
        if !assembly_ids.insert(assembly.assembly_id.as_str()) {
            return Err(AppError::validation(format!(
                "component package header contains duplicate assemblyId '{}'.",
                assembly.assembly_id
            )));
        }
        require_non_empty(
            &assembly.display_name,
            &format!(
                "component package header assembly '{}' must include a non-empty displayName.",
                assembly.assembly_id
            ),
        )?;
    }

    Ok(())
}

fn validate_port_type_definition(port_type: &PortTypeDefinition) -> AppResult<()> {
    require_non_empty(
        &port_type.type_id,
        "port type definitions must include a non-empty typeId.",
    )?;
    require_non_empty(
        &port_type.display_name,
        &format!(
            "port type '{}' must include a non-empty displayName.",
            port_type.type_id
        ),
    )?;
    if let Some(base) = port_type.base.as_deref() {
        if base.trim().is_empty() {
            return Err(AppError::validation(format!(
                "port type '{}' base must be non-empty when present.",
                port_type.type_id
            )));
        }
    }
    validate_non_empty_strings(
        &port_type.interfaces,
        &format!(
            "port type '{}' interfaces must be non-empty.",
            port_type.type_id
        ),
    )?;
    validate_non_empty_strings(
        &port_type.compatible_with,
        &format!(
            "port type '{}' compatibleWith values must be non-empty.",
            port_type.type_id
        ),
    )?;

    let mut ops = HashSet::new();
    for op in &port_type.allowed_ops {
        if !ops.insert(op) {
            return Err(AppError::validation(format!(
                "port type '{}' contains duplicate allowedOps value {:?}.",
                port_type.type_id, op
            )));
        }
    }

    let mut param_keys = HashSet::new();
    for param in &port_type.params {
        require_non_empty(
            &param.key,
            &format!(
                "port type '{}' params must include non-empty keys.",
                port_type.type_id
            ),
        )?;
        if !param_keys.insert(param.key.as_str()) {
            return Err(AppError::validation(format!(
                "port type '{}' contains duplicate param key '{}'.",
                port_type.type_id, param.key
            )));
        }
        require_non_empty(
            &param.label,
            &format!(
                "port type '{}' param '{}' must include a non-empty label.",
                port_type.type_id, param.key
            ),
        )?;
        if let Some(unit) = param.unit.as_deref() {
            if unit.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "port type '{}' param '{}' unit must be non-empty when present.",
                    port_type.type_id, param.key
                )));
            }
        }
    }

    Ok(())
}

fn validate_mate_type_definition(mate_type: &MateTypeDefinition) -> AppResult<()> {
    require_non_empty(
        &mate_type.type_id,
        "mate type definitions must include a non-empty typeId.",
    )?;
    require_non_empty(
        &mate_type.display_name,
        &format!(
            "mate type '{}' must include a non-empty displayName.",
            mate_type.type_id
        ),
    )?;
    if mate_type.allowed_port_type_pairs.is_empty() {
        return Err(AppError::validation(format!(
            "mate type '{}' must include at least one allowedPortTypePair.",
            mate_type.type_id
        )));
    }

    let mut pairs = HashSet::new();
    for pair in &mate_type.allowed_port_type_pairs {
        require_non_empty(
            &pair.a_type_id,
            &format!(
                "mate type '{}' allowedPortTypePairs must include non-empty aTypeId values.",
                mate_type.type_id
            ),
        )?;
        require_non_empty(
            &pair.b_type_id,
            &format!(
                "mate type '{}' allowedPortTypePairs must include non-empty bTypeId values.",
                mate_type.type_id
            ),
        )?;

        let canonical_pair = if pair.a_type_id <= pair.b_type_id {
            (pair.a_type_id.as_str(), pair.b_type_id.as_str())
        } else {
            (pair.b_type_id.as_str(), pair.a_type_id.as_str())
        };
        if !pairs.insert(canonical_pair) {
            return Err(AppError::validation(format!(
                "mate type '{}' contains duplicate allowedPortTypePair '{}'-'{}'.",
                mate_type.type_id, pair.a_type_id, pair.b_type_id
            )));
        }
    }

    let mut param_keys = HashSet::new();
    for param in &mate_type.params {
        require_non_empty(
            &param.key,
            &format!(
                "mate type '{}' params must include non-empty keys.",
                mate_type.type_id
            ),
        )?;
        if !param_keys.insert(param.key.as_str()) {
            return Err(AppError::validation(format!(
                "mate type '{}' contains duplicate param key '{}'.",
                mate_type.type_id, param.key
            )));
        }
        require_non_empty(
            &param.label,
            &format!(
                "mate type '{}' param '{}' must include a non-empty label.",
                mate_type.type_id, param.key
            ),
        )?;
        if let Some(unit) = param.unit.as_deref() {
            if unit.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "mate type '{}' param '{}' unit must be non-empty when present.",
                    mate_type.type_id, param.key
                )));
            }
        }
    }

    Ok(())
}

fn validate_component_definition(component: &ComponentDefinition) -> AppResult<()> {
    require_non_empty(
        &component.component_id,
        "components must include a non-empty componentId.",
    )?;
    require_non_empty(
        &component.version,
        &format!(
            "component '{}' must include a non-empty version.",
            component.component_id
        ),
    )?;
    require_non_empty(
        &component.display_name,
        &format!(
            "component '{}' must include a non-empty displayName.",
            component.component_id
        ),
    )?;

    if let Some(source_ref) = component.source_ref.as_deref() {
        if source_ref.trim().is_empty() {
            return Err(AppError::validation(format!(
                "component '{}' sourceRef must be non-empty when present.",
                component.component_id
            )));
        }
    }

    let mut sketch_ids = HashSet::new();
    for sketch in &component.sketches {
        validate_sketch_definition(&component.component_id, sketch)?;
        if !sketch_ids.insert(sketch.sketch_id.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' contains duplicate sketchId '{}'.",
                component.component_id, sketch.sketch_id
            )));
        }
    }

    let mut keepout_ids = HashSet::new();
    for keepout in &component.keepouts {
        validate_component_keepout(&component.component_id, keepout)?;
        if !keepout_ids.insert(keepout.keepout_id.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' contains duplicate keepoutId '{}'.",
                component.component_id, keepout.keepout_id
            )));
        }
    }

    let mut fusion_zone_ids = HashSet::new();
    for zone in &component.fusion_zones {
        validate_component_fusion_zone(&component.component_id, zone, &keepout_ids)?;
        if !fusion_zone_ids.insert(zone.zone_id.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' contains duplicate fusion zoneId '{}'.",
                component.component_id, zone.zone_id
            )));
        }
    }

    let mut param_keys = HashSet::new();
    for param in &component.params {
        require_non_empty(
            &param.key,
            &format!(
                "component '{}' params must include non-empty keys.",
                component.component_id
            ),
        )?;
        if !param_keys.insert(param.key.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' contains duplicate param key '{}'.",
                component.component_id, param.key
            )));
        }
        require_non_empty(
            &param.label,
            &format!(
                "component '{}' param '{}' must include a non-empty label.",
                component.component_id, param.key
            ),
        )?;
        if let Some(unit) = param.unit.as_deref() {
            if unit.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "component '{}' param '{}' unit must be non-empty when present.",
                    component.component_id, param.key
                )));
            }
        }
    }
    validate_ui_spec(&component.ui_spec)?;
    validate_design_params(&component.initial_params, &component.ui_spec)?;

    let mut port_ids = HashSet::new();
    for port in &component.ports {
        validate_component_port(&component.component_id, port)?;
        if !port_ids.insert(port.port_id.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' contains duplicate portId '{}'.",
                component.component_id, port.port_id
            )));
        }
    }

    Ok(())
}

pub fn validate_sketch_definition(component_id: &str, sketch: &SketchDefinition) -> AppResult<()> {
    require_non_empty(
        &sketch.sketch_id,
        &format!(
            "component '{}' sketches must include non-empty sketchId values.",
            component_id
        ),
    )?;
    if let Some(plane) = &sketch.plane {
        validate_port_frame(component_id, &sketch.sketch_id, plane)?;
    }
    if sketch.primitives.is_empty() {
        return Err(AppError::validation(format!(
            "component '{}' sketch '{}' must include at least one primitive.",
            component_id, sketch.sketch_id
        )));
    }

    let mut primitive_ids = HashSet::new();
    for primitive in &sketch.primitives {
        validate_sketch_primitive(component_id, &sketch.sketch_id, primitive)?;
        if !primitive_ids.insert(primitive.primitive_id.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' sketch '{}' contains duplicate primitiveId '{}'.",
                component_id, sketch.sketch_id, primitive.primitive_id
            )));
        }
    }

    let mut constraint_ids = HashSet::new();
    for constraint in &sketch.constraints {
        require_non_empty(
            &constraint.constraint_id,
            &format!(
                "component '{}' sketch '{}' constraints must include non-empty constraintId values.",
                component_id, sketch.sketch_id
            ),
        )?;
        if !constraint_ids.insert(constraint.constraint_id.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' sketch '{}' contains duplicate constraintId '{}'.",
                component_id, sketch.sketch_id, constraint.constraint_id
            )));
        }
        if constraint.target_ids.is_empty() {
            return Err(AppError::validation(format!(
                "component '{}' sketch '{}' constraint '{}' must reference at least one primitiveId.",
                component_id, sketch.sketch_id, constraint.constraint_id
            )));
        }
        for target_id in &constraint.target_ids {
            if target_id.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "component '{}' sketch '{}' constraint '{}' targetIds must be non-empty.",
                    component_id, sketch.sketch_id, constraint.constraint_id
                )));
            }
            if !primitive_ids.contains(target_id.as_str()) {
                return Err(AppError::validation(format!(
                    "component '{}' sketch '{}' constraint '{}' references unknown primitiveId '{}'.",
                    component_id, sketch.sketch_id, constraint.constraint_id, target_id
                )));
            }
        }
        if let Some(value) = constraint.value {
            if !value.is_finite() {
                return Err(AppError::validation(format!(
                    "component '{}' sketch '{}' constraint '{}' value must be finite.",
                    component_id, sketch.sketch_id, constraint.constraint_id
                )));
            }
        }
    }

    Ok(())
}

fn validate_sketch_primitive(
    component_id: &str,
    sketch_id: &str,
    primitive: &SketchPrimitive,
) -> AppResult<()> {
    require_non_empty(
        &primitive.primitive_id,
        &format!(
            "component '{}' sketch '{}' primitives must include non-empty primitiveId values.",
            component_id, sketch_id
        ),
    )?;
    for point in &primitive.points {
        if point.iter().any(|value| !value.is_finite()) {
            return Err(AppError::validation(format!(
                "component '{}' sketch '{}' primitive '{}' points must contain finite values.",
                component_id, sketch_id, primitive.primitive_id
            )));
        }
    }
    if let Some(radius) = primitive.radius {
        if !radius.is_finite() || radius <= 0.0 {
            return Err(AppError::validation(format!(
                "component '{}' sketch '{}' primitive '{}' radius must be positive and finite.",
                component_id, sketch_id, primitive.primitive_id
            )));
        }
    }

    let point_count = primitive.points.len();
    let valid = match primitive.kind {
        SketchPrimitiveKind::Point => point_count == 1,
        SketchPrimitiveKind::Line => point_count == 2,
        SketchPrimitiveKind::Polyline => {
            point_count >= 2 && (!primitive.closed || point_count >= 3)
        }
        SketchPrimitiveKind::Spline => point_count >= 2,
        SketchPrimitiveKind::Arc => point_count >= 3,
        SketchPrimitiveKind::Circle => point_count == 1 && primitive.radius.is_some(),
    };
    if !valid {
        return Err(AppError::validation(format!(
            "component '{}' sketch '{}' primitive '{}' has invalid geometry for kind {:?}.",
            component_id, sketch_id, primitive.primitive_id, primitive.kind
        )));
    }

    Ok(())
}

fn validate_component_keepout(
    component_id: &str,
    keepout: &ComponentKeepoutVolume,
) -> AppResult<()> {
    require_non_empty(
        &keepout.keepout_id,
        &format!(
            "component '{}' keepouts must include non-empty keepoutId values.",
            component_id
        ),
    )?;
    require_non_empty(
        &keepout.label,
        &format!(
            "component '{}' keepout '{}' must include a non-empty label.",
            component_id, keepout.keepout_id
        ),
    )?;
    if let Some(frame) = &keepout.frame {
        validate_port_frame(component_id, &keepout.keepout_id, frame)?;
    }
    if let Some(size) = keepout.size {
        if size.iter().any(|value| !value.is_finite() || *value <= 0.0) {
            return Err(AppError::validation(format!(
                "component '{}' keepout '{}' size must contain positive finite values.",
                component_id, keepout.keepout_id
            )));
        }
    }
    if let Some(radius) = keepout.radius {
        if !radius.is_finite() || radius <= 0.0 {
            return Err(AppError::validation(format!(
                "component '{}' keepout '{}' radius must be positive and finite.",
                component_id, keepout.keepout_id
            )));
        }
    }
    if let Some(height) = keepout.height {
        if !height.is_finite() || height <= 0.0 {
            return Err(AppError::validation(format!(
                "component '{}' keepout '{}' height must be positive and finite.",
                component_id, keepout.keepout_id
            )));
        }
    }

    let valid_shape = match keepout.kind {
        KeepoutVolumeKind::Box => keepout.size.is_some(),
        KeepoutVolumeKind::Cylinder => keepout.radius.is_some() && keepout.height.is_some(),
        KeepoutVolumeKind::Sphere => keepout.radius.is_some(),
        KeepoutVolumeKind::Custom => true,
    };
    if !valid_shape {
        return Err(AppError::validation(format!(
            "component '{}' keepout '{}' is missing required dimensions for kind {:?}.",
            component_id, keepout.keepout_id, keepout.kind
        )));
    }

    Ok(())
}

fn validate_component_fusion_zone(
    component_id: &str,
    zone: &ComponentFusionZone,
    keepout_ids: &HashSet<&str>,
) -> AppResult<()> {
    require_non_empty(
        &zone.zone_id,
        &format!(
            "component '{}' fusion zones must include non-empty zoneId values.",
            component_id
        ),
    )?;
    require_non_empty(
        &zone.surface_ref,
        &format!(
            "component '{}' fusion zone '{}' must include a non-empty surfaceRef.",
            component_id, zone.zone_id
        ),
    )?;
    if zone.allowed_ops.is_empty() {
        return Err(AppError::validation(format!(
            "component '{}' fusion zone '{}' must include at least one allowedOp.",
            component_id, zone.zone_id
        )));
    }
    let mut ops = HashSet::new();
    for op in &zone.allowed_ops {
        if !ops.insert(op) {
            return Err(AppError::validation(format!(
                "component '{}' fusion zone '{}' contains duplicate allowedOps value {:?}.",
                component_id, zone.zone_id, op
            )));
        }
    }
    if let Some(radius) = zone.max_blend_radius {
        if !radius.is_finite() || radius <= 0.0 {
            return Err(AppError::validation(format!(
                "component '{}' fusion zone '{}' maxBlendRadius must be positive and finite.",
                component_id, zone.zone_id
            )));
        }
    }
    for keepout_id in &zone.keepout_ids {
        if keepout_id.trim().is_empty() {
            return Err(AppError::validation(format!(
                "component '{}' fusion zone '{}' keepoutIds must be non-empty.",
                component_id, zone.zone_id
            )));
        }
        if !keepout_ids.contains(keepout_id.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' fusion zone '{}' references unknown keepoutId '{}'.",
                component_id, zone.zone_id, keepout_id
            )));
        }
    }

    Ok(())
}

fn validate_component_port(component_id: &str, port: &ComponentPort) -> AppResult<()> {
    require_non_empty(
        &port.port_id,
        &format!(
            "component '{}' ports must include non-empty portId values.",
            component_id
        ),
    )?;
    require_non_empty(
        &port.type_id,
        &format!(
            "component '{}' port '{}' must include a non-empty typeId.",
            component_id, port.port_id
        ),
    )?;
    if let Some(frame) = &port.frame {
        validate_port_frame(component_id, &port.port_id, frame)?;
    }
    validate_non_empty_strings(
        &port.target_ids,
        &format!(
            "component '{}' port '{}' targetIds must be non-empty.",
            component_id, port.port_id
        ),
    )?;
    let mut target_ids = HashSet::new();
    for target_id in &port.target_ids {
        if !target_ids.insert(target_id.as_str()) {
            return Err(AppError::validation(format!(
                "component '{}' port '{}' contains duplicate targetId '{}'.",
                component_id, port.port_id, target_id
            )));
        }
    }

    validate_non_empty_strings(
        &port.interfaces,
        &format!(
            "component '{}' port '{}' interfaces must be non-empty.",
            component_id, port.port_id
        ),
    )?;
    validate_non_empty_strings(
        &port.compatible_with,
        &format!(
            "component '{}' port '{}' compatibleWith values must be non-empty.",
            component_id, port.port_id
        ),
    )?;

    let mut ops = HashSet::new();
    for op in &port.allowed_ops {
        if !ops.insert(op) {
            return Err(AppError::validation(format!(
                "component '{}' port '{}' contains duplicate allowedOps value {:?}.",
                component_id, port.port_id, op
            )));
        }
    }

    for key in port.params.keys() {
        if key.trim().is_empty() {
            return Err(AppError::validation(format!(
                "component '{}' port '{}' params must use non-empty keys.",
                component_id, port.port_id
            )));
        }
    }

    Ok(())
}

fn validate_port_frame(component_id: &str, port_id: &str, frame: &PortFrame) -> AppResult<()> {
    for (label, vector) in [
        ("origin", frame.origin),
        ("xAxis", frame.x_axis),
        ("yAxis", frame.y_axis),
        ("zAxis", frame.z_axis),
    ] {
        if vector.iter().any(|value| !value.is_finite()) {
            return Err(AppError::validation(format!(
                "component '{}' port '{}' frame {} must contain finite values.",
                component_id, port_id, label
            )));
        }
    }

    for (label, vector) in [
        ("xAxis", frame.x_axis),
        ("yAxis", frame.y_axis),
        ("zAxis", frame.z_axis),
    ] {
        let magnitude_squared = vector.iter().map(|value| value * value).sum::<f64>();
        if magnitude_squared <= f64::EPSILON {
            return Err(AppError::validation(format!(
                "component '{}' port '{}' frame {} must be non-zero.",
                component_id, port_id, label
            )));
        }
    }

    Ok(())
}

fn validate_assembly_definition(
    assembly: &AssemblyDefinition,
    components_by_id: &HashMap<&str, &ComponentDefinition>,
    mate_types_by_id: &HashMap<&str, &MateTypeDefinition>,
) -> AppResult<()> {
    require_non_empty(
        &assembly.assembly_id,
        "assemblies must include a non-empty assemblyId.",
    )?;
    require_non_empty(
        &assembly.display_name,
        &format!(
            "assembly '{}' must include a non-empty displayName.",
            assembly.assembly_id
        ),
    )?;

    if assembly.components.is_empty() {
        return Err(AppError::validation(format!(
            "assembly '{}' must include at least one component instance.",
            assembly.assembly_id
        )));
    }

    let mut instance_ids = HashSet::new();
    let mut instance_component_ids = HashMap::new();
    for component_ref in &assembly.components {
        require_non_empty(
            &component_ref.instance_id,
            &format!(
                "assembly '{}' component instances must include non-empty instanceId values.",
                assembly.assembly_id
            ),
        )?;
        require_non_empty(
            &component_ref.component_id,
            &format!(
                "assembly '{}' instance '{}' must include a non-empty componentId.",
                assembly.assembly_id, component_ref.instance_id
            ),
        )?;
        if !instance_ids.insert(component_ref.instance_id.as_str()) {
            return Err(AppError::validation(format!(
                "assembly '{}' contains duplicate instanceId '{}'.",
                assembly.assembly_id, component_ref.instance_id
            )));
        }
        if !components_by_id.contains_key(component_ref.component_id.as_str()) {
            return Err(AppError::validation(format!(
                "assembly '{}' instance '{}' references unknown componentId '{}'.",
                assembly.assembly_id, component_ref.instance_id, component_ref.component_id
            )));
        }
        instance_component_ids.insert(
            component_ref.instance_id.as_str(),
            component_ref.component_id.as_str(),
        );
    }

    let mut mate_ids = HashSet::new();
    for mate in &assembly.mates {
        require_non_empty(
            &mate.mate_id,
            &format!(
                "assembly '{}' mates must include non-empty mateId values.",
                assembly.assembly_id
            ),
        )?;
        require_non_empty(
            &mate.type_id,
            &format!(
                "assembly '{}' mate '{}' must include a non-empty typeId.",
                assembly.assembly_id, mate.mate_id
            ),
        )?;
        if !mate_ids.insert(mate.mate_id.as_str()) {
            return Err(AppError::validation(format!(
                "assembly '{}' contains duplicate mateId '{}'.",
                assembly.assembly_id, mate.mate_id
            )));
        }
        let mate_type = if mate_types_by_id.is_empty() {
            None
        } else {
            Some(mate_types_by_id.get(mate.type_id.as_str()).ok_or_else(|| {
                AppError::validation(format!(
                    "assembly '{}' mate '{}' references unknown mate typeId '{}'.",
                    assembly.assembly_id, mate.mate_id, mate.type_id
                ))
            })?)
        };
        let port_a = validate_port_reference(
            &assembly.assembly_id,
            &mate.mate_id,
            &mate.a,
            &instance_component_ids,
            components_by_id,
        )?;
        let port_b = validate_port_reference(
            &assembly.assembly_id,
            &mate.mate_id,
            &mate.b,
            &instance_component_ids,
            components_by_id,
        )?;
        if !ports_are_compatible(port_a, port_b) {
            return Err(AppError::validation(format!(
                "assembly '{}' mate '{}' connects incompatible ports '{}.{}' and '{}.{}'.",
                assembly.assembly_id,
                mate.mate_id,
                mate.a.instance_id,
                mate.a.port_id,
                mate.b.instance_id,
                mate.b.port_id
            )));
        }
        if let Some(mate_type) = mate_type {
            if !mate_type_allows_port_pair(mate_type, &port_a.type_id, &port_b.type_id) {
                return Err(AppError::validation(format!(
                    "assembly '{}' mate '{}' typeId '{}' does not allow port type pair '{}' and '{}'.",
                    assembly.assembly_id,
                    mate.mate_id,
                    mate.type_id,
                    port_a.type_id,
                    port_b.type_id
                )));
            }
        }
        for key in mate.params.keys() {
            if key.trim().is_empty() {
                return Err(AppError::validation(format!(
                    "assembly '{}' mate '{}' params must use non-empty keys.",
                    assembly.assembly_id, mate.mate_id
                )));
            }
        }
    }

    let mut operation_ids = HashSet::new();
    for operation in &assembly.operations {
        validate_assembly_operation(
            assembly,
            operation,
            &instance_component_ids,
            components_by_id,
        )?;
        if !operation_ids.insert(operation.operation_id.as_str()) {
            return Err(AppError::validation(format!(
                "assembly '{}' contains duplicate operationId '{}'.",
                assembly.assembly_id, operation.operation_id
            )));
        }
    }

    Ok(())
}

fn validate_assembly_operation(
    assembly: &AssemblyDefinition,
    operation: &AssemblyOperation,
    instance_component_ids: &HashMap<&str, &str>,
    components_by_id: &HashMap<&str, &ComponentDefinition>,
) -> AppResult<()> {
    require_non_empty(
        &operation.operation_id,
        &format!(
            "assembly '{}' operations must include non-empty operationId values.",
            assembly.assembly_id
        ),
    )?;
    for instance_id in &operation.target_instance_ids {
        if instance_id.trim().is_empty() {
            return Err(AppError::validation(format!(
                "assembly '{}' operation '{}' targetInstanceIds must be non-empty.",
                assembly.assembly_id, operation.operation_id
            )));
        }
        if !instance_component_ids.contains_key(instance_id.as_str()) {
            return Err(AppError::validation(format!(
                "assembly '{}' operation '{}' references unknown instanceId '{}'.",
                assembly.assembly_id, operation.operation_id, instance_id
            )));
        }
    }
    if matches!(
        operation.kind,
        OperationKind::Fuse | OperationKind::Cut | OperationKind::Mold | OperationKind::Blend
    ) && operation.target_instance_ids.len() < 2
    {
        return Err(AppError::validation(format!(
            "assembly '{}' operation '{}' requires at least two targetInstanceIds for {:?}.",
            assembly.assembly_id, operation.operation_id, operation.kind
        )));
    }
    for port_ref in &operation.port_refs {
        validate_port_reference(
            &assembly.assembly_id,
            &operation.operation_id,
            port_ref,
            instance_component_ids,
            components_by_id,
        )?;
    }
    for key in operation.params.keys() {
        if key.trim().is_empty() {
            return Err(AppError::validation(format!(
                "assembly '{}' operation '{}' params must use non-empty keys.",
                assembly.assembly_id, operation.operation_id
            )));
        }
    }

    Ok(())
}

fn validate_port_reference<'a>(
    assembly_id: &str,
    mate_id: &str,
    port_ref: &PortReference,
    instance_component_ids: &HashMap<&str, &str>,
    components_by_id: &'a HashMap<&str, &ComponentDefinition>,
) -> AppResult<&'a ComponentPort> {
    require_non_empty(
        &port_ref.instance_id,
        &format!(
            "assembly '{}' mate '{}' port references must include non-empty instanceId values.",
            assembly_id, mate_id
        ),
    )?;
    require_non_empty(
        &port_ref.port_id,
        &format!(
            "assembly '{}' mate '{}' port references must include non-empty portId values.",
            assembly_id, mate_id
        ),
    )?;

    let Some(component_id) = instance_component_ids.get(port_ref.instance_id.as_str()) else {
        return Err(AppError::validation(format!(
            "assembly '{}' mate '{}' references unknown instanceId '{}'.",
            assembly_id, mate_id, port_ref.instance_id
        )));
    };
    let component = components_by_id.get(component_id).ok_or_else(|| {
        AppError::validation(format!(
            "assembly '{}' mate '{}' references instance '{}' with unknown componentId '{}'.",
            assembly_id, mate_id, port_ref.instance_id, component_id
        ))
    })?;
    let Some(port) = component
        .ports
        .iter()
        .find(|port| port.port_id == port_ref.port_id)
    else {
        return Err(AppError::validation(format!(
            "assembly '{}' mate '{}' references unknown portId '{}' on instance '{}'.",
            assembly_id, mate_id, port_ref.port_id, port_ref.instance_id
        )));
    };

    Ok(port)
}

fn ports_are_compatible(a: &ComponentPort, b: &ComponentPort) -> bool {
    a.compatible_with
        .iter()
        .any(|type_id| type_id == &b.type_id)
        || b.compatible_with
            .iter()
            .any(|type_id| type_id == &a.type_id)
        || a.interfaces
            .iter()
            .any(|interface| b.interfaces.iter().any(|other| other == interface))
}

fn mate_type_allows_port_pair(
    mate_type: &MateTypeDefinition,
    a_type_id: &str,
    b_type_id: &str,
) -> bool {
    mate_type.allowed_port_type_pairs.iter().any(|pair| {
        (pair.a_type_id == a_type_id && pair.b_type_id == b_type_id)
            || (pair.a_type_id == b_type_id && pair.b_type_id == a_type_id)
    })
}

fn require_non_empty(value: &str, message: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::validation(message.to_string()));
    }
    Ok(())
}

fn validate_non_empty_strings(values: &[String], message: &str) -> AppResult<()> {
    for value in values {
        if value.trim().is_empty() {
            return Err(AppError::validation(message.to_string()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn config_deserializes_missing_voice_with_default_stt_language() {
        let config: Config = serde_json::from_value(serde_json::json!({
            "engines": [],
            "selectedEngineId": ""
        }))
        .expect("config");

        assert_eq!(config.voice.stt_language_code, "en-US");
    }

    fn sample_manifest() -> ModelManifest {
        ModelManifest {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "generated-abc123".to_string(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            document: DocumentMetadata {
                document_name: "Doc".to_string(),
                document_label: "Doc".to_string(),
                source_path: None,
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: vec![PartBinding {
                part_id: "part-shell".to_string(),
                freecad_object_name: "Shell".to_string(),
                label: "Shell".to_string(),
                kind: "Part::Feature".to_string(),
                semantic_role: Some("body".to_string()),
                viewer_asset_path: Some("/tmp/node-shell.stl".to_string()),
                viewer_node_ids: vec!["node-shell".to_string()],
                parameter_keys: vec!["radius".to_string()],
                editable: true,
                bounds: None,
                volume: None,
                area: None,
            }],
            parameter_groups: vec![ParameterGroup {
                group_id: "group-shell".to_string(),
                label: "Shell".to_string(),
                parameter_keys: vec!["radius".to_string()],
                part_ids: vec!["part-shell".to_string()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(0),
            }],
            control_primitives: vec![
                ControlPrimitive {
                    primitive_id: "primitive-shell-radius".to_string(),
                    label: "Shell Radius".to_string(),
                    kind: ControlPrimitiveKind::Number,
                    source: ControlViewSource::Generated,
                    part_ids: vec!["part-shell".to_string()],
                    bindings: vec![PrimitiveBinding {
                        parameter_key: "radius".to_string(),
                        scale: 1.0,
                        offset: 0.0,
                        min: None,
                        max: None,
                    }],
                    editable: true,
                    order: 0,
                },
                ControlPrimitive {
                    primitive_id: "primitive-shell-radius-target".to_string(),
                    label: "Shell Radius Target".to_string(),
                    kind: ControlPrimitiveKind::Number,
                    source: ControlViewSource::Generated,
                    part_ids: vec!["part-shell".to_string()],
                    bindings: vec![PrimitiveBinding {
                        parameter_key: "radius".to_string(),
                        scale: 1.0,
                        offset: 0.0,
                        min: None,
                        max: None,
                    }],
                    editable: true,
                    order: 1,
                },
            ],
            control_relations: vec![ControlRelation {
                relation_id: "relation-shell-radius".to_string(),
                source_primitive_id: "primitive-shell-radius".to_string(),
                target_primitive_id: "primitive-shell-radius-target".to_string(),
                mode: ControlRelationMode::Mirror,
                scale: 1.0,
                offset: 0.0,
                enabled: false,
            }],
            control_views: vec![ControlView {
                view_id: "view-shell".to_string(),
                label: "Shell".to_string(),
                scope: ControlViewScope::Part,
                part_ids: vec!["part-shell".to_string()],
                primitive_ids: vec!["primitive-shell-radius".to_string()],
                sections: vec![ControlViewSection {
                    section_id: "section-primary".to_string(),
                    label: "Primary".to_string(),
                    primitive_ids: vec!["primitive-shell-radius".to_string()],
                    collapsed: false,
                }],
                is_default: true,
                source: ControlViewSource::Generated,
                status: EnrichmentStatus::Accepted,
                order: 0,
            }],
            advisories: vec![Advisory {
                advisory_id: "advisory-shell-radius".to_string(),
                label: "Shell note".to_string(),
                severity: AdvisorySeverity::Info,
                primitive_ids: vec!["primitive-shell-radius".to_string()],
                view_ids: vec!["view-shell".to_string()],
                message: "Shell radius drives the body profile.".to_string(),
                condition: AdvisoryCondition::Always,
                threshold: None,
            }],
            selection_targets: vec![SelectionTarget {
                target_id: Some("target-shell".to_string()),
                part_id: "part-shell".to_string(),
                viewer_node_id: "node-shell".to_string(),
                label: "Shell".to_string(),
                kind: SelectionTargetKind::Object,
                editable: true,
                parameter_keys: vec!["radius".to_string()],
                primitive_ids: vec!["primitive-shell-radius".to_string()],
                view_ids: vec!["view-shell".to_string()],
            }],
            measurement_annotations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    fn sample_manifest_with_shape(
        part_count: usize,
        node_count: usize,
        parameter_count: usize,
    ) -> ModelManifest {
        let parts = (0..part_count)
            .map(|index| {
                let part_id = format!("part-{}", index);
                let viewer_node_ids = (0..node_count)
                    .map(|node_index| format!("node-{}-{}", index, node_index))
                    .collect::<Vec<_>>();
                let parameter_keys = (0..parameter_count)
                    .map(|param_index| format!("param_{}_{}", index, param_index))
                    .collect::<Vec<_>>();
                PartBinding {
                    part_id: part_id.clone(),
                    freecad_object_name: format!("Object{}", index),
                    label: format!("Part {}", index),
                    kind: "Part::Feature".to_string(),
                    semantic_role: Some("unknown".to_string()),
                    viewer_asset_path: Some(format!("/tmp/part-{}.stl", index)),
                    viewer_node_ids,
                    parameter_keys: parameter_keys.clone(),
                    editable: !parameter_keys.is_empty(),
                    bounds: None,
                    volume: None,
                    area: None,
                }
            })
            .collect::<Vec<_>>();

        let selection_targets = parts
            .iter()
            .flat_map(|part| {
                part.viewer_node_ids
                    .iter()
                    .map(|node_id| SelectionTarget {
                        target_id: Some(format!("target-{}", node_id)),
                        part_id: part.part_id.clone(),
                        viewer_node_id: node_id.clone(),
                        label: part.label.clone(),
                        kind: SelectionTargetKind::Object,
                        editable: part.editable,
                        parameter_keys: part.parameter_keys.clone(),
                        primitive_ids: Vec::new(),
                        view_ids: Vec::new(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let parameter_groups = parts
            .iter()
            .filter(|part| !part.parameter_keys.is_empty())
            .map(|part| ParameterGroup {
                group_id: format!("group-{}", part.part_id),
                label: part.label.clone(),
                parameter_keys: part.parameter_keys.clone(),
                part_ids: vec![part.part_id.clone()],
                editable: true,
                presentation: Some("primary".to_string()),
                order: Some(0),
            })
            .collect::<Vec<_>>();

        ModelManifest {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: format!(
                "generated-shape-{}-{}-{}",
                part_count, node_count, parameter_count
            ),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            document: DocumentMetadata {
                document_name: "Shape".to_string(),
                document_label: "Shape".to_string(),
                source_path: None,
                object_count: part_count,
                warnings: Vec::new(),
            },
            parts,
            parameter_groups,
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets,
            measurement_annotations: Vec::new(),
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    #[test]
    fn validate_model_manifest_accepts_consistent_manifest() {
        validate_model_manifest(&sample_manifest()).expect("manifest should be valid");
    }

    #[test]
    fn validate_model_manifest_rejects_unknown_selection_part() {
        let mut manifest = sample_manifest();
        manifest.selection_targets[0].part_id = "missing".to_string();
        let err = validate_model_manifest(&manifest).expect_err("manifest should be invalid");
        assert!(err.message.contains("unknown partId"));
    }

    #[test]
    fn validate_model_manifest_rejects_unknown_relation_target() {
        let mut manifest = sample_manifest();
        manifest.control_relations[0].target_primitive_id = "missing-target".to_string();
        let err = validate_model_manifest(&manifest).expect_err("manifest should be invalid");
        assert!(err.message.contains("unknown target primitive"));
    }

    #[test]
    fn validate_model_manifest_rejects_unknown_selection_target_primitive() {
        let mut manifest = sample_manifest();
        manifest.selection_targets[0].primitive_ids = vec!["missing-primitive".to_string()];
        let err = validate_model_manifest(&manifest).expect_err("manifest should be invalid");
        assert!(err.message.contains("unknown primitiveId"));
    }

    #[test]
    fn validate_model_manifest_accepts_measurement_annotations() {
        let mut manifest = sample_manifest();
        manifest.measurement_annotations = vec![MeasurementAnnotation {
            annotation_id: "measurement-shell-outer-radius".to_string(),
            label: "Outer Radius".to_string(),
            basis: MeasurementBasis::Outer,
            axis: MeasurementAxis::Radial,
            parameter_keys: vec!["radius".to_string()],
            primitive_ids: vec!["primitive-shell-radius".to_string()],
            target_ids: vec!["target-shell".to_string()],
            guide_id: Some("guide-shell-radius".to_string()),
            explanation: Some("Measures the outer shell radius.".to_string()),
            formula_hint: Some("outer_radius = radius".to_string()),
            source: MeasurementAnnotationSource::Generated,
        }];

        validate_model_manifest(&manifest).expect("manifest should accept measurement semantics");
    }

    #[test]
    fn validate_model_manifest_rejects_unknown_measurement_target_ids() {
        let mut manifest = sample_manifest();
        manifest.measurement_annotations = vec![MeasurementAnnotation {
            annotation_id: "measurement-shell-outer-radius".to_string(),
            label: "Outer Radius".to_string(),
            basis: MeasurementBasis::Outer,
            axis: MeasurementAxis::Radial,
            parameter_keys: vec!["radius".to_string()],
            primitive_ids: Vec::new(),
            target_ids: vec!["missing-target".to_string()],
            guide_id: None,
            explanation: None,
            formula_hint: None,
            source: MeasurementAnnotationSource::Generated,
        }];

        let err =
            validate_model_manifest(&manifest).expect_err("manifest should reject bad targetId");
        assert!(err.message.contains("unknown targetId"));
    }

    #[test]
    fn validate_model_runtime_bundle_rejects_unknown_measurement_guide_ids() {
        let mut manifest = sample_manifest();
        manifest.measurement_annotations = vec![MeasurementAnnotation {
            annotation_id: "measurement-shell-outer-radius".to_string(),
            label: "Outer Radius".to_string(),
            basis: MeasurementBasis::Outer,
            axis: MeasurementAxis::Radial,
            parameter_keys: vec!["radius".to_string()],
            primitive_ids: vec!["primitive-shell-radius".to_string()],
            target_ids: vec!["target-shell".to_string()],
            guide_id: Some("missing-guide".to_string()),
            explanation: None,
            formula_hint: None,
            source: MeasurementAnnotationSource::Generated,
        }];

        let bundle = ArtifactBundle {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: manifest.model_id.clone(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/model.json".to_string(),
            macro_path: Some("/tmp/model.py".to_string()),
            preview_stl_path: "/tmp/model.stl".to_string(),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: vec![CalloutAnchor {
                anchor_id: "anchor-shell-center".to_string(),
                position: [0.0, 0.0, 0.0],
                normal: None,
            }],
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        };

        let err = validate_model_runtime_bundle(&manifest, &bundle)
            .expect_err("runtime pair should reject bad guide id");
        assert!(err.message.contains("unknown guideId"));
    }

    #[test]
    fn validate_model_runtime_bundle_rejects_edge_target_without_manifest_target() {
        let manifest = sample_manifest();
        let bundle = ArtifactBundle {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: manifest.model_id.clone(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/model.json".to_string(),
            macro_path: Some("/tmp/model.py".to_string()),
            preview_stl_path: "/tmp/model.stl".to_string(),
            viewer_assets: Vec::new(),
            edge_targets: vec![ViewerEdgeTarget {
                target_id: "missing-edge-target".to_string(),
                part_id: "part-shell".to_string(),
                viewer_node_id: "node-shell".to_string(),
                label: "Shell edge".to_string(),
                editable: false,
                start: ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                end: ViewerEdgePoint {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            }],
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        };

        let err = validate_model_runtime_bundle(&manifest, &bundle)
            .expect_err("runtime pair should reject unmatched edge target");
        assert!(err.message.contains("unknown targetId"));
        assert!(err.message.contains("missing-edge-target"));
    }

    #[test]
    fn validate_model_runtime_bundle_rejects_face_target_without_manifest_target() {
        let manifest = sample_manifest();
        let bundle = ArtifactBundle {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: manifest.model_id.clone(),
            source_kind: ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/model.json".to_string(),
            macro_path: Some("/tmp/model.py".to_string()),
            preview_stl_path: "/tmp/model.stl".to_string(),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: vec![ViewerFaceTarget {
                target_id: "missing-face-target".to_string(),
                part_id: "part-shell".to_string(),
                viewer_node_id: "node-shell".to_string(),
                label: "Shell face".to_string(),
                editable: false,
                center: ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                normal: Some([0.0, 0.0, 1.0]),
                area: Some(10.0),
            }],
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        };

        let err = validate_model_runtime_bundle(&manifest, &bundle)
            .expect_err("runtime pair should reject unmatched face target");
        assert!(err.message.contains("unknown targetId"));
        assert!(err.message.contains("missing-face-target"));
    }

    #[test]
    fn genie_traits_from_seed_stays_within_declared_bounds() {
        for seed in [1u32, 7, 42, 1337, u32::MAX - 4] {
            let traits = GenieTraits::from_seed(seed);
            assert_eq!(traits.version, GENIE_TRAITS_VERSION);
            assert_eq!(traits.seed, seed);
            assert!((0.0..360.0).contains(&traits.color_hue));
            assert!((10..=24).contains(&traits.vertex_count));
            assert!((25.0..=34.0).contains(&traits.radius_base));
            assert!((0.9..=1.06).contains(&traits.stretch_y));
            assert!((0.88..=1.14).contains(&traits.asymmetry));
            assert!((2..=6).contains(&traits.chord_skip));
            assert!((0.7..=1.45).contains(&traits.jitter_scale));
            assert!((0.7..=1.35).contains(&traits.pulse_scale));
            assert!((0.8..=1.6).contains(&traits.hover_scale));
            assert!((0.35..=1.25).contains(&traits.warp_scale));
            assert!((-32.0..=32.0).contains(&traits.glow_hue_shift));
            assert!((15.0..=22.5).contains(&traits.eye_spacing));
            assert!((2.0..=3.6).contains(&traits.eye_size));
            assert!((0.6..=2.6).contains(&traits.mouth_curve));
            assert!((0.2..=1.0).contains(&traits.thinking_bias));
            assert!((0.2..=1.0).contains(&traits.repair_bias));
            assert!((0.2..=1.0).contains(&traits.render_bias));
            assert!((0.35..=1.0).contains(&traits.expressiveness));
        }
    }

    #[test]
    fn decode_genie_traits_json_upgrades_legacy_payload_deterministically() {
        let legacy = serde_json::json!({
            "seed": 4242,
            "colorHue": 122.5,
            "vertexCount": 19,
            "jitterScale": 1.12,
            "pulseScale": 0.84,
        })
        .to_string();

        let first = decode_genie_traits_json(&legacy, Some("thread-alpha"))
            .expect("legacy payload should upgrade");
        let second = decode_genie_traits_json(&legacy, Some("thread-alpha"))
            .expect("legacy payload should upgrade deterministically");

        assert_eq!(first, second);
        assert_eq!(first.version, GENIE_TRAITS_VERSION);
        assert_eq!(first.seed, 4242);
        assert_eq!(first.color_hue, 122.5);
        assert_eq!(first.vertex_count, 19);
        assert_eq!(first.jitter_scale, 1.12);
        assert_eq!(first.pulse_scale, 0.84);
    }

    #[test]
    fn upgraded_or_default_genie_traits_uses_thread_seed_when_traits_are_missing() {
        let thread_id = "thread-without-traits";
        let traits = upgraded_or_default_genie_traits(thread_id, None);

        assert_eq!(traits.version, GENIE_TRAITS_VERSION);
        assert_eq!(traits.seed, derive_thread_seed(thread_id));
    }

    #[test]
    fn reconcile_post_processing_controls_inserts_missing_image_field_and_param() {
        let ui_spec = UiSpec {
            fields: vec![UiField::Number {
                key: "width".to_string(),
                label: "width".to_string(),
                min: None,
                max: None,
                step: None,
                min_from: None,
                max_from: None,
                frozen: false,
            }],
        };
        let params = DesignParams::from([("width".to_string(), ParamValue::Number(100.0))]);
        let post = PostProcessingSpec {
            displacement: Some(DisplacementSpec {
                image_param: "image_path".to_string(),
                projection: ProjectionType::Planar,
                depth_mm: 2.0,
                invert: true,
            }),
            lithophane_attachments: vec![],
        };

        let (next_ui_spec, next_params) =
            reconcile_post_processing_controls(&ui_spec, &params, Some(&post));

        assert!(matches!(
            next_ui_spec.fields.first(),
            Some(UiField::Image { key, .. }) if key == "image_path"
        ));
        assert_eq!(
            next_params.get("image_path"),
            Some(&ParamValue::String(String::new()))
        );
    }

    #[test]
    fn validate_design_output_rejects_displacement_without_image_field() {
        let output = DesignOutput {
            title: "Lithophane".to_string(),
            version_name: "V1".to_string(),
            response: "ok".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: "pass".to_string(),
            macro_dialect: MacroDialect::Legacy,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            ui_spec: UiSpec {
                fields: vec![UiField::Number {
                    key: "width".to_string(),
                    label: "width".to_string(),
                    min: None,
                    max: None,
                    step: None,
                    min_from: None,
                    max_from: None,
                    frozen: false,
                }],
            },
            initial_params: DesignParams::from([("width".to_string(), ParamValue::Number(100.0))]),
            post_processing: Some(PostProcessingSpec {
                displacement: Some(DisplacementSpec {
                    image_param: "image_path".to_string(),
                    projection: ProjectionType::Planar,
                    depth_mm: 2.0,
                    invert: false,
                }),
                lithophane_attachments: vec![],
            }),
        };

        let error = validate_design_output(&output).unwrap_err();
        assert!(error.message.contains(
            "postProcessing displacement imageParam 'image_path' must reference a uiSpec field."
        ));
    }

    #[test]
    fn normalize_post_processing_spec_promotes_legacy_displacement() {
        let normalized = normalize_post_processing_spec(Some(PostProcessingSpec {
            displacement: Some(DisplacementSpec {
                image_param: "image_path".to_string(),
                projection: ProjectionType::Planar,
                depth_mm: 2.25,
                invert: true,
            }),
            lithophane_attachments: vec![],
        }))
        .expect("post-processing should normalize");

        assert_eq!(normalized.lithophane_attachments.len(), 1);
        let attachment = &normalized.lithophane_attachments[0];
        assert_eq!(attachment.id, "legacy-image-path");
        assert!(matches!(
            attachment.source,
            LithophaneAttachmentSource::Param { ref image_param } if image_param == "image_path"
        ));
        assert_eq!(attachment.placement.projection, ProjectionType::Planar);
        assert_eq!(attachment.relief.depth_mm, 2.25);
        assert!(attachment.relief.invert);
    }

    #[test]
    fn validate_design_output_rejects_cmyk_for_non_planar_lithophane_attachment() {
        let output = DesignOutput {
            title: "Lithophane".to_string(),
            version_name: "V1".to_string(),
            response: "ok".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: "pass".to_string(),
            macro_dialect: MacroDialect::Legacy,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            ui_spec: UiSpec {
                fields: vec![UiField::Image {
                    key: "image_path".to_string(),
                    label: "Image".to_string(),
                    frozen: false,
                }],
            },
            initial_params: DesignParams::from([(
                "image_path".to_string(),
                ParamValue::String("/tmp/litho.png".to_string()),
            )]),
            post_processing: Some(PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::Param {
                        image_param: "image_path".to_string(),
                    },
                    target_part_id: String::new(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Cylindrical,
                        width_mm: 40.0,
                        height_mm: 20.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 2.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Cmyk,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        };

        let error = validate_design_output(&output).unwrap_err();
        assert!(error
            .message
            .contains("only supports CMYK with planar projection"));
    }

    proptest! {
        #[test]
        fn validate_model_manifest_accepts_generated_shapes(
            part_count in 1usize..6,
            node_count in 1usize..4,
            parameter_count in 0usize..4,
        ) {
            let manifest = sample_manifest_with_shape(part_count, node_count, parameter_count);
            prop_assert!(validate_model_manifest(&manifest).is_ok());
        }

        #[test]
        fn validate_model_manifest_rejects_unknown_viewer_nodes(
            part_count in 1usize..6,
            node_count in 1usize..4,
            parameter_count in 0usize..4,
        ) {
            let mut manifest = sample_manifest_with_shape(part_count, node_count, parameter_count);
            manifest.selection_targets[0].viewer_node_id = "missing-node".to_string();

            let err = validate_model_manifest(&manifest).expect_err("manifest should reject unknown viewer nodes");
            prop_assert!(err.message.contains("unknown viewer node id"));
        }
    }

    #[test]
    fn engine_kind_build123d_mappings() {
        let kind = EngineKind::Build123d;
        assert_eq!(kind.as_str(), "build123d");
        assert_eq!(kind.to_source_language(), SourceLanguage::Build123d);
        assert_eq!(kind.to_geometry_backend(), GeometryBackend::Build123d);
        assert_eq!("build123d".parse::<EngineKind>().unwrap(), kind);
    }

    #[test]
    fn source_language_build123d_mappings() {
        let lang = SourceLanguage::Build123d;
        assert_eq!(lang.as_str(), "build123d");
        assert_eq!("build123d".parse::<SourceLanguage>().unwrap(), lang);
    }

    #[test]
    fn source_language_ecky_mappings() {
        let lang = SourceLanguage::EckyIrV0;
        assert_eq!(lang.as_str(), "ecky");
        assert_eq!("ecky".parse::<SourceLanguage>().unwrap(), lang);
        assert_eq!("eckyIrV0".parse::<SourceLanguage>().unwrap(), lang);
        assert_eq!("ecky_ir_v0".parse::<SourceLanguage>().unwrap(), lang);
    }

    #[test]
    fn geometry_backend_mesh_mappings() {
        let backend = GeometryBackend::EckyRust;
        assert_eq!(backend.as_str(), "mesh");
        assert_eq!("mesh".parse::<GeometryBackend>().unwrap(), backend);
        assert_eq!("eckyRust".parse::<GeometryBackend>().unwrap(), backend);
        assert_eq!("ecky_rust".parse::<GeometryBackend>().unwrap(), backend);
    }

    #[test]
    fn runtime_capabilities_serialize_mesh_backend_name() {
        let capabilities = RuntimeCapabilities {
            freecad: RuntimeBackendCapability {
                available: false,
                detail: "freecad".to_string(),
                path: None,
            },
            build123d: RuntimeBackendCapability {
                available: true,
                detail: "build123d".to_string(),
                path: Some("/tmp/python3".to_string()),
            },
            direct_occt: RuntimeBackendCapability {
                available: false,
                detail: "direct OCCT blocked".to_string(),
                path: None,
            },
            ecky_rust: RuntimeBackendCapability {
                available: true,
                detail: "mesh".to_string(),
                path: None,
            },
            recommended_authoring_context: RuntimeAuthoringContext {
                engine_kind: EngineKind::EckyIrV0,
                source_language: SourceLanguage::EckyIrV0,
                geometry_backend: GeometryBackend::EckyRust,
            },
        };

        let json = serde_json::to_value(&capabilities).expect("serialize capabilities");
        assert_eq!(
            json.get("mesh")
                .and_then(|value| value.get("detail"))
                .and_then(|value| value.as_str()),
            Some("mesh")
        );
        assert_eq!(
            json.get("directOcct")
                .and_then(|value| value.get("detail"))
                .and_then(|value| value.as_str()),
            Some("direct OCCT blocked")
        );
        assert!(json.get("direct_occt").is_none());
        assert!(json.get("eckyRust").is_none());

        let legacy = serde_json::json!({
            "freecad": { "available": false, "detail": "freecad" },
            "build123d": { "available": true, "detail": "build123d", "path": "/tmp/python3" },
            "directOcct": { "available": false, "detail": "direct OCCT blocked" },
            "eckyRust": { "available": true, "detail": "mesh" },
            "recommendedAuthoringContext": {
                "engineKind": "eckyIrV0",
                "sourceLanguage": "ecky",
                "geometryBackend": "mesh"
            }
        });
        let decoded: RuntimeCapabilities =
            serde_json::from_value(legacy).expect("deserialize legacy capability alias");
        assert!(decoded.ecky_rust.available);
    }
}
