use base64::Engine;
use ecky_cad_lib::{
    contracts::{ArtifactBundle, Message, MessageRole, MessageStatus},
    db,
};
use std::path::Path;

fn version(id: &str, timestamp: u64, path: &Path) -> Message {
    let bundle: ArtifactBundle = serde_json::from_value(serde_json::json!({
        "modelId": id, "sourceKind": "generated", "contentHash": id,
        "fcstdPath": "", "manifestPath": "", "modelStlPath": path,
    }))
    .unwrap();
    Message {
        id: id.into(),
        role: MessageRole::Assistant,
        content: "rendered".into(),
        status: MessageStatus::Success,
        output: None,
        usage: None,
        artifact_bundle: Some(bundle),
        model_manifest: None,
        structural_verification: None,
        agent_origin: None,
        timestamp,
        image_data: None,
        visual_kind: None,
        attachment_images: vec![],
    }
}

#[test]
fn given_background_renders_when_runtime_attaches_then_every_version_keeps_png_after_restart_and_red_head(
) {
    let root = std::env::temp_dir().join(format!("ecky-preview-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mesh = "solid mesh\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 10\nendloop\nendfacet\nendsolid mesh";
    let db_path = root.join("history.sqlite");
    let conn = db::init_db(&db_path).unwrap();
    db::create_or_update_thread(&conn, "thread", "Preview", 1, None).unwrap();
    for timestamp in 1..=2 {
        let path = root
            .join("model-runtime")
            .join(format!("v{timestamp}"))
            .join("model.stl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, mesh).unwrap();
        let message = version(&format!("v{timestamp}"), timestamp, &path);
        db::add_message(&conn, "thread", &message).unwrap();
        let image: Option<String> = conn
            .query_row(
                "SELECT image_data FROM messages WHERE id = ?1",
                [&message.id],
                |row| row.get(0),
            )
            .unwrap();
        let image = image
            .expect("successful background render must persist its own preview without viewport");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        assert!(
            image::load_from_memory(&bytes).is_ok(),
            "preview must be a real PNG"
        );
        if timestamp == 1 {
            // Legacy image loss must be repaired before the next append prunes its STL.
            conn.execute("UPDATE messages SET image_data = NULL WHERE id = 'v1'", [])
                .unwrap();
        } else {
            assert!(!root.join("model-runtime/v1/model.stl").exists());
            let recovered: Option<String> = conn
                .query_row(
                    "SELECT image_data FROM messages WHERE id = 'v1'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(recovered.as_deref(), Some(image.as_str()));
        }
    }
    let mut red = version("red", 3, &root.join("missing.stl"));
    red.status = MessageStatus::Error;
    db::add_message(&conn, "thread", &red).unwrap();
    drop(conn);
    let latest = root.join("model-runtime/v2/model.stl");
    if latest.exists() {
        std::fs::remove_file(latest).unwrap();
    }
    let conn = db::init_db(&db_path).unwrap();
    assert!(
        db::get_thread_preview(&conn, "thread").unwrap().is_some(),
        "red head must retain last rendered preview after restart"
    );
    drop(conn);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn given_existing_preview_when_runtime_changes_then_image_tracks_exact_artifact_and_legacy_mesh_backfills(
) {
    let root = std::env::temp_dir().join(format!("ecky-preview-runtime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("old.stl");
    let new_path = root.join("new.stl");
    std::fs::write(&path, "solid mesh\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 5\nendloop\nendfacet\nendsolid mesh").unwrap();
    std::fs::write(&new_path, "solid mesh\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 30 0 0\nvertex 0 10 5\nendloop\nendfacet\nendsolid mesh").unwrap();
    let conn = db::init_db(&root.join("history.sqlite")).unwrap();
    db::create_or_update_thread(&conn, "thread", "Preview", 1, None).unwrap();
    let message = version("version", 1, &path);
    db::add_message(&conn, "thread", &message).unwrap();
    let bundle = message.artifact_bundle.as_ref().unwrap();
    // A matching user viewport screenshot must survive ordinary runtime metadata updates.
    db::update_message_image_data(&conn, "version", "data:image/png;base64,viewport").unwrap();
    db::update_message_artifact_bundle(&conn, "version", bundle).unwrap();
    assert_eq!(
        db::get_thread_preview(&conn, "thread").unwrap().as_deref(),
        Some("data:image/png;base64,viewport")
    );
    let mut new_bundle = bundle.clone();
    new_bundle.model_stl_path = new_path.to_string_lossy().into_owned();
    new_bundle.content_hash = "new-hash".into();
    new_bundle.artifact_version += 1;
    db::update_message_artifact_bundle(&conn, "version", &new_bundle).unwrap();
    let current = db::get_thread_preview(&conn, "thread").unwrap().unwrap();
    assert_ne!(
        current, "data:image/png;base64,viewport",
        "changed runtime must not reuse stale image"
    );
    // Simulate a historical row written by an older app, without changing its source/runtime.
    conn.execute(
        "UPDATE messages SET image_data = NULL WHERE id = 'version'",
        [],
    )
    .unwrap();
    assert_eq!(
        db::get_thread_preview(&conn, "thread").unwrap(),
        Some(current)
    );
    drop(conn);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn given_pending_version_when_background_status_attaches_render_then_png_is_persisted() {
    let root = std::env::temp_dir().join(format!("ecky-preview-status-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("model.stl");
    std::fs::write(&path, "solid mesh\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 5\nendloop\nendfacet\nendsolid mesh").unwrap();
    let conn = db::init_db(&root.join("history.sqlite")).unwrap();
    db::create_or_update_thread(&conn, "thread", "Preview", 1, None).unwrap();
    let mut message = version("pending", 1, &path);
    let bundle = message.artifact_bundle.take().unwrap();
    message.status = MessageStatus::Pending;
    db::add_message(&conn, "thread", &message).unwrap();
    db::update_message_status_and_output(
        &conn,
        &message.id,
        db::MessageStatusUpdate {
            status: &MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: Some(&bundle),
            model_manifest: None,
            structural_verification: None,
            visual_kind: None,
            content: None,
        },
    )
    .unwrap();
    assert!(db::get_thread_preview(&conn, "thread")
        .unwrap()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    drop(conn);
    std::fs::remove_dir_all(root).unwrap();
}
