use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use portable_pty::PtySize;
use tauri::{AppHandle, Manager, State};

use crate::db;
use crate::mcp::runtime;
use crate::models::{
    AgentSession, AppError, AppResult, AppState, LastDesignSnapshot, McpServerStatus,
    ThreadAgentState, ViewportScreenshotCapture,
};
use crate::services::agent_dialogue;

const HARD_BUSY_STALE_TIMEOUT_SECS: u64 = 30;

fn is_hard_busy_phase(phase: &str) -> bool {
    matches!(phase, "rendering" | "saving_version" | "restoring_version")
}

async fn clear_stale_hard_busy_session_for_thread(
    state: &AppState,
    thread_id: &str,
    now: u64,
) -> Option<String> {
    let stale_before = now.saturating_sub(HARD_BUSY_STALE_TIMEOUT_SECS);
    let mut sessions = state.mcp_sessions.lock().await;
    for (session_id, session) in sessions.iter_mut() {
        let matches_thread = session
            .last_target
            .as_ref()
            .map(|target| target.thread_id.as_str())
            == Some(thread_id)
            || session.bound_thread_id.as_deref() == Some(thread_id);
        let Some(phase) = session.phase.as_deref() else {
            continue;
        };
        if !matches_thread
            || !session.busy
            || !is_hard_busy_phase(phase)
            || session.updated_at >= stale_before
        {
            continue;
        }
        session.phase = Some("idle".to_string());
        session.status_text = Some("Ready.".to_string());
        session.busy = false;
        session.activity_label = None;
        session.activity_started_at = None;
        session.attention_kind = None;
        session.waiting_on_prompt = false;
        session.updated_at = now;
        return Some(session_id.clone());
    }
    None
}

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
        .map_err(|e| crate::models::AppError::persistence(e.to_string()))
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

fn last_snapshot_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap()
        .join("last_design.json")
}

pub(crate) fn write_last_snapshot(app: &AppHandle, snapshot: Option<&LastDesignSnapshot>) {
    let path = last_snapshot_path(app);
    match snapshot {
        Some(snapshot) => {
            if let Ok(serialized) = serde_json::to_string_pretty(snapshot) {
                let _ = fs::write(path, serialized);
            }
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
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
        let legacy_message = crate::db::get_thread_messages(&conn, &thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
            .into_iter()
            .find(|message| message.id == message_id)
            .ok_or_else(|| AppError::not_found(format!("Message {} not found.", message_id)))?;
        attachments = legacy_message
            .attachment_images
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

async fn queue_agent_prompt_impl(
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
        }

        crate::db::add_message(
            &conn,
            &thread_id,
            &crate::models::Message {
                id: message_id.clone(),
                role: crate::models::MessageRole::User,
                content,
                status: crate::models::MessageStatus::Pending,
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

    state.emit_history_updated();
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
pub async fn get_message_attachments(
    message_id: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::contracts::Attachment>> {
    get_message_attachments_impl(&message_id, &state).await
}

pub(crate) fn build_runtime_snapshot(
    design: Option<crate::models::DesignOutput>,
    thread_id: Option<String>,
    message_id: Option<String>,
    artifact_bundle: Option<crate::models::ArtifactBundle>,
    model_manifest: Option<crate::models::ModelManifest>,
    selected_part_id: Option<String>,
) -> LastDesignSnapshot {
    LastDesignSnapshot {
        design,
        thread_id,
        message_id,
        artifact_bundle,
        model_manifest,
        selected_part_id,
    }
}

async fn resolve_thread_agent_state_inputs(
    state: &AppState,
    thread_id: &str,
) -> AppResult<runtime::ThreadAgentStateInputs> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = clear_stale_hard_busy_session_for_thread(state, thread_id, now).await;
    let conn = state.db.lock().await;
    let last_session = db::get_thread_last_agent_session(&conn, thread_id)
        .map_err(|e| AppError::persistence(e.to_string()))?;
    drop(conn);

    let (live_session_id, live_session) = {
        let sessions = state.mcp_sessions.lock().await;
        if let Some((session_id, session)) = sessions.iter().find(|(_, session)| {
            session
                .last_target
                .as_ref()
                .map(|target| target.thread_id.as_str())
                == Some(thread_id)
                || session.bound_thread_id.as_deref() == Some(thread_id)
        }) {
            (Some(session_id.clone()), Some(session.clone()))
        } else if let Some(last_session) = last_session.as_ref() {
            (
                Some(last_session.session_id.clone()),
                sessions.get(&last_session.session_id).cloned(),
            )
        } else {
            (None, None)
        }
    };

    let candidate_session_id = live_session_id.clone().or_else(|| {
        last_session
            .as_ref()
            .map(|session| session.session_id.clone())
    });
    let runtime_snapshot =
        runtime::runtime_snapshot_for_thread(state, thread_id, candidate_session_id.as_deref());

    Ok(runtime::ThreadAgentStateInputs {
        live_session_id,
        runtime: runtime_snapshot,
        live_session,
        last_session,
        now,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_last_design(state: State<'_, AppState>) -> AppResult<Option<LastDesignSnapshot>> {
    Ok(state.last_snapshot.lock().unwrap().clone())
}

#[tauri::command]
#[specta::specta]
pub async fn save_last_design(
    snapshot: Option<LastDesignSnapshot>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = snapshot.clone();
    }
    write_last_snapshot(&app, snapshot.as_ref());
    Ok(())
}

/// Returns the current agent state for the given thread — for status bar display.
#[tauri::command]
#[specta::specta]
pub async fn get_thread_agent_state(
    thread_id: String,
    state: State<'_, AppState>,
) -> AppResult<ThreadAgentState> {
    let config = state.config.lock().unwrap().clone();
    let inputs = resolve_thread_agent_state_inputs(&state, &thread_id).await?;

    Ok(runtime::derive_thread_agent_state(
        &config, &thread_id, inputs,
    ))
}

async fn resolve_agent_prompt_impl(
    input: crate::contracts::ResolveAgentPromptInput,
    state: &AppState,
) -> AppResult<()> {
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
        let mut sessions = state.mcp_sessions.lock().await;
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
    let staged_attachments = stage_prompt_attachments(
        state,
        &request_id,
        prompt_control
            .as_ref()
            .map(|control| control.session_id.as_str()),
        &input.attachments,
    )?;
    let mut delivered_input = input.clone();
    delivered_input.attachments = staged_attachments.clone();

    let mut channels = state.prompt_channels.lock().await;
    if let Some(tx) = channels.remove(&request_id) {
        let _ = tx.send(Ok(delivered_input.clone()));
    } else {
        return Err(AppError::not_found(format!(
            "No pending prompt request with id: {}",
            request_id
        )));
    }

    let reply_content = agent_dialogue::build_user_reply_message_content(
        &delivered_input.prompt_text,
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
                        status: &crate::models::MessageStatus::Working,
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
        state.emit_history_updated();
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
            &crate::models::Message {
                id: message_id.clone(),
                role: crate::models::MessageRole::User,
                content: reply_content,
                status: crate::models::MessageStatus::Working,
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
        working_message_ids = vec![message_id];
        state.emit_history_updated();
    }

    if let Some(control) = prompt_control.as_ref() {
        if !working_message_ids.is_empty() {
            let mut sessions = state.mcp_sessions.lock().await;
            if let Some(session) = sessions.get_mut(&control.session_id) {
                session.current_turn_working_message_ids = working_message_ids;
            }
        }
    }
    Ok(())
}

/// Called by the frontend when the user submits a prompt in MCP mode.
/// Resolves the pending oneshot channel so the MCP handler can return the text and attachments.
#[tauri::command]
#[specta::specta]
pub async fn resolve_agent_prompt(
    input: crate::contracts::ResolveAgentPromptInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    resolve_agent_prompt_impl(input, &state).await
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

/// Called by the frontend when the user queues a message in MCP mode and no agent is running.
/// Fires the wake notifier so the supervisor loop can respawn the named agent.
/// Safe to call redundantly — noop if the agent is already running.
#[tauri::command]
#[specta::specta]
pub async fn wake_auto_agent(label: String, state: State<'_, AppState>) -> AppResult<()> {
    runtime::wake_auto_agent_by_label(&state, &label, None).await
}

#[tauri::command]
#[specta::specta]
pub async fn wake_primary_auto_agent(
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    runtime::wake_primary_auto_agent(&state, thread_id, message_id, model_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_primary_auto_agent(
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    runtime::stop_primary_auto_agent(&state, thread_id, message_id, model_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn restart_primary_auto_agent(
    thread_id: Option<String>,
    message_id: Option<String>,
    model_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    runtime::restart_primary_auto_agent(&state, thread_id, message_id, model_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Config, McpConfig};
    use crate::models::{AgentSession, McpSessionState, McpTargetRef};
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
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            default_engine_kind: crate::models::EngineKind::Freecad,
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            default_geometry_backend: crate::models::GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
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
            crate::models::MessageRole::User
        );
        assert_eq!(
            stored_thread.messages[0].status,
            crate::models::MessageStatus::Pending
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
        config.default_source_language = crate::models::SourceLanguage::EckyIrV0;
        config.default_geometry_backend = crate::models::GeometryBackend::Freecad;
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
        config.default_source_language = crate::models::SourceLanguage::EckyIrV0;
        config.default_geometry_backend = crate::models::GeometryBackend::Freecad;
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
        assert_eq!(stored_messages[0].role, crate::models::MessageRole::User);
        assert_eq!(stored_messages[0].content, "Use the smoother lip.");
        assert_eq!(
            stored_messages[0].status,
            crate::models::MessageStatus::Working
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
            crate::models::MessageStatus::Working
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
        assert_eq!(first_message.status, crate::models::MessageStatus::Working);
        assert_eq!(second_message.status, crate::models::MessageStatus::Working);
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

    #[tokio::test]
    async fn resolve_thread_agent_state_inputs_do_not_leak_primary_runtime_into_other_thread() {
        let conn = crate::db::init_db(&test_db_path("thread-state-sources")).expect("db");
        let mut config = test_config();
        config.connection_type = Some("mcp".to_string());
        config.mcp.mode = crate::contracts::McpMode::Active;
        config.mcp.primary_agent_id = Some("agent-primary".to_string());
        config.mcp.auto_agents = vec![crate::contracts::AutoAgent {
            id: "agent-primary".to_string(),
            label: "Primary".to_string(),
            cmd: "claude".to_string(),
            model: None,
            args: Vec::new(),
            enabled: true,
            start_on_demand: true,
        }];
        let state = AppState::new(config, None, conn);
        crate::mcp::runtime::initialize_auto_agent_supervisors(state.clone());

        {
            let conn = state.db.lock().await;
            crate::db::upsert_agent_session(
                &conn,
                &AgentSession {
                    session_id: "session-thread-a".to_string(),
                    client_kind: "mcp-http".to_string(),
                    host_label: "Gemini CLI".to_string(),
                    agent_label: "gemini".to_string(),
                    llm_model_id: None,
                    llm_model_label: Some("Gemini".to_string()),
                    thread_id: Some("thread-a".to_string()),
                    message_id: Some("msg-a".to_string()),
                    model_id: None,
                    phase: "waiting_for_user".to_string(),
                    status_text: "Waiting".to_string(),
                    updated_at: 10,
                },
            )
            .expect("agent session");
        }

        state.mcp_sessions.lock().await.insert(
            "session-thread-a".to_string(),
            McpSessionState {
                client_kind: "mcp-http".to_string(),
                host_label: "Gemini CLI".to_string(),
                agent_label: "gemini".to_string(),
                llm_model_id: None,
                llm_model_label: Some("Gemini".to_string()),
                bound_thread_id: None,
                last_target: Some(McpTargetRef {
                    thread_id: "thread-a".to_string(),
                    message_id: "msg-a".to_string(),
                    model_id: None,
                }),
                phase: Some("waiting_for_user".to_string()),
                status_text: Some("Waiting".to_string()),
                busy: false,
                activity_label: None,
                activity_started_at: None,
                attention_kind: None,
                waiting_on_prompt: true,
                current_turn_id: None,
                current_turn_thread_id: None,
                current_turn_working_message_ids: Vec::new(),
                current_turn_working_version_message_id: None,
                updated_at: 11,
            },
        );

        crate::mcp::runtime::mark_agent_active(
            &state,
            "Primary",
            Some("session-thread-b".to_string()),
            Some("thread-b".to_string()),
            None,
            Some("Busy elsewhere.".to_string()),
        );

        let inputs = resolve_thread_agent_state_inputs(&state, "thread-a")
            .await
            .expect("thread inputs");
        assert!(inputs.runtime.is_none());
        assert_eq!(
            inputs
                .live_session
                .as_ref()
                .map(|session| session.agent_label.as_str()),
            Some("gemini")
        );
    }
}
