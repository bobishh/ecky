use ecky_cad_lib::contracts::{
    Config, DesignParams, GeometryBackend, MessageStatus, SourceLanguage, UiSpec,
};
use ecky_cad_lib::models::{AppState, PathResolver};
use ecky_cad_lib::services::design::{add_manual_version, AddManualVersionRequest};
use ecky_cad_lib::services::manual_code::{apply_manual_code, ManualCodeApplyRequest};
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

async fn fixture() -> (PathBuf, TestResolver, AppState, String) {
    let root = std::env::temp_dir().join(format!("ecky-manual-code-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("temp root");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    let resolver = TestResolver { root: root.clone() };
    let base_message_id = add_manual_version(
        AddManualVersionRequest {
            thread_id: "thread-1".to_string(),
            title: "Cube".to_string(),
            version_name: "V1".to_string(),
            macro_code: "(model (part body (box 10 10 10)))".to_string(),
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

fn request(base_message_id: String, source: &str) -> ManualCodeApplyRequest {
    ManualCodeApplyRequest {
        thread_id: "thread-1".to_string(),
        base_message_id: Some(base_message_id),
        source: source.to_string(),
        persist: true,
        title: Some("Edited cube".to_string()),
        version_name: Some("V2".to_string()),
        ui_spec: UiSpec::default(),
        parameters: DesignParams::new(),
        post_processing: None,
        source_language: Some(SourceLanguage::EckyIrV0),
        geometry_backend: Some(GeometryBackend::EckyRust),
    }
}

#[tokio::test]
async fn changed_code_appends_then_renders_one_success_version_and_snapshot() {
    let (root, resolver, state, base_message_id) = fixture().await;
    let source = "(model (part body (box 12 13 14)))";

    let response = apply_manual_code(request(base_message_id.clone(), source), &state, &resolver)
        .await
        .expect("manual code response");

    assert_eq!(response.status, MessageStatus::Success);
    assert_eq!(
        response.base_message_id.as_deref(),
        Some(base_message_id.as_str())
    );
    assert_ne!(
        response.message_id.as_deref(),
        Some(base_message_id.as_str())
    );
    assert_eq!(response.design_output.macro_code, source);
    assert!(response.error.is_none());
    let bundle = response
        .artifact_bundle
        .as_ref()
        .expect("rendered artifact");
    let manifest = response.model_manifest.as_ref().expect("rendered manifest");
    assert_eq!(bundle.model_id, manifest.model_id);

    let conn = state.db.lock().await;
    let head = ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
        .expect("latest query")
        .expect("latest version");
    assert_eq!(Some(head.id.as_str()), response.message_id.as_deref());
    assert_eq!(head.status, MessageStatus::Success);
    assert_eq!(head.output.expect("design").macro_code, source);
    let binding = ecky_cad_lib::thread_source_binding::get_binding(&conn, "thread-1")
        .expect("binding query")
        .expect("bound source");
    assert_eq!(
        std::fs::read_to_string(&binding.source_path).expect("committed source"),
        source
    );
    drop(conn);

    let snapshot = state
        .last_snapshot
        .lock()
        .unwrap()
        .clone()
        .expect("snapshot");
    assert_eq!(
        snapshot.message_id.as_deref(),
        response.message_id.as_deref()
    );
    assert_eq!(snapshot.design.expect("snapshot design").macro_code, source);
    assert_eq!(
        snapshot
            .artifact_bundle
            .expect("snapshot artifact")
            .model_id,
        bundle.model_id
    );

    let json = serde_json::to_value(&response).expect("serialize response");
    assert!(json.get("threadId").is_some());
    assert!(json.get("messageId").is_some());
    assert!(json.get("designOutput").is_some());
    assert!(json.get("thread_id").is_none());
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn invalid_code_is_already_head_then_records_raw_render_error_without_second_version() {
    let (root, resolver, state, base_message_id) = fixture().await;
    let invalid_source = "(model (part body (box 12 13";

    let response = apply_manual_code(request(base_message_id, invalid_source), &state, &resolver)
        .await
        .expect("domain failure response");

    assert_eq!(response.status, MessageStatus::Error);
    assert!(response.artifact_bundle.is_none());
    assert!(response.model_manifest.is_none());
    let raw_error = response.error.as_ref().expect("raw backend error");
    assert!(!raw_error.message.trim().is_empty());

    let conn = state.db.lock().await;
    let head = ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
        .expect("latest query")
        .expect("failed version");
    assert_eq!(Some(head.id.as_str()), response.message_id.as_deref());
    assert_eq!(head.status, MessageStatus::Error);
    assert_eq!(
        head.output.expect("attempted source").macro_code,
        invalid_source
    );
    assert_eq!(head.content, raw_error.to_string());
    let version_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1' AND role = 'assistant'",
            [],
            |row| row.get(0),
        )
        .expect("count versions");
    assert_eq!(version_count, 2, "base + one failed immutable version");
    drop(conn);

    let snapshot = state
        .last_snapshot
        .lock()
        .unwrap()
        .clone()
        .expect("failed snapshot");
    assert_eq!(
        snapshot.message_id.as_deref(),
        response.message_id.as_deref()
    );
    assert_eq!(
        snapshot.design.expect("attempted snapshot").macro_code,
        invalid_source
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn unchanged_successful_code_apply_reuses_durable_version_without_render_append() {
    let (root, resolver, state, base_message_id) = fixture().await;
    let source = "(model (part body (box 20 21 22)))";
    let first = apply_manual_code(request(base_message_id.clone(), source), &state, &resolver)
        .await
        .expect("first apply");
    let first_message_id = first.message_id.clone().expect("persisted message");
    let binding = {
        let conn = state.db.lock().await;
        ecky_cad_lib::thread_source_binding::get_binding(&conn, "thread-1")
            .expect("binding query")
            .expect("bound source")
    };
    std::fs::write(&binding.source_path, "stale source").expect("simulate stale bound file");
    let second = apply_manual_code(request(first_message_id.clone(), source), &state, &resolver)
        .await
        .expect("unchanged apply");

    assert_eq!(second.status, MessageStatus::Success);
    assert_eq!(
        second.message_id.as_deref(),
        Some(first_message_id.as_str())
    );
    assert_eq!(
        std::fs::read_to_string(&binding.source_path).expect("repaired source"),
        source
    );
    let conn = state.db.lock().await;
    let version_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1' AND role = 'assistant'",
            [],
            |row| row.get(0),
        )
        .expect("count versions");
    assert_eq!(version_count, 2, "base + one changed source version");
    drop(conn);
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn code_preview_renders_snapshot_without_growing_history() {
    let (root, resolver, state, base_message_id) = fixture().await;
    let source = "(model (part body (box 30 31 32)))";
    let mut preview_request = request(base_message_id.clone(), source);
    preview_request.persist = false;

    let response = apply_manual_code(preview_request, &state, &resolver)
        .await
        .expect("preview response");

    assert_eq!(response.status, MessageStatus::Success);
    assert!(response.message_id.is_none());
    assert_eq!(response.design_output.macro_code, source);
    assert!(response.artifact_bundle.is_some());
    let conn = state.db.lock().await;
    let version_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1' AND role = 'assistant'",
            [],
            |row| row.get(0),
        )
        .expect("count versions");
    assert_eq!(version_count, 1, "preview must not append history");
    let binding = ecky_cad_lib::thread_source_binding::get_binding(&conn, "thread-1")
        .expect("binding query")
        .expect("bound source");
    assert_eq!(
        std::fs::read_to_string(&binding.source_path).expect("preview source"),
        source
    );
    drop(conn);
    let snapshot = state
        .last_snapshot
        .lock()
        .unwrap()
        .clone()
        .expect("preview snapshot");
    assert_eq!(
        snapshot.message_id.as_deref(),
        Some(base_message_id.as_str())
    );
    assert_eq!(snapshot.design.expect("preview design").macro_code, source);
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn invalid_code_preview_returns_raw_error_without_appending_history() {
    let (root, resolver, state, base_message_id) = fixture().await;
    let invalid_source = "(model (part body (box 30 31";
    let mut preview_request = request(base_message_id.clone(), invalid_source);
    preview_request.persist = false;

    let response = apply_manual_code(preview_request, &state, &resolver)
        .await
        .expect("preview failure response");

    assert_eq!(response.status, MessageStatus::Error);
    assert!(response.message_id.is_none());
    assert_eq!(response.design_output.macro_code, invalid_source);
    assert!(response.error.is_some());
    let conn = state.db.lock().await;
    let head = ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
        .expect("latest query")
        .expect("base remains head");
    assert_eq!(head.id, base_message_id);
    let version_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1' AND role = 'assistant'",
            [],
            |row| row.get(0),
        )
        .expect("count versions");
    assert_eq!(version_count, 1, "failed preview must not append history");
    let binding = ecky_cad_lib::thread_source_binding::get_binding(&conn, "thread-1")
        .expect("binding query")
        .expect("bound source");
    assert_eq!(
        std::fs::read_to_string(&binding.source_path).expect("unchanged source"),
        "(model (part body (box 10 10 10)))"
    );
    drop(conn);
    std::fs::remove_dir_all(root).expect("cleanup");
}
