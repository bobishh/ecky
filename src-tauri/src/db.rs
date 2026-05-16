use crate::models::{
    normalize_design_output, upgraded_or_default_genie_traits, AgentDraft, ArtifactBundle,
    DeletedMessage, DesignOutput, DesignParams, GenieTraits, Message, MessageRole, MessageStatus,
    ModelManifest, TargetLeaseInfo, Thread, ThreadMessagesPage, ThreadReference, UiSpec,
};
use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use serde::de::DeserializeOwned;

#[derive(Debug, Clone)]
struct ThreadMessageRow {
    message: Message,
    deleted_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct LatestSuccessfulTarget {
    pub thread_id: String,
    pub message_id: String,
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
            agent_origin TEXT,
            timestamp INTEGER NOT NULL,
            image_data TEXT,
            visual_kind TEXT,
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
        "CREATE TABLE IF NOT EXISTS thread_window_layouts (
            thread_id TEXT PRIMARY KEY,
            layout_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_thread_references_source_ordinal_kind
         ON thread_references(source_message_id, ordinal, kind)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_sessions (
            session_id TEXT PRIMARY KEY,
            client_kind TEXT NOT NULL,
            host_label TEXT NOT NULL DEFAULT '',
            agent_label TEXT NOT NULL,
            llm_model_id TEXT,
            llm_model_label TEXT,
            thread_id TEXT,
            message_id TEXT,
            model_id TEXT,
            phase TEXT NOT NULL,
            status_text TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL,
            managed_runtime INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;

    if !table_has_column(&conn, "agent_drafts", "preview_id")? {
        let _ = conn.execute("DROP TABLE IF EXISTS agent_drafts", []);
    }
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_drafts (
            preview_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            base_message_id TEXT,
            design_output TEXT NOT NULL,
            artifact_bundle TEXT NOT NULL,
            model_manifest TEXT NOT NULL,
            draft_feedback TEXT,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;
    let _ = conn.execute(
        "ALTER TABLE agent_drafts ADD COLUMN draft_feedback TEXT",
        [],
    );
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_drafts_session
         ON agent_drafts(session_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_drafts_thread_updated
         ON agent_drafts(thread_id, updated_at DESC)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS target_leases (
            lease_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            model_id TEXT,
            acquired_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            host_label TEXT NOT NULL DEFAULT '',
            agent_label TEXT NOT NULL DEFAULT ''
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_thread_visible_timestamp
         ON messages(thread_id, timestamp DESC)
         WHERE deleted_at IS NULL",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_thread_target_candidates
         ON messages(thread_id, role, status, timestamp DESC)
         WHERE deleted_at IS NULL",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_target_leases_target_expires
         ON target_leases(thread_id, message_id, model_id, expires_at DESC)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_target_leases_session
         ON target_leases(session_id, expires_at DESC)",
        [],
    )?;
    // Migrations for existing databases
    let _ = conn.execute(
        "ALTER TABLE threads ADD COLUMN summary TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN genie_traits TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN image_data TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN visual_kind TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN attachment_images TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN usage TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN artifact_bundle TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN model_manifest TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN agent_origin TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN status TEXT NOT NULL DEFAULT 'success'",
        [],
    );
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN deleted_at INTEGER", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN deleted_at INTEGER", []);
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN trash_hidden_at INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE threads ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
        [],
    );
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN finalized_at INTEGER", []);
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN pending_confirm TEXT", []);
    migrate_threads_drop_authoring_columns(&conn)?;
    let _ = conn.execute(
        "ALTER TABLE agent_sessions ADD COLUMN host_label TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE agent_sessions ADD COLUMN llm_model_id TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE agent_sessions ADD COLUMN llm_model_label TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE agent_sessions ADD COLUMN managed_runtime INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "DROP INDEX IF EXISTS idx_agent_session_trace_session_trace_id",
        [],
    );
    let _ = conn.execute("DROP TABLE IF EXISTS agent_session_trace", []);
    migrate_thread_genie_traits(&conn)?;

    Ok(conn)
}

fn deserialize_thread_genie_traits(thread_id: &str, raw: Option<&str>) -> GenieTraits {
    upgraded_or_default_genie_traits(thread_id, raw)
}

fn deserialize_agent_origin(raw: Option<&str>) -> Option<crate::models::AgentOrigin> {
    raw.and_then(|json| serde_json::from_str(json).ok())
}

fn serialize_json<T: serde::Serialize>(value: &T) -> SqlResult<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn deserialize_json<T: DeserializeOwned>(raw: &str) -> SqlResult<T> {
    serde_json::from_str(raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn deserialize_design_output_json(raw: &str) -> SqlResult<DesignOutput> {
    let parsed: DesignOutput = deserialize_json(raw)?;
    Ok(normalize_design_output(parsed))
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

fn table_has_column(conn: &Connection, table_name: &str, column_name: &str) -> SqlResult<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column_name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_threads_drop_authoring_columns(conn: &Connection) -> SqlResult<()> {
    let has_engine_kind = table_has_column(conn, "threads", "engine_kind")?;
    let has_source_language = table_has_column(conn, "threads", "source_language")?;
    let has_geometry_backend = table_has_column(conn, "threads", "geometry_backend")?;

    if !has_engine_kind && !has_source_language && !has_geometry_backend {
        return Ok(());
    }

    conn.execute_batch(
        "
        PRAGMA foreign_keys = OFF;
        CREATE TABLE IF NOT EXISTS threads_new (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL,
            genie_traits TEXT,
            deleted_at INTEGER,
            status TEXT NOT NULL DEFAULT 'active',
            finalized_at INTEGER,
            pending_confirm TEXT
        );
        INSERT OR REPLACE INTO threads_new (
            id,
            title,
            summary,
            updated_at,
            genie_traits,
            deleted_at,
            status,
            finalized_at,
            pending_confirm
        )
        SELECT
            id,
            title,
            COALESCE(summary, ''),
            updated_at,
            genie_traits,
            deleted_at,
            COALESCE(status, 'active'),
            finalized_at,
            pending_confirm
        FROM threads;
        DROP TABLE threads;
        ALTER TABLE threads_new RENAME TO threads;
        PRAGMA foreign_keys = ON;
        ",
    )?;
    Ok(())
}

pub fn get_all_threads(conn: &Connection) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare("
        SELECT id, title, summary,
        COALESCE(
            (
                SELECT MAX(timestamp)
                FROM messages
                WHERE thread_id = threads.id
                  AND deleted_at IS NULL
                  AND status != 'discarded'
            ),
            updated_at
        ) as last_used_at,
        genie_traits,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'success' AND artifact_bundle IS NOT NULL AND deleted_at IS NULL) as v_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'pending' AND deleted_at IS NULL) as p_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'user' AND status = 'pending' AND deleted_at IS NULL) as q_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'error' AND agent_origin IS NULL AND deleted_at IS NULL) as e_count,
        COALESCE(status, 'active') as thread_status,
        finalized_at,
        pending_confirm
        FROM threads
        WHERE deleted_at IS NULL AND COALESCE(status, 'active') = 'active'
        ORDER BY last_used_at DESC, id DESC
    ")?;
    let thread_iter = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let traits_str: Option<String> = row.get(4)?;
        let status_str: String = row
            .get::<_, String>(9)
            .unwrap_or_else(|_| "active".to_string());
        Ok(Thread {
            id: id.clone(),
            title: row.get(1)?,
            summary: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
            messages: vec![],
            genie_traits: Some(deserialize_thread_genie_traits(&id, traits_str.as_deref())),
            version_count: row.get::<_, i64>(5)? as usize,
            pending_count: row.get::<_, i64>(6)? as usize,
            queued_count: row.get::<_, i64>(7)? as usize,
            error_count: row.get::<_, i64>(8)? as usize,
            status: status_str
                .parse()
                .unwrap_or(crate::models::ThreadStatus::Active),
            finalized_at: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            pending_confirm: row.get(11)?,
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        threads.push(thread?);
    }
    Ok(threads)
}

pub fn get_recent_threads_limited(conn: &Connection, limit: usize) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare(
        "
        SELECT id, title, summary,
        COALESCE(
            (
                SELECT MAX(timestamp)
                FROM messages
                WHERE thread_id = threads.id
                  AND deleted_at IS NULL
                  AND status != 'discarded'
            ),
            updated_at
        ) as last_used_at,
        genie_traits,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'success' AND artifact_bundle IS NOT NULL AND deleted_at IS NULL) as v_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'pending' AND deleted_at IS NULL) as p_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'user' AND status = 'pending' AND deleted_at IS NULL) as q_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'error' AND agent_origin IS NULL AND deleted_at IS NULL) as e_count,
        COALESCE(status, 'active') as thread_status,
        finalized_at,
        pending_confirm
        FROM threads
        WHERE deleted_at IS NULL AND COALESCE(status, 'active') = 'active'
        ORDER BY last_used_at DESC, id DESC
        LIMIT ?1
    ",
    )?;
    let thread_iter = stmt.query_map([limit as i64], |row| {
        let id: String = row.get(0)?;
        let traits_str: Option<String> = row.get(4)?;
        let status_str: String = row
            .get::<_, String>(9)
            .unwrap_or_else(|_| "active".to_string());
        Ok(Thread {
            id: id.clone(),
            title: row.get(1)?,
            summary: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
            messages: vec![],
            genie_traits: Some(deserialize_thread_genie_traits(&id, traits_str.as_deref())),
            version_count: row.get::<_, i64>(5)? as usize,
            pending_count: row.get::<_, i64>(6)? as usize,
            queued_count: row.get::<_, i64>(7)? as usize,
            error_count: row.get::<_, i64>(8)? as usize,
            status: status_str
                .parse()
                .unwrap_or(crate::models::ThreadStatus::Active),
            finalized_at: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            pending_confirm: row.get(11)?,
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        threads.push(thread?);
    }
    Ok(threads)
}

pub fn get_latest_successful_message_id_in_thread(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT m.id
         FROM messages m
         JOIN threads t ON t.id = m.thread_id
         WHERE m.thread_id = ?1
           AND t.deleted_at IS NULL
           AND m.deleted_at IS NULL
           AND m.role = 'assistant'
           AND m.status = 'success'
           AND m.artifact_bundle IS NOT NULL
         ORDER BY m.timestamp DESC, m.id DESC
         LIMIT 1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_latest_successful_target_in_most_recent_thread(
    conn: &Connection,
) -> SqlResult<Option<LatestSuccessfulTarget>> {
    conn.query_row(
        "
        WITH recent_threads AS (
            SELECT id,
                   COALESCE(
                       (
                           SELECT MAX(timestamp)
                           FROM messages
                           WHERE thread_id = threads.id
                             AND deleted_at IS NULL
                             AND status != 'discarded'
                       ),
                       updated_at
                   ) AS last_used_at
            FROM threads
            WHERE deleted_at IS NULL
        )
        SELECT m.thread_id, m.id
        FROM messages m
        INNER JOIN recent_threads rt ON rt.id = m.thread_id
        WHERE m.deleted_at IS NULL
          AND m.role = 'assistant'
          AND m.status = 'success'
          AND m.artifact_bundle IS NOT NULL
        ORDER BY rt.last_used_at DESC, m.timestamp DESC, m.id DESC
        LIMIT 1
        ",
        [],
        |row| {
            Ok(LatestSuccessfulTarget {
                thread_id: row.get(0)?,
                message_id: row.get(1)?,
            })
        },
    )
    .optional()
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
         ON CONFLICT(id) DO UPDATE SET
            title=CASE
                WHEN threads.title IS NULL OR trim(threads.title) = '' THEN excluded.title
                ELSE threads.title
            END,
            updated_at=excluded.updated_at,
            genie_traits=COALESCE(excluded.genie_traits, threads.genie_traits)",
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

pub fn update_thread_title(conn: &Connection, thread_id: &str, title: &str) -> SqlResult<bool> {
    let changed = conn.execute(
        "UPDATE threads SET title = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![title, thread_id],
    )?;
    Ok(changed > 0)
}

pub fn get_thread_title(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT title FROM threads WHERE id = ?1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_visible_thread_title(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT title FROM threads WHERE id = ?1 AND deleted_at IS NULL",
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

pub struct ThreadLifecycle {
    pub status: crate::models::ThreadStatus,
    pub finalized_at: Option<u64>,
    pub pending_confirm: Option<String>,
}

pub fn get_thread_lifecycle(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<ThreadLifecycle>> {
    conn.query_row(
        "SELECT COALESCE(status, 'active'), finalized_at, pending_confirm FROM threads WHERE id = ?1",
        [thread_id],
        |row| {
            let status_str: String = row.get::<_, String>(0).unwrap_or_else(|_| "active".to_string());
            Ok(ThreadLifecycle {
                status: status_str.parse().unwrap_or(crate::models::ThreadStatus::Active),
                finalized_at: row.get::<_, Option<i64>>(1)?.map(|v| v as u64),
                pending_confirm: row.get(2)?,
            })
        },
    )
    .optional()
}

pub fn finalize_thread(conn: &Connection, thread_id: &str, now: i64) -> SqlResult<bool> {
    let changed = conn.execute(
        "UPDATE threads SET status = 'finalized', finalized_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![now, thread_id],
    )?;
    Ok(changed > 0)
}

pub fn reopen_thread(conn: &Connection, thread_id: &str) -> SqlResult<bool> {
    let changed = conn.execute(
        "UPDATE threads SET status = 'active', finalized_at = NULL WHERE id = ?1 AND deleted_at IS NULL",
        [thread_id],
    )?;
    Ok(changed > 0)
}

pub fn get_inventory_threads(conn: &Connection) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare("
        SELECT id, title, summary,
        COALESCE(
            (
                SELECT MAX(timestamp)
                FROM messages
                WHERE thread_id = threads.id
                  AND deleted_at IS NULL
                  AND status != 'discarded'
            ),
            updated_at
        ) as last_used_at,
        genie_traits,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'success' AND artifact_bundle IS NOT NULL AND deleted_at IS NULL) as v_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'pending' AND deleted_at IS NULL) as p_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'user' AND status = 'pending' AND deleted_at IS NULL) as q_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'error' AND agent_origin IS NULL AND deleted_at IS NULL) as e_count,
        COALESCE(status, 'active') as thread_status,
        finalized_at,
        pending_confirm
        FROM threads
        WHERE deleted_at IS NULL AND COALESCE(status, 'active') = 'finalized'
        ORDER BY finalized_at DESC, id DESC
    ")?;
    let thread_iter = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let traits_str: Option<String> = row.get(4)?;
        let status_str: String = row
            .get::<_, String>(9)
            .unwrap_or_else(|_| "finalized".to_string());
        Ok(Thread {
            id: id.clone(),
            title: row.get(1)?,
            summary: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
            messages: vec![],
            genie_traits: Some(deserialize_thread_genie_traits(&id, traits_str.as_deref())),
            version_count: row.get::<_, i64>(5)? as usize,
            pending_count: row.get::<_, i64>(6)? as usize,
            queued_count: row.get::<_, i64>(7)? as usize,
            error_count: row.get::<_, i64>(8)? as usize,
            status: status_str
                .parse()
                .unwrap_or(crate::models::ThreadStatus::Finalized),
            finalized_at: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            pending_confirm: row.get(11)?,
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        threads.push(thread?);
    }
    Ok(threads)
}

pub fn set_thread_pending_confirm(
    conn: &Connection,
    thread_id: &str,
    pending_confirm: Option<&str>,
) -> SqlResult<()> {
    conn.execute(
        "UPDATE threads SET pending_confirm = ?1 WHERE id = ?2",
        params![pending_confirm, thread_id],
    )?;
    Ok(())
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
    let agent_origin_str = msg
        .agent_origin
        .as_ref()
        .and_then(|origin| serde_json::to_string(origin).ok());
    let attachment_images_str = if msg.attachment_images.is_empty() {
        None
    } else {
        serde_json::to_string(&msg.attachment_images).ok()
    };
    conn.execute(
        "INSERT INTO messages (id, thread_id, role, content, status, output, usage, artifact_bundle, model_manifest, agent_origin, timestamp, image_data, visual_kind, attachment_images) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            agent_origin_str,
            msg.timestamp as i64,
            msg.image_data,
            msg.visual_kind,
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
        .filter(|row| {
            row.deleted_at.is_none()
                && row.message.status != MessageStatus::Discarded
                && !is_agent_tool_error_message(&row.message)
        })
        .map(|row| row.message)
        .collect())
}

pub fn get_thread_messages_for_thread_view(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Vec<Message>> {
    Ok(load_thread_message_rows(conn, thread_id, true)?
        .into_iter()
        .filter_map(|mut row| {
            if row.deleted_at.is_some() {
                if is_version_message(&row.message) {
                    row.message.status = MessageStatus::Discarded;
                    return Some(row.message);
                }
                return None;
            }

            if row.message.status == MessageStatus::Discarded && !is_version_message(&row.message) {
                return None;
            }

            if is_agent_tool_error_message(&row.message) {
                return None;
            }

            Some(row.message)
        })
        .collect())
}

pub fn get_thread_latest_version(conn: &Connection, thread_id: &str) -> SqlResult<Option<Message>> {
    let rows = load_thread_message_rows_with_clause(
        conn,
        "thread_id = ?1
         AND deleted_at IS NULL
         AND role = 'assistant'
         AND status = 'success'
         AND artifact_bundle IS NOT NULL",
        &[&thread_id],
        "timestamp DESC, rowid DESC",
        Some(1),
    )?;
    Ok(rows.into_iter().next().map(|row| row.message))
}

pub fn get_thread_message_version(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
) -> SqlResult<Option<Message>> {
    let rows = load_thread_message_rows_with_clause(
        conn,
        "thread_id = ?1
         AND id = ?2
         AND deleted_at IS NULL
         AND role = 'assistant'
         AND status = 'success'
         AND artifact_bundle IS NOT NULL",
        &[&thread_id, &message_id],
        "timestamp DESC, rowid DESC",
        Some(1),
    )?;
    Ok(rows.into_iter().next().map(|row| row.message))
}

pub fn get_latest_pending_user_message_id(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT id
         FROM messages
         WHERE thread_id = ?1
           AND deleted_at IS NULL
           AND role = 'user'
           AND status = 'pending'
         ORDER BY timestamp DESC, rowid DESC
         LIMIT 1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_thread_messages_page(
    conn: &Connection,
    thread_id: &str,
    before: Option<u64>,
    limit: usize,
    include_visual_payloads: bool,
) -> SqlResult<ThreadMessagesPage> {
    let safe_limit = limit.clamp(1, 200);
    let mut rows = if let Some(before_ts) = before {
        load_thread_message_rows_with_clause(
            conn,
            "thread_id = ?1 AND status != 'discarded' AND timestamp < ?2",
            &[&thread_id, &(before_ts as i64)],
            "timestamp DESC, rowid DESC",
            Some(safe_limit + 1),
        )?
    } else {
        load_thread_message_rows_with_clause(
            conn,
            "thread_id = ?1 AND status != 'discarded'",
            &[&thread_id],
            "timestamp DESC, rowid DESC",
            Some(safe_limit + 1),
        )?
    };

    let has_more = rows.len() > safe_limit;
    if has_more {
        rows.truncate(safe_limit);
    }

    let mut messages: Vec<Message> = rows
        .into_iter()
        .filter_map(|mut row| {
            if row.deleted_at.is_some() {
                if is_version_message(&row.message) {
                    row.message.status = MessageStatus::Discarded;
                } else {
                    return None;
                }
            } else if row.message.status == MessageStatus::Discarded
                && !is_version_message(&row.message)
            {
                return None;
            }
            if is_agent_tool_error_message(&row.message) {
                return None;
            }

            if !include_visual_payloads {
                row.message.image_data = None;
                row.message.attachment_images.clear();
            }
            Some(row.message)
        })
        .collect();

    messages.reverse();
    let next_before = messages.first().map(|message| message.timestamp);
    Ok(ThreadMessagesPage {
        messages,
        next_before,
        has_more,
    })
}

pub fn get_thread_window_layout(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<crate::models::ThreadWindowLayout>> {
    conn.query_row(
        "SELECT layout_json FROM thread_window_layouts WHERE thread_id = ?1",
        [thread_id],
        |row| {
            let raw: String = row.get(0)?;
            serde_json::from_str(&raw).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })
        },
    )
    .optional()
}

pub fn save_thread_window_layout(
    conn: &Connection,
    thread_id: &str,
    layout: &crate::models::ThreadWindowLayout,
    updated_at: i64,
) -> SqlResult<bool> {
    let thread_exists = conn
        .query_row(
            "SELECT 1 FROM threads WHERE id = ?1 AND deleted_at IS NULL",
            [thread_id],
            |_row| Ok(()),
        )
        .optional()?
        .is_some();
    if !thread_exists {
        return Ok(false);
    }

    let layout_json = serde_json::to_string(layout)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
    conn.execute(
        "INSERT INTO thread_window_layouts (thread_id, layout_json, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(thread_id) DO UPDATE SET
           layout_json = excluded.layout_json,
           updated_at = excluded.updated_at",
        params![thread_id, layout_json, updated_at],
    )?;
    Ok(true)
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
        if is_agent_tool_error_message(&row.message) {
            continue;
        }

        let skip_deleted_version_prompt = row.message.role == MessageRole::User
            && rows
                .get(index + 1)
                .map(|next| {
                    next.deleted_at.is_some()
                        && next.message.role == MessageRole::Assistant
                        && is_version_message(&next.message)
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
        "SELECT id, role, content, status, output, usage, artifact_bundle, model_manifest, agent_origin, timestamp, image_data, visual_kind, attachment_images, deleted_at
         FROM messages
         WHERE thread_id = ?1 AND status != 'discarded'
         ORDER BY timestamp ASC, rowid ASC"
    } else {
        "SELECT id, role, content, status, output, usage, artifact_bundle, model_manifest, agent_origin, timestamp, image_data, visual_kind, attachment_images, deleted_at
         FROM messages
         WHERE thread_id = ?1 AND status != 'discarded' AND deleted_at IS NULL
         ORDER BY timestamp ASC, rowid ASC"
    };

    let mut stmt = conn.prepare(sql)?;
    load_thread_message_rows_from_stmt(&mut stmt, &[&thread_id])
}

fn load_thread_message_rows_with_clause(
    conn: &Connection,
    where_clause: &str,
    params: &[&dyn rusqlite::ToSql],
    order_by: &str,
    limit: Option<usize>,
) -> SqlResult<Vec<ThreadMessageRow>> {
    let mut sql = format!(
        "SELECT id, role, content, status, output, usage, artifact_bundle, model_manifest, agent_origin, timestamp, image_data, visual_kind, attachment_images, deleted_at
         FROM messages
         WHERE {}
         ORDER BY {}",
        where_clause, order_by
    );
    if let Some(limit) = limit {
        sql.push_str(" LIMIT ");
        sql.push_str(&limit.to_string());
    }
    let mut stmt = conn.prepare(&sql)?;
    load_thread_message_rows_from_stmt(&mut stmt, params)
}

fn load_thread_message_rows_from_stmt(
    stmt: &mut rusqlite::Statement<'_>,
    params: &[&dyn rusqlite::ToSql],
) -> SqlResult<Vec<ThreadMessageRow>> {
    let msg_iter = stmt.query_map(params, |row| {
        let output_str: Option<String> = row.get(4)?;
        let output: Option<DesignOutput> =
            output_str.and_then(|s| serde_json::from_str(&s).ok().map(normalize_design_output));
        let usage_str: Option<String> = row.get(5)?;
        let usage = usage_str.and_then(|s| serde_json::from_str(&s).ok());
        let artifact_bundle_str: Option<String> = row.get(6)?;
        let artifact_bundle = artifact_bundle_str.and_then(|s| serde_json::from_str(&s).ok());
        let model_manifest_str: Option<String> = row.get(7)?;
        let model_manifest = model_manifest_str.and_then(|s| serde_json::from_str(&s).ok());
        let agent_origin_str: Option<String> = row.get(8)?;
        let agent_origin = deserialize_agent_origin(agent_origin_str.as_deref());
        let visual_kind = row.get(11)?;
        let attachment_images_str: Option<String> = row.get(12)?;
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
                agent_origin,
                timestamp: row.get::<_, i64>(9)? as u64,
                image_data: row.get(10)?,
                visual_kind,
                attachment_images,
            },
            deleted_at: row.get(13)?,
        })
    })?;

    let mut messages = Vec::new();
    for msg in msg_iter {
        messages.push(msg?);
    }
    Ok(messages)
}

fn is_version_message(message: &Message) -> bool {
    message.role == MessageRole::Assistant && message.artifact_bundle.is_some()
}

fn is_agent_tool_error_message(message: &Message) -> bool {
    message.role == MessageRole::Assistant
        && message.status == MessageStatus::Error
        && message.agent_origin.is_some()
        && message.artifact_bundle.is_none()
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

pub fn get_message_references(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Vec<ThreadReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, source_message_id, ordinal, kind, name, content, summary, pinned, created_at
         FROM thread_references
         WHERE source_message_id = ?1
         ORDER BY created_at ASC, ordinal ASC",
    )?;
    let iter = stmt.query_map([message_id], |row| {
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

pub fn mark_interrupted_pending_messages(conn: &Connection) -> SqlResult<usize> {
    conn.execute(
        "UPDATE messages
         SET status = 'error',
             content = 'Request interrupted by app restart before provider response completed. Retry the last prompt.'
         WHERE role = 'assistant'
           AND status = 'pending'
           AND deleted_at IS NULL",
        [],
    )
}

pub fn update_message_status_and_output(
    conn: &Connection,
    message_id: &str,
    update: MessageStatusUpdate<'_>,
) -> SqlResult<()> {
    let MessageStatusUpdate {
        status,
        output,
        usage,
        artifact_bundle,
        model_manifest,
        visual_kind,
        content,
    } = update;
    let output_str = output.and_then(|o| serde_json::to_string(o).ok());
    let usage_str = usage.and_then(|value| serde_json::to_string(value).ok());
    let artifact_bundle_str = artifact_bundle.and_then(|value| serde_json::to_string(value).ok());
    let model_manifest_str = model_manifest.and_then(|value| serde_json::to_string(value).ok());
    if let Some(text) = content {
        conn.execute(
            "UPDATE messages SET status = ?1, output = ?2, usage = ?3, artifact_bundle = ?4, model_manifest = ?5, visual_kind = COALESCE(?6, visual_kind), content = ?7 WHERE id = ?8",
            params![
                status,
                output_str,
                usage_str,
                artifact_bundle_str,
                model_manifest_str,
                visual_kind,
                text,
                message_id
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE messages SET status = ?1, output = ?2, usage = ?3, artifact_bundle = ?4, model_manifest = ?5, visual_kind = COALESCE(?6, visual_kind) WHERE id = ?7",
            params![
                status,
                output_str,
                usage_str,
                artifact_bundle_str,
                model_manifest_str,
                visual_kind,
                message_id
            ],
        )?;
    }
    Ok(())
}

pub struct MessageStatusUpdate<'a> {
    pub status: &'a MessageStatus,
    pub output: Option<&'a DesignOutput>,
    pub usage: Option<&'a crate::models::UsageSummary>,
    pub artifact_bundle: Option<&'a crate::models::ArtifactBundle>,
    pub model_manifest: Option<&'a crate::models::ModelManifest>,
    pub visual_kind: Option<&'a crate::models::MessageVisualKind>,
    pub content: Option<&'a str>,
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
    let now = unix_now_i64();

    conn.execute(
        "UPDATE messages SET deleted_at = NULL, trash_hidden_at = NULL, timestamp = ?2 WHERE id = ?1",
        params![id, now],
    )?;

    set_thread_deleted_at(conn, &message.thread_id, None)?;
    conn.execute(
        "UPDATE threads SET updated_at = ?1 WHERE id = ?2",
        params![now, message.thread_id],
    )?;
    Ok(Some(message.thread_id))
}

pub fn get_deleted_messages(conn: &Connection) -> SqlResult<Vec<DeletedMessage>> {
    let mut stmt = conn.prepare("
        SELECT m.id, m.thread_id, t.title as thread_title, m.role, m.content, m.output, m.usage, m.artifact_bundle, m.model_manifest, m.agent_origin, m.timestamp, m.image_data, m.visual_kind, m.attachment_images, m.deleted_at
        FROM messages m
        JOIN threads t ON m.thread_id = t.id
        WHERE m.deleted_at IS NOT NULL
          AND m.trash_hidden_at IS NULL
          AND m.role = 'assistant'
          AND m.artifact_bundle IS NOT NULL
        ORDER BY m.deleted_at DESC
    ")?;
    let iter = stmt.query_map([], |row| {
        let output_str: Option<String> = row.get(5)?;
        let output: Option<DesignOutput> = if let Some(json_str) = output_str {
            serde_json::from_str(&json_str)
                .ok()
                .map(normalize_design_output)
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
        let agent_origin_str: Option<String> = row.get(9)?;
        let agent_origin = deserialize_agent_origin(agent_origin_str.as_deref());
        let visual_kind = row.get(12)?;
        let attachment_images_str: Option<String> = row.get(13)?;
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
            agent_origin,
            timestamp: row.get::<_, i64>(10)? as u64,
            image_data: row.get(11)?,
            visual_kind,
            attachment_images,
            deleted_at: row.get::<_, i64>(14)? as u64,
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
        let parsed: DesignOutput = serde_json::from_str(&json_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let mut output: DesignOutput = normalize_design_output(parsed);
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
        let parsed: DesignOutput = serde_json::from_str(&json_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let mut output: DesignOutput = normalize_design_output(parsed);
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
    manifest: &crate::models::ModelManifest,
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
    bundle: &crate::models::ArtifactBundle,
) -> SqlResult<()> {
    let serialized = serde_json::to_string(bundle)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "UPDATE messages SET artifact_bundle = ?1 WHERE id = ?2",
        params![serialized, message_id],
    )?;
    Ok(())
}

pub fn upsert_agent_draft(conn: &Connection, draft: &AgentDraft) -> SqlResult<()> {
    let design_output = serialize_json(&draft.design_output)?;
    let artifact_bundle = serialize_json(&draft.artifact_bundle)?;
    let model_manifest = serialize_json(&draft.model_manifest)?;
    let draft_feedback = match &draft.draft_feedback {
        Some(feedback) => Some(serialize_json(feedback)?),
        None => None,
    };
    conn.execute(
        "INSERT INTO agent_drafts (
            preview_id,
            session_id,
            thread_id,
            base_message_id,
            design_output,
            artifact_bundle,
            model_manifest,
            draft_feedback,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(session_id) DO UPDATE SET
            preview_id = excluded.preview_id,
            thread_id = excluded.thread_id,
            base_message_id = excluded.base_message_id,
            design_output = excluded.design_output,
            artifact_bundle = excluded.artifact_bundle,
            model_manifest = excluded.model_manifest,
            draft_feedback = excluded.draft_feedback,
            updated_at = excluded.updated_at",
        params![
            draft.preview_id,
            draft.session_id,
            draft.thread_id,
            draft.base_message_id,
            design_output,
            artifact_bundle,
            model_manifest,
            draft_feedback,
            draft.updated_at as i64,
        ],
    )?;
    Ok(())
}

fn agent_draft_from_row(row: &rusqlite::Row<'_>) -> SqlResult<AgentDraft> {
    let design_output: String = row.get(4)?;
    let artifact_bundle: String = row.get(5)?;
    let model_manifest: String = row.get(6)?;
    let draft_feedback: Option<String> = row.get(7)?;
    Ok(AgentDraft {
        preview_id: row.get(0)?,
        session_id: row.get(1)?,
        thread_id: row.get(2)?,
        base_message_id: row.get(3)?,
        design_output: deserialize_design_output_json(&design_output)?,
        artifact_bundle: deserialize_json(&artifact_bundle)?,
        model_manifest: deserialize_json(&model_manifest)?,
        draft_feedback: draft_feedback
            .as_deref()
            .map(deserialize_json)
            .transpose()?,
        updated_at: row.get::<_, i64>(8)? as u64,
    })
}

pub fn get_agent_draft_for_session(
    conn: &Connection,
    session_id: &str,
) -> SqlResult<Option<AgentDraft>> {
    conn.query_row(
        "SELECT preview_id, session_id, thread_id, base_message_id, design_output, artifact_bundle, model_manifest, draft_feedback, updated_at
         FROM agent_drafts
         WHERE session_id = ?1",
        params![session_id],
        agent_draft_from_row,
    )
    .optional()
}

pub fn delete_agent_draft_for_session(conn: &Connection, session_id: &str) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM agent_drafts WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(())
}

pub fn update_message_image_data(
    conn: &Connection,
    message_id: &str,
    image_data: &str,
) -> SqlResult<bool> {
    let changed = conn.execute(
        "UPDATE messages SET image_data = ?1 WHERE id = ?2",
        params![image_data, message_id],
    )?;
    Ok(changed > 0)
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

pub fn upsert_agent_session(
    conn: &Connection,
    session: &crate::models::AgentSession,
) -> SqlResult<()> {
    upsert_agent_session_with_ownership(conn, session, session.client_kind == "managed-mcp-http")
}

pub fn upsert_agent_session_with_ownership(
    conn: &Connection,
    session: &crate::models::AgentSession,
    managed_runtime: bool,
) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO agent_sessions (session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at, managed_runtime)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(session_id) DO UPDATE SET
            client_kind = excluded.client_kind,
            host_label = excluded.host_label,
            agent_label = excluded.agent_label,
            llm_model_id = excluded.llm_model_id,
            llm_model_label = excluded.llm_model_label,
            thread_id = excluded.thread_id,
            message_id = excluded.message_id,
            model_id = excluded.model_id,
            phase = excluded.phase,
            status_text = excluded.status_text,
            updated_at = excluded.updated_at,
            managed_runtime = excluded.managed_runtime",
        params![
            session.session_id,
            session.client_kind,
            session.host_label,
            session.agent_label,
            session.llm_model_id,
            session.llm_model_label,
            session.thread_id,
            session.message_id,
            session.model_id,
            session.phase,
            session.status_text,
            session.updated_at as i64,
            i64::from(managed_runtime)
        ],
    )?;
    Ok(())
}

pub fn delete_agent_session(conn: &Connection, session_id: &str) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM agent_sessions WHERE session_id = ?1",
        [session_id],
    )?;
    Ok(())
}

pub fn get_active_agent_sessions(
    conn: &Connection,
    stale_threshold_secs: u64,
) -> SqlResult<Vec<crate::models::AgentSession>> {
    let now = unix_now_i64();
    let threshold = now - (stale_threshold_secs as i64);

    let mut stmt = conn.prepare(
        "SELECT session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at
         FROM agent_sessions
         WHERE updated_at >= ?1
           AND phase NOT IN ('error', 'disconnected')
         ORDER BY updated_at DESC"
    )?;
    let iter = stmt.query_map([threshold], |row| {
        Ok(crate::models::AgentSession {
            session_id: row.get(0)?,
            client_kind: row.get(1)?,
            host_label: row.get(2)?,
            agent_label: row.get(3)?,
            llm_model_id: row.get(4)?,
            llm_model_label: row.get(5)?,
            thread_id: row.get(6)?,
            message_id: row.get(7)?,
            model_id: row.get(8)?,
            phase: row.get(9)?,
            status_text: row.get(10)?,
            updated_at: row.get::<_, i64>(11)? as u64,
        })
    })?;

    let mut results = Vec::new();
    for item in iter {
        results.push(item?);
    }
    Ok(results)
}

/// Fetch DB records for a specific set of session IDs (used for live-session push events).
pub fn get_sessions_by_ids(
    conn: &Connection,
    ids: &[String],
) -> SqlResult<Vec<crate::models::AgentSession>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at
         FROM agent_sessions
         WHERE session_id IN ({})
         ORDER BY updated_at DESC",
        placeholders
    );
    let mut stmt = conn.prepare(&sql)?;
    let iter = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |row| {
        Ok(crate::models::AgentSession {
            session_id: row.get(0)?,
            client_kind: row.get(1)?,
            host_label: row.get(2)?,
            agent_label: row.get(3)?,
            llm_model_id: row.get(4)?,
            llm_model_label: row.get(5)?,
            thread_id: row.get(6)?,
            message_id: row.get(7)?,
            model_id: row.get(8)?,
            phase: row.get(9)?,
            status_text: row.get(10)?,
            updated_at: row.get::<_, i64>(11)? as u64,
        })
    })?;
    let mut results = Vec::new();
    for item in iter {
        results.push(item?);
    }
    Ok(results)
}

pub fn get_thread_last_agent_session(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<crate::models::AgentSession>> {
    conn.query_row(
        "SELECT session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at
         FROM agent_sessions
         WHERE thread_id = ?1
         ORDER BY updated_at DESC
         LIMIT 1",
        [thread_id],
        |row| {
            Ok(crate::models::AgentSession {
                session_id: row.get(0)?,
                client_kind: row.get(1)?,
                host_label: row.get(2)?,
                agent_label: row.get(3)?,
                llm_model_id: row.get(4)?,
                llm_model_label: row.get(5)?,
                thread_id: row.get(6)?,
                message_id: row.get(7)?,
                model_id: row.get(8)?,
                phase: row.get(9)?,
                status_text: row.get(10)?,
                updated_at: row.get::<_, i64>(11)? as u64,
            })
        },
    )
    .optional()
}

pub fn get_thread_last_agent_session_for_agent(
    conn: &Connection,
    agent_label: &str,
) -> SqlResult<Option<crate::models::AgentSession>> {
    conn.query_row(
        "SELECT session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at
         FROM agent_sessions
         WHERE agent_label = ?1
         ORDER BY updated_at DESC
         LIMIT 1",
        [agent_label],
        |row| {
            Ok(crate::models::AgentSession {
                session_id: row.get(0)?,
                client_kind: row.get(1)?,
                host_label: row.get(2)?,
                agent_label: row.get(3)?,
                llm_model_id: row.get(4)?,
                llm_model_label: row.get(5)?,
                thread_id: row.get(6)?,
                message_id: row.get(7)?,
                model_id: row.get(8)?,
                phase: row.get(9)?,
                status_text: row.get(10)?,
                updated_at: row.get::<_, i64>(11)? as u64,
            })
        },
    )
    .optional()
}

pub fn get_managed_agent_session_ids_not_in(
    conn: &Connection,
    live_session_ids: &[String],
) -> SqlResult<Vec<String>> {
    if live_session_ids.is_empty() {
        let mut stmt = conn.prepare(
            "SELECT session_id
             FROM agent_sessions
             WHERE managed_runtime != 0",
        )?;
        let iter = stmt.query_map([], |row| row.get(0))?;
        let mut session_ids = Vec::new();
        for item in iter {
            session_ids.push(item?);
        }
        return Ok(session_ids);
    }

    let placeholders = live_session_ids
        .iter()
        .enumerate()
        .map(|(index, _)| format!("?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT session_id
         FROM agent_sessions
         WHERE managed_runtime != 0
           AND session_id NOT IN ({})",
        placeholders
    );
    let mut stmt = conn.prepare(&sql)?;
    let iter = stmt.query_map(rusqlite::params_from_iter(live_session_ids.iter()), |row| {
        row.get(0)
    })?;
    let mut session_ids = Vec::new();
    for item in iter {
        session_ids.push(item?);
    }
    Ok(session_ids)
}

pub fn delete_expired_target_leases(conn: &Connection) -> SqlResult<usize> {
    conn.execute(
        "DELETE FROM target_leases WHERE expires_at < ?1",
        [unix_now_i64()],
    )
}

pub fn get_active_target_lease(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
    model_id: Option<&str>,
) -> SqlResult<Option<TargetLeaseInfo>> {
    let _ = delete_expired_target_leases(conn)?;
    conn.query_row(
        "SELECT session_id, thread_id, message_id, model_id, host_label, agent_label, acquired_at, expires_at
         FROM target_leases
         WHERE thread_id = ?1
           AND message_id = ?2
           AND COALESCE(model_id, '') = COALESCE(?3, '')
         ORDER BY expires_at DESC
         LIMIT 1",
        params![thread_id, message_id, model_id],
        |row| {
            Ok(TargetLeaseInfo {
                session_id: row.get(0)?,
                thread_id: row.get(1)?,
                message_id: row.get(2)?,
                model_id: row.get(3)?,
                host_label: row.get(4)?,
                agent_label: row.get(5)?,
                acquired_at: row.get::<_, i64>(6)? as u64,
                expires_at: row.get::<_, i64>(7)? as u64,
            })
        },
    )
    .optional()
}

pub fn upsert_target_lease(conn: &Connection, lease: &TargetLeaseInfo) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO target_leases (lease_id, session_id, thread_id, message_id, model_id, acquired_at, expires_at, host_label, agent_label)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(lease_id) DO UPDATE SET
            session_id = excluded.session_id,
            thread_id = excluded.thread_id,
            message_id = excluded.message_id,
            model_id = excluded.model_id,
            acquired_at = excluded.acquired_at,
            expires_at = excluded.expires_at,
            host_label = excluded.host_label,
            agent_label = excluded.agent_label",
        params![
            format!(
                "{}:{}:{}",
                lease.session_id,
                lease.message_id,
                lease.model_id.clone().unwrap_or_default()
            ),
            lease.session_id,
            lease.thread_id,
            lease.message_id,
            lease.model_id,
            lease.acquired_at as i64,
            lease.expires_at as i64,
            lease.host_label,
            lease.agent_label
        ],
    )?;
    Ok(())
}

pub fn delete_target_lease(
    conn: &Connection,
    session_id: &str,
    thread_id: &str,
    message_id: &str,
    model_id: Option<&str>,
) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM target_leases
         WHERE session_id = ?1
           AND thread_id = ?2
           AND message_id = ?3
           AND COALESCE(model_id, '') = COALESCE(?4, '')",
        params![session_id, thread_id, message_id, model_id],
    )?;
    Ok(())
}

pub fn delete_target_leases_for_session(conn: &Connection, session_id: &str) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM target_leases WHERE session_id = ?1",
        [session_id],
    )?;
    Ok(())
}

pub fn get_message_output_and_thread(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Option<(DesignOutput, String)>> {
    let row: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT m.output, m.thread_id
             FROM messages m
             JOIN threads t ON t.id = m.thread_id
             WHERE m.id = ?1
               AND m.deleted_at IS NULL
               AND m.status != 'discarded'
               AND t.deleted_at IS NULL",
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

    let Ok(output) = serde_json::from_str::<DesignOutput>(&json_str).map(normalize_design_output)
    else {
        return Ok(None);
    };

    Ok(Some((output, thread_id)))
}

pub fn get_message_thread_id(conn: &Connection, message_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT thread_id FROM messages WHERE id = ?1",
        [message_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_visible_message_thread_id(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT m.thread_id
         FROM messages m
         JOIN threads t ON t.id = m.thread_id
         WHERE m.id = ?1
           AND m.deleted_at IS NULL
           AND m.status != 'discarded'
           AND t.deleted_at IS NULL",
        [message_id],
        |row| row.get(0),
    )
    .optional()
}

pub type MessageRuntimeAndThread = (Option<ArtifactBundle>, Option<ModelManifest>, String);

pub fn get_message_runtime_and_thread(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Option<MessageRuntimeAndThread>> {
    let row: Option<(Option<String>, Option<String>, String)> = conn
        .query_row(
            "SELECT m.artifact_bundle, m.model_manifest, m.thread_id
             FROM messages m
             JOIN threads t ON t.id = m.thread_id
             WHERE m.id = ?1
               AND m.deleted_at IS NULL
               AND m.status != 'discarded'
               AND t.deleted_at IS NULL",
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
    use std::fs;
    use std::path::PathBuf;

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
                agent_origin TEXT,
                timestamp INTEGER NOT NULL,
                image_data TEXT,
                visual_kind TEXT,
                attachment_images TEXT,
                deleted_at INTEGER,
                trash_hidden_at INTEGER,
                FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_sessions (
                session_id TEXT PRIMARY KEY,
                client_kind TEXT NOT NULL,
                host_label TEXT NOT NULL DEFAULT '',
                agent_label TEXT NOT NULL,
                llm_model_id TEXT,
                llm_model_label TEXT,
                thread_id TEXT,
                message_id TEXT,
                model_id TEXT,
                phase TEXT NOT NULL,
                status_text TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL,
                managed_runtime INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_drafts (
                preview_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                base_message_id TEXT,
                design_output TEXT NOT NULL,
                artifact_bundle TEXT NOT NULL,
                model_manifest TEXT NOT NULL,
                draft_feedback TEXT,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_drafts_session
             ON agent_drafts(session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_agent_drafts_thread_updated
             ON agent_drafts(thread_id, updated_at DESC)",
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
            "CREATE TABLE IF NOT EXISTS thread_window_layouts (
                thread_id TEXT PRIMARY KEY,
                layout_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )",
            [],
        )?;
        // Migrations: keep in sync with init_db
        let _ = conn.execute(
            "ALTER TABLE threads ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
            [],
        );
        let _ = conn.execute("ALTER TABLE threads ADD COLUMN finalized_at INTEGER", []);
        let _ = conn.execute("ALTER TABLE threads ADD COLUMN pending_confirm TEXT", []);
        Ok(())
    }

    fn sample_output() -> DesignOutput {
        DesignOutput {
            title: "Test".to_string(),
            version_name: "V1".to_string(),
            response: "".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: "print('hi')".to_string(),
            macro_dialect: crate::models::MacroDialect::Legacy,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            ui_spec: UiSpec { fields: Vec::new() },
            initial_params: DesignParams::from([("x".to_string(), ParamValue::Number(10.0))]),
            post_processing: None,
        }
    }

    fn sample_artifact_bundle(model_id: &str) -> ArtifactBundle {
        ArtifactBundle {
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
            agent_origin: None,
            timestamp: now,
            image_data: None,
            visual_kind: None,
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
            agent_origin: None,
            timestamp: 100,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let assistant_msg = Message {
            id: "assistant-1".to_string(),
            role: MessageRole::Assistant,
            content: "Box created".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-1")),
            model_manifest: None,
            agent_origin: None,
            timestamp: 101,
            image_data: None,
            visual_kind: None,
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
            artifact_bundle: Some(sample_artifact_bundle("assistant-manual")),
            model_manifest: None,
            agent_origin: None,
            timestamp: 200,
            image_data: None,
            visual_kind: None,
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
    fn test_restored_version_becomes_latest_version() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-restore-latest";
        create_or_update_thread(&conn, thread_id, "Restore Latest", 100, None).unwrap();

        let older_msg = Message {
            id: "assistant-older".to_string(),
            role: MessageRole::Assistant,
            content: "Older version".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-older")),
            model_manifest: None,
            agent_origin: None,
            timestamp: 100,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let newer_msg = Message {
            id: "assistant-newer".to_string(),
            role: MessageRole::Assistant,
            content: "Newer version".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-newer")),
            model_manifest: None,
            agent_origin: None,
            timestamp: 200,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &older_msg).unwrap();
        add_message(&conn, thread_id, &newer_msg).unwrap();
        delete_version_cluster(&conn, &older_msg.id).unwrap();
        assert_eq!(
            get_thread_latest_version(&conn, thread_id)
                .unwrap()
                .unwrap()
                .id,
            "assistant-newer"
        );

        restore_version_cluster(&conn, &older_msg.id).unwrap();
        assert_eq!(
            get_thread_latest_version(&conn, thread_id)
                .unwrap()
                .unwrap()
                .id,
            "assistant-older"
        );
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
            artifact_bundle: Some(sample_artifact_bundle("assistant-trash")),
            model_manifest: None,
            agent_origin: None,
            timestamp: 250,
            image_data: None,
            visual_kind: None,
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
            agent_origin: None,
            timestamp: 300,
            image_data: Some("data:image/png;base64,viewport".to_string()),
            visual_kind: Some(crate::models::MessageVisualKind::ConceptPreview),
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
            messages[0].visual_kind,
            Some(crate::models::MessageVisualKind::ConceptPreview)
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
    fn test_update_message_image_data_updates_version_preview() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-preview";
        create_or_update_thread(&conn, thread_id, "Preview", 400, None).unwrap();

        let msg = Message {
            id: "assistant-preview".to_string(),
            role: MessageRole::Assistant,
            content: "Rendered".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-preview")),
            model_manifest: None,
            agent_origin: None,
            timestamp: 400,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &msg).unwrap();

        let changed =
            update_message_image_data(&conn, &msg.id, "data:image/jpeg;base64,render-preview")
                .unwrap();
        assert!(changed);

        let latest = get_thread_latest_version(&conn, thread_id)
            .unwrap()
            .expect("latest version");
        assert_eq!(
            latest.image_data.as_deref(),
            Some("data:image/jpeg;base64,render-preview")
        );
    }

    #[test]
    fn test_thread_version_count_ignores_output_only_success_messages() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-renderable-count";
        create_or_update_thread(&conn, thread_id, "Renderable", 500, None).unwrap();

        let output_only = Message {
            id: "assistant-output-only".to_string(),
            role: MessageRole::Assistant,
            content: "Draft only".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            agent_origin: None,
            timestamp: 500,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let rendered = Message {
            id: "assistant-rendered".to_string(),
            role: MessageRole::Assistant,
            content: "Rendered".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-rendered")),
            model_manifest: None,
            agent_origin: None,
            timestamp: 501,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &output_only).unwrap();
        add_message(&conn, thread_id, &rendered).unwrap();

        let threads = get_all_threads(&conn).unwrap();
        assert_eq!(threads[0].version_count, 1);
        assert_eq!(
            get_thread_latest_version(&conn, thread_id)
                .unwrap()
                .unwrap()
                .id,
            rendered.id
        );
    }

    #[test]
    fn test_get_all_threads_orders_by_latest_visible_message_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "older-thread", "Older", 100, None).unwrap();
        create_or_update_thread(&conn, "newer-thread", "Newer", 50, None).unwrap();

        add_message(
            &conn,
            "older-thread",
            &Message {
                id: "older-msg".to_string(),
                role: MessageRole::User,
                content: "older".to_string(),
                status: MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                agent_origin: None,
                timestamp: 200,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        add_message(
            &conn,
            "newer-thread",
            &Message {
                id: "newer-msg".to_string(),
                role: MessageRole::User,
                content: "newer".to_string(),
                status: MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                agent_origin: None,
                timestamp: 300,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        let threads = get_all_threads(&conn).unwrap();
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id, "newer-thread");
        assert_eq!(threads[0].updated_at, 300);
        assert_eq!(threads[1].id, "older-thread");
        assert_eq!(threads[1].updated_at, 200);
    }

    #[test]
    fn test_mark_interrupted_pending_messages_promotes_pending_to_error() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "pending-thread", "Pending", 100, None).unwrap();

        add_message(
            &conn,
            "pending-thread",
            &Message {
                id: "pending-assistant".to_string(),
                role: MessageRole::Assistant,
                content: "Generating...".to_string(),
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

        let changed = mark_interrupted_pending_messages(&conn).unwrap();
        assert_eq!(changed, 1);

        let messages = get_thread_messages(&conn, "pending-thread").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].status, MessageStatus::Error);
        assert!(messages[0]
            .content
            .contains("Request interrupted by app restart before provider response completed"));
    }

    #[test]
    fn get_thread_messages_preserves_insertion_order_for_equal_timestamps() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "thread-order", "Thread", 100, None).unwrap();

        add_message(
            &conn,
            "thread-order",
            &Message {
                id: "msg-a".to_string(),
                role: MessageRole::User,
                content: "first".to_string(),
                status: MessageStatus::Success,
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

        add_message(
            &conn,
            "thread-order",
            &Message {
                id: "msg-b".to_string(),
                role: MessageRole::Assistant,
                content: "second".to_string(),
                status: MessageStatus::Success,
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

        let messages = get_thread_messages(&conn, "thread-order").unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg-a", "msg-b"]
        );
    }

    #[test]
    fn thread_message_reads_hide_agent_tool_errors_from_history() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "thread-agent-errors", "Thread", 100, None).unwrap();
        let visible_user = Message {
            id: "user-visible".to_string(),
            role: MessageRole::User,
            content: "make roof pins".to_string(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            agent_origin: None,
            timestamp: 100,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let agent_error = Message {
            id: "agent-error".to_string(),
            role: MessageRole::Assistant,
            content: "Expected a symbolic head for runtime list expression.".to_string(),
            status: MessageStatus::Error,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            agent_origin: Some(crate::models::AgentOrigin {
                host_label: "Codex MCP Client".to_string(),
                client_kind: "mcp-http".to_string(),
                agent_label: "Ecky".to_string(),
                llm_model_id: None,
                llm_model_label: None,
                session_id: "session-1".to_string(),
                created_at: 101,
            }),
            timestamp: 101,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let generation_error = Message {
            id: "generation-error".to_string(),
            role: MessageRole::Assistant,
            content: "Generation failed.".to_string(),
            status: MessageStatus::Error,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            agent_origin: None,
            timestamp: 102,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        add_message(&conn, "thread-agent-errors", &visible_user).unwrap();
        add_message(&conn, "thread-agent-errors", &agent_error).unwrap();
        add_message(&conn, "thread-agent-errors", &generation_error).unwrap();

        let full = get_thread_messages(&conn, "thread-agent-errors").unwrap();
        assert_eq!(
            full.iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["user-visible", "generation-error"]
        );

        let page = get_thread_messages_page(&conn, "thread-agent-errors", None, 50, true).unwrap();
        assert_eq!(
            page.messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["user-visible", "generation-error"]
        );

        let context = get_thread_messages_for_context(&conn, "thread-agent-errors").unwrap();
        assert_eq!(
            context
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["user-visible", "generation-error"]
        );

        let threads = get_all_threads(&conn).unwrap();
        assert_eq!(threads[0].error_count, 1);
    }

    #[test]
    fn create_or_update_thread_preserves_existing_title_on_conflict() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "thread-keep-title", "Original Thread", 100, None).unwrap();
        create_or_update_thread(&conn, "thread-keep-title", "Version Name Noise", 200, None)
            .unwrap();

        let thread = get_all_threads(&conn)
            .unwrap()
            .into_iter()
            .find(|thread| thread.id == "thread-keep-title")
            .expect("thread exists");
        assert_eq!(thread.title, "Original Thread");
        assert_eq!(thread.updated_at, 200);
    }

    #[test]
    fn create_or_update_thread_inserts_thread_without_authoring_context() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "thread-no-context", "No Context", 100, None).unwrap();

        let thread = get_all_threads(&conn)
            .unwrap()
            .into_iter()
            .find(|thread| thread.id == "thread-no-context")
            .expect("thread exists");
        assert_eq!(thread.title, "No Context");
    }

    #[test]
    fn migrate_threads_drop_authoring_columns_removes_legacy_thread_context() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();
        conn.execute(
            "ALTER TABLE threads ADD COLUMN engine_kind TEXT NOT NULL DEFAULT 'freecad'",
            [],
        )
        .unwrap();
        conn.execute(
            "ALTER TABLE threads ADD COLUMN source_language TEXT NOT NULL DEFAULT 'legacyPython'",
            [],
        )
        .unwrap();
        conn.execute(
            "ALTER TABLE threads ADD COLUMN geometry_backend TEXT NOT NULL DEFAULT 'freecad'",
            [],
        )
        .unwrap();
        create_or_update_thread(&conn, "thread-context", "Context", 100, None).unwrap();
        conn.execute(
            "UPDATE threads SET status = 'finalized', finalized_at = 123, pending_confirm = 'review' WHERE id = 'thread-context'",
            [],
        )
        .unwrap();

        migrate_threads_drop_authoring_columns(&conn).unwrap();

        assert!(!table_has_column(&conn, "threads", "engine_kind").unwrap());
        assert!(!table_has_column(&conn, "threads", "source_language").unwrap());
        assert!(!table_has_column(&conn, "threads", "geometry_backend").unwrap());
        let thread = get_inventory_threads(&conn)
            .unwrap()
            .into_iter()
            .find(|thread| thread.id == "thread-context")
            .expect("thread survives migration");
        assert_eq!(thread.finalized_at, Some(123));
        assert_eq!(thread.pending_confirm.as_deref(), Some("review"));
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
        let db_path = std::path::Path::new(
            "/Users/bogdan/Library/Application Support/com.alcoholics-audacious.ecky-cad/history.sqlite",
        );
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

    #[test]
    fn trimmed_mcp_fixture_keeps_thread_bound_agent_sessions() {
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mcp_regression_fixture.sqlite");
        assert!(
            fixture_path.exists(),
            "missing fixture at {}",
            fixture_path.display()
        );

        let temp_db =
            std::env::temp_dir().join(format!("ecky-mcp-fixture-{}.sqlite", uuid::Uuid::new_v4()));
        fs::copy(&fixture_path, &temp_db).expect("copy fixture");
        let conn = init_db(&temp_db).expect("open fixture copy");

        let raw_thread_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
            .expect("thread count");
        assert_eq!(raw_thread_count, 3);

        let visible_threads = get_all_threads(&conn).expect("threads");
        assert!(
            visible_threads
                .iter()
                .any(|thread| thread.id == "29c64fc4-803b-4d75-bac0-e0f656304881"),
            "fixture should keep the Panelka thread visible"
        );

        let last_panelka_session =
            get_thread_last_agent_session(&conn, "29c64fc4-803b-4d75-bac0-e0f656304881")
                .expect("last session")
                .expect("panelka session");
        assert_eq!(
            last_panelka_session.thread_id.as_deref(),
            Some("29c64fc4-803b-4d75-bac0-e0f656304881")
        );
        assert!(!last_panelka_session.session_id.is_empty());
    }

    #[test]
    fn init_db_drops_legacy_agent_session_trace_table_and_index() {
        let db_path =
            std::env::temp_dir().join(format!("ecky-agent-trace-drop-{}", uuid::Uuid::new_v4()));
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE agent_session_trace (
                    trace_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    summary TEXT NOT NULL
                )",
                [],
            )
            .unwrap();
            conn.execute(
                "CREATE INDEX idx_agent_session_trace_session_trace_id
                 ON agent_session_trace(session_id, trace_id DESC)",
                [],
            )
            .unwrap();
        }

        let conn = init_db(&db_path).unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'agent_session_trace'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_agent_session_trace_session_trace_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);
        assert_eq!(index_count, 0);
    }

    #[test]
    fn init_db_preserves_agent_drafts_table() {
        let db_path =
            std::env::temp_dir().join(format!("ecky-agent-drafts-legacy-{}", uuid::Uuid::new_v4()));
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE agent_drafts (
                    session_id TEXT NOT NULL,
                    thread_id TEXT NOT NULL,
                    base_message_id TEXT NOT NULL,
                    design_output TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                )",
                [],
            )
            .unwrap();
        }

        let conn = init_db(&db_path).unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'agent_drafts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1);
        assert!(table_has_column(&conn, "agent_drafts", "draft_feedback").unwrap());
    }

    #[test]
    fn agent_draft_roundtrip_preserves_draft_feedback() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let draft = AgentDraft {
            preview_id: "preview-1".to_string(),
            session_id: "session-1".to_string(),
            thread_id: "thread-1".to_string(),
            base_message_id: Some("msg-1".to_string()),
            design_output: crate::models::DesignOutput {
                title: "Draft".to_string(),
                version_name: String::new(),
                response: "ok".to_string(),
                interaction_mode: InteractionMode::Design,
                macro_code: "draft_macro()".to_string(),
                macro_dialect: crate::models::MacroDialect::Legacy,
                engine_kind: crate::models::EngineKind::Freecad,
                source_language: crate::models::SourceLanguage::LegacyPython,
                geometry_backend: crate::models::GeometryBackend::Freecad,
                ui_spec: UiSpec { fields: Vec::new() },
                initial_params: DesignParams::from([(
                    "diameter".to_string(),
                    ParamValue::Number(12.0),
                )]),
                post_processing: None,
            },
            artifact_bundle: ArtifactBundle {
                schema_version: crate::models::MODEL_RUNTIME_SCHEMA_VERSION,
                model_id: "model-1".to_string(),
                source_kind: crate::models::ModelSourceKind::Generated,
                engine_kind: crate::models::EngineKind::Freecad,
                geometry_backend: crate::models::GeometryBackend::Freecad,
                source_language: crate::models::SourceLanguage::LegacyPython,
                content_hash: "hash-1".to_string(),
                artifact_version: 1,
                fcstd_path: "/tmp/model-1.FCStd".to_string(),
                manifest_path: "/tmp/model-1.json".to_string(),
                macro_path: Some("/tmp/model-1.py".to_string()),
                preview_stl_path: "/tmp/model-1.stl".to_string(),
                viewer_assets: Vec::new(),
                edge_targets: Vec::new(),
                face_targets: Vec::new(),
                callout_anchors: Vec::new(),
                measurement_guides: Vec::new(),
                export_artifacts: Vec::new(),
            },
            model_manifest: crate::models::ModelManifest {
                schema_version: crate::models::MODEL_RUNTIME_SCHEMA_VERSION,
                model_id: "model-1".to_string(),
                source_kind: crate::models::ModelSourceKind::Generated,
                engine_kind: crate::models::EngineKind::Freecad,
                source_language: crate::models::SourceLanguage::LegacyPython,
                geometry_backend: crate::models::GeometryBackend::Freecad,
                document: crate::models::DocumentMetadata {
                    document_name: "Doc".to_string(),
                    document_label: "Doc".to_string(),
                    source_path: None,
                    object_count: 1,
                    warnings: Vec::new(),
                },
                parts: vec![crate::models::PartBinding {
                    part_id: "body".to_string(),
                    freecad_object_name: "Body".to_string(),
                    label: "Body".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: None,
                    viewer_asset_path: None,
                    viewer_node_ids: vec!["body".to_string()],
                    parameter_keys: Vec::new(),
                    editable: true,
                    bounds: None,
                    volume: None,
                    area: None,
                }],
                parameter_groups: Vec::new(),
                control_primitives: Vec::new(),
                control_relations: Vec::new(),
                control_views: Vec::new(),
                advisories: Vec::new(),
                selection_targets: Vec::new(),
                measurement_annotations: Vec::new(),
                warnings: Vec::new(),
                enrichment_state: crate::contracts::ManifestEnrichmentState {
                    status: crate::contracts::EnrichmentStatus::None,
                    proposals: Vec::new(),
                },
            },
            draft_feedback: Some(crate::models::AgentDraftFeedback {
                session_id: "session-1".to_string(),
                thread_id: "thread-1".to_string(),
                preview_id: "preview-1".to_string(),
                status: crate::models::AgentDraftFeedbackStatus::Failed,
                summary: "Preview STL file not found.".to_string(),
                items: vec![crate::models::AgentDraftFeedbackItem {
                    code: "PREVIEW_STL_MISSING".to_string(),
                    message: "Preview STL file not found.".to_string(),
                }],
                source: crate::models::AgentDraftFeedbackSource::StructuralVerification,
            }),
            updated_at: 123,
        };

        upsert_agent_draft(&conn, &draft).unwrap();
        let loaded = get_agent_draft_for_session(&conn, "session-1")
            .unwrap()
            .expect("draft");

        assert_eq!(loaded.draft_feedback, draft.draft_feedback);
    }

    #[test]
    fn thread_window_layout_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-layout-1";
        create_or_update_thread(&conn, thread_id, "Layout Thread", 100, None).unwrap();

        let mut windows = std::collections::HashMap::new();
        windows.insert(
            "projects".to_string(),
            crate::models::ThreadWindowState {
                visible: true,
                minimized: false,
                x: 50.0,
                y: 60.0,
                width: 400.0,
                height: 300.0,
                z: 1,
            },
        );
        let layout = crate::models::ThreadWindowLayout {
            schema_version: 1,
            remember_layout: true,
            windows,
        };

        let saved = save_thread_window_layout(&conn, thread_id, &layout, 200).unwrap();
        assert!(saved);

        let loaded = get_thread_window_layout(&conn, thread_id).unwrap();
        assert_eq!(loaded, Some(layout));
    }

    #[test]
    fn thread_window_layout_returns_none_when_missing() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-no-layout";
        create_or_update_thread(&conn, thread_id, "No Layout", 100, None).unwrap();

        let loaded = get_thread_window_layout(&conn, thread_id).unwrap();
        assert_eq!(loaded, None);
    }

    #[test]
    fn thread_window_layout_save_fails_for_missing_thread() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let layout = crate::models::ThreadWindowLayout {
            schema_version: 1,
            remember_layout: true,
            windows: std::collections::HashMap::new(),
        };

        let saved = save_thread_window_layout(&conn, "nonexistent", &layout, 200).unwrap();
        assert!(!saved);
    }

    #[test]
    fn thread_window_layout_delete_thread_does_not_break_others() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let t1 = "thread-a";
        let t2 = "thread-b";
        create_or_update_thread(&conn, t1, "A", 100, None).unwrap();
        create_or_update_thread(&conn, t2, "B", 100, None).unwrap();

        let layout1 = crate::models::ThreadWindowLayout {
            schema_version: 1,
            remember_layout: true,
            windows: std::collections::HashMap::new(),
        };
        let mut windows2 = std::collections::HashMap::new();
        windows2.insert(
            "params".to_string(),
            crate::models::ThreadWindowState {
                visible: false,
                minimized: false,
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 200.0,
                z: 0,
            },
        );
        let layout2 = crate::models::ThreadWindowLayout {
            schema_version: 1,
            remember_layout: true,
            windows: windows2,
        };

        save_thread_window_layout(&conn, t1, &layout1, 200).unwrap();
        save_thread_window_layout(&conn, t2, &layout2, 200).unwrap();

        // Soft-delete thread A
        delete_thread(&conn, t1).unwrap();

        // Thread B layout should still work
        let loaded = get_thread_window_layout(&conn, t2).unwrap();
        assert_eq!(loaded, Some(layout2));
    }
}
