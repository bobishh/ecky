use ecky_cad_lib::contracts::{
    Config, DesignParams, DisplacementSpec, GeometryBackend, MessageStatus, ParamValue,
    PostProcessingSpec, ProjectionType, SourceLanguage, UiField, UiSpec,
};
use ecky_cad_lib::models::{AppState, PathResolver};
use ecky_cad_lib::services::manual_code::{apply_manual_code, ManualCodeApplyRequest};
use ecky_cad_lib::services::thread_creation::{
    create_design_thread, CreateDesignThreadIntent, DesignThreadCreationMode,
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

fn fixture() -> (PathBuf, TestResolver, AppState) {
    let root = std::env::temp_dir().join(format!("ecky-thread-create-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("temp root");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    let resolver = TestResolver { root: root.clone() };
    (root, resolver, state)
}

#[tokio::test]
async fn macro_thread_creation_is_one_intent_with_rendered_initial_version() {
    let (root, resolver, state) = fixture();
    let source = "(model (part body (box 12 13 14)))";

    let response = create_design_thread(
        CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Macro,
            title: Some("Starter bracket".into()),
            source: Some(source.into()),
            base_thread_id: None,
            base_message_id: None,
        },
        &state,
        &resolver,
    )
    .await
    .expect("create macro thread");

    let initial = response
        .workspace
        .selected_version
        .as_ref()
        .expect("initial version");
    assert_eq!(initial.status, MessageStatus::Success);
    assert!(initial.artifact_bundle.is_some());
    assert!(initial.model_manifest.is_some());
    assert_eq!(
        response.initial_version_id.as_deref(),
        Some(initial.id.as_str())
    );
    assert_eq!(response.workspace.thread.id, response.thread_id);
    assert_eq!(
        response
            .workspace
            .selected_version
            .as_ref()
            .map(|message| message.id.as_str()),
        response.initial_version_id.as_deref(),
    );
    assert_eq!(
        std::fs::read_to_string(&response.source_document.file).expect("bound source"),
        source,
    );

    let json = serde_json::to_value(&response).expect("serialize response");
    assert!(json.get("threadId").is_some());
    assert!(json.get("sourceDocument").is_some());
    assert!(json.get("initialVersionId").is_some());
    assert!(json.get("thread_id").is_none());

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn blank_thread_creation_atomically_binds_empty_source_without_version() {
    let (root, resolver, state) = fixture();
    let previous = ecky_cad_lib::services::session::build_saved_version_snapshot(
        None,
        "previous-thread".to_string(),
        "previous-version".to_string(),
        None,
        None,
        None,
    );
    *state.last_snapshot.lock().unwrap() = Some(previous.clone());
    ecky_cad_lib::services::session::write_last_snapshot(&resolver, Some(&previous));

    let response = create_design_thread(
        CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Blank,
            title: Some("Blank fixture".into()),
            source: None,
            base_thread_id: None,
            base_message_id: None,
        },
        &state,
        &resolver,
    )
    .await
    .expect("create blank thread");

    assert!(response.initial_version_id.is_none());
    assert!(response.workspace.selected_version.is_none());
    assert!(response.workspace.messages_page.messages.is_empty());
    assert_eq!(response.source_document.source, "");
    assert!(state.last_snapshot.lock().unwrap().is_none());
    assert!(!ecky_cad_lib::services::session::last_snapshot_path(&resolver).exists());
    assert_eq!(
        std::fs::read_to_string(&response.source_document.file).expect("bound source"),
        "",
    );
    let conn = state.db.lock().await;
    let binding = ecky_cad_lib::thread_source_binding::get_binding(&conn, &response.thread_id)
        .expect("binding query")
        .expect("binding");
    assert_eq!(binding.source_path, response.source_document.file);
    drop(conn);

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn invalid_initial_macro_is_preserved_as_error_head_with_raw_detail() {
    let (root, resolver, state) = fixture();
    let invalid_source = "(model (part body (box 12 13";

    let response = create_design_thread(
        CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Macro,
            title: Some("Broken starter".into()),
            source: Some(invalid_source.into()),
            base_thread_id: None,
            base_message_id: None,
        },
        &state,
        &resolver,
    )
    .await
    .expect("failed source remains a created version");

    let version = response
        .workspace
        .selected_version
        .as_ref()
        .expect("error version");
    assert_eq!(version.status, MessageStatus::Error);
    assert_eq!(
        version
            .output
            .as_ref()
            .expect("attempted source")
            .macro_code,
        invalid_source,
    );
    let raw_error = response
        .initial_version_error
        .as_ref()
        .expect("raw initial error");
    assert!(!raw_error.message.trim().is_empty());
    assert_eq!(version.content, raw_error.to_string());
    assert_eq!(response.source_document.source, invalid_source);

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn invalid_creation_intent_mutates_no_history() {
    let (root, resolver, state) = fixture();

    let error = create_design_thread(
        CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Macro,
            title: Some("Missing source".into()),
            source: None,
            base_thread_id: None,
            base_message_id: None,
        },
        &state,
        &resolver,
    )
    .await
    .expect_err("macro source required");

    assert!(error.message.contains("requires non-empty source"));
    let conn = state.db.lock().await;
    assert!(ecky_cad_lib::services::history::get_history(&conn)
        .expect("history")
        .is_empty(),);
    drop(conn);

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn edited_source_fork_preserves_exact_base_authoring_context() {
    let (root, resolver, state) = fixture();
    let base_thread = create_design_thread(
        CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Blank,
            title: Some("Base".into()),
            source: None,
            base_thread_id: None,
            base_message_id: None,
        },
        &state,
        &resolver,
    )
    .await
    .expect("base thread");
    let mut parameters = DesignParams::new();
    parameters.insert("width".into(), ParamValue::Number(12.0));
    parameters.insert(
        "image".into(),
        ParamValue::String("/missing/base.png".into()),
    );
    let ui_spec = UiSpec {
        fields: vec![UiField::Number {
            key: "width".into(),
            label: "Width".into(),
            min: Some(1.0),
            max: Some(100.0),
            step: Some(1.0),
            min_from: None,
            max_from: None,
            frozen: false,
        }],
    };
    let post_processing = Some(PostProcessingSpec {
        displacement: Some(DisplacementSpec {
            image_param: "image".into(),
            projection: ProjectionType::Planar,
            depth_mm: 1.5,
            invert: false,
        }),
        lithophane_attachments: vec![],
    });
    let base_version = apply_manual_code(
        ManualCodeApplyRequest {
            thread_id: base_thread.thread_id.clone(),
            base_message_id: None,
            source: "(model (part body (box 10 11 12)))".into(),
            persist: true,
            title: Some("Base".into()),
            version_name: Some("V-base".into()),
            ui_spec: ui_spec.clone(),
            parameters: parameters.clone(),
            post_processing: post_processing.clone(),
            source_language: Some(SourceLanguage::EckyIrV0),
            geometry_backend: Some(GeometryBackend::EckyRust),
        },
        &state,
        &resolver,
    )
    .await
    .expect("base version");
    let base_message_id = base_version.message_id.expect("base message id");
    let base_output = {
        let conn = state.db.lock().await;
        ecky_cad_lib::db::get_thread_message_version(
            &conn,
            &base_thread.thread_id,
            &base_message_id,
        )
        .expect("base query")
        .and_then(|message| message.output)
        .expect("persisted base output")
    };
    {
        let mut config = state.config.lock().expect("config");
        config.default_source_language = SourceLanguage::LegacyPython;
        config.default_geometry_backend = GeometryBackend::Freecad;
    }

    let fork = create_design_thread(
        CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Macro,
            title: Some("Edited fork".into()),
            source: Some("(model (part body (box 20 21 22)))".into()),
            base_thread_id: Some(base_thread.thread_id),
            base_message_id: Some(base_message_id),
        },
        &state,
        &resolver,
    )
    .await
    .expect("edited source fork");

    let output = fork
        .workspace
        .selected_version
        .as_ref()
        .and_then(|message| message.output.as_ref())
        .expect("fork output");
    assert_eq!(output.source_language, SourceLanguage::EckyIrV0);
    assert_eq!(output.geometry_backend, GeometryBackend::EckyRust);
    assert_eq!(output.initial_params, base_output.initial_params);
    assert_eq!(output.ui_spec, base_output.ui_spec);
    assert_eq!(output.post_processing, base_output.post_processing);

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn partial_or_stale_base_identity_is_rejected_before_mutation() {
    let (root, resolver, state) = fixture();
    let source = Some("(model (part body (box 1 2 3)))".into());

    let partial = create_design_thread(
        CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Macro,
            title: Some("Partial".into()),
            source: source.clone(),
            base_thread_id: Some("source-thread".into()),
            base_message_id: None,
        },
        &state,
        &resolver,
    )
    .await
    .expect_err("partial base rejected");
    assert!(partial.message.contains("must be supplied together"));

    let stale = create_design_thread(
        CreateDesignThreadIntent {
            mode: DesignThreadCreationMode::Macro,
            title: Some("Stale".into()),
            source,
            base_thread_id: Some("source-thread".into()),
            base_message_id: Some("missing-version".into()),
        },
        &state,
        &resolver,
    )
    .await
    .expect_err("stale base rejected");
    assert!(stale.message.contains("was not found"));

    let conn = state.db.lock().await;
    assert!(ecky_cad_lib::services::history::get_history(&conn)
        .expect("history")
        .is_empty());
    drop(conn);
    std::fs::remove_dir_all(root).expect("cleanup");
}
