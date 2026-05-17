use crate::db;
use crate::models::{AppError, AppResult, MessageRole, MessageStatus, Thread, ThreadStatus};
use crate::persist_thread_summary;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub fn get_history(conn: &rusqlite::Connection) -> AppResult<Vec<Thread>> {
    db::get_all_threads(conn).map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))
}

pub fn get_thread(conn: &rusqlite::Connection, id: &str) -> AppResult<Thread> {
    let title = db::get_visible_thread_title(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    let summary = db::get_thread_summary(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .unwrap_or_default();
    let messages = db::get_thread_messages_for_thread_view(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let genie_traits = db::get_thread_genie_traits(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let updated_at = messages.last().map(|m| m.timestamp).unwrap_or(0);
    let version_count = messages
        .iter()
        .filter(|m| is_renderable_version_message(m))
        .count();
    let pending_count = messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant && m.status == MessageStatus::Pending)
        .count();
    let queued_count = messages
        .iter()
        .filter(|m| m.role == MessageRole::User && m.status == MessageStatus::Pending)
        .count();
    let error_count = messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant && m.status == MessageStatus::Error)
        .count();

    let lifecycle = db::get_thread_lifecycle(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .unwrap_or(db::ThreadLifecycle {
            status: ThreadStatus::Active,
            finalized_at: None,
            pending_confirm: None,
        });

    Ok(Thread {
        id: id.to_string(),
        title,
        summary,
        messages,
        updated_at,
        genie_traits,
        version_count,
        pending_count,
        queued_count,
        error_count,
        status: lifecycle.status,
        finalized_at: lifecycle.finalized_at,
        pending_confirm: lifecycle.pending_confirm,
    })
}

pub fn get_thread_latest_version(
    conn: &rusqlite::Connection,
    id: &str,
) -> AppResult<Option<crate::models::Message>> {
    db::get_visible_thread_title(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    db::get_thread_latest_version(conn, id).map_err(|err| AppError::persistence(err.to_string()))
}

pub fn get_thread_message_version(
    conn: &rusqlite::Connection,
    thread_id: &str,
    message_id: &str,
) -> AppResult<Option<crate::models::Message>> {
    db::get_visible_thread_title(conn, thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    db::get_thread_message_version(conn, thread_id, message_id)
        .map_err(|err| AppError::persistence(err.to_string()))
}

pub fn get_thread_messages_page(
    conn: &rusqlite::Connection,
    id: &str,
    before: Option<u64>,
    limit: Option<usize>,
    include_visual_payloads: bool,
) -> AppResult<crate::models::ThreadMessagesPage> {
    db::get_visible_thread_title(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    db::get_thread_messages_page(
        conn,
        id,
        before,
        limit.unwrap_or(50),
        include_visual_payloads,
    )
    .map_err(|err| AppError::persistence(err.to_string()))
}

pub fn finalize_thread(
    conn: &rusqlite::Connection,
    thread_id: &str,
    selected_message_id: Option<&str>,
) -> AppResult<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let title = db::get_visible_thread_title(conn, thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    let summary = db::get_thread_summary(conn, thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .unwrap_or_default();
    let genie_traits = db::get_thread_genie_traits(conn, thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    let messages = db::get_thread_messages(conn, thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let selected_message = if let Some(message_id) = selected_message_id {
        messages
            .iter()
            .find(|message| message.id == message_id && is_renderable_version_message(message))
            .cloned()
            .ok_or_else(|| {
                AppError::validation("Selected final model is not a valid version in this thread.")
            })?
    } else {
        messages
            .iter()
            .rev()
            .find(|message| is_renderable_version_message(message))
            .cloned()
            .ok_or_else(|| AppError::validation("Thread has no successful versions to finalize."))?
    };

    let finalized_thread_id = Uuid::new_v4().to_string();
    let finalized_message_id = Uuid::new_v4().to_string();

    db::create_or_update_thread(
        conn,
        &finalized_thread_id,
        &title,
        now,
        genie_traits.as_ref(),
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;
    db::update_thread_summary(conn, &finalized_thread_id, &summary)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let mut finalized_message = selected_message;
    finalized_message.id = finalized_message_id;
    finalized_message.timestamp = now;
    db::add_message(conn, &finalized_thread_id, &finalized_message)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    db::finalize_thread(conn, &finalized_thread_id, now as i64)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    let deleted =
        db::delete_thread(conn, thread_id).map_err(|err| AppError::persistence(err.to_string()))?;
    if !deleted {
        return Err(AppError::not_found("Thread not found."));
    }

    Ok(())
}

fn is_renderable_version_message(message: &crate::models::Message) -> bool {
    message.role == MessageRole::Assistant
        && message.status == MessageStatus::Success
        && message.artifact_bundle.is_some()
}

pub fn reopen_thread(conn: &rusqlite::Connection, thread_id: &str) -> AppResult<()> {
    let changed =
        db::reopen_thread(conn, thread_id).map_err(|err| AppError::persistence(err.to_string()))?;
    if changed {
        Ok(())
    } else {
        Err(AppError::not_found("Thread not found."))
    }
}

pub fn get_inventory(conn: &rusqlite::Connection) -> AppResult<Vec<Thread>> {
    db::get_inventory_threads(conn).map_err(|err| AppError::persistence(err.to_string()))
}

pub fn delete_version(conn: &rusqlite::Connection, message_id: &str) -> AppResult<()> {
    let thread_id = db::delete_version_cluster(conn, message_id)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;

    if let Some(thread_id) = thread_id {
        let title = db::get_thread_title(conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .unwrap_or_default();
        if db::has_visible_messages(conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
        {
            let _ = persist_thread_summary(conn, &thread_id, &title);
        } else {
            db::update_thread_summary(conn, &thread_id, "")
                .map_err(|err| AppError::persistence(err.to_string()))?;
        }
    }

    Ok(())
}

pub fn restore_version(conn: &rusqlite::Connection, message_id: &str) -> AppResult<()> {
    let thread_id = db::restore_version_cluster(conn, message_id)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;

    if let Some(thread_id) = thread_id {
        let title = db::get_thread_title(conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .unwrap_or_default();
        if db::has_visible_messages(conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
        {
            let _ = persist_thread_summary(conn, &thread_id, &title);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::{
        DesignOutput, GenieTraits, InteractionMode, MacroDialect, Message, UiSpec,
    };
    use std::collections::BTreeMap;

    fn sample_output(version_name: &str) -> DesignOutput {
        DesignOutput {
            title: "Lamp".to_string(),
            version_name: version_name.to_string(),
            response: String::new(),
            interaction_mode: InteractionMode::Design,
            macro_code: "print('hi')".to_string(),
            macro_dialect: MacroDialect::CadFrameworkV1,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            ui_spec: UiSpec { fields: Vec::new() },
            initial_params: BTreeMap::new(),
            post_processing: None,
        }
    }

    fn sample_artifact_bundle(model_id: &str) -> crate::models::ArtifactBundle {
        crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: model_id.to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            content_hash: format!("hash-{model_id}"),
            artifact_version: 1,
            fcstd_path: format!("/tmp/{model_id}.FCStd"),
            manifest_path: format!("/tmp/{model_id}.json"),
            macro_path: None,
            preview_stl_path: format!("/tmp/{model_id}.stl"),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        }
    }

    fn sample_message(id: &str, timestamp: u64, version_name: &str) -> Message {
        Message {
            id: id.to_string(),
            role: MessageRole::Assistant,
            content: format!("Version {}", version_name),
            status: MessageStatus::Success,
            output: Some(sample_output(version_name)),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle(id)),
            model_manifest: None,
            agent_origin: None,
            timestamp,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        }
    }

    #[test]
    fn finalize_thread_promotes_selected_version_to_inventory_and_hides_source_thread() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-finalize-thread-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();

        let thread_id = "thread-source";
        let genie_traits = GenieTraits::from_seed(42);
        db::create_or_update_thread(
            &conn,
            thread_id,
            "Bulb Lamp Shade",
            100,
            Some(&genie_traits),
        )
        .unwrap();
        db::update_thread_summary(&conn, thread_id, "Working thread").unwrap();

        let older = sample_message("msg-older", 100, "V-old");
        let newer = sample_message("msg-newer", 200, "V-new");
        db::add_message(&conn, thread_id, &older).unwrap();
        db::add_message(&conn, thread_id, &newer).unwrap();

        finalize_thread(&conn, thread_id, Some(&older.id)).unwrap();

        let active_threads = db::get_all_threads(&conn).unwrap();
        assert!(active_threads.iter().all(|thread| thread.id != thread_id));

        let inventory_threads = db::get_inventory_threads(&conn).unwrap();
        assert_eq!(inventory_threads.len(), 1);
        let finalized = &inventory_threads[0];
        assert_eq!(finalized.title, "Bulb Lamp Shade");
        assert_eq!(finalized.status, ThreadStatus::Finalized);
        assert_eq!(finalized.version_count, 1);

        let loaded = get_thread(&conn, &finalized.id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(
            loaded.messages[0]
                .output
                .as_ref()
                .map(|output| output.version_name.clone()),
            Some("V-old".to_string())
        );

        let deleted_at: Option<i64> = conn
            .query_row(
                "SELECT deleted_at FROM threads WHERE id = ?1",
                [thread_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(deleted_at.is_some());

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_reports_queued_count_from_pending_user_messages_only() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-queued-count-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();

        db::create_or_update_thread(&conn, "thread-queued", "Queued thread", 100, None).unwrap();

        db::add_message(
            &conn,
            "thread-queued",
            &Message {
                id: "user-pending".to_string(),
                role: MessageRole::User,
                content: "Queued".to_string(),
                status: MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                agent_origin: None,
                timestamp: 100,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        db::add_message(
            &conn,
            "thread-queued",
            &Message {
                id: "user-working".to_string(),
                role: MessageRole::User,
                content: "Claimed".to_string(),
                status: MessageStatus::Working,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                agent_origin: None,
                timestamp: 101,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        db::add_message(
            &conn,
            "thread-queued",
            &Message {
                id: "assistant-pending".to_string(),
                role: MessageRole::Assistant,
                content: "Pending".to_string(),
                status: MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                agent_origin: None,
                timestamp: 102,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        let thread = get_thread(&conn, "thread-queued").unwrap();
        assert_eq!(thread.queued_count, 1);
        assert_eq!(thread.pending_count, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_keeps_deleted_versions_in_history_but_removes_them_from_version_count() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-deleted-version-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();

        db::create_or_update_thread(&conn, "thread-carousel", "Bulb", 100, None).unwrap();

        let live = sample_message("msg-live", 100, "V-live");
        let discarded = sample_message("msg-discarded", 101, "V-discarded");
        db::add_message(&conn, "thread-carousel", &live).unwrap();
        db::add_message(&conn, "thread-carousel", &discarded).unwrap();

        db::delete_version_cluster(&conn, &discarded.id).unwrap();

        let thread = get_thread(&conn, "thread-carousel").unwrap();

        assert_eq!(
            thread
                .messages
                .iter()
                .map(|message| (message.id.as_str(), message.status.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("msg-live", MessageStatus::Success),
                ("msg-discarded", MessageStatus::Discarded),
            ]
        );
        assert_eq!(thread.version_count, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_ignores_output_only_messages_in_version_count() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-output-only-version-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();

        db::create_or_update_thread(&conn, "thread-output-only", "Output only", 100, None).unwrap();

        let mut output_only = sample_message("msg-output-only", 100, "V-output-only");
        output_only.artifact_bundle = None;
        let rendered = sample_message("msg-rendered", 101, "V-rendered");
        db::add_message(&conn, "thread-output-only", &output_only).unwrap();
        db::add_message(&conn, "thread-output-only", &rendered).unwrap();

        let thread = get_thread(&conn, "thread-output-only").unwrap();
        assert_eq!(thread.messages.len(), 2);
        assert_eq!(thread.version_count, 1);

        let latest = get_thread_latest_version(&conn, "thread-output-only")
            .unwrap()
            .expect("latest");
        assert_eq!(latest.id, rendered.id);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_latest_version_returns_newest_renderable_success() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-latest-version-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();

        db::create_or_update_thread(&conn, "thread-latest", "Latest", 100, None).unwrap();
        let older = sample_message("msg-older", 100, "V-old");
        let newer = sample_message("msg-newer", 200, "V-new");
        let mut failed = sample_message("msg-failed", 300, "V-failed");
        failed.status = MessageStatus::Error;
        db::add_message(&conn, "thread-latest", &older).unwrap();
        db::add_message(&conn, "thread-latest", &newer).unwrap();
        db::add_message(&conn, "thread-latest", &failed).unwrap();

        let latest = get_thread_latest_version(&conn, "thread-latest")
            .unwrap()
            .expect("latest");

        assert_eq!(latest.id, "msg-newer");
        assert_eq!(
            latest
                .output
                .as_ref()
                .map(|output| output.version_name.as_str()),
            Some("V-new")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_message_version_returns_pointed_renderable_version() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-pointed-version-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();

        db::create_or_update_thread(&conn, "thread-pointed", "Pointed", 100, None).unwrap();
        let older = sample_message("msg-older", 100, "V-old");
        let newer = sample_message("msg-newer", 200, "V-new");
        db::add_message(&conn, "thread-pointed", &older).unwrap();
        db::add_message(&conn, "thread-pointed", &newer).unwrap();

        let pointed = get_thread_message_version(&conn, "thread-pointed", "msg-older")
            .unwrap()
            .expect("pointed");

        assert_eq!(pointed.id, "msg-older");
        assert_eq!(
            pointed
                .output
                .as_ref()
                .map(|output| output.version_name.as_str()),
            Some("V-old")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_messages_page_strips_visual_payloads_and_paginates() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-message-page-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();

        db::create_or_update_thread(&conn, "thread-page", "Page", 100, None).unwrap();
        for index in 1..=3 {
            let mut message = sample_message(
                &format!("msg-{}", index),
                100 + index,
                &format!("V{}", index),
            );
            message.image_data = Some(format!("data:image/png;base64,{}", index));
            message.attachment_images = vec![format!("/tmp/ref-{}.png", index)];
            db::add_message(&conn, "thread-page", &message).unwrap();
        }

        let first_page =
            get_thread_messages_page(&conn, "thread-page", None, Some(2), false).unwrap();
        assert_eq!(
            first_page
                .messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg-2", "msg-3"]
        );
        assert!(first_page.has_more);
        assert_eq!(first_page.next_before, Some(102));
        assert!(first_page
            .messages
            .iter()
            .all(|message| message.image_data.is_none()));
        assert!(first_page
            .messages
            .iter()
            .all(|message| message.attachment_images.is_empty()));

        let second_page =
            get_thread_messages_page(&conn, "thread-page", first_page.next_before, Some(2), false)
                .unwrap();
        assert_eq!(
            second_page
                .messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg-1"]
        );
        assert!(!second_page.has_more);

        let _ = std::fs::remove_file(db_path);
    }
}
