use std::fs;
use std::path::{Path, PathBuf};

use csgrs::float_types::parry3d::na::Vector3;
use csgrs::traits::CSG;
use sha2::{Digest, Sha256};

use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignParams, DocumentMetadata, EngineKind,
    GeometryBackend, ManifestBounds, ModelManifest, ModelSourceKind, ParameterGroup,
    ParsedParamsResult, PartBinding, PathResolver, SourceLanguage, ViewerAsset, ViewerAssetFormat,
    MODEL_RUNTIME_SCHEMA_VERSION,
};

use super::mesh_ops::eval_geometry_expr;
use super::model::{build_param_env, parse_model, parsed_params_from_model, IrModel};
use super::shared::{validation, IrMesh};
use super::syntax::canonicalize;

pub(super) const MODEL_RUNTIME_ROOT: &str = "model-runtime";
pub(super) const GENERATED_ARTIFACT_DIR: &str = "generated";
pub(super) const BUNDLE_FILE_NAME: &str = "bundle.json";
pub(super) const MANIFEST_FILE_NAME: &str = "manifest.json";
pub(super) const SOURCE_FILE_NAME: &str = "source.ecky";
pub(super) const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
pub(super) const PARTS_DIR_NAME: &str = "parts";
pub(super) fn mesh_volume(mesh: &IrMesh) -> Option<f64> {
    let tri_mesh = mesh.triangulate();
    if tri_mesh.polygons.is_empty() {
        return None;
    }
    let mut volume = 0.0f64;
    for poly in &tri_mesh.polygons {
        if poly.vertices.len() != 3 {
            continue;
        }
        let a = &poly.vertices[0].pos;
        let b = &poly.vertices[1].pos;
        let c = &poly.vertices[2].pos;
        // Signed volume of tetrahedron formed with origin
        let cross = Vector3::new(
            b.y * c.z - b.z * c.y,
            b.z * c.x - b.x * c.z,
            b.x * c.y - b.y * c.x,
        );
        volume += a.x * cross.x + a.y * cross.y + a.z * cross.z;
    }
    let vol = (volume / 6.0).abs();
    if vol.is_finite() && vol > 0.0 {
        Some(vol)
    } else {
        None
    }
}

/// Compute the total surface area of a triangulated mesh.
///
/// For each triangle with vertices (a, b, c):
///   area = ||(b - a) × (c - a)|| / 2
pub(super) fn mesh_area(mesh: &IrMesh) -> Option<f64> {
    let tri_mesh = mesh.triangulate();
    if tri_mesh.polygons.is_empty() {
        return None;
    }
    let mut area = 0.0f64;
    for poly in &tri_mesh.polygons {
        if poly.vertices.len() != 3 {
            continue;
        }
        let a = &poly.vertices[0].pos;
        let b = &poly.vertices[1].pos;
        let c = &poly.vertices[2].pos;
        let ab = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let ac = Vector3::new(c.x - a.x, c.y - a.y, c.z - a.z);
        let cross = ab.cross(&ac);
        area += cross.norm();
    }
    let result = area / 2.0;
    if result.is_finite() && result > 0.0 {
        Some(result)
    } else {
        None
    }
}

pub(super) fn bounds_from_mesh(mesh: &IrMesh) -> ManifestBounds {
    let bb = mesh.bounding_box();
    ManifestBounds {
        x_min: bb.mins.x,
        y_min: bb.mins.y,
        z_min: bb.mins.z,
        x_max: bb.maxs.x,
        y_max: bb.maxs.y,
        z_max: bb.maxs.z,
    }
}

pub(super) fn runtime_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    let root = app.app_data_dir().join(MODEL_RUNTIME_ROOT);
    fs::create_dir_all(&root).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(root)
}

pub(super) fn bundle_dir(app: &dyn PathResolver, model_id: &str) -> AppResult<PathBuf> {
    let path = runtime_root(app)?
        .join(GENERATED_ARTIFACT_DIR)
        .join(model_id);
    fs::create_dir_all(&path).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(path)
}

pub(super) fn write_bundle(path: &Path, bundle: &ArtifactBundle) -> AppResult<()> {
    let data = serde_json::to_string_pretty(bundle)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

pub(super) fn write_manifest(path: &Path, manifest: &ModelManifest) -> AppResult<()> {
    let data = serde_json::to_string_pretty(manifest)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

pub fn derive_controls(source: &str) -> AppResult<ParsedParamsResult> {
    let model = parse_model(source)?;
    derive_controls_from_model(&model)
}

pub(crate) fn derive_controls_from_model(model: &IrModel) -> AppResult<ParsedParamsResult> {
    Ok(parsed_params_from_model(model))
}

pub(super) fn load_cached_bundle(bundle_dir: &Path) -> AppResult<Option<ArtifactBundle>> {
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    if !bundle_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&bundle_path)
        .map_err(|e| AppError::persistence(format!("Failed to read bundle: {}", e)))?;
    let bundle: ArtifactBundle = serde_json::from_str(&raw)
        .map_err(|e| AppError::parse(format!("Failed to parse bundle: {}", e)))?;
    if !Path::new(&bundle.manifest_path).exists() || !Path::new(&bundle.preview_stl_path).exists() {
        return Ok(None);
    }
    Ok(Some(bundle))
}

pub fn render_model(
    source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let model = parse_model(source)?;
    let canonical_source = canonicalize(source)?;
    render_model_from_model(&model, &canonical_source, parameters, app)
}

pub(crate) fn render_model_from_model(
    model: &IrModel,
    source_identity: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let params_json = serde_json::to_string(parameters).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(source_identity.as_bytes());
    hasher.update(b"|");
    hasher.update(params_json.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let model_id = format!("generated-ir-{}", &hash[..12]);
    let dir = bundle_dir(app, &model_id)?;

    if let Some(cached) = load_cached_bundle(&dir)? {
        return Ok(cached);
    }

    let env = build_param_env(model, parameters);
    let parts_dir = dir.join(PARTS_DIR_NAME);
    fs::create_dir_all(&parts_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let mut part_bindings = Vec::new();
    let mut viewer_assets = Vec::new();
    let mut preview_mesh: Option<IrMesh> = None;

    for (index, part) in model.parts.iter().enumerate() {
        let mesh = eval_geometry_expr(&part.expr, &env)?.into_mesh("part")?;
        let part_path = parts_dir.join(format!("{}-{}.stl", index + 1, part.part_id));
        fs::write(
            &part_path,
            mesh.to_stl_binary(&part.part_id)
                .map_err(|err| AppError::persistence(format!("Failed to encode STL: {}", err)))?,
        )
        .map_err(|err| AppError::persistence(err.to_string()))?;

        preview_mesh = Some(match preview_mesh.take() {
            Some(existing) => existing.union(&mesh),
            None => mesh.clone(),
        });

        let asset_path = part_path.to_string_lossy().to_string();
        viewer_assets.push(ViewerAsset {
            part_id: part.part_id.clone(),
            node_id: part.part_id.clone(),
            object_name: part.part_id.clone(),
            label: part.label.clone(),
            path: asset_path.clone(),
            format: ViewerAssetFormat::Stl,
        });
        part_bindings.push(PartBinding {
            part_id: part.part_id.clone(),
            freecad_object_name: part.part_id.clone(),
            label: part.label.clone(),
            kind: "solid".to_string(),
            semantic_role: Some("generated".to_string()),
            viewer_asset_path: Some(asset_path),
            viewer_node_ids: vec![part.part_id.clone()],
            parameter_keys: model
                .params
                .iter()
                .map(|param| param.field.key().to_string())
                .collect(),
            editable: true,
            bounds: Some(bounds_from_mesh(&mesh)),
            volume: mesh_volume(&mesh),
            area: mesh_area(&mesh),
        });
    }

    let preview_mesh =
        preview_mesh.ok_or_else(|| validation("Ecky IR v0 model produced no printable parts."))?;
    let preview_path = dir.join(PREVIEW_STL_FILE_NAME);
    fs::write(
        &preview_path,
        preview_mesh.to_stl_binary("preview").map_err(|err| {
            AppError::persistence(format!("Failed to encode preview STL: {}", err))
        })?,
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;

    let macro_path = dir.join(SOURCE_FILE_NAME);
    fs::write(&macro_path, source_identity.as_bytes())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let manifest = ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.clone(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        document: DocumentMetadata {
            document_name: "Ecky IR v0".to_string(),
            document_label: "Ecky IR v0".to_string(),
            source_path: Some(macro_path.to_string_lossy().to_string()),
            object_count: part_bindings.len(),
            warnings: Vec::new(),
        },
        parts: part_bindings,
        parameter_groups: vec![ParameterGroup {
            group_id: "core".to_string(),
            label: "Core".to_string(),
            parameter_keys: model
                .params
                .iter()
                .map(|param| param.field.key().to_string())
                .collect(),
            part_ids: model
                .parts
                .iter()
                .map(|part| part.part_id.clone())
                .collect(),
            editable: true,
            presentation: Some("primary".to_string()),
            order: Some(0),
        }],
        control_primitives: Vec::new(),
        control_relations: Vec::new(),
        control_views: Vec::new(),
        advisories: Vec::new(),
        selection_targets: Vec::new(),
        measurement_annotations: Vec::new(),
        warnings: Vec::new(),
        enrichment_state: crate::models::ManifestEnrichmentState {
            status: crate::models::EnrichmentStatus::None,
            proposals: Vec::new(),
        },
    };

    let manifest_path = dir.join(MANIFEST_FILE_NAME);
    write_manifest(&manifest_path, &manifest)?;

    let bundle = ArtifactBundle {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id,
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash: hash,
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        macro_path: Some(macro_path.to_string_lossy().to_string()),
        preview_stl_path: preview_path.to_string_lossy().to_string(),
        viewer_assets,
        edge_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: Vec::new(),
    };
    write_bundle(&dir.join(BUNDLE_FILE_NAME), &bundle)?;
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::ecky_ir::model::parse_model;

    fn render_root() -> PathBuf {
        std::env::temp_dir().join(format!("ecky-ir-runtime-test-{}", uuid::Uuid::new_v4()))
    }

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl crate::models::PathResolver for TestResolver {
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

    #[test]
    fn render_model_from_model_renders_typed_build_expr() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = r#"(model
            (part body
              (build
                (shape base (box 20 20 20))
                (shape cut (translate 0 0 10 (cylinder 4 12 24)))
                (result (difference base cut)))))"#;
        let model = parse_model(source).expect("model");

        let bundle = render_model_from_model(&model, source, &DesignParams::new(), &resolver)
            .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }
}
