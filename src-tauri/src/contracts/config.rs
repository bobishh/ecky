use super::{EngineKind, GeometryBackend, SourceLanguage};
use serde::{Deserialize, Serialize};
use specta::Type;

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
    /// Experimental: expose read-only Ecky Core AST tools for agent authoring.
    #[serde(default)]
    pub ecky_ast_authoring: bool,
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
            ecky_ast_authoring: false,
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
    #[serde(default, alias = "cad_text_font_path")]
    pub cad_text_font_path: String,
    #[serde(default)]
    pub freecad_library_roots: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FreecadLibrarySearchRequest {
    pub query: String,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default)]
    pub include_architecture: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FreecadLibraryItem {
    pub id: String,
    pub name: String,
    pub category_path: String,
    pub root_path: String,
    pub relative_path: String,
    pub formats: Vec<String>,
    pub preferred_format: String,
    pub import_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_path: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FreecadLibraryImportRequest {
    pub item: FreecadLibraryItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

fn default_max_generation_attempts() -> u32 {
    3
}

fn default_engine_kind() -> EngineKind {
    EngineKind::EckyIrV0
}

fn default_source_language() -> SourceLanguage {
    SourceLanguage::EckyIrV0
}

fn default_geometry_backend() -> GeometryBackend {
    GeometryBackend::Build123d
}

fn default_true() -> bool {
    true
}
