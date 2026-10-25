use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
pub struct RasterTraceCalibration {
    pub physical_width: f64,
    pub physical_height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RasterTraceAssetIdentity {
    pub image_path: String,
    pub digest: String,
    pub width_pixels: u32,
    pub height_pixels: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RasterTraceProvenance {
    pub kind: String,
    pub asset: RasterTraceAssetIdentity,
    pub view: SketchView,
    pub calibration: RasterTraceCalibration,
    pub threshold: u8,
    pub invert: bool,
    pub contour_id: String,
    pub extractor_version: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<RasterTraceProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RasterTraceRequest {
    pub image_path: String,
    pub view: SketchView,
    pub calibration: RasterTraceCalibration,
    pub threshold: u8,
    #[serde(default)]
    pub invert: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RasterTraceContour {
    pub contour_id: String,
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    pub foreground_pixel_count: usize,
    pub signed_area: f64,
    pub provenance: RasterTraceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RasterTraceResponse {
    pub asset: RasterTraceAssetIdentity,
    pub contours: Vec<RasterTraceContour>,
    pub connected_component_count: usize,
    pub extractor_version: String,
    #[serde(default)]
    pub evidence: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch_document: Option<SketchDocument>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveSketchPreviewDraftRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    #[serde(default)]
    pub new_scope: bool,
    pub draft_source: SketchDraftSource,
    pub artifact_bundle: ArtifactBundle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch_document: Option<SketchDocument>,
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
    /// Optional live-reference export symbol selecting the top-level
    /// `define-component` exposed for `(import-component ...)`. When omitted,
    /// a valid Ecky-symbol `componentId` is the fallback. Interface metadata
    /// only; it does not change copy-inline vendoring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_language: Option<SourceLanguage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_backend: Option<GeometryBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_dialect: Option<MacroDialect>,
    /// Package-carried representation evidence required for live STEP
    /// components. A `.step` suffix alone never proves analytic geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_provenance: Option<crate::contracts::GeometryProvenance>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComponentMateNormalMode {
    Aligned,
    Opposed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComponentMirrorAxis {
    X,
    Y,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComponentMateStatus {
    Solved,
    Failed,
}

/// Durable explanation of one source-authored component placement. Geometry
/// backends consume the solved frame; inspection surfaces retain mate intent.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentPlacementEvidence {
    pub instance_id: String,
    pub component_id: String,
    pub source_port_ref: PortReference,
    pub target_port_ref: PortReference,
    pub placement_frame: PortFrame,
    pub normal_mode: ComponentMateNormalMode,
    pub roll_degrees: f64,
    pub offset: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_axis: Option<ComponentMirrorAxis>,
    pub mate_status: ComponentMateStatus,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resolved_fit_values: BTreeMap<String, ComponentInterfaceValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_end: Option<u32>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cairn: Option<super::CairnDistribution>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cairn: Option<super::CairnDistribution>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_provenance: Option<crate::contracts::GeometryProvenance>,
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

// --- Live package component imports (component-package-imports) ---
//
// These contracts describe the *live reference* mode (`(import-component ...)`),
// which is intentionally separate from copy-inline vendoring (MCP/UI
// `component_import` / `component_get`). Live references keep an exact
// `packageId@version:componentId` coordinate and require a dependency lock;
// they never copy package source into persisted authored source.

/// Schema version for the persisted dependency lock. Bumping it changes the
/// canonical lock digest for every lock.
pub const COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION: u32 = 1;

/// Canonical coordinate of a live-referenced package component.
/// Identity is `<packageId>@<packageVersion>:<componentId>`; package version is
/// the resolver version (component version stays interface metadata).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ComponentCoordinate {
    pub package_id: String,
    pub version: String,
    pub component_id: String,
}

impl ComponentCoordinate {
    /// Canonical identity string `<packageId>@<version>:<componentId>`.
    pub fn canonical_identity(&self) -> String {
        format!("{}@{}:{}", self.package_id, self.version, self.component_id)
    }
}

/// One component entry inside a dependency lock.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDependencyLockComponent {
    pub component_id: String,
    /// Selected export symbol (`entrySymbol`) or `None` when the component id
    /// fallback was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_symbol: Option<String>,
    /// `sha256:<hex>` digest of the resolved package payload that produced this
    /// component's source. Independent of the package-coordinate digest so a
    /// single payload lock survives coordinate reindexing.
    pub payload_digest: String,
    /// Static STEP entries set `step`; source entries set `source`. Optional
    /// only for schema-v1 source-lock backward compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_kind: Option<ComponentPayloadKind>,
    /// Required when `payloadKind=step`; copied from package provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_representation: Option<crate::contracts::GeometryRepresentation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComponentPayloadKind {
    Source,
    Step,
}

/// One package dependency inside a dependency lock.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDependencyLockEntry {
    pub package_id: String,
    pub version: String,
    /// `sha256:<hex>` digest of the package payload at install time.
    pub package_digest: String,
    pub components: Vec<ComponentDependencyLockComponent>,
}

/// Canonical dependency lock produced by successful live resolution and owned
/// by `Message.artifactBundle.componentDependencyLock`. Canonical ordering is
/// dependencies by `(packageId, version)`, components by `componentId`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDependencyLock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cairn: Option<super::CairnLock>,
    #[serde(default = "default_component_dependency_lock_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub dependencies: Vec<ComponentDependencyLockEntry>,
}

fn default_component_dependency_lock_schema_version() -> u32 {
    COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION
}

impl ComponentDependencyLock {
    pub fn package_digests(&self) -> std::collections::BTreeSet<String> {
        self.dependencies
            .iter()
            .map(|entry| entry.package_digest.clone())
            .chain(self.cairn.iter().flat_map(|cairn| {
                cairn
                    .releases
                    .iter()
                    .map(|release| release.package_digest.clone())
            }))
            .collect()
    }
    /// Returns a canonically-ordered copy (dependencies by `(packageId,
    /// version)`, components by `componentId`). Equal inputs always produce
    /// byte-identical canonical lock bytes and therefore equal digests.
    pub fn canonical(self) -> Self {
        let mut dependencies = self.dependencies;
        dependencies.sort_by(|a, b| {
            a.package_id
                .cmp(&b.package_id)
                .then_with(|| a.version.cmp(&b.version))
        });
        for entry in dependencies.iter_mut() {
            entry
                .components
                .sort_by(|a, b| a.component_id.cmp(&b.component_id));
        }
        Self {
            schema_version: self.schema_version,
            dependencies,
            cairn: self.cairn.map(super::CairnLock::canonical),
        }
    }

    /// Compact canonical JSON bytes used to compute the lock digest.
    pub fn canonical_bytes(&self) -> crate::contracts::AppResult<Vec<u8>> {
        serde_json::to_vec(&self.clone().canonical()).map_err(|err| {
            crate::contracts::AppError::internal(format!(
                "Cannot canonicalize component dependency lock: {err}"
            ))
        })
    }

    /// Validate lock ownership data before it is accepted as a committed
    /// version input. Ordering is normalized separately; duplicate coordinates
    /// or components are ambiguous and therefore never canonicalized away.
    pub fn validate(&self) -> crate::contracts::AppResult<()> {
        if let Some(cairn) = &self.cairn {
            cairn.validate()?;
            for dependency in &self.dependencies {
                if !cairn.releases.iter().any(|release| {
                    release.package_id == dependency.package_id
                        && release.version == dependency.version
                        && release.package_digest == dependency.package_digest
                }) {
                    return Err(super::AppError::validation(
                        "Imported component is outside the pinned Cairn package graph",
                    ));
                }
            }
        }
        if self.schema_version != COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION {
            return Err(crate::contracts::AppError::validation(format!(
                "Unsupported component dependency lock schemaVersion '{}'.",
                self.schema_version
            )));
        }
        let mut dependencies = std::collections::BTreeSet::new();
        for dependency in &self.dependencies {
            if dependency.package_id.trim().is_empty()
                || dependency.version.trim().is_empty()
                || dependency.package_digest.trim().is_empty()
            {
                return Err(crate::contracts::AppError::validation(
                    "Component dependency locks require non-empty packageId, version, and packageDigest.",
                ));
            }
            if !dependencies.insert((&dependency.package_id, &dependency.version)) {
                return Err(crate::contracts::AppError::validation(format!(
                    "Component dependency lock duplicates '{}@{}'.",
                    dependency.package_id, dependency.version
                )));
            }
            let mut components = std::collections::BTreeSet::new();
            for component in &dependency.components {
                if component.component_id.trim().is_empty()
                    || component.payload_digest.trim().is_empty()
                {
                    return Err(crate::contracts::AppError::validation(format!(
                        "Component dependency lock '{}' requires non-empty componentId and payloadDigest.",
                        dependency.package_id
                    )));
                }
                match component.payload_kind {
                    None | Some(ComponentPayloadKind::Source) => {
                        if component.payload_digest != dependency.package_digest {
                            return Err(crate::contracts::AppError::validation(format!(
                                "Source component dependency lock '{}@{}:{}' payloadDigest differs from packageDigest.",
                                dependency.package_id, dependency.version, component.component_id
                            )));
                        }
                        if component.geometry_representation.is_some() {
                            return Err(crate::contracts::AppError::validation(format!(
                                "Source component dependency lock '{}@{}:{}' cannot declare geometryRepresentation.",
                                dependency.package_id, dependency.version, component.component_id
                            )));
                        }
                    }
                    Some(ComponentPayloadKind::Step) => {
                        if !matches!(
                            component.geometry_representation.as_ref(),
                            Some(crate::contracts::GeometryRepresentation::AnalyticBrep)
                                | Some(crate::contracts::GeometryRepresentation::FacetedPolyBrep)
                                | Some(crate::contracts::GeometryRepresentation::Hybrid)
                        ) {
                            return Err(crate::contracts::AppError::validation(format!(
                                "STEP component dependency lock '{}@{}:{}' requires analyticBrep, facetedPolyBrep, or hybrid geometryRepresentation.",
                                dependency.package_id, dependency.version, component.component_id
                            )));
                        }
                    }
                }
                if !components.insert(&component.component_id) {
                    return Err(crate::contracts::AppError::validation(format!(
                        "Component dependency lock '{}@{}' duplicates componentId '{}'.",
                        dependency.package_id, dependency.version, component.component_id
                    )));
                }
            }
        }
        Ok(())
    }
}

/// A byte range in authored or ephemeral materialized source. Provenance uses
/// this host-owned value rather than adding package metadata to Core IR spans.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentImportSourceSpan {
    pub start: u32,
    pub end: u32,
}

/// Persisted and transient provenance for one live-referenced component.
/// Lives outside Core IR; equivalent records appear in `ArtifactBundle` and
/// `ModelManifest`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentImportOrigin {
    pub package_id: String,
    pub version: String,
    pub component_id: String,
    /// Model-local alias the export was bound to.
    pub alias: String,
    /// `sha256:<hex>` package payload digest.
    pub payload_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_span: Option<ComponentImportSourceSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_span: Option<ComponentImportSourceSpan>,
    /// Part ids that originated from this import's expansion.
    #[serde(default)]
    pub part_ids: Vec<String>,
    /// Raw Core node ids that originated from this import's expansion.
    #[serde(default)]
    pub node_ids: Vec<u64>,
}

impl ComponentImportOrigin {
    pub fn canonical_identity(&self) -> String {
        format!("{}@{}:{}", self.package_id, self.version, self.component_id)
    }
}

/// Validates the version-owned import evidence that ArtifactBundle and
/// ModelManifest persist in their named sidecar fields. Keeping this contract
/// here lets persistence/render callers prove equality without adding package
/// data to CoreProgram/CoreNode.
pub fn validate_component_import_evidence(
    lock: Option<&ComponentDependencyLock>,
    lock_digest: Option<&str>,
    bundle_origins: &[ComponentImportOrigin],
    manifest_origins: &[ComponentImportOrigin],
) -> crate::contracts::AppResult<()> {
    match (lock, lock_digest) {
        (None, None) => {
            if !bundle_origins.is_empty() || !manifest_origins.is_empty() {
                return Err(crate::contracts::AppError::validation(
                    "Component import origins require componentDependencyLock evidence.",
                ));
            }
        }
        (Some(lock), Some(lock_digest)) => {
            lock.validate()?;
            let actual = format!("sha256:{:x}", Sha256::digest(lock.canonical_bytes()?));
            if actual != lock_digest {
                return Err(crate::contracts::AppError::validation(format!(
                    "componentDependencyLockDigest '{}' does not match canonical lock digest '{}'.",
                    lock_digest, actual
                )));
            }
            for origin in bundle_origins.iter().chain(manifest_origins) {
                let dependency = lock.dependencies.iter().find(|dependency| {
                    dependency.package_id == origin.package_id
                        && dependency.version == origin.version
                });
                let Some(dependency) = dependency else {
                    return Err(crate::contracts::AppError::validation(format!(
                        "Component import origin '{}' is absent from componentDependencyLock.",
                        origin.canonical_identity()
                    )));
                };
                if !dependency.components.iter().any(|component| {
                    component.component_id == origin.component_id
                        && component.payload_digest == origin.payload_digest
                }) {
                    return Err(crate::contracts::AppError::validation(format!(
                        "Component import origin '{}' does not match its locked payload digest.",
                        origin.canonical_identity()
                    )));
                }
            }
        }
        _ => {
            return Err(crate::contracts::AppError::validation(
                "componentDependencyLock and componentDependencyLockDigest must be stored together.",
            ));
        }
    }

    let mut bundle_origins = bundle_origins.to_vec();
    let mut manifest_origins = manifest_origins.to_vec();
    canonicalize_component_import_origins(&mut bundle_origins);
    canonicalize_component_import_origins(&mut manifest_origins);
    if bundle_origins != manifest_origins {
        return Err(crate::contracts::AppError::validation(
            "ArtifactBundle.componentImportOrigins must equal ModelManifest.componentImportOrigins.",
        ));
    }
    Ok(())
}

fn canonicalize_component_import_origins(origins: &mut [ComponentImportOrigin]) {
    for origin in origins.iter_mut() {
        origin.part_ids.sort();
        origin.part_ids.dedup();
        origin.node_ids.sort_unstable();
        origin.node_ids.dedup();
    }
    origins.sort_by(|left, right| {
        left.package_id
            .cmp(&right.package_id)
            .then_with(|| left.version.cmp(&right.version))
            .then_with(|| left.component_id.cmp(&right.component_id))
            .then_with(|| left.alias.cmp(&right.alias))
    });
}

/// One ordered entry in the runtime-owned integrity sidecar. Paths are
/// normalized UTF-8 with `/` separators, sorted by path bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackagePayloadInventoryEntry {
    pub path: String,
    pub sha256: String,
}

/// Runtime-owned `ecky-integrity.json` sidecar. Records the package payload
/// digest plus an ordered per-file inventory. The sidecar itself is never part
/// of the package payload digest input (it would self-reference).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackagePayloadInventory {
    #[serde(default = "default_payload_inventory_schema_version")]
    pub schema_version: u32,
    pub package_digest: String,
    pub entries: Vec<PackagePayloadInventoryEntry>,
}

pub const PACKAGE_PAYLOAD_INVENTORY_SCHEMA_VERSION: u32 = 1;

fn default_payload_inventory_schema_version() -> u32 {
    PACKAGE_PAYLOAD_INVENTORY_SCHEMA_VERSION
}

/// Mutable coordinate-index record mapping an exact `packageId@version` to one
/// payload digest in the global content-addressed store. Unlocked authoring
/// resolves through this index; committed versions resolve their expected
/// digest directly and ignore index mutations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentCoordinateIndexEntry {
    pub package_id: String,
    pub version: String,
    pub package_digest: String,
}

#[cfg(test)]
mod component_import_contract_tests {
    use super::*;

    fn lock_component(
        component_id: &str,
        entry_symbol: Option<&str>,
        payload_digest: &str,
    ) -> ComponentDependencyLockComponent {
        ComponentDependencyLockComponent {
            component_id: component_id.to_string(),
            entry_symbol: entry_symbol.map(str::to_string),
            payload_digest: payload_digest.to_string(),
            payload_kind: None,
            geometry_representation: None,
        }
    }

    fn lock_entry(
        package_id: &str,
        version: &str,
        package_digest: &str,
        components: Vec<ComponentDependencyLockComponent>,
    ) -> ComponentDependencyLockEntry {
        ComponentDependencyLockEntry {
            package_id: package_id.to_string(),
            version: version.to_string(),
            package_digest: package_digest.to_string(),
            components,
        }
    }

    fn sample_lock_shuffled() -> ComponentDependencyLock {
        // Deliberately out of canonical order.
        ComponentDependencyLock {
            cairn: None,
            schema_version: COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
            dependencies: vec![
                lock_entry(
                    "bike.kit",
                    "1.2.0",
                    "sha256:bbb",
                    vec![
                        lock_component("cage", Some("cage-v2"), "sha256:bbb"),
                        lock_component("adapter", None, "sha256:bbb"),
                    ],
                ),
                lock_entry(
                    "bike.kit",
                    "1.0.0",
                    "sha256:aaa",
                    vec![lock_component("rail", None, "sha256:aaa")],
                ),
            ],
        }
    }

    #[test]
    fn coordinate_canonical_identity_is_exact_coordinate() {
        let coordinate = ComponentCoordinate {
            package_id: "bike.kit".to_string(),
            version: "1.2.0".to_string(),
            component_id: "cage".to_string(),
        };
        assert_eq!(coordinate.canonical_identity(), "bike.kit@1.2.0:cage");
    }

    #[test]
    fn lock_canonical_sorts_dependencies_and_components() {
        let canonical = sample_lock_shuffled().canonical();

        assert_eq!(canonical.dependencies.len(), 2);
        assert_eq!(canonical.dependencies[0].version, "1.0.0");
        assert_eq!(canonical.dependencies[1].version, "1.2.0");

        let cage_pkg = &canonical.dependencies[1];
        assert_eq!(cage_pkg.components.len(), 2);
        assert_eq!(cage_pkg.components[0].component_id, "adapter");
        assert_eq!(cage_pkg.components[1].component_id, "cage");
        assert_eq!(
            cage_pkg.components[1].entry_symbol.as_deref(),
            Some("cage-v2")
        );
    }

    #[test]
    fn lock_canonical_bytes_are_independent_of_input_order() {
        let bytes_a = sample_lock_shuffled()
            .canonical()
            .canonical_bytes()
            .expect("bytes a");
        // Same contents, different input order.
        let mut reversed = sample_lock_shuffled();
        reversed.dependencies.reverse();
        reversed.dependencies[0].components.reverse();
        let bytes_b = reversed.canonical().canonical_bytes().expect("bytes b");

        assert_eq!(bytes_a, bytes_b);
    }

    #[test]
    fn lock_json_uses_camel_case_boundary() {
        let lock = sample_lock_shuffled().canonical();
        let json = serde_json::to_value(&lock).expect("serialize lock");
        assert!(json.get("schemaVersion").is_some());
        assert!(json.get("dependencies").is_some());
        assert!(json.get("schema_version").is_none());
        assert_eq!(json["dependencies"][0]["packageId"], "bike.kit");
        assert_eq!(
            json["dependencies"][0]["components"][0]["payloadDigest"],
            "sha256:aaa"
        );
    }

    #[test]
    fn step_header_and_lock_evidence_use_camel_case_contract_fields() {
        let header = ComponentHeader {
            component_id: "bracket".to_string(),
            version: "1.0.0".to_string(),
            display_name: "Bracket".to_string(),
            entry_symbol: None,
            geometry_provenance: Some(crate::contracts::GeometryProvenance {
                representation: crate::contracts::GeometryRepresentation::AnalyticBrep,
                source_mesh_digests: Vec::new(),
                closed: None,
                boundary_or_non_manifold_edge_count: None,
            }),
            params: Vec::new(),
            ui_spec: UiSpec::default(),
            initial_params: DesignParams::new(),
            ports: Vec::new(),
        };
        let header_json = serde_json::to_value(&header).expect("header json");
        assert_eq!(
            header_json["geometryProvenance"]["representation"],
            "analyticBrep"
        );
        assert!(header_json.get("geometry_provenance").is_none());

        let step_digest = format!("sha256:{}", "b".repeat(64));
        let lock = ComponentDependencyLock {
            cairn: None,
            schema_version: COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
            dependencies: vec![lock_entry(
                "fixture.step",
                "1.0.0",
                &format!("sha256:{}", "a".repeat(64)),
                vec![ComponentDependencyLockComponent {
                    component_id: "bracket".to_string(),
                    entry_symbol: None,
                    payload_digest: step_digest,
                    payload_kind: Some(ComponentPayloadKind::Step),
                    geometry_representation: Some(
                        crate::contracts::GeometryRepresentation::FacetedPolyBrep,
                    ),
                }],
            )],
        };
        lock.validate().expect("valid STEP lock");
        let lock_json = serde_json::to_value(&lock).expect("lock json");
        let component = &lock_json["dependencies"][0]["components"][0];
        assert_eq!(component["payloadKind"], "step");
        assert_eq!(component["geometryRepresentation"], "facetedPolyBrep");
        assert!(component.get("payload_kind").is_none());
        assert!(component.get("geometry_representation").is_none());
    }

    #[test]
    fn import_evidence_requires_matching_lock_digest_and_bundle_manifest_origins() {
        let lock = sample_lock_shuffled().canonical();
        let digest = format!(
            "sha256:{:x}",
            Sha256::digest(lock.canonical_bytes().unwrap())
        );
        let origin = ComponentImportOrigin {
            package_id: "bike.kit".to_string(),
            version: "1.0.0".to_string(),
            component_id: "rail".to_string(),
            alias: "rail".to_string(),
            payload_digest: "sha256:aaa".to_string(),
            authored_span: None,
            resolved_span: None,
            part_ids: vec!["holder".to_string()],
            node_ids: vec![7],
        };
        validate_component_import_evidence(
            Some(&lock),
            Some(&digest),
            std::slice::from_ref(&origin),
            std::slice::from_ref(&origin),
        )
        .expect("matching evidence");

        let err = validate_component_import_evidence(
            Some(&lock),
            Some(&digest),
            std::slice::from_ref(&origin),
            &[],
        )
        .expect_err("manifest provenance drift must fail");
        assert!(
            err.message.contains("componentImportOrigins"),
            "{}",
            err.message
        );
    }

    #[test]
    fn coordinate_index_entry_json_uses_camel_case_boundary() {
        let entry = ComponentCoordinateIndexEntry {
            package_id: "bike.kit".to_string(),
            version: "1.2.0".to_string(),
            package_digest: "sha256:aaa".to_string(),
        };
        let json = serde_json::to_value(&entry).expect("serialize index entry");
        assert!(json.get("packageDigest").is_some());
        assert!(json.get("package_digest").is_none());
    }
}
