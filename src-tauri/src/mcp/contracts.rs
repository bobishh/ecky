use crate::models::{
    AgentDraft, AgentOrigin, ArtifactBundle, ControlPrimitive, ControlView, DesignOutput,
    DesignParams, ModelManifest, TargetLeaseInfo, Thread, ThreadStatus, UiSpec,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckRequest {}

// --- request_user_prompt ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPromptRequest {
    pub request_id: Option<String>,
    pub message: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPromptResponse {
    pub request_id: String,
    pub prompt_text: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptEvent {
    pub request_id: String,
    pub message: Option<String>,
    pub agent_label: String,
    pub session_id: String,
}

// --- user_confirm_request ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfirmRequest {
    pub request_id: Option<String>,
    pub message: String,
    pub buttons: Option<Vec<String>>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfirmResponse {
    pub request_id: String,
    pub choice: String,
}

/// Payload emitted as a Tauri event to the frontend.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfirmEvent {
    pub request_id: String,
    pub message: String,
    pub buttons: Vec<String>,
    pub agent_label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResponse {
    pub server_version: String,
    pub db_path: String,
    pub freecad_configured: bool,
    pub db_ready: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListRequest {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListEntry {
    pub thread_id: String,
    pub title: String,
    pub updated_at: u64,
    pub version_count: usize,
    pub pending_count: usize,
    pub error_count: usize,
    pub status: ThreadStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListResponse {
    pub threads: Vec<ThreadListEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOverviewRequest {
    pub agent_label: Option<String>,
    pub llm_model_id: Option<String>,
    pub llm_model_label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOverviewTarget {
    pub thread_id: String,
    pub message_id: String,
    pub title: String,
    pub version_name: String,
    pub model_id: Option<String>,
    pub has_draft: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOverviewBrief {
    pub engine_label: String,
    pub summary: String,
    pub rules: Vec<String>,
    pub resources: Vec<String>,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceControlSurface {
    pub ui_field_count: usize,
    pub range_count: usize,
    pub number_count: usize,
    pub select_count: usize,
    pub checkbox_count: usize,
    pub parameter_count: usize,
    pub has_semantic_manifest: bool,
    pub control_primitive_count: usize,
    pub control_relation_count: usize,
    pub control_view_count: usize,
    pub hints: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOverviewResponse {
    pub agent_brief: WorkspaceOverviewBrief,
    pub control_surface: WorkspaceControlSurface,
    pub default_target: WorkspaceOverviewTarget,
    pub recent_threads: Vec<ThreadListEntry>,
    pub lease_info: Option<TargetLeaseInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGetRequest {
    pub thread_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGetResponse {
    pub thread: Thread,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentitySetRequest {
    pub agent_label: Option<String>,
    pub llm_model_id: Option<String>,
    pub llm_model_label: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentityOverride {
    pub agent_label: Option<String>,
    pub llm_model_id: Option<String>,
    pub llm_model_label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentityResponse {
    pub session_id: String,
    pub client_kind: String,
    pub host_label: String,
    pub agent_label: String,
    pub llm_model_id: Option<String>,
    pub llm_model_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetGetRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetGetResponse {
    pub thread_id: String,
    pub message_id: String,
    pub title: String,
    pub version_name: String,
    pub macro_code: String,
    pub ui_spec: UiSpec,
    pub initial_params: DesignParams,
    pub artifact_bundle: Option<ArtifactBundle>,
    pub model_manifest: Option<ModelManifest>,
    pub latest_draft: Option<AgentDraft>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticManifestRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticManifestResponse {
    pub thread_id: String,
    pub message_id: String,
    pub title: Option<String>,
    pub version_name: Option<String>,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
    pub latest_draft: Option<AgentDraft>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamsPatchRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub parameter_patch: DesignParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_processing: Option<crate::models::PostProcessingSpec>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamsPatchResponse {
    pub thread_id: String,
    pub message_id: String,
    pub merged_params: DesignParams,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
    pub design_output: DesignOutput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoginRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoginResponse {
    pub session_id: String,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLogoutRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLogoutResponse {
    pub success: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResumeRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResumeResponse {
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
    pub last_interaction_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroReplaceRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub macro_code: String,
    pub ui_spec: Option<UiSpec>,
    pub parameters: Option<DesignParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_processing: Option<crate::models::PostProcessingSpec>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroReplaceResponse {
    pub thread_id: String,
    pub message_id: String,
    pub macro_code: String,
    pub ui_spec: UiSpec,
    pub initial_params: DesignParams,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSaveRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub title: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSaveResponse {
    pub thread_id: String,
    pub message_id: String,
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadForkRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub title: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadForkResponse {
    pub thread_id: String,
    pub message_id: String,
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeThreadRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeThreadResponse {
    pub thread_id: String,
    pub finalized_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionRestoreRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub message_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionRestoreResponse {
    pub thread_id: String,
    pub message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlPrimitiveSaveRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub primitive: ControlPrimitive,
    pub title: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlPrimitiveDeleteRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub primitive_id: String,
    pub title: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlViewSaveRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub view: ControlView,
    pub title: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlViewDeleteRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub view_id: String,
    pub title: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticManifestMutationResponse {
    pub thread_id: String,
    pub message_id: String,
    pub model_id: String,
    pub title: String,
    pub version_name: String,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
    pub agent_origin: AgentOrigin,
}
