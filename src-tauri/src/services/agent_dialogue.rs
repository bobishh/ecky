use crate::contracts::{Attachment, AttachmentKind};
use crate::db;
use crate::models::{AgentOrigin, AppError, AppResult, AppState, Message};

#[derive(Debug, Clone)]
pub struct AgentDialogueIdentity {
    pub session_id: String,
    pub client_kind: String,
    pub host_label: String,
    pub agent_label: String,
    pub llm_model_id: Option<String>,
    pub llm_model_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionThreadTarget {
    pub thread_id: String,
    pub message_id: Option<String>,
    pub model_id: Option<String>,
}

pub fn build_agent_origin(identity: &AgentDialogueIdentity, created_at: u64) -> AgentOrigin {
    AgentOrigin {
        host_label: identity.host_label.clone(),
        client_kind: identity.client_kind.clone(),
        agent_label: identity.agent_label.clone(),
        llm_model_id: identity.llm_model_id.clone(),
        llm_model_label: identity.llm_model_label.clone(),
        session_id: identity.session_id.clone(),
        created_at,
    }
}

pub fn default_prompt_request_message(agent_label: &str) -> String {
    format!("{} is waiting for your input.", agent_label)
}

pub fn normalize_prompt_request_message(message: Option<&str>, agent_label: &str) -> String {
    message
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_prompt_request_message(agent_label))
}

pub fn collect_attachment_image_paths(attachments: &[Attachment]) -> Vec<String> {
    attachments
        .iter()
        .filter(|attachment| attachment.kind == AttachmentKind::Image)
        .filter_map(|attachment| {
            attachment
                .data_url
                .as_ref()
                .cloned()
                .or_else(|| (!attachment.path.trim().is_empty()).then_some(attachment.path.clone()))
        })
        .collect()
}

pub fn build_user_reply_message_content(prompt_text: &str, attachments: &[Attachment]) -> String {
    let trimmed = prompt_text.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    let count = attachments.len();
    if count == 0 {
        return "Shared a follow-up reply.".to_string();
    }

    let attachment_names = attachments
        .iter()
        .take(3)
        .map(|attachment| attachment.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if attachment_names.is_empty() {
        return format!(
            "Shared {} attachment{}.",
            count,
            if count == 1 { "" } else { "s" }
        );
    }

    let suffix = if count > attachment_names.len() {
        format!(" (+{} more)", count - attachment_names.len())
    } else {
        String::new()
    };
    format!(
        "Shared attachment{}: {}{}",
        if count == 1 { "" } else { "s" },
        attachment_names.join(", "),
        suffix
    )
}

pub async fn resolve_session_thread_target(
    state: &AppState,
    session_id: &str,
) -> AppResult<Option<SessionThreadTarget>> {
    let live_target = {
        let sessions = state.mcp_sessions.lock().await;
        sessions.get(session_id).and_then(|session| {
            session
                .last_target
                .clone()
                .map(|target| SessionThreadTarget {
                    thread_id: target.thread_id,
                    message_id: Some(target.message_id),
                    model_id: target.model_id,
                })
                .or_else(|| {
                    session
                        .bound_thread_id
                        .clone()
                        .map(|thread_id| SessionThreadTarget {
                            thread_id,
                            message_id: None,
                            model_id: None,
                        })
                })
        })
    };
    if live_target.is_some() {
        return Ok(live_target);
    }

    if let Some(runtime_target) =
        crate::mcp::runtime::runtime_snapshot_by_session_id(state, session_id)
    {
        if let Some(thread_id) = runtime_target.pending_thread_id {
            return Ok(Some(SessionThreadTarget {
                thread_id,
                message_id: runtime_target.pending_message_id,
                model_id: runtime_target.pending_model_id,
            }));
        }
    }

    let conn = state.db.lock().await;
    let stored_session = db::get_sessions_by_ids(&conn, &[session_id.to_string()])
        .map_err(|err| AppError::persistence(err.to_string()))?
        .into_iter()
        .next();

    let stored_target = stored_session.and_then(|session| {
        session.thread_id.map(|thread_id| SessionThreadTarget {
            thread_id,
            message_id: session.message_id,
            model_id: session.model_id,
        })
    });
    Ok(stored_target)
}

pub fn current_snapshot_target(state: &AppState) -> Option<SessionThreadTarget> {
    let snapshot = state.last_snapshot.lock().unwrap().clone()?;
    let thread_id = snapshot.thread_id?;
    Some(SessionThreadTarget {
        thread_id,
        message_id: snapshot.message_id,
        model_id: snapshot.artifact_bundle.map(|bundle| bundle.model_id),
    })
}

pub async fn add_dialogue_message(
    state: &AppState,
    thread_id: &str,
    message: &Message,
) -> AppResult<()> {
    let conn = state.db.lock().await;
    db::add_message(&conn, thread_id, message).map_err(|err| AppError::persistence(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Config, McpConfig};
    use crate::models::{AppState, McpSessionState, McpTargetRef};
    use std::path::PathBuf;

    fn test_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ecky-agent-dialogue-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ))
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
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::models::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
        }
    }

    #[test]
    fn user_reply_content_falls_back_to_attachment_summary() {
        let content = build_user_reply_message_content(
            "",
            &[Attachment {
                path: "/tmp/ref.png".to_string(),
                name: "ref.png".to_string(),
                explanation: String::new(),
                data_url: None,
                kind: AttachmentKind::Image,
            }],
        );

        assert_eq!(content, "Shared attachment: ref.png");
    }

    #[test]
    fn collect_attachment_image_paths_ignores_non_images() {
        let attachments = vec![
            Attachment {
                path: "/tmp/ref.png".to_string(),
                name: "ref.png".to_string(),
                explanation: String::new(),
                data_url: None,
                kind: AttachmentKind::Image,
            },
            Attachment {
                path: "/tmp/model.fcstd".to_string(),
                name: "model.fcstd".to_string(),
                explanation: String::new(),
                data_url: None,
                kind: AttachmentKind::Cad,
            },
        ];

        assert_eq!(
            collect_attachment_image_paths(&attachments),
            vec!["/tmp/ref.png".to_string()]
        );
    }

    #[test]
    fn collect_attachment_image_paths_prefers_inline_image_payloads() {
        let attachments = vec![Attachment {
            path: String::new(),
            name: "ref.png".to_string(),
            explanation: String::new(),
            data_url: Some("data:image/png;base64,Zm9v".to_string()),
            kind: AttachmentKind::Image,
        }];

        assert_eq!(
            collect_attachment_image_paths(&attachments),
            vec!["data:image/png;base64,Zm9v".to_string()]
        );
    }

    #[tokio::test]
    async fn resolve_session_thread_target_prefers_live_target() {
        let conn = crate::db::init_db(&test_db_path("live-target")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        state.mcp_sessions.lock().await.insert(
            "session-1".to_string(),
            McpSessionState {
                client_kind: "mcp-http".to_string(),
                host_label: "Claude".to_string(),
                agent_label: "Claude".to_string(),
                llm_model_id: None,
                llm_model_label: None,
                bound_thread_id: None,
                last_target: Some(McpTargetRef {
                    thread_id: "thread-live".to_string(),
                    message_id: "msg-live".to_string(),
                    model_id: Some("model-live".to_string()),
                }),
                phase: None,
                status_text: None,
                busy: false,
                activity_label: None,
                activity_started_at: None,
                attention_kind: None,
                waiting_on_prompt: false,
                current_turn_id: None,
                current_turn_thread_id: None,
                current_turn_working_message_ids: Vec::new(),
                current_turn_working_version_message_id: None,
                updated_at: 0,
            },
        );

        let target = resolve_session_thread_target(&state, "session-1")
            .await
            .expect("target")
            .expect("live target");
        assert_eq!(target.thread_id, "thread-live");
        assert_eq!(target.message_id.as_deref(), Some("msg-live"));
        assert_eq!(target.model_id.as_deref(), Some("model-live"));
    }

    #[tokio::test]
    async fn resolve_session_thread_target_falls_back_to_bound_thread() {
        let conn = crate::db::init_db(&test_db_path("bound-thread")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        state.mcp_sessions.lock().await.insert(
            "session-1".to_string(),
            McpSessionState {
                client_kind: "mcp-http".to_string(),
                host_label: "Claude".to_string(),
                agent_label: "Claude".to_string(),
                llm_model_id: None,
                llm_model_label: None,
                bound_thread_id: Some("thread-bound".to_string()),
                last_target: None,
                phase: Some("idle".to_string()),
                status_text: None,
                busy: false,
                activity_label: None,
                activity_started_at: None,
                attention_kind: None,
                waiting_on_prompt: false,
                current_turn_id: None,
                current_turn_thread_id: None,
                current_turn_working_message_ids: Vec::new(),
                current_turn_working_version_message_id: None,
                updated_at: 0,
            },
        );

        let target = resolve_session_thread_target(&state, "session-1")
            .await
            .expect("target")
            .expect("bound thread");
        assert_eq!(target.thread_id, "thread-bound");
        assert_eq!(target.message_id, None);
        assert_eq!(target.model_id, None);
    }

    #[tokio::test]
    async fn resolve_session_thread_target_falls_back_to_runtime_pending_target() {
        let conn = crate::db::init_db(&test_db_path("runtime-target")).expect("db");
        let mut config = test_config();
        config.connection_type = Some("mcp".to_string());
        config.mcp.mode = crate::contracts::McpMode::Active;
        config.mcp.primary_agent_id = Some("agent-claude".to_string());
        config.mcp.auto_agents = vec![crate::contracts::AutoAgent {
            id: "agent-claude".to_string(),
            label: "Claude".to_string(),
            cmd: "claude".to_string(),
            model: None,
            args: Vec::new(),
            enabled: true,
            start_on_demand: true,
        }];
        let state = AppState::new(config, None, conn);
        crate::mcp::runtime::initialize_auto_agent_supervisors(state.clone());

        crate::mcp::runtime::wake_primary_auto_agent(
            &state,
            Some("thread-runtime".to_string()),
            Some("msg-runtime".to_string()),
            Some("model-runtime".to_string()),
        )
        .await
        .expect("wake");

        crate::mcp::runtime::bind_managed_http_session(
            &state,
            "agent-claude",
            "session-1",
            Some("Connected".to_string()),
        );
        crate::mcp::runtime::associate_session_target(
            &state,
            "session-1",
            Some(&McpTargetRef {
                thread_id: "thread-runtime".to_string(),
                message_id: "msg-runtime".to_string(),
                model_id: Some("model-runtime".to_string()),
            }),
        );

        let target = resolve_session_thread_target(&state, "session-1")
            .await
            .expect("target")
            .expect("runtime target");
        assert_eq!(target.thread_id, "thread-runtime");
        assert_eq!(target.message_id.as_deref(), Some("msg-runtime"));
        assert_eq!(target.model_id.as_deref(), Some("model-runtime"));
    }

    #[tokio::test]
    async fn resolve_session_thread_target_falls_back_to_persisted_session_row() {
        let conn = crate::db::init_db(&test_db_path("stored-target")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        {
            let conn = state.db.lock().await;
            crate::db::upsert_agent_session(
                &conn,
                &crate::contracts::AgentSession {
                    session_id: "session-1".to_string(),
                    client_kind: "mcp-http".to_string(),
                    host_label: "Claude".to_string(),
                    agent_label: "Claude".to_string(),
                    llm_model_id: None,
                    llm_model_label: None,
                    thread_id: Some("thread-db".to_string()),
                    message_id: Some("msg-db".to_string()),
                    model_id: Some("model-db".to_string()),
                    phase: "reading".to_string(),
                    status_text: String::new(),
                    updated_at: 1,
                },
            )
            .expect("session row");
        }

        let target = resolve_session_thread_target(&state, "session-1")
            .await
            .expect("target")
            .expect("stored target");
        assert_eq!(target.thread_id, "thread-db");
        assert_eq!(target.message_id.as_deref(), Some("msg-db"));
        assert_eq!(target.model_id.as_deref(), Some("model-db"));
    }

    #[tokio::test]
    async fn resolve_session_thread_target_does_not_fall_back_to_current_snapshot_target() {
        let conn = crate::db::init_db(&test_db_path("snapshot-target")).expect("db");
        let state = AppState::new(
            test_config(),
            Some(crate::models::LastDesignSnapshot {
                design: None,
                thread_id: Some("thread-snapshot".to_string()),
                message_id: Some("msg-snapshot".to_string()),
                artifact_bundle: Some(crate::models::ArtifactBundle {
                    schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
                    model_id: "model-snapshot".to_string(),
                    source_kind: crate::models::ModelSourceKind::Generated,
                    engine_kind: crate::models::EngineKind::Freecad,
                    source_language: crate::models::SourceLanguage::LegacyPython,
                    geometry_backend: crate::models::GeometryBackend::Freecad,
                    content_hash: "hash-snapshot".to_string(),
                    artifact_version: 1,
                    fcstd_path: "/tmp/model.FCStd".to_string(),
                    manifest_path: "/tmp/manifest.json".to_string(),
                    macro_path: Some("/tmp/source.py".to_string()),
                    preview_stl_path: "/tmp/preview.stl".to_string(),
                    viewer_assets: Vec::new(),
                    edge_targets: Vec::new(),
                    face_targets: Vec::new(),
                    callout_anchors: Vec::new(),
                    measurement_guides: Vec::new(),
                    export_artifacts: Vec::new(),
                }),
                model_manifest: None,
                selected_part_id: None,
            }),
            conn,
        );

        let target = resolve_session_thread_target(&state, "session-1")
            .await
            .expect("target");
        assert_eq!(target, None);
    }
}
