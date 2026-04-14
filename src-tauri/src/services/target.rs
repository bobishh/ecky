use crate::db;
use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignOutput, LastDesignSnapshot, ModelManifest,
    PathResolver,
};
use crate::services::session::last_snapshot_path;
use std::fs;

pub struct ResolvedTarget {
    pub thread_id: String,
    pub message_id: String,
    pub design: Option<DesignOutput>,
    pub artifact_bundle: Option<ArtifactBundle>,
    pub model_manifest: Option<ModelManifest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditableTargetSource {
    Base,
}

impl EditableTargetSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Base => "base",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EditableTarget {
    pub thread_id: String,
    pub message_id: String,
    pub resolved_from: EditableTargetSource,
    pub design_output: DesignOutput,
    pub artifact_bundle: Option<ArtifactBundle>,
    pub model_manifest: Option<ModelManifest>,
}

impl EditableTarget {
    pub fn model_id(&self) -> Option<String> {
        self.artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone())
    }
}

pub fn resolve_target(
    conn: &rusqlite::Connection,
    app: &dyn PathResolver,
    thread_id: Option<String>,
    message_id: Option<String>,
) -> AppResult<ResolvedTarget> {
    if let Some(msg_id) = message_id {
        let (output, tid) = db::get_message_output_and_thread(conn, &msg_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| AppError::not_found(format!("Message {} not found.", msg_id)))?;

        if let Some(expected_thread_id) = thread_id.as_ref() {
            if expected_thread_id != &tid {
                return Err(AppError::validation(format!(
                    "Message {} does not belong to thread {}.",
                    msg_id, expected_thread_id
                )));
            }
        }

        let (artifact_bundle, model_manifest, _) =
            db::get_message_runtime_and_thread(conn, &msg_id)
                .map_err(|e| AppError::persistence(e.to_string()))?
                .unwrap_or((None, None, tid.clone()));

        return Ok(ResolvedTarget {
            thread_id: tid,
            message_id: msg_id,
            design: Some(output),
            artifact_bundle,
            model_manifest,
        });
    }

    if let Some(tid) = thread_id {
        db::get_visible_thread_title(conn, &tid)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| AppError::not_found(format!("Thread {} not found.", tid)))?;
        let message_id = db::get_latest_successful_message_id_in_thread(conn, &tid)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .ok_or_else(|| {
                AppError::validation(format!("Thread {} has no successful versions.", tid))
            })?;

        let target = resolve_target(conn, app, Some(tid.clone()), Some(message_id.clone()))?;

        return Ok(ResolvedTarget {
            thread_id: target.thread_id,
            message_id: target.message_id,
            design: target.design,
            artifact_bundle: target.artifact_bundle,
            model_manifest: target.model_manifest,
        });
    }

    // Try last_design.json
    let path = last_snapshot_path(app);
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(snapshot) = serde_json::from_str::<LastDesignSnapshot>(&data) {
                if let (Some(tid), Some(msg_id)) = (snapshot.thread_id, snapshot.message_id) {
                    return resolve_target(conn, app, Some(tid), Some(msg_id));
                }
            }
        }
    }

    Err(AppError::validation("No active target available."))
}

pub fn resolve_editable_target(
    conn: &rusqlite::Connection,
    app: &dyn PathResolver,
    thread_id: Option<String>,
    message_id: Option<String>,
) -> AppResult<EditableTarget> {
    let target = resolve_target(conn, app, thread_id, message_id)?;
    let design_output = target
        .design
        .clone()
        .ok_or_else(|| AppError::validation("Target has no design output."))?;

    Ok(EditableTarget {
        thread_id: target.thread_id,
        message_id: target.message_id,
        resolved_from: EditableTargetSource::Base,
        design_output,
        artifact_bundle: target.artifact_bundle,
        model_manifest: target.model_manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{ParamValue, UiSpec};
    use crate::models::{
        AppErrorCode, InteractionMode, MacroDialect, Message, MessageRole, MessageStatus,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use uuid::Uuid;

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

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn test_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{}-{}", name, Uuid::new_v4()))
    }

    fn sample_design(title: &str) -> DesignOutput {
        DesignOutput {
            title: title.to_string(),
            version_name: "v1".to_string(),
            response: "ok".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: "build()".to_string(),
            macro_dialect: MacroDialect::Legacy,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            ui_spec: UiSpec { fields: Vec::new() },
            initial_params: BTreeMap::from([("diameter".to_string(), ParamValue::Number(130.0))]),
            post_processing: None,
        }
    }

    #[test]
    fn resolve_target_rejects_message_ids_from_a_different_thread() {
        let root = std::env::temp_dir().join(format!("ecky-target-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestPathResolver { root };
        let conn = crate::db::init_db(&test_db_path("target-mismatch")).expect("db");

        crate::db::create_or_update_thread(
            &conn,
            "thread-1",
            "Thread One",
            1,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        crate::db::create_or_update_thread(
            &conn,
            "thread-2",
            "Thread Two",
            1,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        crate::db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "msg-1".to_string(),
                role: MessageRole::Assistant,
                content: "Base version".to_string(),
                status: MessageStatus::Success,
                output: Some(sample_design("Thread One Design")),
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: 1,
            },
        )
        .unwrap();

        let err = resolve_target(
            &conn,
            &resolver,
            Some("thread-2".to_string()),
            Some("msg-1".to_string()),
        )
        .err()
        .expect("mismatched target should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err
            .message
            .contains("Message msg-1 does not belong to thread thread-2."));
    }

    #[test]
    fn resolve_target_rejects_deleted_thread_targets() {
        let root = std::env::temp_dir().join(format!("ecky-target-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestPathResolver { root };
        let conn = crate::db::init_db(&test_db_path("target-deleted")).expect("db");

        crate::db::create_or_update_thread(
            &conn,
            "thread-1",
            "Thread One",
            1,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        crate::db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "msg-1".to_string(),
                role: MessageRole::Assistant,
                content: "Base version".to_string(),
                status: MessageStatus::Success,
                output: Some(sample_design("Thread One Design")),
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: 1,
            },
        )
        .unwrap();
        crate::db::delete_thread(&conn, "thread-1").unwrap();

        let thread_err = resolve_target(&conn, &resolver, Some("thread-1".to_string()), None)
            .err()
            .expect("deleted thread target should fail");
        assert_eq!(thread_err.code, AppErrorCode::NotFound);

        let message_err = resolve_target(&conn, &resolver, None, Some("msg-1".to_string()))
            .err()
            .expect("message from deleted thread should fail");
        assert_eq!(message_err.code, AppErrorCode::NotFound);
    }
}
