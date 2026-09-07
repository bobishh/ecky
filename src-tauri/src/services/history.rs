use crate::contracts::{AppError, AppResult, MessageRole, MessageStatus, Thread, ThreadStatus};
use crate::db;
use crate::persist_thread_summary;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn get_history(conn: &rusqlite::Connection) -> AppResult<Vec<Thread>> {
    let mut threads = db::get_recent_threads_limited(conn, 100)
        .map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))?;
    for thread in &mut threads {
        thread.title = crate::transport_budget::bounded_text(&thread.title, 512);
        thread.summary = crate::transport_budget::bounded_text(&thread.summary, 1_024);
    }
    crate::transport_budget::require_serialized_budget(
        "threadList",
        &threads,
        crate::transport_budget::THREAD_LIST_MAX_BYTES,
        "thread summary pagination",
    )?;
    Ok(threads)
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
        .filter(|m| m.status != MessageStatus::Discarded && db::is_version_message(m))
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
    let is_blank = title == "Untitled design"
        && !db::thread_has_durable_content(conn, id)
            .map_err(|err| AppError::persistence(err.to_string()))?;

    let lifecycle = db::get_thread_lifecycle(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .unwrap_or_else(db::ThreadLifecycle::active);

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
        is_blank,
        status: lifecycle.status(),
        finalized_at: lifecycle.finalized_at(),
        pending_confirm: lifecycle.pending_confirm().map(str::to_string),
    })
}

pub fn get_thread_summary(conn: &rusqlite::Connection, id: &str) -> AppResult<Thread> {
    db::get_thread_summary_by_id(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))
}

pub fn get_thread_latest_version(
    conn: &rusqlite::Connection,
    id: &str,
) -> AppResult<Option<crate::contracts::Message>> {
    db::get_visible_thread_title(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    db::get_thread_version_detail(conn, id, None)
        .map(|detail| detail.map(|detail| detail.message))
        .map_err(|err| AppError::persistence(err.to_string()))
}

pub fn get_thread_message_version(
    conn: &rusqlite::Connection,
    thread_id: &str,
    message_id: &str,
) -> AppResult<Option<crate::contracts::Message>> {
    db::get_visible_thread_title(conn, thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    db::get_thread_version_detail(conn, thread_id, Some(message_id))
        .map(|detail| detail.map(|detail| detail.message))
        .map_err(|err| AppError::persistence(err.to_string()))
}

pub fn get_version_detail(
    conn: &rusqlite::Connection,
    thread_id: &str,
    message_id: &str,
) -> AppResult<crate::contracts::VersionDetail> {
    db::get_thread_version_detail(conn, thread_id, Some(message_id))
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Version not found."))
}

pub fn get_version_source_window(
    conn: &rusqlite::Connection,
    thread_id: &str,
    message_id: &str,
    start_byte: usize,
    max_bytes: usize,
) -> AppResult<crate::contracts::SourceWindow> {
    const SOURCE_WINDOW_MAX_BYTES: usize = 256 * 1024;
    let (source, stored_digest) = db::get_version_source(conn, thread_id, message_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Version source not found."))?;
    let total_bytes = source.len();
    let mut start = start_byte.min(total_bytes);
    while start < total_bytes && !source.is_char_boundary(start) {
        start += 1;
    }
    let mut end = start
        .saturating_add(max_bytes.clamp(1, SOURCE_WINDOW_MAX_BYTES))
        .min(total_bytes);
    while end > start && !source.is_char_boundary(end) {
        end -= 1;
    }
    let digest =
        stored_digest.unwrap_or_else(|| format!("sha256:{:x}", Sha256::digest(source.as_bytes())));
    Ok(crate::contracts::SourceWindow {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        digest,
        content: source[start..end].to_string(),
        start_byte: start,
        end_byte: end,
        total_bytes,
        next_start_byte: (end < total_bytes).then_some(end),
        truncated: end < total_bytes || start > 0,
    })
}

fn topology_cursor_identity(thread_id: &str, message_id: &str, kind: &str) -> u64 {
    format!("{thread_id}\0{message_id}\0{kind}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

pub fn get_dense_topology_page(
    conn: &rusqlite::Connection,
    thread_id: &str,
    message_id: &str,
    kind: crate::contracts::DenseTopologyKind,
    cursor: Option<String>,
    limit: Option<usize>,
) -> AppResult<crate::contracts::DenseTopologyPage> {
    use crate::contracts::{DenseTopologyItem, DenseTopologyKind, DenseTopologyPage};
    let kind_name = match kind {
        DenseTopologyKind::Edge => "edge",
        DenseTopologyKind::Face => "face",
        DenseTopologyKind::Selection => "selection",
    };
    let identity = topology_cursor_identity(thread_id, message_id, kind_name);
    let offset = if let Some(cursor) = cursor {
        let mut parts = cursor.split(':');
        let valid = parts.next() == Some("v1")
            && parts
                .next()
                .is_some_and(|part| part == format!("{identity:016x}"));
        let offset = parts.next().and_then(|part| part.parse::<usize>().ok());
        if !valid || parts.next().is_some() || offset.is_none() {
            return Err(AppError::validation("Invalid dense topology cursor."));
        }
        offset.unwrap_or(0)
    } else {
        0
    };
    let safe_limit = limit.unwrap_or(500).clamp(1, 500);
    let (column, path) = match kind {
        DenseTopologyKind::Edge => ("artifact_bundle", "$.edgeTargets"),
        DenseTopologyKind::Face => ("artifact_bundle", "$.faceTargets"),
        DenseTopologyKind::Selection => ("model_manifest", "$.selectionTargets"),
    };
    let (raw_items, total_count) = db::get_dense_topology_json_page(
        conn,
        thread_id,
        message_id,
        column,
        path,
        offset,
        safe_limit + 1,
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;
    let mut items = Vec::new();
    let mut observed_bytes = 2usize;
    for raw in raw_items.into_iter().take(safe_limit) {
        let item = match kind {
            DenseTopologyKind::Edge => DenseTopologyItem::Edge(
                serde_json::from_str(&raw).map_err(|err| AppError::persistence(err.to_string()))?,
            ),
            DenseTopologyKind::Face => DenseTopologyItem::Face(
                serde_json::from_str(&raw).map_err(|err| AppError::persistence(err.to_string()))?,
            ),
            DenseTopologyKind::Selection => DenseTopologyItem::Selection(
                serde_json::from_str(&raw).map_err(|err| AppError::persistence(err.to_string()))?,
            ),
        };
        let item_bytes = serde_json::to_vec(&item)
            .map(|value| value.len())
            .unwrap_or(0);
        if observed_bytes.saturating_add(item_bytes)
            > crate::transport_budget::TOPOLOGY_PAGE_MAX_BYTES
        {
            if items.is_empty() {
                return Err(AppError::validation(format!(
                    "Dense topology item is {} bytes; allowed page is {} bytes. Reduce target metadata.",
                    item_bytes,
                    crate::transport_budget::TOPOLOGY_PAGE_MAX_BYTES
                )));
            }
            break;
        }
        observed_bytes = observed_bytes.saturating_add(item_bytes);
        items.push(item);
    }
    let next_offset = offset + items.len();
    Ok(DenseTopologyPage {
        snapshot_ref: format!("topology:{thread_id}:{message_id}"),
        kind,
        next_cursor: (next_offset < total_count)
            .then(|| format!("v1:{identity:016x}:{next_offset}")),
        total_count,
        observed_bytes,
        items,
    })
}

pub fn get_thread_messages_page(
    conn: &rusqlite::Connection,
    id: &str,
    before: Option<String>,
    limit: Option<usize>,
    include_visual_payloads: bool,
) -> AppResult<crate::contracts::ThreadMessagesPage> {
    if include_visual_payloads {
        return Err(AppError::validation(
            "Timeline pages never include visual/runtime payloads. Hydrate one selected version instead.",
        ));
    }
    db::get_visible_thread_title(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    db::get_thread_messages_page(
        conn,
        id,
        before.as_deref(),
        limit.unwrap_or(50),
        include_visual_payloads,
    )
    .map_err(|err| AppError::persistence(err.to_string()))
}

/// Loads the bounded timeline and its selected full version from one SQLite
/// read transaction. A stale persisted pointer falls back to the current head.
pub fn get_workspace_projection(
    conn: &rusqlite::Connection,
    thread_id: &str,
    preferred_message_id: Option<&str>,
    message_limit: Option<usize>,
) -> AppResult<crate::contracts::WorkspaceProjection> {
    let transaction = conn
        .unchecked_transaction()
        .map_err(|err| AppError::persistence(err.to_string()))?;
    let projection = get_workspace_projection_read(
        &transaction,
        thread_id,
        preferred_message_id,
        message_limit,
    )?;
    transaction
        .commit()
        .map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(projection)
}

pub(crate) fn get_workspace_projection_read(
    conn: &rusqlite::Connection,
    thread_id: &str,
    preferred_message_id: Option<&str>,
    message_limit: Option<usize>,
) -> AppResult<crate::contracts::WorkspaceProjection> {
    let thread = get_thread_summary(conn, thread_id)?;
    let messages_page = get_thread_messages_page(conn, thread_id, None, message_limit, false)?;
    let preferred_version = match preferred_message_id {
        Some(message_id) => get_thread_message_version(conn, thread_id, message_id)?,
        None => None,
    };
    let requested_message_found = preferred_version.is_some();
    let selected_version = match preferred_version {
        Some(version) => Some(version),
        None => get_thread_latest_version(conn, thread_id)?,
    };

    let projection = crate::contracts::WorkspaceProjection {
        thread,
        messages_page,
        selected_version,
        requested_message_found,
    };
    crate::transport_budget::require_serialized_budget(
        "workspaceProjection",
        &projection,
        crate::transport_budget::ORDINARY_JSON_MAX_BYTES,
        "get_thread_messages_page and get_version_detail",
    )?;
    Ok(projection)
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForkDesignRequest {
    pub source_thread_id: String,
    pub source_message_id: String,
    pub title: Option<String>,
    pub version_name: Option<String>,
    pub message_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ForkDesignResponse {
    pub thread_id: String,
    pub message_id: String,
    pub workspace: crate::contracts::WorkspaceProjection,
}

fn fork_title(source: &crate::contracts::Message) -> String {
    source
        .output
        .as_ref()
        .map(|output| output.title.trim())
        .filter(|title| !title.is_empty())
        .or_else(|| {
            source
                .model_manifest
                .as_ref()
                .map(|manifest| manifest.document.document_label.trim())
                .filter(|title| !title.is_empty())
        })
        .or_else(|| {
            source
                .model_manifest
                .as_ref()
                .map(|manifest| manifest.document.document_name.trim())
                .filter(|title| !title.is_empty())
        })
        .or_else(|| {
            source
                .artifact_bundle
                .as_ref()
                .map(|bundle| bundle.model_id.trim())
                .filter(|title| !title.is_empty())
        })
        .unwrap_or("Imported Model")
        .to_string()
}

/// Forks one exact immutable version into a new thread and returns the full
/// first-load projection. Callers choose the source; Rust owns payload kind,
/// new identities, append semantics, source binding, and canonical hydration.
pub async fn fork_design(
    request: ForkDesignRequest,
    state: &crate::models::AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<ForkDesignResponse> {
    let source = {
        let conn = state.db.lock().await;
        db::get_thread_message_version(&conn, &request.source_thread_id, &request.source_message_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "Fork source version '{}' was not found in thread '{}'.",
                    request.source_message_id, request.source_thread_id
                ))
            })?
    };
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fork_title(&source));
    let new_thread_id = uuid::Uuid::new_v4().to_string();

    let message_id = if let Some(output) = source.output.as_ref() {
        let version_name = request
            .version_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                (!output.version_name.trim().is_empty()).then(|| output.version_name.clone())
            })
            .unwrap_or_else(|| "Forked".to_string());
        crate::services::design::add_manual_version(
            crate::services::design::AddManualVersionRequest {
                thread_id: new_thread_id.clone(),
                title: title.clone(),
                version_name,
                macro_code: output.macro_code.clone(),
                source_language: Some(output.source_language),
                geometry_backend: Some(output.geometry_backend),
                parameters: output.initial_params.clone(),
                ui_spec: output.ui_spec.clone(),
                post_processing: output.post_processing.clone(),
                artifact_bundle: source.artifact_bundle.clone(),
                model_manifest: source.model_manifest.clone(),
                response_text: Some("Design forked into a new thread.".to_string()),
                agent_origin: None,
                status: Some(source.status.clone()),
                error_message: (source.status == crate::contracts::MessageStatus::Error)
                    .then(|| source.content.clone()),
            },
            state,
            app,
        )
        .await?
    } else {
        let artifact_bundle = source
            .artifact_bundle
            .clone()
            .ok_or_else(|| AppError::validation("Fork source has no reusable payload."))?;
        let model_manifest = match source.model_manifest.clone() {
            Some(manifest) => manifest,
            None => crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)
                .map_err(|error| {
                    AppError::validation(format!(
                        "Fork source runtime manifest is unavailable: {error}"
                    ))
                })?,
        };
        crate::services::imported_model::persist_imported_runtime(
            Some(new_thread_id.clone()),
            Some(title.clone()),
            artifact_bundle,
            model_manifest,
            state,
            app,
        )
        .await?
        .message_id
    };

    let workspace = {
        let conn = state.db.lock().await;
        get_workspace_projection(
            &conn,
            &new_thread_id,
            Some(&message_id),
            request.message_limit,
        )?
    };
    let selected = workspace
        .selected_version
        .as_ref()
        .ok_or_else(|| AppError::persistence("Forked version projection is unavailable."))?;
    let snapshot = crate::services::session::build_saved_version_snapshot(
        selected.output.clone(),
        new_thread_id.clone(),
        message_id.clone(),
        selected.artifact_bundle.clone(),
        selected.model_manifest.clone(),
        None,
    );
    {
        let mut last = state.last_snapshot.lock().unwrap();
        *last = Some(snapshot.clone());
    }
    crate::services::session::write_last_snapshot(app, Some(&snapshot));
    state.emit_history_changed(
        Some(new_thread_id.clone()),
        Some(message_id.clone()),
        "threadForked",
    );

    Ok(ForkDesignResponse {
        thread_id: new_thread_id,
        message_id,
        workspace,
    })
}

pub fn get_thread_messages_page_filtered(
    conn: &rusqlite::Connection,
    id: &str,
    before: Option<String>,
    limit: Option<usize>,
    roles: Option<&[MessageRole]>,
) -> AppResult<crate::contracts::ThreadMessagesPage> {
    db::get_visible_thread_title(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    db::get_thread_messages_page_filtered(conn, id, before.as_deref(), limit.unwrap_or(50), roles)
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

    db::get_visible_thread_title(conn, thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    let has_version = db::has_renderable_thread_version(conn, thread_id, selected_message_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    if !has_version {
        return Err(AppError::validation(if selected_message_id.is_some() {
            "Selected final model is not a valid version in this thread."
        } else {
            "Thread has no successful versions to finalize."
        }));
    }

    let changed = db::finalize_thread(conn, thread_id, now as i64)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    if !changed {
        return Err(AppError::not_found("Thread not found."));
    }

    Ok(())
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

pub fn get_thread_preview(
    conn: &rusqlite::Connection,
    thread_id: &str,
) -> AppResult<Option<String>> {
    db::get_thread_preview(conn, thread_id).map_err(|err| AppError::persistence(err.to_string()))
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

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VersionHistoryIntent {
    pub message_id: String,
    pub selected_thread_id: Option<String>,
    pub selected_message_id: Option<String>,
    pub message_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VersionHistoryIntentResponse {
    pub thread_id: String,
    pub workspace: Option<crate::contracts::WorkspaceProjection>,
    pub thread_removed: bool,
}

fn persist_intent_workspace_selection(
    selected_thread_id: Option<&str>,
    thread_id: &str,
    workspace: Option<&crate::contracts::WorkspaceProjection>,
    state: &crate::models::AppState,
    app: &dyn crate::models::PathResolver,
) {
    if selected_thread_id != Some(thread_id) {
        return;
    }

    let snapshot = workspace
        .and_then(|projection| projection.selected_version.as_ref())
        .map(|message| crate::contracts::LastDesignSnapshot {
            design: message.output.clone(),
            thread_id: Some(thread_id.to_string()),
            message_id: Some(message.id.clone()),
            artifact_bundle: message.artifact_bundle.clone(),
            model_manifest: message.model_manifest.clone(),
            selected_part_id: None,
            target_ref: Some(crate::contracts::AuthoringTargetRef::SavedVersion {
                thread_id: thread_id.to_string(),
                message_id: message.id.clone(),
            }),
        });
    *state.last_snapshot.lock().unwrap() = snapshot.clone();
    crate::services::session::write_last_snapshot(app, snapshot.as_ref());
}

pub async fn delete_version_intent(
    input: VersionHistoryIntent,
    state: &crate::models::AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<VersionHistoryIntentResponse> {
    let (thread_id, workspace, thread_removed) = {
        let mut conn = state.db.lock().await;
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        let thread_id = db::get_message_thread_id(&transaction, &input.message_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .ok_or_else(|| AppError::not_found("Version not found."))?;
        delete_version(&transaction, &input.message_id)?;
        let thread_removed = db::get_visible_thread_title(&transaction, &thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .is_none();
        let preferred_message_id = (input.selected_thread_id.as_deref() == Some(&thread_id)
            && input.selected_message_id.as_deref() != Some(input.message_id.as_str()))
        .then_some(input.selected_message_id.as_deref())
        .flatten();
        let workspace = (!thread_removed)
            .then(|| {
                get_workspace_projection_read(
                    &transaction,
                    &thread_id,
                    preferred_message_id,
                    input.message_limit,
                )
            })
            .transpose()?;
        transaction
            .commit()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        (thread_id, workspace, thread_removed)
    };

    persist_intent_workspace_selection(
        input.selected_thread_id.as_deref(),
        &thread_id,
        workspace.as_ref(),
        state,
        app,
    );
    Ok(VersionHistoryIntentResponse {
        thread_id,
        workspace,
        thread_removed,
    })
}

pub async fn restore_version_intent(
    input: VersionHistoryIntent,
    state: &crate::models::AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<VersionHistoryIntentResponse> {
    let (thread_id, workspace) = {
        let mut conn = state.db.lock().await;
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        let thread_id = db::get_message_thread_id(&transaction, &input.message_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .ok_or_else(|| AppError::not_found("Version not found."))?;
        restore_version(&transaction, &input.message_id)?;
        let preferred_message_id = (input.selected_thread_id.as_deref() == Some(&thread_id))
            .then_some(input.message_id.as_str());
        let workspace = get_workspace_projection_read(
            &transaction,
            &thread_id,
            preferred_message_id,
            input.message_limit,
        )?;
        transaction
            .commit()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        (thread_id, workspace)
    };

    persist_intent_workspace_selection(
        input.selected_thread_id.as_deref(),
        &thread_id,
        Some(&workspace),
        state,
        app,
    );
    Ok(VersionHistoryIntentResponse {
        thread_id,
        workspace: Some(workspace),
        thread_removed: false,
    })
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadLifecycleIntent {
    pub thread_id: String,
    pub selected_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadLifecycleProjection {
    pub thread_id: String,
    pub history: Vec<Thread>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenInventoryThreadIntent {
    pub thread_id: String,
    pub message_limit: Option<usize>,
}

fn clear_restart_pointer_if_targeted(
    thread_id: &str,
    state: &crate::models::AppState,
    app: &dyn crate::models::PathResolver,
) {
    let mut last = state.last_snapshot.lock().unwrap();
    if last
        .as_ref()
        .and_then(|snapshot| snapshot.thread_id.as_deref())
        != Some(thread_id)
    {
        return;
    }
    *last = None;
    drop(last);
    crate::services::session::write_last_snapshot(app, None);
}

pub async fn delete_thread_intent(
    input: ThreadLifecycleIntent,
    state: &crate::models::AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<ThreadLifecycleProjection> {
    let history = {
        let mut conn = state.db.lock().await;
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        let changed = db::delete_thread(&transaction, &input.thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?;
        if !changed {
            return Err(AppError::not_found("Thread not found."));
        }
        let history = get_history(&transaction)?;
        transaction
            .commit()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        history
    };
    clear_restart_pointer_if_targeted(&input.thread_id, state, app);
    Ok(ThreadLifecycleProjection {
        thread_id: input.thread_id,
        history,
    })
}

pub async fn finalize_thread_intent(
    input: ThreadLifecycleIntent,
    state: &crate::models::AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<ThreadLifecycleProjection> {
    let history = {
        let mut conn = state.db.lock().await;
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        finalize_thread(
            &transaction,
            &input.thread_id,
            input.selected_message_id.as_deref(),
        )?;
        let history = get_history(&transaction)?;
        transaction
            .commit()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        history
    };
    clear_restart_pointer_if_targeted(&input.thread_id, state, app);
    Ok(ThreadLifecycleProjection {
        thread_id: input.thread_id,
        history,
    })
}

pub async fn reopen_thread_intent(
    input: ThreadLifecycleIntent,
    state: &crate::models::AppState,
) -> AppResult<ThreadLifecycleProjection> {
    let history = {
        let mut conn = state.db.lock().await;
        let transaction = conn
            .transaction()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        reopen_thread(&transaction, &input.thread_id)?;
        let history = get_history(&transaction)?;
        transaction
            .commit()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        history
    };
    Ok(ThreadLifecycleProjection {
        thread_id: input.thread_id,
        history,
    })
}

pub async fn open_inventory_thread_intent(
    input: OpenInventoryThreadIntent,
    state: &crate::models::AppState,
    app: &dyn crate::models::PathResolver,
) -> AppResult<crate::contracts::WorkspaceProjection> {
    let workspace = {
        let conn = state.db.lock().await;
        let thread = get_thread_summary(&conn, &input.thread_id)?;
        if thread.status != ThreadStatus::Finalized {
            return Err(AppError::validation("Thread is not a completed project."));
        }
        get_workspace_projection(&conn, &input.thread_id, None, input.message_limit)?
    };
    persist_intent_workspace_selection(
        Some(&input.thread_id),
        &input.thread_id,
        Some(&workspace),
        state,
        app,
    );
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::ThreadStatus;
    use crate::contracts::{
        DesignOutput, GenieTraits, InteractionMode, MacroDialect, Message, UiSpec,
    };
    use crate::db;
    use std::collections::BTreeMap;

    #[test]
    fn version_history_intent_accepts_only_camel_case_boundary_fields() {
        serde_json::from_value::<VersionHistoryIntent>(serde_json::json!({
            "messageId": "version-1",
            "selectedThreadId": "thread-1",
            "selectedMessageId": "version-1",
            "messageLimit": 20
        }))
        .expect("camelCase intent");
        assert!(
            serde_json::from_value::<VersionHistoryIntent>(serde_json::json!({
                "message_id": "version-1"
            }))
            .is_err()
        );

        serde_json::from_value::<ThreadLifecycleIntent>(serde_json::json!({
            "threadId": "thread-1",
            "selectedMessageId": "version-1"
        }))
        .expect("camelCase lifecycle intent");
        assert!(
            serde_json::from_value::<OpenInventoryThreadIntent>(serde_json::json!({
                "thread_id": "thread-1"
            }))
            .is_err()
        );
    }

    fn sample_output(version_name: &str) -> DesignOutput {
        DesignOutput {
            title: "Lamp".to_string(),
            version_name: version_name.to_string(),
            response: String::new(),
            interaction_mode: InteractionMode::Design,
            macro_code: "print('hi')".to_string(),
            macro_dialect: MacroDialect::CadFrameworkV1,
            engine_kind: crate::contracts::EngineKind::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            ui_spec: UiSpec { fields: Vec::new() },
            initial_params: BTreeMap::new(),
            post_processing: None,
        }
    }

    fn sample_artifact_bundle(model_id: &str) -> crate::contracts::ArtifactBundle {
        crate::contracts::ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: model_id.to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            content_hash: format!("hash-{model_id}"),
            artifact_version: 1,
            fcstd_path: format!("/tmp/{model_id}.FCStd"),
            manifest_path: format!("/tmp/{model_id}.json"),
            macro_path: None,
            model_stl_path: format!("/tmp/{model_id}.stl"),
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
            structural_verification: None,
            agent_origin: None,
            timestamp,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        }
    }

    #[test]
    fn finalize_thread_keeps_project_identity_and_full_history_in_completed() {
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

        let inventory_threads = db::get_inventory_threads(&conn).unwrap();
        assert_eq!(inventory_threads.len(), 1);
        let finalized = &inventory_threads[0];
        assert_eq!(finalized.id, thread_id);
        assert_eq!(finalized.title, "Bulb Lamp Shade");
        assert_eq!(finalized.status, ThreadStatus::Finalized);
        assert_eq!(finalized.version_count, 2);

        let loaded = get_thread(&conn, &finalized.id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
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
        assert!(deleted_at.is_none());

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
                structural_verification: None,
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
                structural_verification: None,
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
                structural_verification: None,
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
    fn history_marks_only_completely_empty_thread_as_reusable_blank() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-blank-classification-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();
        db::create_or_update_thread(&conn, "blank", "Untitled design", 200, None).unwrap();
        db::create_or_update_thread(&conn, "touched-empty", "Untitled design", 175, None).unwrap();
        db::create_or_update_thread(&conn, "renamed-blank", "Untitled design", 150, None).unwrap();
        db::create_or_update_thread(&conn, "provider-only", "Untitled design", 125, None).unwrap();
        db::update_thread_title(&conn, "renamed-blank", "Hydrant cap").unwrap();
        conn.execute(
            "UPDATE threads SET updated_at = updated_at + 1 WHERE id = 'touched-empty'",
            [],
        )
        .unwrap();

        let root = std::env::temp_dir().join(format!(
            "ecky-thread-blank-bindings-{}",
            uuid::Uuid::new_v4()
        ));
        crate::thread_source_binding::upsert_binding_row(
            &conn,
            "blank",
            &root.join("blank"),
            &crate::thread_source_binding::source_digest(""),
            None,
        )
        .unwrap();
        crate::thread_source_binding::upsert_binding_row(
            &conn,
            "touched-empty",
            &root.join("touched-empty"),
            &crate::thread_source_binding::source_digest(""),
            None,
        )
        .unwrap();
        crate::thread_source_binding::upsert_binding_row(
            &conn,
            "renamed-blank",
            &root.join("renamed-blank"),
            &crate::thread_source_binding::source_digest(""),
            None,
        )
        .unwrap();
        crate::thread_source_binding::upsert_binding_row(
            &conn,
            "provider-only",
            &root.join("provider-only"),
            &crate::thread_source_binding::source_digest(""),
            None,
        )
        .unwrap();
        crate::services::codex_takeover::bind_owned_thread(
            &conn,
            "provider-only",
            "provider-thread",
            "Untitled design",
            root.join("provider-only").to_string_lossy().as_ref(),
            125,
        )
        .unwrap();
        crate::services::codex_takeover::persist_provider_turn_user_input(
            &conn,
            "provider-only",
            crate::services::codex_takeover::CODEX_PROVIDER_ID,
            "provider-thread",
            "turn-1",
            "Draw an engine concept",
            &[],
            126,
        )
        .unwrap();

        let threads = get_history(&conn).unwrap();
        assert!(
            threads
                .iter()
                .find(|thread| thread.id == "blank")
                .unwrap()
                .is_blank
        );
        assert!(
            threads
                .iter()
                .find(|thread| thread.id == "touched-empty")
                .unwrap()
                .is_blank
        );
        assert!(
            !threads
                .iter()
                .find(|thread| thread.id == "renamed-blank")
                .unwrap()
                .is_blank
        );
        let provider_only = threads
            .iter()
            .find(|thread| thread.id == "provider-only")
            .unwrap();
        assert!(!provider_only.is_blank);
        assert_eq!(provider_only.title, "Draw an engine concept");
        assert!(provider_only.updated_at > 125);

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_dir_all(root);
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
    fn get_thread_counts_output_only_messages_as_authored_versions() {
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
        assert_eq!(thread.version_count, 2);

        let latest = get_thread_latest_version(&conn, "thread-output-only")
            .unwrap()
            .expect("latest");
        assert_eq!(latest.id, rendered.id);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_latest_version_returns_newest_authored_version() {
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

        assert_eq!(latest.id, "msg-failed");
        assert_eq!(
            latest
                .output
                .as_ref()
                .map(|output| output.version_name.as_str()),
            Some("V-failed")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_latest_version_returns_newest_authored_version_even_when_failed() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-latest-failed-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).expect("schema");
        db::create_or_update_thread(&conn, "thread-latest-failed", "Latest", 100, None)
            .expect("thread");

        let older = sample_message("msg-old-success", 100, "V-old");
        let mut failed = sample_message("msg-new-failed", 200, "V-failed");
        failed.status = MessageStatus::Error;
        failed.artifact_bundle = None;
        db::add_message(&conn, "thread-latest-failed", &older).expect("old");
        db::add_message(&conn, "thread-latest-failed", &failed).expect("failed");

        let latest = get_thread_latest_version(&conn, "thread-latest-failed")
            .expect("latest query")
            .expect("latest version");
        assert_eq!(latest.id, "msg-new-failed");
        assert_eq!(latest.status, MessageStatus::Error);
        let pointed = get_thread_message_version(&conn, "thread-latest-failed", "msg-new-failed")
            .expect("pointed query")
            .expect("pointed version");
        assert_eq!(pointed.id, "msg-new-failed");
        drop(conn);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_thread_counts_failed_artifactless_authored_versions() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-count-failed-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).expect("schema");
        db::create_or_update_thread(&conn, "thread-count-failed", "Count", 100, None)
            .expect("thread");

        let mut failed = sample_message("msg-count-failed", 100, "V-failed");
        failed.status = MessageStatus::Error;
        failed.artifact_bundle = None;
        db::add_message(&conn, "thread-count-failed", &failed).expect("failed");

        let thread = get_thread(&conn, "thread-count-failed").expect("thread query");
        assert_eq!(thread.version_count, 1);
        drop(conn);
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
            let bundle = message.artifact_bundle.as_mut().unwrap();
            bundle
                .edge_targets
                .push(crate::contracts::ViewerEdgeTarget {
                    target_id: format!("edge-{index}"),
                    durable_target_id: None,
                    canonical_target_id: None,
                    alias_ids: Vec::new(),
                    part_id: "body".to_string(),
                    viewer_node_id: "body".to_string(),
                    label: "Edge".to_string(),
                    editable: false,
                    start: crate::contracts::ViewerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    end: crate::contracts::ViewerEdgePoint {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                });
            bundle
                .face_targets
                .push(crate::contracts::ViewerFaceTarget {
                    target_id: format!("face-{index}"),
                    durable_target_id: None,
                    canonical_target_id: None,
                    alias_ids: Vec::new(),
                    part_id: "body".to_string(),
                    viewer_node_id: "body".to_string(),
                    label: "Face".to_string(),
                    editable: false,
                    center: crate::contracts::ViewerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    normal: Some([0.0, 0.0, 1.0]),
                    area: Some(1.0),
                });
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
        assert!(first_page
            .next_before
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("v2:")));
        assert!(first_page.messages.iter().all(|message| message.has_image));
        assert!(first_page
            .messages
            .iter()
            .all(|message| message.attachment_count == 1));
        assert!(first_page.messages.iter().all(|message| message
            .version_summary
            .as_ref()
            .is_some_and(|version| {
                version.has_output && version.has_runtime && !version.has_manifest
            })));
        assert!(first_page.observed_bytes < 1_048_576);

        let full_page_error =
            get_thread_messages_page(&conn, "thread-page", None, Some(2), true).unwrap_err();
        assert!(full_page_error
            .to_string()
            .contains("Hydrate one selected version"));

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

    #[test]
    fn get_thread_messages_page_cursor_preserves_equal_timestamp_order() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-message-cursor-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();
        db::create_or_update_thread(&conn, "thread-cursor", "Cursor", 100, None).unwrap();
        for index in 1..=4 {
            db::add_message(
                &conn,
                "thread-cursor",
                &sample_message(&format!("same-{index}"), 100, &format!("V{index}")),
            )
            .unwrap();
        }

        let first = get_thread_messages_page(&conn, "thread-cursor", None, Some(2), false).unwrap();
        let second = get_thread_messages_page(
            &conn,
            "thread-cursor",
            first.next_before.clone(),
            Some(2),
            false,
        )
        .unwrap();
        let ids = first
            .messages
            .iter()
            .chain(second.messages.iter())
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 4);
        assert_eq!(
            ids.iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
        assert!(!second.has_more);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn filtered_timeline_applies_role_before_limit_and_rejects_cross_thread_cursor() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-thread-filtered-page-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();
        for thread_id in ["thread-a", "thread-b"] {
            db::create_or_update_thread(&conn, thread_id, thread_id, 100, None).unwrap();
        }
        db::add_message(
            &conn,
            "thread-a",
            &Message {
                id: "user-old".into(),
                role: MessageRole::User,
                content: "user".into(),
                status: MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
                timestamp: 100,
            },
        )
        .unwrap();
        db::add_message(
            &conn,
            "thread-a",
            &sample_message("assistant-new", 200, "V2"),
        )
        .unwrap();
        let page = get_thread_messages_page_filtered(
            &conn,
            "thread-a",
            None,
            Some(1),
            Some(&[MessageRole::User]),
        )
        .unwrap();
        assert_eq!(page.messages[0].id, "user-old");

        db::add_message(&conn, "thread-b", &sample_message("other", 200, "V1")).unwrap();
        let cursor = get_thread_messages_page(&conn, "thread-a", None, Some(1), false)
            .unwrap()
            .next_before;
        let error = get_thread_messages_page(&conn, "thread-b", cursor, Some(1), false)
            .expect_err("cursor is bound to thread");
        assert!(error.to_string().contains("Invalid thread timeline cursor"));
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn selected_version_core_excludes_dense_targets_and_pages_them_by_reference() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-version-detail-topology-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = db::init_db(&db_path).unwrap();
        db::create_or_update_thread(&conn, "thread-detail", "Detail", 100, None).unwrap();
        let mut message = sample_message("version-detail", 100, "V1");
        for index in 0..3 {
            message.artifact_bundle.as_mut().unwrap().edge_targets.push(
                crate::contracts::ViewerEdgeTarget {
                    target_id: format!("edge-{index}"),
                    durable_target_id: None,
                    canonical_target_id: None,
                    alias_ids: Vec::new(),
                    part_id: "body".into(),
                    viewer_node_id: "body".into(),
                    label: "Edge".into(),
                    editable: false,
                    start: crate::contracts::ViewerEdgePoint {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    end: crate::contracts::ViewerEdgePoint {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                },
            );
        }
        db::add_message(&conn, "thread-detail", &message).unwrap();

        let detail = get_version_detail(&conn, "thread-detail", "version-detail").unwrap();
        assert_eq!(detail.edge_count, 3);
        assert_eq!(
            detail.message.artifact_bundle.unwrap().edge_targets.len(),
            0
        );
        assert!(detail.dense_topology_ref.is_some());
        assert!(detail.observed_bytes < crate::transport_budget::VERSION_CORE_MAX_BYTES);

        let first = get_dense_topology_page(
            &conn,
            "thread-detail",
            "version-detail",
            crate::contracts::DenseTopologyKind::Edge,
            None,
            Some(2),
        )
        .unwrap();
        assert_eq!(first.items.len(), 2);
        assert_eq!(first.total_count, 3);
        assert!(first.next_cursor.is_some());
        let second = get_dense_topology_page(
            &conn,
            "thread-detail",
            "version-detail",
            crate::contracts::DenseTopologyKind::Edge,
            first.next_cursor,
            Some(2),
        )
        .unwrap();
        assert_eq!(second.items.len(), 1);
        assert!(second.next_cursor.is_none());

        let source =
            get_version_source_window(&conn, "thread-detail", "version-detail", 0, 8).unwrap();
        assert!(source.content.len() <= 8);
        assert_eq!(source.total_bytes, message.output.unwrap().macro_code.len());
        let _ = std::fs::remove_file(db_path);
    }
}
