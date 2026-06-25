
use super::*;
use crate::contracts::{
    AppErrorCode, Config, ControlPrimitiveKind, ControlRelationMode, ControlViewScope,
    DesignParams, DocumentMetadata, EnrichmentStatus, McpConfig, MeasurementAnnotation,
    MeasurementAnnotationSource, MeasurementAxis, MeasurementBasis, Message, MessageRole,
    MessageStatus, MessageVisualKind, ParamValue, UiField,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

struct TestPathResolver {
    root: PathBuf,
}

impl PathResolver for TestPathResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.root.clone()
    }

    fn app_data_dir(&self) -> PathBuf {
        self.root.clone()
    }

    fn resource_path(&self, path: &str) -> Option<PathBuf> {
        Some(self.root.join(path))
    }
}

fn test_db_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ecky-mcp-{}-{}", name, Uuid::new_v4()))
}

fn write_executable(path: &std::path::Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}

fn write_closed_tetra_binary_stl(path: &std::path::Path) {
    let triangles = [
        [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [[0.0f32, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
        [[0.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [[1.0f32, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
    ];

    write_binary_stl(path, &triangles);
}

fn write_binary_stl(path: &std::path::Path, triangles: &[[[f32; 3]; 3]]) {
    let mut bytes = vec![0u8; 80];
    bytes.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for triangle in triangles.iter().copied() {
        for normal_component in [0.0f32, 0.0, 0.0] {
            bytes.extend_from_slice(&normal_component.to_le_bytes());
        }
        for vertex in triangle {
            for component in vertex {
                bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
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
        voice: crate::models::VoiceConfig::default(),
        mcp: McpConfig::default(),
        has_seen_onboarding: true,
        connection_type: None,
        default_engine_kind: crate::models::EngineKind::Freecad,
        default_geometry_backend: crate::models::GeometryBackend::Freecad,
        default_source_language: crate::models::SourceLanguage::LegacyPython,
        max_generation_attempts: 3,
        max_verify_attempts: 0,
        projects_root: None,
    }
}

fn test_session_id() -> String {
    // Globals like MACRO_BUFFERS and SESSION_RENDER_PREVIEWS are keyed by
    // session id; a per-test-thread nonce keeps tests from contaminating
    // each other through them while staying stable within one test.
    thread_local! {
        static NONCE: String = uuid::Uuid::new_v4().simple().to_string();
    }
    NONCE.with(|nonce| format!("session-1-{nonce}"))
}

fn test_session_id_other() -> String {
    thread_local! {
        static NONCE: String = uuid::Uuid::new_v4().simple().to_string();
    }
    NONCE.with(|nonce| format!("session-2-{nonce}"))
}

fn test_ctx() -> AgentContext {
    AgentContext {
        session_id: test_session_id(),
        client_kind: "http".to_string(),
        host_label: "Claude Code".to_string(),
        agent_label: "claude".to_string(),
        llm_model_id: None,
        llm_model_label: Some("Claude Sonnet".to_string()),
    }
}

fn test_ctx_other() -> AgentContext {
    AgentContext {
        session_id: test_session_id_other(),
        client_kind: "http".to_string(),
        host_label: "Codex".to_string(),
        agent_label: "codex".to_string(),
        llm_model_id: None,
        llm_model_label: Some("GPT-5.4".to_string()),
    }
}

#[test]
fn infer_macro_source_language_maps_dialect_to_authoring_language() {
    assert_eq!(
        infer_macro_source_language(&MacroDialect::Legacy),
        crate::models::SourceLanguage::LegacyPython
    );
    assert_eq!(
        infer_macro_source_language(&MacroDialect::CadFrameworkV1),
        crate::models::SourceLanguage::LegacyPython
    );
    assert_eq!(
        infer_macro_source_language(&MacroDialect::EckyIrV0),
        crate::models::SourceLanguage::EckyIrV0
    );
    assert_eq!(
        infer_macro_source_language(&MacroDialect::Build123d),
        crate::models::SourceLanguage::Build123d
    );
}

#[test]
fn macro_replacement_authoring_context_rejects_source_language_change() {
    let err = resolve_macro_authoring_context(
        crate::models::SourceLanguage::LegacyPython,
        crate::models::GeometryBackend::Freecad,
        &MacroDialect::EckyIrV0,
        None,
        crate::models::GeometryBackend::EckyRust,
    )
    .expect_err("ecky macro should not replace legacy python model source");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("source language"));
}

#[test]
fn first_version_authoring_context_rejects_raw_freecad_by_policy() {
    let conn = crate::db::init_db(&test_db_path("mcp-first-version-policy")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    let base = first_version_authoring_context(&state, &MacroDialect::Legacy, None);

    assert_eq!(
        base.source_language,
        crate::models::SourceLanguage::EckyIrV0
    );
    assert_eq!(
        base.geometry_backend,
        crate::models::GeometryBackend::Freecad
    );

    let err = resolve_macro_authoring_context(
        base.source_language,
        base.geometry_backend,
        &MacroDialect::Legacy,
        None,
        crate::models::GeometryBackend::EckyRust,
    )
    .expect_err("raw FreeCAD macro must not bootstrap a new MCP version");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("source language"));
}

#[test]
fn macro_replacement_authoring_context_rejects_non_ecky_backend_override() {
    let err = resolve_macro_authoring_context(
        crate::models::SourceLanguage::Build123d,
        crate::models::GeometryBackend::Build123d,
        &MacroDialect::Build123d,
        Some(crate::models::GeometryBackend::Freecad),
        crate::models::GeometryBackend::EckyRust,
    )
    .expect_err("non-ecky model must follow version backend setting");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("Geometry backend override"));
}

#[test]
fn macro_replacement_authoring_context_allows_ecky_backend_override() {
    let context = resolve_macro_authoring_context(
        crate::models::SourceLanguage::EckyIrV0,
        crate::models::GeometryBackend::EckyRust,
        &MacroDialect::EckyIrV0,
        Some(crate::models::GeometryBackend::Build123d),
        crate::models::GeometryBackend::EckyRust,
    )
    .expect("ecky source should allow geometry backend override");

    assert_eq!(
        context.source_language,
        crate::models::SourceLanguage::EckyIrV0
    );
    assert_eq!(
        context.geometry_backend,
        crate::models::GeometryBackend::Build123d
    );
}

#[test]
fn ecky_geometry_backend_follows_global_config_over_version() {
    // A version last rendered on build123d, no explicit per-render request:
    // the global config engine wins, so switching config re-renders it on
    // native without forking a new thread.
    let context = resolve_macro_authoring_context(
        crate::models::SourceLanguage::EckyIrV0,
        crate::models::GeometryBackend::Build123d, // version's stored backend
        &MacroDialect::EckyIrV0,
        None,                                       // no explicit override
        crate::models::GeometryBackend::EckyRust,   // config default
    )
    .expect("ecky source resolves a backend");
    assert_eq!(
        context.geometry_backend,
        crate::models::GeometryBackend::EckyRust,
        "global config must override the version's stored backend for Ecky source"
    );
}

#[test]
fn non_ecky_backend_stays_pinned_to_version_ignoring_config() {
    // Legacy python is bound to FreeCAD; the config engine must not switch it.
    let context = resolve_macro_authoring_context(
        crate::models::SourceLanguage::LegacyPython,
        crate::models::GeometryBackend::Freecad,
        &MacroDialect::Legacy,
        None,
        crate::models::GeometryBackend::EckyRust, // config default ignored here
    )
    .expect("legacy python resolves its own backend");
    assert_eq!(
        context.geometry_backend,
        crate::models::GeometryBackend::Freecad
    );
}

fn sample_ui_spec() -> UiSpec {
    UiSpec {
        fields: vec![
            UiField::Range {
                key: "diameter".to_string(),
                label: "Diameter".to_string(),
                min: Some(10.0),
                max: Some(200.0),
                step: Some(1.0),
                min_from: None,
                max_from: None,
                frozen: false,
            },
            UiField::Select {
                key: "mount".to_string(),
                label: "Mount".to_string(),
                options: vec![crate::contracts::SelectOption {
                    label: "Inner".to_string(),
                    value: crate::contracts::SelectValue::String("inner".to_string()),
                }],
                frozen: false,
            },
            UiField::Checkbox {
                key: "lip".to_string(),
                label: "Lip".to_string(),
                frozen: false,
            },
        ],
    }
}

fn sample_params() -> DesignParams {
    BTreeMap::from([
        ("diameter".to_string(), ParamValue::Number(130.0)),
        ("mount".to_string(), ParamValue::String("inner".to_string())),
        ("lip".to_string(), ParamValue::Boolean(true)),
    ])
}

fn sample_design(title: &str, version_name: &str, macro_code: &str) -> DesignOutput {
    DesignOutput {
        title: title.to_string(),
        version_name: version_name.to_string(),
        response: "ok".to_string(),
        interaction_mode: InteractionMode::Design,
        macro_code: macro_code.to_string(),
        macro_dialect: MacroDialect::Legacy,
        engine_kind: crate::models::EngineKind::Freecad,
        geometry_backend: crate::models::GeometryBackend::Freecad,
        source_language: crate::models::SourceLanguage::LegacyPython,
        ui_spec: sample_ui_spec(),
        initial_params: sample_params(),
        post_processing: Some(crate::contracts::PostProcessingSpec {
            displacement: None,
            lithophane_attachments: vec![],
        }),
    }
}

fn sample_bundle(model_id: &str, preview_name: &str) -> ArtifactBundle {
    ArtifactBundle {
        schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: crate::models::EngineKind::Freecad,
        geometry_backend: crate::models::GeometryBackend::Freecad,
        source_language: crate::models::SourceLanguage::LegacyPython,
        content_hash: format!("hash-{}", model_id),
        artifact_version: 1,
        fcstd_path: format!("/tmp/{}.FCStd", model_id),
        manifest_path: format!("/tmp/{}.json", model_id),
        macro_path: Some(format!("/tmp/{}.py", model_id)),
        preview_stl_path: format!("/tmp/{}", preview_name),
        viewer_assets: Vec::new(),
        edge_targets: Vec::new(),
        face_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: Vec::new(),
    }
}

fn sample_manifest(model_id: &str) -> ModelManifest {
    ModelManifest {
        schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.to_string(),
        source_kind: ModelSourceKind::Generated,
        source_digest: None,
        core_digest: None,
        ast_schema_version: None,
        engine_kind: crate::models::EngineKind::Freecad,
        geometry_backend: crate::models::GeometryBackend::Freecad,
        source_language: crate::models::SourceLanguage::LegacyPython,
        document: DocumentMetadata {
            document_name: "Doc".to_string(),
            document_label: "Doc".to_string(),
            source_path: None,
            object_count: 1,
            warnings: Vec::new(),
        },
        parts: vec![crate::models::PartBinding {
            part_id: "body".to_string(),
            freecad_object_name: "Body".to_string(),
            label: "Body".to_string(),
            kind: "solid".to_string(),
            semantic_role: None,
            viewer_asset_path: None,
            viewer_node_ids: vec!["body".to_string()],
            parameter_keys: Vec::new(),
            editable: true,
            bounds: None,
            volume: None,
            area: None,
        }],
        parameter_groups: Vec::new(),
        control_primitives: vec![
            ControlPrimitive {
                primitive_id: "diameter".to_string(),
                label: "Diameter".to_string(),
                kind: ControlPrimitiveKind::Number,
                source: ControlViewSource::Llm,
                part_ids: Vec::new(),
                bindings: vec![crate::contracts::PrimitiveBinding {
                    parameter_key: "diameter".to_string(),
                    scale: 1.0,
                    offset: 0.0,
                    min: None,
                    max: None,
                }],
                editable: true,
                order: 1,
            },
            ControlPrimitive {
                primitive_id: "lip".to_string(),
                label: "Lip".to_string(),
                kind: ControlPrimitiveKind::Toggle,
                source: ControlViewSource::Llm,
                part_ids: Vec::new(),
                bindings: vec![crate::contracts::PrimitiveBinding {
                    parameter_key: "lip".to_string(),
                    scale: 1.0,
                    offset: 0.0,
                    min: None,
                    max: None,
                }],
                editable: true,
                order: 2,
            },
        ],
        control_relations: vec![crate::contracts::ControlRelation {
            relation_id: "rel-1".to_string(),
            source_primitive_id: "diameter".to_string(),
            target_primitive_id: "lip".to_string(),
            mode: ControlRelationMode::Mirror,
            scale: 1.0,
            offset: 0.0,
            enabled: true,
        }],
        control_views: vec![ControlView {
            view_id: "main".to_string(),
            label: "Main".to_string(),
            scope: ControlViewScope::Global,
            part_ids: Vec::new(),
            primitive_ids: vec!["diameter".to_string(), "lip".to_string()],
            sections: Vec::new(),
            is_default: true,
            source: ControlViewSource::Llm,
            status: EnrichmentStatus::Accepted,
            order: 1,
        }],
        preview_views: Vec::new(),
        advisories: Vec::new(),
        selection_targets: vec![
            crate::models::SelectionTarget {
                target_id: Some("body:edge:0:0-0-0_10-0-0".to_string()),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "body".to_string(),
                label: "Body.Edge1".to_string(),
                kind: crate::models::SelectionTargetKind::Edge,
                editable: true,
                parameter_keys: Vec::new(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            },
            crate::models::SelectionTarget {
                target_id: Some("body:face:0:5-5-5:100".to_string()),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "body".to_string(),
                label: "Body.Face1".to_string(),
                kind: crate::models::SelectionTargetKind::Face,
                editable: true,
                parameter_keys: Vec::new(),
                primitive_ids: Vec::new(),
                view_ids: Vec::new(),
            },
        ],
        measurement_annotations: Vec::new(),
        tagged_anchors: std::collections::BTreeMap::new(),
        feature_graph: None,
        correspondence_graph: None,
        warnings: Vec::new(),
        enrichment_state: crate::contracts::ManifestEnrichmentState {
            status: EnrichmentStatus::None,
            proposals: Vec::new(),
        },
    }
}

async fn seed_target() -> (AppState, TestPathResolver) {
    seed_target_with_macro("Base Pot", "V-base", "base_macro()").await
}

async fn seed_ecky_verify_target(
    source: &str,
    model_id: &str,
    preview_name: &str,
    include_step_export: bool,
) -> (AppState, TestPathResolver) {
    let (state, resolver) = seed_target_with_macro("Verify Target", "V-verify", source).await;
    let preview_stl_path = resolver.root.join(preview_name);
    write_closed_tetra_binary_stl(&preview_stl_path);
    let source_path = resolver.root.join(format!("{model_id}.ecky"));
    fs::write(&source_path, source).expect("write ecky source");

    let mut design = sample_design("Verify Target", "V-verify", source);
    design.macro_dialect = MacroDialect::EckyIrV0;
    design.engine_kind = crate::models::EngineKind::EckyIrV0;
    design.geometry_backend = crate::models::GeometryBackend::EckyRust;
    design.source_language = crate::models::SourceLanguage::EckyIrV0;
    design.post_processing = None;

    let mut bundle = sample_bundle(model_id, preview_name);
    bundle.engine_kind = crate::models::EngineKind::EckyIrV0;
    bundle.geometry_backend = crate::models::GeometryBackend::EckyRust;
    bundle.source_language = crate::models::SourceLanguage::EckyIrV0;
    bundle.content_hash = format!("verify-{model_id}");
    bundle.macro_path = Some(source_path.display().to_string());
    bundle.preview_stl_path = preview_stl_path.display().to_string();
    if include_step_export {
        bundle.export_artifacts.push(crate::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: format!("/tmp/{model_id}.step"),
            role: "cad-exchange".to_string(),
        });
    }

    let mut manifest = sample_manifest(model_id);
    manifest.engine_kind = crate::models::EngineKind::EckyIrV0;
    manifest.geometry_backend = crate::models::GeometryBackend::EckyRust;
    manifest.source_language = crate::models::SourceLanguage::EckyIrV0;
    manifest.source_digest = Some(crate::mcp::macro_buffer::source_digest(source));

    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");
    {
        let conn = state.db.lock().await;
        conn.execute(
                "UPDATE messages SET output = ?1, artifact_bundle = ?2, model_manifest = ?3 WHERE id = 'msg-1'",
                rusqlite::params![
                    serde_json::to_string(&design).expect("design json"),
                    serde_json::to_string(&bundle).expect("bundle json"),
                    serde_json::to_string(&manifest).expect("manifest json"),
                ],
            )
            .expect("update verify target");
    }

    (state, resolver)
}

#[test]
fn carry_forward_semantic_manifest_keeps_controls_and_face_bindings() {
    let mut previous = sample_manifest("model-base");
    previous.selection_targets[1].parameter_keys = vec!["diameter".to_string()];
    previous.selection_targets[1].primitive_ids = vec!["diameter".to_string()];
    previous.selection_targets[1].view_ids = vec!["main".to_string()];

    let mut next = sample_manifest("model-next");
    next.control_primitives.clear();
    next.control_relations.clear();
    next.control_views.clear();
    next.selection_targets[1].parameter_keys.clear();
    next.selection_targets[1].primitive_ids.clear();
    next.selection_targets[1].view_ids.clear();
    let mut bundle = sample_bundle("model-next", "next.stl");
    bundle.edge_targets.push(crate::models::ViewerEdgeTarget {
        target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
        durable_target_id: None,
        canonical_target_id: None,
        alias_ids: Vec::new(),
        part_id: "body".to_string(),
        viewer_node_id: "body".to_string(),
        label: "Body.Edge1".to_string(),
        editable: true,
        start: crate::models::ViewerEdgePoint {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        end: crate::models::ViewerEdgePoint {
            x: 10.0,
            y: 0.0,
            z: 0.0,
        },
    });
    bundle.face_targets.push(crate::models::ViewerFaceTarget {
        target_id: "body:face:0:5-5-5:100".to_string(),
        durable_target_id: None,
        canonical_target_id: None,
        alias_ids: Vec::new(),
        part_id: "body".to_string(),
        viewer_node_id: "body".to_string(),
        label: "Body.Face1".to_string(),
        editable: true,
        center: crate::models::ViewerEdgePoint {
            x: 5.0,
            y: 5.0,
            z: 5.0,
        },
        normal: Some([0.0, 0.0, 1.0]),
        area: Some(100.0),
    });

    let merged = carry_forward_semantic_manifest(Some(&previous), next, &bundle);

    assert_eq!(merged.control_primitives.len(), 2);
    assert_eq!(merged.control_views.len(), 1);
    assert_eq!(
        merged.selection_targets[1].parameter_keys,
        vec!["diameter".to_string()]
    );
    assert_eq!(
        merged.selection_targets[1].primitive_ids,
        vec!["diameter".to_string()]
    );
    assert_eq!(
        merged.selection_targets[1].view_ids,
        vec!["main".to_string()]
    );
    assert!(merged.warnings.is_empty());
}

#[test]
fn carry_forward_semantic_manifest_ignores_broad_target_bindings() {
    let mut previous = sample_manifest("model-base");
    previous.selection_targets[1].parameter_keys = vec![
        "diameter".to_string(),
        "height".to_string(),
        "clearance".to_string(),
    ];

    let mut next = sample_manifest("model-next");
    next.selection_targets[1].parameter_keys.clear();
    let mut bundle = sample_bundle("model-next", "next.stl");
    bundle.edge_targets.push(crate::models::ViewerEdgeTarget {
        target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
        durable_target_id: None,
        canonical_target_id: None,
        alias_ids: Vec::new(),
        part_id: "body".to_string(),
        viewer_node_id: "body".to_string(),
        label: "Body.Edge1".to_string(),
        editable: true,
        start: crate::models::ViewerEdgePoint {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        end: crate::models::ViewerEdgePoint {
            x: 10.0,
            y: 0.0,
            z: 0.0,
        },
    });
    bundle.face_targets.push(crate::models::ViewerFaceTarget {
        target_id: "body:face:0:5-5-5:100".to_string(),
        durable_target_id: None,
        canonical_target_id: None,
        alias_ids: Vec::new(),
        part_id: "body".to_string(),
        viewer_node_id: "body".to_string(),
        label: "Body.Face1".to_string(),
        editable: true,
        center: crate::models::ViewerEdgePoint {
            x: 5.0,
            y: 5.0,
            z: 5.0,
        },
        normal: Some([0.0, 0.0, 1.0]),
        area: Some(100.0),
    });

    let merged = carry_forward_semantic_manifest(Some(&previous), next, &bundle);

    assert_eq!(merged.control_primitives.len(), 2);
    assert!(merged.selection_targets[1].parameter_keys.is_empty());
}

async fn seed_target_with_macro(
    title: &str,
    version_name: &str,
    macro_code: &str,
) -> (AppState, TestPathResolver) {
    let root = std::env::temp_dir().join(format!("ecky-mcp-root-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let conn = crate::db::init_db(&test_db_path("target-read")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    let resolver = TestPathResolver { root };
    let now = now_secs();

    let mut base_bundle = sample_bundle("model-base", "base.stl");
    base_bundle
        .export_artifacts
        .push(crate::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: "/tmp/model-base.step".to_string(),
            role: "cad-exchange".to_string(),
        });
    base_bundle
        .edge_targets
        .push(crate::models::ViewerEdgeTarget {
            target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Edge1".to_string(),
            editable: true,
            start: crate::models::ViewerEdgePoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            end: crate::models::ViewerEdgePoint {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            },
        });
    base_bundle
        .face_targets
        .push(crate::models::ViewerFaceTarget {
            target_id: "body:face:0:5-5-5:100".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.Face1".to_string(),
            editable: true,
            center: crate::models::ViewerEdgePoint {
                x: 5.0,
                y: 5.0,
                z: 5.0,
            },
            normal: Some([0.0, 0.0, 1.0]),
            area: Some(100.0),
        });
    let base_manifest = sample_manifest("model-base");
    let mut base_design = sample_design(title, version_name, macro_code);
    if macro_code.trim_start().starts_with("(model") {
        base_design.macro_dialect = MacroDialect::EckyIrV0;
        base_design.engine_kind = crate::models::EngineKind::EckyIrV0;
        base_design.geometry_backend = crate::models::GeometryBackend::EckyRust;
        base_design.source_language = crate::models::SourceLanguage::EckyIrV0;
    }

    {
        let conn = state.db.lock().await;
        db::create_or_update_thread(&conn, "thread-1", "Thread", now, None).unwrap();
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "msg-1".to_string(),
                role: MessageRole::Assistant,
                content: "Base version".to_string(),
                status: MessageStatus::Success,
                output: Some(base_design),
                usage: None,
                artifact_bundle: Some(base_bundle),
                model_manifest: Some(base_manifest),
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now,
            },
        )
        .unwrap();
    }

    (state, resolver)
}

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn health_check_includes_runtime_capabilities() {
    let _guard = crate::build123d_test_env_lock().lock().unwrap();
    let root = std::env::temp_dir().join(format!("ecky-mcp-health-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let python = root.join("bin").join("python3");
    write_executable(&python, "#!/bin/sh\nprintf '%s\\n' \"$0\"\nexit 0\n");
    std::env::set_var("BUILD123D_PYTHON", &python);

    let conn = crate::db::init_db(&test_db_path("health-check")).expect("db");
    let mut config = test_config();
    config.freecad_cmd = "/missing/freecadcmd".to_string();
    let state = AppState::new(config, None, conn);
    let resolver = TestPathResolver { root };

    let response = handle_health_check(&state, &resolver)
        .await
        .expect("health check");

    std::env::remove_var("BUILD123D_PYTHON");

    assert!(response.db_ready);
    assert!(!response.freecad_configured);
    assert!(!response.runtime_capabilities.freecad.available);
    assert!(response.runtime_capabilities.build123d.available);
    assert_eq!(
        response
            .runtime_capabilities
            .recommended_authoring_context
            .geometry_backend,
        crate::models::GeometryBackend::Build123d
    );
}

async fn seed_live_session(state: &AppState) {
    state.mcp_sessions.lock().await.insert(
        test_session_id(),
        crate::models::McpSessionState {
            client_kind: "mcp-http".to_string(),
            host_label: "Claude Code".to_string(),
            agent_label: "claude".to_string(),
            llm_model_id: None,
            llm_model_label: Some("Claude Sonnet".to_string()),
            bound_thread_id: None,
            last_target: Some(session_target_ref(
                "thread-1".to_string(),
                "msg-1".to_string(),
                Some("model-base".to_string()),
            )),
            phase: Some("idle".to_string()),
            status_text: Some("Agent joined the workspace.".to_string()),
            busy: false,
            activity_label: None,
            activity_started_at: None,
            attention_kind: None,
            waiting_on_prompt: false,
            current_turn_id: None,
            current_turn_thread_id: None,
            current_turn_working_message_ids: Vec::new(),
            current_turn_working_version_message_id: None,
            updated_at: now_secs(),
        },
    );
}

#[tokio::test]
async fn thread_create_creates_blank_thread_and_binds_session() {
    let conn = crate::db::init_db(&test_db_path("thread-create")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    seed_live_session(&state).await;

    let response = handle_thread_create(
        &state,
        ThreadCreateRequest {
            identity: AgentIdentityOverride::default(),
            title: Some("Seven Petal Badge".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("thread create");

    assert_eq!(response.title, "Seven Petal Badge");

    let conn = state.db.lock().await;
    let thread = history::get_thread(&conn, &response.thread_id).expect("created thread");
    assert_eq!(thread.title, "Seven Petal Badge");
    assert_eq!(thread.version_count, 0);
    let stored_session = db::get_sessions_by_ids(&conn, &[test_session_id()])
        .expect("stored session")
        .into_iter()
        .next()
        .expect("session row");
    assert_eq!(
        stored_session.thread_id.as_deref(),
        Some(response.thread_id.as_str())
    );
    assert!(stored_session.message_id.is_none());
    drop(conn);

    let live_session = state
        .mcp_sessions
        .lock()
        .await
        .get(&test_session_id())
        .cloned()
        .expect("live session");
    assert_eq!(
        live_session.bound_thread_id.as_deref(),
        Some(response.thread_id.as_str())
    );
    assert!(live_session.last_target.is_none());
    assert!(!live_session.busy);
}

#[tokio::test]
async fn thread_borrow_switches_current_session_target_without_logout() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;
    {
        let conn = state.db.lock().await;
        db::create_or_update_thread(&conn, "thread-2", "Thread Two", now_secs(), None).unwrap();
    }

    let response = handle_thread_borrow(
        &state,
        ThreadBorrowRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-2".to_string()),
            message_id: None,
            model_id: None,
            steal_thread: false,
        },
        &test_ctx(),
    )
    .await
    .expect("borrow thread");

    assert_eq!(response.thread_id, "thread-2");
    assert_eq!(response.title, "Thread Two");
    assert_eq!(response.message_id, None);

    let live_session = state
        .mcp_sessions
        .lock()
        .await
        .get(&test_session_id())
        .cloned()
        .expect("live session");
    assert_eq!(live_session.bound_thread_id.as_deref(), Some("thread-2"));
    assert!(live_session.last_target.is_none());

    let conn = state.db.lock().await;
    let stored_session = db::get_sessions_by_ids(&conn, &[test_session_id()])
        .expect("stored session")
        .into_iter()
        .next()
        .expect("session row");
    assert_eq!(stored_session.thread_id.as_deref(), Some("thread-2"));
    assert!(stored_session.message_id.is_none());
}

#[tokio::test]
async fn thread_borrow_message_target_sets_last_target() {
    let (state, _resolver) = seed_target().await;
    state.mcp_sessions.lock().await.insert(
        test_session_id(),
        crate::models::McpSessionState::new("mcp-http".to_string(), "Claude Code".to_string()),
    );

    let response = handle_thread_borrow(
        &state,
        ThreadBorrowRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: None,
            message_id: Some("msg-1".to_string()),
            model_id: Some("model-base".to_string()),
            steal_thread: false,
        },
        &test_ctx(),
    )
    .await
    .expect("borrow message target");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.message_id.as_deref(), Some("msg-1"));
    assert_eq!(response.model_id.as_deref(), Some("model-base"));

    let live_session = state
        .mcp_sessions
        .lock()
        .await
        .get(&test_session_id())
        .cloned()
        .expect("live session");
    assert_eq!(live_session.bound_thread_id.as_deref(), Some("thread-1"));
    let last_target = live_session.last_target.expect("last target");
    assert_eq!(last_target.thread_id, "thread-1");
    assert_eq!(last_target.message_id, "msg-1");
    assert_eq!(last_target.model_id.as_deref(), Some("model-base"));
}

#[tokio::test]
async fn resolve_prompt_thread_context_returns_bound_thread_identity() {
    let (state, _resolver) = seed_target().await;

    let (thread_id, thread_title) = resolve_prompt_thread_context(
        &state,
        Some(&agent_dialogue::SessionThreadTarget {
            thread_id: "thread-1".to_string(),
            message_id: Some("msg-1".to_string()),
            model_id: Some("model-base".to_string()),
        }),
    )
    .await
    .expect("prompt thread context");

    assert_eq!(thread_id.as_deref(), Some("thread-1"));
    assert_eq!(thread_title.as_deref(), Some("Thread"));
}

#[tokio::test]
async fn request_user_prompt_target_does_not_fall_back_to_current_snapshot() {
    let (state, _resolver) = seed_target().await;
    {
        let mut snapshot = state.last_snapshot.lock().unwrap();
        *snapshot = Some(crate::models::LastDesignSnapshot {
            design: None,
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            artifact_bundle: Some(sample_bundle("model-base", "base.stl")),
            model_manifest: None,
            selected_part_id: None,
        });
    }

    let target = resolve_request_user_prompt_target(
        &state,
        &test_session_id(),
        &UserPromptRequest {
            request_id: None,
            message: Some("Hello".to_string()),
            timeout_secs: Some(30),
            thread_id: None,
            message_id: None,
            model_id: None,
        },
    )
    .await
    .expect("request target");

    assert_eq!(target, None);
}

#[test]
fn configured_prompt_timeout_prefers_request_override_and_config_default() {
    let conn = crate::db::init_db(&test_db_path("prompt-timeout-config")).expect("db");
    let mut config = test_config();
    config.mcp.prompt_timeout_secs = 1444;
    let state = AppState::new(config, None, conn);

    assert_eq!(configured_prompt_timeout_secs(&state, None), 1444);
    assert_eq!(configured_prompt_timeout_secs(&state, Some(45)), 45);
    assert_eq!(configured_prompt_timeout_secs(&state, Some(0)), 10);
    assert_eq!(configured_prompt_timeout_secs(&state, Some(999_999)), 1800);
}

#[tokio::test]
async fn request_user_prompt_allows_explicit_cross_thread_target() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;
    {
        let conn = state.db.lock().await;
        db::create_or_update_thread(&conn, "thread-2", "Thread 2", now_secs(), None).unwrap();
    }

    let target = resolve_request_user_prompt_target(
        &state,
        &test_session_id(),
        &UserPromptRequest {
            request_id: None,
            message: Some("Hello".to_string()),
            timeout_secs: Some(30),
            thread_id: Some("thread-2".to_string()),
            message_id: None,
            model_id: None,
        },
    )
    .await
    .expect("cross-thread prompt override should resolve");

    let target = target.expect("explicit target");
    assert_eq!(target.thread_id, "thread-2");
    assert_eq!(target.message_id, None);
}

#[tokio::test]
async fn target_meta_get_returns_lightweight_summary_without_heavy_fields() {
    let (state, resolver) = seed_target().await;
    let response = handle_target_meta_get(
        &state,
        &resolver,
        TargetMetaRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("target meta");

    assert_eq!(response.resolved_from, TargetResolvedFrom::Base);
    assert_eq!(response.model_id.as_deref(), Some("model-base"));
    assert_eq!(response.source_language, "legacyPython");
    assert_eq!(response.macro_dialect, "legacy");
    assert_eq!(response.geometry_backend, "freecad");
    assert!(!response.has_draft);
    assert!(response.has_artifact_bundle);
    assert!(response.has_runtime_manifest);
    assert_eq!(response.export_formats, vec!["step".to_string()]);
    assert!(response.has_step_export);
    assert_eq!(
        response.step_export_path.as_deref(),
        Some("/tmp/model-base.step")
    );
    assert_eq!(response.edge_target_count, 1);
    assert_eq!(response.face_target_count, 1);
    assert_eq!(response.ui_field_count, 3);
    assert_eq!(response.range_count, 1);
    assert_eq!(response.select_count, 1);
    assert_eq!(response.checkbox_count, 1);
    assert_eq!(response.parameter_count, 3);
    assert!(response.has_semantic_manifest);
    assert_eq!(response.control_primitive_count, 2);
    assert_eq!(response.control_relation_count, 1);
    assert_eq!(response.control_view_count, 1);
    assert_eq!(response.scene_packet.schema_version, 1);
    assert_eq!(response.scene_packet.active_lens.as_str(), "exact");
    assert_eq!(
        response
            .scene_packet
            .representations
            .iter()
            .map(|entry| (entry.kind.as_str(), entry.status.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("sketchIntent", "rebuildable"),
            ("meshDraft", "stale"),
            ("exactModel", "committed")
        ]
    );
    assert!(response
        .scene_packet
        .allowed_patch_targets
        .contains(&"macroBufferReplaceAndPreview".to_string()));
    assert_eq!(response.scene_packet.topology.edge_target_count, 1);
    assert_eq!(response.scene_packet.topology.face_target_count, 1);

    let value = serde_json::to_value(&response).unwrap();
    assert!(value.get("scenePacket").is_some());
    assert!(value.get("macroCode").is_none());
    assert!(value.get("artifactBundle").is_none());
    assert!(value.get("modelManifest").is_none());
    assert!(value.get("latestDraft").is_none());
    assert!(value.get("cadSdkSnippet").is_none());
}

#[tokio::test]
async fn target_meta_get_marks_ecky_source_as_ast_patchable() {
    let (state, resolver) =
        seed_target_with_macro("Ecky block", "V-ecky", "(model\n  (box :size 10))").await;
    let response = handle_target_meta_get(
        &state,
        &resolver,
        TargetMetaRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("target meta");

    assert_eq!(response.source_language, "ecky");
    assert!(response
        .scene_packet
        .allowed_patch_targets
        .contains(&"eckyAstReplaceAndRender".to_string()));
    assert_eq!(response.scene_packet.active_lens.as_str(), "exact");
}

#[tokio::test]
async fn managed_session_log_in_allows_no_bound_target() {
    let conn = crate::db::init_db(&test_db_path("managed-session-login-target")).expect("db");
    let mut config = test_config();
    config.connection_type = Some("mcp".to_string());
    config.mcp.mode = crate::contracts::McpMode::Active;
    config.mcp.primary_agent_id = Some("agent-1".to_string());
    config.mcp.auto_agents = vec![crate::contracts::AutoAgent {
        id: "agent-1".to_string(),
        label: "claude".to_string(),
        cmd: "claude".to_string(),
        model: None,
        args: Vec::new(),
        enabled: true,
        start_on_demand: true,
    }];
    let state = AppState::new(config, None, conn);
    crate::mcp::runtime::initialize_auto_agent_supervisors(state.clone());
    crate::mcp::runtime::bind_managed_http_session(
        &state,
        "agent-1",
        &test_session_id(),
        Some("Connected to Ecky.".to_string()),
    );
    state.mcp_sessions.lock().await.insert(
        test_session_id(),
        crate::models::McpSessionState::new("mcp-http".to_string(), "Claude Code".to_string()),
    );

    let response = handle_session_log_in(
        &state,
        SessionLoginRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: None,
            message_id: None,
            model_id: None,
            steal_thread: false,
        },
        &test_ctx(),
    )
    .await
    .expect("managed session should log in without a bound target");

    assert_eq!(response.thread_id, None);
    assert_eq!(response.message_id, None);
    assert_eq!(response.model_id, None);

    let conn = state.db.lock().await;
    let stored_session = db::get_sessions_by_ids(&conn, &[test_session_id()])
        .expect("stored session")
        .into_iter()
        .next()
        .expect("session row");
    assert_eq!(stored_session.thread_id, None);
    assert_eq!(stored_session.message_id, None);
    drop(conn);

    let live_session = state
        .mcp_sessions
        .lock()
        .await
        .get(&test_session_id())
        .cloned()
        .expect("live session");
    assert_eq!(live_session.bound_thread_id, None);
    assert!(live_session.last_target.is_none());
    assert!(!live_session.busy);
}

#[tokio::test]
async fn passive_session_log_in_allows_no_thread_target_without_snapshot_fallback() {
    let (state, _resolver) = seed_target().await;
    {
        let mut snapshot = state.last_snapshot.lock().unwrap();
        *snapshot = Some(crate::models::LastDesignSnapshot {
            design: None,
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            artifact_bundle: Some(sample_bundle("model-base", "base.stl")),
            model_manifest: None,
            selected_part_id: None,
        });
    }

    let response = handle_session_log_in(
        &state,
        SessionLoginRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: None,
            message_id: None,
            model_id: None,
            steal_thread: false,
        },
        &test_ctx(),
    )
    .await
    .expect("passive session log in should allow no thread target");

    assert_eq!(response.thread_id, None);
    assert_eq!(response.message_id, None);
    assert_eq!(response.model_id, None);

    let conn = state.db.lock().await;
    let stored_session = db::get_sessions_by_ids(&conn, &[test_session_id()])
        .expect("stored session")
        .into_iter()
        .next()
        .expect("session row");
    assert_eq!(stored_session.thread_id, None);
    assert_eq!(stored_session.message_id, None);
}

#[tokio::test]
async fn managed_session_log_in_keeps_runtime_thread_without_snapshot_message_fallback() {
    let (state, _resolver) = seed_target().await;
    let mut config = state.config.lock().unwrap().clone();
    config.connection_type = Some("mcp".to_string());
    config.mcp.mode = crate::contracts::McpMode::Active;
    config.mcp.primary_agent_id = Some("agent-1".to_string());
    config.mcp.auto_agents = vec![crate::contracts::AutoAgent {
        id: "agent-1".to_string(),
        label: "claude".to_string(),
        cmd: "claude".to_string(),
        model: None,
        args: Vec::new(),
        enabled: true,
        start_on_demand: true,
    }];
    {
        *state.config.lock().unwrap() = config;
    }
    crate::mcp::runtime::initialize_auto_agent_supervisors(state.clone());
    crate::mcp::runtime::bind_managed_http_session(
        &state,
        "agent-1",
        &test_session_id(),
        Some("Connected to Ecky.".to_string()),
    );
    crate::mcp::runtime::wake_auto_agent_by_label(&state, "claude", Some("thread-1".to_string()))
        .await
        .expect("wake should capture the thread-only target");
    {
        let mut snapshot = state.last_snapshot.lock().unwrap();
        *snapshot = Some(crate::models::LastDesignSnapshot {
            design: None,
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            artifact_bundle: Some(sample_bundle("model-base", "base.stl")),
            model_manifest: None,
            selected_part_id: None,
        });
    }

    let response = handle_session_log_in(
        &state,
        SessionLoginRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: None,
            message_id: None,
            model_id: None,
            steal_thread: false,
        },
        &test_ctx(),
    )
    .await
    .expect("managed session log in should bind from runtime thread");

    assert_eq!(response.thread_id.as_deref(), Some("thread-1"));
    assert_eq!(response.message_id, None);
    assert_eq!(response.model_id, None);
}

#[tokio::test]
async fn session_log_in_blocks_claimed_thread_without_steal() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;
    {
        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            &test_ctx(),
            Some("thread-1".to_string()),
            Some("msg-1".to_string()),
            Some("model-base".to_string()),
            "idle",
            "Agent joined the workspace.",
        )
        .unwrap();
    }

    let err = handle_session_log_in(
        &state,
        SessionLoginRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            model_id: Some("model-base".to_string()),
            steal_thread: false,
        },
        &test_ctx_other(),
    )
    .await
    .expect_err("claimed thread should require explicit steal");

    assert_eq!(err.code, AppErrorCode::Conflict);
    assert!(err.message.contains("stealThread"));
    assert!(err.message.contains("claude"));
}

#[tokio::test]
async fn session_log_in_with_steal_transfers_thread_claim() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;
    {
        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            &test_ctx(),
            Some("thread-1".to_string()),
            Some("msg-1".to_string()),
            Some("model-base".to_string()),
            "idle",
            "Agent joined the workspace.",
        )
        .unwrap();
    }

    state.mcp_sessions.lock().await.insert(
        test_session_id_other(),
        crate::models::McpSessionState::new("http".to_string(), "Codex".to_string()),
    );

    let response = handle_session_log_in(
        &state,
        SessionLoginRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            model_id: Some("model-base".to_string()),
            steal_thread: true,
        },
        &test_ctx_other(),
    )
    .await
    .expect("steal should transfer thread claim");

    assert_eq!(response.thread_id.as_deref(), Some("thread-1"));
    let sessions = state.mcp_sessions.lock().await;
    let prior_owner = sessions.get(&test_session_id()).expect("prior owner");
    assert_eq!(prior_owner.bound_thread_id, None);
    assert!(prior_owner.last_target.is_none());
    let new_owner = sessions.get(&test_session_id_other()).expect("new owner");
    assert_eq!(new_owner.bound_thread_id.as_deref(), Some("thread-1"));
    drop(sessions);

    let conn = state.db.lock().await;
    let stored = db::get_sessions_by_ids(&conn, &[test_session_id(), test_session_id_other()])
        .expect("stored sessions");
    let old_row = stored
        .iter()
        .find(|session| session.session_id == test_session_id())
        .expect("old row");
    let new_row = stored
        .iter()
        .find(|session| session.session_id == test_session_id_other())
        .expect("new row");
    assert_eq!(old_row.thread_id, None);
    assert_eq!(new_row.thread_id.as_deref(), Some("thread-1"));
}

#[tokio::test]
async fn session_resume_blocks_claimed_thread_without_explicit_steal_path() {
    let (state, _resolver) = seed_target().await;
    state.mcp_sessions.lock().await.insert(
        test_session_id_other(),
        crate::models::McpSessionState {
            client_kind: "mcp-http".to_string(),
            host_label: "Codex".to_string(),
            agent_label: "codex".to_string(),
            llm_model_id: None,
            llm_model_label: Some("GPT-5".to_string()),
            bound_thread_id: None,
            last_target: Some(session_target_ref(
                "thread-1".to_string(),
                "msg-1".to_string(),
                Some("model-base".to_string()),
            )),
            phase: Some("idle".to_string()),
            status_text: Some("Agent joined the workspace.".to_string()),
            busy: false,
            activity_label: None,
            activity_started_at: None,
            attention_kind: None,
            waiting_on_prompt: false,
            current_turn_id: None,
            current_turn_thread_id: None,
            current_turn_working_message_ids: Vec::new(),
            current_turn_working_version_message_id: None,
            updated_at: now_secs(),
        },
    );
    {
        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            &test_ctx(),
            Some("thread-1".to_string()),
            Some("msg-1".to_string()),
            Some("model-base".to_string()),
            "disconnected",
            "Agent left the workspace.",
        )
        .unwrap();
        persist_agent_session(
            &conn,
            &test_ctx_other(),
            Some("thread-1".to_string()),
            Some("msg-1".to_string()),
            Some("model-base".to_string()),
            "idle",
            "Agent joined the workspace.",
        )
        .unwrap();
    }

    let err = handle_session_resume(
        &state,
        SessionResumeRequest {
            identity: AgentIdentityOverride::default(),
        },
        &test_ctx(),
    )
    .await
    .expect_err("resume should not steal another live thread claim");

    assert_eq!(err.code, AppErrorCode::Conflict);
    assert!(err.message.contains("stealThread"));
    assert!(err.message.contains("codex"));
}

#[tokio::test]
async fn thread_list_and_get_surface_live_claim_owner() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;

    let list = handle_thread_list(&state).await.expect("thread list");
    assert_eq!(list.threads.len(), 1);

    let thread = handle_thread_get(
        &state,
        ThreadGetRequest {
            thread_id: "thread-1".to_string(),
        },
    )
    .await
    .expect("thread get");
    assert_eq!(
        thread
            .claim_owner
            .as_ref()
            .map(|owner| owner.agent_label.as_str()),
        Some("claude")
    );
}

#[tokio::test]
async fn concept_preview_save_stores_agent_image_without_selected_engine() {
    let conn = crate::db::init_db(&test_db_path("concept-preview-save")).expect("db");
    let state = AppState::new(test_config(), None, conn);
    seed_live_session(&state).await;
    {
        let conn = state.db.lock().await;
        db::create_or_update_thread(&conn, "thread-1", "Thread", now_secs(), None).unwrap();
    }

    let response = handle_concept_preview_save(
        &state,
        ConceptPreviewSaveRequest {
            image_data: "data:image/svg+xml;base64,PHN2Zy8+".to_string(),
            caption: "Agent sketch.".to_string(),
            thread_id: Some("thread-1".to_string()),
            message_id: None,
            identity: AgentIdentityOverride::default(),
        },
        &test_ctx(),
    )
    .await
    .expect("concept preview save");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.caption, "Agent sketch.");
    let messages = {
        let conn = state.db.lock().await;
        db::get_thread_messages(&conn, "thread-1").expect("messages")
    };
    let saved = messages
        .iter()
        .find(|message| message.id == response.message_id)
        .expect("saved concept preview");
    assert_eq!(saved.content, "Agent sketch.");
    assert_eq!(saved.role, MessageRole::Assistant);
    assert_eq!(
        saved.image_data.as_deref(),
        Some(response.image_data.as_str())
    );
    assert_eq!(saved.visual_kind, Some(MessageVisualKind::ConceptPreview));
    assert_eq!(saved.usage, None);
}

#[tokio::test]
async fn thread_list_and_meta_surface_pending_inbox_anchor() {
    let (state, _resolver) = seed_target().await;
    let now = now_secs();

    {
        let conn = state.db.lock().await;
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "assistant-pending-1".to_string(),
                role: MessageRole::Assistant,
                content: "Working on it".to_string(),
                status: MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now,
            },
        )
        .unwrap();
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "user-pending-1".to_string(),
                role: MessageRole::User,
                content: "first".to_string(),
                status: MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now,
            },
        )
        .unwrap();
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "user-pending-2".to_string(),
                role: MessageRole::User,
                content: "second".to_string(),
                status: MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now,
            },
        )
        .unwrap();
        db::set_thread_pending_confirm(&conn, "thread-1", Some("needs-review")).unwrap();
    }

    let list = handle_thread_list(&state).await.expect("thread list");
    assert_eq!(list.threads.len(), 1);
    let entry = &list.threads[0];
    assert_eq!(entry.pending_count, 1);
    assert_eq!(entry.queued_count, 2);
    assert_eq!(entry.pending_confirm.as_deref(), Some("needs-review"));
    assert_eq!(
        entry.latest_pending_message_id.as_deref(),
        Some("user-pending-2")
    );

    let meta = handle_thread_meta_get(
        &state,
        ThreadMetaRequest {
            thread_id: "thread-1".to_string(),
        },
    )
    .await
    .expect("thread meta");
    assert_eq!(meta.pending_count, 1);
    assert_eq!(meta.queued_count, 2);
    assert_eq!(meta.pending_confirm.as_deref(), Some("needs-review"));
    assert_eq!(
        meta.latest_pending_message_id.as_deref(),
        Some("user-pending-2")
    );
}

#[tokio::test]
async fn thread_get_rejects_deleted_thread_as_normal_mcp_thread() {
    let (state, _resolver) = seed_target().await;
    {
        let conn = state.db.lock().await;
        db::delete_thread(&conn, "thread-1").unwrap();
    }

    let list = handle_thread_list(&state).await.expect("thread list");
    assert!(list.threads.is_empty());

    let err = handle_thread_get(
        &state,
        ThreadGetRequest {
            thread_id: "thread-1".to_string(),
        },
    )
    .await
    .expect_err("deleted thread should not load through normal MCP thread_get");

    assert_eq!(err.code, AppErrorCode::NotFound);
}

#[tokio::test]
async fn session_log_in_rejects_deleted_thread_target() {
    let (state, _resolver) = seed_target().await;
    {
        let conn = state.db.lock().await;
        db::delete_thread(&conn, "thread-1").unwrap();
    }

    let err = handle_session_log_in(
        &state,
        SessionLoginRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: None,
            model_id: None,
            steal_thread: false,
        },
        &test_ctx(),
    )
    .await
    .expect_err("deleted thread should not accept normal MCP session claim");

    assert_eq!(err.code, AppErrorCode::NotFound);
}

#[tokio::test]
async fn thread_messages_get_compacts_content_and_keeps_payload_flags() {
    let (state, _resolver) = seed_target().await;
    let long_content = "connector ".repeat(40);
    {
        let conn = state.db.lock().await;
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "msg-2".to_string(),
                role: MessageRole::Assistant,
                content: long_content.clone(),
                status: MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: Some(sample_bundle("model-2", "preview.stl")),
                model_manifest: Some(sample_manifest("model-2")),
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now_secs() + 1,
            },
        )
        .unwrap();
    }

    let response = handle_thread_messages_get(
        &state,
        ThreadMessagesRequest {
            thread_id: "thread-1".to_string(),
            limit: Some(1),
            before: None,
            roles: None,
        },
    )
    .await
    .expect("thread messages");

    assert_eq!(response.messages.len(), 1);
    assert_eq!(response.messages[0].id, "msg-2");
    assert!(response.messages[0].content.len() < long_content.len());
    assert!(response.messages[0].content.ends_with('…'));
    assert!(response.messages[0].has_artifacts);
    assert!(response.messages[0].has_manifest);
}

#[tokio::test]
async fn target_macro_get_returns_active_macro_payload() {
    let (state, resolver) = seed_target().await;
    let response = handle_target_macro_get(
        &state,
        &resolver,
        TargetMacroRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            start_line: None,
            end_line: None,
        },
        &test_ctx(),
    )
    .await
    .expect("target macro");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.message_id, "msg-1");
    assert_eq!(response.title, "Base Pot");
    assert_eq!(response.version_name, "V-base");
    assert_eq!(response.resolved_from, TargetResolvedFrom::Base);
    assert_eq!(response.line_count, 1);
    assert_eq!(response.window_start_line, 1);
    assert_eq!(response.window_end_line, 1);
    assert!(!response.truncated);
    assert_eq!(response.lines[0].text, "base_macro()");
    assert_eq!(response.macro_dialect, MacroDialect::Legacy);
    let value = serde_json::to_value(&response).expect("target macro json");
    assert!(value.get("macroCode").is_none());
    let artifact_digest = response.artifact_digest.as_ref().expect("artifact digest");
    assert_eq!(artifact_digest.model_id, "model-base");
    assert!(artifact_digest.has_step_export);
    assert_eq!(
        artifact_digest.step_export_path.as_deref(),
        Some("/tmp/model-base.step")
    );
    assert!(response.post_processing.is_none());
    assert_eq!(response.authoring_context.source_language, "legacyPython");
    assert_eq!(response.authoring_context.macro_dialect, "legacy");
    assert_eq!(response.authoring_context.geometry_backend, "freecad");
    assert!(response
        .authoring_context
        .authoring_card
        .contains("Ecky authoring card"));
    assert!(response
        .authoring_context
        .guide_uris
        .iter()
        .any(|uri| uri == "ecky://guides/authoring-card"));
}

#[tokio::test]
async fn target_get_returns_artifact_digest_for_export_truth() {
    let (state, resolver) = seed_target().await;
    let response = handle_target_get(
        &state,
        &resolver,
        TargetGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("target get");

    let artifact_digest = response.artifact_digest.expect("artifact digest");
    assert_eq!(artifact_digest.model_id, "model-base");
    assert_eq!(artifact_digest.export_formats, vec!["step"]);
    assert!(artifact_digest.has_step_export);
    assert_eq!(
        artifact_digest.step_export_path.as_deref(),
        Some("/tmp/model-base.step")
    );
}

#[test]
fn artifact_bundle_digest_reports_topology_target_counts() {
    let mut bundle = sample_bundle("model-topology", "topology.stl");
    bundle.edge_targets.push(crate::models::ViewerEdgeTarget {
        target_id: "body:edge:0:0-0-0_10-0-0".to_string(),
        durable_target_id: None,
        canonical_target_id: None,
        alias_ids: Vec::new(),
        part_id: "body".to_string(),
        viewer_node_id: "body".to_string(),
        label: "Body.Edge1".to_string(),
        editable: true,
        start: crate::models::ViewerEdgePoint {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        end: crate::models::ViewerEdgePoint {
            x: 10.0,
            y: 0.0,
            z: 0.0,
        },
    });
    bundle.face_targets.push(crate::models::ViewerFaceTarget {
        target_id: "body:face:0:5-5-5:100".to_string(),
        durable_target_id: None,
        canonical_target_id: None,
        alias_ids: Vec::new(),
        part_id: "body".to_string(),
        viewer_node_id: "body".to_string(),
        label: "Body.Face1".to_string(),
        editable: true,
        center: crate::models::ViewerEdgePoint {
            x: 5.0,
            y: 5.0,
            z: 5.0,
        },
        normal: Some([0.0, 0.0, 1.0]),
        area: Some(100.0),
    });

    let digest = artifact_bundle_digest(&bundle);

    assert_eq!(digest.edge_target_count, 1);
    assert_eq!(digest.face_target_count, 1);
}

#[test]
fn render_mutation_responses_include_artifact_digest_for_export_truth() {
    let mut bundle = sample_bundle("model-render", "render.stl");
    bundle.export_artifacts.push(crate::models::ExportArtifact {
        label: "STEP".to_string(),
        format: "step".to_string(),
        path: "/tmp/model-render.step".to_string(),
        role: "cad-exchange".to_string(),
    });
    let digest = artifact_bundle_digest(&bundle);
    let manifest = sample_manifest("model-render");
    let design = sample_design("Render", "V-render", "render_macro()");
    let sv = crate::services::structural_verification::verify_structure(&bundle, &manifest);

    let macro_response = MacroReplaceResponse {
        thread_id: "thread-1".to_string(),
        message_id: "msg-render".to_string(),
        macro_code: design.macro_code.clone(),
        ui_spec: design.ui_spec.clone(),
        initial_params: design.initial_params.clone(),
        artifact_bundle: bundle.clone(),
        model_manifest: manifest.clone(),
        structural_verification: Some(sv.clone()),
        artifact_digest: digest.clone(),
    };
    let params_response = ParamsPatchResponse {
        thread_id: "thread-1".to_string(),
        message_id: "msg-render".to_string(),
        merged_params: design.initial_params.clone(),
        artifact_bundle: bundle.clone(),
        model_manifest: manifest.clone(),
        design_output: design.clone(),
        structural_verification: Some(sv.clone()),
        artifact_digest: digest.clone(),
    };
    let buffer_response = MacroBufferReplaceAndRenderResponse {
        thread_id: "thread-1".to_string(),
        message_id: "msg-render".to_string(),
        digest: "source-digest".to_string(),
        line_count: 1,
        macro_code: design.macro_code.clone(),
        ui_spec: design.ui_spec.clone(),
        initial_params: design.initial_params.clone(),
        artifact_bundle: bundle,
        model_manifest: manifest,
        structural_verification: Some(sv),
        artifact_digest: digest,
    };

    for value in [
        serde_json::to_value(macro_response).expect("macro response json"),
        serde_json::to_value(params_response).expect("params response json"),
        serde_json::to_value(buffer_response).expect("buffer response json"),
    ] {
        assert_eq!(value["artifactDigest"]["modelId"], "model-render");
        assert_eq!(value["artifactDigest"]["hasStepExport"], true);
        assert_eq!(
            value["artifactDigest"]["stepExportPath"],
            "/tmp/model-render.step"
        );
    }
}

#[tokio::test]
async fn macro_buffer_get_returns_artifact_digest_for_export_truth() {
    let (state, resolver) = seed_target().await;
    let response = handle_macro_buffer_get(
        &state,
        &resolver,
        MacroBufferGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            start_line: None,
            end_line: None,
        },
        &test_ctx(),
    )
    .await
    .expect("macro buffer");

    let artifact_digest = response.artifact_digest.as_ref().expect("artifact digest");
    assert_eq!(artifact_digest.model_id, "model-base");
    assert!(artifact_digest.has_step_export);
    assert_eq!(
        artifact_digest.step_export_path.as_deref(),
        Some("/tmp/model-base.step")
    );

    let value = serde_json::to_value(&response).expect("macro buffer json");
    assert!(value.get("macroCode").is_none());
    assert_eq!(value["lineCount"], 1);
    assert_eq!(value["windowStartLine"], 1);
    assert_eq!(value["windowEndLine"], 1);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["lines"][0]["text"], "base_macro()");
}

#[tokio::test]
async fn macro_buffer_get_returns_requested_window_without_full_source() {
    let (state, resolver) = seed_target_with_macro(
        "window",
        "V-window",
        &(1..=205)
            .map(|line| format!("line_{line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .await;

    let response = handle_macro_buffer_get(
        &state,
        &resolver,
        MacroBufferGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            start_line: Some(201),
            end_line: Some(205),
        },
        &test_ctx(),
    )
    .await
    .expect("macro buffer");

    let value = serde_json::to_value(&response).expect("macro buffer json");
    assert!(value.get("macroCode").is_none());
    assert_eq!(value["lineCount"], 205);
    assert_eq!(value["windowStartLine"], 201);
    assert_eq!(value["windowEndLine"], 205);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["lines"].as_array().expect("lines").len(), 5);
    assert_eq!(value["lines"][0]["text"], "line_201");
}

#[tokio::test]
async fn ecky_ast_get_requires_feature_toggle() {
    let (state, resolver) =
        seed_target_with_macro("Box", "V-ast", "(model (part body (box 1 2 3)))").await;

    let err = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: None,
            depth: None,
            max_nodes: None,
            include_source: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("feature toggle should gate AST tool");

    assert!(err.message.contains("mcp.eckyAstAuthoring=true"));
}

#[tokio::test]
async fn ecky_ast_get_returns_bounded_core_nodes_when_enabled() {
    let (state, resolver) =
        seed_target_with_macro("Box", "V-ast", "(model (part body (box 1 2 3)))").await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

    let response = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: None,
            depth: Some(1),
            max_nodes: Some(4),
            include_source: None,
        },
        &test_ctx(),
    )
    .await
    .expect("ast response");

    let value = serde_json::to_value(&response).expect("ast json");
    assert_eq!(
        value["sourceDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"),
        true
    );
    assert_eq!(value["rootPaths"][0], "/parts/body/root");
    let nodes = value["nodes"].as_array().expect("nodes");
    assert!(nodes.len() >= 1);
    let root_node = nodes
        .iter()
        .find(|node| node["path"] == "/parts/body/root")
        .expect("root node");
    assert!(root_node["digest"].as_str().unwrap().starts_with("sha256:"));
    assert!(root_node["stableNodeKey"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(root_node["sourceAddressable"], true);
    assert_eq!(root_node["editableOps"], serde_json::json!(["replace"]));
    assert!(root_node.get("nonEditableReason").is_none());
    assert!(root_node.get("source").is_none());
    assert!(value.get("macroCode").is_none());
}

async fn stable_key_for_path(path: &str, source: &str) -> String {
    let (state, resolver) = seed_target_with_macro("StableKey", "V-stable-key", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let response = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: Some(path.to_string()),
            depth: Some(0),
            max_nodes: Some(8),
            include_source: Some(false),
        },
        &test_ctx(),
    )
    .await
    .expect("ast response");
    response
        .nodes
        .first()
        .map(|node| node.stable_node_key.clone())
        .expect("stable node key")
}

#[tokio::test]
async fn given_unrelated_param_insert_when_ast_reloaded_then_unchanged_node_keeps_stable_key() {
    let path = "/params/width";
    let source_before =
        "(model (params (number width 12) (number height 8)) (part body (box width 2 3)))";
    let source_after = "(model (params (number depth 4) (number width 12) (number height 8)) (part body (box width 2 3)))";

    let key_before = stable_key_for_path(path, source_before).await;
    let key_after = stable_key_for_path(path, source_after).await;

    assert_eq!(key_before, key_after);
}

#[tokio::test]
async fn given_unrelated_param_reorder_when_ast_reloaded_then_unchanged_node_keeps_stable_key() {
    let path = "/params/width";
    let source_before =
            "(model (params (number width 12) (number height 8) (number depth 4)) (part body (box width 2 3)))";
    let source_after =
            "(model (params (number depth 4) (number height 8) (number width 12)) (part body (box width 2 3)))";

    let key_before = stable_key_for_path(path, source_before).await;
    let key_after = stable_key_for_path(path, source_after).await;

    assert_eq!(key_before, key_after);
}

#[tokio::test]
async fn given_numeric_change_elsewhere_when_ast_reloaded_then_unchanged_node_keeps_stable_key() {
    let path = "/params/width";
    let source_before =
        "(model (params (number width 12) (number height 8)) (part body (box width 2 3)))";
    let source_after =
        "(model (params (number width 12) (number height 9)) (part body (box width 2 3)))";

    let key_before = stable_key_for_path(path, source_before).await;
    let key_after = stable_key_for_path(path, source_after).await;

    assert_eq!(key_before, key_after);
}

#[tokio::test]
async fn given_whitespace_only_change_when_ast_reloaded_then_unchanged_node_keeps_stable_key() {
    let path = "/params/width";
    let source_before =
        "(model (params (number width 12) (number height 8)) (part body (box width 2 3)))";
    let source_after =
            "(model\n  (params   (number width 12)\n           (number height 8))\n  (part body (box width 2 3)))";

    let key_before = stable_key_for_path(path, source_before).await;
    let key_after = stable_key_for_path(path, source_after).await;

    assert_eq!(key_before, key_after);
}

#[tokio::test]
async fn given_ast_get_include_source_false_when_serialized_then_nodes_omit_source() {
    let (state, resolver) = seed_target_with_macro(
        "Params",
        "V-ast-source-off",
        "(model (params (number width 12)) (part body (box width 2 3)))",
    )
    .await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

    let response = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: Some("/params/width".to_string()),
            depth: Some(0),
            max_nodes: Some(4),
            include_source: Some(false),
        },
        &test_ctx(),
    )
    .await
    .expect("ast response");

    let value = serde_json::to_value(&response).expect("ast json");
    assert!(value["nodes"][0].get("source").is_none());
}

#[tokio::test]
async fn given_ast_get_include_source_true_when_param_path_then_exact_bounded_source_returns() {
    let (state, resolver) = seed_target_with_macro(
        "Params",
        "V-ast-source-on",
        "(model (params (number width 12)) (part body (box width 2 3)))",
    )
    .await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

    let response = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: Some("/params/width".to_string()),
            depth: Some(0),
            max_nodes: Some(4),
            include_source: Some(true),
        },
        &test_ctx(),
    )
    .await
    .expect("ast response");

    let value = serde_json::to_value(&response).expect("ast json");
    let source = &value["nodes"][0]["source"];
    assert_eq!(source["text"], "(number width 12)");
    assert_eq!(source["span"], value["nodes"][0]["span"]);
    assert_eq!(source["truncated"], false);
    assert_eq!(source["maxBytes"], 4096);
    assert_eq!(source["byteLen"], "(number width 12)".len());
}

#[test]
fn given_source_slice_exceeds_limit_when_bounded_then_text_truncates_with_metadata() {
    let source = format!("({})", "a".repeat(ECKY_AST_SOURCE_MAX_BYTES + 100));
    let slice = bounded_ecky_ast_source_slice(&source, (0, source.len())).expect("source slice");

    assert_eq!(slice.text.len(), ECKY_AST_SOURCE_MAX_BYTES);
    assert_eq!(slice.byte_len, source.len());
    assert_eq!(slice.max_bytes, ECKY_AST_SOURCE_MAX_BYTES);
    assert!(slice.truncated);
    assert_eq!(slice.span.start, 0);
    assert_eq!(slice.span.end, source.len() as u32);
}

#[tokio::test]
async fn given_lowered_if_child_path_when_ast_get_then_node_reports_not_source_addressable() {
    let (state, resolver) = seed_target_with_macro(
            "Conditional",
            "V-ast-if",
            "(model (params (toggle raised true)) (part body (if raised (sphere 10) (cylinder 10 20))))",
        )
        .await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

    let response = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: Some("/parts/body/root/if/condition".to_string()),
            depth: Some(0),
            max_nodes: Some(4),
            include_source: Some(true),
        },
        &test_ctx(),
    )
    .await
    .expect("ast response");

    let value = serde_json::to_value(&response).expect("ast json");
    let node = &value["nodes"][0];
    assert_eq!(node["path"], "/parts/body/root/if/condition");
    assert_eq!(node["sourceAddressable"], false);
    assert_eq!(node["editableOps"], serde_json::json!([]));
    assert!(node["stableNodeKey"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(node["nonEditableReason"]
        .as_str()
        .unwrap()
        .contains("not source-span addressable"));
    assert!(node.get("source").is_none());
}

#[tokio::test]
async fn given_ecky_params_when_ast_get_then_param_paths_are_visible() {
    let (state, resolver) = seed_target_with_macro(
        "Params",
        "V-ast-params",
        "(model (params (number width 12)) (part body (box width 2 3)))",
    )
    .await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

    let response = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: Some("/params/width".to_string()),
            depth: Some(0),
            max_nodes: Some(4),
            include_source: None,
        },
        &test_ctx(),
    )
    .await
    .expect("ast response");

    let value = serde_json::to_value(&response).expect("ast json");
    assert_eq!(value["rootPaths"][0], "/params/width");
    assert_eq!(value["nodes"][0]["path"], "/params/width");
    assert_eq!(value["nodes"][0]["kind"], "Param");
    assert!(value["nodes"][0]["digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
}

#[tokio::test]
async fn given_ecky_param_path_when_ecky_dependency_get_then_core_reference_paths_return() {
    let (state, resolver) = seed_target_with_macro(
        "Params",
        "V-deps",
        "(model (params (number width 12) (number height 6)) (part body (box width height 3)))",
    )
    .await;

    let response = handle_ecky_dependency_get(
        &state,
        &resolver,
        EckyDependencyGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: "/params/width".to_string(),
        },
        &test_ctx(),
    )
    .await
    .expect("dependency response");

    assert_eq!(response.path, "/params/width");
    assert_eq!(response.dependency_kind, "parameterReference");
    assert_eq!(response.reference_count, 1);
    assert_eq!(response.impacted_part_ids, vec!["body".to_string()]);
    assert_eq!(
        response.impact_labels,
        vec!["part-local".to_string(), "export-affecting".to_string()]
    );
    assert_eq!(
        response.dependent_source_paths,
        vec!["/parts/body/root/call/args/0".to_string()]
    );
}

#[tokio::test]
async fn given_unsupported_path_when_ecky_dependency_get_then_validation_names_supported_shape() {
    let (state, resolver) = seed_target_with_macro(
        "Params",
        "V-deps",
        "(model (params (number width 12)) (part body (box width 2 3)))",
    )
    .await;

    let err = handle_ecky_dependency_get(
        &state,
        &resolver,
        EckyDependencyGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: "/parts/body/root".to_string(),
        },
        &test_ctx(),
    )
    .await
    .expect_err("unsupported path should fail");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("/params/{key}"));
    assert!(err.message.contains("/targets/{targetId}"));
    assert!(err.message.contains("/parts/body/root"));
}

#[tokio::test]
async fn given_target_path_when_ecky_dependency_get_then_returns_feature_and_parameter_bindings() {
    let (state, resolver) = seed_target_with_macro(
        "Params",
        "V-deps-target",
        "(model (params (number lens_bore_d 42)) (part body (box lens_bore_d 2 3)))",
    )
    .await;

    let mut manifest = sample_manifest("model-base");
    manifest.source_language = crate::models::SourceLanguage::EckyIrV0;
    manifest.geometry_backend = crate::models::GeometryBackend::EckyRust;
    manifest.selection_targets[1].parameter_keys = vec!["lens_bore_d".to_string()];
    manifest.feature_graph = Some(crate::models::FeatureGraph {
        nodes: vec![crate::models::FeatureNode {
            feature_id: "lens_bore".to_string(),
            kind: "bore".to_string(),
            label: "Lens Bore".to_string(),
            source_ref: Some(crate::models::SourceRef {
                source_id: Some("source-main".to_string()),
                path: Some("/parts/body/root".to_string()),
                start_byte: None,
                end_byte: None,
            }),
            dependency_ids: Vec::new(),
            output_refs: vec![crate::models::FeatureOutputRef {
                feature_id: "lens_bore".to_string(),
                output_id: "carrier-bore".to_string(),
                target_ids: vec!["body:face:0:5-5-5:100".to_string()],
            }],
            ports: Vec::new(),
        }],
    });

    {
        let conn = state.db.lock().await;
        conn.execute(
            "UPDATE messages SET model_manifest = ?1 WHERE id = 'msg-1'",
            rusqlite::params![serde_json::to_string(&manifest).expect("manifest json")],
        )
        .expect("update manifest");
    }

    let response = handle_ecky_dependency_get(
        &state,
        &resolver,
        EckyDependencyGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: "/targets/body:face:0:5-5-5:100".to_string(),
        },
        &test_ctx(),
    )
    .await
    .expect("dependency response");

    assert_eq!(response.path, "/targets/body:face:0:5-5-5:100");
    assert_eq!(response.dependency_kind, "selectionTargetReference");
    assert_eq!(response.impacted_part_ids, vec!["body".to_string()]);
    assert_eq!(response.parameter_keys, vec!["lens_bore_d".to_string()]);
    assert_eq!(response.feature_ids, vec!["lens_bore".to_string()]);
    assert_eq!(
        response.target_ids,
        vec!["body:face:0:5-5-5:100".to_string()]
    );
    assert_eq!(
        response.dependent_source_paths,
        vec!["/parts/body/root".to_string()]
    );
}

#[tokio::test]
async fn given_single_target_with_one_feature_and_params_when_selector_resolve_then_exact() {
    let (state, resolver) = seed_target_with_macro(
        "Selector",
        "V-selector-exact",
        "(model (params (number lens_bore_d 42)) (part body (box lens_bore_d 2 3)))",
    )
    .await;

    let mut manifest = sample_manifest("model-base");
    manifest.selection_targets[1].parameter_keys = vec!["lens_bore_d".to_string()];
    manifest.selection_targets[1].primitive_ids = vec!["primitive-face-1".to_string()];
    manifest.selection_targets[1].durable_target_id = Some("durable-face-1".to_string());
    manifest.selection_targets[1].canonical_target_id = Some("canonical-face-1".to_string());
    manifest.feature_graph = Some(crate::models::FeatureGraph {
        nodes: vec![crate::models::FeatureNode {
            feature_id: "lens_bore".to_string(),
            kind: "bore".to_string(),
            label: "Lens Bore".to_string(),
            source_ref: Some(crate::models::SourceRef {
                source_id: Some("source-main".to_string()),
                path: Some("/parts/body/root".to_string()),
                start_byte: None,
                end_byte: None,
            }),
            dependency_ids: Vec::new(),
            output_refs: vec![crate::models::FeatureOutputRef {
                feature_id: "lens_bore".to_string(),
                output_id: "carrier-bore".to_string(),
                target_ids: vec!["body:face:0:5-5-5:100".to_string()],
            }],
            ports: Vec::new(),
        }],
    });

    {
        let conn = state.db.lock().await;
        conn.execute(
            "UPDATE messages SET model_manifest = ?1 WHERE id = 'msg-1'",
            rusqlite::params![serde_json::to_string(&manifest).expect("manifest json")],
        )
        .expect("update manifest");
    }

    let response = handle_ecky_selector_resolve(
        &state,
        &resolver,
        EckySelectorResolveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            target_id: "body:face:0:5-5-5:100".to_string(),
        },
        &test_ctx(),
    )
    .await
    .expect("selector response");

    assert_eq!(response.target_id, "body:face:0:5-5-5:100");
    assert_eq!(
        response.durable_target_id.as_deref(),
        Some("durable-face-1")
    );
    assert_eq!(
        response.canonical_target_id.as_deref(),
        Some("canonical-face-1")
    );
    assert_eq!(response.feature_ids, vec!["lens_bore".to_string()]);
    assert_eq!(response.parameter_keys, vec!["lens_bore_d".to_string()]);
    assert_eq!(
        response.provenance_candidates.feature_role.as_deref(),
        Some("face")
    );
    assert_eq!(
        response.provenance_candidates.operation_kinds,
        vec!["bore".to_string()]
    );
    assert_eq!(
        response.provenance_candidates.primitive_ids,
        vec!["primitive-face-1".to_string()]
    );
    assert_eq!(
        response.provenance_candidates.source_stable_node_keys.len(),
        1
    );
    assert!(!response.provenance_candidates.source_stable_node_keys[0]
        .trim()
        .is_empty());
    assert_eq!(response.confidence, EckySelectorResolveConfidence::Exact);
}

#[tokio::test]
async fn given_alias_collision_when_selector_resolve_then_ambiguous() {
    let (state, resolver) = seed_target_with_macro(
        "Selector",
        "V-selector-ambiguous",
        "(model (params (number width 12)) (part body (box width 2 3)))",
    )
    .await;

    let mut manifest = sample_manifest("model-base");
    manifest.selection_targets[0].alias_ids = vec!["shared-face".to_string()];
    manifest.selection_targets[1].alias_ids = vec!["shared-face".to_string()];
    manifest.selection_targets[0].parameter_keys = vec!["edge_param".to_string()];
    manifest.selection_targets[1].parameter_keys = vec!["face_param".to_string()];

    {
        let conn = state.db.lock().await;
        conn.execute(
            "UPDATE messages SET model_manifest = ?1 WHERE id = 'msg-1'",
            rusqlite::params![serde_json::to_string(&manifest).expect("manifest json")],
        )
        .expect("update manifest");
    }

    let response = handle_ecky_selector_resolve(
        &state,
        &resolver,
        EckySelectorResolveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            target_id: "shared-face".to_string(),
        },
        &test_ctx(),
    )
    .await
    .expect("selector response");

    assert_eq!(
        response.confidence,
        EckySelectorResolveConfidence::Ambiguous
    );
    assert_eq!(response.target_id, "shared-face");
    assert!(response.reason.contains("Alias collision"));
    assert_eq!(response.durable_target_id, None);
    assert_eq!(response.canonical_target_id, None);
    assert_eq!(
        response.parameter_keys,
        vec!["edge_param".to_string(), "face_param".to_string()]
    );
}

#[tokio::test]
async fn given_missing_target_when_selector_resolve_then_none() {
    let (state, resolver) = seed_target_with_macro(
        "Selector",
        "V-selector-none",
        "(model (params (number width 12)) (part body (box width 2 3)))",
    )
    .await;

    let response = handle_ecky_selector_resolve(
        &state,
        &resolver,
        EckySelectorResolveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            target_id: "missing-target".to_string(),
        },
        &test_ctx(),
    )
    .await
    .expect("selector response");

    assert_eq!(response.confidence, EckySelectorResolveConfidence::None);
    assert_eq!(response.target_id, "missing-target");
    assert!(response.reason.contains("No selection target matched"));
    assert!(response.feature_ids.is_empty());
    assert!(response.parameter_keys.is_empty());
    assert!(response.provenance_candidates.feature_role.is_none());
    assert!(response
        .provenance_candidates
        .source_stable_node_keys
        .is_empty());
    assert!(response.provenance_candidates.operation_kinds.is_empty());
    assert!(response.provenance_candidates.primitive_ids.is_empty());
}

#[tokio::test]
async fn given_provided_params_when_ecky_constraints_validate_then_reports_pass_fail_rows() {
    let (state, resolver) = seed_target_with_macro(
            "Constrained",
            "V-constraints",
            "(model (params (number width 12 :min 10 :max 20 :step 2) (select mount inner :options ((Inner inner) (Outer outer)))) (part body (box width 2 3)))",
        )
        .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: Some(BTreeMap::from([
                ("width".to_string(), ParamValue::Number(13.0)),
                ("mount".to_string(), ParamValue::String("outer".to_string())),
            ])),
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    assert_eq!(response.parameter_source, "provided");
    assert_eq!(response.pass_count, 1);
    assert_eq!(response.fail_count, 1);
    let width = response
        .rows
        .iter()
        .find(|row| row.path == "/params/width")
        .expect("width row");
    assert_eq!(width.status, "fail");
    assert_eq!(width.severity, "error");
    assert_eq!(width.raw_value, serde_json::json!(13.0));
    assert!(width.message.contains("step"));
    assert_eq!(width.involved_param_keys, vec!["width".to_string()]);
    assert_eq!(width.source_stable_node_keys.len(), 1);
    assert!(!width.source_stable_node_keys[0].trim().is_empty());
    let mount = response
        .rows
        .iter()
        .find(|row| row.path == "/params/mount")
        .expect("mount row");
    assert_eq!(mount.status, "pass");
    assert_eq!(mount.severity, "info");
    assert_eq!(mount.involved_param_keys, vec!["mount".to_string()]);
    assert_eq!(mount.source_stable_node_keys.len(), 1);
    assert!(!mount.source_stable_node_keys[0].trim().is_empty());
}

#[tokio::test]
async fn given_missing_params_when_ecky_constraints_validate_then_uses_core_defaults() {
    let (state, resolver) = seed_target_with_macro(
        "Defaults",
        "V-constraints-default",
        "(model (params (number width 12 :min 10 :max 20 :step 2)) (part body (box width 2 3)))",
    )
    .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    assert_eq!(response.parameter_source, "initialOrDefault");
    assert_eq!(response.pass_count, 1);
    assert_eq!(response.fail_count, 0);
    assert_eq!(response.rows[0].path, "/params/width");
    assert_eq!(response.rows[0].severity, "info");
    assert_eq!(response.rows[0].raw_value, serde_json::json!(12.0));
    assert_eq!(
        response.rows[0].involved_param_keys,
        vec!["width".to_string()]
    );
    assert_eq!(response.rows[0].source_stable_node_keys.len(), 1);
    assert!(!response.rows[0].source_stable_node_keys[0]
        .trim()
        .is_empty());
}

#[tokio::test]
async fn given_passing_relation_when_ecky_constraints_validate_then_relation_row_passes() {
    let (state, resolver) = seed_target_with_macro(
            "Relation pass",
            "V-relation-pass",
            "(model (params (number lens_bore_d 8) (number tunnel_aperture_h 10) :relations ((< lens_bore_d tunnel_aperture_h))) (part body (box lens_bore_d 2 3)))",
        )
        .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: Some(BTreeMap::from([
                ("lens_bore_d".to_string(), ParamValue::Number(8.0)),
                ("tunnel_aperture_h".to_string(), ParamValue::Number(10.0)),
            ])),
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    assert_eq!(response.pass_count, 3);
    assert_eq!(response.fail_count, 0);
    let relation = response
        .rows
        .iter()
        .find(|row| row.path == "/params/:relations/0")
        .expect("relation row");
    assert_eq!(relation.status, "pass");
    assert_eq!(relation.severity, "info");
    assert_eq!(
        relation.involved_param_keys,
        vec!["lens_bore_d".to_string(), "tunnel_aperture_h".to_string()]
    );
}

#[tokio::test]
async fn given_failing_relation_when_ecky_constraints_validate_then_relation_row_fails() {
    let (state, resolver) = seed_target_with_macro(
            "Relation fail",
            "V-relation-fail",
            "(model (params (number lens_bore_d 8) (number tunnel_aperture_h 10) :relations ((< lens_bore_d tunnel_aperture_h))) (part body (box lens_bore_d 2 3)))",
        )
        .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: Some(BTreeMap::from([
                ("lens_bore_d".to_string(), ParamValue::Number(12.0)),
                ("tunnel_aperture_h".to_string(), ParamValue::Number(10.0)),
            ])),
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    assert_eq!(response.pass_count, 2);
    assert_eq!(response.fail_count, 1);
    let relation = response
        .rows
        .iter()
        .find(|row| row.path == "/params/:relations/0")
        .expect("relation row");
    assert_eq!(relation.status, "fail");
    assert_eq!(relation.severity, "error");
    assert!(
        relation.message.contains("Relation < failed"),
        "{}",
        relation.message
    );
    assert_eq!(
        relation.involved_param_keys,
        vec!["lens_bore_d".to_string(), "tunnel_aperture_h".to_string()]
    );
}

#[tokio::test]
async fn given_relation_row_when_ecky_constraints_validate_then_row_includes_relation_metadata() {
    let (state, resolver) = seed_target_with_macro(
            "Relation metadata",
            "V-relation-metadata",
            "(model (params (number lens_bore_d 8) (number tunnel_aperture_h 10) :relations ((< lens_bore_d tunnel_aperture_h))) (part body (box lens_bore_d 2 3)))",
        )
        .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let relation = response
        .rows
        .iter()
        .find(|row| row.path == "/params/:relations/0")
        .expect("relation row");
    let value = serde_json::to_value(relation).expect("row json");

    assert_eq!(value["constraintId"], "relation_0");
    assert_eq!(value["label"], "Relation #1");
    assert_eq!(value["kind"], "relation");
    assert!(value["sourceStableNodeKey"]
        .as_str()
        .is_some_and(|text| !text.trim().is_empty()));
    assert_eq!(
        value["dependsOnParamKeys"],
        serde_json::json!(["lens_bore_d", "tunnel_aperture_h"])
    );
    assert!(value["affectsStableNodeKeys"]
        .as_array()
        .is_some_and(|arr| !arr.is_empty()));
}

#[tokio::test]
async fn given_repeated_anonymous_delta_when_ecky_constraints_validate_then_authoring_lint_suggests_holder_margin_x(
) {
    let (state, resolver) = seed_target_with_macro(
            "Anonymous delta lint",
            "V-anonymous-delta-lint",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) (+ holder_w 12) 3)))",
        )
        .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let value = serde_json::to_value(response).expect("response json");
    let lints = value["authoringLints"]
        .as_array()
        .expect("authoring lints array");
    assert!(
        !lints.is_empty(),
        "expected at least one authoring lint for repeated anonymous delta"
    );
    assert!(lints.iter().any(|lint| {
        lint["kind"] == "anonymousDelta"
            && lint["paramKey"] == "holder_w"
            && lint["delta"] == 12.0
            && lint["suggestedParamKey"] == "holder_margin_x"
    }));
}

#[tokio::test]
async fn given_single_anonymous_delta_when_ecky_constraints_validate_then_no_authoring_lint() {
    let (state, resolver) = seed_target_with_macro(
        "Anonymous delta no lint",
        "V-anonymous-delta-no-lint",
        "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) 8 3)))",
    )
    .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let value = serde_json::to_value(response).expect("response json");
    assert_eq!(value["authoringLints"], serde_json::json!([]));
}

#[tokio::test]
async fn given_repeated_anonymous_delta_when_preview_stored_then_draft_feedback_payload_includes_authoring_lints(
) {
    let (state, resolver) = seed_target_with_macro(
            "Anonymous delta draft feedback lint",
            "V-anonymous-delta-feedback-lint",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) (+ holder_w 12) 3)))",
        )
        .await;
    let ctx = test_ctx();

    let mut design_output = sample_design(
            "Anonymous delta draft feedback lint",
            "",
            "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) (+ holder_w 12) 3)))",
        );
    design_output.macro_dialect = MacroDialect::EckyIrV0;
    design_output.engine_kind = crate::models::EngineKind::EckyIrV0;
    design_output.geometry_backend = crate::models::GeometryBackend::EckyRust;
    design_output.source_language = crate::models::SourceLanguage::EckyIrV0;

    let preview = store_session_render_preview(
        &state,
        &resolver,
        &ctx,
        StoreSessionRenderPreviewRequest {
            thread_id: "thread-1".to_string(),
            base_message_id: Some("msg-1".to_string()),
            design_output: design_output.clone(),
            artifact_bundle: sample_bundle("model-feedback-lint", "feedback-lint.stl"),
            model_manifest: sample_manifest("model-feedback-lint"),
            draft_feedback: Some(DraftFeedbackSeed {
                status: crate::models::AgentDraftFeedbackStatus::Warning,
                summary: "Draft warnings.".to_string(),
                items: Vec::new(),
                authoring_lints: Vec::new(),
                source: crate::models::AgentDraftFeedbackSource::StructuralVerification,
            }),
        },
    )
    .await
    .expect("store preview");

    let event = crate::contracts::AgentDraftPreviewUpdatedEvent {
        session_id: preview.session_id.clone(),
        thread_id: preview.thread_id.clone(),
        preview_id: preview.preview_id.clone(),
        base_message_id: preview.base_message_id.clone(),
        model_id: Some(preview.artifact_bundle.model_id.clone()),
        design: preview.design_output.clone(),
        artifact_bundle: preview.artifact_bundle.clone(),
        model_manifest: preview.model_manifest.clone(),
        feedback: preview.draft_feedback.clone(),
    };
    let value = serde_json::to_value(&event).expect("event json");
    let lints = value["feedback"]["authoringLints"]
        .as_array()
        .expect("authoring lints array");

    assert!(lints.iter().any(|lint| {
        lint["kind"] == "anonymousDelta"
            && lint["paramKey"] == "holder_w"
            && lint["delta"] == 12.0
            && lint["suggestedParamKey"] == "holder_margin_x"
    }));
}

#[tokio::test]
async fn given_no_repeated_anonymous_delta_when_preview_stored_then_draft_feedback_payload_has_empty_authoring_lints(
) {
    let (state, resolver) = seed_target_with_macro(
        "Anonymous delta draft feedback no lint",
        "V-anonymous-delta-feedback-no-lint",
        "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) 8 3)))",
    )
    .await;
    let ctx = test_ctx();

    let mut design_output = sample_design(
        "Anonymous delta draft feedback no lint",
        "",
        "(model (params (number holder_w 40)) (part holder (box (+ holder_w 12) 8 3)))",
    );
    design_output.macro_dialect = MacroDialect::EckyIrV0;
    design_output.engine_kind = crate::models::EngineKind::EckyIrV0;
    design_output.geometry_backend = crate::models::GeometryBackend::EckyRust;
    design_output.source_language = crate::models::SourceLanguage::EckyIrV0;

    let preview = store_session_render_preview(
        &state,
        &resolver,
        &ctx,
        StoreSessionRenderPreviewRequest {
            thread_id: "thread-1".to_string(),
            base_message_id: Some("msg-1".to_string()),
            design_output: design_output.clone(),
            artifact_bundle: sample_bundle("model-feedback-no-lint", "feedback-no-lint.stl"),
            model_manifest: sample_manifest("model-feedback-no-lint"),
            draft_feedback: Some(DraftFeedbackSeed {
                status: crate::models::AgentDraftFeedbackStatus::Passed,
                summary: "Draft passed.".to_string(),
                items: Vec::new(),
                authoring_lints: Vec::new(),
                source: crate::models::AgentDraftFeedbackSource::StructuralVerification,
            }),
        },
    )
    .await
    .expect("store preview");

    let event = crate::contracts::AgentDraftPreviewUpdatedEvent {
        session_id: preview.session_id.clone(),
        thread_id: preview.thread_id.clone(),
        preview_id: preview.preview_id.clone(),
        base_message_id: preview.base_message_id.clone(),
        model_id: Some(preview.artifact_bundle.model_id.clone()),
        design: preview.design_output.clone(),
        artifact_bundle: preview.artifact_bundle.clone(),
        model_manifest: preview.model_manifest.clone(),
        feedback: preview.draft_feedback.clone(),
    };
    let value = serde_json::to_value(&event).expect("event json");
    assert_eq!(value["feedback"]["authoringLints"], serde_json::json!([]));
}

#[tokio::test]
async fn given_physical_decision_calibration_defaults_when_ecky_constraints_validate_then_relation_rows_pass(
) {
    let source =
        include_str!("../../../../model-runtime/examples/physical-decision-calibration.ecky");
    let (state, resolver) = seed_target_with_macro(
        "Physical Decision Calibration",
        "V-physical-decision-defaults",
        source,
    )
    .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let relation_rows = response
        .rows
        .iter()
        .filter(|row| row.path.starts_with("/params/:relations/"))
        .collect::<Vec<_>>();

    assert_eq!(response.parameter_source, "initialOrDefault");
    assert_eq!(response.fail_count, 0);
    assert_eq!(relation_rows.len(), 13);
    assert!(relation_rows.iter().all(|row| row.status == "pass"));
}

#[tokio::test]
async fn given_physical_decision_calibration_overrides_when_ecky_constraints_validate_then_relation_rows_fail_with_expected_involved_keys(
) {
    let source =
        include_str!("../../../../model-runtime/examples/physical-decision-calibration.ecky");
    let (state, resolver) = seed_target_with_macro(
        "Physical Decision Calibration",
        "V-physical-decision-overrides",
        source,
    )
    .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: Some(BTreeMap::from([
                ("thread_clearance".to_string(), ParamValue::Number(0.10)),
                ("lens_bore_d".to_string(), ParamValue::Number(58.70)),
            ])),
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let failing_relation_rows = response
        .rows
        .iter()
        .filter(|row| row.path.starts_with("/params/:relations/") && row.status == "fail")
        .collect::<Vec<_>>();

    assert_eq!(response.parameter_source, "provided");
    assert!(response.fail_count >= 2);

    let thread_clearance_row = failing_relation_rows
        .iter()
        .find(|row| {
            row.involved_param_keys
                .iter()
                .any(|key| key == "thread_clearance")
                && row
                    .involved_param_keys
                    .iter()
                    .any(|key| key == "thread_clearance_min")
        })
        .expect("thread clearance relation fail row");
    assert!(thread_clearance_row.message.contains("Relation >="));

    let lens_row = failing_relation_rows
        .iter()
        .find(|row| {
            row.involved_param_keys
                .iter()
                .any(|key| key == "lens_bore_d")
                && row
                    .involved_param_keys
                    .iter()
                    .any(|key| key == "lens_fit_floor")
        })
        .expect("lens relation fail row");
    assert!(lens_row.message.contains("Relation >="));
}

#[tokio::test]
async fn given_physical_decision_calibration_failure_when_ecky_constraints_validate_then_failing_relation_includes_source_handles(
) {
    let source =
        include_str!("../../../../model-runtime/examples/physical-decision-calibration.ecky");
    let (state, resolver) = seed_target_with_macro(
        "Physical Decision Calibration",
        "V-physical-decision-traceability",
        source,
    )
    .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: Some(BTreeMap::from([(
                "thread_clearance".to_string(),
                ParamValue::Number(0.10),
            )])),
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let failing_row = response
        .rows
        .iter()
        .find(|row| {
            row.path.starts_with("/params/:relations/")
                && row.status == "fail"
                && row
                    .involved_param_keys
                    .iter()
                    .any(|key| key == "thread_clearance")
                && row
                    .involved_param_keys
                    .iter()
                    .any(|key| key == "thread_clearance_min")
        })
        .expect("thread clearance failing relation row");

    for key in &failing_row.involved_param_keys {
        let param_row = response
            .rows
            .iter()
            .find(|row| row.path == format!("/params/{key}"))
            .expect("param row for failing key");
        assert!(!param_row.source_stable_node_keys.is_empty());
        assert!(param_row
            .source_stable_node_keys
            .iter()
            .all(|stable_key| !stable_key.trim().is_empty()));
    }
}

fn load_physical_decision_calibration_fail_fixture() -> String {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../model-runtime/examples/physical-decision-calibration-fail.ecky");
    fs::read_to_string(&fixture_path).unwrap_or_else(|_| {
        "(model
                (params
                    (number lens_bore_d 58.70)
                    (number lens_fit_floor 58.80)
                    (number thread_clearance 0.10)
                    (number thread_clearance_min 0.25)
                    :relations
                    (
                        (>= lens_bore_d lens_fit_floor)
                        (>= thread_clearance thread_clearance_min)
                    )
                )
                (part calibration (box 1 1 1))
            )"
        .to_string()
    })
}

#[tokio::test]
async fn given_physical_decision_fail_fixture_when_ecky_constraints_validate_then_multiple_relation_rows_fail(
) {
    let source = load_physical_decision_calibration_fail_fixture();
    let (state, resolver) = seed_target_with_macro(
        "Physical Decision Calibration Fail",
        "V-physical-decision-fail-relations",
        &source,
    )
    .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let failing_relation_rows = response
        .rows
        .iter()
        .filter(|row| row.path.starts_with("/params/:relations/") && row.status == "fail")
        .collect::<Vec<_>>();

    assert!(
        failing_relation_rows.len() >= 2,
        "expected >=2 failing relation rows, got {}",
        failing_relation_rows.len()
    );
    assert!(response.fail_count >= 2);
}

#[tokio::test]
async fn given_physical_decision_fail_fixture_when_ecky_constraints_validate_then_failing_rows_include_keys_and_source_traceability(
) {
    let source = load_physical_decision_calibration_fail_fixture();
    let (state, resolver) = seed_target_with_macro(
        "Physical Decision Calibration Fail",
        "V-physical-decision-fail-traceability",
        &source,
    )
    .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let failing_relation_rows = response
        .rows
        .iter()
        .filter(|row| row.path.starts_with("/params/:relations/") && row.status == "fail")
        .collect::<Vec<_>>();

    assert!(
        !failing_relation_rows.is_empty(),
        "expected failing relation rows"
    );

    for relation_row in failing_relation_rows {
        assert!(
            !relation_row.involved_param_keys.is_empty(),
            "missing involvedParamKeys for {}",
            relation_row.path
        );
        for key in &relation_row.involved_param_keys {
            let param_row = response
                .rows
                .iter()
                .find(|row| row.path == format!("/params/{key}"))
                .expect("param row for involved key");
            assert!(
                !param_row.source_stable_node_keys.is_empty(),
                "missing source_stable_node_keys for {}",
                param_row.path
            );
            assert!(param_row
                .source_stable_node_keys
                .iter()
                .all(|stable_key| !stable_key.trim().is_empty()));
        }
    }
}

#[test]
fn ecky_ast_replace_source_rewrites_spanned_node_with_digest_guards() {
    let source = "(model (part body (box 1 2 3)))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = "/parts/body/root";
    let node = find_core_ast_node_in_program(&program, &path).expect("node");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = core_node_digest(node);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Replace,
        Some("(box 4 5 6)"),
        None,
    )
    .expect("replace");

    assert_eq!(next, "(model (part body (box 4 5 6)))");
}

fn source_edit_digest(source: &str, path: &str) -> String {
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    edit_digest_for_ecky_path(&program, source, path).expect("path digest")
}

#[test]
fn given_param_path_when_replace_then_source_rewrites_param_decl() {
    let source = "(model (params (number width 12)) (part body (box width 2 3)))";
    let path = "/params/width";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Replace,
        Some("(number width 24)"),
        None,
    )
    .expect("replace param");

    assert_eq!(
        next,
        "(model (params (number width 24)) (part body (box width 2 3)))"
    );
}

#[test]
fn given_param_path_when_rename_then_decl_and_refs_update() {
    let source = "(model (params (number width 12)) (part body (box width 2 3)))";
    let path = "/params/width";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Rename,
        None,
        Some("height"),
    )
    .expect("rename param");

    assert_eq!(
        next,
        "(model (params (number height 12)) (part body (box height 2 3)))"
    );
}

#[test]
fn given_part_path_when_rename_then_part_name_updates() {
    let source = "(model (part body (box 1 2 3)) (part cap (sphere 2)))";
    let path = "/parts/cap";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Rename,
        None,
        Some("panel"),
    )
    .expect("rename part");

    assert_eq!(
        next,
        "(model (part body (box 1 2 3)) (part panel (sphere 2)))"
    );
}

#[test]
fn given_ast_arg_path_when_insert_after_then_source_adds_sibling() {
    let source = "(model (part body (union (box 1 2 3) (sphere 4))))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = "/parts/body/root/call/args/1";
    let node = find_core_ast_node_in_program(&program, &path).expect("node");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = core_node_digest(node);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::InsertAfter,
        Some("(cylinder 2 8)"),
        None,
    )
    .expect("insert");

    assert_eq!(
        next,
        "(model (part body (union (box 1 2 3) (sphere 4) (cylinder 2 8))))"
    );
}

#[test]
fn given_ast_arg_path_when_delete_then_source_removes_sibling() {
    let source = "(model (part body (union (box 1 2 3) (sphere 4))))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = "/parts/body/root/call/args/1";
    let node = find_core_ast_node_in_program(&program, &path).expect("node");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = core_node_digest(node);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Delete,
        None,
        None,
    )
    .expect("delete");

    assert_eq!(next, "(model (part body (union (box 1 2 3))))");
}

#[test]
fn given_ast_keyword_path_when_delete_then_source_removes_keyword_pair() {
    let source = "(model (part body (fillet 2 :edges \"top\" (box 1 2 3))))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = "/parts/body/root/call/keywords/edges";
    let node = find_core_ast_node_in_program(&program, &path).expect("node");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = core_node_digest(node);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Delete,
        None,
        None,
    )
    .expect("delete keyword");

    assert_eq!(next, "(model (part body (fillet 2 (box 1 2 3))))");
}

#[test]
fn given_param_path_when_insert_after_then_source_adds_param_sibling() {
    let source = "(model (params (number width 12)) (part body (box width height 3)))";
    let path = "/params/width";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::InsertAfter,
        Some("(number height 6)"),
        None,
    )
    .expect("insert param");

    assert_eq!(
        next,
        "(model (params (number width 12) (number height 6)) (part body (box width height 3)))"
    );
}

#[test]
fn given_part_path_when_insert_after_then_source_adds_part_sibling() {
    let source = "(model (part body (box 1 2 3)))";
    let path = "/parts/body";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::InsertAfter,
        Some("(part lid (sphere 2))"),
        None,
    )
    .expect("insert part");

    assert_eq!(
        next,
        "(model (part body (box 1 2 3)) (part lid (sphere 2)))"
    );
}

#[test]
fn given_part_path_when_delete_then_source_removes_part_clause() {
    let source = "(model (part body (box 1 2 3)) (part lid (sphere 2)))";
    let path = "/parts/lid";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Delete,
        None,
        None,
    )
    .expect("delete part");

    assert_eq!(next, "(model (part body (box 1 2 3)))");
}

#[test]
fn given_build_binding_path_when_replace_then_source_rewrites_shape_value() {
    let source = "(model (part body (build (shape rail (box 1 2 3)) (result rail))))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = "/parts/body/root/build/bindings/rail";
    let node = find_core_ast_node_in_program(&program, path).expect("node");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = core_node_digest(node);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Replace,
        Some("(cylinder 2 8)"),
        None,
    )
    .expect("replace build binding");

    assert_eq!(
        next,
        "(model (part body (build (shape rail (cylinder 2 8)) (result rail))))"
    );
}

#[test]
fn given_build_binding_path_when_insert_after_then_source_adds_shape_sibling() {
    let source = "(model (part body (build (shape rail (box 1 2 3)) (result rail))))";
    let path = "/parts/body/root/build/bindings/rail";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::InsertAfter,
        Some("(shape cap (translate 0 0 1 rail))"),
        None,
    )
    .expect("insert build shape");

    assert_eq!(
            next,
            "(model (part body (build (shape rail (box 1 2 3)) (shape cap (translate 0 0 1 rail)) (result rail))))"
        );
}

#[test]
fn given_build_binding_path_when_delete_then_source_removes_shape_clause() {
    let source =
        "(model (part body (build (shape rail (box 1 2 3)) (shape cap (sphere 2)) (result cap))))";
    let path = "/parts/body/root/build/bindings/rail";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Delete,
        None,
        None,
    )
    .expect("delete build shape");

    assert_eq!(
        next,
        "(model (part body (build (shape cap (sphere 2)) (result cap))))"
    );
}

#[test]
fn given_let_binding_path_when_replace_then_source_rewrites_binding_value() {
    let source = "(model (part body (let ((lift 3)) (translate 0 0 lift (box 1 2 3)))))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = core_node_child_paths(&program.parts[0].root, "/parts/body/root")
        .into_iter()
        .find_map(|(path, _)| path.contains("/let/bindings/").then_some(path))
        .expect("let binding path");
    let node = find_core_ast_node_in_program(&program, path.as_str()).expect("node");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = core_node_digest(node);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        &path,
        &node_digest,
        &EckyAstEditOperation::Replace,
        Some("6"),
        None,
    )
    .expect("replace let binding");

    assert_eq!(
        next,
        "(model (part body (let ((lift 6)) (translate 0 0 lift (box 1 2 3)))))"
    );
}

#[test]
fn given_let_binding_path_when_insert_after_then_source_adds_binding_sibling() {
    let source = "(model (part body (let ((lift 3)) (translate 0 0 lift (box 1 2 3)))))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = core_node_child_paths(&program.parts[0].root, "/parts/body/root")
        .into_iter()
        .find_map(|(path, _)| path.contains("/let/bindings/").then_some(path))
        .expect("let binding path");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, &path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        &path,
        &node_digest,
        &EckyAstEditOperation::InsertAfter,
        Some("(drop 4)"),
        None,
    )
    .expect("insert let binding");

    assert_eq!(
        next,
        "(model (part body (let ((lift 3) (drop 4)) (translate 0 0 lift (box 1 2 3)))))"
    );
}

#[test]
fn given_let_binding_path_when_delete_then_source_removes_binding_pair() {
    let source = "(model (part body (let ((lift 3) (drop 4)) (translate 0 0 drop (box 1 2 3)))))";
    let path = "/parts/body/root/let/bindings/lift";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Delete,
        None,
        None,
    )
    .expect("delete let binding");

    assert_eq!(
        next,
        "(model (part body (let ((drop 4)) (translate 0 0 drop (box 1 2 3)))))"
    );
}

#[test]
fn given_build_binding_path_when_rename_then_refs_update_in_scope() {
    let source = "(model (part body (build (shape rail (box 1 2 3)) (shape cap (translate 0 0 1 rail)) (result cap))))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = "/parts/body/root/build/bindings/rail";
    let node = find_core_ast_node_in_program(&program, path).expect("node");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = core_node_digest(node);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        path,
        &node_digest,
        &EckyAstEditOperation::Rename,
        None,
        Some("spine"),
    )
    .expect("rename build binding");

    assert_eq!(
            next,
            "(model (part body (build (shape spine (box 1 2 3)) (shape cap (translate 0 0 1 spine)) (result cap))))"
        );
}

#[test]
fn given_let_binding_path_when_rename_then_body_refs_update_not_binding_value() {
    let source = "(model (part body (let ((lift height)) (translate 0 0 lift (box 1 2 3)))))";
    let program = crate::ecky_scheme::compile_to_core_program(source).expect("compile");
    let path = core_node_child_paths(&program.parts[0].root, "/parts/body/root")
        .into_iter()
        .find_map(|(path, _)| path.contains("/let/bindings/").then_some(path))
        .expect("let binding path");
    let node = find_core_ast_node_in_program(&program, path.as_str()).expect("node");
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = core_node_digest(node);

    let next = replace_ecky_ast_source(
        source,
        &source_digest,
        &path,
        &node_digest,
        &EckyAstEditOperation::Rename,
        None,
        Some("zlift"),
    )
    .expect("rename let binding");

    assert_eq!(
        next,
        "(model (part body (let ((zlift height)) (translate 0 0 zlift (box 1 2 3)))))"
    );
}

#[test]
fn ecky_ast_replace_source_rejects_stale_node_digest() {
    let source = "(model (part body (box 1 2 3)))";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);

    let err = replace_ecky_ast_source(
        source,
        &source_digest,
        "/parts/body/root",
        "sha256:not-current",
        &EckyAstEditOperation::Replace,
        Some("(box 4 5 6)"),
        None,
    )
    .expect_err("stale node digest should fail");

    assert!(err.message.contains("node digest mismatch"));
}

#[tokio::test]
async fn given_valid_replace_when_ecky_ast_patch_validate_then_structured_diff_returns_without_render_payload(
) {
    let source = "(model (part body (box 1 2 3)))";
    let (state, resolver) = seed_target_with_macro("Box", "V-validate", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/parts/body/root";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let node_digest = source_edit_digest(source, path);

    let response = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest,
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: node_digest.clone(),
            replacement_source: Some("(box 4 5 6)".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("patch validate");

    let value = serde_json::to_value(&response).expect("validate json");
    assert_eq!(value["operation"], "replace");
    assert_eq!(value["editedPath"], path);
    assert_eq!(value["status"], "valid");
    assert_eq!(value["oldNodeDigest"], node_digest);
    assert!(value["newNodeDigest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_ne!(value["sourceDigest"], value["newSourceDigest"]);
    assert_eq!(value["affectedPaths"], serde_json::json!([path]));
    assert_eq!(
        value["affectedPathDetails"],
        serde_json::json!([{
            "change": "replace",
            "oldPath": path,
            "newPath": path,
            "oldDigest": value["oldNodeDigest"].clone(),
            "newDigest": value["newNodeDigest"].clone(),
        }])
    );
    assert_eq!(
        value["affectedNodeKeys"].as_array().map(|v| v.len()),
        Some(2)
    );
    assert!(value["affectedNodeKeys"][0]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(value["affectedNodeKeys"][1]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(
        value["dependencyImpact"]["dependencyKind"],
        serde_json::json!("pathLocal")
    );
    assert_eq!(
        value["dependencyImpact"]["impactedPartIds"],
        serde_json::json!(["body"])
    );
    assert_eq!(
        value["dependencyImpact"]["impactLabels"],
        serde_json::json!(["part-local", "export-affecting"])
    );
    assert!(value["diff"]["old"]["byteLen"].as_u64().unwrap_or(0) > 0);
    assert!(value["diff"]["new"]["byteLen"].as_u64().unwrap_or(0) > 0);
    assert!(value["diff"]["old"]["digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(value["diff"]["new"]["digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(value.get("macroCode").is_none());
    assert!(value.get("artifactBundle").is_none());
    assert!(value.get("modelManifest").is_none());
    assert!(value.get("artifactDigest").is_none());
    assert!(value.get("draft").is_none());
}

#[tokio::test]
async fn given_stable_node_key_when_ecky_ast_patch_validate_then_patch_resolves_path() {
    let source = "(model (part body (box 1 2 3)))";
    let (state, resolver) = seed_target_with_macro("Box", "V-validate-key", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/parts/body/root";
    let node_digest = source_edit_digest(source, path);

    let ast = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: Some(path.to_string()),
            depth: Some(0),
            max_nodes: Some(8),
            include_source: Some(false),
        },
        &test_ctx(),
    )
    .await
    .expect("ast response");
    let stable_node_key = ast
        .nodes
        .first()
        .map(|node| node.stable_node_key.clone())
        .expect("stable node key");

    let response = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: Some(stable_node_key),
            path: None,
            expected_node_digest: node_digest,
            replacement_source: Some("(box 9 9 9)".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("patch validate");

    assert_eq!(response.edited_path, path);
}

#[tokio::test]
async fn given_bogus_stable_node_key_when_ecky_ast_patch_validate_then_rejects_cleanly() {
    let source = "(model (part body (box 1 2 3)))";
    let (state, resolver) = seed_target_with_macro("Box", "V-validate-bogus-key", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

    let err = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: Some("sha256:not-a-real-node".to_string()),
            path: None,
            expected_node_digest: source_edit_digest(source, "/parts/body/root"),
            replacement_source: Some("(box 4 5 6)".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("bogus stable node key");

    assert!(err.message.contains("stableNodeKey not found in AST"));
}

#[tokio::test]
async fn given_mismatched_stable_node_key_and_path_when_ecky_ast_patch_validate_then_rejects() {
    let source = "(model (part body (box 1 2 3)))";
    let (state, resolver) = seed_target_with_macro("Box", "V-validate-key-mismatch", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let key_path = "/parts/body/root";

    let ast = handle_ecky_ast_get(
        &state,
        &resolver,
        EckyAstGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: Some(key_path.to_string()),
            depth: Some(0),
            max_nodes: Some(8),
            include_source: Some(false),
        },
        &test_ctx(),
    )
    .await
    .expect("ast response");
    let stable_node_key = ast
        .nodes
        .first()
        .map(|node| node.stable_node_key.clone())
        .expect("stable node key");

    let err = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: Some(stable_node_key),
            path: Some("/parts/body/root/call/args/0".to_string()),
            expected_node_digest: source_edit_digest(source, key_path),
            replacement_source: Some("9".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("key/path mismatch");

    assert!(err.message.contains("stableNodeKey/path mismatch"));
}

#[tokio::test]
async fn given_param_patch_when_ecky_ast_patch_validate_then_dependency_impact_uses_param_helpers()
{
    let source =
        "(model (params (number width 12) (number height 6)) (part body (box width height 3)))";
    let (state, resolver) =
        seed_target_with_macro("Params", "V-validate-param-impact", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/params/width";

    let response = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: source_edit_digest(source, path),
            replacement_source: Some("(number width 24)".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("param patch validate");

    let value = serde_json::to_value(&response).expect("param patch json");
    assert_eq!(
        value["dependencyImpact"]["dependencyKind"],
        serde_json::json!("parameterReference")
    );
    assert_eq!(
        value["dependencyImpact"]["path"],
        serde_json::json!("/params/width")
    );
    assert_eq!(
        value["dependencyImpact"]["impactedPartIds"],
        serde_json::json!(["body"])
    );
    assert_eq!(
        value["dependencyImpact"]["dependentSourcePaths"],
        serde_json::json!(["/parts/body/root/call/args/0"])
    );
    assert_eq!(
        value["dependencyImpact"]["referenceCount"],
        serde_json::json!(1)
    );
}

#[tokio::test]
async fn given_film_coupon_fixture_when_film_gap_patch_validate_then_patch_preview_renders() {
    let source =
        include_str!("../../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
    let (state, resolver) = seed_target_with_macro("Film Coupon", "V-film-coupon", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/params/film_gap";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let expected_node_digest = source_edit_digest(source, path);
    let replacement_source =
        "(number film_gap 0.45 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01)".to_string();

    let validate = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: source_digest.clone(),
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: expected_node_digest.clone(),
            replacement_source: Some(replacement_source.clone()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("film gap patch validate");

    assert_eq!(validate.status, "valid");
    assert_eq!(validate.edited_path, path);
    assert_ne!(validate.source_digest, validate.new_source_digest);
    assert_eq!(
        validate
            .dependency_impact
            .as_ref()
            .map(|impact| impact.dependency_kind.as_str()),
        Some("parameterReference")
    );

    let preview = handle_ecky_ast_replace_and_render(
        &state,
        &resolver,
        EckyAstReplaceAndRenderRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest,
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest,
            replacement_source: Some(replacement_source),
            new_name: None,
            parameters: None,
            post_processing: None,
            geometry_backend: None,
        },
        &test_ctx(),
    )
    .await
    .expect("film gap patch preview");

    assert_eq!(preview.thread_id, "thread-1");
    assert_ne!(preview.message_id, "msg-1");
    assert!(preview.macro_code.contains("(number film_gap 0.45"));
    assert_eq!(
        preview.artifact_bundle.source_language,
        crate::models::SourceLanguage::EckyIrV0
    );
    assert!(!preview.artifact_bundle.preview_stl_path.trim().is_empty());
}

#[tokio::test]
async fn given_film_coupon_fixture_when_film_gap_patch_preview_then_commit_returns_model_id_and_digest(
) {
    let source =
        include_str!("../../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
    let (state, resolver) =
        seed_target_with_macro("Film Coupon", "V-film-coupon-commit", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/params/film_gap";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let expected_node_digest = source_edit_digest(source, path);
    let replacement_source =
        "(number film_gap 0.53 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01)".to_string();

    let preview = handle_ecky_ast_replace_and_render(
        &state,
        &resolver,
        EckyAstReplaceAndRenderRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest,
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest,
            replacement_source: Some(replacement_source),
            new_name: None,
            parameters: None,
            post_processing: None,
            geometry_backend: None,
        },
        &test_ctx(),
    )
    .await
    .expect("film gap preview for commit");

    assert_eq!(preview.thread_id, "thread-1");
    assert_eq!(
        preview.artifact_digest.model_id,
        preview.artifact_bundle.model_id
    );
    assert_eq!(
        preview.artifact_digest.content_hash,
        preview.artifact_bundle.content_hash
    );

    let commit = handle_commit_preview_version(
        &state,
        &resolver,
        VersionSaveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(preview.thread_id.clone()),
            message_id: Some(preview.message_id.clone()),
            title: Some("Film Coupon Committed".to_string()),
            version_name: Some("V-film-gap-commit".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("commit film gap preview");

    assert_eq!(commit.thread_id, "thread-1");
    assert_eq!(commit.model_id, preview.artifact_bundle.model_id);

    let conn = state.db.lock().await;
    let messages = db::get_thread_messages(&conn, "thread-1").expect("thread messages");
    let committed = messages
        .iter()
        .find(|message| message.id == commit.message_id)
        .expect("committed message");
    assert!(committed
        .output
        .as_ref()
        .expect("committed output")
        .macro_code
        .contains("(number film_gap 0.53"));
}

#[tokio::test]
async fn given_wrapper_param_path_when_ecky_ast_replace_and_render_then_only_numeric_token_changes_and_preview_renders(
) {
    let source = "(model\n  (params\n    ; keep formatting + comment\n    (number film_gap 0.35 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01))\n  (part body (box film_gap 2 3)))";
    let (state, resolver) =
        seed_target_with_macro("Wrapper Path", "V-wrapper-number", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/params/film_gap";
    let source_digest = crate::mcp::macro_buffer::source_digest(source);
    let expected_node_digest = source_edit_digest(source, path);

    let preview = handle_ecky_ast_replace_and_render(
        &state,
        &resolver,
        EckyAstReplaceAndRenderRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest,
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest,
            replacement_source: Some(
                "(number film_gap 0.45 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01)"
                    .to_string(),
            ),
            new_name: None,
            parameters: None,
            post_processing: None,
            geometry_backend: None,
        },
        &test_ctx(),
    )
    .await
    .expect("wrapper-path numeric preview");

    assert!(preview.macro_code.contains("; keep formatting + comment"));
    assert!(preview
        .macro_code
        .contains("(number film_gap 0.45 :label \"film gap\" :min 0.2 :max 1.2 :step 0.01)"));
    assert!(!preview.macro_code.contains("(number film_gap 0.35 "));
    assert!(!preview.artifact_bundle.preview_stl_path.trim().is_empty());
}

#[tokio::test]
async fn given_ecky_ast_shape_patch_when_params_omitted_then_preview_preserves_current_values() {
    let source =
        "(model (params (number width 10) (number height 3)) (part body (box width height 2)))";
    let (state, resolver) =
        seed_target_with_macro("Preserve Params", "V-preserve-params", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    {
        let conn = state.db.lock().await;
        let mut messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
        let mut message = messages.pop().expect("seed message");
        let mut output = message.output.take().expect("output");
        output
            .initial_params
            .insert("width".to_string(), ParamValue::Number(42.0));
        output
            .initial_params
            .insert("height".to_string(), ParamValue::Number(9.0));
        db::update_message_status_and_output(
            &conn,
            "msg-1",
            db::MessageStatusUpdate {
                status: &MessageStatus::Success,
                output: Some(&output),
                usage: None,
                artifact_bundle: message.artifact_bundle.as_ref(),
                model_manifest: message.model_manifest.as_ref(),
                structural_verification: None,
                visual_kind: None,
                content: Some("Base version"),
            },
        )
        .expect("update params");
    }

    let preview = handle_ecky_ast_replace_and_render(
        &state,
        &resolver,
        EckyAstReplaceAndRenderRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some("/parts/body".to_string()),
            expected_node_digest: source_edit_digest(source, "/parts/body"),
            replacement_source: Some("(part body (box width height 4))".to_string()),
            new_name: None,
            parameters: None,
            post_processing: None,
            geometry_backend: None,
        },
        &test_ctx(),
    )
    .await
    .expect("shape patch preview");

    assert_eq!(
        preview.initial_params.get("width"),
        Some(&ParamValue::Number(42.0))
    );
    assert_eq!(
        preview.initial_params.get("height"),
        Some(&ParamValue::Number(9.0))
    );
}

#[tokio::test]
async fn given_macro_preview_on_existing_target_when_agent_passes_parameters_then_current_values_win(
) {
    let source =
        "(model (params (number width 10) (number height 3)) (part body (box width height 2)))";
    let (state, resolver) = seed_target_with_macro("Macro Params", "V-macro-params", source).await;
    {
        let conn = state.db.lock().await;
        let mut messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
        let mut message = messages.pop().expect("seed message");
        let mut output = message.output.take().expect("output");
        output
            .initial_params
            .insert("width".to_string(), ParamValue::Number(42.0));
        output
            .initial_params
            .insert("height".to_string(), ParamValue::Number(9.0));
        db::update_message_status_and_output(
            &conn,
            "msg-1",
            db::MessageStatusUpdate {
                status: &MessageStatus::Success,
                output: Some(&output),
                usage: None,
                artifact_bundle: message.artifact_bundle.as_ref(),
                model_manifest: message.model_manifest.as_ref(),
                structural_verification: None,
                visual_kind: None,
                content: Some("Base version"),
            },
        )
        .expect("update params");
    }

    let preview = handle_macro_preview_render(
            &state,
            &resolver,
            MacroReplaceRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                macro_code:
                    "(model (params (number width 10) (number height 3)) (part body (box width height 4)))"
                        .to_string(),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                ui_spec: None,
                parameters: Some(BTreeMap::from([
                    ("width".to_string(), ParamValue::Number(999.0)),
                    ("height".to_string(), ParamValue::Number(888.0)),
                ])),
                post_processing: None,
                geometry_backend: None,
            },
            &test_ctx(),
        )
        .await
        .expect("macro preview");

    assert_eq!(
        preview.initial_params.get("width"),
        Some(&ParamValue::Number(42.0))
    );
    assert_eq!(
        preview.initial_params.get("height"),
        Some(&ParamValue::Number(9.0))
    );
}

#[tokio::test]
async fn given_macro_preview_adds_first_params_to_existing_target_then_new_macro_defaults_win() {
    let source = "(model (part body (box 1 2 3)))";
    let (state, resolver) = seed_target_with_macro("First Params", "V-first-params", source).await;
    {
        let conn = state.db.lock().await;
        let mut messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
        let mut message = messages.pop().expect("seed message");
        let mut output = message.output.take().expect("output");
        output.initial_params.clear();
        db::update_message_status_and_output(
            &conn,
            "msg-1",
            db::MessageStatusUpdate {
                status: &MessageStatus::Success,
                output: Some(&output),
                usage: None,
                artifact_bundle: message.artifact_bundle.as_ref(),
                model_manifest: message.model_manifest.as_ref(),
                structural_verification: None,
                visual_kind: None,
                content: Some("Base version"),
            },
        )
        .expect("clear params");
    }

    let preview = handle_macro_preview_render(
            &state,
            &resolver,
            MacroReplaceRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                macro_code:
                    "(model (params (number width 100) (number height 7)) (part body (box width height 4)))"
                        .to_string(),
                macro_dialect: Some(MacroDialect::EckyIrV0),
                ui_spec: None,
                parameters: Some(BTreeMap::from([
                    ("width".to_string(), ParamValue::Number(999.0)),
                    ("height".to_string(), ParamValue::Number(888.0)),
                ])),
                post_processing: None,
                geometry_backend: None,
            },
            &test_ctx(),
        )
        .await
        .expect("macro preview");

    assert_eq!(
        preview.initial_params.get("width"),
        Some(&ParamValue::Number(100.0))
    );
    assert_eq!(
        preview.initial_params.get("height"),
        Some(&ParamValue::Number(7.0))
    );
}

#[tokio::test]
async fn given_lens_bore_patch_when_ecky_ast_patch_validate_then_dependency_scope_stays_on_bore_controls(
) {
    let source = "(model (params (number lens_bore_d 42) (number wall_t 3)) (part bore_carrier (cylinder lens_bore_d 6)) (part wall (box wall_t wall_t 4)))";
    let (state, resolver) =
        seed_target_with_macro("Lens Bore Scope", "V-lens-bore-scope", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/params/lens_bore_d";

    let response = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: source_edit_digest(source, path),
            replacement_source: Some("(number lens_bore_d 44)".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("lens bore patch validate");

    assert_eq!(response.status, "valid");
    assert_eq!(response.edited_path, path);
    assert_eq!(response.affected_paths, vec![path.to_string()]);
    let impact = response
        .dependency_impact
        .as_ref()
        .expect("dependency impact");
    assert_eq!(impact.path, path);
    assert_eq!(impact.dependency_kind, "parameterReference");
    assert_eq!(impact.impacted_part_ids, vec!["bore_carrier".to_string()]);
    assert_eq!(
        impact.dependent_source_paths,
        vec!["/parts/bore_carrier/root/call/args/0".to_string()]
    );
    assert_eq!(impact.reference_count, 1);
    assert!(!impact.impacted_part_ids.iter().any(|id| id == "wall"));
    assert!(!impact
        .dependent_source_paths
        .iter()
        .any(|path| path.contains("/parts/wall/")));
}

#[tokio::test]
async fn given_lens_bore_dependency_fixture_when_ecky_dependency_get_then_downstream_roles_return_together(
) {
    let source = "(model
  (params (number lens_bore_d 42))
  (part carrier (cylinder lens_bore_d 6))
  (part socket (cylinder lens_bore_d 5))
  (part thread (cylinder lens_bore_d 4))
  (part stop_lip (cylinder lens_bore_d 3)))";
    let (state, resolver) =
        seed_target_with_macro("Lens Bore Dependency", "V-lens-bore-deps", source).await;

    let response = handle_ecky_dependency_get(
        &state,
        &resolver,
        EckyDependencyGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            path: "/params/lens_bore_d".to_string(),
        },
        &test_ctx(),
    )
    .await
    .expect("lens bore dependency");

    assert_eq!(response.path, "/params/lens_bore_d");
    assert_eq!(response.dependency_kind, "parameterReference");
    let mut impacted = response.impacted_part_ids.clone();
    impacted.sort();
    assert_eq!(
        impacted,
        vec![
            "carrier".to_string(),
            "socket".to_string(),
            "stop_lip".to_string(),
            "thread".to_string(),
        ]
    );
    assert_eq!(response.reference_count, 4);
    assert_eq!(
        response.dependent_source_paths,
        vec![
            "/parts/carrier/root/call/args/0".to_string(),
            "/parts/socket/root/call/args/0".to_string(),
            "/parts/thread/root/call/args/0".to_string(),
            "/parts/stop_lip/root/call/args/0".to_string(),
        ]
    );
}

#[tokio::test]
async fn given_stale_source_digest_when_ecky_ast_patch_validate_then_rejects() {
    let source = "(model (part body (box 1 2 3)))";
    let (state, resolver) = seed_target_with_macro("Box", "V-validate-stale-source", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/parts/body/root";

    let err = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: "sha256:stale".to_string(),
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: source_edit_digest(source, path),
            replacement_source: Some("(box 4 5 6)".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("stale source digest");

    assert!(err.message.contains("digest mismatch"));
}

#[tokio::test]
async fn given_stale_node_digest_when_ecky_ast_patch_validate_then_rejects() {
    let source = "(model (part body (box 1 2 3)))";
    let (state, resolver) = seed_target_with_macro("Box", "V-validate-stale-node", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

    let err = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some("/parts/body/root".to_string()),
            expected_node_digest: "sha256:not-current".to_string(),
            replacement_source: Some("(box 4 5 6)".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("stale node digest");

    assert!(err.message.contains("node digest mismatch"));
}

#[tokio::test]
async fn given_invalid_replacement_when_ecky_ast_patch_validate_then_rejects_before_render() {
    let source = "(model (part body (box 1 2 3)))";
    let (state, resolver) = seed_target_with_macro("Box", "V-validate-invalid", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/parts/body/root";

    let err = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: source_edit_digest(source, path),
            replacement_source: Some("(box 4 5 6))".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("invalid replacement");

    assert!(err
        .message
        .contains("Replacement produced invalid Ecky source"));
    assert_eq!(err.operation.as_deref(), Some("replace"));
    assert!(err
        .stable_node_key
        .as_deref()
        .is_some_and(|value| value.starts_with("sha256:")));
    assert!(err.start_line.is_some());
    assert!(err.end_line.is_some());
    assert!(err.start_line.unwrap() <= err.end_line.unwrap());
}

#[tokio::test]
async fn given_helical_ridge_parse_failure_when_patch_validate_then_error_keeps_stable_key_and_span_lines(
) {
    let source = "(model
  (part body
    (helical-ridge
      (cylinder 12 8)
      :pitch 2
      :height 6)))";
    let (state, resolver) =
        seed_target_with_macro("Helical Ridge", "V-helical-ridge-err", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/parts/body/root";

    let err = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: source_edit_digest(source, path),
            replacement_source: Some(
                "(helical-ridge (cylinder 12 8) :pitch 2 :height 6))".to_string(),
            ),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("helical ridge parse failure");

    assert!(err
        .message
        .contains("Replacement produced invalid Ecky source"));
    assert_eq!(err.operation.as_deref(), Some("replace"));
    assert!(err
        .stable_node_key
        .as_deref()
        .is_some_and(|value| value.starts_with("sha256:")));
    assert!(err.start_line.is_some());
    assert!(err.end_line.is_some());
    assert!(err.start_line.unwrap() <= err.end_line.unwrap());
    assert!(err.message.contains("/parts/body/root"), "{err:?}");
}

#[tokio::test]
async fn given_render_lowering_failures_when_macro_preview_render_then_mcp_error_keeps_diagnostics_and_raw_details(
) {
    let lowering_source = r#"(model
  (part body
    (def body
      (box 10 10 10))))"#;
    let (state, resolver) =
        seed_target_with_macro("Lowering Fail", "V-lowering-fail", lowering_source).await;

    let lowering_err = handle_macro_preview_render(
        &state,
        &resolver,
        MacroReplaceRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            macro_code: lowering_source.to_string(),
            macro_dialect: Some(MacroDialect::EckyIrV0),
            ui_spec: None,
            parameters: Some(BTreeMap::from([(
                "clearance".to_string(),
                ParamValue::Number(0.3),
            )])),
            post_processing: None,
            geometry_backend: Some(crate::models::GeometryBackend::Build123d),
        },
        &test_ctx(),
    )
    .await
    .expect_err("build123d lowering should fail for unsupported operation");

    if lowering_err
        .message
        .contains("build123d import check failed")
    {
        assert_eq!(lowering_err.code, AppErrorCode::Render);
        return;
    }

    assert!(
        lowering_err
            .details
            .as_deref()
            .is_some_and(|details| details.contains("def")),
        "{lowering_err:?}"
    );
    let lowering_context = lowering_err
        .diagnostic_context
        .as_ref()
        .expect("lowering diagnostic context");
    assert_eq!(lowering_context.resolved_params.len(), 1);
    assert_eq!(lowering_context.resolved_params[0].key, "clearance");

    let malformed_source = "(model\n  (part body (box 1 2 3))\n$)";
    let (state, resolver) =
        seed_target_with_macro("Malformed", "V-malformed", malformed_source).await;
    let malformed_err = handle_macro_preview_render(
        &state,
        &resolver,
        MacroReplaceRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            macro_code: malformed_source.to_string(),
            macro_dialect: Some(MacroDialect::EckyIrV0),
            ui_spec: None,
            parameters: None,
            post_processing: None,
            geometry_backend: Some(crate::models::GeometryBackend::Build123d),
        },
        &test_ctx(),
    )
    .await
    .expect_err("invalid Ecky source should fail in lowering path");

    assert_eq!(malformed_err.operation.as_deref(), Some("lower:build123d"));
    // The raw parser diagnostic (naming the offending `$` token) must survive
    // to the MCP error, whatever the parser's exact phrasing is.
    assert!(
        malformed_err.message.contains('$'),
        "{malformed_err:?}"
    );
    assert!(
        malformed_err
            .start_line
            .zip(malformed_err.end_line)
            .is_none_or(|(start, end)| start <= end),
        "{malformed_err:?}"
    );
    if let Some(stable_node_key) = malformed_err.stable_node_key.as_deref() {
        assert!(stable_node_key.starts_with("sha256:"), "{malformed_err:?}");
    }
}

#[tokio::test]
async fn given_delete_operation_when_ecky_ast_patch_validate_then_returns_valid_delete_diff() {
    let source =
        "(model (part body (build (shape rail (box 1 2 3)) (shape cap (sphere 2)) (result cap))))";
    let (state, resolver) = seed_target_with_macro("Delete", "V-validate-delete", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/parts/body/root/build/bindings/rail";

    let response = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Delete,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: source_edit_digest(source, path),
            replacement_source: None,
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("delete validate");

    let value = serde_json::to_value(&response).expect("delete json");
    assert_eq!(value["operation"], "delete");
    assert_eq!(value["status"], "valid");
    assert_eq!(value["newNodeDigest"], "deleted");
}

#[tokio::test]
async fn given_rename_operation_when_ecky_ast_patch_validate_then_returns_new_path() {
    let source = "(model (part body (build (shape rail (box 1 2 3)) (shape cap (translate 0 0 1 rail)) (result cap))))";
    let (state, resolver) = seed_target_with_macro("Rename", "V-validate-rename", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;
    let path = "/parts/body/root/build/bindings/rail";

    let response = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Rename,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some(path.to_string()),
            expected_node_digest: source_edit_digest(source, path),
            replacement_source: None,
            new_name: Some("spine".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("rename validate");

    let value = serde_json::to_value(&response).expect("rename json");
    assert_eq!(value["operation"], "rename");
    assert_eq!(
        value["affectedPathDetails"][0]["newPath"],
        "/parts/body/root/build/bindings/spine"
    );
    assert_ne!(value["newNodeDigest"], "deleted");
}

#[tokio::test]
async fn given_non_source_addressable_path_when_ecky_ast_patch_validate_then_rejects() {
    let source = "(model (params (toggle raised true)) (part body (if raised (sphere 10) (cylinder 10 20))))";
    let (state, resolver) = seed_target_with_macro("Conditional", "V-validate-path", source).await;
    state.config.lock().unwrap().mcp.ecky_ast_authoring = true;

    let err = handle_ecky_ast_patch_validate(
        &state,
        &resolver,
        EckyAstPatchValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            operation: EckyAstEditOperation::Replace,
            source_digest: crate::mcp::macro_buffer::source_digest(source),
            stable_node_key: None,
            path: Some("/parts/body/root/if/condition".to_string()),
            expected_node_digest: "sha256:not-used".to_string(),
            replacement_source: Some("raised".to_string()),
            new_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("non source-addressable path");

    assert!(err.message.contains("not source-span addressable"));
}

#[tokio::test]
async fn version_restore_returns_artifact_digest_for_export_truth() {
    let (state, _resolver) = seed_target().await;
    let response = handle_version_restore(
        &state,
        VersionRestoreRequest {
            identity: AgentIdentityOverride::default(),
            message_id: "msg-1".to_string(),
        },
        &test_ctx(),
    )
    .await
    .expect("version restore");

    let artifact_digest = response.artifact_digest.expect("artifact digest");
    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.message_id, "msg-1");
    assert_eq!(artifact_digest.model_id, "model-base");
    assert!(artifact_digest.has_step_export);
    assert_eq!(
        artifact_digest.step_export_path.as_deref(),
        Some("/tmp/model-base.step")
    );
}

#[tokio::test]
async fn target_detail_get_returns_requested_ui_spec_only() {
    let (state, resolver) = seed_target().await;
    let response = handle_target_detail_get(
        &state,
        &resolver,
        TargetDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            section: TargetDetailSection::UiSpec,
            shape_graph_filters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("target uiSpec detail");

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["section"], "uiSpec");
    assert_eq!(value["authoringContext"]["sourceLanguage"], "legacyPython");
    assert_eq!(value["authoringContext"]["geometryBackend"], "freecad");
    assert!(value["authoringContext"]["authoringCard"]
        .as_str()
        .unwrap()
        .contains("Ecky authoring card"));
    assert!(value.get("uiSpec").is_some());
    assert!(value.get("initialParams").is_none());
    assert!(value.get("artifactBundle").is_none());
    assert!(value.get("latestDraft").is_none());
}

#[tokio::test]
async fn target_detail_get_returns_requested_initial_params_only() {
    let (state, resolver) = seed_target().await;
    let response = handle_target_detail_get(
        &state,
        &resolver,
        TargetDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            section: TargetDetailSection::InitialParams,
            shape_graph_filters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("target params detail");

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["section"], "initialParams");
    assert_eq!(value["initialParams"]["diameter"], 130.0);
    assert!(value.get("uiSpec").is_none());
    assert!(value.get("artifactBundle").is_none());
    assert!(value.get("latestDraft").is_none());
}

#[tokio::test]
async fn target_detail_get_returns_active_artifact_bundle_only() {
    let (state, resolver) = seed_target().await;
    let response = handle_target_detail_get(
        &state,
        &resolver,
        TargetDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            section: TargetDetailSection::ArtifactBundle,
            shape_graph_filters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("target artifact detail");

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["section"], "artifactBundle");
    assert_eq!(value["artifactBundle"]["modelId"], "model-base");
    assert_eq!(value["artifactBundle"]["sourceLanguage"], "legacyPython");
    assert_eq!(value["artifactBundle"]["geometryBackend"], "freecad");
    assert_eq!(value["artifactBundle"]["hasPreviewStl"], true);
    assert_eq!(
        value["artifactBundle"]["exportFormats"],
        serde_json::json!(["step"])
    );
    assert_eq!(value["artifactBundle"]["hasStepExport"], true);
    assert_eq!(
        value["artifactBundle"]["stepExportPath"],
        "/tmp/model-base.step"
    );
    assert!(value.get("uiSpec").is_none());
    assert!(value.get("initialParams").is_none());
    assert!(value.get("latestDraft").is_none());
}

#[tokio::test]
async fn target_detail_get_shape_graph_returns_compact_packet_without_source_text() {
    let source = "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part body (box holder_w holder_h 3)))";
    let (state, resolver) = seed_target_with_macro("ShapeGraph", "V-shape-graph", source).await;
    let response = handle_target_detail_get(
        &state,
        &resolver,
        TargetDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            section: TargetDetailSection::ShapeGraph,
            shape_graph_filters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("target shape graph detail");

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["section"], "shapeGraph");
    assert!(value.get("macroCode").is_none());
    assert!(value.get("uiSpec").is_none());
    assert_eq!(value["shapeGraph"]["parts"]["items"][0]["partId"], "body");
    assert_eq!(
        value["shapeGraph"]["constraints"]["items"][0]["kind"],
        "relation"
    );
    assert_eq!(
        value["shapeGraph"]["dependencies"]["items"][0]["parameterKey"],
        "holder_w"
    );
    assert!(value["shapeGraph"]["sourceDigest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(value["shapeGraph"]["coreDigest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
}

#[tokio::test]
async fn target_detail_get_shape_graph_honors_section_filters() {
    let source = "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part body (box holder_w holder_h 3)))";
    let (state, resolver) =
        seed_target_with_macro("ShapeGraphFilter", "V-shape-graph-filter", source).await;
    let response = handle_target_detail_get(
        &state,
        &resolver,
        TargetDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            section: TargetDetailSection::ShapeGraph,
            shape_graph_filters: Some(vec![ShapeGraphFilterSection::Constraints]),
        },
        &test_ctx(),
    )
    .await
    .expect("target filtered shape graph detail");

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["section"], "shapeGraph");
    assert!(value["shapeGraph"].get("constraints").is_some());
    assert!(value["shapeGraph"].get("parts").is_none());
    assert!(value["shapeGraph"].get("instances").is_none());
    assert!(value["shapeGraph"].get("dependencies").is_none());
}

#[tokio::test]
async fn given_agent_needs_intent_without_full_source_when_target_detail_get_section_shape_graph_then_returns_parts_constraints_dependencies_without_macro_or_source_payload(
) {
    let source = "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part body (box holder_w holder_h 3)))";
    let (state, resolver) =
        seed_target_with_macro("ShapeGraphIntent", "V-shape-graph-intent", source).await;
    let response = handle_target_detail_get(
        &state,
        &resolver,
        TargetDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            section: TargetDetailSection::ShapeGraph,
            shape_graph_filters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("target shape graph detail");

    let value = serde_json::to_value(&response).expect("response json");
    assert_eq!(value["section"], "shapeGraph");
    assert!(value["shapeGraph"].get("parts").is_some());
    assert!(value["shapeGraph"].get("constraints").is_some());
    assert!(value["shapeGraph"].get("dependencies").is_some());
    assert!(value.get("macroCode").is_none());
    assert!(value.get("macro_code").is_none());
    assert!(value["shapeGraph"]["parts"]["items"][0]
        .get("source")
        .is_none());
    assert!(value["shapeGraph"]["constraints"]["items"][0]
        .get("source")
        .is_none());
    assert!(value["shapeGraph"]["dependencies"]["items"][0]
        .get("source")
        .is_none());
}

#[tokio::test]
async fn given_agent_validates_physical_edits_when_ecky_constraints_validate_with_repeated_anonymous_offsets_then_returns_actionable_authoring_lint_and_relation_metadata(
) {
    let (state, resolver) = seed_target_with_macro(
            "Anonymous delta relation metadata",
            "V-anonymous-delta-relation-metadata",
            "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part holder (box (+ holder_w 12) (+ holder_w 12) 3)))",
        )
        .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    let relation = response
        .rows
        .iter()
        .find(|row| row.path == "/params/:relations/0")
        .expect("relation row");
    assert_eq!(relation.constraint_id.as_deref(), Some("relation_0"));
    assert_eq!(relation.kind.as_deref(), Some("relation"));
    assert_eq!(
        relation.depends_on_param_keys,
        vec!["holder_h".to_string(), "holder_w".to_string()]
    );
    assert!(relation
        .source_stable_node_key
        .as_ref()
        .is_some_and(|key| !key.trim().is_empty()));
    assert!(!relation.affects_stable_node_keys.is_empty());

    let lint = response
        .authoring_lints
        .iter()
        .find(|lint| {
            lint.kind == "anonymousDelta" && lint.param_key == "holder_w" && lint.delta == 12.0
        })
        .expect("anonymous delta lint");
    assert_eq!(lint.suggested_param_key, "holder_margin_x");
    assert!(lint.message.contains("holder_margin_x"));
    assert!(!lint.source_stable_node_keys.is_empty());
}

#[tokio::test]
async fn given_no_repeated_offsets_when_ecky_constraints_validate_then_returns_no_authoring_lint_noise(
) {
    let (state, resolver) = seed_target_with_macro(
            "No repeated anonymous delta",
            "V-no-repeated-anonymous-delta",
            "(model (params (number holder_w 40) (number holder_h 20) :relations ((< holder_h holder_w))) (part holder (box (+ holder_w 12) holder_h 3)))",
        )
        .await;

    let response = handle_ecky_constraints_validate(
        &state,
        &resolver,
        EckyConstraintsValidateRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            parameters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("constraint validation");

    assert!(response.authoring_lints.is_empty());
}

#[tokio::test]
async fn artifact_manifest_get_returns_full_valid_runtime_manifest() {
    let (state, resolver) = seed_target().await;
    let response = handle_artifact_manifest_get(
        &state,
        &resolver,
        ArtifactManifestRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            model_id: None,
        },
        &test_ctx(),
    )
    .await
    .expect("artifact manifest");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.message_id, "msg-1");
    assert_eq!(response.model_id, "model-base");
    assert!(response.runtime_manifest_valid);
    assert_eq!(response.digest.model_id, "model-base");
    assert_eq!(response.digest.geometry_backend, "freecad");
    assert!(response.digest.has_step_export);
    assert_eq!(
        response.digest.step_export_path.as_deref(),
        Some("/tmp/model-base.step")
    );
    assert_eq!(response.artifact_bundle.model_id, "model-base");
    assert_eq!(response.model_manifest.model_id, "model-base");

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["runtimeManifestValid"], true);
    assert_eq!(
        value["artifactBundle"]["exportArtifacts"][0]["format"],
        "step"
    );
    assert_eq!(
        value["modelManifest"]["controlPrimitives"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn artifact_manifest_get_rejects_bundle_manifest_mismatch() {
    let (state, resolver) = seed_target().await;
    let mut bad_bundle = sample_bundle("model-bad", "bad.stl");
    bad_bundle
        .export_artifacts
        .push(crate::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: "/tmp/model-bad.step".to_string(),
            role: "cad-exchange".to_string(),
        });
    let bad_manifest = sample_manifest("model-other");
    {
        let conn = state.db.lock().await;
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "msg-bad".to_string(),
                role: MessageRole::Assistant,
                content: "Bad version".to_string(),
                status: MessageStatus::Success,
                output: Some(sample_design("Bad", "V-bad", "bad_macro()")),
                usage: None,
                artifact_bundle: Some(bad_bundle),
                model_manifest: Some(bad_manifest),
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now_secs() + 1,
            },
        )
        .unwrap();
    }

    let err = handle_artifact_manifest_get(
        &state,
        &resolver,
        ArtifactManifestRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-bad".to_string()),
            model_id: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("mismatched runtime manifest should be rejected");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("model id"), "{}", err.message);
}

#[tokio::test]
async fn artifact_feature_graph_get_reads_runtime_manifest_and_returns_backfilled_graphs() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-feature-graph";
    let mut bundle = sample_bundle(model_id, "feature-graph.stl");
    bundle.export_artifacts.push(crate::models::ExportArtifact {
        label: "STEP".to_string(),
        format: "step".to_string(),
        path: "/tmp/generated-feature-graph.step".to_string(),
        role: "cad-exchange".to_string(),
    });
    let mut runtime_manifest = sample_manifest(model_id);
    runtime_manifest.correspondence_graph = Some(crate::models::CorrespondenceGraph {
        edges: vec![crate::models::CorrespondenceEdge {
            edge_id: "edge-1".to_string(),
            source: crate::models::FeatureOutputRef {
                feature_id: "part:body".to_string(),
                output_id: "selectionTargets".to_string(),
                target_ids: vec!["body:face:0:5-5-5:100".to_string()],
            },
            target: crate::models::FeatureOutputRef {
                feature_id: "part:body".to_string(),
                output_id: "selectionTargets".to_string(),
                target_ids: vec!["body:face:0:5-5-5:100".to_string()],
            },
            relation: "sameTopology".to_string(),
            source_ref: None,
        }],
    });
    let (stored_bundle, _stored_manifest) =
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &runtime_manifest)
            .expect("runtime bundle");
    let stale_message_manifest = sample_manifest(model_id);
    assert!(stale_message_manifest.feature_graph.is_none());
    assert!(stale_message_manifest.correspondence_graph.is_none());
    {
        let conn = state.db.lock().await;
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "msg-feature-graph".to_string(),
                role: MessageRole::Assistant,
                content: "Feature graph version".to_string(),
                status: MessageStatus::Success,
                output: Some(sample_design("Graph", "V-graph", "graph_macro()")),
                usage: None,
                artifact_bundle: Some(stored_bundle),
                model_manifest: Some(stale_message_manifest),
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now_secs() + 1,
            },
        )
        .unwrap();
    }

    let response = handle_artifact_feature_graph_get(
        &state,
        &resolver,
        ArtifactFeatureGraphGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-feature-graph".to_string()),
            model_id: None,
        },
        &test_ctx(),
    )
    .await
    .expect("feature graph");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.message_id, "msg-feature-graph");
    assert_eq!(response.model_id, model_id);
    assert_eq!(response.artifact_digest.model_id, model_id);
    assert!(response.artifact_digest.has_step_export);
    let feature_graph = response.feature_graph.as_ref().expect("feature graph");
    assert_eq!(feature_graph.nodes.len(), 1);
    assert_eq!(feature_graph.nodes[0].feature_id, "part:body");
    assert_eq!(
        feature_graph.nodes[0].output_refs[0].target_ids,
        vec![
            "body:edge:0:0-0-0_10-0-0".to_string(),
            "body:face:0:5-5-5:100".to_string()
        ]
    );
    assert_eq!(
        response
            .correspondence_graph
            .as_ref()
            .expect("correspondence graph")
            .edges[0]
            .edge_id,
        "edge-1"
    );

    let value = serde_json::to_value(&response).expect("feature graph json");
    assert_eq!(value["modelId"], model_id);
    assert!(value["artifactDigest"]["hasStepExport"].as_bool().unwrap());
    assert_eq!(value["featureGraph"]["nodes"][0]["featureId"], "part:body");
    assert_eq!(
        value["correspondenceGraph"]["edges"][0]["relation"],
        "sameTopology"
    );
}

#[tokio::test]
async fn artifact_feature_graph_get_preserves_feature_ports() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-feature-ports";
    let bundle = sample_bundle(model_id, "feature-ports.stl");
    let mut runtime_manifest = sample_manifest(model_id);
    runtime_manifest.feature_graph = Some(crate::models::FeatureGraph {
        nodes: vec![crate::models::FeatureNode {
            feature_id: "part:body".to_string(),
            kind: "part".to_string(),
            label: "Body".to_string(),
            source_ref: Some(crate::models::SourceRef {
                source_id: Some("source-main".to_string()),
                path: Some("/parts/body/root".to_string()),
                start_byte: Some(0),
                end_byte: Some(42),
            }),
            dependency_ids: Vec::new(),
            output_refs: vec![crate::models::FeatureOutputRef {
                feature_id: "part:body".to_string(),
                output_id: "selectionTargets".to_string(),
                target_ids: vec!["body:face:0:5-5-5:100".to_string()],
            }],
            ports: vec![crate::models::FeaturePort {
                port_id: "mount-face".to_string(),
                type_id: "mechanical.mount".to_string(),
                target_ids: vec!["body:face:0:5-5-5:100".to_string()],
                frame: Some(crate::models::PortFrame::identity()),
                interfaces: vec!["m3-clearance".to_string()],
                params: std::collections::BTreeMap::from([(
                    "clearanceMm".to_string(),
                    crate::models::ComponentInterfaceValue::Number(0.3),
                )]),
                source_ref: None,
                confidence: Some(0.85),
                target_role: Some("mountingFace".to_string()),
            }],
        }],
    });
    let (stored_bundle, _stored_manifest) =
        crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &runtime_manifest)
            .expect("runtime bundle");
    {
        let conn = state.db.lock().await;
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "msg-feature-ports".to_string(),
                role: MessageRole::Assistant,
                content: "Feature ports version".to_string(),
                status: MessageStatus::Success,
                output: Some(sample_design("Ports", "V-ports", "ports_macro()")),
                usage: None,
                artifact_bundle: Some(stored_bundle),
                model_manifest: Some(sample_manifest(model_id)),
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now_secs() + 1,
            },
        )
        .unwrap();
    }

    let response = handle_artifact_feature_graph_get(
        &state,
        &resolver,
        ArtifactFeatureGraphGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-feature-ports".to_string()),
            model_id: None,
        },
        &test_ctx(),
    )
    .await
    .expect("feature graph");

    let port = &response
        .feature_graph
        .as_ref()
        .expect("feature graph")
        .nodes[0]
        .ports[0];
    assert_eq!(port.port_id, "mount-face");
    assert_eq!(port.target_ids, vec!["body:face:0:5-5-5:100"]);
    assert_eq!(port.interfaces, vec!["m3-clearance"]);
    assert_eq!(port.confidence, Some(0.85));
    assert_eq!(port.target_role.as_deref(), Some("mountingFace"));

    let value = serde_json::to_value(&response).expect("feature ports json");
    assert_eq!(
        value["featureGraph"]["nodes"][0]["ports"][0]["portId"],
        "mount-face"
    );
    assert_eq!(
        value["featureGraph"]["nodes"][0]["ports"][0]["params"]["clearanceMm"],
        0.3
    );
}

#[tokio::test]
async fn artifact_feature_graph_get_film_adapter_fixture_exposes_expected_kinds_source_keys_and_targets(
) {
    let source =
        include_str!("../../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
    let model_id = "generated-film-adapter-feature-graph";
    let (state, resolver, _) =
        seed_ecky_printability_target(source, model_id, "film-adapter-feature-graph.stl").await;
    let bundle =
        crate::model_runtime::read_artifact_bundle(&resolver, model_id).expect("runtime bundle");
    let mut manifest =
        crate::model_runtime::read_model_manifest(&resolver, model_id).expect("runtime manifest");
    let rendered_target_ids = manifest
        .selection_targets
        .iter()
        .filter_map(|target| target.target_id.clone())
        .collect::<Vec<_>>();
    assert!(
        rendered_target_ids.len() >= 2,
        "expected seeded manifest selection targets"
    );
    let edge_target_id = rendered_target_ids[0].clone();
    let face_target_id = rendered_target_ids[1].clone();
    manifest.feature_graph = Some(crate::models::FeatureGraph {
        nodes: vec![
            crate::models::FeatureNode {
                feature_id: "film_path".to_string(),
                kind: "film_path".to_string(),
                label: "Film Path".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-film-path".to_string()),
                    path: Some("/parts/body/film_path".to_string()),
                    start_byte: Some(10),
                    end_byte: Some(40),
                }),
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "film_path".to_string(),
                    output_id: "film-path-solid".to_string(),
                    target_ids: vec![edge_target_id.clone()],
                }],
                ports: Vec::new(),
            },
            crate::models::FeatureNode {
                feature_id: "insert_clamp".to_string(),
                kind: "insert_clamp".to_string(),
                label: "Insert Clamp".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-insert-clamp".to_string()),
                    path: Some("/parts/body/insert_clamp".to_string()),
                    start_byte: Some(50),
                    end_byte: Some(80),
                }),
                dependency_ids: vec!["film_path".to_string()],
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "insert_clamp".to_string(),
                    output_id: "insert-clamp-solid".to_string(),
                    target_ids: vec![face_target_id.clone()],
                }],
                ports: Vec::new(),
            },
            crate::models::FeatureNode {
                feature_id: "helicoid_thread".to_string(),
                kind: "helicoid_thread".to_string(),
                label: "Helicoid Thread".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-helicoid-thread".to_string()),
                    path: Some("/parts/body/helicoid_thread".to_string()),
                    start_byte: Some(90),
                    end_byte: Some(130),
                }),
                dependency_ids: vec!["insert_clamp".to_string()],
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "helicoid_thread".to_string(),
                    output_id: "helicoid-thread-solid".to_string(),
                    target_ids: vec![face_target_id.clone()],
                }],
                ports: Vec::new(),
            },
            crate::models::FeatureNode {
                feature_id: "lens_bore".to_string(),
                kind: "lens_bore".to_string(),
                label: "Lens Bore".to_string(),
                source_ref: Some(crate::models::SourceRef {
                    source_id: Some("source-lens-bore".to_string()),
                    path: Some("/parts/body/lens_bore".to_string()),
                    start_byte: Some(140),
                    end_byte: Some(170),
                }),
                dependency_ids: vec!["helicoid_thread".to_string()],
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "lens_bore".to_string(),
                    output_id: "lens-bore-solid".to_string(),
                    target_ids: vec![edge_target_id.clone()],
                }],
                ports: Vec::new(),
            },
        ],
    });
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle with explicit film adapter graph");

    let response = handle_artifact_feature_graph_get(
        &state,
        &resolver,
        ArtifactFeatureGraphGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            model_id: Some(model_id.to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("feature graph from film adapter fixture");

    let graph = response.feature_graph.expect("feature graph");
    assert!(!graph.nodes.is_empty());
    assert_eq!(graph.nodes.len(), 4);
    let expected = [
        (
            "film_path",
            "film_path",
            "source-film-path",
            "/parts/body/film_path",
            vec![edge_target_id.clone()],
        ),
        (
            "insert_clamp",
            "insert_clamp",
            "source-insert-clamp",
            "/parts/body/insert_clamp",
            vec![face_target_id.clone()],
        ),
        (
            "helicoid_thread",
            "helicoid_thread",
            "source-helicoid-thread",
            "/parts/body/helicoid_thread",
            vec![face_target_id.clone()],
        ),
        (
            "lens_bore",
            "lens_bore",
            "source-lens-bore",
            "/parts/body/lens_bore",
            vec![edge_target_id.clone()],
        ),
    ];
    for (feature_id, kind, source_id, path, target_ids) in expected {
        let node = graph
            .nodes
            .iter()
            .find(|node| node.feature_id == feature_id)
            .expect("expected feature node");
        assert_eq!(node.kind, kind);
        let source_ref = node.source_ref.as_ref().expect("feature source ref");
        assert_eq!(source_ref.source_id.as_deref(), Some(source_id));
        assert_eq!(source_ref.path.as_deref(), Some(path));
        assert_eq!(node.output_refs.len(), 1);
        assert_eq!(node.output_refs[0].target_ids, target_ids);
        assert!(
            node.output_refs[0]
                .target_ids
                .iter()
                .all(|target_id| rendered_target_ids.contains(target_id)),
            "feature {feature_id} must anchor only rendered target ids"
        );
    }
}

#[tokio::test]
async fn artifact_feature_graph_get_reports_validation_when_runtime_manifest_missing() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-no-feature-manifest";
    let bundle = sample_bundle(model_id, "no-feature-manifest.stl");
    let stored_bundle = crate::model_runtime::write_artifact_bundle(&resolver, model_id, &bundle)
        .expect("artifact bundle");
    {
        let conn = state.db.lock().await;
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "msg-no-feature-manifest".to_string(),
                role: MessageRole::Assistant,
                content: "No manifest version".to_string(),
                status: MessageStatus::Success,
                output: Some(sample_design("No Manifest", "V-none", "none_macro()")),
                usage: None,
                artifact_bundle: Some(stored_bundle),
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now_secs() + 1,
            },
        )
        .unwrap();
    }

    let err = handle_artifact_feature_graph_get(
        &state,
        &resolver,
        ArtifactFeatureGraphGetRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-no-feature-manifest".to_string()),
            model_id: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("missing manifest should fail");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("No model manifest found"));
    assert!(err.message.contains(model_id));
    assert!(err.message.contains("artifact_feature_graph_get"));
}

#[tokio::test]
async fn structural_verification_summary_includes_artifact_digest() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-verify";
    let mut bundle = sample_bundle(model_id, "verify.stl");
    bundle.export_artifacts.push(crate::models::ExportArtifact {
        label: "STEP".to_string(),
        format: "step".to_string(),
        path: "/tmp/generated-verify.step".to_string(),
        role: "cad-exchange".to_string(),
    });
    let manifest = sample_manifest(model_id);
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");

    let response =
        handle_structural_verification_summary(&state, &resolver, "thread-1", "msg-1", model_id)
            .expect("verification summary");

    assert_eq!(response.artifact_digest.model_id, model_id);
    assert!(response.artifact_digest.has_step_export);
    assert_eq!(
        response.artifact_digest.step_export_path.as_deref(),
        Some("/tmp/generated-verify.step")
    );
}

#[tokio::test]
async fn verify_generated_model_merges_authored_verify_failure_into_structural_result() {
    let source = r#"
            (model
              (params
                (number clearance 0.2)
                (number expected_clearance 0.3))
              (verify
                (tag body_shell)
                (metric check (manifest has-step))
                (expect check (= false)))
              (part body (box clearance 10 expected_clearance)))
        "#;
    let model_id = "generated-authored-verify-fail";
    let (state, resolver) =
        seed_ecky_verify_target(source, model_id, "authored-verify-fail.stl", true).await;

    let response =
        handle_verify_generated_model(&state, &resolver, "thread-1", "msg-1", model_id, "")
            .expect("verification response");

    assert!(!response.result.passed);
    assert!(response
        .result
        .issues
        .iter()
        .any(|issue| issue.code == "AUTHORED_VERIFY_FAILED"));
    assert!(response.result.summary.contains("AUTHORED_VERIFY_FAILED"));

    // MCP-first verify-TDD: the agent must read a machine-readable delta,
    // not parse the message string. The failed check carries metric origin,
    // comparator, and expected vs actual as typed values.
    let check = response
        .result
        .authored_verify_checks
        .iter()
        .find(|check| check.tag == "body_shell")
        .expect("authored verify check for body_shell");
    assert_eq!(
        check.status,
        crate::contracts::AuthoredVerifyCheckStatus::Failed
    );
    // Clickable in the New Params map: chip stableNodeId == verify node id.
    assert_eq!(check.stable_node_id.as_deref(), Some("verify:body_shell"));
    assert_eq!(check.metric_source.as_deref(), Some("manifest"));
    assert_eq!(check.metric_key.as_deref(), Some("has-step"));
    assert_eq!(check.comparator.as_deref(), Some("="));
    assert_eq!(
        check.expected,
        Some(crate::contracts::AuthoredVerifyValue::Boolean(false))
    );
    assert!(matches!(
        check.actual,
        Some(crate::contracts::AuthoredVerifyValue::Boolean(_))
    ));
    let context = check
        .diagnostic_context
        .as_ref()
        .expect("verify diagnostic context");
    assert_eq!(context.part_key.as_deref(), Some("body"));
    assert_eq!(context.op_name.as_deref(), Some("verify:manifest/has-step"));
    let resolved_keys = context
        .resolved_params
        .iter()
        .map(|param| param.key.as_str())
        .collect::<Vec<_>>();
    assert!(resolved_keys.contains(&"clearance"));
    assert!(resolved_keys.contains(&"expected_clearance"));
    let authored_issue = response
        .result
        .issues
        .iter()
        .find(|issue| issue.code == "AUTHORED_VERIFY_FAILED")
        .expect("authored verify issue");
    assert_eq!(authored_issue.part_id.as_deref(), Some("body"));
    assert!(authored_issue.diagnostic_context.is_some());
}

#[tokio::test]
async fn verify_generated_model_surfaces_authored_verify_errors() {
    let source = r#"
            (model
              (verify
                (tag body_shell)
                (metric check (bogus has-step))
                (expect check (= true)))
              (part body (box 10 10 10)))
        "#;
    let model_id = "generated-authored-verify-error";
    let (state, resolver) =
        seed_ecky_verify_target(source, model_id, "authored-verify-error.stl", true).await;

    let response =
        handle_verify_generated_model(&state, &resolver, "thread-1", "msg-1", model_id, "")
            .expect("verification response");

    assert!(!response.result.passed);
    assert!(response
        .result
        .issues
        .iter()
        .any(|issue| issue.code == "AUTHORED_VERIFY_ERROR"));
}

#[tokio::test]
async fn structural_verification_summary_reflects_authored_verify_failures() {
    let source = r#"
            (model
              (verify
                (tag body_shell)
                (metric check (manifest has-step))
                (expect check (= false)))
              (part body (box 10 10 10)))
        "#;
    let model_id = "generated-authored-verify-summary";
    let (state, resolver) =
        seed_ecky_verify_target(source, model_id, "authored-verify-summary.stl", true).await;

    let response =
        handle_structural_verification_summary(&state, &resolver, "thread-1", "msg-1", model_id)
            .expect("verification summary");

    assert!(!response.passed);
    assert_eq!(response.issue_count, 1);
    assert!(response.summary.contains("AUTHORED_VERIFY_FAILED"));
}

#[tokio::test]
async fn printability_analyze_reads_preview_stl_and_includes_artifact_digest() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-printability";
    let preview_stl_path = resolver.root.join("printability-preview.stl");
    write_closed_tetra_binary_stl(&preview_stl_path);
    let mut bundle = sample_bundle(model_id, "printability-preview.stl");
    bundle.preview_stl_path = preview_stl_path.display().to_string();
    bundle.export_artifacts.push(crate::models::ExportArtifact {
        label: "STEP".to_string(),
        format: "step".to_string(),
        path: "/tmp/generated-printability.step".to_string(),
        role: "cad-exchange".to_string(),
    });
    let manifest = sample_manifest(model_id);
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");

    let response = handle_printability_analyze(&state, &resolver, "thread-1", "msg-1", model_id)
        .expect("printability analysis");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.message_id, "msg-1");
    assert_eq!(response.model_id, model_id);
    assert_eq!(response.artifact_digest.model_id, model_id);
    assert!(response.artifact_digest.has_step_export);
    assert_eq!(
        response.preview_stl_path,
        preview_stl_path.display().to_string()
    );
    assert_eq!(response.analysis.triangle_count, 4);
    assert_eq!(response.analysis.topology.component_count, Some(1));
    assert_eq!(response.analysis.risk_metrics.bridge_span_mm, Some(1.0));
    assert_eq!(response.analysis.risk_metrics.thin_wall_mm, Some(1.0));

    let value = serde_json::to_value(&response).expect("printability json");
    assert_eq!(value["artifactDigest"]["modelId"], model_id);
    assert_eq!(
        value["previewStlPath"],
        preview_stl_path.display().to_string()
    );
    assert_eq!(value["analysis"]["triangleCount"], 4);
    assert_eq!(value["analysis"]["topology"]["componentCount"], 1);
    assert_eq!(value["analysis"]["riskMetrics"]["bridgeSpanMm"], 1.0);
    assert_eq!(value["analysis"]["riskMetrics"]["thinWallMm"], 1.0);
}

#[tokio::test]
async fn printability_analyze_anchors_suggestions_when_feature_graph_has_one_clear_target() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-printability-anchor";
    let preview_stl_path = resolver.root.join("printability-anchor-preview.stl");
    write_binary_stl(
        &preview_stl_path,
        &[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ],
    );
    let mut bundle = sample_bundle(model_id, "printability-anchor-preview.stl");
    bundle.preview_stl_path = preview_stl_path.display().to_string();
    let mut manifest = sample_manifest(model_id);
    manifest.feature_graph = Some(crate::models::FeatureGraph {
        nodes: vec![crate::models::FeatureNode {
            feature_id: "feature-ledge".to_string(),
            kind: "extrude".to_string(),
            label: "Ledge".to_string(),
            source_ref: Some(crate::models::SourceRef {
                source_id: Some("source-main".to_string()),
                path: Some("/parts/body/ledge".to_string()),
                start_byte: Some(12),
                end_byte: Some(42),
            }),
            dependency_ids: Vec::new(),
            output_refs: vec![crate::models::FeatureOutputRef {
                feature_id: "feature-ledge".to_string(),
                output_id: "solid".to_string(),
                target_ids: vec!["body:face:0:5-5-5:100".to_string()],
            }],
            ports: Vec::new(),
        }],
    });
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");

    let response = handle_printability_analyze(&state, &resolver, "thread-1", "msg-1", model_id)
        .expect("printability analysis");

    let suggestions = &response.analysis.transform_suggestions;
    assert!(
        !suggestions.is_empty(),
        "expected transform suggestions for overhang mesh"
    );
    assert_eq!(
        response.analysis.risk_metrics.unsupported_island_count,
        Some(1)
    );
    let split_suggestion = suggestions
        .iter()
        .find(|suggestion| {
            suggestion.kind
                == crate::services::printability::PrintabilityTransformSuggestionKind::Split
        })
        .expect("split suggestion");
    assert_eq!(split_suggestion.unsupported_island_count, Some(1));
    assert!(suggestions.iter().all(|suggestion| {
        suggestion.source_anchor.as_deref()
            == Some("feature:feature-ledge@source:source-main:/parts/body/ledge:12-42")
    }));
    assert!(suggestions
        .iter()
        .all(|suggestion| suggestion
            .risk_anchor
            .as_ref()
            .is_some_and(|risk_anchor| risk_anchor.feature_id.as_deref()
                == Some("feature-ledge")
                && risk_anchor.target_ids == vec!["body:face:0:5-5-5:100".to_string()]
                && risk_anchor.stable_node_keys.is_empty())));

    let value = serde_json::to_value(&response).expect("printability json");
    assert_eq!(
        value["analysis"]["transformSuggestions"][0]["riskAnchor"]["featureId"],
        "feature-ledge"
    );
    assert_eq!(
        value["analysis"]["transformSuggestions"][0]["riskAnchor"]["targetIds"][0],
        "body:face:0:5-5-5:100"
    );
    assert!(value["analysis"]["transformSuggestions"][0]["riskAnchor"]["stableNodeKeys"].is_null());
    assert_eq!(
        value["analysis"]["riskMetrics"]["unsupportedIslandCount"],
        1
    );
    let split_suggestion_json = value["analysis"]["transformSuggestions"]
        .as_array()
        .expect("transform suggestions array")
        .iter()
        .find(|suggestion| suggestion["kind"] == "split")
        .expect("split suggestion json");
    assert_eq!(split_suggestion_json["unsupportedIslandCount"], 1);
}

#[tokio::test]
async fn printability_helicoid_fixture_analysis_and_recipes_include_risk_suggestions_and_anchors() {
    let source =
        include_str!("../../../../model-runtime/examples/film-adapter-film-gap-coupon.ecky");
    let model_id = "generated-printability-helicoid-fixture";
    let (state, resolver, _) = seed_ecky_printability_target(
        source,
        model_id,
        "printability-helicoid-fixture-preview.stl",
    )
    .await;

    let mut bundle =
        crate::model_runtime::read_artifact_bundle(&resolver, model_id).expect("runtime bundle");
    bundle.face_targets.push(crate::models::ViewerFaceTarget {
        target_id: "body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string(),
        durable_target_id: None,
        canonical_target_id: None,
        alias_ids: Vec::new(),
        part_id: "body".to_string(),
        viewer_node_id: "body".to_string(),
        label: "Body.HelicoidFace".to_string(),
        editable: true,
        center: crate::models::ViewerEdgePoint {
            x: 5.0,
            y: 5.0,
            z: 5.0,
        },
        normal: Some([0.0, 0.0, 1.0]),
        area: Some(100.0),
    });
    let mut manifest =
        crate::model_runtime::read_model_manifest(&resolver, model_id).expect("runtime manifest");
    manifest
        .selection_targets
        .push(crate::models::SelectionTarget {
            target_id: Some("body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string()),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body.HelicoidFace".to_string(),
            kind: crate::models::SelectionTargetKind::Face,
            editable: true,
            parameter_keys: Vec::new(),
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        });
    manifest.feature_graph = Some(crate::models::FeatureGraph {
        nodes: vec![crate::models::FeatureNode {
            feature_id: "feature-helicoid-thread".to_string(),
            kind: "helical-ridge".to_string(),
            label: "Helicoid Thread".to_string(),
            source_ref: Some(crate::models::SourceRef {
                source_id: Some("source-main".to_string()),
                path: Some("/parts/body/helicoid".to_string()),
                start_byte: Some(320),
                end_byte: Some(420),
            }),
            dependency_ids: Vec::new(),
            output_refs: vec![crate::models::FeatureOutputRef {
                feature_id: "feature-helicoid-thread".to_string(),
                output_id: "solid".to_string(),
                target_ids: vec!["body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string()],
            }],
            ports: Vec::new(),
        }],
    });
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle with helicoid feature graph");

    let analyze_response =
        handle_printability_analyze(&state, &resolver, "thread-1", "msg-1", model_id)
            .expect("printability analysis");
    assert!(analyze_response
            .analysis
            .transform_suggestions
            .iter()
            .any(|suggestion| {
                suggestion.kind
                    == crate::services::printability::PrintabilityTransformSuggestionKind::Split
                    || suggestion.kind
                        == crate::services::printability::PrintabilityTransformSuggestionKind::OrientationHint
            }));
    assert!(analyze_response
        .analysis
        .transform_suggestions
        .iter()
        .all(|suggestion| suggestion
            .risk_anchor
            .as_ref()
            .is_some_and(|risk_anchor| risk_anchor.feature_id.as_deref()
                == Some("feature-helicoid-thread")
                && risk_anchor.target_ids
                    == vec!["body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string()]
                && risk_anchor.stable_node_keys == vec!["body.helicoid".to_string()])));

    let analyze_json = serde_json::to_value(&analyze_response).expect("analysis json");
    assert_eq!(
        analyze_json["analysis"]["transformSuggestions"][0]["riskAnchor"]["featureId"],
        "feature-helicoid-thread"
    );
    assert_eq!(
        analyze_json["analysis"]["transformSuggestions"][0]["riskAnchor"]["stableNodeKeys"][0],
        "body.helicoid"
    );

    let recipes_response =
        handle_printability_transform_recipes_get(&state, &resolver, "thread-1", "msg-1", model_id)
            .expect("transform recipes");
    assert!(recipes_response.recipes.iter().any(|recipe| {
        recipe.action_kind
            == crate::services::printability::SupportlessFdmRecipeActionKind::Clearance
            || recipe.action_kind
                == crate::services::printability::SupportlessFdmRecipeActionKind::Reorient
    }));
    assert!(recipes_response.recipes.iter().all(|recipe| {
        recipe.risk_anchor.as_ref().is_some_and(|risk_anchor| {
            risk_anchor.feature_id.as_deref() == Some("feature-helicoid-thread")
                && risk_anchor.target_ids
                    == vec!["body:stable-node-key:body.helicoid:face:0:5-5-5:100".to_string()]
                && risk_anchor.stable_node_keys == vec!["body.helicoid".to_string()]
        })
    }));
}

#[tokio::test]
async fn printability_analyze_preserves_empty_anchor_when_feature_graph_is_ambiguous() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-printability-ambiguous-anchor";
    let preview_stl_path = resolver
        .root
        .join("printability-ambiguous-anchor-preview.stl");
    write_binary_stl(
        &preview_stl_path,
        &[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ],
    );
    let mut bundle = sample_bundle(model_id, "printability-ambiguous-anchor-preview.stl");
    bundle.preview_stl_path = preview_stl_path.display().to_string();
    let mut manifest = sample_manifest(model_id);
    manifest.feature_graph = Some(crate::models::FeatureGraph {
        nodes: vec![
            crate::models::FeatureNode {
                feature_id: "feature-left".to_string(),
                kind: "part".to_string(),
                label: "Left".to_string(),
                source_ref: None,
                dependency_ids: Vec::new(),
                output_refs: Vec::new(),
                ports: Vec::new(),
            },
            crate::models::FeatureNode {
                feature_id: "feature-right".to_string(),
                kind: "part".to_string(),
                label: "Right".to_string(),
                source_ref: None,
                dependency_ids: Vec::new(),
                output_refs: Vec::new(),
                ports: Vec::new(),
            },
        ],
    });
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");

    let response = handle_printability_analyze(&state, &resolver, "thread-1", "msg-1", model_id)
        .expect("printability analysis");

    let suggestions = response.analysis.transform_suggestions;
    assert!(
        !suggestions.is_empty(),
        "expected transform suggestions for overhang mesh"
    );
    assert!(suggestions
        .iter()
        .all(|suggestion| suggestion.source_anchor.is_none()));
}

#[tokio::test]
async fn printability_transform_recipes_get_returns_digest_guarded_overhang_recipes() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-printability-recipes";
    let preview_stl_path = resolver.root.join("printability-recipes-preview.stl");
    write_binary_stl(
        &preview_stl_path,
        &[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ],
    );
    let mut bundle = sample_bundle(model_id, "printability-recipes-preview.stl");
    bundle.preview_stl_path = preview_stl_path.display().to_string();
    let mut manifest = sample_manifest(model_id);
    manifest.feature_graph = Some(crate::models::FeatureGraph {
        nodes: vec![crate::models::FeatureNode {
            feature_id: "feature-ledge".to_string(),
            kind: "extrude".to_string(),
            label: "Ledge".to_string(),
            source_ref: None,
            dependency_ids: Vec::new(),
            output_refs: vec![crate::models::FeatureOutputRef {
                feature_id: "feature-ledge".to_string(),
                output_id: "solid".to_string(),
                target_ids: vec!["body:face:0:5-5-5:100".to_string()],
            }],
            ports: Vec::new(),
        }],
    });
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");

    let response =
        handle_printability_transform_recipes_get(&state, &resolver, "thread-1", "msg-1", model_id)
            .expect("transform recipes");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.message_id, "msg-1");
    assert_eq!(response.model_id, model_id);
    assert_eq!(response.artifact_digest.model_id, model_id);
    assert_eq!(
        response.preview_stl_path,
        preview_stl_path.display().to_string()
    );
    let recipe = response
        .recipes
        .iter()
        .find(|recipe| {
            recipe.action_kind
                == crate::services::printability::SupportlessFdmRecipeActionKind::Reorient
        })
        .expect("reorient recipe");
    assert_eq!(
        recipe.source_anchor.as_deref(),
        Some("feature:feature-ledge")
    );
    assert_eq!(recipe.target.as_deref(), Some("rotateX270"));
    assert_eq!(
        recipe.preview_support_status,
        crate::services::printability::TransformRecipeSupportStatus::Pending
    );
    assert_eq!(
        recipe.apply_support_status,
        crate::services::printability::TransformRecipeSupportStatus::Unsupported
    );
    assert!(recipe
        .risk_anchor
        .as_ref()
        .is_some_and(
            |risk_anchor| risk_anchor.feature_id.as_deref() == Some("feature-ledge")
                && risk_anchor.target_ids == vec!["body:face:0:5-5-5:100".to_string()]
                && risk_anchor.stable_node_keys.is_empty()
        ));
    assert!(response.recipes.iter().any(|recipe| {
        recipe.action_kind == crate::services::printability::SupportlessFdmRecipeActionKind::Relief
    }));
    let clearance = response
        .recipes
        .iter()
        .find(|recipe| {
            recipe.action_kind
                == crate::services::printability::SupportlessFdmRecipeActionKind::Clearance
        })
        .expect("clearance recipe");
    assert_eq!(clearance.bridge_span_mm, Some(1.0));
    assert_eq!(clearance.thin_wall_mm, Some(1.0));
    assert_eq!(clearance.unsupported_island_count, Some(1));

    let value = serde_json::to_value(&response).expect("recipes json");
    assert_eq!(value["artifactDigest"]["modelId"], model_id);
    assert_eq!(
        value["artifactDigest"]["contentHash"],
        format!("hash-{model_id}")
    );
    assert_eq!(value["recipes"][0]["previewSupportStatus"], "pending");
    assert_eq!(value["recipes"][0]["applySupportStatus"], "unsupported");
    assert_eq!(
        value["recipes"][0]["riskAnchor"]["featureId"],
        "feature-ledge"
    );
    let clearance_json = value["recipes"]
        .as_array()
        .expect("recipes array")
        .iter()
        .find(|recipe| recipe["actionKind"] == "clearance")
        .expect("clearance recipe json");
    assert_eq!(clearance_json["bridgeSpanMm"], 1.0);
    assert_eq!(clearance_json["thinWallMm"], 1.0);
    assert_eq!(clearance_json["unsupportedIslandCount"], 1);
}

#[tokio::test]
async fn printability_transform_recipes_get_returns_empty_for_no_risk_stl() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-printability-no-risk-recipes";
    let preview_stl_path = resolver.root.join("printability-no-risk-preview.stl");
    // A unit tetra reads as a 1.00 mm thin wall (below the 1.20 mm
    // advisory); scale it up so the mesh is genuinely risk-free.
    let triangles = [
        [[0.0f32, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
        [[0.0f32, 0.0, 0.0], [0.0, 0.0, 10.0], [10.0, 0.0, 0.0]],
        [[0.0f32, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
        [[10.0f32, 0.0, 0.0], [0.0, 0.0, 10.0], [0.0, 10.0, 0.0]],
    ];
    write_binary_stl(&preview_stl_path, &triangles);
    let mut bundle = sample_bundle(model_id, "printability-no-risk-preview.stl");
    bundle.preview_stl_path = preview_stl_path.display().to_string();
    let manifest = sample_manifest(model_id);
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");

    let response =
        handle_printability_transform_recipes_get(&state, &resolver, "thread-1", "msg-1", model_id)
            .expect("transform recipes");

    assert!(response.recipes.is_empty(), "{:?}", response.recipes);
}

#[tokio::test]
async fn printability_transform_recipes_get_reports_missing_preview_stl() {
    let (state, resolver) = seed_target().await;
    let model_id = "generated-printability-missing-preview";
    let mut bundle = sample_bundle(model_id, "missing-preview.stl");
    bundle.preview_stl_path.clear();
    let manifest = sample_manifest(model_id);
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");

    let err =
        handle_printability_transform_recipes_get(&state, &resolver, "thread-1", "msg-1", model_id)
            .expect_err("missing preview STL should fail");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert_eq!(err.message, "Artifact bundle has no preview STL path.");
}

async fn seed_ecky_printability_target(
    source: &str,
    model_id: &str,
    preview_name: &str,
) -> (AppState, TestPathResolver, SemanticTransformArtifactGuard) {
    let (state, resolver) = seed_target_with_macro("Ecky Pot", "V-ecky", source).await;
    let preview_stl_path = resolver.root.join(preview_name);
    write_binary_stl(
        &preview_stl_path,
        &[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ],
    );
    let source_path = resolver.root.join(format!("{model_id}.ecky"));
    fs::write(&source_path, source).expect("write ecky source");

    let mut design = sample_design("Ecky Pot", "V-ecky", source);
    design.macro_dialect = MacroDialect::EckyIrV0;
    design.engine_kind = crate::models::EngineKind::EckyIrV0;
    design.geometry_backend = crate::models::GeometryBackend::EckyRust;
    design.source_language = crate::models::SourceLanguage::EckyIrV0;
    design.post_processing = None;

    let mut bundle = sample_bundle(model_id, preview_name);
    bundle.engine_kind = crate::models::EngineKind::EckyIrV0;
    bundle.geometry_backend = crate::models::GeometryBackend::EckyRust;
    bundle.source_language = crate::models::SourceLanguage::EckyIrV0;
    bundle.content_hash = format!("content-{model_id}");
    bundle.macro_path = Some(source_path.display().to_string());
    bundle.preview_stl_path = preview_stl_path.display().to_string();

    let mut manifest = sample_manifest(model_id);
    manifest.engine_kind = crate::models::EngineKind::EckyIrV0;
    manifest.geometry_backend = crate::models::GeometryBackend::EckyRust;
    manifest.source_language = crate::models::SourceLanguage::EckyIrV0;
    manifest.source_digest = Some(crate::mcp::macro_buffer::source_digest(source));

    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");
    {
        let conn = state.db.lock().await;
        conn.execute(
                "UPDATE messages SET output = ?1, artifact_bundle = ?2, model_manifest = ?3 WHERE id = 'msg-1'",
                rusqlite::params![
                    serde_json::to_string(&design).expect("design json"),
                    serde_json::to_string(&bundle).expect("bundle json"),
                    serde_json::to_string(&manifest).expect("manifest json"),
                ],
            )
            .expect("update ecky target");
    }

    let guard = SemanticTransformArtifactGuard {
        model_id: model_id.to_string(),
        preview_stl_path: bundle.preview_stl_path.clone(),
        content_hash: bundle.content_hash.clone(),
    };
    (state, resolver, guard)
}

#[tokio::test]
async fn semantic_transform_preview_reorient_recipe_creates_preview_draft_without_committed_message(
) {
    let source = "(model (part body (box 10 20 30)))";
    let (state, resolver, expected_artifact) = seed_ecky_printability_target(
        source,
        "generated-semantic-reorient",
        "semantic-reorient.stl",
    )
    .await;

    let response = handle_semantic_transform_preview(
        &state,
        &resolver,
        SemanticTransformPreviewRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            model_id: Some("generated-semantic-reorient".to_string()),
            recipe_id: "supportless-fdm-orientation-best".to_string(),
            action_kind: crate::services::printability::SupportlessFdmRecipeActionKind::Reorient,
            expected_artifact,
        },
        &test_ctx(),
    )
    .await
    .expect("semantic reorient preview");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.base_message_id, "msg-1");
    assert_eq!(response.model_id, response.artifact_digest.model_id);
    let rendered_bundle = crate::model_runtime::read_artifact_bundle(&resolver, &response.model_id)
        .expect("rendered runtime bundle");
    assert_eq!(
        response.artifact_digest.content_hash,
        rendered_bundle.content_hash
    );
    assert_eq!(response.recipe_id, "supportless-fdm-orientation-best");
    assert_eq!(
        response.action_kind,
        crate::services::printability::SupportlessFdmRecipeActionKind::Reorient
    );
    assert_eq!(
        response.preview_support_status,
        crate::services::printability::TransformRecipeSupportStatus::Pending
    );
    assert_eq!(
        response.apply_support_status,
        crate::services::printability::TransformRecipeSupportStatus::Unsupported
    );
    assert_eq!(
        response.source_digest,
        crate::mcp::macro_buffer::source_digest(source)
    );
    assert_ne!(response.source_digest, response.new_source_digest);

    let draft = {
        let conn = state.db.lock().await;
        let committed_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE id = ?1",
                [&response.preview_id],
                |row| row.get(0),
            )
            .expect("message count");
        assert_eq!(committed_count, 0);
        db::get_agent_draft_for_session(&conn, &test_ctx().session_id)
            .expect("draft query")
            .expect("draft")
    };
    assert_eq!(draft.preview_id, response.preview_id);
    assert!(draft.design_output.macro_code.contains("(rotate 270 0 0"));
    assert!(draft.design_output.macro_code.contains("(part body"));
}

#[tokio::test]
async fn semantic_transform_preview_stale_model_id_or_preview_stl_guard_rejects_because_digest_lacks_preview_path(
) {
    let (state, resolver, mut expected_artifact) = seed_ecky_printability_target(
        "(model (part body (box 10 20 30)))",
        "generated-semantic-stale",
        "semantic-stale.stl",
    )
    .await;
    expected_artifact.preview_stl_path = "/tmp/stale-preview.stl".to_string();

    let err = handle_semantic_transform_preview(
        &state,
        &resolver,
        SemanticTransformPreviewRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            model_id: Some("generated-semantic-stale".to_string()),
            recipe_id: "supportless-fdm-orientation-best".to_string(),
            action_kind: crate::services::printability::SupportlessFdmRecipeActionKind::Reorient,
            expected_artifact,
        },
        &test_ctx(),
    )
    .await
    .expect_err("stale artifact guard should fail");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("artifact guard mismatch"));
}

#[tokio::test]
async fn semantic_transform_preview_missing_content_hash_guard_rejects_at_request_boundary() {
    let req = serde_json::json!({
        "threadId": "thread-1",
        "messageId": "msg-1",
        "modelId": "generated-semantic-missing-hash",
        "recipeId": "supportless-fdm-orientation-best",
        "actionKind": "reorient",
        "expectedArtifact": {
            "modelId": "generated-semantic-missing-hash",
            "previewStlPath": "/tmp/semantic-missing-hash.stl"
        }
    });

    let err = serde_json::from_value::<SemanticTransformPreviewRequest>(req)
        .expect_err("missing contentHash should fail deserialization");

    assert!(err.to_string().contains("contentHash"));
}

#[tokio::test]
async fn semantic_transform_preview_stale_content_hash_guard_rejects() {
    let (state, resolver, mut expected_artifact) = seed_ecky_printability_target(
        "(model (part body (box 10 20 30)))",
        "generated-semantic-stale-hash",
        "semantic-stale-hash.stl",
    )
    .await;
    expected_artifact.content_hash = "stale-content-hash".to_string();

    let err = handle_semantic_transform_preview(
        &state,
        &resolver,
        SemanticTransformPreviewRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            model_id: Some("generated-semantic-stale-hash".to_string()),
            recipe_id: "supportless-fdm-orientation-best".to_string(),
            action_kind: crate::services::printability::SupportlessFdmRecipeActionKind::Reorient,
            expected_artifact,
        },
        &test_ctx(),
    )
    .await
    .expect_err("stale contentHash should fail");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("artifact guard mismatch"));
    assert!(err.message.contains("contentHash"));
}

#[tokio::test]
async fn semantic_transform_preview_unsupported_actions_return_explicit_validation_errors() {
    let (state, resolver, expected_artifact) = seed_ecky_printability_target(
        "(model (part body (box 10 20 30)))",
        "generated-semantic-unsupported",
        "semantic-unsupported.stl",
    )
    .await;

    for action_kind in [
        crate::services::printability::SupportlessFdmRecipeActionKind::Chamfer,
        crate::services::printability::SupportlessFdmRecipeActionKind::Split,
        crate::services::printability::SupportlessFdmRecipeActionKind::Relief,
        crate::services::printability::SupportlessFdmRecipeActionKind::Clearance,
    ] {
        let err = handle_semantic_transform_preview(
            &state,
            &resolver,
            SemanticTransformPreviewRequest {
                identity: AgentIdentityOverride::default(),
                thread_id: Some("thread-1".to_string()),
                message_id: Some("msg-1".to_string()),
                model_id: Some("generated-semantic-unsupported".to_string()),
                recipe_id: "supportless-fdm-unsupported".to_string(),
                action_kind,
                expected_artifact: expected_artifact.clone(),
            },
            &test_ctx(),
        )
        .await
        .expect_err("unsupported transform should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("unsupported"));
        assert!(err.message.contains(match action_kind {
            crate::services::printability::SupportlessFdmRecipeActionKind::Chamfer => "chamfer",
            crate::services::printability::SupportlessFdmRecipeActionKind::Split => "split",
            crate::services::printability::SupportlessFdmRecipeActionKind::Relief => "relief",
            crate::services::printability::SupportlessFdmRecipeActionKind::Clearance => "clearance",
            crate::services::printability::SupportlessFdmRecipeActionKind::Reorient =>
                unreachable!(),
        }));
    }
}

#[tokio::test]
async fn semantic_transform_preview_non_ecky_source_is_unsupported() {
    let (state, resolver) = seed_target().await;
    let preview_stl_path = resolver.root.join("semantic-legacy.stl");
    write_binary_stl(
        &preview_stl_path,
        &[
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, 2.0], [0.0, 1.0, 2.0], [1.0, 0.0, 2.0]],
        ],
    );
    let model_id = "generated-semantic-legacy";
    let mut bundle = sample_bundle(model_id, "semantic-legacy.stl");
    bundle.preview_stl_path = preview_stl_path.display().to_string();
    let manifest = sample_manifest(model_id);
    crate::model_runtime::write_runtime_bundle(&resolver, model_id, &bundle, &manifest)
        .expect("runtime bundle");

    let err = handle_semantic_transform_preview(
        &state,
        &resolver,
        SemanticTransformPreviewRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            model_id: Some(model_id.to_string()),
            recipe_id: "supportless-fdm-orientation-best".to_string(),
            action_kind: crate::services::printability::SupportlessFdmRecipeActionKind::Reorient,
            expected_artifact: SemanticTransformArtifactGuard {
                model_id: model_id.to_string(),
                preview_stl_path: bundle.preview_stl_path.clone(),
                content_hash: bundle.content_hash.clone(),
            },
        },
        &test_ctx(),
    )
    .await
    .expect_err("non-Ecky source should fail");

    assert_eq!(err.code, AppErrorCode::Validation);
    assert!(err.message.contains("sourceLanguage=ecky"));
}

#[tokio::test]
async fn given_durable_preview_feedback_when_latest_draft_requested_then_response_restores_feedback(
) {
    let (state, resolver) = seed_target().await;
    let ctx = test_ctx();
    let preview_stl_path = resolver.root.join("preview-pass.stl");
    write_closed_tetra_binary_stl(&preview_stl_path);

    let mut preview_bundle = sample_bundle("model-preview-pass", "preview-pass.stl");
    preview_bundle.preview_stl_path = preview_stl_path.display().to_string();
    let preview = store_session_render_preview(
        &state,
        &resolver,
        &ctx,
        StoreSessionRenderPreviewRequest {
            thread_id: "thread-1".to_string(),
            base_message_id: Some("msg-1".to_string()),
            design_output: sample_design("Preview Pass", "", "preview_pass_macro()"),
            artifact_bundle: preview_bundle,
            model_manifest: sample_manifest("model-preview-pass"),
            draft_feedback: None,
        },
    )
    .await
    .expect("store preview");
    assert_eq!(
        preview
            .draft_feedback
            .as_ref()
            .expect("preview feedback")
            .status,
        crate::models::AgentDraftFeedbackStatus::Passed
    );

    clear_session_render_preview(&ctx.session_id);

    let response = handle_target_detail_get(
        &state,
        &resolver,
        TargetDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            section: TargetDetailSection::LatestDraft,
            shape_graph_filters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("target draft detail");

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["section"], "latestDraft");
    assert!(value.get("latestDraft").is_some());
    assert_eq!(value["latestDraft"]["previewId"], preview.preview_id);
    assert_eq!(value["latestDraft"]["draftFeedback"]["status"], "passed");
    assert_eq!(
        value["latestDraft"]["draftFeedback"]["source"],
        "structuralVerification"
    );
    assert!(value.get("uiSpec").is_none());
    assert!(value.get("initialParams").is_none());
    assert!(value.get("artifactBundle").is_none());
}

#[tokio::test]
async fn target_detail_get_returns_latest_draft_null_when_absent() {
    let (state, resolver) = seed_target().await;
    let response = handle_target_detail_get(
        &state,
        &resolver,
        TargetDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            section: TargetDetailSection::LatestDraft,
            shape_graph_filters: None,
        },
        &test_ctx(),
    )
    .await
    .expect("target null draft detail");

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["section"], "latestDraft");
    assert!(value.get("latestDraft").is_some());
    assert!(value["latestDraft"].is_null());
}

#[tokio::test]
async fn given_preview_render_when_commit_runs_then_history_gets_one_version() {
    let (state, resolver) = seed_target().await;
    let ctx = test_ctx();
    let initial_count = {
        let conn = state.db.lock().await;
        db::get_thread_messages(&conn, "thread-1").unwrap().len()
    };
    let preview_design = sample_design("Preview Pot", "", "preview_macro()");
    let preview_bundle = sample_bundle("model-preview", "preview.stl");
    let preview_manifest = sample_manifest("model-preview");

    let preview = store_session_render_preview(
        &state,
        &resolver,
        &ctx,
        StoreSessionRenderPreviewRequest {
            thread_id: "thread-1".to_string(),
            base_message_id: Some("msg-1".to_string()),
            design_output: preview_design.clone(),
            artifact_bundle: preview_bundle.clone(),
            model_manifest: preview_manifest.clone(),
            draft_feedback: None,
        },
    )
    .await
    .expect("store preview");

    {
        let conn = state.db.lock().await;
        assert_eq!(
            db::get_thread_messages(&conn, "thread-1").unwrap().len(),
            initial_count
        );
    }
    assert_eq!(
        session_render_preview_for_request(
            &ctx,
            Some("thread-1"),
            Some(preview.preview_id.as_str())
        )
        .expect("session preview")
        .design_output
        .macro_code,
        "preview_macro()"
    );

    let response = handle_commit_preview_version(
        &state,
        &resolver,
        VersionSaveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some(preview.preview_id.clone()),
            title: Some("Committed Pot".to_string()),
            version_name: Some("V-preview".to_string()),
        },
        &ctx,
    )
    .await
    .expect("commit preview");

    {
        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").unwrap();
        assert_eq!(messages.len(), initial_count + 1);
        let committed = messages
            .iter()
            .find(|message| message.id == response.message_id)
            .expect("committed message");
        assert_eq!(
            committed.output.as_ref().unwrap().macro_code,
            "preview_macro()"
        );
        assert_eq!(committed.output.as_ref().unwrap().version_name, "V-preview");
    }
    assert!(session_render_preview_for_request(
        &ctx,
        Some("thread-1"),
        Some(preview.preview_id.as_str())
    )
    .is_none());
}

#[tokio::test]
async fn given_preview_render_when_session_memory_clears_then_commit_by_preview_id_uses_durable_draft(
) {
    let (state, resolver) = seed_target().await;
    let ctx = test_ctx();
    let initial_count = {
        let conn = state.db.lock().await;
        db::get_thread_messages(&conn, "thread-1").unwrap().len()
    };

    let preview = store_session_render_preview(
        &state,
        &resolver,
        &ctx,
        StoreSessionRenderPreviewRequest {
            thread_id: "thread-1".to_string(),
            base_message_id: Some("msg-1".to_string()),
            design_output: sample_design("Durable Pot", "", "durable_preview_macro()"),
            artifact_bundle: sample_bundle("model-durable-preview", "durable-preview.stl"),
            model_manifest: sample_manifest("model-durable-preview"),
            draft_feedback: Some(DraftFeedbackSeed {
                status: crate::models::AgentDraftFeedbackStatus::Failed,
                summary: "Draft failed structural verification.".to_string(),
                items: vec![crate::models::AgentDraftFeedbackItem {
                    code: "non_manifold".to_string(),
                    message: "Mesh contains a non-manifold edge.".to_string(),
                }],
                authoring_lints: Vec::new(),
                source: crate::models::AgentDraftFeedbackSource::StructuralVerification,
            }),
        },
    )
    .await
    .expect("store preview");
    assert_eq!(
        preview
            .draft_feedback
            .as_ref()
            .expect("draft feedback")
            .summary,
        "Draft failed structural verification."
    );

    clear_session_render_preview(&ctx.session_id);
    assert!(session_render_preview_for_request(
        &ctx,
        Some("thread-1"),
        Some(preview.preview_id.as_str())
    )
    .is_none());
    let restored = resolve_session_render_preview_for_request(
        &state,
        &ctx,
        Some("thread-1"),
        Some(preview.preview_id.as_str()),
    )
    .await
    .expect("resolve durable preview")
    .expect("durable preview restored");
    assert_eq!(
        restored
            .draft_feedback
            .as_ref()
            .expect("restored draft feedback")
            .summary,
        "Draft failed structural verification."
    );

    let response = handle_commit_preview_version(
        &state,
        &resolver,
        VersionSaveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some(preview.preview_id.clone()),
            title: None,
            version_name: Some("V-durable".to_string()),
        },
        &ctx,
    )
    .await
    .expect("commit durable preview");

    let conn = state.db.lock().await;
    let messages = db::get_thread_messages(&conn, "thread-1").unwrap();
    assert_eq!(messages.len(), initial_count + 1);
    let committed = messages
        .iter()
        .find(|message| message.id == response.message_id)
        .expect("committed message");
    assert_eq!(
        committed.output.as_ref().unwrap().macro_code,
        "durable_preview_macro()"
    );
    assert_eq!(committed.output.as_ref().unwrap().version_name, "V-durable");
    assert!(db::get_agent_draft_for_session(&conn, &ctx.session_id)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn measurement_annotation_save_persists_semantic_annotation_in_new_version() {
    let (state, resolver) = seed_target().await;
    let response = handle_measurement_annotation_save(
        &state,
        &resolver,
        MeasurementAnnotationSaveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            annotation: MeasurementAnnotation {
                annotation_id: "measurement-outer-diameter".to_string(),
                label: "Outer Diameter".to_string(),
                basis: MeasurementBasis::Outer,
                axis: MeasurementAxis::X,
                parameter_keys: vec!["diameter".to_string()],
                primitive_ids: vec!["diameter".to_string()],
                target_ids: Vec::new(),
                guide_id: None,
                explanation: Some("Measures the outside width.".to_string()),
                formula_hint: None,
                source: MeasurementAnnotationSource::Manual,
            },
            title: None,
            version_name: Some("V-mcp-measurement".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("measurement annotation save");

    assert_eq!(response.version_name, "V-mcp-measurement");
    assert_eq!(response.measurement_annotation_count, 1);
    assert_eq!(response.artifact_digest.model_id, "model-base");
    let value = serde_json::to_value(&response).expect("semantic mutation json");
    assert!(value.get("artifactBundle").is_none());
    assert!(value.get("modelManifest").is_none());
    let detail = handle_semantic_manifest_detail_get(
        &state,
        &resolver,
        SemanticManifestDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(response.thread_id.clone()),
            message_id: Some(response.message_id.clone()),
            section: SemanticManifestSection::MeasurementAnnotations,
        },
        &test_ctx(),
    )
    .await
    .expect("measurement detail");
    let annotations = detail
        .measurement_annotations
        .expect("measurement annotations");
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].source, MeasurementAnnotationSource::Llm);
    assert_eq!(annotations[0].annotation_id, "measurement-outer-diameter");
}

#[tokio::test]
async fn semantic_manifest_get_includes_measurement_annotations() {
    let (state, resolver) = seed_target().await;

    let created = handle_measurement_annotation_save(
        &state,
        &resolver,
        MeasurementAnnotationSaveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            annotation: MeasurementAnnotation {
                annotation_id: "measurement-inner-width".to_string(),
                label: "Inner Width".to_string(),
                basis: MeasurementBasis::Inner,
                axis: MeasurementAxis::X,
                parameter_keys: vec!["diameter".to_string()],
                primitive_ids: vec!["diameter".to_string()],
                target_ids: Vec::new(),
                guide_id: None,
                explanation: Some("Measures the inner cavity span.".to_string()),
                formula_hint: None,
                source: MeasurementAnnotationSource::Manual,
            },
            title: None,
            version_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("seed measurement annotation");

    let response = handle_semantic_manifest_get(
        &state,
        &resolver,
        SemanticManifestRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(created.thread_id.clone()),
            message_id: Some(created.message_id.clone()),
        },
        &test_ctx(),
    )
    .await
    .expect("semantic manifest with measurements");

    assert_eq!(response.measurement_annotation_count, 1);
}

#[tokio::test]
async fn measurement_annotation_delete_removes_existing_annotation() {
    let (state, resolver) = seed_target().await;

    let created = handle_measurement_annotation_save(
        &state,
        &resolver,
        MeasurementAnnotationSaveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            annotation: MeasurementAnnotation {
                annotation_id: "measurement-wall".to_string(),
                label: "Wall Thickness".to_string(),
                basis: MeasurementBasis::Wall,
                axis: MeasurementAxis::Normal,
                parameter_keys: vec!["diameter".to_string()],
                primitive_ids: vec!["diameter".to_string()],
                target_ids: Vec::new(),
                guide_id: None,
                explanation: None,
                formula_hint: None,
                source: MeasurementAnnotationSource::Manual,
            },
            title: None,
            version_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("seed annotation");

    let deleted = handle_measurement_annotation_delete(
        &state,
        &resolver,
        MeasurementAnnotationDeleteRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(created.thread_id.clone()),
            message_id: Some(created.message_id.clone()),
            annotation_id: "measurement-wall".to_string(),
            title: None,
            version_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("delete annotation");

    assert_eq!(deleted.measurement_annotation_count, 0);
    let detail = handle_semantic_manifest_detail_get(
        &state,
        &resolver,
        SemanticManifestDetailRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(deleted.thread_id.clone()),
            message_id: Some(deleted.message_id.clone()),
            section: SemanticManifestSection::MeasurementAnnotations,
        },
        &test_ctx(),
    )
    .await
    .expect("measurement detail after delete");
    assert!(detail
        .measurement_annotations
        .expect("measurement annotations")
        .is_empty());
}

#[tokio::test]
async fn session_reply_save_persists_final_reply_to_thread_history_and_logs() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;

    {
        let conn = state.db.lock().await;
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "user-working-1".to_string(),
                role: MessageRole::User,
                content: "Please adjust the frame.".to_string(),
                status: MessageStatus::Working,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now_secs(),
            },
        )
        .unwrap();
    }
    {
        let mut sessions = state.mcp_sessions.lock().await;
        let session = sessions.get_mut(&test_session_id()).expect("live session");
        session.current_turn_id = Some("turn-1".to_string());
        session.current_turn_thread_id = Some("thread-1".to_string());
        session.current_turn_working_message_ids = vec!["user-working-1".to_string()];
    }

    let response = handle_session_reply_save(
        &state,
        SessionReplySaveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            body: "Saved in the current pot frame thread.".to_string(),
            fatal: false,
        },
        &test_ctx(),
    )
    .await
    .expect("session reply save");

    assert_eq!(response.thread_id, "thread-1");

    let messages = {
        let conn = state.db.lock().await;
        db::get_thread_messages(&conn, "thread-1").expect("messages")
    };
    let saved = messages
        .iter()
        .find(|message| message.id == response.message_id)
        .expect("saved reply");
    assert_eq!(saved.content, "Saved in the current pot frame thread.");
    assert_eq!(saved.role, MessageRole::Assistant);
    assert_eq!(
        saved
            .agent_origin
            .as_ref()
            .map(|origin| origin.session_id.as_str()),
        Some(test_session_id().as_str())
    );

    let working_message = messages
        .iter()
        .find(|message| message.id == "user-working-1")
        .expect("working user message");
    assert_eq!(working_message.status, MessageStatus::Success);
    let live_session = state
        .mcp_sessions
        .lock()
        .await
        .get(&test_session_id())
        .cloned()
        .expect("live session");
    assert!(live_session.current_turn_working_message_ids.is_empty());

    let logs = state.app_logs.lock().unwrap();
    let last = logs.back().expect("log entry");
    assert!(last.message.contains("kind=final_reply_save"));
    assert!(last
        .message
        .contains("Saved in the current pot frame thread."));
}

#[tokio::test]
async fn long_action_notice_updates_live_session_and_logs() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;

    let response = handle_long_action_notice(
        &state,
        LongActionNoticeRequest {
            identity: AgentIdentityOverride::default(),
            message: "Developing the next iteration".to_string(),
            phase: Some("working".to_string()),
            details: Some("connector placement pass".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("long action notice");

    assert_eq!(response.phase, "working");
    assert!(response.busy);
    assert_eq!(response.activity_label, "Developing the next iteration");

    let live_session = state
        .mcp_sessions
        .lock()
        .await
        .get(&test_session_id())
        .cloned()
        .expect("live session");
    assert!(live_session.busy);
    assert_eq!(
        live_session.activity_label.as_deref(),
        Some("Developing the next iteration")
    );
    assert_eq!(live_session.phase.as_deref(), Some("working"));

    let logs = state.app_logs.lock().unwrap();
    let last = logs.back().expect("log entry");
    assert!(last.message.contains("kind=session_activity_set"));
    assert!(last.message.contains("connector placement pass"));
}

#[tokio::test]
async fn long_action_clear_resets_live_session_busy_state() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;
    handle_long_action_notice(
        &state,
        LongActionNoticeRequest {
            identity: AgentIdentityOverride::default(),
            message: "Developing the next iteration".to_string(),
            phase: Some("working".to_string()),
            details: None,
        },
        &test_ctx(),
    )
    .await
    .expect("seed long action");

    let response = handle_long_action_clear(
        &state,
        LongActionClearRequest {
            identity: AgentIdentityOverride::default(),
            phase: Some("idle".to_string()),
            status_text: Some("Ready for the next queued message.".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("long action clear");

    assert_eq!(response.phase, "idle");
    assert!(!response.busy);

    let live_session = state
        .mcp_sessions
        .lock()
        .await
        .get(&test_session_id())
        .cloned()
        .expect("live session");
    assert!(!live_session.busy);
    assert_eq!(live_session.activity_label, None);
    assert_eq!(live_session.phase.as_deref(), Some("idle"));
    assert_eq!(
        live_session.status_text.as_deref(),
        Some("Ready for the next queued message.")
    );
}

#[tokio::test]
async fn mark_as_read_drains_pending_thread_batch_and_sets_session_working() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;
    let now = now_secs();

    {
        let conn = state.db.lock().await;
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "user-pending-1".to_string(),
                role: MessageRole::User,
                content: "Please thin the lip.".to_string(),
                status: MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now,
            },
        )
        .unwrap();
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "user-pending-2".to_string(),
                role: MessageRole::User,
                content: "Also widen the top opening.".to_string(),
                status: MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: now,
            },
        )
        .unwrap();
        persist_agent_session(
            &conn,
            &test_ctx(),
            Some("thread-1".to_string()),
            Some("msg-1".to_string()),
            Some("model-base".to_string()),
            "idle",
            "Agent joined the workspace.",
        )
        .unwrap();
    }

    let response = handle_mark_as_read(
        &state,
        MarkAsReadRequest {
            message_id: "user-pending-1".to_string(),
            thread_id: Some("thread-1".to_string()),
            identity: AgentIdentityOverride::default(),
        },
        &test_ctx(),
    )
    .await
    .expect("mark_as_read");

    assert_eq!(response.thread_id, "thread-1");
    assert_eq!(response.message_id, "user-pending-1");
    assert_eq!(
        response.message_ids,
        vec!["user-pending-1".to_string(), "user-pending-2".to_string()]
    );
    assert_eq!(response.status, "working");

    let conn = state.db.lock().await;
    let messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
    let statuses = messages
        .into_iter()
        .filter(|message| message.role == MessageRole::User)
        .map(|message| (message.id, message.status))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        statuses.get("user-pending-1"),
        Some(&MessageStatus::Working)
    );
    assert_eq!(
        statuses.get("user-pending-2"),
        Some(&MessageStatus::Working)
    );
    let sessions = db::get_sessions_by_ids(&conn, &[test_session_id()]).expect("sessions");
    assert_eq!(sessions[0].phase, "working");
    assert_eq!(sessions[0].message_id.as_deref(), Some("user-pending-1"));
}

#[tokio::test]
async fn session_log_out_removes_live_session_and_hides_it_from_active_sessions() {
    let (state, _resolver) = seed_target().await;
    seed_live_session(&state).await;

    {
        let conn = state.db.lock().await;
        persist_agent_session(
            &conn,
            &test_ctx(),
            Some("thread-1".to_string()),
            Some("msg-1".to_string()),
            Some("model-base".to_string()),
            "idle",
            "Agent joined the workspace.",
        )
        .unwrap();
    }

    handle_session_log_out(
        &state,
        SessionLogoutRequest {
            identity: AgentIdentityOverride::default(),
        },
        &test_ctx(),
    )
    .await
    .expect("session_log_out");

    assert!(state
        .mcp_sessions
        .lock()
        .await
        .get(&test_session_id())
        .is_none());

    let conn = state.db.lock().await;
    let stored = db::get_sessions_by_ids(&conn, &[test_session_id()]).expect("stored");
    assert_eq!(stored[0].phase, "disconnected");
    let active = db::get_active_agent_sessions(&conn, 600).expect("active sessions");
    assert!(active
        .into_iter()
        .all(|session| session.session_id != test_session_id()));
}

#[test]
fn macro_buffer_replaces_line_range_with_digest_guard() {
    let source = "(model\n  (part body (box 1 1 1))\n)\n";
    let digest = macro_buffer_digest(source);
    let patched = apply_macro_buffer_replacements(
        source,
        &digest,
        &[MacroBufferReplacement {
            start_line: 2,
            end_line: 2,
            new_text: "  (part body (box 2 2 2))".to_string(),
        }],
    )
    .expect("patched macro");

    assert_eq!(patched, "(model\n  (part body (box 2 2 2))\n)\n");
}

#[test]
fn macro_buffer_edit_response_omits_full_macro_code() {
    let response = MacroBufferEditResponse {
        digest: "digest".to_string(),
        line_count: 2,
        window_start_line: 1,
        window_end_line: 2,
        truncated: false,
        lines: vec![
            MacroBufferLine {
                line_number: 1,
                text: "(model".to_string(),
            },
            MacroBufferLine {
                line_number: 2,
                text: ")".to_string(),
            },
        ],
    };

    let value = serde_json::to_value(response).expect("edit response json");
    assert!(value.get("macroCode").is_none());
    assert_eq!(value["windowStartLine"], 1);
    assert_eq!(value["windowEndLine"], 2);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["lines"].as_array().expect("lines").len(), 2);
}

#[test]
fn macro_buffer_rejects_stale_digest() {
    let err = apply_macro_buffer_replacements(
        "(model)\n",
        "stale",
        &[MacroBufferReplacement {
            start_line: 1,
            end_line: 1,
            new_text: "(model\n)".to_string(),
        }],
    )
    .expect_err("stale digest should fail");

    assert!(err.message.contains("Macro buffer digest mismatch"));
}

#[tokio::test]
async fn project_folder_export_edit_apply_commits_new_version() {
    let (state, resolver) =
        seed_target_with_macro("Bracket", "V-base", "(model (part body (box 10 10 5)))").await;

    let export = handle_project_folder_export(
        &state,
        &resolver,
        ProjectFolderExportRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            slug: None,
        },
        &test_ctx(),
    )
    .await
    .expect("export");
    assert!(export.slug.starts_with("bracket-"), "{}", export.slug);
    assert_eq!(export.manifest.thread_id, "thread-1");
    assert_eq!(export.manifest.message_id, "msg-1");

    let status = handle_project_folder_status(
        &state,
        &resolver,
        ProjectFolderStatusRequest {
            slug: export.slug.clone(),
        },
    )
    .await
    .expect("status");
    assert_eq!(status.state, crate::project_mirror::ProjectSyncState::Clean);

    // External edit through the plain filesystem, like any editor or LLM
    // file skill would do.
    let source_path =
        std::path::Path::new(&export.folder).join(crate::project_mirror::PROJECT_SOURCE_FILE_NAME);
    std::fs::write(&source_path, "(model (part body (box 12 10 5)))").expect("edit file");

    let status = handle_project_folder_status(
        &state,
        &resolver,
        ProjectFolderStatusRequest {
            slug: export.slug.clone(),
        },
    )
    .await
    .expect("status after edit");
    assert_eq!(
        status.state,
        crate::project_mirror::ProjectSyncState::FileChanged
    );

    let applied = handle_project_folder_apply(
        &state,
        &resolver,
        ProjectFolderApplyRequest {
            identity: AgentIdentityOverride::default(),
            slug: export.slug.clone(),
            force: false,
            title: None,
            version_name: Some("V-folder-edit".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("apply");
    assert!(!applied.no_op);
    assert_eq!(
        applied.state_before,
        crate::project_mirror::ProjectSyncState::FileChanged
    );
    assert_eq!(applied.thread_id, "thread-1");
    assert_ne!(applied.message_id, "msg-1");

    {
        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
        let committed = messages
            .iter()
            .find(|message| message.id == applied.message_id)
            .expect("committed version");
        assert!(committed
            .output
            .as_ref()
            .expect("output")
            .macro_code
            .contains("box 12 10 5"));
    }

    // Manifest rebased: folder reads clean against the new head.
    let status = handle_project_folder_status(
        &state,
        &resolver,
        ProjectFolderStatusRequest {
            slug: export.slug.clone(),
        },
    )
    .await
    .expect("status after apply");
    assert_eq!(status.state, crate::project_mirror::ProjectSyncState::Clean);

    // Idempotent: applying a clean folder is a no-op.
    let noop = handle_project_folder_apply(
        &state,
        &resolver,
        ProjectFolderApplyRequest {
            identity: AgentIdentityOverride::default(),
            slug: export.slug.clone(),
            force: false,
            title: None,
            version_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect("noop apply");
    assert!(noop.no_op);
}

#[tokio::test]
async fn project_folder_apply_refuses_stale_and_conflicted_folders() {
    let (state, resolver) =
        seed_target_with_macro("Mount", "V-base", "(model (part body (box 8 8 4)))").await;

    let export = handle_project_folder_export(
        &state,
        &resolver,
        ProjectFolderExportRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            slug: Some("mount-stale-check".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("export");

    // Advance the thread behind the folder's back (normal in-app edit).
    let preview = handle_macro_preview_render(
        &state,
        &resolver,
        MacroReplaceRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            macro_code: "(model (part body (box 9 8 4)))".to_string(),
            macro_dialect: None,
            ui_spec: None,
            parameters: None,
            post_processing: None,
            geometry_backend: None,
        },
        &test_ctx(),
    )
    .await
    .expect("in-app preview");
    handle_commit_preview_version(
        &state,
        &resolver,
        VersionSaveRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(preview.thread_id.clone()),
            message_id: Some(preview.message_id.clone()),
            title: None,
            version_name: Some("V-in-app".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("in-app commit");

    // File untouched + thread advanced -> stale, must re-export.
    let err = handle_project_folder_apply(
        &state,
        &resolver,
        ProjectFolderApplyRequest {
            identity: AgentIdentityOverride::default(),
            slug: export.slug.clone(),
            force: false,
            title: None,
            version_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("stale folder refused");
    assert!(err.message.contains("stale"), "{}", err.message);
    assert!(
        err.message.contains("project_folder_export"),
        "{}",
        err.message
    );

    // File ALSO edited -> conflict; refused without force, applied with it.
    let source_path =
        std::path::Path::new(&export.folder).join(crate::project_mirror::PROJECT_SOURCE_FILE_NAME);
    std::fs::write(&source_path, "(model (part body (box 7 7 7)))").expect("edit file");

    let err = handle_project_folder_apply(
        &state,
        &resolver,
        ProjectFolderApplyRequest {
            identity: AgentIdentityOverride::default(),
            slug: export.slug.clone(),
            force: false,
            title: None,
            version_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("conflict refused without force");
    assert!(err.message.contains("conflict"), "{}", err.message);
    assert!(err.message.contains("force"), "{}", err.message);

    let applied = handle_project_folder_apply(
        &state,
        &resolver,
        ProjectFolderApplyRequest {
            identity: AgentIdentityOverride::default(),
            slug: export.slug.clone(),
            force: true,
            title: None,
            version_name: Some("V-forced".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("forced apply");
    assert_eq!(
        applied.state_before,
        crate::project_mirror::ProjectSyncState::Conflict
    );
    let conn = state.db.lock().await;
    let messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
    assert!(messages
        .iter()
        .any(|message| message.id == applied.message_id));
}

#[tokio::test]
async fn project_folder_apply_reports_missing_folder() {
    let (state, resolver) = seed_target().await;
    let err = handle_project_folder_apply(
        &state,
        &resolver,
        ProjectFolderApplyRequest {
            identity: AgentIdentityOverride::default(),
            slug: "never-exported".to_string(),
            force: false,
            title: None,
            version_name: None,
        },
        &test_ctx(),
    )
    .await
    .expect_err("missing folder");
    assert!(
        err.message.contains("project_folder_export"),
        "{}",
        err.message
    );
}

#[tokio::test]
async fn project_folder_watcher_applies_settled_edits_in_place() {
    let (state, resolver) = seed_target_with_macro(
        "Live Bracket",
        "V-base",
        "(model (part body (box 10 10 5)))",
    )
    .await;
    let export = handle_project_folder_export(
        &state,
        &resolver,
        ProjectFolderExportRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            slug: Some("live-bracket-watch".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("export");

    let mut watcher = ProjectFolderWatcher::new();
    let ctx = test_ctx();

    // Clean folder: ticks are silent.
    assert!(watcher.tick(&state, &resolver, &ctx).await.is_empty());

    let source_path =
        std::path::Path::new(&export.folder).join(crate::project_mirror::PROJECT_SOURCE_FILE_NAME);
    std::fs::write(&source_path, "(model (part body (box 11 10 5)))").expect("edit");

    // First tick after the edit: settle, no apply yet.
    assert!(watcher.tick(&state, &resolver, &ctx).await.is_empty());

    // Second tick: digest unchanged -> applied and committed.
    let events = watcher.tick(&state, &resolver, &ctx).await;
    assert_eq!(events.len(), 1);
    let ProjectFolderWatchEvent::Applied {
        slug, message_id, ..
    } = &events[0]
    else {
        panic!("expected Applied, got {events:?}");
    };
    assert_eq!(slug, "live-bracket-watch");
    {
        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").expect("messages");
        let committed = messages
            .iter()
            .find(|message| &message.id == message_id)
            .expect("watcher-committed version");
        assert!(committed
            .output
            .as_ref()
            .expect("output")
            .macro_code
            .contains("box 11 10 5"));
    }

    // Folder is clean again; nothing further happens.
    assert!(watcher.tick(&state, &resolver, &ctx).await.is_empty());
}

#[tokio::test]
async fn project_folder_watcher_reports_broken_edit_once_and_retries_after_change() {
    let (state, resolver) =
        seed_target_with_macro("Watch Errors", "V-base", "(model (part body (box 8 8 4)))").await;
    let export = handle_project_folder_export(
        &state,
        &resolver,
        ProjectFolderExportRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some("thread-1".to_string()),
            message_id: Some("msg-1".to_string()),
            slug: Some("watch-errors".to_string()),
        },
        &test_ctx(),
    )
    .await
    .expect("export");
    let source_path =
        std::path::Path::new(&export.folder).join(crate::project_mirror::PROJECT_SOURCE_FILE_NAME);

    let mut watcher = ProjectFolderWatcher::new();
    let ctx = test_ctx();

    std::fs::write(&source_path, "(model (part body (box 1 1 1))$)").expect("broken edit");
    assert!(watcher.tick(&state, &resolver, &ctx).await.is_empty());
    let events = watcher.tick(&state, &resolver, &ctx).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(
        matches!(&events[0], ProjectFolderWatchEvent::ApplyFailed { slug, .. } if slug == "watch-errors"),
        "{events:?}"
    );

    // Same broken digest: memoized, no re-render attempts.
    assert!(watcher.tick(&state, &resolver, &ctx).await.is_empty());
    assert!(watcher.tick(&state, &resolver, &ctx).await.is_empty());

    // Fixing the file retries and applies.
    std::fs::write(&source_path, "(model (part body (box 2 2 2)))").expect("fixed edit");
    assert!(watcher.tick(&state, &resolver, &ctx).await.is_empty());
    let events = watcher.tick(&state, &resolver, &ctx).await;
    assert!(
        matches!(&events[0], ProjectFolderWatchEvent::Applied { .. }),
        "{events:?}"
    );
}
