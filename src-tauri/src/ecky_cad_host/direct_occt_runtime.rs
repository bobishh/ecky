#![allow(dead_code)]

use std::fs;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::direct_occt_executor::export_core_program_step_stl_with_params;
use super::direct_occt_sdk::{DirectOcctSdkLayout, NativeExportOutcome};
use crate::ecky_core_ir::CoreProgram;
use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignParams, DocumentMetadata, EngineKind,
    EnrichmentStatus, ExportArtifact, GeometryBackend, ManifestEnrichmentState, ModelManifest,
    ModelSourceKind, ParameterGroup, PartBinding, PathResolver, SelectionTarget,
    SelectionTargetKind, SourceLanguage, ViewerEdgePoint, ViewerEdgeTarget, ViewerFaceTarget,
    MODEL_RUNTIME_SCHEMA_VERSION,
};

const SOURCE_FILE_NAME: &str = "source.ecky";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
const STEP_FILE_NAME: &str = "model.step";
const TOPOLOGY_FILE_NAME: &str = "topology.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectOcctTopologyReport {
    #[serde(default)]
    parts: Vec<DirectOcctTopologyPart>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyPart {
    part_id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    edges: Vec<DirectOcctTopologyEdge>,
    #[serde(default)]
    faces: Vec<DirectOcctTopologyFace>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyEdge {
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    edge_index: Option<u32>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    start: Option<DirectOcctTopologyPoint>,
    #[serde(default)]
    end: Option<DirectOcctTopologyPoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyFace {
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    face_index: Option<u32>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    center: Option<DirectOcctTopologyPoint>,
    #[serde(default)]
    normal: Option<[f64; 3]>,
    #[serde(default)]
    area: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectOcctTopologyPoint {
    x: f64,
    y: f64,
    z: f64,
}

pub(crate) fn render_core_program_runtime_bundle(
    program: &CoreProgram,
    source_identity: &str,
    parameters: &DesignParams,
    layout: &DirectOcctSdkLayout,
    app: &dyn PathResolver,
) -> AppResult<(ArtifactBundle, ModelManifest)> {
    let params_json =
        serde_json::to_string(parameters).map_err(|err| AppError::validation(err.to_string()))?;
    let content_hash = content_hash(source_identity, &params_json);
    let model_id = model_id_from_hash(&content_hash);
    let bundle_dir = crate::model_runtime::runtime_bundle_dir(app, &model_id)?;

    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;
    let source_path = bundle_dir.join(SOURCE_FILE_NAME);
    fs::write(&source_path, source_identity)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let export_outcome =
        match export_core_program_step_stl_with_params(program, parameters, layout, &bundle_dir) {
            Ok(outcome) => outcome,
            Err(err) => {
                let _ = fs::remove_dir_all(&bundle_dir);
                return Err(err);
            }
        };

    match export_outcome {
        NativeExportOutcome::Exported {
            step_path,
            stl_path,
        } => {
            if program.parts.is_empty() {
                return Err(AppError::validation(
                    "Direct OCCT runtime requires at least one Core IR part.",
                ));
            }
            let topology_path = bundle_dir.join(TOPOLOGY_FILE_NAME);
            let topology_report = read_direct_occt_topology_report(&topology_path)?;
            let parameter_keys = program
                .parameters
                .iter()
                .map(|parameter| parameter.key.clone())
                .collect::<Vec<_>>();
            let part_specs = program
                .parts
                .iter()
                .map(|part| (part.key.clone(), part.label.clone()))
                .collect::<Vec<_>>();
            let manifest = build_direct_occt_manifest(
                &model_id,
                &source_path,
                &part_specs,
                &parameter_keys,
                topology_report.as_ref(),
            )?;
            let bundle = build_direct_occt_bundle(
                &model_id,
                &content_hash,
                &source_path,
                &stl_path,
                &step_path,
                topology_report.as_ref(),
                &manifest,
            )?;
            crate::model_runtime::write_runtime_bundle(app, &model_id, &bundle, &manifest)
        }
        NativeExportOutcome::Blocked { blockers } => {
            let _ = fs::remove_dir_all(&bundle_dir);
            Err(AppError::render(format!(
                "Direct OCCT runtime blocked: {}",
                if blockers.is_empty() {
                    "unknown runtime blocker".to_string()
                } else {
                    blockers.join("; ")
                }
            )))
        }
    }
}

pub(crate) fn build_direct_occt_manifest(
    model_id: &str,
    source_path: &Path,
    parts: &[(String, String)],
    parameter_keys: &[String],
    topology_report: Option<&DirectOcctTopologyReport>,
) -> AppResult<ModelManifest> {
    let part_bindings = direct_occt_part_bindings(parts, parameter_keys);
    let part_ids = part_bindings
        .iter()
        .map(|part| part.part_id.clone())
        .collect::<Vec<_>>();
    let selection_targets = direct_occt_selection_targets(&part_bindings, topology_report)?;

    Ok(ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        document: DocumentMetadata {
            document_name: "Direct OCCT".to_string(),
            document_label: "Direct OCCT".to_string(),
            source_path: Some(path_to_string(source_path)?),
            object_count: part_bindings.len(),
            warnings: Vec::new(),
        },
        parts: part_bindings,
        parameter_groups: vec![ParameterGroup {
            group_id: "core".to_string(),
            label: "Core".to_string(),
            parameter_keys: parameter_keys.to_vec(),
            part_ids,
            editable: true,
            presentation: Some("primary".to_string()),
            order: Some(0),
        }],
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
    })
}

fn direct_occt_part_bindings(
    parts: &[(String, String)],
    parameter_keys: &[String],
) -> Vec<PartBinding> {
    let specs = if parts.is_empty() {
        vec![("body".to_string(), "Body".to_string())]
    } else {
        parts.to_vec()
    };

    specs
        .into_iter()
        .enumerate()
        .map(|(index, (key, label))| {
            let fallback_id = if index == 0 {
                "body".to_string()
            } else {
                format!("part_{}", index + 1)
            };
            let part_id = if key.trim().is_empty() {
                fallback_id
            } else {
                key
            };
            let label = if label.trim().is_empty() {
                part_id.clone()
            } else {
                label
            };
            PartBinding {
                part_id: part_id.clone(),
                freecad_object_name: part_id.clone(),
                label,
                kind: "solid".to_string(),
                semantic_role: Some("generated".to_string()),
                viewer_asset_path: Some(PREVIEW_STL_FILE_NAME.to_string()),
                viewer_node_ids: vec![part_id.clone()],
                parameter_keys: parameter_keys.to_vec(),
                editable: true,
                bounds: None,
                volume: None,
                area: None,
            }
        })
        .collect()
}

pub(crate) fn build_direct_occt_bundle(
    model_id: &str,
    content_hash: &str,
    source_path: &Path,
    preview_stl_path: &Path,
    step_path: &Path,
    topology_report: Option<&DirectOcctTopologyReport>,
    manifest: &ModelManifest,
) -> AppResult<ArtifactBundle> {
    Ok(ArtifactBundle {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash: content_hash.to_string(),
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: MANIFEST_FILE_NAME.to_string(),
        macro_path: Some(path_to_string(source_path)?),
        preview_stl_path: path_to_string(preview_stl_path)?,
        viewer_assets: Vec::new(),
        edge_targets: direct_occt_edge_targets(topology_report, manifest),
        face_targets: direct_occt_face_targets(topology_report, manifest),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: direct_occt_step_export_artifacts(step_path)?,
    })
}

pub(crate) fn direct_occt_step_export_artifacts(
    step_path: &Path,
) -> AppResult<Vec<ExportArtifact>> {
    Ok(vec![ExportArtifact {
        label: "STEP".to_string(),
        format: "step".to_string(),
        path: path_to_string(step_path)?,
        role: "primary".to_string(),
    }])
}

fn content_hash(source_identity: &str, params_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_identity.as_bytes());
    hasher.update(b"|");
    hasher.update(params_json.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn model_id_from_hash(hash: &str) -> String {
    format!("generated-direct-occt-{}", &hash[..12])
}

fn read_direct_occt_topology_report(path: &Path) -> AppResult<Option<DirectOcctTopologyReport>> {
    if !path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path).map_err(|err| {
        AppError::persistence(format!(
            "Direct OCCT topology report could not be read '{}': {}",
            path.display(),
            err
        ))
    })?;
    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|err| AppError::validation(format!("Direct OCCT topology report invalid: {err}")))
}

fn direct_occt_selection_targets(
    part_bindings: &[PartBinding],
    topology_report: Option<&DirectOcctTopologyReport>,
) -> AppResult<Vec<SelectionTarget>> {
    let Some(topology_report) = topology_report else {
        return Ok(Vec::new());
    };
    let mut selection_targets = Vec::new();

    for topology_part in &topology_report.parts {
        let part_id = topology_part.part_id.trim();
        let Some(part_binding) = part_bindings.iter().find(|part| part.part_id == part_id) else {
            return Err(AppError::validation(format!(
                "Direct OCCT topology report references unknown partId '{}'.",
                topology_part.part_id
            )));
        };
        let viewer_node_id = part_binding
            .viewer_node_ids
            .first()
            .cloned()
            .unwrap_or_else(|| part_binding.part_id.clone());

        for edge in topology_part
            .edges
            .iter()
            .filter(|edge| edge.start.is_some() && edge.end.is_some())
        {
            selection_targets.push(SelectionTarget {
                target_id: Some(direct_occt_edge_target_id(part_id, edge)),
                part_id: part_binding.part_id.clone(),
                viewer_node_id: viewer_node_id.clone(),
                label: direct_occt_edge_label(topology_part, edge),
                kind: SelectionTargetKind::Edge,
                editable: part_binding.editable,
                parameter_keys: part_binding.parameter_keys.clone(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            });
        }

        for face in topology_part
            .faces
            .iter()
            .filter(|face| face.center.is_some())
        {
            selection_targets.push(SelectionTarget {
                target_id: Some(direct_occt_face_target_id(part_id, face)),
                part_id: part_binding.part_id.clone(),
                viewer_node_id: viewer_node_id.clone(),
                label: direct_occt_face_label(topology_part, face),
                kind: SelectionTargetKind::Face,
                editable: part_binding.editable,
                parameter_keys: part_binding.parameter_keys.clone(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            });
        }
    }

    Ok(selection_targets)
}

fn direct_occt_edge_targets(
    topology_report: Option<&DirectOcctTopologyReport>,
    manifest: &ModelManifest,
) -> Vec<ViewerEdgeTarget> {
    let Some(topology_report) = topology_report else {
        return Vec::new();
    };
    let selection_targets_by_id = manifest
        .selection_targets
        .iter()
        .filter(|target| target.kind == SelectionTargetKind::Edge)
        .filter_map(|target| {
            target
                .target_id
                .as_deref()
                .map(|target_id| (target_id, target))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut edge_targets = Vec::new();

    for topology_part in &topology_report.parts {
        let part_id = topology_part.part_id.trim();
        for edge in topology_part
            .edges
            .iter()
            .filter(|edge| edge.start.is_some() && edge.end.is_some())
        {
            let target_id = direct_occt_edge_target_id(part_id, edge);
            let Some(selection_target) = selection_targets_by_id.get(target_id.as_str()) else {
                continue;
            };
            let Some(start) = edge.start.as_ref() else {
                continue;
            };
            let Some(end) = edge.end.as_ref() else {
                continue;
            };

            edge_targets.push(ViewerEdgeTarget {
                target_id,
                part_id: selection_target.part_id.clone(),
                viewer_node_id: selection_target.viewer_node_id.clone(),
                label: direct_occt_edge_label(topology_part, edge),
                editable: selection_target.editable,
                start: direct_occt_point_to_viewer(start),
                end: direct_occt_point_to_viewer(end),
            });
        }
    }

    edge_targets
}

fn direct_occt_face_targets(
    topology_report: Option<&DirectOcctTopologyReport>,
    manifest: &ModelManifest,
) -> Vec<ViewerFaceTarget> {
    let Some(topology_report) = topology_report else {
        return Vec::new();
    };
    let selection_targets_by_id = manifest
        .selection_targets
        .iter()
        .filter(|target| target.kind == SelectionTargetKind::Face)
        .filter_map(|target| {
            target
                .target_id
                .as_deref()
                .map(|target_id| (target_id, target))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut face_targets = Vec::new();

    for topology_part in &topology_report.parts {
        let part_id = topology_part.part_id.trim();
        for face in topology_part
            .faces
            .iter()
            .filter(|face| face.center.is_some())
        {
            let target_id = direct_occt_face_target_id(part_id, face);
            let Some(selection_target) = selection_targets_by_id.get(target_id.as_str()) else {
                continue;
            };
            let Some(center) = face.center.as_ref() else {
                continue;
            };

            face_targets.push(ViewerFaceTarget {
                target_id,
                part_id: selection_target.part_id.clone(),
                viewer_node_id: selection_target.viewer_node_id.clone(),
                label: direct_occt_face_label(topology_part, face),
                editable: selection_target.editable,
                center: direct_occt_point_to_viewer(center),
                normal: face.normal,
                area: face.area,
            });
        }
    }

    face_targets
}

fn direct_occt_edge_target_id(part_id: &str, edge: &DirectOcctTopologyEdge) -> String {
    let explicit_target_id = edge.target_id.as_deref().unwrap_or_default().trim();
    if !explicit_target_id.is_empty() {
        return explicit_target_id.to_string();
    }

    let edge_index = edge
        .edge_index
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let edge_signature = edge
        .start
        .as_ref()
        .zip(edge.end.as_ref())
        .map(|(start, end)| direct_occt_edge_signature(start, end));
    match edge_signature {
        Some(signature) => format!("{part_id}:edge:{edge_index}:{signature}"),
        None => format!("{part_id}:edge:{edge_index}"),
    }
}

fn direct_occt_edge_label(
    topology_part: &DirectOcctTopologyPart,
    edge: &DirectOcctTopologyEdge,
) -> String {
    let label = edge.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }

    let part_label = if topology_part.label.trim().is_empty() {
        topology_part.part_id.trim()
    } else {
        topology_part.label.trim()
    };
    let edge_index = edge
        .edge_index
        .map(|index| index.saturating_add(1).to_string())
        .unwrap_or_else(|| "?".to_string());
    format!("{part_label}.Edge{edge_index}")
}

fn direct_occt_face_target_id(part_id: &str, face: &DirectOcctTopologyFace) -> String {
    let explicit_target_id = face.target_id.as_deref().unwrap_or_default().trim();
    if !explicit_target_id.is_empty() {
        return explicit_target_id.to_string();
    }

    let face_index = face
        .face_index
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let face_signature = face.center.as_ref().map(|center| {
        let center_signature = direct_occt_point_signature(center);
        let area_signature = face
            .area
            .map(format_topology_coordinate)
            .unwrap_or_else(|| "unknown".to_string());
        format!("{center_signature}:{area_signature}")
    });

    match face_signature {
        Some(signature) => format!("{part_id}:face:{face_index}:{signature}"),
        None => format!("{part_id}:face:{face_index}"),
    }
}

fn direct_occt_face_label(
    topology_part: &DirectOcctTopologyPart,
    face: &DirectOcctTopologyFace,
) -> String {
    let label = face.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }

    let part_label = if topology_part.label.trim().is_empty() {
        topology_part.part_id.trim()
    } else {
        topology_part.label.trim()
    };
    let face_index = face
        .face_index
        .map(|index| index.saturating_add(1).to_string())
        .unwrap_or_else(|| "?".to_string());
    format!("{part_label}.Face{face_index}")
}

fn direct_occt_point_to_viewer(point: &DirectOcctTopologyPoint) -> ViewerEdgePoint {
    ViewerEdgePoint {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}

fn direct_occt_point_signature(point: &DirectOcctTopologyPoint) -> String {
    [point.x, point.y, point.z]
        .into_iter()
        .map(format_topology_coordinate)
        .collect::<Vec<_>>()
        .join("-")
}

fn direct_occt_edge_signature(
    start: &DirectOcctTopologyPoint,
    end: &DirectOcctTopologyPoint,
) -> String {
    let mut endpoints = [
        direct_occt_point_signature(start),
        direct_occt_point_signature(end),
    ];
    endpoints.sort();
    endpoints.join("_")
}

fn format_topology_coordinate(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let formatted = format!("{value:.3}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" || trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn path_to_string(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| AppError::internal("Non-UTF-8 path encountered."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_cad_host::direct_occt_sdk::{
        bundled_build123d_runtime_root_from_repo, inspect_build123d_ocp_runtime,
    };
    use crate::models::{
        validate_model_runtime_bundle, ParamValue, PathResolver, SelectionTargetKind,
        ViewerAssetFormat,
    };
    use std::path::PathBuf;

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{label}-{}", uuid::Uuid::new_v4()))
    }

    fn compile(source: &str) -> CoreProgram {
        crate::ecky_scheme::compile_to_core_program(source).expect("compile")
    }

    fn blocked_layout(root: PathBuf) -> DirectOcctSdkLayout {
        DirectOcctSdkLayout {
            runtime_root: root,
            ocp_root: None,
            dylib_dir: None,
            include_dir: None,
            missing_headers: vec!["BRepPrimAPI_MakeBox.hxx".to_string()],
            missing_libs: vec!["TKernel".to_string()],
            install_name_prefix: "/DLC/OCP/.dylibs",
        }
    }

    #[test]
    fn builds_valid_direct_occt_bundle_manifest_for_exported_box() {
        let root = temp_root("direct-occt-bundle");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        fs::create_dir_all(&bundle_dir).expect("bundle dir");
        let source_path = bundle_dir.join(SOURCE_FILE_NAME);
        let preview_path = bundle_dir.join(PREVIEW_STL_FILE_NAME);
        let step_path = bundle_dir.join(STEP_FILE_NAME);
        fs::write(&source_path, source).expect("source");
        fs::write(&preview_path, b"solid preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21;").expect("step");

        let manifest = build_direct_occt_manifest(
            &model_id,
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            None,
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            &model_id,
            &hash,
            &source_path,
            &preview_path,
            &step_path,
            None,
            &manifest,
        )
        .expect("bundle");
        let (stored, stored_manifest) =
            crate::model_runtime::write_runtime_bundle(&resolver, &model_id, &bundle, &manifest)
                .expect("write runtime bundle");

        assert!(stored.fcstd_path.is_empty());
        assert_eq!(stored.geometry_backend, GeometryBackend::EckyRust);
        assert_eq!(stored.export_artifacts[0].format, "step");
        assert_eq!(stored.viewer_assets.len(), 1);
        assert_eq!(stored.viewer_assets[0].format, ViewerAssetFormat::Stl);
        assert_eq!(stored_manifest.parts[0].viewer_node_ids, vec!["body"]);

        let (read_bundle, read_manifest) =
            crate::model_runtime::read_runtime_bundle(&resolver, &model_id).expect("read");
        assert_eq!(read_bundle.model_id, model_id);
        assert_eq!(read_manifest.model_id, model_id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_manifest_preserves_multi_part_bindings() {
        let root = temp_root("direct-occt-multipart-manifest");
        let source_path = root.join(SOURCE_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model)").expect("source");

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[
                ("base".to_string(), "Base".to_string()),
                ("post".to_string(), "Post".to_string()),
            ],
            &["width".to_string()],
            None,
        )
        .expect("manifest");

        assert_eq!(manifest.document.object_count, 2);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "post"]
        );
        assert_eq!(
            manifest.parameter_groups[0].part_ids,
            vec!["base".to_string(), "post".to_string()]
        );
        assert!(manifest
            .parts
            .iter()
            .all(|part| part.viewer_asset_path.as_deref() == Some(PREVIEW_STL_FILE_NAME)));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_runtime_maps_topology_report_to_face_targets() {
        let root = temp_root("direct-occt-face-topology");
        let source_path = root.join(SOURCE_FILE_NAME);
        let preview_path = root.join(PREVIEW_STL_FILE_NAME);
        let step_path = root.join(STEP_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        fs::write(&preview_path, b"solid preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21;").expect("step");
        let topology = DirectOcctTopologyReport {
            parts: vec![DirectOcctTopologyPart {
                part_id: "body".to_string(),
                label: "Body".to_string(),
                edges: Vec::new(),
                faces: vec![DirectOcctTopologyFace {
                    target_id: None,
                    face_index: Some(0),
                    label: String::new(),
                    center: Some(DirectOcctTopologyPoint {
                        x: 5.0,
                        y: 10.0,
                        z: 15.0,
                    }),
                    normal: Some([0.0, 0.0, 1.0]),
                    area: Some(200.0),
                }],
            }],
        };

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            Some(&topology),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            "model-1",
            "hash",
            &source_path,
            &preview_path,
            &step_path,
            Some(&topology),
            &manifest,
        )
        .expect("bundle");

        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        let face_target_id = "body:face:0:5-10-15:200";
        assert!(manifest.selection_targets.iter().any(|target| {
            target.kind == SelectionTargetKind::Face
                && target.target_id.as_deref() == Some(face_target_id)
        }));
        assert_eq!(bundle.face_targets.len(), 1);
        assert_eq!(bundle.face_targets[0].target_id, face_target_id);
        assert_eq!(bundle.face_targets[0].part_id, "body");
        assert_eq!(bundle.face_targets[0].viewer_node_id, "body");
        assert_eq!(bundle.face_targets[0].label, "Body.Face1");
        assert_eq!(bundle.face_targets[0].center.x, 5.0);
        assert_eq!(bundle.face_targets[0].normal, Some([0.0, 0.0, 1.0]));
        assert_eq!(bundle.face_targets[0].area, Some(200.0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_runtime_maps_topology_report_to_edge_targets() {
        let root = temp_root("direct-occt-edge-topology");
        let source_path = root.join(SOURCE_FILE_NAME);
        let preview_path = root.join(PREVIEW_STL_FILE_NAME);
        let step_path = root.join(STEP_FILE_NAME);
        let topology_path = root.join(TOPOLOGY_FILE_NAME);
        fs::create_dir_all(&root).expect("root");
        fs::write(&source_path, "(model (part body (box 10 20 30)))").expect("source");
        fs::write(&preview_path, b"solid preview").expect("preview");
        fs::write(&step_path, b"ISO-10303-21;").expect("step");
        fs::write(
            &topology_path,
            r#"{"parts":[{"partId":"body","label":"Body","edges":[{"edgeIndex":0,"start":{"x":0.0,"y":0.0,"z":0.0},"end":{"x":10.0,"y":0.0,"z":0.0}}],"faces":[]}]}"#,
        )
        .expect("topology");
        let topology = read_direct_occt_topology_report(&topology_path)
            .expect("read topology")
            .expect("topology report");

        let manifest = build_direct_occt_manifest(
            "model-1",
            &source_path,
            &[("body".to_string(), "Body".to_string())],
            &Vec::<String>::new(),
            Some(&topology),
        )
        .expect("manifest");
        let bundle = build_direct_occt_bundle(
            "model-1",
            "hash",
            &source_path,
            &preview_path,
            &step_path,
            Some(&topology),
            &manifest,
        )
        .expect("bundle");

        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        let edge_target_id = "body:edge:0:0-0-0_10-0-0";
        assert!(manifest.selection_targets.iter().any(|target| {
            target.kind == SelectionTargetKind::Edge
                && target.target_id.as_deref() == Some(edge_target_id)
        }));
        assert_eq!(bundle.edge_targets.len(), 1);
        assert_eq!(bundle.edge_targets[0].target_id, edge_target_id);
        assert_eq!(bundle.edge_targets[0].part_id, "body");
        assert_eq!(bundle.edge_targets[0].viewer_node_id, "body");
        assert_eq!(bundle.edge_targets[0].label, "Body.Edge1");
        assert_eq!(bundle.edge_targets[0].start.x, 0.0);
        assert_eq!(bundle.edge_targets[0].end.x, 10.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_edge_target_id_sorts_endpoint_signature() {
        let forward = DirectOcctTopologyEdge {
            target_id: None,
            edge_index: Some(0),
            label: String::new(),
            start: Some(DirectOcctTopologyPoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            end: Some(DirectOcctTopologyPoint {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            }),
        };
        let reversed = DirectOcctTopologyEdge {
            target_id: None,
            edge_index: Some(0),
            label: String::new(),
            start: forward.end.clone(),
            end: forward.start.clone(),
        };

        assert_eq!(
            direct_occt_edge_target_id("body", &forward),
            "body:edge:0:0-0-0_10-0-0"
        );
        assert_eq!(
            direct_occt_edge_target_id("body", &forward),
            direct_occt_edge_target_id("body", &reversed)
        );
    }

    #[test]
    fn blocked_direct_occt_sdk_returns_error_without_bundle() {
        let root = temp_root("direct-occt-blocked");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 10 20 30)))";
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let program = compile(source);

        let err = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &blocked_layout(root.clone()),
            &resolver,
        )
        .expect_err("blocked runtime");

        assert!(
            err.to_string().contains("Direct OCCT runtime blocked"),
            "{err}"
        );
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        assert!(!bundle_dir.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_export_errors_remove_partial_bundle_dir() {
        let root = temp_root("direct-occt-export-error");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model)";
        let hash = content_hash(source, "{}");
        let model_id = model_id_from_hash(&hash);
        let program = CoreProgram::new(
            crate::ecky_core_ir::ProgramId::new(1),
            Vec::new(),
            Vec::new(),
        );

        let err = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &blocked_layout(root.clone()),
            &resolver,
        )
        .expect_err("invalid program");

        assert!(
            err.to_string().contains("requires one part") || err.to_string().contains("part"),
            "{err}"
        );
        let bundle_dir =
            crate::model_runtime::runtime_bundle_dir(&resolver, &model_id).expect("dir");
        assert!(!bundle_dir.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_occt_model_id_includes_parameter_values() {
        let source = "(model (params (number width 10)) (part body (box width 12 14)))";
        let params_a = DesignParams::from([("width".to_string(), ParamValue::Number(10.0))]);
        let params_b = DesignParams::from([("width".to_string(), ParamValue::Number(24.0))]);
        let params_a_json = serde_json::to_string(&params_a).expect("params a");
        let params_b_json = serde_json::to_string(&params_b).expect("params b");

        let model_id_a = model_id_from_hash(&content_hash(source, &params_a_json));
        let model_id_b = model_id_from_hash(&content_hash(source, &params_b_json));

        assert_ne!(model_id_a, model_id_b);
    }

    #[test]
    fn live_direct_occt_runtime_writes_bundle_manifest_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        if !layout.can_compile_native_shim() {
            return;
        }

        let root = temp_root("direct-occt-live-bundle");
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (union (box 10 10 10) (translate 8 0 0 (sphere 4)))))";
        let program = compile(source);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT runtime bundle");

        assert!(Path::new(&bundle.preview_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(
            std::fs::metadata(&bundle.preview_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert_eq!(manifest.parts[0].part_id, "body");
        validate_model_runtime_bundle(&manifest, &bundle).expect("runtime contract");
        assert!(!bundle.edge_targets.is_empty(), "missing edge targets");
        assert!(!bundle.face_targets.is_empty(), "missing face targets");
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Edge));
        assert!(manifest
            .selection_targets
            .iter()
            .any(|target| target.kind == SelectionTargetKind::Face));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_writes_multi_part_manifest_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        if !layout.can_compile_native_shim() {
            return;
        }

        let root = temp_root("direct-occt-live-multipart-bundle");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"
            (model
              (part base (box 20 14 3))
              (part post (translate 0 0 3 (cylinder 3 12))))
        "#;
        let program = compile(source);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT runtime bundle");

        assert!(Path::new(&bundle.preview_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert_eq!(manifest.document.object_count, 2);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "post"]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_uses_parameter_overrides_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        if !layout.can_compile_native_shim() {
            return;
        }

        let root = temp_root("direct-occt-live-params");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"
            (model
              (params (number width 10))
              (part body (box width 12 14)))
        "#;
        let program = compile(source);
        let params = DesignParams::from([("width".to_string(), ParamValue::Number(24.0))]);

        let (bundle, manifest) =
            render_core_program_runtime_bundle(&program, source, &params, &layout, &resolver)
                .expect("direct OCCT runtime bundle");

        assert!(Path::new(&bundle.preview_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert_eq!(manifest.parts[0].parameter_keys, vec!["width"]);
        assert_eq!(manifest.parameter_groups[0].parameter_keys, vec!["width"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_exports_snap_clip_fixture_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        if !layout.can_compile_native_shim() {
            return;
        }

        let root = temp_root("direct-occt-live-snap-clip");
        let resolver = TestResolver { root: root.clone() };
        let source = include_str!("../../tests/fixtures/cad/surface/direct_occt_snap_clip.ecky");
        let program = compile(source);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT snap clip runtime bundle");

        assert!(Path::new(&bundle.preview_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(
            std::fs::metadata(&bundle.preview_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert_eq!(manifest.document.object_count, 2);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["saddle", "latch"]
        );
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_direct_occt_runtime_exports_frame_array_bracket_fixture_when_sdk_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        if !layout.can_compile_native_shim() {
            return;
        }

        let root = temp_root("direct-occt-live-frame-array-bracket");
        let resolver = TestResolver { root: root.clone() };
        let source =
            include_str!("../../tests/fixtures/cad/surface/direct_occt_frame_array_bracket.ecky");
        let program = compile(source);

        let (bundle, manifest) = render_core_program_runtime_bundle(
            &program,
            source,
            &DesignParams::new(),
            &layout,
            &resolver,
        )
        .expect("direct OCCT frame/array bracket runtime bundle");

        assert!(Path::new(&bundle.preview_stl_path).is_file());
        assert!(Path::new(&bundle.export_artifacts[0].path).is_file());
        assert!(
            std::fs::metadata(&bundle.preview_stl_path)
                .expect("stl")
                .len()
                > 512
        );
        assert!(
            std::fs::metadata(&bundle.export_artifacts[0].path)
                .expect("step")
                .len()
                > 1024
        );
        assert_eq!(manifest.document.object_count, 1);
        assert_eq!(manifest.parts[0].part_id, "bracket");
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        let _ = fs::remove_dir_all(root);
    }
}
