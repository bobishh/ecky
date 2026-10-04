use ecky_cad_lib::contracts::{
    ArtifactBundle, Config, DesignParams, GeometryBackend, MessageStatus, ModelManifest,
    ParamValue, SourceLanguage,
};
use ecky_cad_lib::models::{AppState, PathResolver};
use ecky_cad_lib::services::imported_model::{
    apply_imported_parameters, persist_imported_runtime, ImportedParameterApplyRequest,
};
use sha2::{Digest, Sha256};
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

fn test_config(projects_root: PathBuf) -> Config {
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
        projects_root: Some(projects_root.to_string_lossy().to_string()),
    }
}

fn imported_runtime(root: &std::path::Path) -> (ArtifactBundle, ModelManifest) {
    let part_id = format!(
        "part-body-{}",
        &format!("{:x}", Sha256::digest(b"body"))[..10]
    );
    let source_path = root.join("Bearing.FCStd");
    std::fs::write(&source_path, b"fixture").expect("source");
    let bundle: ArtifactBundle = serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "modelId": "imported-fcstd-bearing",
        "sourceKind": "importedFcstd",
        "engineKind": "freecad",
        "sourceLanguage": "legacyPython",
        "geometryBackend": "freecad",
        "contentHash": "fixture-hash",
        "artifactVersion": 1,
        "fcstdPath": source_path,
        "manifestPath": root.join("manifest.json"),
        "modelStlPath": root.join("model.stl")
    }))
    .expect("bundle");
    let manifest: ModelManifest = serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "modelId": "imported-fcstd-bearing",
        "sourceKind": "importedFcstd",
        "engineKind": "freecad",
        "sourceLanguage": "legacyPython",
        "geometryBackend": "freecad",
        "document": {
            "documentName": "Bearing",
            "documentLabel": "608 Bearing",
            "sourcePath": source_path,
            "objectCount": 1,
            "warnings": []
        },
        "parts": [{
            "partId": part_id.clone(),
            "freecadObjectName": "Body",
            "label": "Bearing body",
            "kind": "solid",
            "semanticRole": "body",
            "viewerNodeIds": ["body"],
            "parameterKeys": ["outer_diameter"],
            "editable": true,
            "bounds": {"xMin": 0, "yMin": 0, "zMin": 0, "xMax": 22, "yMax": 22, "zMax": 7}
        }],
        "parameterGroups": [{
            "groupId": "body-params",
            "label": "Body",
            "parameterKeys": ["outer_diameter"],
            "partIds": [part_id],
            "editable": true
        }],
        "controlPrimitives": [],
        "controlRelations": [],
        "controlViews": [],
        "previewViews": [],
        "advisories": [],
        "selectionTargets": [],
        "measurementAnnotations": [],
        "warnings": [],
        "enrichmentState": {"status": "none", "proposals": []}
    }))
    .expect("manifest");
    (bundle, manifest)
}

#[cfg(unix)]
fn fake_freecad_command(root: &std::path::Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let command = root.join("fake-freecad.sh");
    std::fs::write(
        &command,
        r#"#!/bin/sh
set -eu
mkdir -p "$ECKYCAD_PARTS_DIR"
printf 'solid model\nendsolid model\n' > "$ECKYCAD_STL"
printf 'ISO-10303-21;\nEND-ISO-10303-21;\n' > "$ECKYCAD_STEP"
part_path="$ECKYCAD_PARTS_DIR/body.stl"
printf 'solid body\nendsolid body\n' > "$part_path"
cat > "$ECKYCAD_REPORT" <<EOF
{
  "document_name": "Bearing",
  "document_label": "608 Bearing",
  "warnings": [],
  "objects": [{
    "part_id": "body",
    "object_name": "Body",
    "label": "Bearing body",
    "type_id": "PartDesign::Body",
    "export_path": "$part_path",
    "bounds": {"x_min": 0, "y_min": 0, "z_min": 0, "x_max": 30, "y_max": 30, "z_max": 7}
  }]
}
EOF
"#,
    )
    .expect("fake FreeCAD command");
    let mut permissions = std::fs::metadata(&command)
        .expect("fake command metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&command, permissions).expect("fake command permissions");
    command
}

#[test]
fn imported_parameter_intent_contract_serializes_camel_case() {
    let value = serde_json::to_value(ImportedParameterApplyRequest {
        thread_id: "thread-1".to_string(),
        target_message_id: "message-1".to_string(),
        parameters: DesignParams::new(),
        persist: false,
        title: None,
        version_name: Some("Imported V2".to_string()),
    })
    .expect("request json");

    assert_eq!(value["threadId"], "thread-1");
    assert_eq!(value["targetMessageId"], "message-1");
    assert_eq!(value["versionName"], "Imported V2");
    assert!(value.get("thread_id").is_none());
    assert!(value.get("target_message_id").is_none());
}

#[tokio::test]
async fn imported_runtime_intent_builds_controls_and_persists_one_canonical_projection() {
    let root = std::env::temp_dir().join(format!("ecky-import-intent-{}", Uuid::new_v4()));
    let projects_root = root.join("projects");
    std::fs::create_dir_all(&root).expect("root");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let state = AppState::new(test_config(projects_root), None, conn);
    let resolver = TestResolver { root: root.clone() };
    let (bundle, manifest) = imported_runtime(&root);

    let result = persist_imported_runtime(None, None, bundle, manifest, &state, &resolver)
        .await
        .expect("import projection");

    assert_eq!(result.title, "608 Bearing");
    assert_eq!(result.design_output.ui_spec.fields.len(), 1);
    assert_eq!(
        result.design_output.initial_params.get("outer_diameter"),
        Some(&ecky_cad_lib::contracts::ParamValue::Number(22.0))
    );
    assert_eq!(result.model_manifest.control_primitives.len(), 1);
    assert!(result
        .model_manifest
        .control_views
        .iter()
        .any(|view| view.view_id == "view-model"));

    let db = state.db.lock().await;
    let messages = ecky_cad_lib::db::get_thread_messages(&db, &result.thread_id).expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, result.message_id);
    assert_eq!(messages[0].output.as_ref(), Some(&result.design_output));
    assert_eq!(
        messages[0].model_manifest.as_ref(),
        Some(&result.model_manifest)
    );
    drop(db);

    let snapshot = state
        .last_snapshot
        .lock()
        .unwrap()
        .clone()
        .expect("snapshot");
    assert_eq!(
        snapshot.thread_id.as_deref(),
        Some(result.thread_id.as_str())
    );
    assert_eq!(
        snapshot.message_id.as_deref(),
        Some(result.message_id.as_str())
    );
    assert_eq!(snapshot.design.as_ref(), Some(&result.design_output));

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn imported_parameter_intent_materializes_and_appends_one_canonical_version() {
    let root = std::env::temp_dir().join(format!("ecky-import-params-{}", Uuid::new_v4()));
    let projects_root = root.join("projects");
    std::fs::create_dir_all(&root).expect("root");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let mut config = test_config(projects_root);
    config.freecad_cmd = fake_freecad_command(&root).to_string_lossy().to_string();
    let state = AppState::new(config, None, conn);
    let resolver = TestResolver { root: root.clone() };
    let (bundle, manifest) = imported_runtime(&root);
    let imported = persist_imported_runtime(None, None, bundle, manifest, &state, &resolver)
        .await
        .expect("import projection");
    let parameters = DesignParams::from([("outer_diameter".to_string(), ParamValue::Number(30.0))]);

    let preview = apply_imported_parameters(
        ImportedParameterApplyRequest {
            thread_id: imported.thread_id.clone(),
            target_message_id: imported.message_id.clone(),
            parameters: parameters.clone(),
            persist: false,
            title: None,
            version_name: Some("Imported V2".to_string()),
        },
        &state,
        &resolver,
    )
    .await
    .expect("imported parameter preview");

    assert_eq!(preview.status, MessageStatus::Success);
    assert!(preview.message_id.is_none());
    assert_eq!(preview.design_output.initial_params, parameters);
    {
        let db = state.db.lock().await;
        let messages = ecky_cad_lib::db::get_thread_messages(&db, &imported.thread_id)
            .expect("preview messages");
        assert_eq!(messages.len(), 1);
    }
    let preview_snapshot = state
        .last_snapshot
        .lock()
        .unwrap()
        .clone()
        .expect("preview snapshot");
    assert_eq!(
        preview_snapshot.message_id.as_deref(),
        Some(imported.message_id.as_str())
    );

    let result = apply_imported_parameters(
        ImportedParameterApplyRequest {
            thread_id: imported.thread_id.clone(),
            target_message_id: imported.message_id.clone(),
            parameters: parameters.clone(),
            persist: true,
            title: None,
            version_name: Some("Imported V2".to_string()),
        },
        &state,
        &resolver,
    )
    .await
    .expect("imported parameter response");

    assert_eq!(result.status, MessageStatus::Success, "{:?}", result.error);
    assert_eq!(result.base_message_id, imported.message_id);
    assert_ne!(
        result.message_id.as_deref(),
        Some(result.base_message_id.as_str())
    );
    assert_eq!(result.design_output.initial_params, parameters);
    assert_eq!(result.design_output.version_name, "Imported V2");
    assert!(result.error.is_none());
    let next_bundle = result.artifact_bundle.expect("canonical artifact bundle");
    let next_manifest = result.model_manifest.expect("canonical model manifest");
    assert_eq!(next_bundle.model_id, next_manifest.model_id);
    assert_eq!(
        next_bundle.artifact_version,
        imported.artifact_bundle.artifact_version + 1
    );
    assert_eq!(
        next_manifest.control_primitives,
        imported.model_manifest.control_primitives
    );
    assert_eq!(
        next_manifest.control_views,
        imported.model_manifest.control_views
    );

    let db = state.db.lock().await;
    let messages =
        ecky_cad_lib::db::get_thread_messages(&db, &imported.thread_id).expect("messages");
    assert_eq!(messages.len(), 2);
    let head = messages.last().expect("appended version");
    assert_eq!(Some(head.id.as_str()), result.message_id.as_deref());
    assert_eq!(head.status, MessageStatus::Success);
    drop(db);

    let snapshot = state
        .last_snapshot
        .lock()
        .unwrap()
        .clone()
        .expect("snapshot");
    assert_eq!(snapshot.message_id.as_deref(), result.message_id.as_deref());
    assert_eq!(snapshot.artifact_bundle.as_ref(), Some(&next_bundle));
    assert_eq!(snapshot.model_manifest.as_ref(), Some(&next_manifest));
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn imported_parameter_intent_appends_raw_runner_error_version() {
    let root = std::env::temp_dir().join(format!("ecky-import-params-error-{}", Uuid::new_v4()));
    let projects_root = root.join("projects");
    std::fs::create_dir_all(&root).expect("root");
    let conn = ecky_cad_lib::db::init_db(&root.join("history.sqlite")).expect("db");
    let mut config = test_config(projects_root);
    config.freecad_cmd = "/usr/bin/false".to_string();
    let state = AppState::new(config, None, conn);
    let resolver = TestResolver { root: root.clone() };
    let (bundle, manifest) = imported_runtime(&root);
    let imported = persist_imported_runtime(None, None, bundle, manifest, &state, &resolver)
        .await
        .expect("import projection");

    let result = apply_imported_parameters(
        ImportedParameterApplyRequest {
            thread_id: imported.thread_id.clone(),
            target_message_id: imported.message_id.clone(),
            parameters: DesignParams::from([(
                "outer_diameter".to_string(),
                ParamValue::Number(30.0),
            )]),
            persist: true,
            title: None,
            version_name: None,
        },
        &state,
        &resolver,
    )
    .await
    .expect("domain error response");

    assert_eq!(result.status, MessageStatus::Error);
    assert!(result.artifact_bundle.is_none());
    assert!(result.model_manifest.is_none());
    let error = result.error.expect("raw runner error");
    assert_eq!(error.code, ecky_cad_lib::contracts::AppErrorCode::Render);
    assert!(
        error.to_string().contains("FreeCAD runner failed"),
        "{error}"
    );

    let db = state.db.lock().await;
    let messages =
        ecky_cad_lib::db::get_thread_messages(&db, &imported.thread_id).expect("messages");
    assert_eq!(messages.len(), 2);
    let head = messages.last().expect("failed version");
    assert_eq!(head.status, MessageStatus::Error);
    assert_eq!(head.content, error.to_string());
    drop(db);
    std::fs::remove_dir_all(root).expect("cleanup");
}
