use crate::models::{
    AgentOrigin, ArtifactBundle, ControlPrimitive, ControlView, DesignOutput, DesignParams,
    MeasurementAnnotation, ModelManifest, StructuralVerificationResult, TargetLeaseInfo, Thread,
    ThreadStatus, UiSpec,
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
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPromptResponse {
    pub request_id: String,
    pub prompt_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::contracts::Attachment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptPreviewGenerateRequest {
    pub prompt: String,
    #[serde(default)]
    pub attachments: Vec<crate::contracts::Attachment>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptPreviewGenerateResponse {
    pub thread_id: String,
    pub message_id: String,
    pub image_data: String,
    pub caption: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkAsReadRequest {
    pub message_id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkAsReadResponse {
    pub thread_id: String,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_ids: Vec<String>,
    pub status: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptEvent {
    pub request_id: String,
    pub message: Option<String>,
    pub agent_label: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptClosedEvent {
    pub request_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetModelScreenshotRequest {
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    #[serde(default)]
    pub include_overlays: Option<bool>,
    #[serde(default)]
    pub camera: Option<crate::contracts::ViewportCameraState>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentViewportScreenshotEvent {
    pub request_id: String,
    pub thread_id: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub preview_stl_path: String,
    #[serde(default)]
    pub viewer_assets: Vec<crate::contracts::ViewerAsset>,
    pub include_overlays: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<crate::contracts::ViewportCameraState>,
}

// --- compare_models ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareModelsRequest {
    pub ref_path: String,
    pub gen_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareModelsResponse {
    pub reference_volume: f64,
    pub generated_volume: f64,
    pub volume_difference_percent: f64,
    pub bounding_box_match_error: f64,
    pub status: String,
    pub details: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiDispatchRequest {
    pub action: String,
    pub target: String,
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiDispatchResponse {
    pub success: bool,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentUiDispatchEvent {
    pub action: String,
    pub target: String,
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResponse {
    pub server_version: String,
    pub db_path: String,
    pub freecad_configured: bool,
    pub db_ready: bool,
    pub runtime_capabilities: crate::contracts::RuntimeCapabilities,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListRequest {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListEntry {
    pub thread_id: String,
    pub title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListResponse {
    pub threads: Vec<ThreadListEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCreateRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCreateResponse {
    pub thread_id: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadBorrowRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
    #[serde(default)]
    pub steal_thread: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadBorrowResponse {
    pub session_id: String,
    pub thread_id: String,
    pub title: String,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMetaRequest {
    pub thread_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMetaResponse {
    pub thread_id: String,
    pub title: String,
    pub updated_at: u64,
    pub version_count: usize,
    pub pending_count: usize,
    pub queued_count: usize,
    pub error_count: usize,
    pub status: ThreadStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_owner: Option<crate::models::AgentSession>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessagesRequest {
    pub thread_id: String,
    pub limit: Option<usize>,
    pub before: Option<String>,
    pub roles: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessageEntry {
    pub id: String,
    pub role: String,
    pub status: String,
    pub timestamp: u64,
    pub content: String,
    pub has_output: bool,
    pub has_artifacts: bool,
    pub has_manifest: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessagesResponse {
    pub thread_id: String,
    pub messages: Vec<ThreadMessageEntry>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_name: Option<String>,
    pub model_id: Option<String>,
    pub has_draft: bool,
    pub has_version: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_owner: Option<crate::models::AgentSession>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOverviewBrief {
    pub engine_label: String,
    pub source_language: String,
    pub macro_dialect: String,
    pub geometry_backend: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_owner: Option<crate::models::AgentSession>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetResolvedFrom {
    Base,
    Draft,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetMetaRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetMetaResponse {
    pub thread_id: String,
    pub message_id: String,
    pub title: String,
    pub version_name: String,
    pub model_id: Option<String>,
    pub source_language: String,
    pub macro_dialect: String,
    pub geometry_backend: String,
    pub has_draft: bool,
    pub resolved_from: TargetResolvedFrom,
    pub has_artifact_bundle: bool,
    pub has_runtime_manifest: bool,
    pub export_formats: Vec<String>,
    pub has_step_export: bool,
    pub step_export_path: Option<String>,
    pub edge_target_count: usize,
    pub face_target_count: usize,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetAuthoringContext {
    pub source_language: String,
    pub macro_dialect: String,
    pub geometry_backend: String,
    pub file_extension: String,
    pub authoring_card: String,
    pub guide_uris: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetMacroRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetMacroResponse {
    pub thread_id: String,
    pub message_id: String,
    pub title: String,
    pub version_name: String,
    pub resolved_from: TargetResolvedFrom,
    pub macro_code: String,
    pub macro_dialect: crate::models::MacroDialect,
    pub post_processing: Option<crate::models::PostProcessingSpec>,
    pub authoring_context: TargetAuthoringContext,
    pub artifact_digest: Option<ArtifactBundleDigest>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferLine {
    pub line_number: usize,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferGetRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferGetResponse {
    pub thread_id: String,
    pub message_id: String,
    pub title: String,
    pub version_name: String,
    pub resolved_from: TargetResolvedFrom,
    pub digest: String,
    pub line_count: usize,
    pub macro_code: String,
    pub lines: Vec<MacroBufferLine>,
    pub source_language: String,
    pub macro_dialect: crate::models::MacroDialect,
    pub geometry_backend: String,
    pub post_processing: Option<crate::models::PostProcessingSpec>,
    pub authoring_context: TargetAuthoringContext,
    pub artifact_digest: Option<ArtifactBundleDigest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferReplacement {
    pub start_line: usize,
    pub end_line: usize,
    pub new_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferApplyPatchRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub expected_digest: String,
    pub patch: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferRenderRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub expected_digest: String,
    pub ui_spec: Option<UiSpec>,
    pub parameters: Option<DesignParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_processing: Option<crate::models::PostProcessingSpec>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferEditResponse {
    pub digest: String,
    pub line_count: usize,
    pub macro_code: String,
    pub lines: Vec<MacroBufferLine>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferReplaceAndRenderRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub expected_digest: String,
    pub replacements: Vec<MacroBufferReplacement>,
    pub ui_spec: Option<UiSpec>,
    pub parameters: Option<DesignParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_processing: Option<crate::models::PostProcessingSpec>,
    pub geometry_backend: Option<crate::models::GeometryBackend>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroBufferReplaceAndRenderResponse {
    pub thread_id: String,
    pub message_id: String,
    pub digest: String,
    pub line_count: usize,
    pub macro_code: String,
    pub ui_spec: UiSpec,
    pub initial_params: DesignParams,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structural_verification: Option<StructuralVerificationResult>,
    pub artifact_digest: ArtifactBundleDigest,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TargetDetailSection {
    UiSpec,
    InitialParams,
    ArtifactBundle,
    ArtifactPaths,
    ViewerAssets,
    ExportArtifacts,
    LatestDraft,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactBundleDigest {
    pub model_id: String,
    pub source_language: String,
    pub geometry_backend: String,
    pub has_preview_stl: bool,
    pub viewer_asset_count: usize,
    pub edge_target_count: usize,
    pub face_target_count: usize,
    pub export_format_count: usize,
    pub export_formats: Vec<String>,
    pub has_step_export: bool,
    pub step_export_path: Option<String>,
    pub multipart: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactManifestRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactManifestResponse {
    pub thread_id: String,
    pub message_id: String,
    pub model_id: String,
    pub digest: ArtifactBundleDigest,
    pub artifact_bundle: ArtifactBundle,
    pub model_manifest: ModelManifest,
    pub runtime_manifest_valid: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDetailRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub section: TargetDetailSection,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDetailResponse {
    pub thread_id: String,
    pub message_id: String,
    pub title: String,
    pub version_name: String,
    pub resolved_from: TargetResolvedFrom,
    pub section: TargetDetailSection,
    pub authoring_context: TargetAuthoringContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_spec: Option<UiSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_params: Option<DesignParams>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_bundle: Option<Option<ArtifactBundleDigest>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_paths: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_assets: Option<Vec<crate::models::ViewerAsset>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_artifacts: Option<Vec<crate::models::ExportArtifact>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_draft: Option<Option<()>>,
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
    pub artifact_digest: Option<ArtifactBundleDigest>,
    pub model_manifest: Option<ModelManifest>,
    pub latest_draft: Option<()>,
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
    pub control_primitive_count: usize,
    pub relation_count: usize,
    pub view_count: usize,
    pub advisory_count: usize,
    pub measurement_annotation_count: usize,
    pub part_count: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SemanticManifestSection {
    ControlPrimitives,
    ControlRelations,
    ControlViews,
    Advisories,
    MeasurementAnnotations,
    Parts,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticManifestDetailRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub section: SemanticManifestSection,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticManifestDetailResponse {
    pub thread_id: String,
    pub message_id: String,
    pub section: SemanticManifestSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_primitives: Option<Vec<ControlPrimitive>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_relations: Option<Vec<crate::models::ControlRelation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_views: Option<Vec<ControlView>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advisories: Option<Vec<crate::models::Advisory>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement_annotations: Option<Vec<MeasurementAnnotation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<crate::models::PartBinding>>,
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
    pub geometry_backend: Option<crate::models::GeometryBackend>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structural_verification: Option<StructuralVerificationResult>,
    pub artifact_digest: ArtifactBundleDigest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoginRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
    #[serde(default)]
    pub steal_thread: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoginResponse {
    pub session_id: String,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
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
pub struct SessionReplySaveRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub body: String,
    #[serde(default)]
    pub fatal: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReplySaveResponse {
    pub thread_id: String,
    pub message_id: String,
    pub fatal: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongActionNoticeRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub message: String,
    pub phase: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LongActionNoticeResponse {
    pub session_id: String,
    pub phase: String,
    pub busy: bool,
    pub activity_label: String,
    pub activity_started_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivitySetRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub phase: String,
    pub label: Option<String>,
    pub detail: Option<String>,
    pub attention_kind: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivitySetResponse {
    pub session_id: String,
    pub phase: String,
    pub busy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_label: Option<String>,
    pub activity_started_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongActionClearRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub phase: Option<String>,
    pub status_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LongActionClearResponse {
    pub session_id: String,
    pub phase: String,
    pub busy: bool,
    pub status_text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivityClearRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub phase: Option<String>,
    pub status_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivityClearResponse {
    pub session_id: String,
    pub phase: String,
    pub busy: bool,
    pub status_text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroReplaceRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub macro_code: String,
    pub macro_dialect: Option<crate::models::MacroDialect>,
    pub ui_spec: Option<UiSpec>,
    pub parameters: Option<DesignParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_processing: Option<crate::models::PostProcessingSpec>,
    pub geometry_backend: Option<crate::models::GeometryBackend>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structural_verification: Option<StructuralVerificationResult>,
    pub artifact_digest: ArtifactBundleDigest,
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
    #[serde(default)]
    pub message_id: Option<String>,
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
    pub artifact_digest: Option<ArtifactBundleDigest>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementAnnotationSaveRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub annotation: MeasurementAnnotation,
    pub title: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementAnnotationDeleteRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub annotation_id: String,
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

// ── Structural verification MCP ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyGeneratedModelRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
    pub original_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyGeneratedModelResponse {
    pub thread_id: String,
    pub message_id: String,
    pub model_id: String,
    pub artifact_digest: ArtifactBundleDigest,
    pub result: crate::contracts::StructuralVerificationResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralVerificationSummaryRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralVerificationSummaryResponse {
    pub thread_id: String,
    pub message_id: String,
    pub model_id: String,
    pub artifact_digest: ArtifactBundleDigest,
    pub passed: bool,
    pub summary: String,
    pub issue_count: usize,
    pub verifier_status: crate::contracts::VerifierStatus,
    pub verifier_source: Option<crate::contracts::VerifierSource>,
}
