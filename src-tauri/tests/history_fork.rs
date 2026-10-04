use ecky_cad_lib::contracts::{
    ArtifactBundle, Config, DesignParams, DocumentMetadata, EnrichmentStatus, GeometryBackend,
    ManifestEnrichmentState, Message, MessageRole, MessageStatus, ModelManifest, ModelSourceKind,
    SourceLanguage, ThreadStatus, UiSpec,
};
use ecky_cad_lib::models::{AppState, PathResolver};
use ecky_cad_lib::services::design::{add_manual_version, AddManualVersionRequest};
use ecky_cad_lib::services::history::{
    delete_thread_intent, delete_version_intent, finalize_thread_intent, fork_design,
    open_inventory_thread_intent, reopen_thread_intent, restore_version_intent, ForkDesignRequest,
    OpenInventoryThreadIntent, ThreadLifecycleIntent, VersionHistoryIntent,
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

async fn fixture() -> (PathBuf, TestResolver, AppState, String, String) {
    let root = std::env::temp_dir().join(format!("ecky-history-fork-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("temp root");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    let resolver = TestResolver { root: root.clone() };
    let source = "(model (part body (box 10 20 30)))";
    let older_id = add_manual_version(
        AddManualVersionRequest {
            thread_id: "source-thread".to_string(),
            title: "Bracket".to_string(),
            version_name: "V1".to_string(),
            macro_code: source.to_string(),
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
    .expect("older version");
    let newer_id = add_manual_version(
        AddManualVersionRequest {
            thread_id: "source-thread".to_string(),
            title: "Bracket".to_string(),
            version_name: "V2".to_string(),
            macro_code: "(model (part body (box 40 50 60)))".to_string(),
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
    .expect("newer version");
    (root, resolver, state, older_id, newer_id)
}

fn seed_saved_restart_pointer(
    state: &AppState,
    resolver: &TestResolver,
    thread_id: &str,
    message_id: &str,
) {
    let snapshot = ecky_cad_lib::services::session::build_saved_version_snapshot(
        None,
        thread_id.to_string(),
        message_id.to_string(),
        None,
        None,
        None,
    );
    *state.last_snapshot.lock().unwrap() = Some(snapshot.clone());
    ecky_cad_lib::services::session::write_last_snapshot(resolver, Some(&snapshot));
}

#[tokio::test]
async fn fork_design_clones_exact_selected_version_and_returns_canonical_projection() {
    let (root, resolver, state, older_id, newer_id) = fixture().await;

    let response = fork_design(
        ForkDesignRequest {
            source_thread_id: "source-thread".to_string(),
            source_message_id: older_id.clone(),
            title: None,
            version_name: None,
            message_limit: Some(20),
        },
        &state,
        &resolver,
    )
    .await
    .expect("fork response");

    assert_ne!(response.thread_id, "source-thread");
    assert_ne!(response.message_id, older_id);
    assert_ne!(response.message_id, newer_id);
    assert_eq!(response.workspace.thread.id, response.thread_id);
    assert_eq!(response.workspace.messages_page.messages.len(), 1);
    assert_eq!(
        response
            .workspace
            .selected_version
            .as_ref()
            .map(|message| message.id.as_str()),
        Some(response.message_id.as_str())
    );
    assert!(response.workspace.requested_message_found);
    let forked = response
        .workspace
        .selected_version
        .as_ref()
        .expect("selected fork");
    assert_eq!(forked.status, MessageStatus::Success);
    let output = forked.output.as_ref().expect("manual output");
    assert_eq!(output.title, "Bracket");
    assert_eq!(output.version_name, "V1");
    assert_eq!(output.macro_code, "(model (part body (box 10 20 30)))");

    let json = serde_json::to_value(&response).expect("serialize response");
    assert!(json.get("threadId").is_some());
    assert!(json.get("messageId").is_some());
    assert!(json.get("source_thread_id").is_none());

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn fork_design_rejects_message_outside_source_thread_without_creating_history() {
    let (root, resolver, state, older_id, _) = fixture().await;
    let before = {
        let conn = state.db.lock().await;
        ecky_cad_lib::services::history::get_history(&conn)
            .expect("history")
            .len()
    };

    let error = fork_design(
        ForkDesignRequest {
            source_thread_id: "wrong-thread".to_string(),
            source_message_id: older_id,
            title: None,
            version_name: None,
            message_limit: Some(20),
        },
        &state,
        &resolver,
    )
    .await
    .expect_err("cross-thread source must fail");

    assert!(error.to_string().contains("source version"));
    let after = {
        let conn = state.db.lock().await;
        ecky_cad_lib::services::history::get_history(&conn)
            .expect("history")
            .len()
    };
    assert_eq!(after, before);

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn delete_selected_version_returns_next_canonical_workspace_and_bounded_page() {
    let (root, resolver, state, older_id, newer_id) = fixture().await;

    let response = delete_version_intent(
        VersionHistoryIntent {
            message_id: newer_id.clone(),
            selected_thread_id: Some("source-thread".to_string()),
            selected_message_id: Some(newer_id),
            message_limit: Some(1),
        },
        &state,
        &resolver,
    )
    .await
    .expect("delete projection");

    let boundary = serde_json::to_value(&response).expect("camelCase response");
    assert!(boundary.get("threadId").is_some());
    assert!(boundary.get("threadRemoved").is_some());
    assert!(boundary.get("thread_id").is_none());

    assert_eq!(response.thread_id, "source-thread");
    assert!(!response.thread_removed);
    let workspace = response.workspace.expect("remaining workspace");
    assert_eq!(workspace.messages_page.messages.len(), 1);
    assert_eq!(
        workspace
            .selected_version
            .as_ref()
            .map(|message| message.id.as_str()),
        Some(older_id.as_str())
    );
    assert_eq!(
        state
            .last_snapshot
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|snapshot| snapshot.message_id.as_deref()),
        Some(older_id.as_str())
    );
    let persisted_message_id = {
        let conn = state.db.lock().await;
        ecky_cad_lib::services::session::read_last_snapshot(&resolver, &conn)
            .and_then(|snapshot| snapshot.message_id)
    };
    assert_eq!(persisted_message_id.as_deref(), Some(older_id.as_str()));

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn restore_version_returns_restored_canonical_workspace_without_follow_up_reads() {
    let (root, resolver, state, older_id, newer_id) = fixture().await;
    delete_version_intent(
        VersionHistoryIntent {
            message_id: older_id.clone(),
            selected_thread_id: Some("source-thread".to_string()),
            selected_message_id: Some(newer_id.clone()),
            message_limit: Some(1),
        },
        &state,
        &resolver,
    )
    .await
    .expect("delete older version");

    let response = restore_version_intent(
        VersionHistoryIntent {
            message_id: older_id.clone(),
            selected_thread_id: Some("source-thread".to_string()),
            selected_message_id: Some(newer_id),
            message_limit: Some(1),
        },
        &state,
        &resolver,
    )
    .await
    .expect("restore projection");

    let workspace = response.workspace.expect("restored workspace");
    assert_eq!(workspace.messages_page.messages.len(), 1);
    assert_eq!(
        workspace
            .selected_version
            .as_ref()
            .map(|message| message.id.as_str()),
        Some(older_id.as_str())
    );
    assert_eq!(
        state
            .last_snapshot
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|snapshot| snapshot.message_id.as_deref()),
        Some(older_id.as_str())
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn delete_active_thread_returns_bounded_history_and_clears_restart_pointer() {
    let (root, resolver, state, _, newer_id) = fixture().await;
    seed_saved_restart_pointer(&state, &resolver, "source-thread", &newer_id);
    let response = delete_thread_intent(
        ThreadLifecycleIntent {
            thread_id: "source-thread".to_string(),
            selected_message_id: Some(newer_id),
        },
        &state,
        &resolver,
    )
    .await
    .expect("delete thread intent");

    assert!(response
        .history
        .iter()
        .all(|thread| thread.id != "source-thread"));
    assert!(state.last_snapshot.lock().unwrap().is_none());
    assert!(!ecky_cad_lib::services::session::last_snapshot_path(&resolver).exists());
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn finalize_reopen_and_inventory_open_return_canonical_bounded_projections() {
    let (root, resolver, state, _, _) = fixture().await;
    let source_id = "runtime-final-version";
    {
        let conn = state.db.lock().await;
        ecky_cad_lib::db::add_legacy_message(
            &conn,
            "source-thread",
            &runtime_only_message(source_id),
        )
        .expect("runtime final version");
    }
    seed_saved_restart_pointer(&state, &resolver, "source-thread", source_id);

    let finalized = finalize_thread_intent(
        ThreadLifecycleIntent {
            thread_id: "source-thread".to_string(),
            selected_message_id: Some(source_id.to_string()),
        },
        &state,
        &resolver,
    )
    .await
    .expect("finalize intent");
    assert!(finalized
        .history
        .iter()
        .all(|thread| thread.id != "source-thread"));
    assert!(state.last_snapshot.lock().unwrap().is_none());

    let opened = open_inventory_thread_intent(
        OpenInventoryThreadIntent {
            thread_id: "source-thread".to_string(),
            message_limit: Some(1),
        },
        &state,
        &resolver,
    )
    .await
    .expect("open inventory workspace");
    assert_eq!(opened.thread.id, "source-thread");
    assert_eq!(opened.thread.status, ThreadStatus::Finalized);
    assert_eq!(opened.messages_page.messages.len(), 1);
    let opened_message_id = opened
        .selected_version
        .as_ref()
        .map(|message| message.id.as_str())
        .expect("completed workspace selected version");
    assert_eq!(
        state
            .last_snapshot
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|snapshot| snapshot.message_id.as_deref()),
        Some(opened_message_id)
    );

    let reopened = reopen_thread_intent(
        ThreadLifecycleIntent {
            thread_id: "source-thread".to_string(),
            selected_message_id: None,
        },
        &state,
    )
    .await
    .expect("reopen intent");
    assert!(reopened
        .history
        .iter()
        .any(|thread| thread.id == "source-thread"));
    std::fs::remove_dir_all(root).expect("cleanup");
}

fn runtime_only_message(id: &str) -> Message {
    let model_id = "imported-mesh-runtime".to_string();
    let bundle = ArtifactBundle {
        schema_version: 1,
        model_id: model_id.clone(),
        source_kind: ModelSourceKind::ImportedMesh,
        engine_kind: ecky_cad_lib::contracts::EngineKind::Freecad,
        source_language: SourceLanguage::LegacyPython,
        geometry_backend: GeometryBackend::Freecad,
        content_hash: "sha256:runtime".to_string(),
        artifact_version: 1,
        fcstd_path: "/tmp/runtime.FCStd".to_string(),
        manifest_path: "/tmp/runtime.json".to_string(),
        macro_path: None,
        model_stl_path: "/tmp/runtime.stl".to_string(),
        viewer_assets: Vec::new(),
        edge_targets: Vec::new(),
        face_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: Vec::new(),
        geometry_provenance: None,
        component_dependency_lock: None,
        component_dependency_lock_digest: None,
        component_import_origins: Vec::new(),
        component_placement_evidence: Vec::new(),
    };
    let manifest = ModelManifest {
        schema_version: 1,
        model_id,
        source_kind: ModelSourceKind::ImportedMesh,
        source_digest: None,
        core_digest: None,
        ast_schema_version: None,
        engine_kind: ecky_cad_lib::contracts::EngineKind::Freecad,
        source_language: SourceLanguage::LegacyPython,
        geometry_backend: GeometryBackend::Freecad,
        document: DocumentMetadata {
            document_name: "RuntimeOnly".to_string(),
            document_label: "Runtime only model".to_string(),
            source_path: None,
            object_count: 0,
            warnings: Vec::new(),
        },
        parts: Vec::new(),
        parameter_groups: Vec::new(),
        control_primitives: Vec::new(),
        control_relations: Vec::new(),
        control_views: Vec::new(),
        preview_views: Vec::new(),
        advisories: Vec::new(),
        selection_targets: Vec::new(),
        measurement_annotations: Vec::new(),
        tagged_anchors: Default::default(),
        feature_graph: None,
        correspondence_graph: None,
        analysis_declarations: Vec::new(),
        warnings: Vec::new(),
        enrichment_state: ManifestEnrichmentState {
            status: EnrichmentStatus::None,
            proposals: Vec::new(),
        },
        geometry_provenance: None,
        component_import_origins: Vec::new(),
        component_placement_evidence: Vec::new(),
    };
    Message {
        id: id.to_string(),
        role: MessageRole::Assistant,
        content: "Runtime model".to_string(),
        status: MessageStatus::Success,
        output: None,
        usage: None,
        artifact_bundle: Some(bundle),
        model_manifest: Some(manifest),
        structural_verification: None,
        agent_origin: None,
        image_data: None,
        visual_kind: None,
        attachment_images: Vec::new(),
        timestamp: 500,
    }
}

#[tokio::test]
async fn fork_design_clones_runtime_only_version_without_frontend_payload_branching() {
    let (root, resolver, state, _, _) = fixture().await;
    let source_id = "runtime-only-source";
    {
        let conn = state.db.lock().await;
        ecky_cad_lib::db::add_legacy_message(
            &conn,
            "source-thread",
            &runtime_only_message(source_id),
        )
        .expect("runtime-only source");
    }

    let response = fork_design(
        ForkDesignRequest {
            source_thread_id: "source-thread".to_string(),
            source_message_id: source_id.to_string(),
            title: None,
            version_name: None,
            message_limit: Some(20),
        },
        &state,
        &resolver,
    )
    .await
    .expect("runtime-only fork");

    let forked = response.workspace.selected_version.expect("selected fork");
    assert!(forked.output.is_some());
    assert_eq!(
        forked
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.as_str()),
        Some("imported-mesh-runtime")
    );
    assert_eq!(
        forked
            .model_manifest
            .as_ref()
            .map(|manifest| manifest.model_id.as_str()),
        Some("imported-mesh-runtime")
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
