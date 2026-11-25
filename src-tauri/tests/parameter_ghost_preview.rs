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
    let draft = fs::read(&draft_path).unwrap();
    let draft_dir = Path::new(&draft_path).parent().unwrap();
    assert_eq!(fs::read_dir(draft_dir).unwrap().count(), 1);
    assert!(triangle_count(&draft) > 0);
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
    let production = applied.artifact_bundle.unwrap();
    assert!(Path::new(&production.model_stl_path).is_file());
    assert!(!production.export_artifacts.is_empty());
    assert_ne!(production.model_stl_path, draft_path);
    assert!(triangle_count(&fs::read(production.model_stl_path).unwrap()) > triangle_count(&draft));
    state.component_index_worker.wait_idle().await;
}
