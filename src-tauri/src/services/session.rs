use crate::contracts::{AuthoringTargetRef, LastDesignSnapshot};
use crate::db;
use crate::models::PathResolver;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const RESTART_POINTER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LastDesignPointer {
    schema_version: u32,
    target: AuthoringTargetRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_part_id: Option<String>,
}

pub fn last_snapshot_path(app: &dyn PathResolver) -> PathBuf {
    app.app_config_dir().join("last_design.edn")
}

fn legacy_last_snapshot_path(app: &dyn PathResolver) -> PathBuf {
    app.app_config_dir().join("last_design.json")
}

pub fn write_last_snapshot(app: &dyn PathResolver, snapshot: Option<&LastDesignSnapshot>) {
    let path = last_snapshot_path(app);
    match snapshot {
        Some(snapshot) => {
            if snapshot.target_ref.is_none() {
                return;
            }
            if let Some(serialized) = serialize_restart_pointer(snapshot) {
                if fs::write(&path, serialized).is_ok() {
                    let _ = fs::remove_file(legacy_last_snapshot_path(app));
                }
            } else {
                let _ = fs::remove_file(path);
                let _ = fs::remove_file(legacy_last_snapshot_path(app));
            }
        }
        None => {
            let _ = fs::remove_file(path);
            let _ = fs::remove_file(legacy_last_snapshot_path(app));
        }
    }
}

pub fn read_last_snapshot(app: &dyn PathResolver, conn: &Connection) -> Option<LastDesignSnapshot> {
    let pointer = match fs::read(last_snapshot_path(app)) {
        Ok(data) => crate::strict_edn::from_slice::<LastDesignPointer>(&data).ok()?,
        Err(_) => {
            let legacy_path = legacy_last_snapshot_path(app);
            let data = fs::read_to_string(&legacy_path).ok()?;
            let pointer = serde_json::from_str::<LastDesignPointer>(&data)
                .ok()
                .or_else(|| legacy_pointer(&data))?;
            if let Ok(serialized) = crate::strict_edn::to_vec(&pointer) {
                if fs::write(last_snapshot_path(app), serialized).is_ok() {
                    let _ = fs::remove_file(legacy_path);
                }
            }
            pointer
        }
    };
    resolve_restart_pointer(conn, &pointer)
}

fn serialize_restart_pointer(snapshot: &LastDesignSnapshot) -> Option<Vec<u8>> {
    let target = snapshot.target_ref.clone()?;
    crate::strict_edn::to_vec(&LastDesignPointer {
        schema_version: RESTART_POINTER_SCHEMA_VERSION,
        target,
        selected_part_id: snapshot.selected_part_id.clone(),
    })
    .ok()
}

fn legacy_pointer(data: &str) -> Option<LastDesignPointer> {
    let snapshot = serde_json::from_str::<LastDesignSnapshot>(data).ok()?;
    let target = snapshot.target_ref.or_else(|| {
        Some(AuthoringTargetRef::SavedVersion {
            thread_id: snapshot.thread_id?,
            message_id: snapshot.message_id?,
        })
    })?;
    Some(LastDesignPointer {
        schema_version: RESTART_POINTER_SCHEMA_VERSION,
        target,
        selected_part_id: snapshot.selected_part_id,
    })
}

fn resolve_restart_pointer(
    conn: &Connection,
    pointer: &LastDesignPointer,
) -> Option<LastDesignSnapshot> {
    match &pointer.target {
        AuthoringTargetRef::Draft {
            thread_id,
            preview_id,
            session_id,
        } => {
            let draft = db::get_agent_draft_by_preview_id(conn, preview_id).ok()??;
            if &draft.thread_id != thread_id || &draft.session_id != session_id {
                return None;
            }
            Some(LastDesignSnapshot {
                design: Some(draft.design_output),
                thread_id: Some(draft.thread_id),
                message_id: Some(draft.preview_id),
                artifact_bundle: Some(draft.artifact_bundle),
                model_manifest: Some(draft.model_manifest),
                selected_part_id: pointer.selected_part_id.clone(),
                target_ref: Some(pointer.target.clone()),
            })
        }
        AuthoringTargetRef::SavedVersion {
            thread_id,
            message_id,
        } => resolve_saved_pointer(conn, thread_id, message_id, pointer),
        AuthoringTargetRef::LatestSaved { thread_id } => {
            let message_id = db::get_thread_latest_version(conn, thread_id).ok()??.id;
            resolve_saved_pointer(conn, thread_id, &message_id, pointer)
        }
    }
}

fn resolve_saved_pointer(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
    pointer: &LastDesignPointer,
) -> Option<LastDesignSnapshot> {
    let (design, output_thread_id) = db::get_message_output_and_thread(conn, message_id).ok()??;
    let (artifact_bundle, model_manifest, runtime_thread_id) =
        db::get_message_runtime_and_thread(conn, message_id).ok()??;
    if output_thread_id != thread_id || runtime_thread_id != thread_id {
        return None;
    }
    Some(LastDesignSnapshot {
        design: Some(design),
        thread_id: Some(thread_id.to_string()),
        message_id: Some(message_id.to_string()),
        artifact_bundle,
        model_manifest,
        selected_part_id: pointer.selected_part_id.clone(),
        target_ref: Some(AuthoringTargetRef::SavedVersion {
            thread_id: thread_id.to_string(),
            message_id: message_id.to_string(),
        }),
    })
}

pub fn build_saved_version_snapshot(
    design: Option<crate::contracts::DesignOutput>,
    thread_id: String,
    message_id: String,
    artifact_bundle: Option<crate::contracts::ArtifactBundle>,
    model_manifest: Option<crate::contracts::ModelManifest>,
    selected_part_id: Option<String>,
) -> LastDesignSnapshot {
    LastDesignSnapshot {
        design,
        thread_id: Some(thread_id.clone()),
        message_id: Some(message_id.clone()),
        artifact_bundle,
        model_manifest,
        selected_part_id,
        target_ref: Some(AuthoringTargetRef::SavedVersion {
            thread_id,
            message_id,
        }),
    }
}

pub fn build_draft_snapshot(
    design: Option<crate::contracts::DesignOutput>,
    thread_id: String,
    preview_id: String,
    session_id: String,
    artifact_bundle: Option<crate::contracts::ArtifactBundle>,
    model_manifest: Option<crate::contracts::ModelManifest>,
    selected_part_id: Option<String>,
) -> LastDesignSnapshot {
    LastDesignSnapshot {
        design,
        thread_id: Some(thread_id.clone()),
        message_id: Some(preview_id.clone()),
        artifact_bundle,
        model_manifest,
        selected_part_id,
        target_ref: Some(AuthoringTargetRef::Draft {
            thread_id,
            preview_id,
            session_id,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        AgentDraft, ArtifactBundle, AuthoringTargetRef, DesignOutput, EngineKind, GeometryBackend,
        InteractionMode, MacroDialect, Message, MessageRole, MessageStatus, ModelManifest,
        SourceLanguage, UiSpec,
    };

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

    fn design() -> DesignOutput {
        DesignOutput {
            title: "Recovered".to_string(),
            version_name: "V1".to_string(),
            response: String::new(),
            interaction_mode: InteractionMode::Design,
            macro_code: "(model (part body (box width 10 10)))".to_string(),
            macro_dialect: MacroDialect::EckyIrV0,
            engine_kind: EngineKind::EckyIrV0,
            source_language: SourceLanguage::EckyIrV0,
            geometry_backend: GeometryBackend::EckyRust,
            ui_spec: UiSpec::default(),
            initial_params: Default::default(),
            post_processing: None,
        }
    }

    fn bundle() -> ArtifactBundle {
        serde_json::from_value(serde_json::json!({
            "modelId": "model-1",
            "sourceKind": "generated",
            "engineKind": "eckyIrV0",
            "sourceLanguage": "eckyIrV0",
            "geometryBackend": "eckyRust",
            "contentHash": "sha256:artifact",
            "fcstdPath": "",
            "manifestPath": "/tmp/model-1.json",
            "modelStlPath": "/tmp/model-1.stl"
        }))
        .unwrap()
    }

    fn manifest() -> ModelManifest {
        serde_json::from_value(serde_json::json!({
            "modelId": "model-1",
            "sourceKind": "generated",
            "engineKind": "eckyIrV0",
            "sourceLanguage": "eckyIrV0",
            "geometryBackend": "eckyRust",
            "document": {
                "documentName": "Recovered",
                "documentLabel": "Recovered",
                "objectCount": 0,
                "warnings": []
            }
        }))
        .unwrap()
    }

    #[test]
    fn restart_cache_serializes_only_tagged_pointer() {
        let snapshot = LastDesignSnapshot {
            design: None,
            thread_id: Some("thread-1".to_string()),
            message_id: Some("preview-1".to_string()),
            artifact_bundle: None,
            model_manifest: None,
            selected_part_id: Some("part-1".to_string()),
            target_ref: Some(AuthoringTargetRef::Draft {
                thread_id: "thread-1".to_string(),
                preview_id: "preview-1".to_string(),
                session_id: "session-1".to_string(),
            }),
        };

        let serialized = serialize_restart_pointer(&snapshot).expect("tagged pointer");
        let text = std::str::from_utf8(&serialized).unwrap();
        let value: LastDesignPointer = crate::strict_edn::from_slice(&serialized).unwrap();

        assert!(text.contains(":schema-version"));
        assert!(text.contains(":selected-part-id"));
        assert_eq!(value.schema_version, 1);
        assert!(matches!(
            value.target,
            AuthoringTargetRef::Draft { ref preview_id, .. } if preview_id == "preview-1"
        ));
        assert_eq!(value.selected_part_id.as_deref(), Some("part-1"));
    }

    #[test]
    fn restart_pointer_writes_canonical_edn_only() {
        let root = std::env::temp_dir().join(format!(
            "ecky-last-design-edn-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("last_design.json"), "{}").unwrap();
        let resolver = TestResolver { root: root.clone() };
        let snapshot = LastDesignSnapshot {
            design: None,
            thread_id: Some("thread-1".to_string()),
            message_id: Some("message-1".to_string()),
            artifact_bundle: None,
            model_manifest: None,
            selected_part_id: None,
            target_ref: Some(AuthoringTargetRef::SavedVersion {
                thread_id: "thread-1".to_string(),
                message_id: "message-1".to_string(),
            }),
        };

        write_last_snapshot(&resolver, Some(&snapshot));

        assert!(root.join("last_design.edn").is_file());
        assert!(!root.join("last_design.json").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn untagged_snapshot_cannot_become_restart_authority() {
        let snapshot = LastDesignSnapshot {
            design: None,
            thread_id: Some("thread-1".to_string()),
            message_id: Some("message-1".to_string()),
            artifact_bundle: None,
            model_manifest: None,
            selected_part_id: None,
            target_ref: None,
        };

        assert!(serialize_restart_pointer(&snapshot).is_none());
    }

    #[test]
    fn saved_version_builder_always_tags_exact_restart_identity() {
        let snapshot = build_saved_version_snapshot(
            Some(design()),
            "thread-1".to_string(),
            "message-1".to_string(),
            Some(bundle()),
            Some(manifest()),
            None,
        );

        assert!(matches!(
            snapshot.target_ref,
            Some(AuthoringTargetRef::SavedVersion {
                ref thread_id,
                ref message_id,
            }) if thread_id == "thread-1" && message_id == "message-1"
        ));
    }

    #[test]
    fn untagged_preview_cannot_overwrite_saved_restart_pointer() {
        let root = std::env::temp_dir().join(format!(
            "ecky-last-design-untagged-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root: root.clone() };
        let saved = build_saved_version_snapshot(
            Some(design()),
            "thread-1".to_string(),
            "message-1".to_string(),
            Some(bundle()),
            Some(manifest()),
            None,
        );
        write_last_snapshot(&resolver, Some(&saved));
        let before = fs::read(last_snapshot_path(&resolver)).expect("saved pointer");
        let untagged_preview = LastDesignSnapshot {
            target_ref: None,
            message_id: Some("preview-untagged".to_string()),
            ..saved
        };

        write_last_snapshot(&resolver, Some(&untagged_preview));

        assert_eq!(
            fs::read(last_snapshot_path(&resolver)).expect("preserved saved pointer"),
            before
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn saved_pointer_recovers_from_database_without_payload_cache() {
        let root = std::env::temp_dir().join(format!(
            "ecky-saved-pointer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let conn = db::init_db(&root.join("history.sqlite")).unwrap();
        db::create_or_update_thread(&conn, "thread-1", "Recovered", 1, None).unwrap();
        db::add_message(
            &conn,
            "thread-1",
            &Message {
                id: "message-1".to_string(),
                parent_message_id: None,
                role: MessageRole::Assistant,
                content: "Recovered".to_string(),
                status: MessageStatus::Success,
                output: Some(design()),
                usage: None,
                artifact_bundle: Some(bundle()),
                model_manifest: Some(manifest()),
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: 1,
            },
        )
        .unwrap();

        let pointer = LastDesignPointer {
            schema_version: RESTART_POINTER_SCHEMA_VERSION,
            target: AuthoringTargetRef::SavedVersion {
                thread_id: "thread-1".to_string(),
                message_id: "message-1".to_string(),
            },
            selected_part_id: None,
        };

        let recovered = resolve_restart_pointer(&conn, &pointer).expect("durable saved version");
        assert_eq!(recovered.message_id.as_deref(), Some("message-1"));
        assert_eq!(recovered.design.unwrap().title, "Recovered");
        assert_eq!(recovered.artifact_bundle.unwrap().model_id, "model-1");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_pointer_recovers_from_durable_draft_without_process_cache() {
        let root = std::env::temp_dir().join(format!(
            "ecky-draft-pointer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let conn = db::init_db(&root.join("history.sqlite")).unwrap();
        db::create_or_update_thread(&conn, "thread-1", "Recovered", 1, None).unwrap();
        db::upsert_agent_draft(
            &conn,
            &AgentDraft {
                preview_id: "preview-1".to_string(),
                session_id: "session-1".to_string(),
                thread_id: "thread-1".to_string(),
                base_message_id: Some("message-1".to_string()),
                design_output: design(),
                artifact_bundle: bundle(),
                model_manifest: manifest(),
                draft_feedback: None,
                updated_at: 1,
            },
        )
        .unwrap();

        let pointer = LastDesignPointer {
            schema_version: RESTART_POINTER_SCHEMA_VERSION,
            target: AuthoringTargetRef::Draft {
                thread_id: "thread-1".to_string(),
                preview_id: "preview-1".to_string(),
                session_id: "session-1".to_string(),
            },
            selected_part_id: None,
        };

        let recovered = resolve_restart_pointer(&conn, &pointer).expect("durable draft");
        assert_eq!(recovered.message_id.as_deref(), Some("preview-1"));
        assert_eq!(recovered.design.unwrap().title, "Recovered");
        assert_eq!(recovered.model_manifest.unwrap().model_id, "model-1");
        let _ = fs::remove_dir_all(root);
    }
}
