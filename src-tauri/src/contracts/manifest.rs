use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, HashSet};

use super::{
    AppError, AppResult, ComponentInterfaceValue, EngineKind, GeometryBackend, PortFrame,
    SourceLanguage, MODEL_RUNTIME_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelSourceKind {
    Generated,
    ImportedFcstd,
    ImportedStep,
    ImportedMesh,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_target_id: Option<String>,
    #[serde(default)]
    pub alias_ids: Vec<String>,
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
pub struct PreviewViewOffset {
    pub part_id: String,
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    pub view_id: String,
    pub label: String,
    #[serde(default)]
    pub offsets: Vec<PreviewViewOffset>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_byte: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_byte: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeatureOutputRef {
    pub feature_id: String,
    pub output_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeaturePort {
    pub port_id: String,
    pub type_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<PortFrame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ComponentInterfaceValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeatureNode {
    pub feature_id: String,
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependency_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_refs: Vec<FeatureOutputRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<FeaturePort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeatureGraph {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<FeatureNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceEdge {
    pub edge_id: String,
    pub source: FeatureOutputRef,
    pub target: FeatureOutputRef,
    pub relation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<SourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceGraph {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<CorrespondenceEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaggedAnchorKind {
    Face,
    Edge,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaggedAnchorBinding {
    pub kind: TaggedAnchorKind,
    pub authored_selector: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub durable_target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub canonical_target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    #[serde(default = "default_model_runtime_schema_version")]
    pub schema_version: u32,
    pub model_id: String,
    pub source_kind: ModelSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ast_schema_version: Option<u32>,
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
    pub preview_views: Vec<PreviewView>,
    #[serde(default)]
    pub advisories: Vec<Advisory>,
    #[serde(default)]
    pub selection_targets: Vec<SelectionTarget>,
    #[serde(default)]
    pub measurement_annotations: Vec<MeasurementAnnotation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tagged_anchors: BTreeMap<String, TaggedAnchorBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_graph: Option<FeatureGraph>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correspondence_graph: Option<CorrespondenceGraph>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default = "default_manifest_enrichment_state")]
    pub enrichment_state: ManifestEnrichmentState,
}

fn default_model_runtime_schema_version() -> u32 {
    MODEL_RUNTIME_SCHEMA_VERSION
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

fn validate_feature_source_ref_path(
    path: &str,
    owner_id: &str,
    owner_kind: &str,
    part_ids: &HashSet<&str>,
    parameter_keys: &HashSet<&str>,
) -> AppResult<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() || !trimmed.starts_with('/') {
        return Err(AppError::validation(format!(
            "{owner_kind} '{owner_id}' has invalid sourceRef.path '{}'.",
            path
        )));
    }

    let segments = trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return Ok(());
    }

    match segments[0] {
        "parts" => {
            let part_id = segments[1];
            if !part_ids.contains(part_id) {
                return Err(AppError::validation(format!(
                    "{owner_kind} '{owner_id}' references stale sourceRef partId '{}'.",
                    part_id
                )));
            }
        }
        "params" => {
            let parameter_key = segments[1];
            if !parameter_keys.contains(parameter_key) {
                return Err(AppError::validation(format!(
                    "{owner_kind} '{owner_id}' references stale sourceRef parameterKey '{}'.",
                    parameter_key
                )));
            }
        }
        _ => {}
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

    for view in &manifest.preview_views {
        if view.view_id.trim().is_empty() {
            return Err(AppError::validation(
                "preview views must include a non-empty viewId.",
            ));
        }
        if !view_ids.insert(view.view_id.as_str()) {
            return Err(AppError::validation(format!(
                "preview view '{}' is duplicated.",
                view.view_id
            )));
        }
        if view.label.trim().is_empty() {
            return Err(AppError::validation(format!(
                "preview view '{}' must include a non-empty label.",
                view.view_id
            )));
        }
        for offset in &view.offsets {
            if !part_ids.contains(offset.part_id.as_str()) {
                return Err(AppError::validation(format!(
                    "preview view '{}' references unknown partId '{}'.",
                    view.view_id, offset.part_id
                )));
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
        if let Some(durable_target_id) = target.durable_target_id.as_deref() {
            if durable_target_id.trim().is_empty() {
                return Err(AppError::validation(
                    "selection targets with durableTargetId must use a non-empty value.",
                ));
            }
            if !selection_target_ids.insert(durable_target_id) {
                return Err(AppError::validation(format!(
                    "selection target durable id '{}' is duplicated.",
                    durable_target_id
                )));
            }
        }
        if let Some(canonical_target_id) = target.canonical_target_id.as_deref() {
            if canonical_target_id.trim().is_empty() {
                return Err(AppError::validation(
                    "selection targets with canonicalTargetId must use a non-empty value.",
                ));
            }
            if !selection_target_ids.insert(canonical_target_id) {
                return Err(AppError::validation(format!(
                    "selection target canonical id '{}' is duplicated.",
                    canonical_target_id
                )));
            }
        }
        for alias_id in &target.alias_ids {
            if alias_id.trim().is_empty() {
                return Err(AppError::validation(
                    "selection targets with aliasIds must use non-empty values.",
                ));
            }
            if !selection_target_ids.insert(alias_id) {
                return Err(AppError::validation(format!(
                    "selection target alias '{}' is duplicated.",
                    alias_id
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

    for (tag_name, anchor) in &manifest.tagged_anchors {
        if tag_name.trim().is_empty() {
            return Err(AppError::validation(
                "taggedAnchors must use non-empty tag names.",
            ));
        }
        if anchor.authored_selector.trim().is_empty() {
            return Err(AppError::validation(format!(
                "tagged anchor '{}' must use a non-empty authoredSelector.",
                tag_name
            )));
        }
        if anchor.target.trim().is_empty() {
            return Err(AppError::validation(format!(
                "tagged anchor '{}' must use a non-empty target.",
                tag_name
            )));
        }
        if anchor.target_ids.is_empty() {
            return Err(AppError::validation(format!(
                "tagged anchor '{}' must record at least one targetId.",
                tag_name
            )));
        }
        for target_id in &anchor.target_ids {
            if !selection_target_ids.contains(target_id.as_str()) {
                return Err(AppError::validation(format!(
                    "tagged anchor '{}' references unknown targetId '{}'.",
                    tag_name, target_id
                )));
            }
        }
        for durable_target_id in &anchor.durable_target_ids {
            if !selection_target_ids.contains(durable_target_id.as_str()) {
                return Err(AppError::validation(format!(
                    "tagged anchor '{}' references unknown durableTargetId '{}'.",
                    tag_name, durable_target_id
                )));
            }
        }
        for canonical_target_id in &anchor.canonical_target_ids {
            if !selection_target_ids.contains(canonical_target_id.as_str()) {
                return Err(AppError::validation(format!(
                    "tagged anchor '{}' references unknown canonicalTargetId '{}'.",
                    tag_name, canonical_target_id
                )));
            }
        }
        for alias_id in &anchor.alias_ids {
            if !selection_target_ids.contains(alias_id.as_str()) {
                return Err(AppError::validation(format!(
                    "tagged anchor '{}' references unknown aliasId '{}'.",
                    tag_name, alias_id
                )));
            }
        }
    }

    if let Some(feature_graph) = manifest.feature_graph.as_ref() {
        let mut feature_ids = HashSet::new();
        for node in &feature_graph.nodes {
            if node.feature_id.trim().is_empty() {
                return Err(AppError::validation(
                    "feature graph nodes must include non-empty featureId values.",
                ));
            }
            if !feature_ids.insert(node.feature_id.as_str()) {
                return Err(AppError::validation(format!(
                    "feature '{}' is duplicated.",
                    node.feature_id
                )));
            }
        }

        let mut feature_port_ids = HashSet::new();
        let mut feature_output_refs = HashSet::new();
        for node in &feature_graph.nodes {
            if let Some(source_ref) = node.source_ref.as_ref() {
                if let Some(path) = source_ref.path.as_deref() {
                    validate_feature_source_ref_path(
                        path,
                        &node.feature_id,
                        "feature",
                        &part_ids,
                        &known_parameter_keys,
                    )?;
                }
                if let (Some(start), Some(end)) = (source_ref.start_byte, source_ref.end_byte) {
                    if start >= end {
                        return Err(AppError::validation(format!(
                            "feature '{}' has invalid sourceRef byte range {}..{}.",
                            node.feature_id, start, end
                        )));
                    }
                }
            }
            for dependency_id in &node.dependency_ids {
                let is_feature_dependency = feature_ids.contains(dependency_id.as_str());
                let is_param_dependency = known_parameter_keys.contains(dependency_id.as_str())
                    || dependency_id
                        .strip_prefix("/params/")
                        .is_some_and(|key| known_parameter_keys.contains(key));
                if !is_feature_dependency && !is_param_dependency {
                    return Err(AppError::validation(format!(
                        "feature '{}' references unknown dependency '{}'.",
                        node.feature_id, dependency_id
                    )));
                }
            }
            for output in &node.output_refs {
                if output.feature_id != node.feature_id {
                    return Err(AppError::validation(format!(
                        "feature '{}' outputRef uses mismatched featureId '{}'.",
                        node.feature_id, output.feature_id
                    )));
                }
                if output.output_id.trim().is_empty() {
                    return Err(AppError::validation(format!(
                        "feature '{}' contains an outputRef with an empty outputId.",
                        node.feature_id
                    )));
                }
                let output_ref_key = format!("{}::{}", output.feature_id, output.output_id);
                if !feature_output_refs.insert(output_ref_key.clone()) {
                    return Err(AppError::validation(format!(
                        "feature outputRef '{}' is duplicated.",
                        output_ref_key
                    )));
                }
                for target_id in &output.target_ids {
                    if !selection_target_ids.contains(target_id.as_str()) {
                        return Err(AppError::validation(format!(
                            "feature '{}' outputRef '{}' references unknown targetId '{}'.",
                            node.feature_id, output.output_id, target_id
                        )));
                    }
                }
            }
            for port in &node.ports {
                if port.port_id.trim().is_empty() {
                    return Err(AppError::validation(format!(
                        "feature '{}' contains a port with an empty portId.",
                        node.feature_id
                    )));
                }
                if !feature_port_ids.insert(port.port_id.as_str()) {
                    return Err(AppError::validation(format!(
                        "feature port '{}' is duplicated.",
                        port.port_id
                    )));
                }
                if port.type_id.trim().is_empty() {
                    return Err(AppError::validation(format!(
                        "feature port '{}' must include a non-empty typeId.",
                        port.port_id
                    )));
                }
                if let Some(confidence) = port.confidence {
                    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
                        return Err(AppError::validation(format!(
                            "feature port '{}' confidence must be between 0 and 1.",
                            port.port_id
                        )));
                    }
                }
                if let Some(source_ref) = port.source_ref.as_ref() {
                    if let Some(path) = source_ref.path.as_deref() {
                        validate_feature_source_ref_path(
                            path,
                            &port.port_id,
                            "feature port",
                            &part_ids,
                            &known_parameter_keys,
                        )?;
                    }
                    if let (Some(start), Some(end)) = (source_ref.start_byte, source_ref.end_byte) {
                        if start >= end {
                            return Err(AppError::validation(format!(
                                "feature port '{}' has invalid sourceRef byte range {}..{}.",
                                port.port_id, start, end
                            )));
                        }
                    }
                }
                for target_id in &port.target_ids {
                    if !selection_target_ids.contains(target_id.as_str()) {
                        return Err(AppError::validation(format!(
                            "feature port '{}' references unknown targetId '{}'.",
                            port.port_id, target_id
                        )));
                    }
                }
            }
        }

        if let Some(correspondence_graph) = manifest.correspondence_graph.as_ref() {
            let mut edge_ids = HashSet::new();
            for edge in &correspondence_graph.edges {
                if edge.edge_id.trim().is_empty() {
                    return Err(AppError::validation(
                        "correspondence graph edges must include non-empty edgeId values.",
                    ));
                }
                if !edge_ids.insert(edge.edge_id.as_str()) {
                    return Err(AppError::validation(format!(
                        "correspondence edge '{}' is duplicated.",
                        edge.edge_id
                    )));
                }
                if edge.relation.trim().is_empty() {
                    return Err(AppError::validation(format!(
                        "correspondence edge '{}' must include a non-empty relation.",
                        edge.edge_id
                    )));
                }
                for output in [&edge.source, &edge.target] {
                    if output.feature_id.trim().is_empty() || output.output_id.trim().is_empty() {
                        return Err(AppError::validation(format!(
                            "correspondence edge '{}' references an empty feature/output id.",
                            edge.edge_id
                        )));
                    }
                    let output_ref_key = format!("{}::{}", output.feature_id, output.output_id);
                    if !feature_output_refs.contains(output_ref_key.as_str()) {
                        return Err(AppError::validation(format!(
                            "correspondence edge '{}' references unknown feature output '{}'.",
                            edge.edge_id, output_ref_key
                        )));
                    }
                    for target_id in &output.target_ids {
                        if !selection_target_ids.contains(target_id.as_str()) {
                            return Err(AppError::validation(format!(
                                "correspondence edge '{}' references unknown targetId '{}'.",
                                edge.edge_id, target_id
                            )));
                        }
                    }
                }
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
