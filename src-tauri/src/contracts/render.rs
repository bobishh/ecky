use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;

use super::{
    default_engine_kind, default_geometry_backend, default_model_runtime_schema_version,
    default_source_language, AppError, AppResult, EngineKind, GeometryBackend, ModelSourceKind,
    PortFrame, SourceLanguage,
};

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ViewerAssetFormat {
    Stl,
    Gltf,
    Glb,
    Obj,
    #[serde(rename = "3mf")]
    #[specta(rename = "3mf")]
    ThreeMf,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_ids: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_ids: Vec<String>,
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

fn default_artifact_version() -> u32 {
    1
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
