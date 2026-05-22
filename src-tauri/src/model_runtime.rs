use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{
    validate_artifact_bundle, validate_model_manifest, validate_model_runtime_bundle, AppError,
    AppResult, ArtifactBundle, FeatureGraph, FeatureNode, FeatureOutputRef, ModelManifest,
    ModelSourceKind, PathResolver, SelectionTarget, ViewerAsset, ViewerAssetFormat,
};

const MODEL_RUNTIME_ROOT: &str = "model-runtime";
const GENERATED_ARTIFACT_DIR: &str = "generated";
const IMPORTED_FCSTD_ARTIFACT_DIR: &str = "imported-fcstd";
const IMPORTED_STEP_ARTIFACT_DIR: &str = "imported-step";
const IMPORTED_MESH_ARTIFACT_DIR: &str = "imported-mesh";
const BUNDLE_FILE_NAME: &str = "bundle.json";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const FCSTD_FILE_NAME: &str = "model.FCStd";
const PREVIEW_STL_FILE_NAME: &str = "preview.stl";

pub fn runtime_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    let root = app.app_data_dir().join(MODEL_RUNTIME_ROOT);
    fs::create_dir_all(&root).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(root)
}

pub fn runtime_bundle_dir(app: &dyn PathResolver, model_id: &str) -> AppResult<PathBuf> {
    artifact_dir(app, source_kind_from_model_id(model_id)?, model_id)
}

pub fn read_artifact_bundle(app: &dyn PathResolver, model_id: &str) -> AppResult<ArtifactBundle> {
    let bundle_dir = runtime_bundle_dir(app, model_id)?;
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    let bundle = read_bundle_file(&bundle_path)?;
    if bundle.model_id != model_id {
        return Err(AppError::validation(format!(
            "Artifact bundle modelId '{}' does not match requested model id '{}'.",
            bundle.model_id, model_id
        )));
    }
    if let Some(manifest) = read_manifest_if_exists(&bundle_dir, &bundle)? {
        bundle_from_manifest(&bundle_dir, bundle, &manifest)
    } else {
        validate_artifact_bundle(&bundle)?;
        Ok(bundle)
    }
}

pub fn write_artifact_bundle(
    app: &dyn PathResolver,
    model_id: &str,
    bundle: &ArtifactBundle,
) -> AppResult<ArtifactBundle> {
    if bundle.model_id != model_id {
        return Err(AppError::validation(format!(
            "Artifact bundle modelId '{}' does not match requested model id '{}'.",
            bundle.model_id, model_id
        )));
    }
    validate_model_id_source_kind(model_id, bundle.source_kind.clone())?;
    validate_artifact_bundle(bundle)?;

    let bundle_dir = artifact_dir(app, bundle.source_kind.clone(), model_id)?;
    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;
    let stored = read_manifest_if_exists(&bundle_dir, bundle)?
        .map(|manifest| bundle_from_manifest(&bundle_dir, bundle.clone(), &manifest))
        .transpose()?
        .unwrap_or_else(|| bundle.clone());
    write_bundle_file(&bundle_dir, &stored)?;
    Ok(stored)
}

pub fn read_model_manifest(app: &dyn PathResolver, model_id: &str) -> AppResult<ModelManifest> {
    let bundle_dir = runtime_bundle_dir(app, model_id)?;
    let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
    let manifest = read_manifest_file(&manifest_path)?;
    if manifest.model_id != model_id {
        return Err(AppError::validation(format!(
            "Model manifest modelId '{}' does not match requested model id '{}'.",
            manifest.model_id, model_id
        )));
    }
    Ok(manifest)
}

pub fn write_model_manifest(
    app: &dyn PathResolver,
    model_id: &str,
    manifest: &ModelManifest,
) -> AppResult<ModelManifest> {
    if manifest.model_id != model_id {
        return Err(AppError::validation(format!(
            "Model manifest modelId '{}' does not match requested model id '{}'.",
            manifest.model_id, model_id
        )));
    }
    validate_model_id_source_kind(model_id, manifest.source_kind.clone())?;
    let mut stored_manifest = manifest.clone();
    backfill_feature_graph_from_parts(&mut stored_manifest);
    validate_model_manifest(&stored_manifest)?;

    let bundle_dir = artifact_dir(app, stored_manifest.source_kind.clone(), model_id)?;
    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;
    let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
    write_manifest_file(&manifest_path, &stored_manifest)?;
    refresh_stored_bundle_for_manifest(&bundle_dir, &stored_manifest)?;
    Ok(stored_manifest)
}

pub fn read_runtime_bundle(
    app: &dyn PathResolver,
    model_id: &str,
) -> AppResult<(ArtifactBundle, ModelManifest)> {
    let bundle_dir = runtime_bundle_dir(app, model_id)?;
    let bundle = read_bundle_file(&bundle_dir.join(BUNDLE_FILE_NAME))?;
    let manifest = read_manifest_file(&bundle_dir.join(MANIFEST_FILE_NAME))?;
    let bundle = bundle_from_manifest(&bundle_dir, bundle, &manifest)?;
    Ok((bundle, manifest))
}

pub fn write_runtime_bundle(
    app: &dyn PathResolver,
    model_id: &str,
    bundle: &ArtifactBundle,
    manifest: &ModelManifest,
) -> AppResult<(ArtifactBundle, ModelManifest)> {
    if bundle.model_id != model_id || manifest.model_id != model_id {
        return Err(AppError::validation(format!(
            "Runtime bundle model ids must match requested model id '{}'.",
            model_id
        )));
    }
    validate_model_id_source_kind(model_id, manifest.source_kind.clone())?;
    let mut stored_manifest = manifest.clone();
    backfill_feature_graph_from_parts(&mut stored_manifest);
    validate_model_manifest(&stored_manifest)?;
    validate_artifact_bundle(bundle)?;

    let bundle_dir = artifact_dir(app, stored_manifest.source_kind.clone(), model_id)?;
    fs::create_dir_all(&bundle_dir).map_err(|err| AppError::persistence(err.to_string()))?;
    let stored_bundle = bundle_from_manifest(&bundle_dir, bundle.clone(), &stored_manifest)?;
    write_manifest_file(&bundle_dir.join(MANIFEST_FILE_NAME), &stored_manifest)?;
    write_bundle_file(&bundle_dir, &stored_bundle)?;
    Ok((stored_bundle, stored_manifest))
}

pub fn refresh_artifact_bundle_from_manifest(
    app: &dyn PathResolver,
    model_id: &str,
) -> AppResult<ArtifactBundle> {
    let bundle_dir = runtime_bundle_dir(app, model_id)?;
    let bundle = read_bundle_file(&bundle_dir.join(BUNDLE_FILE_NAME))?;
    let manifest = read_manifest_file(&bundle_dir.join(MANIFEST_FILE_NAME))?;
    let refreshed = bundle_from_manifest(&bundle_dir, bundle, &manifest)?;
    write_bundle_file(&bundle_dir, &refreshed)?;
    Ok(refreshed)
}

fn artifact_dir(
    app: &dyn PathResolver,
    source_kind: ModelSourceKind,
    model_id: &str,
) -> AppResult<PathBuf> {
    Ok(runtime_root(app)?
        .join(source_kind_dir_name(source_kind))
        .join(model_id))
}

fn source_kind_from_model_id(model_id: &str) -> AppResult<ModelSourceKind> {
    if model_id.starts_with("generated-") {
        Ok(ModelSourceKind::Generated)
    } else if model_id.starts_with("imported-fcstd-") {
        Ok(ModelSourceKind::ImportedFcstd)
    } else if model_id.starts_with("imported-step-") {
        Ok(ModelSourceKind::ImportedStep)
    } else if model_id.starts_with("imported-mesh-") {
        Ok(ModelSourceKind::ImportedMesh)
    } else {
        Err(AppError::not_found(format!(
            "Unknown model id '{}'.",
            model_id
        )))
    }
}

fn validate_model_id_source_kind(model_id: &str, source_kind: ModelSourceKind) -> AppResult<()> {
    let expected = source_kind_from_model_id(model_id)?;
    if expected != source_kind {
        return Err(AppError::validation(format!(
            "Model id '{}' does not match sourceKind {:?}.",
            model_id, source_kind
        )));
    }
    Ok(())
}

fn source_kind_dir_name(source_kind: ModelSourceKind) -> &'static str {
    match source_kind {
        ModelSourceKind::Generated => GENERATED_ARTIFACT_DIR,
        ModelSourceKind::ImportedFcstd => IMPORTED_FCSTD_ARTIFACT_DIR,
        ModelSourceKind::ImportedStep => IMPORTED_STEP_ARTIFACT_DIR,
        ModelSourceKind::ImportedMesh => IMPORTED_MESH_ARTIFACT_DIR,
    }
}

fn read_bundle_file(path: &Path) -> AppResult<ArtifactBundle> {
    let raw = fs::read_to_string(path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read artifact bundle '{}': {}",
            path.display(),
            err
        ))
    })?;
    let bundle: ArtifactBundle = serde_json::from_str(&raw)
        .map_err(|err| AppError::parse(format!("Failed to parse artifact bundle: {}", err)))?;
    validate_artifact_bundle(&bundle)?;
    Ok(bundle)
}

fn write_bundle_file(bundle_dir: &Path, bundle: &ArtifactBundle) -> AppResult<()> {
    let path = bundle_dir.join(BUNDLE_FILE_NAME);
    let data = serde_json::to_string_pretty(bundle)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(&path, data).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write artifact bundle '{}': {}",
            path.display(),
            err
        ))
    })
}

fn read_manifest_file(path: &Path) -> AppResult<ModelManifest> {
    let raw = fs::read_to_string(path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read model manifest '{}': {}",
            path.display(),
            err
        ))
    })?;
    let mut manifest: ModelManifest = serde_json::from_str(&raw)
        .map_err(|err| AppError::parse(format!("Failed to parse model manifest: {}", err)))?;
    backfill_feature_graph_from_parts(&mut manifest);
    validate_model_manifest(&manifest)?;
    Ok(manifest)
}

fn write_manifest_file(path: &Path, manifest: &ModelManifest) -> AppResult<()> {
    let data = serde_json::to_string_pretty(manifest)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write model manifest '{}': {}",
            path.display(),
            err
        ))
    })
}

fn read_manifest_if_exists(
    bundle_dir: &Path,
    bundle: &ArtifactBundle,
) -> AppResult<Option<ModelManifest>> {
    let manifest_path = canonical_manifest_path(bundle_dir, bundle);
    if !manifest_path.exists() {
        return Ok(None);
    }
    read_manifest_file(&manifest_path).map(Some)
}

fn refresh_stored_bundle_for_manifest(
    bundle_dir: &Path,
    manifest: &ModelManifest,
) -> AppResult<()> {
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    if !bundle_path.exists() {
        return Ok(());
    }
    let bundle = read_bundle_file(&bundle_path)?;
    let refreshed = bundle_from_manifest(bundle_dir, bundle, manifest)?;
    write_bundle_file(bundle_dir, &refreshed)
}

fn bundle_from_manifest(
    bundle_dir: &Path,
    mut bundle: ArtifactBundle,
    manifest: &ModelManifest,
) -> AppResult<ArtifactBundle> {
    if bundle.model_id != manifest.model_id || bundle.source_kind != manifest.source_kind {
        return Err(AppError::validation(
            "Artifact bundle does not match the model manifest.",
        ));
    }

    bundle.schema_version = manifest.schema_version;
    bundle.engine_kind = manifest.engine_kind;
    bundle.source_language = manifest.source_language;
    bundle.geometry_backend = manifest.geometry_backend;
    bundle.manifest_path = path_to_string(&canonical_manifest_path(bundle_dir, &bundle))?;
    bundle.preview_stl_path = path_to_string(&canonical_preview_path(bundle_dir, &bundle))?;
    if !bundle.fcstd_path.trim().is_empty()
        || matches!(
            bundle.source_kind,
            ModelSourceKind::ImportedFcstd | ModelSourceKind::ImportedStep
        )
    {
        bundle.fcstd_path = path_to_string(&canonical_fcstd_path(bundle_dir, &bundle))?;
    }
    bundle.viewer_assets = viewer_assets_from_manifest(bundle_dir, manifest)?;
    validate_model_runtime_bundle(manifest, &bundle)?;
    Ok(bundle)
}

fn canonical_fcstd_path(bundle_dir: &Path, bundle: &ArtifactBundle) -> PathBuf {
    let canonical = bundle_dir.join(FCSTD_FILE_NAME);
    if canonical.exists() {
        canonical
    } else {
        normalize_bundle_relative_path(bundle_dir, Path::new(&bundle.fcstd_path))
    }
}

fn canonical_manifest_path(bundle_dir: &Path, bundle: &ArtifactBundle) -> PathBuf {
    let canonical = bundle_dir.join(MANIFEST_FILE_NAME);
    if canonical.exists() {
        canonical
    } else {
        normalize_bundle_relative_path(bundle_dir, Path::new(&bundle.manifest_path))
    }
}

fn canonical_preview_path(bundle_dir: &Path, bundle: &ArtifactBundle) -> PathBuf {
    let canonical = bundle_dir.join(PREVIEW_STL_FILE_NAME);
    if canonical.exists() {
        canonical
    } else {
        normalize_bundle_relative_path(bundle_dir, Path::new(&bundle.preview_stl_path))
    }
}

fn normalize_bundle_relative_path(bundle_dir: &Path, path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() || path.is_absolute() {
        path.to_path_buf()
    } else {
        bundle_dir.join(path)
    }
}

fn viewer_assets_from_manifest(
    bundle_dir: &Path,
    manifest: &ModelManifest,
) -> AppResult<Vec<ViewerAsset>> {
    let mut assets = Vec::new();
    for part in &manifest.parts {
        let Some(path) = part.viewer_asset_path.as_ref() else {
            continue;
        };
        let normalized_path =
            path_to_string(&normalize_bundle_relative_path(bundle_dir, Path::new(path)))?;
        assets.extend(part.viewer_node_ids.iter().map(|node_id| ViewerAsset {
            part_id: part.part_id.clone(),
            node_id: node_id.clone(),
            object_name: part.freecad_object_name.clone(),
            label: part.label.clone(),
            path: normalized_path.clone(),
            format: ViewerAssetFormat::Stl,
        }));
    }
    Ok(assets)
}

fn backfill_feature_graph_from_parts(manifest: &mut ModelManifest) {
    if manifest.feature_graph.is_some() {
        return;
    }

    let nodes = manifest
        .parts
        .iter()
        .map(|part| {
            let feature_id = format!("part:{}", part.part_id);
            let target_ids = manifest
                .selection_targets
                .iter()
                .filter(|target| target.part_id == part.part_id)
                .filter_map(selection_target_output_id)
                .map(str::to_string)
                .collect::<Vec<_>>();
            let output_refs = if target_ids.is_empty() {
                Vec::new()
            } else {
                vec![FeatureOutputRef {
                    feature_id: feature_id.clone(),
                    output_id: "selectionTargets".to_string(),
                    target_ids,
                }]
            };

            FeatureNode {
                feature_id,
                kind: "part".to_string(),
                label: if part.label.trim().is_empty() {
                    part.part_id.clone()
                } else {
                    part.label.clone()
                },
                source_ref: None,
                dependency_ids: Vec::new(),
                output_refs,
                ports: Vec::new(),
            }
        })
        .collect();

    manifest.feature_graph = Some(FeatureGraph { nodes });
}

fn selection_target_output_id(target: &SelectionTarget) -> Option<&str> {
    target
        .target_id
        .as_deref()
        .or(target.durable_target_id.as_deref())
        .or(target.canonical_target_id.as_deref())
        .or_else(|| target.alias_ids.first().map(String::as_str))
}

fn path_to_string(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| AppError::internal("Non-UTF-8 path encountered."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        DocumentMetadata, EngineKind, EnrichmentStatus, GeometryBackend, ManifestEnrichmentState,
        PartBinding, SelectionTarget, SelectionTargetKind, SourceLanguage,
        MODEL_RUNTIME_SCHEMA_VERSION,
    };

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

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ecky-model-runtime-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ))
    }

    fn manifest(model_id: &str, source_kind: ModelSourceKind) -> ModelManifest {
        ModelManifest {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind,
            source_digest: None,
            core_digest: None,
            ast_schema_version: None,
            engine_kind: EngineKind::Build123d,
            source_language: SourceLanguage::Build123d,
            geometry_backend: GeometryBackend::Build123d,
            document: DocumentMetadata {
                document_name: "Doc".to_string(),
                document_label: "Doc".to_string(),
                source_path: None,
                object_count: 1,
                warnings: Vec::new(),
            },
            parts: vec![PartBinding {
                part_id: "body".to_string(),
                freecad_object_name: "Body".to_string(),
                label: "Body".to_string(),
                kind: "solid".to_string(),
                semantic_role: None,
                viewer_asset_path: Some("parts/body.stl".to_string()),
                viewer_node_ids: vec!["node-body".to_string()],
                parameter_keys: Vec::new(),
                editable: false,
                bounds: None,
                volume: None,
                area: None,
            }],
            parameter_groups: Vec::new(),
            control_primitives: Vec::new(),
            control_relations: Vec::new(),
            control_views: Vec::new(),
            preview_views: Vec::new(),
            advisories: Vec::new(),
            selection_targets: Vec::new(),
            measurement_annotations: Vec::new(),
            tagged_anchors: std::collections::BTreeMap::new(),
            feature_graph: None,
            correspondence_graph: None,
            warnings: Vec::new(),
            enrichment_state: ManifestEnrichmentState {
                status: EnrichmentStatus::None,
                proposals: Vec::new(),
            },
        }
    }

    fn bundle(model_id: &str, source_kind: ModelSourceKind) -> ArtifactBundle {
        ArtifactBundle {
            schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: model_id.to_string(),
            source_kind,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: "manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: "preview.stl".to_string(),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        }
    }

    #[test]
    fn write_manifest_refreshes_non_freecad_bundle_assets() {
        let root = test_root("refresh");
        let resolver = TestResolver { root: root.clone() };
        let model_id = "generated-b123d-test";
        let dir = runtime_bundle_dir(&resolver, model_id).expect("dir");
        fs::create_dir_all(dir.join("parts")).expect("parts");
        fs::write(dir.join("preview.stl"), b"solid preview").expect("preview");
        fs::write(dir.join("parts/body.stl"), b"solid body").expect("part");

        let initial_bundle = bundle(model_id, ModelSourceKind::Generated);
        write_artifact_bundle(&resolver, model_id, &initial_bundle).expect("bundle");
        write_model_manifest(
            &resolver,
            model_id,
            &manifest(model_id, ModelSourceKind::Generated),
        )
        .expect("manifest");

        let stored = read_artifact_bundle(&resolver, model_id).expect("stored");
        assert_eq!(stored.geometry_backend, GeometryBackend::Build123d);
        assert!(stored.fcstd_path.is_empty());
        assert_eq!(stored.viewer_assets.len(), 1);
        assert_eq!(stored.viewer_assets[0].node_id, "node-body");
        assert_eq!(
            stored.viewer_assets[0].path,
            dir.join("parts/body.stl").to_string_lossy()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imported_fcstd_model_ids_use_imported_directory() {
        let root = test_root("imported");
        let resolver = TestResolver { root: root.clone() };
        let model_id = "imported-fcstd-test";
        let dir = runtime_bundle_dir(&resolver, model_id).expect("dir");

        assert!(dir.ends_with(Path::new(
            "model-runtime/imported-fcstd/imported-fcstd-test"
        )));
        assert!(!dir.ends_with(Path::new("model-runtime/generated/imported-fcstd-test")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imported_step_model_ids_use_imported_directory() {
        let root = test_root("imported-step");
        let resolver = TestResolver { root: root.clone() };
        let model_id = "imported-step-test";
        let dir = runtime_bundle_dir(&resolver, model_id).expect("dir");

        assert!(dir.ends_with(Path::new("model-runtime/imported-step/imported-step-test")));
        assert!(!dir.ends_with(Path::new("model-runtime/generated/imported-step-test")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn read_model_manifest_backfills_missing_feature_graph_from_parts_and_selection_targets() {
        let root = test_root("feature-graph");
        let resolver = TestResolver { root: root.clone() };
        let model_id = "generated-feature-graph-test";
        let dir = runtime_bundle_dir(&resolver, model_id).expect("dir");
        fs::create_dir_all(&dir).expect("dir");

        let mut manifest = manifest(model_id, ModelSourceKind::Generated);
        manifest.parts.push(PartBinding {
            part_id: "lid".to_string(),
            freecad_object_name: "Lid".to_string(),
            label: "Lid".to_string(),
            kind: "solid".to_string(),
            semantic_role: None,
            viewer_asset_path: Some("parts/lid.stl".to_string()),
            viewer_node_ids: vec!["node-lid".to_string()],
            parameter_keys: Vec::new(),
            editable: false,
            bounds: None,
            volume: None,
            area: None,
        });
        manifest.selection_targets = vec![
            SelectionTarget {
                target_id: Some("target-body".to_string()),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "node-body".to_string(),
                label: "Body".to_string(),
                kind: SelectionTargetKind::Object,
                editable: false,
                parameter_keys: Vec::new(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            },
            SelectionTarget {
                target_id: Some("target-lid".to_string()),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "lid".to_string(),
                viewer_node_id: "node-lid".to_string(),
                label: "Lid".to_string(),
                kind: SelectionTargetKind::Object,
                editable: false,
                parameter_keys: Vec::new(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            },
        ];

        fs::write(
            dir.join(MANIFEST_FILE_NAME),
            serde_json::to_string_pretty(&manifest).expect("manifest json"),
        )
        .expect("write manifest");

        let read_manifest = read_model_manifest(&resolver, model_id).expect("manifest");
        let feature_graph = read_manifest.feature_graph.expect("feature graph");

        assert_eq!(feature_graph.nodes.len(), 2);
        assert_eq!(feature_graph.nodes[0].feature_id, "part:body");
        assert_eq!(feature_graph.nodes[0].kind, "part");
        assert_eq!(feature_graph.nodes[0].label, "Body");
        assert_eq!(
            feature_graph.nodes[0].output_refs[0].target_ids,
            vec!["target-body"]
        );
        assert_eq!(feature_graph.nodes[1].feature_id, "part:lid");
        assert_eq!(
            feature_graph.nodes[1].output_refs[0].target_ids,
            vec!["target-lid"]
        );

        let _ = fs::remove_dir_all(root);
    }
}
