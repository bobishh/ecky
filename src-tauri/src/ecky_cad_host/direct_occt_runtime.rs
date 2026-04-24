#![allow(dead_code)]

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::direct_occt_executor::export_core_program_step_stl;
use super::direct_occt_sdk::{DirectOcctSdkLayout, NativeExportOutcome};
use crate::ecky_core_ir::CoreProgram;
use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignParams, DocumentMetadata, EngineKind,
    EnrichmentStatus, ExportArtifact, GeometryBackend, ManifestEnrichmentState, ModelManifest,
    ModelSourceKind, ParameterGroup, PartBinding, PathResolver, SourceLanguage,
    MODEL_RUNTIME_SCHEMA_VERSION,
};

const SOURCE_FILE_NAME: &str = "source.ecky";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
const STEP_FILE_NAME: &str = "model.step";

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

    match export_core_program_step_stl(program, layout, &bundle_dir)? {
        NativeExportOutcome::Exported {
            step_path,
            stl_path,
        } => {
            let part = program.parts.first().ok_or_else(|| {
                AppError::validation("Direct OCCT runtime requires one Core IR part.")
            })?;
            let parameter_keys = program
                .parameters
                .iter()
                .map(|parameter| parameter.key.clone())
                .collect::<Vec<_>>();
            let manifest = build_direct_occt_manifest(
                &model_id,
                &source_path,
                &part.key,
                &part.label,
                &parameter_keys,
            )?;
            let bundle = build_direct_occt_bundle(
                &model_id,
                &content_hash,
                &source_path,
                &stl_path,
                &step_path,
            )?;
            crate::model_runtime::write_runtime_bundle(app, &model_id, &bundle, &manifest)
        }
        NativeExportOutcome::Blocked { blockers } => Err(AppError::render(format!(
            "Direct OCCT runtime blocked: {}",
            if blockers.is_empty() {
                "unknown runtime blocker".to_string()
            } else {
                blockers.join("; ")
            }
        ))),
    }
}

pub(crate) fn build_direct_occt_manifest(
    model_id: &str,
    source_path: &Path,
    part_key: &str,
    part_label: &str,
    parameter_keys: &[String],
) -> AppResult<ModelManifest> {
    let part_id = if part_key.trim().is_empty() {
        "body"
    } else {
        part_key
    };
    let label = if part_label.trim().is_empty() {
        part_id
    } else {
        part_label
    };

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
            object_count: 1,
            warnings: Vec::new(),
        },
        parts: vec![PartBinding {
            part_id: part_id.to_string(),
            freecad_object_name: part_id.to_string(),
            label: label.to_string(),
            kind: "solid".to_string(),
            semantic_role: Some("generated".to_string()),
            viewer_asset_path: Some(PREVIEW_STL_FILE_NAME.to_string()),
            viewer_node_ids: vec![part_id.to_string()],
            parameter_keys: parameter_keys.to_vec(),
            editable: true,
            bounds: None,
            volume: None,
            area: None,
        }],
        parameter_groups: vec![ParameterGroup {
            group_id: "core".to_string(),
            label: "Core".to_string(),
            parameter_keys: parameter_keys.to_vec(),
            part_ids: vec![part_id.to_string()],
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
        enrichment_state: ManifestEnrichmentState {
            status: EnrichmentStatus::None,
            proposals: Vec::new(),
        },
    })
}

pub(crate) fn build_direct_occt_bundle(
    model_id: &str,
    content_hash: &str,
    source_path: &Path,
    preview_stl_path: &Path,
    step_path: &Path,
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
        edge_targets: Vec::new(),
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
    use crate::models::{PathResolver, ViewerAssetFormat};
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
            "body",
            "Body",
            &Vec::<String>::new(),
        )
        .expect("manifest");
        let bundle =
            build_direct_occt_bundle(&model_id, &hash, &source_path, &preview_path, &step_path)
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
        assert!(!bundle_dir.join("bundle.json").exists());

        let _ = fs::remove_dir_all(root);
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

        let _ = fs::remove_dir_all(root);
    }
}
