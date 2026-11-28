use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ecky_cad_lib::contracts::{
    Config, DesignParams, GeometryBackend, MessageStatus, ParamValue, SourceLanguage, UiSpec,
};
use ecky_cad_lib::models::{AppState, PathResolver};
use ecky_cad_lib::services::design::{add_manual_version, AddManualVersionRequest};
use ecky_cad_lib::services::manual_parameters::{
    apply_manual_parameters, ManualParameterApplyRequest,
};
use ecky_cad_lib::services::parameter_ghost::{
    preview_parameter_ghost, ParameterGhostPreviewInput,
};

struct ProjectPaths(PathBuf);

impl Drop for ProjectPaths {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl PathResolver for ProjectPaths {
    fn app_config_dir(&self) -> PathBuf {
        self.0.join("config")
    }

    fn app_data_dir(&self) -> PathBuf {
        self.0.join("data")
    }

    fn resource_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }
}

fn radius(value: f64) -> DesignParams {
    DesignParams::from([("radius".to_string(), ParamValue::Number(value))])
}

fn triangle_count(mesh: &[u8]) -> u32 {
    u32::from_le_bytes(mesh[80..84].try_into().unwrap())
}

#[tokio::test]
async fn draft_renders_only_affected_geometry_at_the_saved_placement_and_apply_exports_full_mesh() {
    let directory =
        std::env::temp_dir().join(format!("ecky-parameter-ghost-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let app = Arc::new(ProjectPaths(directory.clone()));
    let config: Config = serde_json::from_value(serde_json::json!({
        "engines": [],
        "selectedEngineId": "",
        "defaultEngineKind": "ecky",
        "defaultSourceLanguage": "ecky",
        "defaultGeometryBackend": "mesh"
    }))
    .unwrap();
    let conn = ecky_cad_lib::db::init_db(&directory.join("history.sqlite")).unwrap();
    let state = AppState::new(config, None, conn);
    let source = r#"(define-component bulb ((number size 5)) (sphere size))
(model
  (params (number radius 5 :min 1 :max 20))
  (part left (translate 40 0 0 (bulb :size radius)))
  (part right (translate 80 0 0 (bulb :size radius)))
  (part stand (translate -40 0 0 (box 2 2 2))))"#;
    let base = add_manual_version(
        AddManualVersionRequest {
            thread_id: "project".into(),
            parent_message_id: None,
            title: "Assembly".into(),
            version_name: "Base".into(),
            macro_code: source.into(),
            source_language: Some(SourceLanguage::EckyIrV0),
            geometry_backend: Some(GeometryBackend::EckyRust),
            parameters: radius(5.0),
            ui_spec: UiSpec::default(),
            post_processing: None,
            artifact_bundle: None,
            model_manifest: None,
            response_text: None,
            agent_origin: None,
            status: None,
            error_message: None,
        },
        &state,
        app.as_ref(),
    )
    .await
    .unwrap();
    let apply_request = |target_message_id, parameters| ManualParameterApplyRequest {
        thread_id: "project".into(),
        target_message_id,
        parameters,
        persist: true,
        title: None,
        version_name: None,
    };
    let saved = apply_manual_parameters(apply_request(base, radius(5.0)), &state, app.as_ref())
        .await
        .unwrap();
    assert_eq!(saved.status, MessageStatus::Success, "{:?}", saved.error);
    let saved_id = saved.message_id.unwrap();
    let saved_bundle = saved.artifact_bundle.unwrap();
    let saved_mesh = fs::read(&saved_bundle.model_stl_path).unwrap();

    let preview = preview_parameter_ghost(
        ParameterGhostPreviewInput {
            thread_id: "project".into(),
            target_message_id: saved_id.clone(),
            parameters: radius(7.0),
            request_id: "draft-radius".into(),
        },
        &state,
        app.clone(),
    )
    .await
    .unwrap();
    let draft_path = preview.model_stl_path.unwrap();
    let draft_part_ids = preview
        .part_meshes
        .iter()
        .map(|entry| entry.part_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(draft_part_ids, vec!["left", "right"]);
    for part_mesh in &preview.part_meshes {
        assert!(Path::new(&part_mesh.stl_path).is_file());
    }
    let draft = fs::read(&draft_path).unwrap();
    let draft_dir = Path::new(&draft_path).parent().unwrap();
    assert!(!draft_dir.join("plan.json").exists());
    assert!(triangle_count(&draft) > 0);
    // Repeated preview against the same version hits the compiled/baseline cache cleanly.
    let preview_cached = preview_parameter_ghost(
        ParameterGhostPreviewInput {
            thread_id: "project".into(),
            target_message_id: saved_id.clone(),
            parameters: radius(8.0),
            request_id: "draft-radius-cached".into(),
        },
        &state,
        app.clone(),
    )
    .await
    .unwrap();
    assert!(preview_cached.model_stl_path.is_some());
    let (triangles, tail) = draft[84..].as_chunks::<50>();
    assert!(tail.is_empty());
    assert_eq!(triangles.len(), triangle_count(&draft) as usize);
    let x_coordinates = triangles
        .iter()
        .flat_map(|triangle| {
            [12, 24, 36]
                .map(|offset| f32::from_le_bytes(triangle[offset..offset + 4].try_into().unwrap()))
        })
        .collect::<Vec<_>>();
    assert!(x_coordinates.iter().all(|x| *x >= 32.99));
    assert!(x_coordinates.iter().any(|x| *x > 85.0));
    assert!(x_coordinates.iter().any(|x| *x < 35.0));
    assert_eq!(fs::read(&saved_bundle.model_stl_path).unwrap(), saved_mesh);
    {
        let conn = state.db.lock().await;
        let head = ecky_cad_lib::db::get_thread_latest_version(&conn, "project")
            .unwrap()
            .unwrap();
        assert_eq!(head.id, saved_id);
        let binding = ecky_cad_lib::thread_source_binding::get_binding(&conn, "project")
            .unwrap()
            .unwrap();
        assert_eq!(
            fs::read_to_string(binding.source_path).unwrap().trim(),
            source
        );
    }

    let applied =
        apply_manual_parameters(apply_request(saved_id, radius(7.0)), &state, app.as_ref())
            .await
            .unwrap();
    assert_eq!(
        applied.status,
        MessageStatus::Success,
        "{:?}",
        applied.error
    );
    let applied_id = applied.message_id.clone().expect("applied message id");
    let display_bundle = applied.artifact_bundle.unwrap();
    let display_manifest = applied.model_manifest.unwrap();
    assert!(display_bundle.production_pending);
    assert!(Path::new(&display_bundle.model_stl_path).is_file());
    assert!(display_bundle.export_artifacts.is_empty());
    assert_ne!(display_bundle.model_stl_path, draft_path);
    let display_triangles = triangle_count(&fs::read(&display_bundle.model_stl_path).unwrap());
    assert!(display_triangles > triangle_count(&draft));

    // Display-tier Apply preserves distinct per-part viewer assets and per-part bounds
    // for Solo isolation and viewport picking before first export.
    assert_eq!(display_bundle.viewer_assets.len(), 3);
    let left_part = display_manifest
        .parts
        .iter()
        .find(|part| part.part_id == "left")
        .expect("left part");
    let right_part = display_manifest
        .parts
        .iter()
        .find(|part| part.part_id == "right")
        .expect("right part");
    let stand_part = display_manifest
        .parts
        .iter()
        .find(|part| part.part_id == "stand")
        .expect("stand part");
    assert_eq!(
        left_part.viewer_asset_path.as_deref(),
        Some("parts/left.stl")
    );
    assert_eq!(
        right_part.viewer_asset_path.as_deref(),
        Some("parts/right.stl")
    );
    assert_eq!(
        stand_part.viewer_asset_path.as_deref(),
        Some("parts/stand.stl")
    );
    let left_bounds = left_part.bounds.as_ref().expect("left bounds");
    let right_bounds = right_part.bounds.as_ref().expect("right bounds");
    let stand_bounds = stand_part.bounds.as_ref().expect("stand bounds");
    assert!(left_bounds.x_min > 30.0 && left_bounds.x_max < 50.0);
    assert!(right_bounds.x_min > 70.0 && right_bounds.x_max < 90.0);
    assert!(stand_bounds.x_min < -35.0 && stand_bounds.x_max < -30.0);

    // Rust-owned lazy materialization upgrades a pending display model by model_id (used by FEM).
    let ensured_model_id = ecky_cad_lib::services::render::ensure_production_bundle_for_model(
        &display_bundle.model_id,
        &state,
        app.as_ref(),
    )
    .await
    .unwrap();
    assert_ne!(ensured_model_id, display_bundle.model_id);
    let ensured_bundle =
        ecky_cad_lib::model_runtime::read_artifact_bundle(app.as_ref(), &ensured_model_id).unwrap();
    assert!(!ensured_bundle.production_pending);

    let materialized = ecky_cad_lib::services::render::materialize_production_bundle(
        "project".into(),
        applied_id.clone(),
        &state,
        app.as_ref(),
    )
    .await
    .unwrap();
    let production = materialized.artifact_bundle;
    assert!(!production.production_pending);
    assert!(Path::new(&production.model_stl_path).is_file());
    let step_artifact = production
        .export_artifacts
        .iter()
        .find(|artifact| artifact.format == "step")
        .expect("materialized STEP export artifact");
    assert!(Path::new(&step_artifact.path).is_file());
    assert!(fs::metadata(&step_artifact.path).unwrap().len() > 0);
    assert!(triangle_count(&fs::read(&production.model_stl_path).unwrap()) > display_triangles);
    assert!(
        !materialized.model_manifest.assembly_tree.is_empty(),
        "manifest must expose assembly tree"
    );
    state.component_index_worker.wait_idle().await;
}

fn write_ascii_box_stl(path: &Path, size: f64) {
    let s = size;
    let triangles = [
        ([0.0, 0.0, 0.0], [s, s, 0.0], [s, 0.0, 0.0]),
        ([0.0, 0.0, 0.0], [0.0, s, 0.0], [s, s, 0.0]),
        ([0.0, 0.0, s], [s, 0.0, s], [s, s, s]),
        ([0.0, 0.0, s], [s, s, s], [0.0, s, s]),
        ([0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, 0.0, s]),
        ([0.0, 0.0, 0.0], [s, 0.0, s], [0.0, 0.0, s]),
        ([0.0, s, 0.0], [s, s, s], [s, s, 0.0]),
        ([0.0, s, 0.0], [0.0, s, s], [s, s, s]),
        ([0.0, 0.0, 0.0], [0.0, 0.0, s], [0.0, s, s]),
        ([0.0, 0.0, 0.0], [0.0, s, s], [0.0, s, 0.0]),
        ([s, 0.0, 0.0], [s, s, s], [s, 0.0, s]),
        ([s, 0.0, 0.0], [s, s, 0.0], [s, s, s]),
    ];
    let mut stl = String::from("solid box\n");
    for (a, b, c) in triangles {
        stl.push_str("facet normal 0 0 0\n  outer loop\n");
        for v in [a, b, c] {
            stl.push_str(&format!("    vertex {} {} {}\n", v[0], v[1], v[2]));
        }
        stl.push_str("  endloop\nendfacet\n");
    }
    stl.push_str("endsolid box\n");
    fs::write(path, stl).unwrap();
}

#[tokio::test]
async fn deferred_materialization_preserves_mesh_provenance_and_rejects_modified_external_stl() {
    let directory =
        std::env::temp_dir().join(format!("ecky-deferred-stl-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let stl_input = directory.join("input.stl");
    write_ascii_box_stl(&stl_input, 4.0);

    let app = Arc::new(ProjectPaths(directory.clone()));
    let config: Config = serde_json::from_value(serde_json::json!({
        "engines": [],
        "selectedEngineId": "",
        "defaultEngineKind": "ecky",
        "defaultSourceLanguage": "ecky",
        "defaultGeometryBackend": "mesh"
    }))
    .unwrap();
    let conn = ecky_cad_lib::db::init_db(&directory.join("history.sqlite")).unwrap();
    let state = AppState::new(config, None, conn);
    let source = format!(
        "(model\n  (params (number pad 2 :min 1 :max 10))\n  (part body (solidify (import-stl {:?})))\n  (part tab (translate 20 0 0 (box pad 2 2))))",
        stl_input.to_string_lossy()
    );
    let params = DesignParams::from([("pad".to_string(), ParamValue::Number(3.0))]);
    let base = add_manual_version(
        AddManualVersionRequest {
            thread_id: "stl-project".into(),
            parent_message_id: None,
            title: "StlHybrid".into(),
            version_name: "Base".into(),
            macro_code: source.clone(),
            source_language: Some(SourceLanguage::EckyIrV0),
            geometry_backend: Some(GeometryBackend::EckyRust),
            parameters: params.clone(),
            ui_spec: UiSpec::default(),
            post_processing: None,
            artifact_bundle: None,
            model_manifest: None,
            response_text: None,
            agent_origin: None,
            status: None,
            error_message: None,
        },
        &state,
        app.as_ref(),
    )
    .await
    .unwrap();
    let initial = apply_manual_parameters(
        ManualParameterApplyRequest {
            thread_id: "stl-project".into(),
            target_message_id: base,
            parameters: params.clone(),
            persist: true,
            title: None,
            version_name: None,
        },
        &state,
        app.as_ref(),
    )
    .await
    .unwrap();
    assert_eq!(
        initial.status,
        MessageStatus::Success,
        "{:?}",
        initial.error
    );
    let mut pending_bundle = initial.artifact_bundle.unwrap();
    let pending_manifest = initial.model_manifest.unwrap();
    let provenance = pending_bundle
        .geometry_provenance
        .as_ref()
        .expect("bundle provenance");
    assert!(
        !provenance.source_mesh_digests.is_empty(),
        "bundle must record external STL digest"
    );
    pending_bundle.production_pending = true;
    pending_bundle.export_artifacts.clear();

    let saved_id = add_manual_version(
        AddManualVersionRequest {
            thread_id: "stl-project".into(),
            parent_message_id: None,
            title: "StlHybrid".into(),
            version_name: "DisplayPending".into(),
            macro_code: source,
            source_language: Some(SourceLanguage::EckyIrV0),
            geometry_backend: Some(GeometryBackend::EckyRust),
            parameters: params,
            ui_spec: UiSpec::default(),
            post_processing: None,
            artifact_bundle: Some(pending_bundle),
            model_manifest: Some(pending_manifest),
            response_text: None,
            agent_origin: None,
            status: Some(MessageStatus::Success),
            error_message: None,
        },
        &state,
        app.as_ref(),
    )
    .await
    .unwrap();

    // Mutate external STL file on disk after display revision was committed.
    write_ascii_box_stl(&stl_input, 9.0);

    let err = ecky_cad_lib::services::render::materialize_production_bundle(
        "stl-project".into(),
        saved_id,
        &state,
        app.as_ref(),
    )
    .await
    .expect_err("mutated external STL must block silent deferred export divergence");
    assert!(
        err.message.to_ascii_lowercase().contains("external stl"),
        "unexpected error: {}",
        err.message
    );
    state.component_index_worker.wait_idle().await;
}
