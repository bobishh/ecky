use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

use super::{
    ArtifactBundle, DesignParams, GeometryBackend, MacroDialect, ModelManifest, SourceLanguage,
    UiSpec,
};

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

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SketchPrimitiveTopology {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_id: Option<String>,
    #[serde(default)]
    pub edge_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_role: Option<BrepProjectedLoopRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_class: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topology: Option<SketchPrimitiveTopology>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceSceneLens {
    Sketch,
    Draft,
    Exact,
}

impl WorkspaceSceneLens {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sketch => "sketch",
            Self::Draft => "draft",
            Self::Exact => "exact",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceSceneRepresentationKind {
    SketchIntent,
    MeshDraft,
    ExactModel,
}

impl WorkspaceSceneRepresentationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SketchIntent => "sketchIntent",
            Self::MeshDraft => "meshDraft",
            Self::ExactModel => "exactModel",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceSceneRepresentationStatus {
    Pending,
    Fresh,
    Stale,
    Rebuildable,
    Failed,
    Committed,
}

impl WorkspaceSceneRepresentationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Rebuildable => "rebuildable",
            Self::Failed => "failed",
            Self::Committed => "committed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSceneRepresentation {
    pub kind: WorkspaceSceneRepresentationKind,
    pub status: WorkspaceSceneRepresentationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSceneTopology {
    pub edge_target_count: usize,
    pub face_target_count: usize,
    pub selection_target_count: usize,
    pub control_primitive_count: usize,
    pub control_relation_count: usize,
    pub control_view_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentScenePacket {
    pub schema_version: u32,
    pub active_lens: WorkspaceSceneLens,
    pub representations: Vec<WorkspaceSceneRepresentation>,
    pub topology: WorkspaceSceneTopology,
    pub allowed_patch_targets: Vec<String>,
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
pub struct SketchPreviewDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    pub draft_source: SketchDraftSource,
    pub artifact_bundle: ArtifactBundle,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveSketchPreviewDraftRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    pub draft_source: SketchDraftSource,
    pub artifact_bundle: ArtifactBundle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadSketchPreviewDraftRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClearSketchPreviewDraftRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
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
pub enum BrepHiddenLineWarningKind {
    ProjectionNoEdges,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrepHiddenLineWarning {
    pub kind: BrepHiddenLineWarningKind,
    pub view: SketchView,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrepHiddenLineProjectionResponse {
    pub model_id: String,
    pub source_artifact_path: String,
    #[serde(default)]
    pub views: Vec<BrepHiddenLineProjectionView>,
    #[serde(default)]
    pub warning_entries: Vec<BrepHiddenLineWarning>,
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
pub enum SketchValidationIssueKind {
    MissingClosedProfile,
    MissingProjectionEdges,
    BoundsMismatch,
    ContainmentMismatch,
    TopologyMismatch,
    ConcavityMismatch,
    ProjectionReplayCoverageGap,
    CandidateGraphNoVertices,
    CandidateGraphNoEdges,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SketchValidationIssue {
    pub sketch_id: String,
    pub kind: SketchValidationIssueKind,
    pub view: SketchView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primitive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topology: Option<SketchPrimitiveTopology>,
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
