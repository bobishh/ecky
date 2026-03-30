use crate::db;
use crate::mcp::runtime;
use crate::models::{
    AgentOrigin, AgentSession, AppError, AppResult, AppState, ArtifactBundle, DesignOutput,
    McpTargetRef, Message, MessageRole, MessageStatus, ModelManifest, PathResolver,
    TargetLeaseInfo,
};

const AGENT_TARGET_LEASE_TTL_SECS: u64 = 45;

#[derive(Debug, Clone)]
pub struct SaveOrUpdateAgentVersionRequest {
    pub session_id: String,
    pub thread_id: String,
    pub base_message_id: String,
    pub model_id: Option<String>,
    pub design_output: DesignOutput,
    pub artifact_bundle: Option<ArtifactBundle>,
    pub model_manifest: Option<ModelManifest>,
    pub updated_at: u64,
    pub response_text_created: String,
    pub response_text_updated: String,
    pub preserve_existing_title: bool,
    pub preserve_existing_version_name: bool,
    pub force_create_new_message: bool,
    pub announce_created_working_version: bool,
}

#[derive(Debug, Clone)]
pub struct SaveOrUpdateAgentVersionResult {
    pub thread_id: String,
    pub message_id: String,
    pub model_id: Option<String>,
    pub created: bool,
    pub version_name: String,
    pub agent_origin: AgentOrigin,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn default_agent_version_name() -> String {
    let now = chrono::Local::now();
    format!("V-mcp-{}", now.format("%Y%m%d-%H%M%S"))
}

pub fn build_agent_origin(
    session_id: &str,
    updated_at: u64,
    stored_session: Option<&AgentSession>,
    live_session: Option<&crate::models::McpSessionState>,
) -> AgentOrigin {
    if let Some(session) = stored_session {
        return AgentOrigin {
            host_label: session.host_label.clone(),
            client_kind: session.client_kind.clone(),
            agent_label: session.agent_label.clone(),
            llm_model_id: session.llm_model_id.clone(),
            llm_model_label: session.llm_model_label.clone(),
            session_id: session_id.to_string(),
            created_at: updated_at,
        };
    }

    if let Some(session) = live_session {
        return AgentOrigin {
            host_label: session.host_label.clone(),
            client_kind: session.client_kind.clone(),
            agent_label: session.agent_label.clone(),
            llm_model_id: session.llm_model_id.clone(),
            llm_model_label: session.llm_model_label.clone(),
            session_id: session_id.to_string(),
            created_at: updated_at,
        };
    }

    AgentOrigin {
        host_label: "MCP".to_string(),
        client_kind: "mcp".to_string(),
        agent_label: "External Agent".to_string(),
        llm_model_id: None,
        llm_model_label: None,
        session_id: session_id.to_string(),
        created_at: updated_at,
    }
}

pub fn updatable_agent_message_for_request(
    messages: &[Message],
    session_id: &str,
    base_message_id: &str,
) -> Option<Message> {
    messages
        .iter()
        .find(|message| {
            message.id == base_message_id
                && message.role == MessageRole::Assistant
                && message.status != MessageStatus::Error
                && (message.output.is_some() || message.artifact_bundle.is_some())
                && message
                    .agent_origin
                    .as_ref()
                    .map(|origin| origin.session_id.as_str())
                    == Some(session_id)
        })
        .cloned()
}

pub fn latest_agent_message_for_session(messages: &[Message], session_id: &str) -> Option<Message> {
    messages
        .iter()
        .rev()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message.status != MessageStatus::Error
                && (message.output.is_some() || message.artifact_bundle.is_some())
                && message
                    .agent_origin
                    .as_ref()
                    .map(|origin| origin.session_id.as_str())
                    == Some(session_id)
        })
        .cloned()
}

pub async fn save_or_update_agent_version_for_session(
    state: &AppState,
    _app: &dyn PathResolver,
    request: SaveOrUpdateAgentVersionRequest,
) -> AppResult<SaveOrUpdateAgentVersionResult> {
    let SaveOrUpdateAgentVersionRequest {
        session_id,
        thread_id,
        base_message_id,
        model_id,
        mut design_output,
        artifact_bundle,
        model_manifest,
        updated_at,
        response_text_created,
        response_text_updated,
        preserve_existing_title,
        preserve_existing_version_name,
        force_create_new_message,
        announce_created_working_version,
    } = request;

    let (stored_session, existing_agent_message) = {
        let conn = state.db.lock().await;
        let stored_session = db::get_sessions_by_ids(&conn, std::slice::from_ref(&session_id))
            .map_err(|e| AppError::persistence(e.to_string()))?
            .into_iter()
            .next();
        let existing_agent_message = if force_create_new_message {
            None
        } else {
            updatable_agent_message_for_request(
                &db::get_thread_messages(&conn, &thread_id)
                    .map_err(|e| AppError::persistence(e.to_string()))?,
                &session_id,
                &base_message_id,
            )
        };
        (stored_session, existing_agent_message)
    };

    let live_session = state.mcp_sessions.lock().await.get(&session_id).cloned();
    let now = now_secs();
    let existing_origin = existing_agent_message
        .as_ref()
        .and_then(|message| message.agent_origin.clone());
    let agent_origin = existing_origin.unwrap_or_else(|| {
        build_agent_origin(
            &session_id,
            updated_at.max(now),
            stored_session.as_ref(),
            live_session.as_ref(),
        )
    });

    if let Some(existing_output) = existing_agent_message
        .as_ref()
        .and_then(|message| message.output.as_ref())
    {
        if preserve_existing_title && !existing_output.title.trim().is_empty() {
            design_output.title = existing_output.title.clone();
        }
        if preserve_existing_version_name && !existing_output.version_name.trim().is_empty() {
            design_output.version_name = existing_output.version_name.clone();
        }
    } else if design_output.version_name.trim().is_empty() {
        design_output.version_name = default_agent_version_name();
    }
    let (next_ui_spec, next_params) = crate::models::reconcile_post_processing_controls(
        &design_output.ui_spec,
        &design_output.initial_params,
        design_output.post_processing.as_ref(),
    );
    design_output.ui_spec = next_ui_spec;
    design_output.initial_params = next_params;
    crate::models::validate_design_output(&design_output)?;

    let agent_label = agent_origin.agent_label.clone();
    let resolved_model_id = model_id
        .clone()
        .or_else(|| {
            model_manifest
                .as_ref()
                .map(|manifest| manifest.model_id.clone())
        })
        .or_else(|| {
            artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.clone())
        });

    let (message_id, created) = {
        let conn = state.db.lock().await;
        let thread_missing = db::get_thread_title(&conn, &thread_id)
            .map_err(|e| AppError::persistence(e.to_string()))?
            .is_none();
        let thread_traits = thread_missing.then(crate::generate_genie_traits);
        db::create_or_update_thread(
            &conn,
            &thread_id,
            &design_output.title,
            now,
            thread_traits.as_ref(),
            Some(design_output.engine_kind),
            Some(design_output.source_language),
            Some(design_output.geometry_backend),
        )
        .map_err(|e| AppError::persistence(e.to_string()))?;

        let message_id = if let Some(existing_message) = existing_agent_message.as_ref() {
            let update_text = if response_text_updated.trim().is_empty() {
                format!("{} updated the MCP working version.", agent_label)
            } else {
                response_text_updated.clone()
            };
            db::update_message_status_and_output(
                &conn,
                &existing_message.id,
                db::MessageStatusUpdate {
                    status: &MessageStatus::Success,
                    output: Some(&design_output),
                    usage: None,
                    artifact_bundle: artifact_bundle.as_ref(),
                    model_manifest: model_manifest.as_ref(),
                    visual_kind: None,
                    content: Some(update_text.as_str()),
                },
            )
            .map_err(|e| AppError::persistence(e.to_string()))?;
            existing_message.id.clone()
        } else {
            let message_id = uuid::Uuid::new_v4().to_string();
            let create_text = if response_text_created.trim().is_empty() {
                format!("{} created an MCP working version.", agent_label)
            } else {
                response_text_created.clone()
            };
            db::add_message(
                &conn,
                &thread_id,
                &Message {
                    id: message_id.clone(),
                    role: MessageRole::Assistant,
                    content: create_text,
                    status: MessageStatus::Success,
                    output: Some(design_output.clone()),
                    usage: None,
                    artifact_bundle: artifact_bundle.clone(),
                    model_manifest: model_manifest.clone(),
                    agent_origin: Some(agent_origin.clone()),
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .map_err(|e| AppError::persistence(e.to_string()))?;
            message_id
        };

        db::delete_target_leases_for_session(&conn, &session_id)
            .map_err(|e| AppError::persistence(e.to_string()))?;
        db::upsert_target_lease(
            &conn,
            &TargetLeaseInfo {
                session_id: session_id.clone(),
                thread_id: thread_id.clone(),
                message_id: message_id.clone(),
                model_id: resolved_model_id.clone(),
                host_label: agent_origin.host_label.clone(),
                agent_label: agent_origin.agent_label.clone(),
                acquired_at: now,
                expires_at: now + AGENT_TARGET_LEASE_TTL_SECS,
            },
        )
        .map_err(|e| AppError::persistence(e.to_string()))?;

        if let Some(mut session) = stored_session.clone() {
            session.thread_id = Some(thread_id.clone());
            session.message_id = Some(message_id.clone());
            session.model_id = resolved_model_id.clone();
            session.phase = "idle".to_string();
            session.updated_at = now;
            session.status_text = if existing_agent_message.is_some() {
                format!("{} updated {}.", agent_label, design_output.version_name)
            } else {
                format!("{} created {}.", agent_label, design_output.version_name)
            };
            db::upsert_agent_session(&conn, &session)
                .map_err(|e| AppError::persistence(e.to_string()))?;
        }

        (message_id, existing_agent_message.is_none())
    };

    let next_target = McpTargetRef {
        thread_id: thread_id.clone(),
        message_id: message_id.clone(),
        model_id: resolved_model_id.clone(),
    };
    {
        let mut sessions = state.mcp_sessions.lock().await;
        if let Some(session) = sessions.get_mut(&session_id) {
            session.last_target = Some(next_target.clone());
            session.phase = Some("saving_version".to_string());
            session.status_text = Some(if created {
                format!("{} created {}.", agent_label, design_output.version_name)
            } else {
                format!("{} updated {}.", agent_label, design_output.version_name)
            });
            session.busy = true;
            session.waiting_on_prompt = false;
            if session.activity_started_at.is_none() {
                session.activity_started_at = Some(now);
            }
            session.updated_at = now;
        }
    }
    runtime::associate_session_target(state, &session_id, Some(&next_target));
    let trace_summary = if created {
        format!("{} created {}.", agent_label, design_output.version_name)
    } else {
        format!("{} updated {}.", agent_label, design_output.version_name)
    };
    state.push_log(format!("[MCP] {}", trace_summary));
    state.push_log(format!(
        "[MCP] Bound session {} to {} / {}.",
        session_id, thread_id, design_output.version_name
    ));
    if runtime::runtime_snapshot_by_session_id(state, &session_id).is_some() {
        runtime::mark_managed_session_turn_busy(
            state,
            &session_id,
            Some(thread_id.clone()),
            agent_origin.llm_model_label.clone(),
            Some(trace_summary),
        );
    }

    if created && announce_created_working_version {
        state.emit_agent_working_version_created(&crate::contracts::AgentWorkingVersionEvent {
            session_id: session_id.clone(),
            thread_id: thread_id.clone(),
            message_id: message_id.clone(),
            model_id: resolved_model_id.clone(),
        });
    }

    Ok(SaveOrUpdateAgentVersionResult {
        thread_id,
        message_id,
        model_id: resolved_model_id,
        created,
        version_name: design_output.version_name,
        agent_origin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        Config, DisplacementSpec, McpConfig, ParamValue, PostProcessingSpec, ProjectionType,
        UiField, UiSpec,
    };
    use crate::models::{InteractionMode, MacroDialect};
    use std::collections::BTreeMap;
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

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn test_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{}-{}", name, uuid::Uuid::new_v4()))
    }

    fn test_config() -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            assets: Vec::new(),
            microwave: None,
            mcp: McpConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            default_engine_kind: crate::models::EngineKind::Freecad,
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::models::GeometryBackend::Freecad,
        }
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
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            ui_spec: UiSpec {
                fields: vec![UiField::Number {
                    key: "width".to_string(),
                    label: "width".to_string(),
                    min: None,
                    max: None,
                    step: None,
                    min_from: None,
                    max_from: None,
                    frozen: false,
                }],
            },
            initial_params: BTreeMap::from([(String::from("width"), ParamValue::Number(42.0))]),
            post_processing: None,
        }
    }

    #[tokio::test]
    async fn save_or_update_agent_version_creates_separate_agent_version_without_mutating_base() {
        let conn = crate::db::init_db(&test_db_path("agent-version-create")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let resolver = TestPathResolver {
            root: std::env::temp_dir().join(format!("ecky-agent-version-{}", uuid::Uuid::new_v4())),
        };
        std::fs::create_dir_all(&resolver.root).unwrap();
        let now = now_secs();

        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-1", "Thread", now, None, None, None, None)
                .unwrap();
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "base-1".to_string(),
                    role: MessageRole::Assistant,
                    content: "Base version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("Base", "V-user", "base_macro()")),
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: None,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
            db::upsert_agent_session(
                &conn,
                &AgentSession {
                    session_id: "session-1".to_string(),
                    client_kind: "http".to_string(),
                    host_label: "Claude Code".to_string(),
                    agent_label: "claude".to_string(),
                    llm_model_id: Some("claude-sonnet".to_string()),
                    llm_model_label: Some("Claude Sonnet".to_string()),
                    thread_id: Some("thread-1".to_string()),
                    message_id: Some("base-1".to_string()),
                    model_id: None,
                    phase: "rendering".to_string(),
                    status_text: "Working".to_string(),
                    updated_at: now,
                },
            )
            .unwrap();
        }

        let result = save_or_update_agent_version_for_session(
            &state,
            &resolver,
            SaveOrUpdateAgentVersionRequest {
                session_id: "session-1".to_string(),
                thread_id: "thread-1".to_string(),
                base_message_id: "base-1".to_string(),
                model_id: None,
                design_output: sample_design("Agent", "", "agent_macro()"),
                artifact_bundle: None,
                model_manifest: None,
                updated_at: now + 1,
                response_text_created: "Claude created an MCP working version.".to_string(),
                response_text_updated: "Claude updated the MCP working version.".to_string(),
                preserve_existing_title: true,
                preserve_existing_version_name: true,
                force_create_new_message: false,
                announce_created_working_version: false,
            },
        )
        .await
        .expect("save working version");

        assert!(result.created);
        assert_ne!(result.message_id, "base-1");

        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").unwrap();
        assert_eq!(messages.len(), 2);
        let base = messages
            .iter()
            .find(|message| message.id == "base-1")
            .unwrap();
        assert_eq!(
            base.output
                .as_ref()
                .map(|output| output.macro_code.as_str()),
            Some("base_macro()")
        );
        let agent = messages
            .iter()
            .find(|message| message.id == result.message_id)
            .unwrap();
        assert_eq!(
            agent
                .output
                .as_ref()
                .map(|output| output.macro_code.as_str()),
            Some("agent_macro()")
        );
        assert_eq!(
            agent
                .agent_origin
                .as_ref()
                .map(|origin| origin.session_id.as_str()),
            Some("session-1")
        );
        assert_eq!(
            db::get_active_target_lease(&conn, "thread-1", &result.message_id, None)
                .unwrap()
                .as_ref()
                .map(|lease| lease.session_id.as_str()),
            Some("session-1")
        );
    }

    #[tokio::test]
    async fn save_or_update_agent_version_inserts_missing_displacement_image_field() {
        let conn = crate::db::init_db(&test_db_path("agent-version-displacement")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let resolver = TestPathResolver {
            root: std::env::temp_dir().join(format!(
                "ecky-agent-version-displacement-{}",
                uuid::Uuid::new_v4()
            )),
        };
        std::fs::create_dir_all(&resolver.root).unwrap();
        let now = now_secs();

        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-1", "Thread", now, None, None, None, None)
                .unwrap();
            db::upsert_agent_session(
                &conn,
                &AgentSession {
                    session_id: "session-1".to_string(),
                    client_kind: "http".to_string(),
                    host_label: "Gemini CLI".to_string(),
                    agent_label: "gemini".to_string(),
                    llm_model_id: Some("gemini-2.5-pro".to_string()),
                    llm_model_label: Some("Gemini 2.5 Pro".to_string()),
                    thread_id: Some("thread-1".to_string()),
                    message_id: None,
                    model_id: None,
                    phase: "rendering".to_string(),
                    status_text: "Working".to_string(),
                    updated_at: now,
                },
            )
            .unwrap();
        }

        let mut design = sample_design("Lithophane", "", "agent_macro()");
        design.post_processing = Some(PostProcessingSpec {
            displacement: Some(DisplacementSpec {
                image_param: "image_path".to_string(),
                projection: ProjectionType::Planar,
                depth_mm: 2.0,
                invert: true,
            }),
            lithophane_attachments: vec![],
        });

        let result = save_or_update_agent_version_for_session(
            &state,
            &resolver,
            SaveOrUpdateAgentVersionRequest {
                session_id: "session-1".to_string(),
                thread_id: "thread-1".to_string(),
                base_message_id: "base-1".to_string(),
                model_id: None,
                design_output: design,
                artifact_bundle: None,
                model_manifest: None,
                updated_at: now + 1,
                response_text_created: "Gemini created an MCP working version.".to_string(),
                response_text_updated: "Gemini updated the MCP working version.".to_string(),
                preserve_existing_title: true,
                preserve_existing_version_name: true,
                force_create_new_message: false,
                announce_created_working_version: false,
            },
        )
        .await
        .expect("save working version");

        let conn = state.db.lock().await;
        let saved = db::get_thread_messages(&conn, "thread-1")
            .unwrap()
            .into_iter()
            .find(|message| message.id == result.message_id)
            .and_then(|message| message.output)
            .expect("saved output");

        assert!(saved.ui_spec.fields.iter().any(|field| matches!(
            field,
            UiField::Image { key, .. } if key == "image_path"
        )));
        assert_eq!(
            saved.initial_params.get("image_path"),
            Some(&ParamValue::String(String::new()))
        );
    }

    #[tokio::test]
    async fn save_or_update_agent_version_updates_explicit_agent_base_message_for_same_session() {
        let conn = crate::db::init_db(&test_db_path("agent-version-update")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let resolver = TestPathResolver {
            root: std::env::temp_dir().join(format!("ecky-agent-version-{}", uuid::Uuid::new_v4())),
        };
        std::fs::create_dir_all(&resolver.root).unwrap();
        let now = now_secs();

        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-1", "Thread", now, None, None, None, None)
                .unwrap();
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "base-1".to_string(),
                    role: MessageRole::Assistant,
                    content: "Base version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("Base", "V-user", "base_macro()")),
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
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
                    id: "agent-1".to_string(),
                    role: MessageRole::Assistant,
                    content: "Claude created an MCP working version.".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("Agent", "V-mcp-keep", "old_macro()")),
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: Some(AgentOrigin {
                        host_label: "Claude Code".to_string(),
                        client_kind: "http".to_string(),
                        agent_label: "claude".to_string(),
                        llm_model_id: Some("claude-sonnet".to_string()),
                        llm_model_label: Some("Claude Sonnet".to_string()),
                        session_id: "session-1".to_string(),
                        created_at: now,
                    }),
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
            db::upsert_agent_session(
                &conn,
                &AgentSession {
                    session_id: "session-1".to_string(),
                    client_kind: "http".to_string(),
                    host_label: "Claude Code".to_string(),
                    agent_label: "claude".to_string(),
                    llm_model_id: Some("claude-sonnet".to_string()),
                    llm_model_label: Some("Claude Sonnet".to_string()),
                    thread_id: Some("thread-1".to_string()),
                    message_id: Some("agent-1".to_string()),
                    model_id: None,
                    phase: "rendering".to_string(),
                    status_text: "Working".to_string(),
                    updated_at: now,
                },
            )
            .unwrap();
        }

        let result = save_or_update_agent_version_for_session(
            &state,
            &resolver,
            SaveOrUpdateAgentVersionRequest {
                session_id: "session-1".to_string(),
                thread_id: "thread-1".to_string(),
                base_message_id: "agent-1".to_string(),
                model_id: None,
                design_output: sample_design("Agent", "", "new_macro()"),
                artifact_bundle: None,
                model_manifest: None,
                updated_at: now + 1,
                response_text_created: "Claude created an MCP working version.".to_string(),
                response_text_updated: "Claude updated the MCP working version.".to_string(),
                preserve_existing_title: true,
                preserve_existing_version_name: true,
                force_create_new_message: false,
                announce_created_working_version: false,
            },
        )
        .await
        .expect("update working version");

        assert!(!result.created);
        assert_eq!(result.message_id, "agent-1");
        assert_eq!(result.version_name, "V-mcp-keep");

        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").unwrap();
        assert_eq!(messages.len(), 2);
        let agent = messages
            .iter()
            .find(|message| message.id == "agent-1")
            .unwrap();
        assert_eq!(
            agent
                .output
                .as_ref()
                .map(|output| output.macro_code.as_str()),
            Some("new_macro()")
        );
        assert_eq!(
            agent
                .output
                .as_ref()
                .map(|output| output.version_name.as_str()),
            Some("V-mcp-keep")
        );
        let session = db::get_sessions_by_ids(&conn, &[String::from("session-1")])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(session.message_id.as_deref(), Some("agent-1"));
    }

    #[tokio::test]
    async fn save_or_update_agent_version_does_not_overwrite_unrelated_agent_message() {
        let conn = crate::db::init_db(&test_db_path("agent-version-no-overwrite")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let resolver = TestPathResolver {
            root: std::env::temp_dir().join(format!("ecky-agent-version-{}", uuid::Uuid::new_v4())),
        };
        std::fs::create_dir_all(&resolver.root).unwrap();
        let now = now_secs();

        {
            let conn = state.db.lock().await;
            db::create_or_update_thread(&conn, "thread-1", "Thread", now, None, None, None, None)
                .unwrap();
            db::add_message(
                &conn,
                "thread-1",
                &Message {
                    id: "base-1".to_string(),
                    role: MessageRole::Assistant,
                    content: "Base version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("Base", "V-user", "base_macro()")),
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
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
                    id: "agent-1".to_string(),
                    role: MessageRole::Assistant,
                    content: "Claude created an MCP working version.".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_design("Agent", "V-mcp-keep", "old_macro()")),
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    agent_origin: Some(AgentOrigin {
                        host_label: "Claude Code".to_string(),
                        client_kind: "http".to_string(),
                        agent_label: "claude".to_string(),
                        llm_model_id: Some("claude-sonnet".to_string()),
                        llm_model_label: Some("Claude Sonnet".to_string()),
                        session_id: "session-1".to_string(),
                        created_at: now,
                    }),
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                    timestamp: now,
                },
            )
            .unwrap();
        }

        let result = save_or_update_agent_version_for_session(
            &state,
            &resolver,
            SaveOrUpdateAgentVersionRequest {
                session_id: "session-1".to_string(),
                thread_id: "thread-1".to_string(),
                base_message_id: "base-1".to_string(),
                model_id: None,
                design_output: sample_design("Agent", "", "new_macro()"),
                artifact_bundle: None,
                model_manifest: None,
                updated_at: now + 1,
                response_text_created: "Claude created an MCP working version.".to_string(),
                response_text_updated: "Claude updated the MCP working version.".to_string(),
                preserve_existing_title: true,
                preserve_existing_version_name: true,
                force_create_new_message: false,
                announce_created_working_version: false,
            },
        )
        .await
        .expect("create new working version");

        assert!(result.created);
        assert_ne!(result.message_id, "agent-1");

        let conn = state.db.lock().await;
        let messages = db::get_thread_messages(&conn, "thread-1").unwrap();
        assert_eq!(messages.len(), 3);
        let prior_agent = messages
            .iter()
            .find(|message| message.id == "agent-1")
            .unwrap();
        assert_eq!(
            prior_agent
                .output
                .as_ref()
                .map(|output| output.macro_code.as_str()),
            Some("old_macro()")
        );
    }
}
