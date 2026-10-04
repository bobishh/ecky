use ecky_cad_lib::contracts::{
    AppErrorCode, Config, DesignParams, GeometryBackend, MessageStatus, ParamValue, SourceLanguage,
    UiSpec,
};
use ecky_cad_lib::models::{AppState, PathResolver};
use ecky_cad_lib::services::design::{add_manual_version, AddManualVersionRequest};
use ecky_cad_lib::services::manual_parameters::{
    apply_manual_parameters, ManualParameterApplyRequest,
};
use std::path::PathBuf;
use uuid::Uuid;

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

fn test_config() -> Config {
    Config {
        engines: Vec::new(),
        selected_engine_id: String::new(),
        freecad_cmd: String::new(),
        cad_text_font_path: String::new(),
        freecad_library_roots: Vec::new(),
        assets: Vec::new(),
        microwave: None,
        voice: Default::default(),
        mcp: Default::default(),
        fem_compute: Default::default(),
        has_seen_onboarding: true,
        connection_type: None,
        provider_models: Default::default(),
        jev_classifier: Default::default(),
        default_engine_kind: ecky_cad_lib::contracts::EngineKind::EckyIrV0,
        default_source_language: SourceLanguage::EckyIrV0,
        default_geometry_backend: GeometryBackend::EckyRust,
        max_generation_attempts: 3,
        max_verify_attempts: 0,
        projects_root: None,
    }
}

fn source() -> &'static str {
    "(model (params (number size 10 :min 1 :max 100)) (part body (box size size size)))"
}

async fn fixture() -> (PathBuf, TestResolver, AppState, String) {
    let root = std::env::temp_dir().join(format!("ecky-manual-parameters-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("temp root");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    let resolver = TestResolver { root: root.clone() };
    let base_message_id = add_manual_version(
        AddManualVersionRequest {
            thread_id: "thread-1".to_string(),
            title: "Parametric cube".to_string(),
            version_name: "V1".to_string(),
            macro_code: source().to_string(),
            source_language: Some(SourceLanguage::EckyIrV0),
            geometry_backend: Some(GeometryBackend::EckyRust),
            parameters: DesignParams::new(),
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
        &resolver,
    )
    .await
    .expect("base version");
    (root, resolver, state, base_message_id)
}

#[tokio::test]
async fn persisted_parameter_apply_renders_manifest_and_appends_success_version_atomically() {
    let (root, resolver, state, base_message_id) = fixture().await;
    let parameters = DesignParams::from([("size".to_string(), ParamValue::Number(24.0))]);

    let response = apply_manual_parameters(
        ManualParameterApplyRequest {
            thread_id: "thread-1".to_string(),
            target_message_id: base_message_id.clone(),
            parameters: parameters.clone(),
            persist: true,
            title: None,
            version_name: Some("V2".to_string()),
        },
        &state,
        &resolver,
    )
    .await
    .expect("controller response");

    assert_eq!(response.status, MessageStatus::Success);
    assert_eq!(response.base_message_id, base_message_id);
    assert_ne!(
        response.message_id.as_deref(),
        Some(base_message_id.as_str())
    );
    assert_eq!(response.design_output.initial_params, parameters);
    assert!(response.error.is_none());
    let bundle = response.artifact_bundle.expect("rendered artifact");
    let manifest = response.model_manifest.expect("rendered manifest");
    assert_eq!(bundle.model_id, manifest.model_id);

    let conn = state.db.lock().await;
    let head = ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
        .expect("latest query")
        .expect("latest version");
    assert_eq!(Some(head.id.as_str()), response.message_id.as_deref());
    assert_eq!(head.status, MessageStatus::Success);
    assert_eq!(head.output.expect("design").initial_params, parameters);
    assert_eq!(
        head.artifact_bundle.expect("persisted artifact").model_id,
        bundle.model_id
    );
    assert_eq!(
        head.model_manifest.expect("persisted manifest").model_id,
        manifest.model_id
    );
    drop(conn);
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn failed_persisted_parameter_apply_appends_error_version_with_raw_backend_error() {
    let (root, resolver, state, base_message_id) = fixture().await;
    let invalid = DesignParams::from([(
        "size".to_string(),
        ParamValue::String("not-a-number".to_string()),
    )]);

    let response = apply_manual_parameters(
        ManualParameterApplyRequest {
            thread_id: "thread-1".to_string(),
            target_message_id: base_message_id,
            parameters: invalid.clone(),
            persist: true,
            title: None,
            version_name: Some("Broken params".to_string()),
        },
        &state,
        &resolver,
    )
    .await
    .expect("domain failure is returned with durable version identity");

    assert_eq!(response.status, MessageStatus::Error);
    assert!(response.artifact_bundle.is_none());
    assert!(response.model_manifest.is_none());
    let error = response.error.expect("raw backend error");
    assert_eq!(error.code, AppErrorCode::Validation);
    assert!(error.message.contains("size"), "{}", error.message);

    let conn = state.db.lock().await;
    let head = ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
        .expect("latest query")
        .expect("failed version");
    assert_eq!(Some(head.id.as_str()), response.message_id.as_deref());
    assert_eq!(head.status, MessageStatus::Error);
    assert_eq!(head.content, error.to_string());
    assert_eq!(head.output.expect("failed design").initial_params, invalid);
    drop(conn);
    std::fs::remove_dir_all(root).expect("cleanup");
}
