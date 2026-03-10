use crate::models::{
    upgraded_or_default_genie_traits, ArtifactBundle, DeletedMessage, DesignOutput, DesignParams,
    GenieTraits, Message, MessageRole, MessageStatus, ModelManifest, Thread, ThreadReference,
    UiSpec,
};
use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};

#[derive(Debug, Clone)]
struct ThreadMessageRow {
    message: Message,
    deleted_at: Option<i64>,
}

pub fn init_db(db_path: &std::path::Path) -> SqlResult<Connection> {
    let conn = Connection::open(db_path)?;

    // Enable WAL mode for better concurrency and prevent "database is locked" errors
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS threads (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL,
            genie_traits TEXT,
            deleted_at INTEGER
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'success',
            output TEXT,
            usage TEXT,
            artifact_bundle TEXT,
            model_manifest TEXT,
            timestamp INTEGER NOT NULL,
            image_data TEXT,
            attachment_images TEXT,
            deleted_at INTEGER,
            trash_hidden_at INTEGER,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thread_references (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            source_message_id TEXT,
            ordinal INTEGER NOT NULL DEFAULT 0,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            summary TEXT NOT NULL DEFAULT '',
            pinned INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_thread_references_source_ordinal_kind
         ON thread_references(source_message_id, ordinal, kind)",
        [],
    )?;

    // Migrations for existing databases
    let _ = conn.execute(
        "ALTER TABLE threads ADD COLUMN summary TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN genie_traits TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN image_data TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN attachment_images TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN usage TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN artifact_bundle TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN model_manifest TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN status TEXT NOT NULL DEFAULT 'success'",
        [],
    );
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN deleted_at INTEGER", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN deleted_at INTEGER", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN trash_hidden_at INTEGER", []);
    migrate_thread_genie_traits(&conn)?;

    Ok(conn)
}

fn deserialize_thread_genie_traits(thread_id: &str, raw: Option<&str>) -> GenieTraits {
    upgraded_or_default_genie_traits(thread_id, raw)
}

fn migrate_thread_genie_traits(conn: &Connection) -> SqlResult<()> {
    let mut stmt = conn.prepare("SELECT id, genie_traits FROM threads")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for (thread_id, raw) in rows {
        let traits = deserialize_thread_genie_traits(&thread_id, raw.as_deref());
        let traits_json = serde_json::to_string(&traits).unwrap_or_default();
        conn.execute(
            "UPDATE threads SET genie_traits = ?1 WHERE id = ?2",
            params![traits_json, thread_id],
        )?;
    }

    Ok(())
}

pub fn get_all_threads(conn: &Connection) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare("
        SELECT id, title, summary, updated_at, genie_traits,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND (output IS NOT NULL OR artifact_bundle IS NOT NULL) AND deleted_at IS NULL) as v_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'pending' AND deleted_at IS NULL) as p_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'error' AND deleted_at IS NULL) as e_count
        FROM threads 
        WHERE deleted_at IS NULL
        ORDER BY updated_at DESC
    ")?;
    let thread_iter = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let traits_str: Option<String> = row.get(4)?;
        Ok(Thread {
            id: id.clone(),
            title: row.get(1)?,
            summary: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
            messages: vec![], // Messages are now lazy-loaded
            genie_traits: Some(deserialize_thread_genie_traits(&id, traits_str.as_deref())),
            version_count: row.get::<_, i64>(5)? as usize,
            pending_count: row.get::<_, i64>(6)? as usize,
            error_count: row.get::<_, i64>(7)? as usize,
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        threads.push(thread?);
    }
    Ok(threads)
}

pub fn create_or_update_thread(
    conn: &Connection,
    thread_id: &str,
    title: &str,
    updated_at: u64,
    genie_traits: Option<&GenieTraits>,
) -> SqlResult<()> {
    let traits_str = genie_traits.and_then(|t| serde_json::to_string(t).ok());
    conn.execute(
        "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET title=excluded.title, updated_at=excluded.updated_at, genie_traits=COALESCE(excluded.genie_traits, threads.genie_traits)",
        params![thread_id, title, updated_at as i64, traits_str],
    )?;
    Ok(())
}

pub fn get_thread_genie_traits(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<GenieTraits>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT genie_traits FROM threads WHERE id = ?1",
            [thread_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    Ok(Some(deserialize_thread_genie_traits(
        thread_id,
        raw.as_deref(),
    )))
}

pub fn update_thread_summary(conn: &Connection, thread_id: &str, summary: &str) -> SqlResult<()> {
    conn.execute(
        "UPDATE threads SET summary = ?1 WHERE id = ?2",
        params![summary, thread_id],
    )?;
    Ok(())
}

pub fn get_thread_title(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT title FROM threads WHERE id = ?1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_thread_summary(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT summary FROM threads WHERE id = ?1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn add_message(conn: &Connection, thread_id: &str, msg: &Message) -> SqlResult<()> {
    let output_str = msg
        .output
        .as_ref()
        .and_then(|o| serde_json::to_string(o).ok());
    let usage_str = msg
        .usage
        .as_ref()
        .and_then(|usage| serde_json::to_string(usage).ok());
    let artifact_bundle_str = msg
        .artifact_bundle
        .as_ref()
        .and_then(|bundle| serde_json::to_string(bundle).ok());
    let model_manifest_str = msg
        .model_manifest
        .as_ref()
        .and_then(|manifest| serde_json::to_string(manifest).ok());
    let attachment_images_str = if msg.attachment_images.is_empty() {
        None
    } else {
        serde_json::to_string(&msg.attachment_images).ok()
    };
    conn.execute(
        "INSERT INTO messages (id, thread_id, role, content, status, output, usage, artifact_bundle, model_manifest, timestamp, image_data, attachment_images) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            msg.id,
            thread_id,
            msg.role,
            msg.content,
            msg.status,
            output_str,
            usage_str,
            artifact_bundle_str,
            model_manifest_str,
            msg.timestamp as i64,
            msg.image_data,
            attachment_images_str,
        ],
    )?;
    Ok(())
}

pub fn add_thread_reference(conn: &Connection, reference: &ThreadReference) -> SqlResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO thread_references
         (id, thread_id, source_message_id, ordinal, kind, name, content, summary, pinned, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            reference.id,
            reference.thread_id,
            reference.source_message_id,
            reference.ordinal,
            reference.kind,
            reference.name,
            reference.content,
            reference.summary,
            if reference.pinned { 1 } else { 0 },
            reference.created_at as i64
        ],
    )?;
    Ok(())
}

pub fn get_thread_messages(conn: &Connection, thread_id: &str) -> SqlResult<Vec<Message>> {
    Ok(load_thread_message_rows(conn, thread_id, false)?
        .into_iter()
        .filter(|row| row.deleted_at.is_none() && row.message.status != MessageStatus::Discarded)
        .map(|row| row.message)
        .collect())
}

pub fn get_thread_messages_for_context(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Vec<Message>> {
    let rows = load_thread_message_rows(conn, thread_id, true)?;
    let mut messages = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        if row.deleted_at.is_some() || row.message.status == MessageStatus::Discarded {
            continue;
        }

        let skip_deleted_version_prompt = row.message.role == MessageRole::User
            && rows
                .get(index + 1)
                .map(|next| {
                    next.deleted_at.is_some()
                        && next.message.role == MessageRole::Assistant
                        && (next.message.output.is_some() || next.message.artifact_bundle.is_some())
                        && next.message.status != MessageStatus::Discarded
                        && next.message.timestamp.saturating_sub(row.message.timestamp) <= 2
                })
                .unwrap_or(false);

        if skip_deleted_version_prompt {
            continue;
        }

        messages.push(row.message.clone());
    }

    Ok(messages)
}

fn load_thread_message_rows(
    conn: &Connection,
    thread_id: &str,
    include_deleted: bool,
) -> SqlResult<Vec<ThreadMessageRow>> {
    let sql = if include_deleted {
        "SELECT id, role, content, status, output, usage, artifact_bundle, model_manifest, timestamp, image_data, attachment_images, deleted_at
         FROM messages
         WHERE thread_id = ?1 AND status != 'discarded'
         ORDER BY timestamp ASC"
    } else {
        "SELECT id, role, content, status, output, usage, artifact_bundle, model_manifest, timestamp, image_data, attachment_images, deleted_at
         FROM messages
         WHERE thread_id = ?1 AND status != 'discarded' AND deleted_at IS NULL
         ORDER BY timestamp ASC"
    };

    let mut stmt = conn.prepare(sql)?;
    let msg_iter = stmt.query_map([thread_id], |row| {
        let output_str: Option<String> = row.get(4)?;
        let output: Option<DesignOutput> = output_str.and_then(|s| serde_json::from_str(&s).ok());
        let usage_str: Option<String> = row.get(5)?;
        let usage = usage_str.and_then(|s| serde_json::from_str(&s).ok());
        let artifact_bundle_str: Option<String> = row.get(6)?;
        let artifact_bundle = artifact_bundle_str.and_then(|s| serde_json::from_str(&s).ok());
        let model_manifest_str: Option<String> = row.get(7)?;
        let model_manifest = model_manifest_str.and_then(|s| serde_json::from_str(&s).ok());
        let attachment_images_str: Option<String> = row.get(10)?;
        let attachment_images = attachment_images_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Ok(ThreadMessageRow {
            message: Message {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                status: row.get(3)?,
                output,
                usage,
                artifact_bundle,
                model_manifest,
                timestamp: row.get::<_, i64>(8)? as u64,
                image_data: row.get(9)?,
                attachment_images,
            },
            deleted_at: row.get(11)?,
        })
    })?;

    let mut messages = Vec::new();
    for msg in msg_iter {
        messages.push(msg?);
    }
    Ok(messages)
}

pub fn get_thread_references(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Vec<ThreadReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, source_message_id, ordinal, kind, name, content, summary, pinned, created_at
         FROM thread_references
         WHERE thread_id = ?1 AND pinned = 1
         ORDER BY created_at ASC, ordinal ASC"
    )?;
    let iter = stmt.query_map([thread_id], |row| {
        Ok(ThreadReference {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            source_message_id: row.get(2)?,
            ordinal: row.get(3)?,
            kind: row.get(4)?,
            name: row.get(5)?,
            content: row.get(6)?,
            summary: row.get(7)?,
            pinned: row.get::<_, i64>(8)? != 0,
            created_at: row.get::<_, i64>(9)? as u64,
        })
    })?;
    let mut refs = Vec::new();
    for item in iter {
        refs.push(item?);
    }
    Ok(refs)
}

pub fn clear_history(conn: &Connection) -> SqlResult<()> {
    conn.execute("DELETE FROM threads", [])?;
    Ok(())
}

pub fn update_message_status_and_output(
    conn: &Connection,
    message_id: &str,
    status: &MessageStatus,
    output: Option<&DesignOutput>,
    usage: Option<&crate::models::UsageSummary>,
    artifact_bundle: Option<&crate::models::ArtifactBundle>,
    model_manifest: Option<&crate::models::ModelManifest>,
    content: Option<&str>,
) -> SqlResult<()> {
    let output_str = output.and_then(|o| serde_json::to_string(o).ok());
    let usage_str = usage.and_then(|value| serde_json::to_string(value).ok());
    let artifact_bundle_str = artifact_bundle.and_then(|value| serde_json::to_string(value).ok());
    let model_manifest_str = model_manifest.and_then(|value| serde_json::to_string(value).ok());
    if let Some(text) = content {
        conn.execute(
            "UPDATE messages SET status = ?1, output = ?2, usage = ?3, artifact_bundle = ?4, model_manifest = ?5, content = ?6 WHERE id = ?7",
            params![
                status,
                output_str,
                usage_str,
                artifact_bundle_str,
                model_manifest_str,
                text,
                message_id
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE messages SET status = ?1, output = ?2, usage = ?3, artifact_bundle = ?4, model_manifest = ?5 WHERE id = ?6",
            params![
                status,
                output_str,
                usage_str,
                artifact_bundle_str,
                model_manifest_str,
                message_id
            ],
        )?;
    }
    Ok(())
}

pub fn delete_thread(conn: &Connection, id: &str) -> SqlResult<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute(
        "UPDATE threads SET deleted_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn delete_message(conn: &Connection, id: &str) -> SqlResult<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute(
        "UPDATE messages SET deleted_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn restore_message(conn: &Connection, id: &str) -> SqlResult<()> {
    conn.execute("UPDATE messages SET deleted_at = NULL WHERE id = ?", [id])?;
    Ok(())
}

fn unix_now_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[derive(Debug, Clone)]
struct MessageContextInfo {
    thread_id: String,
}

fn get_message_context_info(conn: &Connection, id: &str) -> SqlResult<Option<MessageContextInfo>> {
    conn.query_row(
        "SELECT thread_id
         FROM messages
         WHERE id = ?1",
        [id],
        |row| {
            Ok(MessageContextInfo {
                thread_id: row.get(0)?,
            })
        },
    )
    .optional()
}

fn set_thread_deleted_at(
    conn: &Connection,
    thread_id: &str,
    deleted_at: Option<i64>,
) -> SqlResult<()> {
    conn.execute(
        "UPDATE threads SET deleted_at = ?1 WHERE id = ?2",
        params![deleted_at, thread_id],
    )?;
    Ok(())
}

pub fn has_visible_messages(conn: &Connection, thread_id: &str) -> SqlResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE thread_id = ?1 AND status != 'discarded' AND deleted_at IS NULL",
        [thread_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn delete_version_cluster(conn: &Connection, id: &str) -> SqlResult<Option<String>> {
    let Some(message) = get_message_context_info(conn, id)? else {
        return Ok(None);
    };
    let now = unix_now_i64();

    conn.execute(
        "UPDATE messages SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![now, id],
    )?;

    if !has_visible_messages(conn, &message.thread_id)? {
        set_thread_deleted_at(conn, &message.thread_id, Some(now))?;
        update_thread_summary(conn, &message.thread_id, "")?;
    }

    Ok(Some(message.thread_id))
}

pub fn restore_version_cluster(conn: &Connection, id: &str) -> SqlResult<Option<String>> {
    let Some(message) = get_message_context_info(conn, id)? else {
        return Ok(None);
    };

    conn.execute(
        "UPDATE messages SET deleted_at = NULL, trash_hidden_at = NULL WHERE id = ?1",
        [id],
    )?;

    set_thread_deleted_at(conn, &message.thread_id, None)?;
    Ok(Some(message.thread_id))
}

pub fn get_deleted_messages(conn: &Connection) -> SqlResult<Vec<DeletedMessage>> {
    let mut stmt = conn.prepare("
        SELECT m.id, m.thread_id, t.title as thread_title, m.role, m.content, m.output, m.usage, m.artifact_bundle, m.model_manifest, m.timestamp, m.image_data, m.attachment_images, m.deleted_at
        FROM messages m
        JOIN threads t ON m.thread_id = t.id
        WHERE m.deleted_at IS NOT NULL
          AND m.trash_hidden_at IS NULL
          AND m.role = 'assistant'
          AND (m.output IS NOT NULL OR m.artifact_bundle IS NOT NULL)
        ORDER BY m.deleted_at DESC
    ")?;
    let iter = stmt.query_map([], |row| {
        let output_str: Option<String> = row.get(5)?;
        let output: Option<DesignOutput> = if let Some(json_str) = output_str {
            serde_json::from_str(&json_str).ok()
        } else {
            None
        };
        let usage_str: Option<String> = row.get(6)?;
        let usage = usage_str.and_then(|json_str| serde_json::from_str(&json_str).ok());
        let artifact_bundle_str: Option<String> = row.get(7)?;
        let artifact_bundle =
            artifact_bundle_str.and_then(|json_str| serde_json::from_str(&json_str).ok());
        let model_manifest_str: Option<String> = row.get(8)?;
        let model_manifest =
            model_manifest_str.and_then(|json_str| serde_json::from_str(&json_str).ok());
        let attachment_images_str: Option<String> = row.get(11)?;
        let attachment_images = attachment_images_str
            .and_then(|json_str| serde_json::from_str(&json_str).ok())
            .unwrap_or_default();

        Ok(DeletedMessage {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            thread_title: row.get(2)?,
            role: row.get(3)?,
            content: row.get(4)?,
            output,
            usage,
            artifact_bundle,
            model_manifest,
            timestamp: row.get::<_, i64>(9)? as u64,
            image_data: row.get(10)?,
            attachment_images,
            deleted_at: row.get::<_, i64>(12)? as u64,
        })
    })?;

    let mut results = Vec::new();
    for item in iter {
        results.push(item?);
    }
    Ok(results)
}

pub fn hide_deleted_message(conn: &Connection, id: &str) -> SqlResult<bool> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let changed = conn.execute(
        "UPDATE messages
         SET trash_hidden_at = ?1
         WHERE id = ?2
           AND deleted_at IS NOT NULL
           AND trash_hidden_at IS NULL",
        params![now, id],
    )?;
    Ok(changed > 0)
}

pub fn update_message_ui_spec(
    conn: &Connection,
    message_id: &str,
    ui_spec: &UiSpec,
) -> SqlResult<()> {
    let output_str: Option<String> = conn.query_row(
        "SELECT output FROM messages WHERE id = ?1",
        [message_id],
        |row| row.get(0),
    )?;

    if let Some(json_str) = output_str {
        let mut output: DesignOutput = serde_json::from_str(&json_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        output.ui_spec = ui_spec.clone();
        let updated = serde_json::to_string(&output)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        conn.execute(
            "UPDATE messages SET output = ?1 WHERE id = ?2",
            params![updated, message_id],
        )?;
    }
    Ok(())
}

pub fn update_message_parameters(
    conn: &Connection,
    message_id: &str,
    parameters: &DesignParams,
) -> SqlResult<()> {
    let output_str: Option<String> = conn.query_row(
        "SELECT output FROM messages WHERE id = ?1",
        [message_id],
        |row| row.get(0),
    )?;

    if let Some(json_str) = output_str {
        let mut output: DesignOutput = serde_json::from_str(&json_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        output.initial_params = parameters.clone();
        let updated = serde_json::to_string(&output)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        conn.execute(
            "UPDATE messages SET output = ?1 WHERE id = ?2",
            params![updated, message_id],
        )?;
    }
    Ok(())
}

pub fn update_message_model_manifest(
    conn: &Connection,
    message_id: &str,
    manifest: &ModelManifest,
) -> SqlResult<()> {
    let serialized = serde_json::to_string(manifest)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "UPDATE messages SET model_manifest = ?1 WHERE id = ?2",
        params![serialized, message_id],
    )?;
    Ok(())
}

pub fn update_message_artifact_bundle(
    conn: &Connection,
    message_id: &str,
    bundle: &ArtifactBundle,
) -> SqlResult<()> {
    let serialized = serde_json::to_string(bundle)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "UPDATE messages SET artifact_bundle = ?1 WHERE id = ?2",
        params![serialized, message_id],
    )?;
    Ok(())
}

pub fn update_message_output(
    conn: &Connection,
    message_id: &str,
    output: &DesignOutput,
) -> SqlResult<()> {
    let serialized = serde_json::to_string(output)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "UPDATE messages SET output = ?1 WHERE id = ?2",
        params![serialized, message_id],
    )?;
    Ok(())
}

pub fn get_message_output_and_thread(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Option<(DesignOutput, String)>> {
    let row: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT output, thread_id FROM messages WHERE id = ?1",
            [message_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let Some((output_str, thread_id)) = row else {
        return Ok(None);
    };

    let Some(json_str) = output_str else {
        return Ok(None);
    };

    let Ok(output) = serde_json::from_str::<DesignOutput>(&json_str) else {
        return Ok(None);
    };

    Ok(Some((output, thread_id)))
}

pub fn get_message_runtime_and_thread(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Option<(Option<ArtifactBundle>, Option<ModelManifest>, String)>> {
    let row: Option<(Option<String>, Option<String>, String)> = conn
        .query_row(
            "SELECT artifact_bundle, model_manifest, thread_id FROM messages WHERE id = ?1",
            [message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;

    let Some((artifact_bundle_str, model_manifest_str, thread_id)) = row else {
        return Ok(None);
    };

    let artifact_bundle =
        artifact_bundle_str.and_then(|json_str| serde_json::from_str(&json_str).ok());
    let model_manifest =
        model_manifest_str.and_then(|json_str| serde_json::from_str(&json_str).ok());

    Ok(Some((artifact_bundle, model_manifest, thread_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        DesignParams, InteractionMode, MessageRole, MessageStatus, ParamValue, UiField, UiSpec,
    };

    fn init_db_internal(conn: &Connection) -> SqlResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                summary TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL,
                genie_traits TEXT,
                deleted_at INTEGER
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'success',
                output TEXT,
                usage TEXT,
                artifact_bundle TEXT,
                model_manifest TEXT,
                timestamp INTEGER NOT NULL,
                image_data TEXT,
                attachment_images TEXT,
                deleted_at INTEGER,
                trash_hidden_at INTEGER,
                FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS thread_references (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                source_message_id TEXT,
                ordinal INTEGER NOT NULL DEFAULT 0,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                summary TEXT NOT NULL DEFAULT '',
                pinned INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )",
            [],
        )?;
        Ok(())
    }

    fn sample_output() -> DesignOutput {
        DesignOutput {
            title: "Test".to_string(),
            version_name: "V1".to_string(),
            response: "".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: "print('hi')".to_string(),
            ui_spec: UiSpec { fields: Vec::new() },
            initial_params: DesignParams::from([("x".to_string(), ParamValue::Number(10.0))]),
        }
    }

    #[test]
    fn test_update_ui_spec_and_params() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "test-thread";
        let msg_id = "test-msg";
        let now = 123456789;

        create_or_update_thread(&conn, thread_id, "Test Thread", now, None).unwrap();

        let msg = Message {
            id: msg_id.to_string(),
            role: MessageRole::Assistant,
            content: "Hello".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            timestamp: now,
            image_data: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &msg).unwrap();

        // Update UI Spec
        let new_spec = UiSpec {
            fields: vec![UiField::Number {
                key: "y".to_string(),
                label: "Y".to_string(),
                min: None,
                max: None,
                step: None,
                min_from: None,
                max_from: None,
                frozen: false,
            }],
        };
        update_message_ui_spec(&conn, msg_id, &new_spec).unwrap();

        // Update Params
        let new_params = DesignParams::from([
            ("x".to_string(), ParamValue::Number(20.0)),
            ("y".to_string(), ParamValue::Number(5.0)),
        ]);
        update_message_parameters(&conn, msg_id, &new_params).unwrap();

        // Verify
        let (output, tid) = get_message_output_and_thread(&conn, msg_id)
            .unwrap()
            .unwrap();
        assert_eq!(tid, thread_id);
        assert_eq!(output.ui_spec, new_spec);
        assert_eq!(output.initial_params, new_params);
    }

    #[test]
    fn test_delete_version_keeps_prompt_visible_and_only_surfaces_deleted_models() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-1";
        create_or_update_thread(&conn, thread_id, "Thread", 100, None).unwrap();

        let user_msg = Message {
            id: "user-1".to_string(),
            role: MessageRole::User,
            content: "Make a box".to_string(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            timestamp: 100,
            image_data: None,
            attachment_images: Vec::new(),
        };
        let assistant_msg = Message {
            id: "assistant-1".to_string(),
            role: MessageRole::Assistant,
            content: "Box created".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            timestamp: 101,
            image_data: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &user_msg).unwrap();
        add_message(&conn, thread_id, &assistant_msg).unwrap();
        add_thread_reference(
            &conn,
            &ThreadReference {
                id: "ref-1".to_string(),
                thread_id: thread_id.to_string(),
                source_message_id: Some(user_msg.id.clone()),
                ordinal: 0,
                kind: "python_macro".to_string(),
                name: "prompt_macro_1".to_string(),
                content: "box()".to_string(),
                summary: "Prompt macro".to_string(),
                pinned: true,
                created_at: 100,
            },
        )
        .unwrap();

        delete_version_cluster(&conn, &assistant_msg.id).unwrap();

        let visible_messages = get_thread_messages(&conn, thread_id).unwrap();
        assert_eq!(visible_messages.len(), 1);
        assert_eq!(visible_messages[0].id, user_msg.id);
        assert!(has_visible_messages(&conn, thread_id).unwrap());

        let context_messages = get_thread_messages_for_context(&conn, thread_id).unwrap();
        assert!(context_messages.is_empty());

        let deleted = get_deleted_messages(&conn).unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].id, assistant_msg.id);

        let refs = get_thread_references(&conn, thread_id).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0].source_message_id.as_deref(),
            Some(user_msg.id.as_str())
        );
    }

    #[test]
    fn test_delete_and_restore_manual_version_hides_and_restores_empty_thread() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-2";
        create_or_update_thread(&conn, thread_id, "Manual", 200, None).unwrap();

        let assistant_msg = Message {
            id: "assistant-manual".to_string(),
            role: MessageRole::Assistant,
            content: "Manual version".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            timestamp: 200,
            image_data: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &assistant_msg).unwrap();
        delete_version_cluster(&conn, &assistant_msg.id).unwrap();
        assert!(!has_visible_messages(&conn, thread_id).unwrap());
        assert!(get_all_threads(&conn).unwrap().is_empty());

        restore_version_cluster(&conn, &assistant_msg.id).unwrap();
        assert!(has_visible_messages(&conn, thread_id).unwrap());
        assert_eq!(get_all_threads(&conn).unwrap().len(), 1);
    }

    #[test]
    fn test_hide_deleted_message_removes_it_from_trash_listing() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-trash";
        create_or_update_thread(&conn, thread_id, "Trash", 250, None).unwrap();

        let assistant_msg = Message {
            id: "assistant-trash".to_string(),
            role: MessageRole::Assistant,
            content: "Trash candidate".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            timestamp: 250,
            image_data: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &assistant_msg).unwrap();
        delete_version_cluster(&conn, &assistant_msg.id).unwrap();
        assert_eq!(get_deleted_messages(&conn).unwrap().len(), 1);

        assert!(hide_deleted_message(&conn, &assistant_msg.id).unwrap());
        assert!(get_deleted_messages(&conn).unwrap().is_empty());

        restore_version_cluster(&conn, &assistant_msg.id).unwrap();
        assert!(get_deleted_messages(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_message_attachment_images_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-images";
        create_or_update_thread(&conn, thread_id, "Images", 300, None).unwrap();

        let msg = Message {
            id: "user-images".to_string(),
            role: MessageRole::User,
            content: "See references".to_string(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            timestamp: 300,
            image_data: Some("data:image/png;base64,viewport".to_string()),
            attachment_images: vec![
                "data:image/png;base64,ref-1".to_string(),
                "data:image/png;base64,ref-2".to_string(),
            ],
        };

        add_message(&conn, thread_id, &msg).unwrap();

        let messages = get_thread_messages(&conn, thread_id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].image_data.as_deref(),
            Some("data:image/png;base64,viewport")
        );
        assert_eq!(
            messages[0].attachment_images,
            vec![
                "data:image/png;base64,ref-1".to_string(),
                "data:image/png;base64,ref-2".to_string(),
            ]
        );
    }

    #[test]
    fn test_migrate_thread_genie_traits_upgrades_legacy_and_missing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        conn.execute(
            "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, ?4)",
            params![
                "legacy-thread",
                "Legacy",
                100i64,
                r#"{"seed":77,"colorHue":150.0,"vertexCount":18,"jitterScale":1.1,"pulseScale":0.9}"#
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, NULL)",
            params!["missing-thread", "Missing", 101i64],
        )
        .unwrap();

        migrate_thread_genie_traits(&conn).unwrap();

        let legacy_traits = get_thread_genie_traits(&conn, "legacy-thread")
            .unwrap()
            .expect("legacy thread should have traits after migration");
        assert_eq!(legacy_traits.version, crate::models::GENIE_TRAITS_VERSION);
        assert_eq!(legacy_traits.seed, 77);
        assert_eq!(legacy_traits.color_hue, 150.0);
        assert_eq!(legacy_traits.vertex_count, 18);
        assert_eq!(legacy_traits.jitter_scale, 1.1);
        assert_eq!(legacy_traits.pulse_scale, 0.9);

        let missing_traits = get_thread_genie_traits(&conn, "missing-thread")
            .unwrap()
            .expect("missing thread should get synthesized traits");
        assert_eq!(missing_traits.version, crate::models::GENIE_TRAITS_VERSION);
        assert_eq!(
            missing_traits.seed,
            crate::models::derive_thread_seed("missing-thread")
        );

        let raw: String = conn
            .query_row(
                "SELECT genie_traits FROM threads WHERE id = ?1",
                ["missing-thread"],
                |row| row.get(0),
            )
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            parsed.get("version").and_then(serde_json::Value::as_u64),
            Some(crate::models::GENIE_TRAITS_VERSION as u64)
        );
    }

    #[test]
    fn test_read_real_db() {
        let db_path = std::path::Path::new("/Users/bogdan/Library/Application Support/com.alcoholics-audacious.ecky-cad/history.sqlite");
        if !db_path.exists() {
            println!("No DB found at path");
            return;
        }
        let conn = init_db(db_path).unwrap();
        match get_all_threads(&conn) {
            Ok(threads) => {
                println!("Found {} threads", threads.len());
                for t in threads.iter().take(1) {
                    println!("Thread JSON: {}", serde_json::to_string_pretty(&t).unwrap());
                }
            }
            Err(e) => {
                println!("Failed to get threads: {:?}", e);
                panic!("DB read failed");
            }
        }
    }
}
