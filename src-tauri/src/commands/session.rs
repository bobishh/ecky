use std::fs;
use std::io::Write;
use std::path::Path;

use portable_pty::PtySize;
use tauri::{AppHandle, Manager, State};

use crate::contracts::{AgentSession, AppError, AppResult, LastDesignSnapshot, McpServerStatus};
use crate::db;
use crate::mcp::runtime;
use crate::models::{AppState, ViewportScreenshotCapture};
use crate::services::agent_dialogue;
use crate::services::managed_jev_trace;

fn encode_control_key(key: &str) -> Option<u8> {
    if key.eq_ignore_ascii_case("space") {
        return Some(0);
    }

    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }

    match ch {
        'a'..='z' | 'A'..='Z' => Some((ch.to_ascii_uppercase() as u8) & 0x1f),
        '@' | '`' | ' ' => Some(0),
        '[' => Some(27),
        '\\' => Some(28),
        ']' => Some(29),
        '^' | '6' => Some(30),
        '_' | '-' => Some(31),
        _ => None,
    }
}

fn encode_named_terminal_key(key: &str) -> Option<&'static [u8]> {
    match key {
        "Enter" => Some(b"\r"),
        "Tab" => Some(b"\t"),
        "Escape" => Some(b"\x1b"),
        "Backspace" => Some(b"\x7f"),
        "Delete" => Some(b"\x1b[3~"),
        "ArrowUp" => Some(b"\x1b[A"),
        "ArrowDown" => Some(b"\x1b[B"),
        "ArrowRight" => Some(b"\x1b[C"),
        "ArrowLeft" => Some(b"\x1b[D"),
        "Home" => Some(b"\x1b[H"),
        "End" => Some(b"\x1b[F"),
        "PageUp" => Some(b"\x1b[5~"),
        "PageDown" => Some(b"\x1b[6~"),
        "Insert" => Some(b"\x1b[2~"),
        _ => None,
    }
}

fn encode_terminal_key_input(key: &str, ctrl: bool, alt: bool) -> AppResult<Vec<u8>> {
    let mut payload = Vec::new();
    if alt {
        payload.push(0x1b);
    }

    let bytes = if ctrl {
        vec![encode_control_key(key).ok_or_else(|| {
            AppError::validation(format!("Unsupported terminal control key: {}", key))
        })?]
    } else if let Some(named) = encode_named_terminal_key(key) {
        named.to_vec()
    } else {
        let mut chars = key.chars();
        let ch = chars
            .next()
            .ok_or_else(|| AppError::validation("Terminal key input must not be empty."))?;
        if chars.next().is_some() {
            return Err(AppError::validation(format!(
                "Unsupported terminal key: {}",
                key
            )));
        }
        let mut buffer = [0_u8; 4];
        ch.encode_utf8(&mut buffer).as_bytes().to_vec()
    };

    payload.extend(bytes);
    Ok(payload)
}

fn encode_agent_terminal_input(input: &crate::contracts::AgentTerminalInput) -> AppResult<Vec<u8>> {
    let mut payload = Vec::new();

    if !input.text.is_empty() {
        payload.extend_from_slice(input.text.as_bytes());
    }

    if let Some(key) = input.key.as_deref() {
        payload.extend(encode_terminal_key_input(key, input.ctrl, input.alt)?);
    }

    if input.submit || (payload.is_empty() && input.key.is_none()) {
        payload.extend_from_slice(b"\r");
    }

    if payload.is_empty() {
        return Err(AppError::validation(
            "Agent terminal input is empty and produced no PTY bytes.",
        ));
    }

    Ok(payload)
}

#[tauri::command]
#[specta::specta]
pub async fn get_active_agent_sessions(state: State<'_, AppState>) -> AppResult<Vec<AgentSession>> {
    let conn = state.db.lock().await;
    db::get_active_agent_sessions(&conn, 600)
        .map_err(|e| crate::contracts::AppError::persistence(e.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn get_mcp_server_status(state: State<'_, AppState>) -> AppResult<McpServerStatus> {
    Ok(state.mcp_status())
}

#[tauri::command]
#[specta::specta]
pub async fn get_agent_terminal_snapshots(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::contracts::AgentTerminalSnapshot>> {
    Ok(state
        .agent_terminals
        .lock()
        .unwrap()
        .values()
        .map(|runtime| runtime.snapshot.clone())
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn send_agent_terminal_input(
    input: crate::contracts::AgentTerminalInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let writer = {
        let mut terminals = state.agent_terminals.lock().unwrap();
        let Some(runtime) = terminals.get_mut(&input.agent_id) else {
            return Err(AppError::not_found(format!(
                "No active terminal for agent {}.",
                input.agent_id
            )));
        };
        if !runtime.snapshot.active {
            return Err(AppError::validation(format!(
                "{} terminal is not accepting input right now.",
                runtime.snapshot.agent_label
            )));
        }
        runtime.snapshot.attention_required = false;
        runtime.snapshot.summary = None;
        runtime.snapshot.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let snapshot = runtime.snapshot.clone();
        let writer = runtime.writer.clone();
        drop(terminals);
        state.emit_agent_terminal_update(&snapshot);
        writer
    };

    let payload = encode_agent_terminal_input(&input)?;

    let mut writer = writer.lock().unwrap();
    writer
        .write_all(&payload)
        .map_err(|err| AppError::internal(format!("Failed to write to agent terminal: {}", err)))?;
    writer
        .flush()
        .map_err(|err| AppError::internal(format!("Failed to flush agent terminal: {}", err)))?;
    Ok(())
}

fn resize_agent_terminal_impl(
    agent_id: &str,
    cols: u16,
    rows: u16,
    state: &AppState,
) -> AppResult<()> {
    if cols < 2 || rows < 1 {
        return Err(AppError::validation(format!(
            "Invalid PTY size {}x{}.",
            cols, rows
        )));
    }

    let pty = {
        let terminals = state.agent_terminals.lock().unwrap();
        let Some(runtime) = terminals.get(agent_id) else {
            return Err(AppError::not_found(format!(
                "No active terminal for agent {}.",
                agent_id
            )));
        };
        if !runtime.snapshot.active {
            return Err(AppError::validation(format!(
                "{} terminal is not accepting resize right now.",
                runtime.snapshot.agent_label
            )));
        }
        runtime.pty.clone()
    };

    let pty = pty.lock().unwrap();
    pty.resize(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
    .map_err(|err| AppError::internal(format!("Failed to resize agent terminal: {}", err)))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn resize_agent_terminal(
    agent_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, AppState>,
) -> AppResult<()> {
    resize_agent_terminal_impl(&agent_id, cols, rows, &state)
}

fn sanitize_attachment_file_name(name: &str) -> String {
    let trimmed = name.trim();
    let candidate = if trimmed.is_empty() {
        "attachment"
    } else {
        trimmed
    };
    candidate
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '\0' => '_',
            _ => ch,
        })
        .collect()
}

fn parse_workspace_capture_data_url(data_url: &str) -> AppResult<(&'static str, String)> {
    let Some(rest) = data_url.strip_prefix("data:") else {
        return Err(AppError::validation(
            "Workspace capture did not return a data URL.",
        ));
    };
    let Some((metadata, payload)) = rest.split_once(',') else {
        return Err(AppError::validation(
            "Workspace capture data URL is malformed.",
        ));
    };
    let mut parts = metadata.split(';');
    let mime_type = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::validation("Workspace capture is missing a MIME type."))?;
    if !parts.any(|part| part.eq_ignore_ascii_case("base64")) {
        return Err(AppError::validation(
            "Workspace capture must use base64 encoding.",
        ));
    }
    if payload.trim().is_empty() {
        return Err(AppError::validation("Workspace capture payload is empty."));
    }
    let extension = match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        other => {
            return Err(AppError::validation(format!(
                "Unsupported workspace capture MIME type: {}",
                other
            )));
        }
    };
    Ok((extension, payload.to_string()))
}

fn stage_prompt_workspace_capture_to_dir(
    input: &crate::contracts::PreparePromptWorkspaceCaptureInput,
) -> AppResult<crate::contracts::Attachment> {
    let (extension, _) = parse_workspace_capture_data_url(&input.data_url)?;
    let requested_name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("workspace-view");
    let requested_name = sanitize_attachment_file_name(requested_name);
    let file_stem = Path::new(&requested_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("workspace-view");
    let display_name = if requested_name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(extension))
    {
        requested_name
    } else {
        format!("{}.{}", file_stem, extension)
    };
    Ok(crate::contracts::Attachment {
        path: String::new(),
        name: display_name,
        explanation: input
            .explanation
            .clone()
            .unwrap_or_else(|| "Current workspace view.".to_string()),
        data_url: Some(input.data_url.clone()),
        kind: crate::contracts::AttachmentKind::Image,
    })
}

fn stage_prompt_attachments_to_dir(
    staging_root: &Path,
    request_id: &str,
    session_id: Option<&str>,
    attachments: &[crate::contracts::Attachment],
) -> AppResult<Vec<crate::contracts::Attachment>> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }

    let session_folder = session_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("standalone-session");
    let target_dir = staging_root
        .join("mcp-attachments")
        .join(session_folder)
        .join(request_id);
    fs::create_dir_all(&target_dir).map_err(|err| {
        AppError::internal(format!(
            "Failed to prepare staged attachment directory {}: {}",
            target_dir.display(),
            err
        ))
    })?;

    attachments
        .iter()
        .enumerate()
        .map(|(index, attachment)| {
            if attachment.kind == crate::contracts::AttachmentKind::Image
                && attachment
                    .data_url
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| value.starts_with("data:image/"))
            {
                return Ok(crate::contracts::Attachment {
                    path: String::new(),
                    name: attachment.name.clone(),
                    explanation: attachment.explanation.clone(),
                    data_url: attachment.data_url.clone(),
                    kind: attachment.kind.clone(),
                });
            }
            let source_path = Path::new(&attachment.path);
            let file_name = sanitize_attachment_file_name(
                source_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(&attachment.name),
            );
            let destination_path = target_dir.join(format!("{:02}-{}", index + 1, file_name));
            fs::copy(source_path, &destination_path).map_err(|err| {
                AppError::internal(format!(
                    "Failed to stage attachment '{}' into {}: {}",
                    attachment.path,
                    destination_path.display(),
                    err
                ))
            })?;
            Ok(crate::contracts::Attachment {
                path: destination_path.to_string_lossy().to_string(),
                name: attachment.name.clone(),
                explanation: attachment.explanation.clone(),
                data_url: attachment.data_url.clone(),
                kind: attachment.kind.clone(),
            })
        })
        .collect()
}

fn stage_prompt_attachments(
    state: &AppState,
    request_id: &str,
    session_id: Option<&str>,
    attachments: &[crate::contracts::Attachment],
) -> AppResult<Vec<crate::contracts::Attachment>> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }
    let app = state.app_handle.lock().unwrap().clone();
    let Some(app) = app else {
        return Ok(attachments.to_vec());
    };
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|err| AppError::internal(format!("Failed to resolve app config dir: {}", err)))?;
    let staged_root = config_dir.join("mcp-attachments");
    if attachments_already_staged(&staged_root, attachments) {
        return Ok(attachments.to_vec());
    }
    stage_prompt_attachments_to_dir(&config_dir, request_id, session_id, attachments)
}

fn attachments_already_staged(
    staged_root: &Path,
    attachments: &[crate::contracts::Attachment],
) -> bool {
    attachments.iter().all(|attachment| {
        if attachment.kind == crate::contracts::AttachmentKind::Image
            && attachment
                .data_url
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| value.starts_with("data:image/"))
        {
            return true;
        }
        let path = Path::new(&attachment.path);
        path.is_absolute() && path.starts_with(staged_root)
    })
}

fn parse_attachment_kind(value: &str) -> crate::contracts::AttachmentKind {
    match value.trim().to_ascii_lowercase().as_str() {
        "image" => crate::contracts::AttachmentKind::Image,
        _ => crate::contracts::AttachmentKind::Cad,
    }
}

async fn get_message_attachments_impl(
    message_id: &str,
    state: &AppState,
) -> AppResult<Vec<crate::contracts::Attachment>> {
    let conn = state.db.lock().await;
    let references = crate::db::get_message_references(&conn, message_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    let mut attachments = references
        .into_iter()
        .filter(|reference| reference.kind == "attachment_meta")
        .filter_map(|reference| {
            serde_json::from_str::<crate::AttachmentReferenceMeta>(&reference.content)
                .ok()
                .map(|meta| crate::contracts::Attachment {
                    path: meta.path,
                    name: meta.name,
                    explanation: meta.explanation,
                    data_url: meta.data_url,
                    kind: parse_attachment_kind(&meta.kind),
                })
        })
        .collect::<Vec<_>>();

    if attachments.is_empty() {
        let thread_id = crate::db::get_message_thread_id(&conn, message_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .ok_or_else(|| AppError::not_found(format!("Message {} not found.", message_id)))?;
        let legacy_images = crate::db::get_message_attachment_images(&conn, &thread_id, message_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .ok_or_else(|| AppError::not_found(format!("Message {} not found.", message_id)))?;
        attachments = legacy_images
            .into_iter()
            .map(|image_ref| {
                let is_inline = image_ref.trim_start().starts_with("data:image/");
                let name = if is_inline {
                    "attachment.png".to_string()
                } else {
                    Path::new(&image_ref)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("attachment.png")
                        .to_string()
                };
                crate::contracts::Attachment {
                    path: if is_inline {
                        String::new()
                    } else {
                        image_ref.clone()
                    },
                    name,
                    explanation: String::new(),
                    data_url: is_inline.then_some(image_ref),
                    kind: crate::contracts::AttachmentKind::Image,
                }
            })
            .collect();
    }

    Ok(attachments)
}

#[derive(Debug)]
struct QueuedPromptBatch {
    message_ids: Vec<String>,
    prompt_text: String,
    attachments: Vec<crate::contracts::Attachment>,
}

async fn collect_queued_prompt_batch(
    thread_id: &str,
    state: &AppState,
) -> AppResult<Option<QueuedPromptBatch>> {
    let queued_messages = {
        let conn = state.db.lock().await;
        crate::db::get_thread_messages(&conn, thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .into_iter()
            .filter(|message| {
                message.role == crate::contracts::MessageRole::User
                    && message.status == crate::contracts::MessageStatus::Pending
            })
            .collect::<Vec<_>>()
    };
    if queued_messages.is_empty() {
        return Ok(None);
    }

    let mut attachment_keys = std::collections::HashSet::new();
    let mut attachments = Vec::new();
    for message in &queued_messages {
        for attachment in get_message_attachments_impl(&message.id, state).await? {
            let key = format!(
                "{}\u{0}{}\u{0}{}",
                attachment.path,
                attachment.data_url.as_deref().unwrap_or_default(),
                attachment.name
            );
            if attachment_keys.insert(key) {
                attachments.push(attachment);
            }
        }
    }

    Ok(Some(QueuedPromptBatch {
        message_ids: queued_messages
            .iter()
            .map(|message| message.id.clone())
            .collect(),
        prompt_text: queued_messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n"),
        attachments,
    }))
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_prompt_attachments(
    attachments: Vec<crate::contracts::Attachment>,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::contracts::Attachment>> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }
    let request_id = format!("queued-{}", uuid::Uuid::new_v4());
    stage_prompt_attachments(&state, &request_id, None, &attachments)
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_prompt_workspace_capture(
    input: crate::contracts::PreparePromptWorkspaceCaptureInput,
    state: State<'_, AppState>,
) -> AppResult<crate::contracts::Attachment> {
    let app = state.app_handle.lock().unwrap().clone();
    let Some(_app) = app else {
        return stage_prompt_workspace_capture_to_dir(&input);
    };
    stage_prompt_workspace_capture_to_dir(&input)
}

fn build_mcp_thread_title(
    prompt_text: &str,
    attachments: &[crate::contracts::Attachment],
) -> String {
    let content = agent_dialogue::build_user_reply_message_content(prompt_text, attachments);
    let trimmed = content.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() > 30 {
        format!("{}...", chars[..27].iter().collect::<String>())
    } else if !trimmed.is_empty() {
        trimmed.to_string()
    } else {
        "Queued Message".to_string()
    }
}

pub(crate) async fn queue_agent_prompt_impl(
    input: crate::contracts::QueueAgentPromptInput,
    state: &AppState,
) -> AppResult<crate::contracts::QueuedAgentPrompt> {
    if input.prompt_text.trim().is_empty() && input.attachments.is_empty() {
        return Err(AppError::validation(
            "Queued MCP messages need text or at least one attachment.",
        ));
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let thread_id = input
        .thread_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let message_id = uuid::Uuid::new_v4().to_string();
    let content =
        agent_dialogue::build_user_reply_message_content(&input.prompt_text, &input.attachments);
    let attachment_images = agent_dialogue::collect_attachment_image_paths(&input.attachments);
    let app = state.app_handle.lock().unwrap().clone();
    let configured_root = state.config.lock().unwrap().projects_root.clone();

    {
        let conn = state.db.lock().await;
        if let Some(title) = crate::db::get_thread_title(&conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
        {
            crate::db::create_or_update_thread(&conn, &thread_id, &title, now, None)
                .map_err(|err| AppError::persistence(err.to_string()))?;
        } else {
            let title = build_mcp_thread_title(&input.prompt_text, &input.attachments);
            let traits = crate::generate_genie_traits();
            crate::db::create_or_update_thread(&conn, &thread_id, &title, now, Some(&traits))
                .map_err(|err| AppError::persistence(err.to_string()))?;
            if let Some(app) = app.as_ref() {
                crate::thread_source_binding::bind_new_thread(
                    app,
                    &conn,
                    configured_root.as_deref(),
                    &thread_id,
                    &title,
                )?;
            }
        }

        crate::db::add_message_checked(
            &conn,
            &thread_id,
            &crate::contracts::Message {
                id: message_id.clone(),
                role: crate::contracts::MessageRole::User,
                content,
                status: crate::contracts::MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images,
                timestamp: now,
            },
        )
        .map_err(|err| AppError::persistence(err.to_string()))?;

        crate::persist_user_prompt_references(
            &conn,
            &thread_id,
            &message_id,
            &input.prompt_text,
            Some(&input.attachments),
            now,
        )
        .map_err(AppError::persistence)?;
    }

    state.emit_history_changed(
        Some(thread_id.clone()),
        Some(message_id.clone()),
        "messageQueued",
    );
    Ok(crate::contracts::QueuedAgentPrompt {
        thread_id,
        message_id,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn queue_agent_prompt(
    input: crate::contracts::QueueAgentPromptInput,
    state: State<'_, AppState>,
) -> AppResult<crate::contracts::QueuedAgentPrompt> {
    queue_agent_prompt_impl(input, &state).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_last_design(state: State<'_, AppState>) -> AppResult<Option<LastDesignSnapshot>> {
    Ok(state
        .last_snapshot
        .lock()
        .unwrap()
        .as_ref()
        .map(|snapshot| LastDesignSnapshot {
            design: None,
            thread_id: snapshot.thread_id.clone(),
            message_id: snapshot.message_id.clone(),
            artifact_bundle: None,
            model_manifest: None,
            selected_part_id: snapshot.selected_part_id.clone(),
            target_ref: snapshot.target_ref.clone(),
        }))
}

#[tauri::command]
#[specta::specta]
pub async fn get_web_content_recovery_state() -> AppResult<crate::contracts::WebContentRecoveryState>
{
    Ok(crate::web_content_recovery::state())
}

#[tauri::command]
#[specta::specta]
pub async fn acknowledge_web_content_recovery() -> AppResult<()> {
    crate::web_content_recovery::acknowledge_stable();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_agent_draft_preview(
    thread_id: String,
    preview_id: String,
    state: State<'_, AppState>,
) -> AppResult<crate::contracts::AgentDraftProjection> {
    let conn = state.db.lock().await;
    let mut projection = db::get_agent_draft_projection_by_preview_id(&conn, &preview_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| AppError::not_found("Draft preview not found."))?;
    if projection.thread_id != thread_id {
        return Err(AppError::validation(
            "Draft preview does not belong to the requested thread.",
        ));
    }
    crate::services::render_snapshot::validate_render_compatibility(
        &projection.design_output,
        &projection.artifact_bundle,
        &projection.model_manifest,
    )?;
    let initial_bytes = crate::transport_budget::serialized_size(&projection)?;
    if initial_bytes > crate::transport_budget::VERSION_CORE_MAX_BYTES {
        projection.design_output.response =
            crate::transport_budget::bounded_text(&projection.design_output.response, 64 * 1024);
        projection.design_output.macro_code =
            crate::transport_budget::bounded_text(&projection.design_output.macro_code, 512 * 1024);
        projection
            .truncated_fields
            .push("designOutput.source".to_string());
    }
    projection.observed_bytes = crate::transport_budget::require_serialized_budget(
        "agentDraftProjection",
        &projection,
        crate::transport_budget::VERSION_CORE_MAX_BYTES,
        "version source window or dense topology page",
    )?;
    Ok(projection)
}

#[tauri::command]
#[specta::specta]
pub async fn get_agent_draft_topology_page(
    thread_id: String,
    preview_id: String,
    kind: crate::contracts::DenseTopologyKind,
    cursor: Option<String>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> AppResult<crate::contracts::DenseTopologyPage> {
    use crate::contracts::{DenseTopologyItem, DenseTopologyKind, DenseTopologyPage};
    let conn = state.db.lock().await;
    let stored_thread_id = db::get_agent_draft_thread_id(&conn, &preview_id)
        .map_err(|error| AppError::persistence(error.to_string()))?
        .ok_or_else(|| AppError::not_found("Draft preview not found."))?;
    if stored_thread_id != thread_id {
        return Err(AppError::validation(
            "Draft preview does not belong to the requested thread.",
        ));
    }
    let kind_name = match kind {
        DenseTopologyKind::Edge => "edge",
        DenseTopologyKind::Face => "face",
        DenseTopologyKind::Selection => "selection",
    };
    let identity = format!("{thread_id}\0{preview_id}\0{kind_name}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    let offset = if let Some(cursor) = cursor {
        let mut parts = cursor.split(':');
        let valid = parts.next() == Some("v1")
            && parts
                .next()
                .is_some_and(|part| part == format!("{identity:016x}"));
        let offset = parts.next().and_then(|part| part.parse::<usize>().ok());
        if !valid || parts.next().is_some() || offset.is_none() {
            return Err(AppError::validation("Invalid draft topology cursor."));
        }
        offset.unwrap_or(0)
    } else {
        0
    };
    let safe_limit = limit.unwrap_or(500).clamp(1, 500);
    let (json_column, json_path) = match kind {
        DenseTopologyKind::Edge => ("artifact_bundle", "$.edgeTargets"),
        DenseTopologyKind::Face => ("artifact_bundle", "$.faceTargets"),
        DenseTopologyKind::Selection => ("model_manifest", "$.selectionTargets"),
    };
    let (raw_items, total_count) = db::get_agent_draft_topology_json_page(
        &conn,
        &preview_id,
        json_column,
        json_path,
        offset,
        safe_limit,
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    let mut observed_bytes = 2usize;
    let mut items = Vec::new();
    for raw_item in raw_items {
        let item = match kind {
            DenseTopologyKind::Edge => DenseTopologyItem::Edge(
                serde_json::from_str(&raw_item)
                    .map_err(|error| AppError::persistence(error.to_string()))?,
            ),
            DenseTopologyKind::Face => DenseTopologyItem::Face(
                serde_json::from_str(&raw_item)
                    .map_err(|error| AppError::persistence(error.to_string()))?,
            ),
            DenseTopologyKind::Selection => DenseTopologyItem::Selection(
                serde_json::from_str(&raw_item)
                    .map_err(|error| AppError::persistence(error.to_string()))?,
            ),
        };
        let bytes = crate::transport_budget::serialized_size(&item)?;
        if observed_bytes.saturating_add(bytes) > crate::transport_budget::TOPOLOGY_PAGE_MAX_BYTES {
            if items.is_empty() {
                return Err(AppError::validation(format!(
                    "Draft topology item is {bytes} bytes; allowed page is {} bytes.",
                    crate::transport_budget::TOPOLOGY_PAGE_MAX_BYTES
                )));
            }
            break;
        }
        observed_bytes = observed_bytes.saturating_add(bytes);
        items.push(item);
    }
    let next_offset = offset + items.len();
    Ok(DenseTopologyPage {
        snapshot_ref: format!("draft-topology:{thread_id}:{preview_id}"),
        kind,
        items,
        next_cursor: (next_offset < total_count)
            .then(|| format!("v1:{identity:016x}:{next_offset}")),
        total_count,
        observed_bytes,
    })
}

fn snapshot_thread_changed(
    previous: Option<&LastDesignSnapshot>,
    next: Option<&LastDesignSnapshot>,
) -> bool {
    previous.and_then(|snapshot| snapshot.thread_id.as_deref())
        != next.and_then(|snapshot| snapshot.thread_id.as_deref())
}

#[tauri::command]
#[specta::specta]
pub async fn save_last_design(
    snapshot: Option<LastDesignSnapshot>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    let thread_changed = {
        let mut last = state.last_snapshot.lock().unwrap();
        let changed = snapshot_thread_changed(last.as_ref(), snapshot.as_ref());
        *last = snapshot.clone();
        changed
    };
    if thread_changed {
        crate::mcp::handlers::cancel_active_project_folder_render();
    }
    crate::services::session::write_last_snapshot(&app, snapshot.as_ref());
    Ok(())
}

async fn resolve_agent_prompt_impl(
    input: crate::contracts::ResolveAgentPromptInput,
    state: &AppState,
) -> AppResult<()> {
    resolve_agent_prompt_with_classifier(input, state, None).await
}

async fn classify_managed_prompt(
    input: &crate::contracts::ResolveAgentPromptInput,
    state: &AppState,
    classifier: Option<&dyn crate::jev_classifier::TurnClassifier>,
) -> AppResult<
    Option<(
        String,
        String,
        crate::provider_turn::ProviderTurnPolicy,
        crate::llm_eval::EvalRun,
    )>,
> {
    let config = state.config.lock().unwrap().jev_classifier.clone();
    if !config.enabled {
        return Ok(None);
    }
    config.validate()?;
    let (thread_id, session_id) = state
        .prompt_waits
        .lock()
        .unwrap()
        .get(&input.request_id)
        .and_then(|control| {
            control
                .thread_id
                .clone()
                .map(|thread_id| (thread_id, control.session_id.clone()))
        })
        .ok_or_else(|| AppError::validation("Managed Jev request has no bound Ecky thread."))?;
    let (summary, recent, starting_identity) = {
        let conn = state.db.lock().await;
        let summary = crate::db::get_thread_summary(&conn, &thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .unwrap_or_default();
        let identity = crate::llm_eval::latest_version_identity(&conn, &thread_id)
            .map_err(AppError::persistence)?;
        let recent = crate::db::get_thread_messages(&conn, &thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .into_iter()
            .rev()
            .take(crate::jev_classifier::MAX_RECENT_MESSAGES)
            .map(|message| crate::jev_classifier::RecentMessage {
                role: message.role.as_str().to_owned(),
                content: message.content,
            })
            .collect::<Vec<_>>();
        (summary, recent, identity)
    };
    let attachments = input
        .attachments
        .iter()
        .map(|attachment| crate::jev_classifier::AttachmentModality {
            kind: attachment.kind.as_str().to_owned(),
            explanation: attachment.explanation.clone(),
        })
        .collect();
    let request = crate::jev_classifier::ClassifierRequest::bounded(
        &input.prompt_text,
        recent,
        &summary,
        "Ecky-owned managed MCP user request",
        attachments,
        Vec::new(),
    );
    let route = match classifier {
        Some(classifier) => classifier.classify(request.clone()).await?,
        None => {
            crate::jev_classifier::classify_configured_request(&config, request.clone()).await?
        }
    };
    if route.model.is_some() {
        return Err(AppError::provider(
            "Managed MCP provider does not support a verified model override.",
        ));
    }
    let current_config = state.config.lock().unwrap().jev_classifier.clone();
    if current_config.enabled != config.enabled || current_config.api_key != config.api_key {
        return Err(AppError::conflict(
            "Jev configuration changed before managed MCP delivery; retry this request.",
        ));
    }
    let current_identity = {
        let conn = state.db.lock().await;
        crate::llm_eval::latest_version_identity(&conn, &thread_id)
            .map_err(AppError::persistence)?
    };
    if current_identity != starting_identity {
        return Err(AppError::conflict(
            "Managed MCP artifact changed before Jev route delivery.",
        ));
    }
    let current_thread_id = state
        .prompt_waits
        .lock()
        .unwrap()
        .get(&input.request_id)
        .and_then(|control| control.thread_id.clone());
    if current_thread_id.as_deref() != Some(&thread_id)
        || !state
            .prompt_channels
            .lock()
            .await
            .contains_key(&input.request_id)
    {
        return Err(AppError::conflict(
            "Managed MCP request changed before Jev route delivery.",
        ));
    }
    let policy = crate::provider_turn::ProviderTurnPolicy::routed(route.intent, route.answer_first);
    let trace = managed_jev_trace::accepted_run(
        &thread_id,
        &session_id,
        &input.prompt_text,
        &request,
        &route,
        starting_identity,
    );
    Ok(Some((thread_id, session_id, policy, trace)))
}

async fn resolve_agent_prompt_with_classifier(
    input: crate::contracts::ResolveAgentPromptInput,
    state: &AppState,
    classifier: Option<&dyn crate::jev_classifier::TurnClassifier>,
) -> AppResult<()> {
    let managed_route = match classify_managed_prompt(&input, state, classifier).await {
        Ok(route) => route,
        Err(error) => {
            let control = state
                .prompt_waits
                .lock()
                .unwrap()
                .get(&input.request_id)
                .cloned();
            if state.config.lock().unwrap().jev_classifier.enabled {
                if let Some(control) = control {
                    if let Some(thread_id) = control.thread_id {
                        let run = managed_jev_trace::failed_run(
                            &thread_id,
                            &control.session_id,
                            &input.prompt_text,
                            &error,
                        );
                        managed_jev_trace::persist_if_app_handle(state, &run)?;
                    }
                }
            }
            return Err(error);
        }
    };
    if let Some((_thread_id, session_id, _policy, trace)) = managed_route.as_ref() {
        let previous_pending = {
            let runs = state.managed_jev_runs.lock().await;
            runs.contains_key(session_id)
        };
        if previous_pending {
            managed_jev_trace::finish_run(
                state,
                session_id,
                "Managed agent opened another request before saving a final reply.",
                true,
            )
            .await?;
        }
        managed_jev_trace::persist_if_app_handle(state, trace)?;
    }
    if let Some((thread_id, _session_id, policy, trace)) = managed_route.as_ref() {
        let message_id = input.message_ids.first().or(input.message_id.as_ref());
        let conn = state.db.lock().await;
        crate::services::jev_classifications::save_accepted(
            &conn,
            thread_id,
            "managedMcp",
            &input.request_id,
            message_id.map(String::as_str),
            policy.intent(),
            &trace
                .route
                .jev
                .as_ref()
                .expect("accepted Jev route")
                .intent_probabilities,
            chrono::Utc::now().timestamp(),
        )?;
        if let Some(app) = state.app_handle.lock().unwrap().clone() {
            use tauri::Emitter;
            let _ = app.emit(
                "jev-classification-accepted",
                serde_json::json!({"threadId": thread_id}),
            );
        }
    }
    let request_id = input.request_id.clone();

    // Wake a frozen active-mode agent before unblocking its HTTP request.
    let prompt_control = runtime::release_prompt_wait(state, &request_id);
    #[cfg(unix)]
    if let Some(pgid) = prompt_control.as_ref().and_then(|control| control.pgid) {
        eprintln!("[MCP] SIGCONT pgid {} (prompt: {})", pgid, request_id);
    }
    if let Some(control) = prompt_control.as_ref() {
        if runtime::runtime_snapshot_by_session_id(state, &control.session_id).is_some() {
            runtime::mark_managed_session_turn_busy(
                state,
                &control.session_id,
                control.thread_id.clone(),
                None,
                Some("Working through the queued message.".to_string()),
            );
        }
        let mut sessions = state.mcp_session_registry.with_sessions().lock().await;
        if let Some(session) = sessions.get_mut(&control.session_id) {
            session.current_turn_id = Some(uuid::Uuid::new_v4().to_string());
            session.current_turn_thread_id = control.thread_id.clone();
            session.phase = Some("working".to_string());
            session.status_text = Some("Working through the queued message.".to_string());
            session.busy = true;
            session.waiting_on_prompt = false;
            session.attention_kind = None;
            session.activity_label = None;
            if session.activity_started_at.is_none() {
                session.activity_started_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                );
            }
            session.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }
    }
    let staged_attachments = match stage_prompt_attachments(
        state,
        &request_id,
        prompt_control
            .as_ref()
            .map(|control| control.session_id.as_str()),
        &input.attachments,
    ) {
        Ok(attachments) => attachments,
        Err(error) => {
            if let Some((_thread_id, _session_id, _policy, trace)) = managed_route.as_ref() {
                let failed = managed_jev_trace::failed_delivery(trace.clone(), &error);
                managed_jev_trace::persist_if_app_handle(state, &failed)?;
            }
            return Err(error);
        }
    };
    let mut delivered_input = input.clone();
    delivered_input.attachments = staged_attachments.clone();
    if let Some((thread_id, _session_id, policy, _trace)) = managed_route.as_ref() {
        state.set_provider_turn_policy(thread_id, *policy).await;
        delivered_input.prompt_text = policy
            .wrap_user_message_for_turn(&input.prompt_text, &uuid::Uuid::new_v4().to_string());
    } else if let Some(thread_id) = prompt_control
        .as_ref()
        .and_then(|control| control.thread_id.as_deref())
    {
        state
            .set_provider_turn_policy(
                thread_id,
                crate::provider_turn::ProviderTurnPolicy::prompt_based(),
            )
            .await;
    }

    let mut channels = state.prompt_channels.lock().await;
    if let Some(tx) = channels.remove(&request_id) {
        let managed_session_id = managed_route.as_ref().map(|route| route.1.clone());
        if let Some((_thread_id, session_id, _policy, trace)) = managed_route {
            state
                .managed_jev_runs
                .lock()
                .await
                .insert(session_id, trace);
        }
        let delivered = tx.send(Ok(delivered_input.clone())).is_ok();
        drop(channels);
        if let Some(session_id) = managed_session_id {
            if delivered {
                managed_jev_trace::mark_delivered(state, &session_id).await?;
            } else {
                let error = AppError::conflict("Managed prompt receiver closed before delivery.");
                managed_jev_trace::finish_run(state, &session_id, &error.message, true).await?;
                return Err(error);
            }
        }
    } else {
        let error =
            AppError::not_found(format!("No pending prompt request with id: {}", request_id));
        if let Some((_thread_id, _session_id, _policy, trace)) = managed_route.as_ref() {
            let failed = managed_jev_trace::failed_delivery(trace.clone(), &error);
            managed_jev_trace::persist_if_app_handle(state, &failed)?;
        }
        return Err(error);
    }

    let reply_content = agent_dialogue::build_user_reply_message_content(
        &input.prompt_text,
        &delivered_input.attachments,
    );
    let mut working_message_ids = if !delivered_input.message_ids.is_empty() {
        delivered_input.message_ids.clone()
    } else {
        delivered_input
            .message_id
            .clone()
            .into_iter()
            .collect::<Vec<_>>()
    };
    if !working_message_ids.is_empty() {
        {
            let conn = state.db.lock().await;
            for message_id in &working_message_ids {
                crate::db::update_message_status_and_output(
                    &conn,
                    message_id,
                    crate::db::MessageStatusUpdate {
                        status: &crate::contracts::MessageStatus::Working,
                        output: None,
                        usage: None,
                        artifact_bundle: None,
                        model_manifest: None,
                        structural_verification: None,
                        visual_kind: None,
                        content: None,
                    },
                )
                .map_err(|err| AppError::persistence(err.to_string()))?;
            }
        }
        state.emit_history_changed(
            prompt_control
                .as_ref()
                .and_then(|control| control.thread_id.clone()),
            working_message_ids.last().cloned(),
            "messageUpdated",
        );
    } else if let Some(thread_id) = prompt_control
        .as_ref()
        .and_then(|control| control.thread_id.clone())
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let message_id = uuid::Uuid::new_v4().to_string();
        let attachment_images =
            agent_dialogue::collect_attachment_image_paths(&delivered_input.attachments);
        agent_dialogue::add_dialogue_message(
            state,
            &thread_id,
            &crate::contracts::Message {
                id: message_id.clone(),
                role: crate::contracts::MessageRole::User,
                content: reply_content,
                status: crate::contracts::MessageStatus::Working,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images,
                timestamp,
            },
        )
        .await?;
        working_message_ids = vec![message_id.clone()];
        state.emit_history_changed(
            Some(thread_id.clone()),
            Some(message_id.clone()),
            "messageUpdated",
        );
    }

    if let Some(control) = prompt_control.as_ref() {
        if !working_message_ids.is_empty() {
            let mut sessions = state.mcp_session_registry.with_sessions().lock().await;
            if let Some(session) = sessions.get_mut(&control.session_id) {
                session.current_turn_working_message_ids = working_message_ids;
            }
        }
    }
    Ok(())
}

pub(crate) async fn auto_deliver_queued_prompt_batch(
    request_id: &str,
    thread_id: &str,
    state: &AppState,
) -> AppResult<bool> {
    auto_deliver_queued_prompt_batch_with_classifier(request_id, thread_id, state, None).await
}

async fn auto_deliver_queued_prompt_batch_with_classifier(
    request_id: &str,
    thread_id: &str,
    state: &AppState,
    classifier: Option<&dyn crate::jev_classifier::TurnClassifier>,
) -> AppResult<bool> {
    let Some(batch) = collect_queued_prompt_batch(thread_id, state).await? else {
        return Ok(false);
    };
    let delivered = resolve_agent_prompt_with_classifier(
        crate::contracts::ResolveAgentPromptInput {
            request_id: request_id.to_string(),
            prompt_text: batch.prompt_text,
            message_ids: batch.message_ids.clone(),
            message_id: batch.message_ids.first().cloned(),
            attachments: batch.attachments,
        },
        state,
        classifier,
    )
    .await;
    match delivered {
        Ok(()) => Ok(true),
        Err(error) => {
            let channel_still_pending = state.prompt_channels.lock().await.contains_key(request_id);
            if channel_still_pending {
                let route_failed_before_delivery =
                    state.config.lock().unwrap().jev_classifier.enabled
                        && state.prompt_waits.lock().unwrap().contains_key(request_id);
                if route_failed_before_delivery {
                    {
                        let conn = state.db.lock().await;
                        for message_id in &batch.message_ids {
                            crate::db::update_message_status_and_output(
                                &conn,
                                message_id,
                                crate::db::MessageStatusUpdate {
                                    status: &crate::contracts::MessageStatus::Error,
                                    output: None,
                                    usage: None,
                                    artifact_bundle: None,
                                    model_manifest: None,
                                    structural_verification: None,
                                    visual_kind: None,
                                    content: None,
                                },
                            )
                            .map_err(|failure| AppError::persistence(failure.to_string()))?;
                        }
                    }
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    agent_dialogue::add_dialogue_message(
                        state,
                        thread_id,
                        &crate::contracts::Message {
                            id: uuid::Uuid::new_v4().to_string(),
                            role: crate::contracts::MessageRole::Assistant,
                            content: error.message.clone(),
                            status: crate::contracts::MessageStatus::Error,
                            output: None,
                            usage: None,
                            artifact_bundle: None,
                            model_manifest: None,
                            structural_verification: None,
                            agent_origin: None,
                            image_data: None,
                            visual_kind: None,
                            attachment_images: Vec::new(),
                            timestamp,
                        },
                    )
                    .await?;
                    state.emit_history_changed(
                        Some(thread_id.to_string()),
                        batch.message_ids.last().cloned(),
                        "messageUpdated",
                    );
                }
                Err(error)
            } else {
                eprintln!(
                    "[MCP] queued prompt batch reached agent but post-delivery projection failed: {}",
                    error.message
                );
                Ok(true)
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn submit_agent_prompt_reply(
    input: crate::contracts::SubmitAgentPromptReplyInput,
    state: State<'_, AppState>,
) -> AppResult<crate::contracts::SubmitAgentPromptReplyResult> {
    submit_agent_prompt_reply_impl(input, &state).await
}

pub(crate) async fn submit_agent_prompt_reply_impl(
    input: crate::contracts::SubmitAgentPromptReplyInput,
    state: &AppState,
) -> AppResult<crate::contracts::SubmitAgentPromptReplyResult> {
    let resolved = resolve_agent_prompt_impl(
        crate::contracts::ResolveAgentPromptInput {
            request_id: input.request_id,
            prompt_text: input.prompt_text.clone(),
            message_ids: Vec::new(),
            message_id: None,
            attachments: input.attachments.clone(),
        },
        state,
    )
    .await;
    match resolved {
        Ok(()) => Ok(crate::contracts::SubmitAgentPromptReplyResult::Resolved),
        Err(error) if error.code == crate::contracts::AppErrorCode::NotFound => {
            let queued = queue_agent_prompt_impl(
                crate::contracts::QueueAgentPromptInput {
                    thread_id: Some(input.thread_id),
                    prompt_text: input.prompt_text,
                    attachments: input.attachments,
                },
                state,
            )
            .await?;
            Ok(crate::contracts::SubmitAgentPromptReplyResult::Queued {
                thread_id: queued.thread_id,
                message_id: queued.message_id,
            })
        }
        Err(error) => Err(error),
    }
}

/// Called by the frontend when the user clicks a confirmation button.
/// Resolves the pending oneshot channel so the MCP handler can return.
#[tauri::command]
#[specta::specta]
pub async fn resolve_agent_confirm(
    request_id: String,
    choice: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let mut channels = state.confirm_channels.lock().await;
    if let Some(tx) = channels.remove(&request_id) {
        let _ = tx.send(choice);
    } else {
        return Err(AppError::not_found(format!(
            "No pending confirmation with id: {}",
            request_id
        )));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn resolve_agent_viewport_screenshot(
    input: crate::contracts::ResolveViewportScreenshotInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let mut channels = state.viewport_screenshot_channels.lock().await;
    if let Some(tx) = channels.remove(&input.request_id) {
        let _ = tx.send(Ok(ViewportScreenshotCapture {
            data_url: input.data_url,
            width: input.width,
            height: input.height,
            camera: input.camera,
            source: input.source,
            thread_id: input.thread_id,
            message_id: input.message_id,
            model_id: input.model_id,
            include_overlays: input.include_overlays,
        }));
    } else {
        return Err(AppError::not_found(format!(
            "No pending viewport screenshot with id: {}",
            input.request_id
        )));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn reject_agent_viewport_screenshot(
    input: crate::contracts::RejectViewportScreenshotInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let mut channels = state.viewport_screenshot_channels.lock().await;
    if let Some(tx) = channels.remove(&input.request_id) {
        let _ = tx.send(Err(input.error));
    } else {
        return Err(AppError::not_found(format!(
            "No pending viewport screenshot with id: {}",
            input.request_id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Config, McpConfig};
    use portable_pty::native_pty_system;
    use std::path::PathBuf;

    fn test_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{}-{}", name, uuid::Uuid::new_v4()))
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
            voice: crate::contracts::VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: crate::contracts::EngineKind::Freecad,
            default_source_language: crate::contracts::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::contracts::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
    }

    #[tokio::test]
    async fn submit_agent_prompt_reply_resolves_live_request_without_queueing() {
        let conn = crate::db::init_db(&test_db_path("submit-agent-prompt-resolve")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let (tx, rx) = tokio::sync::oneshot::channel();
        state
            .prompt_channels
            .lock()
            .await
            .insert("request-live".to_string(), tx);

        let result = submit_agent_prompt_reply_impl(
            crate::contracts::SubmitAgentPromptReplyInput {
                request_id: "request-live".to_string(),
                thread_id: "thread-live".to_string(),
                prompt_text: "Continue.".to_string(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("reply");

        assert_eq!(
            result,
            crate::contracts::SubmitAgentPromptReplyResult::Resolved
        );
        assert_eq!(
            rx.await.expect("channel").expect("reply").prompt_text,
            "Continue."
        );
        let conn = state.db.lock().await;
        assert!(crate::services::history::get_thread(&conn, "thread-live").is_err());
    }

    #[tokio::test]
    async fn submit_agent_prompt_reply_queues_when_request_expired() {
        let conn = crate::db::init_db(&test_db_path("submit-agent-prompt-queue")).expect("db");
        let state = AppState::new(test_config(), None, conn);

        let result = submit_agent_prompt_reply_impl(
            crate::contracts::SubmitAgentPromptReplyInput {
                request_id: "request-expired".to_string(),
                thread_id: "thread-queued".to_string(),
                prompt_text: "Keep this message.".to_string(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queued fallback");

        let crate::contracts::SubmitAgentPromptReplyResult::Queued {
            thread_id,
            message_id,
        } = result
        else {
            panic!("expected queued result");
        };
        assert_eq!(thread_id, "thread-queued");
        let conn = state.db.lock().await;
        let thread = crate::services::history::get_thread(&conn, &thread_id).expect("thread");
        assert!(thread
            .messages
            .iter()
            .any(|message| message.id == message_id));
    }

    #[tokio::test]
    async fn queued_batch_auto_delivery_owns_order_attachments_and_working_status() {
        let conn = crate::db::init_db(&test_db_path("queued-batch-auto-delivery")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let attachment = crate::contracts::Attachment {
            path: String::new(),
            name: "reference.png".to_string(),
            explanation: "Shared reference".to_string(),
            data_url: Some("data:image/png;base64,AA==".to_string()),
            kind: crate::contracts::AttachmentKind::Image,
        };
        let first = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-batch".to_string()),
                prompt_text: "First instruction.".to_string(),
                attachments: vec![attachment.clone()],
            },
            &state,
        )
        .await
        .expect("first queue");
        let second = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-batch".to_string()),
                prompt_text: "Second instruction.".to_string(),
                attachments: vec![attachment],
            },
            &state,
        )
        .await
        .expect("second queue");
        let (tx, rx) = tokio::sync::oneshot::channel();
        state
            .prompt_channels
            .lock()
            .await
            .insert("request-batch".to_string(), tx);

        assert!(
            auto_deliver_queued_prompt_batch("request-batch", "thread-batch", &state)
                .await
                .expect("auto delivery")
        );
        let delivered = rx.await.expect("channel").expect("reply");
        assert_eq!(
            delivered.message_ids,
            vec![first.message_id, second.message_id]
        );
        assert!(delivered.prompt_text.contains("First instruction."));
        assert!(delivered.prompt_text.contains("Second instruction."));
        assert_eq!(delivered.attachments.len(), 1);

        let conn = state.db.lock().await;
        let messages = crate::db::get_thread_messages(&conn, "thread-batch").expect("messages");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.role == crate::contracts::MessageRole::User)
                .map(|message| message.status.clone())
                .collect::<Vec<_>>(),
            vec![
                crate::contracts::MessageStatus::Working,
                crate::contracts::MessageStatus::Working,
            ]
        );
    }

    #[tokio::test]
    async fn managed_prompt_delivery_uses_global_jev_contract_before_agent_receives_text() {
        use crate::provider_turn::{ProviderTurnIntent, ProviderTurnPolicy};

        let conn = crate::db::init_db(&test_db_path("managed-jev-delivery")).expect("db");
        let mut config = test_config();
        config.jev_classifier.enabled = true;
        config.jev_classifier.api_key = "fixture-token".into();
        let state = AppState::new(config, None, conn);
        let queued = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-managed-jev".into()),
                prompt_text: "Explain current geometry.".into(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue");
        let (tx, rx) = tokio::sync::oneshot::channel();
        state
            .prompt_channels
            .lock()
            .await
            .insert("request-managed-jev".into(), tx);
        state.prompt_waits.lock().unwrap().insert(
            "request-managed-jev".into(),
            crate::models::PromptResumeState {
                pgid: None,
                agent_label: "Managed agent".into(),
                session_id: "session-managed-jev".into(),
                thread_id: Some(queued.thread_id.clone()),
            },
        );
        let classifier = crate::jev_classifier::MockJevClassifier::fixed(
            crate::jev_classifier::AcceptedRoute::test_route(
                ProviderTurnIntent::Answer,
                None,
                false,
            ),
        );

        assert!(auto_deliver_queued_prompt_batch_with_classifier(
            "request-managed-jev",
            &queued.thread_id,
            &state,
            Some(&classifier),
        )
        .await
        .expect("delivery"));
        let delivered = rx.await.expect("channel").expect("prompt");
        assert!(delivered.prompt_text.contains("Intent: ANSWER"));
        assert!(delivered.prompt_text.contains("Explain current geometry."));
        assert_eq!(
            state.provider_turn_policy(&queued.thread_id).await,
            Some(ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer))
        );
        let conn = state.db.lock().await;
        let messages = crate::db::get_thread_messages(&conn, &queued.thread_id).expect("history");
        assert_eq!(messages[0].content, "Explain current geometry.");
        let classifications =
            crate::services::jev_classifications::list_for_thread(&conn, &queued.thread_id)
                .unwrap();
        assert_eq!(classifications.len(), 1);
        assert_eq!(classifications[0].intent, "answer");
        assert_eq!(
            classifications[0].message_id.as_deref(),
            Some(queued.message_id.as_str())
        );
        let traces = state.managed_jev_runs.lock().await;
        let run = traces
            .get("session-managed-jev")
            .expect("one in-flight managed trace");
        assert_eq!(run.prompt, "Explain current geometry.");
        assert_eq!(run.route.provider, "managed-mcp");
        assert!(run.route.jev.is_some());
        assert_eq!(run.events[0].name.as_deref(), Some("request"));
        assert_eq!(run.events[1].name.as_deref(), Some("jev"));
        assert_eq!(run.events[2].name.as_deref(), Some("delivery"));
        assert_eq!(run.events[2].state, "done");
        let pending = run.clone();
        drop(traces);
        let root = std::env::temp_dir().join(format!("managed-jev-eval-{}", uuid::Uuid::new_v4()));
        managed_jev_trace::persist_to(&root, &pending).expect("persist running trace");
        let restored_pending =
            crate::llm_eval::read_run(&root.join("evals/runs").join(&pending.run_id))
                .expect("read running trace");
        assert_eq!(restored_pending.status, "running");
        assert_eq!(restored_pending.events[2].state, "done");
        let mut before_delivery = pending.clone();
        before_delivery.events[2].state = "pending".into();
        let stage_error = AppError::internal("Attachment staging failed before delivery.");
        let failed_delivery = managed_jev_trace::failed_delivery(before_delivery, &stage_error);
        assert_eq!(failed_delivery.status, "error");
        assert_eq!(failed_delivery.events[2].state, "error");
        managed_jev_trace::persist_to(&root, &failed_delivery)
            .expect("persist strict pre-delivery trace");
        let restored_failure =
            crate::llm_eval::read_run(&root.join("evals/runs").join(&pending.run_id))
                .expect("read pre-delivery trace");
        assert_eq!(
            restored_failure.raw_error.as_deref(),
            Some(stage_error.message.as_str())
        );
        let trace_run_id = managed_jev_trace::tool_started(
            &state,
            "session-managed-jev",
            "session_reply_save",
            Some(serde_json::json!({"body": "Current geometry has two ribs."})),
        )
        .await
        .expect("managed trace run id");
        managed_jev_trace::tool_finished(
            &state,
            "session-managed-jev",
            &trace_run_id,
            "session_reply_save",
            &Ok(serde_json::json!({"saved": true})),
        )
        .await;
        let completed = managed_jev_trace::finish_run(
            &state,
            "session-managed-jev",
            "Current geometry has two ribs.",
            false,
        )
        .await
        .expect("finish trace")
        .expect("managed trace");
        assert_eq!(completed.status, "success");
        assert!(completed.turn_id.is_empty());
        assert_eq!(completed.events.len(), 6);
        assert!(completed.policy_violations.is_empty());
        managed_jev_trace::persist_to(&root, &completed).expect("persist strict EDN trace");
        let restored = crate::llm_eval::read_run(&root.join("evals/runs").join(&completed.run_id))
            .expect("read strict EDN trace");
        assert_eq!(restored.events.len(), 6);
        assert_eq!(
            restored.response.as_deref(),
            Some("Current geometry has two ribs.")
        );
        std::fs::remove_dir_all(root).expect("remove test eval");
    }

    #[tokio::test]
    async fn managed_trace_does_not_attach_previous_prompt_result_to_next_request() {
        let conn = crate::db::init_db(&test_db_path("managed-jev-trace-race")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let request = crate::jev_classifier::ClassifierRequest::bounded(
            "First request",
            Vec::new(),
            "",
            "managed request",
            Vec::new(),
            Vec::new(),
        );
        let route = crate::jev_classifier::AcceptedRoute::test_route(
            crate::provider_turn::ProviderTurnIntent::Answer,
            None,
            false,
        );
        let first = managed_jev_trace::accepted_run(
            "thread-1",
            "session-1",
            "First request",
            &request,
            &route,
            None,
        );
        state
            .managed_jev_runs
            .lock()
            .await
            .insert("session-1".into(), first);
        let first_run_id =
            managed_jev_trace::tool_started(&state, "session-1", "request_user_prompt", None)
                .await
                .expect("first run id");
        managed_jev_trace::finish_run(
            &state,
            "session-1",
            "Agent requested another prompt before replying.",
            true,
        )
        .await
        .expect("close first");
        let second = managed_jev_trace::accepted_run(
            "thread-1",
            "session-1",
            "Second request",
            &request,
            &route,
            None,
        );
        state
            .managed_jev_runs
            .lock()
            .await
            .insert("session-1".into(), second);
        managed_jev_trace::tool_finished(
            &state,
            "session-1",
            &first_run_id,
            "request_user_prompt",
            &Ok(serde_json::json!({"promptText": "Second request"})),
        )
        .await;
        let runs = state.managed_jev_runs.lock().await;
        let current = runs.get("session-1").expect("second run");
        assert_eq!(current.prompt, "Second request");
        assert_eq!(current.events.len(), 3);
    }

    #[tokio::test]
    async fn managed_prompt_without_jev_token_keeps_agent_waiting_and_message_pending() {
        let conn = crate::db::init_db(&test_db_path("managed-jev-missing-token")).expect("db");
        let mut config = test_config();
        config.jev_classifier.enabled = true;
        let state = AppState::new(config, None, conn);
        let queued = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-managed-blocked".into()),
                prompt_text: "Change the bracket.".into(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue");
        let (tx, _rx) = tokio::sync::oneshot::channel();
        state
            .prompt_channels
            .lock()
            .await
            .insert("request-managed-blocked".into(), tx);
        state.prompt_waits.lock().unwrap().insert(
            "request-managed-blocked".into(),
            crate::models::PromptResumeState {
                pgid: None,
                agent_label: "Managed agent".into(),
                session_id: "session-managed-blocked".into(),
                thread_id: Some(queued.thread_id.clone()),
            },
        );

        let error =
            auto_deliver_queued_prompt_batch("request-managed-blocked", &queued.thread_id, &state)
                .await
                .expect_err("missing Jev token");
        assert!(error.message.contains("Jev API token"));
        assert!(state
            .prompt_channels
            .lock()
            .await
            .contains_key("request-managed-blocked"));
        assert!(state
            .prompt_waits
            .lock()
            .unwrap()
            .contains_key("request-managed-blocked"));
        assert!(state
            .provider_turn_policy(&queued.thread_id)
            .await
            .is_none());
        let conn = state.db.lock().await;
        let messages = crate::db::get_thread_messages(&conn, &queued.thread_id).expect("history");
        assert_eq!(messages[0].status, crate::contracts::MessageStatus::Error);
        assert!(messages.iter().any(|message| {
            message.role == crate::contracts::MessageRole::Assistant
                && message.status == crate::contracts::MessageStatus::Error
                && message.content.contains("Jev API token")
        }));
    }

    #[tokio::test]
    async fn managed_prompt_restores_prompt_based_policy_when_jev_disabled() {
        use crate::provider_turn::{ProviderTurnIntent, ProviderTurnPolicy};

        let conn = crate::db::init_db(&test_db_path("managed-jev-disabled")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        state
            .set_provider_turn_policy(
                "thread-managed-disabled",
                ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer),
            )
            .await;
        let queued = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-managed-disabled".into()),
                prompt_text: "Build a bracket.".into(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue");
        let (tx, rx) = tokio::sync::oneshot::channel();
        state
            .prompt_channels
            .lock()
            .await
            .insert("request-managed-disabled".into(), tx);
        state.prompt_waits.lock().unwrap().insert(
            "request-managed-disabled".into(),
            crate::models::PromptResumeState {
                pgid: None,
                agent_label: "Managed agent".into(),
                session_id: "session-managed-disabled".into(),
                thread_id: Some(queued.thread_id.clone()),
            },
        );

        assert!(auto_deliver_queued_prompt_batch(
            "request-managed-disabled",
            &queued.thread_id,
            &state,
        )
        .await
        .expect("delivery"));
        assert_eq!(
            rx.await.expect("channel").expect("prompt").prompt_text,
            "Build a bracket."
        );
        assert_eq!(
            state.provider_turn_policy(&queued.thread_id).await,
            Some(ProviderTurnPolicy::prompt_based())
        );
        assert!(state.managed_jev_runs.lock().await.is_empty());
    }

    #[tokio::test]
    async fn queued_batch_auto_delivery_keeps_live_channel_when_no_batch_exists() {
        let conn = crate::db::init_db(&test_db_path("queued-batch-none")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let (tx, _rx) = tokio::sync::oneshot::channel();
        state
            .prompt_channels
            .lock()
            .await
            .insert("request-empty".to_string(), tx);

        assert!(
            !auto_deliver_queued_prompt_batch("request-empty", "thread-empty", &state)
                .await
                .expect("no batch")
        );
        assert!(state
            .prompt_channels
            .lock()
            .await
            .contains_key("request-empty"));
    }

    #[tokio::test]
    async fn queued_batch_attachment_load_failure_preserves_prompt_channel_and_wait() {
        let conn =
            crate::db::init_db(&test_db_path("queued-batch-attachment-failure")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-failure".to_string()),
                prompt_text: "Keep this queued.".to_string(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue");
        {
            let conn = state.db.lock().await;
            conn.execute("DROP TABLE thread_references", [])
                .expect("break attachment lookup");
        }
        let (tx, _rx) = tokio::sync::oneshot::channel();
        state
            .prompt_channels
            .lock()
            .await
            .insert("request-failure".to_string(), tx);
        state.prompt_waits.lock().unwrap().insert(
            "request-failure".to_string(),
            crate::models::PromptResumeState {
                pgid: None,
                agent_label: "Claude".to_string(),
                session_id: "session-failure".to_string(),
                thread_id: Some("thread-failure".to_string()),
            },
        );

        let error = auto_deliver_queued_prompt_batch("request-failure", "thread-failure", &state)
            .await
            .expect_err("attachment failure");

        assert!(error.message.contains("thread_references"));
        assert!(state
            .prompt_channels
            .lock()
            .await
            .contains_key("request-failure"));
        assert!(state
            .prompt_waits
            .lock()
            .unwrap()
            .contains_key("request-failure"));
    }

    #[test]
    fn changing_restart_thread_requires_canceling_old_folder_render() {
        let snapshot = |thread_id: &str| LastDesignSnapshot {
            design: None,
            thread_id: Some(thread_id.to_string()),
            message_id: None,
            artifact_bundle: None,
            model_manifest: None,
            selected_part_id: None,
            target_ref: None,
        };
        let previous = snapshot("thread-old");
        let same = snapshot("thread-old");
        let next = snapshot("thread-next");

        assert!(!snapshot_thread_changed(Some(&previous), Some(&same)));
        assert!(snapshot_thread_changed(Some(&previous), Some(&next)));
        assert!(snapshot_thread_changed(Some(&previous), None));
    }

    #[tokio::test]
    async fn queue_agent_prompt_impl_creates_a_pending_user_message_in_a_new_thread() {
        let conn = crate::db::init_db(&test_db_path("queue-agent-prompt-new-thread")).expect("db");
        let state = AppState::new(test_config(), None, conn);

        let queued = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: None,
                prompt_text: "Make the rim thinner.".to_string(),
                attachments: vec![crate::contracts::Attachment {
                    path: "/tmp/rim-reference.png".to_string(),
                    name: "rim-reference.png".to_string(),
                    explanation: "Outer rim profile".to_string(),
                    data_url: None,
                    kind: crate::contracts::AttachmentKind::Image,
                }],
            },
            &state,
        )
        .await
        .expect("queue prompt");

        let stored_thread = {
            let conn = state.db.lock().await;
            crate::services::history::get_thread(&conn, &queued.thread_id).expect("thread")
        };
        assert_eq!(stored_thread.messages.len(), 1);
        assert_eq!(stored_thread.messages[0].id, queued.message_id);
        assert_eq!(
            stored_thread.messages[0].role,
            crate::contracts::MessageRole::User
        );
        assert_eq!(
            stored_thread.messages[0].status,
            crate::contracts::MessageStatus::Pending
        );
        assert_eq!(stored_thread.messages[0].content, "Make the rim thinner.");
        assert_eq!(
            stored_thread.messages[0].attachment_images,
            vec!["/tmp/rim-reference.png".to_string()]
        );
    }

    #[tokio::test]
    async fn queue_agent_prompt_impl_creates_thread_without_authoring_context() {
        let conn =
            crate::db::init_db(&test_db_path("queue-agent-prompt-authoring-defaults")).expect("db");
        let mut config = test_config();
        config.default_source_language = crate::contracts::SourceLanguage::EckyIrV0;
        config.default_geometry_backend = crate::contracts::GeometryBackend::Freecad;
        let state = AppState::new(config, None, conn);

        let queued = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: None,
                prompt_text: "Make the rim thinner.".to_string(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue prompt");

        let stored_thread = {
            let conn = state.db.lock().await;
            crate::services::history::get_thread(&conn, &queued.thread_id).expect("thread")
        };
        assert_eq!(stored_thread.title, "Make the rim thinner.");
        assert_eq!(stored_thread.queued_count, 1);
    }

    #[tokio::test]
    async fn queue_agent_prompt_impl_preserves_existing_thread_identity() {
        let conn =
            crate::db::init_db(&test_db_path("queue-agent-prompt-existing-authoring")).expect("db");
        let mut config = test_config();
        config.default_source_language = crate::contracts::SourceLanguage::EckyIrV0;
        config.default_geometry_backend = crate::contracts::GeometryBackend::Freecad;
        let state = AppState::new(config, None, conn);

        {
            let conn = state.db.lock().await;
            crate::db::create_or_update_thread(&conn, "thread-existing", "Existing", 42, None)
                .expect("thread");
        }

        queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-existing".to_string()),
                prompt_text: "Make the rim thinner.".to_string(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue prompt");

        let stored_thread = {
            let conn = state.db.lock().await;
            crate::services::history::get_thread(&conn, "thread-existing").expect("thread")
        };
        assert_eq!(stored_thread.title, "Existing");
        assert_eq!(stored_thread.queued_count, 1);
    }

    #[tokio::test]
    async fn resolve_agent_prompt_impl_preserves_attachments() {
        let conn =
            crate::db::init_db(&test_db_path("resolve-agent-prompt-attachments")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let (tx, rx) = tokio::sync::oneshot::channel();

        state
            .prompt_channels
            .lock()
            .await
            .insert("req-1".to_string(), tx);

        resolve_agent_prompt_impl(
            crate::contracts::ResolveAgentPromptInput {
                request_id: "req-1".to_string(),
                prompt_text: "show outer frame".to_string(),
                message_ids: Vec::new(),
                message_id: None,
                attachments: vec![crate::contracts::Attachment {
                    path: "/tmp/frame.png".to_string(),
                    name: "frame.png".to_string(),
                    explanation: "Reference photo".to_string(),
                    data_url: None,
                    kind: crate::contracts::AttachmentKind::Image,
                }],
            },
            &state,
        )
        .await
        .expect("resolve agent prompt");

        let delivered = rx.await.expect("prompt delivery").expect("prompt ok");
        assert_eq!(delivered.prompt_text, "show outer frame");
        assert_eq!(delivered.attachments.len(), 1);
        assert_eq!(delivered.attachments[0].path, "/tmp/frame.png");
        assert_eq!(
            delivered.attachments[0].kind,
            crate::contracts::AttachmentKind::Image
        );
    }

    #[tokio::test]
    async fn resolve_agent_prompt_impl_persists_user_reply_to_thread_history() {
        let conn = crate::db::init_db(&test_db_path("resolve-agent-prompt-history")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let timestamp = 42_u64;

        {
            let conn = state.db.lock().await;
            crate::db::create_or_update_thread(&conn, "thread-1", "Thread", timestamp, None)
                .expect("thread");
        }

        state
            .prompt_channels
            .lock()
            .await
            .insert("req-2".to_string(), tx);
        state.prompt_waits.lock().unwrap().insert(
            "req-2".to_string(),
            crate::models::PromptResumeState {
                pgid: None,
                agent_label: "Claude".to_string(),
                session_id: "session-1".to_string(),
                thread_id: Some("thread-1".to_string()),
            },
        );

        resolve_agent_prompt_impl(
            crate::contracts::ResolveAgentPromptInput {
                request_id: "req-2".to_string(),
                prompt_text: "Use the smoother lip.".to_string(),
                message_ids: Vec::new(),
                message_id: None,
                attachments: vec![crate::contracts::Attachment {
                    path: "/tmp/reference.png".to_string(),
                    name: "reference.png".to_string(),
                    explanation: "Reference".to_string(),
                    data_url: None,
                    kind: crate::contracts::AttachmentKind::Image,
                }],
            },
            &state,
        )
        .await
        .expect("resolve prompt");

        let delivered = rx.await.expect("prompt delivery").expect("prompt ok");
        assert_eq!(delivered.prompt_text, "Use the smoother lip.");

        let stored_messages = {
            let conn = state.db.lock().await;
            crate::db::get_thread_messages(&conn, "thread-1").expect("messages")
        };
        assert_eq!(stored_messages.len(), 1);
        assert_eq!(stored_messages[0].role, crate::contracts::MessageRole::User);
        assert_eq!(stored_messages[0].content, "Use the smoother lip.");
        assert_eq!(
            stored_messages[0].status,
            crate::contracts::MessageStatus::Working
        );
        assert_eq!(
            stored_messages[0].attachment_images,
            vec!["/tmp/reference.png".to_string()]
        );
    }

    #[tokio::test]
    async fn resolve_agent_prompt_impl_marks_a_queued_history_message_delivered_without_duplication(
    ) {
        let conn = crate::db::init_db(&test_db_path("resolve-agent-prompt-queued")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let (tx, rx) = tokio::sync::oneshot::channel();

        let queued = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-queued".to_string()),
                prompt_text: "Add a softer chamfer.".to_string(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue prompt");

        state
            .prompt_channels
            .lock()
            .await
            .insert("req-queued".to_string(), tx);

        resolve_agent_prompt_impl(
            crate::contracts::ResolveAgentPromptInput {
                request_id: "req-queued".to_string(),
                prompt_text: "Add a softer chamfer.".to_string(),
                message_ids: vec![queued.message_id.clone()],
                message_id: Some(queued.message_id.clone()),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("resolve prompt");

        let delivered = rx.await.expect("prompt delivery").expect("prompt ok");
        assert_eq!(delivered.prompt_text, "Add a softer chamfer.");
        assert_eq!(
            delivered.message_id.as_deref(),
            Some(queued.message_id.as_str())
        );

        let stored_messages = {
            let conn = state.db.lock().await;
            crate::db::get_thread_messages(&conn, &queued.thread_id).expect("messages")
        };
        assert_eq!(stored_messages.len(), 1);
        assert_eq!(stored_messages[0].id, queued.message_id);
        assert_eq!(
            stored_messages[0].status,
            crate::contracts::MessageStatus::Working
        );
        assert_eq!(stored_messages[0].content, "Add a softer chamfer.");
    }

    #[tokio::test]
    async fn resolve_agent_prompt_impl_drains_a_batch_of_pending_messages_into_working() {
        let conn = crate::db::init_db(&test_db_path("resolve-agent-prompt-batch")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let (tx, rx) = tokio::sync::oneshot::channel();

        let first = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-batch".to_string()),
                prompt_text: "Make it taller.".to_string(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue first");
        let second = queue_agent_prompt_impl(
            crate::contracts::QueueAgentPromptInput {
                thread_id: Some("thread-batch".to_string()),
                prompt_text: "Then soften the rim.".to_string(),
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("queue second");

        state
            .prompt_channels
            .lock()
            .await
            .insert("req-batch".to_string(), tx);

        resolve_agent_prompt_impl(
            crate::contracts::ResolveAgentPromptInput {
                request_id: "req-batch".to_string(),
                prompt_text: "Make it taller.\n\nThen soften the rim.".to_string(),
                message_ids: vec![first.message_id.clone(), second.message_id.clone()],
                message_id: None,
                attachments: Vec::new(),
            },
            &state,
        )
        .await
        .expect("resolve prompt batch");

        let delivered = rx.await.expect("prompt delivery").expect("prompt ok");
        assert_eq!(delivered.message_ids.len(), 2);

        let stored_messages = {
            let conn = state.db.lock().await;
            crate::db::get_thread_messages(&conn, &first.thread_id).expect("messages")
        };
        let first_message = stored_messages
            .iter()
            .find(|message| message.id == first.message_id)
            .expect("first");
        let second_message = stored_messages
            .iter()
            .find(|message| message.id == second.message_id)
            .expect("second");
        assert_eq!(
            first_message.status,
            crate::contracts::MessageStatus::Working
        );
        assert_eq!(
            second_message.status,
            crate::contracts::MessageStatus::Working
        );
        assert_eq!(first_message.content, "Make it taller.");
        assert_eq!(second_message.content, "Then soften the rim.");
    }

    #[test]
    fn stage_prompt_attachments_to_dir_copies_files_into_a_session_scoped_inbox() {
        let staging_root =
            std::env::temp_dir().join(format!("ecky-stage-attachments-{}", uuid::Uuid::new_v4()));
        let source_path = staging_root.join("source.png");
        fs::create_dir_all(&staging_root).expect("staging root");
        fs::write(&source_path, b"png-data").expect("source file");

        let staged = stage_prompt_attachments_to_dir(
            &staging_root,
            "request-1",
            Some("session-1"),
            &[crate::contracts::Attachment {
                path: source_path.to_string_lossy().to_string(),
                name: "source.png".to_string(),
                explanation: "Reference".to_string(),
                data_url: None,
                kind: crate::contracts::AttachmentKind::Image,
            }],
        )
        .expect("staged attachments");

        assert_eq!(staged.len(), 1);
        assert!(
            staged[0]
                .path
                .contains("mcp-attachments/session-1/request-1/01-source.png"),
            "unexpected staged path: {}",
            staged[0].path
        );
        assert_eq!(fs::read(&staged[0].path).expect("staged file"), b"png-data");
    }

    #[test]
    fn attachments_already_staged_accepts_only_absolute_paths_inside_the_managed_inbox() {
        let staged_root = PathBuf::from("/tmp/ecky-config/mcp-attachments");
        let inside = vec![crate::contracts::Attachment {
            path: "/tmp/ecky-config/mcp-attachments/session-a/request-a/01-reference.png"
                .to_string(),
            name: "reference.png".to_string(),
            explanation: "Reference".to_string(),
            data_url: None,
            kind: crate::contracts::AttachmentKind::Image,
        }];
        let outside = vec![crate::contracts::Attachment {
            path: "/var/folders/example/reference.png".to_string(),
            name: "reference.png".to_string(),
            explanation: "Reference".to_string(),
            data_url: None,
            kind: crate::contracts::AttachmentKind::Image,
        }];
        let relative = vec![crate::contracts::Attachment {
            path: "reference.png".to_string(),
            name: "reference.png".to_string(),
            explanation: "Reference".to_string(),
            data_url: None,
            kind: crate::contracts::AttachmentKind::Image,
        }];

        assert!(attachments_already_staged(&staged_root, &inside));
        assert!(!attachments_already_staged(&staged_root, &outside));
        assert!(!attachments_already_staged(&staged_root, &relative));
    }

    #[test]
    fn stage_prompt_workspace_capture_to_dir_returns_inline_image_attachment() {
        let staged = stage_prompt_workspace_capture_to_dir(
            &crate::contracts::PreparePromptWorkspaceCaptureInput {
                data_url: "data:image/png;base64,Zm9v".to_string(),
                thread_id: Some("thread-1".to_string()),
                name: Some("workspace-view".to_string()),
                explanation: Some("Current workspace with annotations.".to_string()),
            },
        )
        .expect("workspace capture");

        assert_eq!(staged.path, "");
        assert_eq!(staged.name, "workspace-view.png");
        assert_eq!(staged.explanation, "Current workspace with annotations.");
        assert_eq!(
            staged.data_url.as_deref(),
            Some("data:image/png;base64,Zm9v")
        );
        assert_eq!(staged.kind, crate::contracts::AttachmentKind::Image);
    }

    #[test]
    fn stage_prompt_workspace_capture_to_dir_accepts_svg_images() {
        let staged = stage_prompt_workspace_capture_to_dir(
            &crate::contracts::PreparePromptWorkspaceCaptureInput {
                data_url: "data:image/svg+xml;base64,PHN2Zy8+".to_string(),
                thread_id: Some("thread-1".to_string()),
                name: Some("overlay".to_string()),
                explanation: None,
            },
        )
        .expect("svg workspace capture");

        assert_eq!(staged.path, "");
        assert_eq!(staged.name, "overlay.svg");
        assert_eq!(
            staged.data_url.as_deref(),
            Some("data:image/svg+xml;base64,PHN2Zy8+")
        );
        assert_eq!(staged.kind, crate::contracts::AttachmentKind::Image);
    }

    #[test]
    fn stage_prompt_workspace_capture_to_dir_rejects_non_image_data_urls() {
        let err = stage_prompt_workspace_capture_to_dir(
            &crate::contracts::PreparePromptWorkspaceCaptureInput {
                data_url: "data:text/plain;base64,Zm9v".to_string(),
                thread_id: None,
                name: None,
                explanation: None,
            },
        )
        .expect_err("non-image data url should fail");

        assert!(err
            .message
            .contains("Unsupported workspace capture MIME type"));
    }

    #[test]
    fn stage_prompt_attachments_to_dir_keeps_inline_images_in_memory() {
        let staging_root =
            std::env::temp_dir().join(format!("ecky-stage-attachments-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&staging_root).expect("staging root");

        let staged = stage_prompt_attachments_to_dir(
            &staging_root,
            "request-inline",
            Some("session-1"),
            &[crate::contracts::Attachment {
                path: String::new(),
                name: "concept.png".to_string(),
                explanation: "Inline".to_string(),
                data_url: Some("data:image/png;base64,Zm9v".to_string()),
                kind: crate::contracts::AttachmentKind::Image,
            }],
        )
        .expect("inline attachments");

        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].path, "");
        assert_eq!(
            staged[0].data_url.as_deref(),
            Some("data:image/png;base64,Zm9v")
        );
    }

    #[test]
    fn encode_agent_terminal_input_appends_enter_for_submit_lines() {
        let payload = encode_agent_terminal_input(&crate::contracts::AgentTerminalInput {
            agent_id: "claude".to_string(),
            text: "2".to_string(),
            key: None,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            submit: true,
        })
        .expect("payload");

        assert_eq!(payload, b"2\r");
    }

    #[test]
    fn encode_agent_terminal_input_supports_arrow_keys() {
        let payload = encode_agent_terminal_input(&crate::contracts::AgentTerminalInput {
            agent_id: "claude".to_string(),
            text: String::new(),
            key: Some("ArrowDown".to_string()),
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            submit: false,
        })
        .expect("payload");

        assert_eq!(payload, b"\x1b[B");
    }

    #[test]
    fn encode_agent_terminal_input_supports_ctrl_shortcuts() {
        let payload = encode_agent_terminal_input(&crate::contracts::AgentTerminalInput {
            agent_id: "claude".to_string(),
            text: String::new(),
            key: Some("c".to_string()),
            ctrl: true,
            alt: false,
            shift: false,
            meta: false,
            submit: false,
        })
        .expect("payload");

        assert_eq!(payload, vec![0x03]);
    }

    #[tokio::test]
    async fn resize_agent_terminal_updates_pty_size() {
        let conn = crate::db::init_db(&test_db_path("resize-agent-terminal")).expect("db");
        let state = AppState::new(test_config(), None, conn);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("pty");
        let writer = pair.master.take_writer().expect("writer");
        let pty: crate::models::AgentTerminalPty =
            std::sync::Arc::new(std::sync::Mutex::new(pair.master));

        state.agent_terminals.lock().unwrap().insert(
            "gemini".to_string(),
            crate::models::AgentTerminalRuntime {
                snapshot: crate::contracts::AgentTerminalSnapshot {
                    agent_id: "gemini".to_string(),
                    agent_label: "gemini".to_string(),
                    provider_kind: Some("gemini".to_string()),
                    session_id: None,
                    session_nonce: 1,
                    screen_text: String::new(),
                    vt_stream: String::new(),
                    vt_delta: None,
                    attention_required: false,
                    busy: false,
                    activity_label: None,
                    activity_started_at: None,
                    attention_kind: None,
                    summary: None,
                    active: true,
                    updated_at: 1,
                },
                writer: std::sync::Arc::new(std::sync::Mutex::new(writer)),
                pty: pty.clone(),
                pending_utf8: Vec::new(),
                pending_escape: String::new(),
                last_emitted_at: None,
            },
        );

        resize_agent_terminal_impl("gemini", 132, 41, &state).expect("resize");

        let size = pty.lock().unwrap().get_size().expect("size");
        assert_eq!(size.cols, 132);
        assert_eq!(size.rows, 41);
    }
}
