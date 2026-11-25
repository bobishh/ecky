use ecky_cad_lib::contracts::{
    DesignOutput, EngineKind, GeometryBackend, InteractionMode, MacroDialect, Message, MessageRole,
    MessageStatus, SourceLanguage, UiSpec,
};
use ecky_cad_lib::{db, services::history::get_workspace_projection};
use std::collections::BTreeMap;

fn version(id: &str, timestamp: u64, version_name: &str) -> Message {
    Message {
        id: id.to_string(),
        parent_message_id: None,
        role: MessageRole::Assistant,
        content: version_name.to_string(),
        status: MessageStatus::Success,
        output: Some(DesignOutput {
            title: "Workspace".to_string(),
            version_name: version_name.to_string(),
            response: String::new(),
            interaction_mode: InteractionMode::Design,
            macro_code: format!("export {version_name}"),
            macro_dialect: MacroDialect::CadFrameworkV1,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            ui_spec: UiSpec { fields: Vec::new() },
            initial_params: BTreeMap::new(),
            post_processing: None,
        }),
        usage: None,
        artifact_bundle: None,
        model_manifest: None,
        structural_verification: None,
        agent_origin: None,
        image_data: None,
        visual_kind: None,
        attachment_images: Vec::new(),
        timestamp,
    }
}

#[test]
fn workspace_projection_loads_timeline_and_preferred_version_atomically() {
    let path = std::env::temp_dir().join(format!(
        "ecky-workspace-projection-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let conn = db::init_db(&path).expect("schema");
    db::create_or_update_thread(&conn, "thread-1", "Workspace", 100, None).expect("thread");
    db::add_message(&conn, "thread-1", &version("version-1", 100, "V1")).expect("older version");
    db::add_message(&conn, "thread-1", &version("version-2", 200, "V2")).expect("newer version");

    let projection = get_workspace_projection(&conn, "thread-1", Some("version-1"), Some(20))
        .expect("projection");

    assert_eq!(projection.thread.id, "thread-1");
    assert!(projection.thread.messages.is_empty());
    assert_eq!(
        projection
            .messages_page
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["version-1", "version-2"]
    );
    assert_eq!(
        projection
            .selected_version
            .as_ref()
            .map(|message| message.id.as_str()),
        Some("version-1")
    );
    assert!(projection.requested_message_found);

    let json = serde_json::to_value(&projection).expect("serialize projection");
    assert!(json.get("messagesPage").is_some());
    assert!(json.get("selectedVersion").is_some());
    assert!(json.get("requestedMessageFound").is_some());
    assert!(json.get("messages_page").is_none());

    drop(conn);
    let _ = std::fs::remove_file(path);
}

#[test]
fn workspace_projection_falls_back_to_latest_when_pointer_is_stale() {
    let path = std::env::temp_dir().join(format!(
        "ecky-workspace-projection-fallback-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let conn = db::init_db(&path).expect("schema");
    db::create_or_update_thread(&conn, "thread-1", "Workspace", 100, None).expect("thread");
    db::add_message(&conn, "thread-1", &version("version-1", 100, "V1")).expect("older version");
    db::add_message(&conn, "thread-1", &version("version-2", 200, "V2")).expect("newer version");

    let projection = get_workspace_projection(&conn, "thread-1", Some("deleted-version"), Some(20))
        .expect("projection");

    assert_eq!(
        projection
            .selected_version
            .as_ref()
            .map(|message| message.id.as_str()),
        Some("version-2")
    );
    assert!(!projection.requested_message_found);

    drop(conn);
    let _ = std::fs::remove_file(path);
}
