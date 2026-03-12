use crate::db;
use crate::models::{AppError, AppResult, MessageRole, MessageStatus, Thread, ThreadStatus};
use crate::persist_thread_summary;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn get_history(conn: &rusqlite::Connection) -> AppResult<Vec<Thread>> {
    db::get_all_threads(conn).map_err(|err: rusqlite::Error| AppError::persistence(err.to_string()))
}

pub fn get_thread(conn: &rusqlite::Connection, id: &str) -> AppResult<Thread> {
    let title = db::get_thread_title(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .ok_or_else(|| AppError::not_found("Thread not found."))?;
    let summary = db::get_thread_summary(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .unwrap_or_default();
    let messages =
        db::get_thread_messages(conn, id).map_err(|err| AppError::persistence(err.to_string()))?;

    let genie_traits = db::get_thread_genie_traits(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let updated_at = messages.last().map(|m| m.timestamp).unwrap_or(0);
    let version_count = messages
        .iter()
        .filter(|m| {
            m.role == MessageRole::Assistant && (m.output.is_some() || m.artifact_bundle.is_some())
        })
        .count();
    let pending_count = messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant && m.status == MessageStatus::Pending)
        .count();
    let error_count = messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant && m.status == MessageStatus::Error)
        .count();

    let lifecycle = db::get_thread_lifecycle(conn, id)
        .map_err(|err| AppError::persistence(err.to_string()))?
        .unwrap_or_else(|| db::ThreadLifecycle {
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
        error_count,
        status: lifecycle.status,
        finalized_at: lifecycle.finalized_at,
        pending_confirm: lifecycle.pending_confirm,
    })
}

pub fn finalize_thread(conn: &rusqlite::Connection, thread_id: &str) -> AppResult<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let changed = db::finalize_thread(conn, thread_id, now)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    if changed {
        Ok(())
    } else {
        Err(AppError::not_found("Thread not found."))
    }
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
