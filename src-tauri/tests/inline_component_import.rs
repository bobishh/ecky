use base64::{engine::general_purpose, Engine as _};
use ecky_cad_lib::contracts::{
    Config, DesignParams, GeometryBackend, MessageStatus, SourceLanguage, UiSpec,
};
use ecky_cad_lib::models::{AppState, PathResolver};
use ecky_cad_lib::services::design::{add_manual_version, AddManualVersionRequest};
use ecky_cad_lib::services::inline_component_import::{
    apply_inline_component_import, ApplyInlineComponentImportInput,
};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

struct TestResolver {
    root: PathBuf,
}

impl PathResolver for TestResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    fn app_data_dir(&self) -> PathBuf {
        self.root.join("data")
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

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    for (name, bytes) in entries {
        writer.start_file(*name, options).expect("start zip entry");
        writer.write_all(bytes).expect("write zip entry");
    }
    writer.finish().expect("finish zip").into_inner()
}

fn install_test_component(root: &Path, resolver: &TestResolver) {
    let source = b"(define-component peg () (box 2 2 2))";
    let manifest = br#"{
        "schemaVersion": 1,
        "packageId": "test.library",
        "version": "1.0.0",
        "displayName": "Test Library",
        "visibility": "source",
        "components": [{
            "componentId": "peg",
            "version": "1.0.0",
            "displayName": "Peg",
            "sourceRef": "components/peg/source.ecky",
            "entrySymbol": "peg"
        }]
    }"#;
    let payload = zip_bytes(&[
        ("ecky-package.json", manifest),
        ("components/peg/source.ecky", source),
    ]);
    let header = br#"{
        "schemaVersion": 1,
        "packageId": "test.library",
        "version": "1.0.0",
        "displayName": "Test Library",
        "visibility": "source",
        "components": [{
            "componentId": "peg",
            "version": "1.0.0",
            "displayName": "Peg",
            "entrySymbol": "peg"
        }],
        "assemblies": []
    }"#;
    let archive_path = root.join("test-library.ecky");
    let file = std::fs::File::create(&archive_path).expect("create package archive");
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    writer
        .start_file("ecky-header.json", options)
        .expect("start header");
    writer.write_all(header).expect("write header");
    writer
        .start_file("ecky-payload.b64", options)
        .expect("start payload");
    writer
        .write_all(general_purpose::STANDARD.encode(payload).as_bytes())
        .expect("write payload");
    writer.finish().expect("finish package archive");
    ecky_cad_lib::component_package_runtime::install_component_package_to_store(
        resolver,
        &archive_path,
    )
    .expect("install component package");
}

struct Fixture {
    root: PathBuf,
    resolver: TestResolver,
    state: AppState,
    base_message_id: String,
    source: String,
    source_digest: String,
}

async fn fixture(extra_part: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "ecky-inline-component-import-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("temp root");
    let resolver = TestResolver { root: root.clone() };
    install_test_component(&root, &resolver);
    let source = format!("(model (part base (box 1 1 1)) {extra_part})");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    let base_message_id = add_manual_version(
        AddManualVersionRequest {
            thread_id: "thread-1".to_string(),
            title: "Assembly".to_string(),
            version_name: "V1".to_string(),
            macro_code: source.clone(),
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
    let source_digest = ecky_cad_lib::services::render_snapshot::canonical_source_digest(&source);
    Fixture {
        root,
        resolver,
        state,
        base_message_id,
        source,
        source_digest,
    }
}

fn input(fixture: &Fixture) -> ApplyInlineComponentImportInput {
    ApplyInlineComponentImportInput {
        thread_id: "thread-1".to_string(),
        base_message_id: fixture.base_message_id.clone(),
        expected_source_digest: fixture.source_digest.clone(),
        package_id: "test.library".to_string(),
        version: "1.0.0".to_string(),
        component_id: "peg".to_string(),
    }
}

#[tokio::test]
async fn copy_inline_import_patches_renders_and_commits_one_version() {
    let fixture = fixture("").await;
    let result = apply_inline_component_import(input(&fixture), &fixture.state, &fixture.resolver)
        .await
        .expect("apply component import");

    assert_eq!(result.version.status, MessageStatus::Success);
    assert!(result.version.artifact_bundle.is_some());
    assert!(result.version.model_manifest.is_some());
    assert_eq!(result.entry_symbol, "peg");
    assert_eq!(result.part_key, "peg");
    assert!(result
        .version
        .design_output
        .macro_code
        .contains("(define-component peg"));
    assert!(result
        .version
        .design_output
        .macro_code
        .contains("(part peg (peg))"));
    assert!(!result
        .version
        .design_output
        .macro_code
        .contains("import-component"));
    let binding = {
        let conn = fixture.state.db.lock().await;
        ecky_cad_lib::thread_source_binding::get_binding(&conn, "thread-1")
            .expect("binding")
            .expect("bound source")
    };
    assert_eq!(
        std::fs::read_to_string(binding.source_path).expect("persisted source"),
        result.version.design_output.macro_code
    );
    let head = {
        let conn = fixture.state.db.lock().await;
        ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
            .expect("latest")
            .expect("committed import")
    };
    assert_eq!(Some(head.id.as_str()), result.version.message_id.as_deref());
    std::fs::remove_dir_all(fixture.root).expect("cleanup");
}

#[tokio::test]
async fn stale_source_digest_rejects_before_source_or_version_write() {
    let fixture = fixture("").await;
    let before = {
        let conn = fixture.state.db.lock().await;
        ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
            .expect("latest")
            .expect("base")
            .id
    };
    let mut request = input(&fixture);
    request.expected_source_digest = "sha256:stale".to_string();

    let error = apply_inline_component_import(request, &fixture.state, &fixture.resolver)
        .await
        .expect_err("stale source");
    assert!(error.message.contains("changed before component import"));
    let after = {
        let conn = fixture.state.db.lock().await;
        ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
            .expect("latest")
            .expect("base")
            .id
    };
    assert_eq!(before, after);
    let binding = {
        let conn = fixture.state.db.lock().await;
        ecky_cad_lib::thread_source_binding::get_binding(&conn, "thread-1")
            .expect("binding")
            .expect("bound source")
    };
    assert_eq!(
        std::fs::read_to_string(binding.source_path).expect("persisted source"),
        fixture.source
    );
    std::fs::remove_dir_all(fixture.root).expect("cleanup");
}

#[tokio::test]
async fn render_failure_returns_raw_error_and_commits_one_error_version() {
    let fixture = fixture("(part broken (sphere -1))").await;
    let result = apply_inline_component_import(input(&fixture), &fixture.state, &fixture.resolver)
        .await
        .expect("domain failure response");

    assert_eq!(result.version.status, MessageStatus::Error);
    assert!(!result
        .version
        .error
        .as_ref()
        .expect("raw render error")
        .message
        .trim()
        .is_empty());
    let head = {
        let conn = fixture.state.db.lock().await;
        ecky_cad_lib::db::get_thread_latest_version(&conn, "thread-1")
            .expect("latest")
            .expect("error version")
    };
    assert_eq!(head.status, MessageStatus::Error);
    assert_eq!(Some(head.id.as_str()), result.version.message_id.as_deref());
    std::fs::remove_dir_all(fixture.root).expect("cleanup");
}

#[test]
fn inline_component_import_contract_serializes_camel_case() {
    let value = serde_json::to_value(ApplyInlineComponentImportInput {
        thread_id: "thread-1".to_string(),
        base_message_id: "message-1".to_string(),
        expected_source_digest: "sha256:source".to_string(),
        package_id: "test.library".to_string(),
        version: "1.0.0".to_string(),
        component_id: "peg".to_string(),
    })
    .expect("serialize contract");
    assert_eq!(value["threadId"], "thread-1");
    assert_eq!(value["baseMessageId"], "message-1");
    assert_eq!(value["expectedSourceDigest"], "sha256:source");
    assert_eq!(value["packageId"], "test.library");
    assert_eq!(value["componentId"], "peg");
    assert!(value.get("thread_id").is_none());
}
