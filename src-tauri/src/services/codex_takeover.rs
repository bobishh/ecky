use crate::contracts::{
    AppError, AppResult, Attachment, CodexDialogueMessage, CodexMessagePage, CodexQueuedPrompt,
    CodexTakeoverBinding,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rusqlite::{params, types::Type, Connection, OptionalExtension};

pub const CODEX_BOOTSTRAP_VERSION: u32 = 5;
pub const CODEX_PROVIDER_ID: &str = "codex";
pub const JEV_CLASSIFIER_ERROR_PREFIX: &str = "Jev classifier failed; automatic retry disabled.";
static CODEX_QUEUE_WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();

pub fn notify_queue_supervisor() {
    CODEX_QUEUE_WAKE.notify_one();
}

pub async fn wait_for_queue_supervisor() {
    CODEX_QUEUE_WAKE.notified().await;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentThreadBindingRecord {
    pub ecky_thread_id: String,
    pub provider: String,
    pub external_thread_id: String,
    pub external_title: String,
    pub external_cwd: String,
    pub bootstrap_version: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentThreadBindingLineageRecord {
    pub ecky_thread_id: String,
    pub provider: String,
    pub external_thread_id: String,
    pub activated_at: i64,
    pub superseded_at: Option<i64>,
    pub superseded_reason: Option<String>,
}

pub fn error_text(error: &AppError) -> String {
    match error
        .details
        .as_deref()
        .filter(|details| !details.trim().is_empty())
    {
        Some(details) => format!("{}\n{}", error.message, details),
        None => error.message.clone(),
    }
}

pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    let binding_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'agent_thread_bindings')",
        [],
        |row| row.get(0),
    )?;
    let (
        queue_has_provider,
        queue_has_attachments,
        queue_has_dispatch_started,
        queue_has_failure_phase,
    ) = if binding_exists {
        let mut stmt = conn.prepare("PRAGMA table_info(agent_prompt_queue)")?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        (
            columns.iter().any(|column| column == "provider"),
            columns.iter().any(|column| column == "attachments_json"),
            columns.iter().any(|column| column == "dispatch_started"),
            columns.iter().any(|column| column == "failure_phase"),
        )
    } else {
        (true, true, true, true)
    };
    if binding_exists && !queue_has_provider {
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             DROP INDEX IF EXISTS idx_agent_prompt_queue_fifo;
             ALTER TABLE agent_prompt_queue RENAME TO agent_prompt_queue_legacy;
             ALTER TABLE agent_thread_bindings RENAME TO agent_thread_bindings_legacy;
             CREATE TABLE agent_thread_bindings (
                ecky_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
                provider TEXT NOT NULL,
                external_thread_id TEXT NOT NULL,
                external_title TEXT NOT NULL,
                external_cwd TEXT NOT NULL,
                bootstrap_version INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY(ecky_thread_id, provider),
                UNIQUE(provider, external_thread_id)
             );
             INSERT INTO agent_thread_bindings
                SELECT ecky_thread_id, provider, external_thread_id, external_title,
                       external_cwd, bootstrap_version, created_at, updated_at
                FROM agent_thread_bindings_legacy;
             CREATE TABLE agent_prompt_queue (
                id TEXT PRIMARY KEY,
                ecky_thread_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                prompt_text TEXT NOT NULL,
                attachments_json TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL CHECK(status IN ('queued', 'sending', 'failed')),
                dispatch_started INTEGER NOT NULL DEFAULT 0,
                failure_phase TEXT,
                error TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY(ecky_thread_id, provider)
                    REFERENCES agent_thread_bindings(ecky_thread_id, provider) ON DELETE CASCADE
             );
             INSERT INTO agent_prompt_queue
                (id, ecky_thread_id, provider, prompt_text, attachments_json, status, error, created_at, updated_at)
                SELECT q.id, q.ecky_thread_id, b.provider, q.prompt_text, '[]', q.status,
                       q.error, q.created_at, q.updated_at
                FROM agent_prompt_queue_legacy q
                JOIN agent_thread_bindings_legacy b ON b.ecky_thread_id = q.ecky_thread_id;
             DROP TABLE agent_prompt_queue_legacy;
             DROP TABLE agent_thread_bindings_legacy;
             PRAGMA foreign_keys = ON;",
        )?;
    }
    if queue_has_provider && !queue_has_attachments {
        conn.execute(
            "ALTER TABLE agent_prompt_queue ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    if queue_has_provider && !queue_has_dispatch_started {
        conn.execute(
            "ALTER TABLE agent_prompt_queue ADD COLUMN dispatch_started INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if queue_has_provider && !queue_has_failure_phase {
        conn.execute(
            "ALTER TABLE agent_prompt_queue ADD COLUMN failure_phase TEXT",
            [],
        )?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_thread_bindings (
            ecky_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            external_thread_id TEXT NOT NULL,
            external_title TEXT NOT NULL,
            external_cwd TEXT NOT NULL,
            bootstrap_version INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(ecky_thread_id, provider),
            UNIQUE(provider, external_thread_id)
        );
        CREATE TABLE IF NOT EXISTS agent_prompt_queue (
            id TEXT PRIMARY KEY,
            ecky_thread_id TEXT NOT NULL,
            provider TEXT NOT NULL,
            prompt_text TEXT NOT NULL,
            attachments_json TEXT NOT NULL DEFAULT '[]',
            status TEXT NOT NULL CHECK(status IN ('queued', 'sending', 'failed')),
            dispatch_started INTEGER NOT NULL DEFAULT 0,
            failure_phase TEXT,
            error TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY(ecky_thread_id, provider)
                REFERENCES agent_thread_bindings(ecky_thread_id, provider) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_agent_prompt_queue_fifo
            ON agent_prompt_queue(ecky_thread_id, provider, created_at ASC, id ASC);
        CREATE TABLE IF NOT EXISTS agent_provider_process_leases (
            queue_id TEXT PRIMARY KEY REFERENCES agent_prompt_queue(id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            external_thread_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            process_id INTEGER NOT NULL,
            process_group_id INTEGER,
            executable TEXT NOT NULL,
            started_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS agent_provider_messages (
            id TEXT PRIMARY KEY,
            ecky_thread_id TEXT NOT NULL,
            provider TEXT NOT NULL,
            external_thread_id TEXT NOT NULL,
            role TEXT NOT NULL CHECK(role IN ('user', 'assistant')),
            content TEXT NOT NULL,
            attachments_json TEXT NOT NULL DEFAULT '[]',
            status TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(ecky_thread_id, provider)
                REFERENCES agent_thread_bindings(ecky_thread_id, provider) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_agent_provider_messages_page
            ON agent_provider_messages(ecky_thread_id, provider, created_at DESC, id DESC);
        CREATE TABLE IF NOT EXISTS agent_thread_binding_lineage (
            ecky_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            external_thread_id TEXT NOT NULL,
            activated_at INTEGER NOT NULL,
            superseded_at INTEGER,
            superseded_reason TEXT,
            PRIMARY KEY(provider, external_thread_id)
        );
        CREATE INDEX IF NOT EXISTS idx_agent_thread_binding_lineage
            ON agent_thread_binding_lineage(ecky_thread_id, provider, activated_at ASC);
        INSERT OR IGNORE INTO agent_thread_binding_lineage (
            ecky_thread_id, provider, external_thread_id, activated_at,
            superseded_at, superseded_reason
        )
        SELECT ecky_thread_id, provider, external_thread_id, created_at, NULL, NULL
        FROM agent_thread_bindings;",
    )?;
    let message_columns = conn
        .prepare("PRAGMA table_info(agent_provider_messages)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !message_columns
        .iter()
        .any(|column| column == "attachments_json")
    {
        conn.execute(
            "ALTER TABLE agent_provider_messages ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    let thread_columns = conn
        .prepare("PRAGMA table_info(threads)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if ["title", "created_at", "updated_at"]
        .iter()
        .all(|required| thread_columns.iter().any(|column| column == required))
    {
        backfill_provider_thread_projection(conn)?;
    }
    Ok(())
}

pub fn backfill_provider_thread_projection(conn: &Connection) -> rusqlite::Result<()> {
    let legacy_titles = {
        let mut statement = conn.prepare(
            "SELECT threads.id, threads.title, (
                 SELECT content
                 FROM agent_provider_messages
                 WHERE ecky_thread_id = threads.id
                   AND role = 'user'
                   AND trim(content) != ''
                 ORDER BY created_at ASC, id ASC
                 LIMIT 1
             ), threads.created_at, threads.updated_at
             FROM threads
             WHERE EXISTS (
                   SELECT 1 FROM agent_provider_messages
                   WHERE ecky_thread_id = threads.id
               )",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (thread_id, existing_title, content, created_at, updated_at) in legacy_titles {
        let Some(content) = content else {
            continue;
        };
        let raw_title = provider_thread_title(&content);
        let public_title = provider_thread_title(&public_provider_content("user", content));
        let replace = (existing_title == "Untitled design" && created_at == updated_at)
            || existing_title == raw_title.unwrap_or_default();
        let Some(title) = public_title.filter(|title| replace && *title != existing_title) else {
            continue;
        };
        conn.execute(
            "UPDATE threads SET title = ?1 WHERE id = ?2 AND title = ?3",
            params![title, thread_id, existing_title],
        )?;
    }
    conn.execute(
        "UPDATE threads
         SET updated_at = MAX(
                 updated_at + 1,
                 COALESCE((
                     SELECT MAX(provider_message.created_at)
                     FROM agent_provider_messages AS provider_message
                     WHERE provider_message.ecky_thread_id = threads.id
                 ), updated_at + 1)
             )
         WHERE threads.created_at = threads.updated_at
           AND EXISTS (
               SELECT 1 FROM agent_provider_messages
               WHERE ecky_thread_id = threads.id
           )",
        [],
    )?;
    Ok(())
}

pub fn get_agent_binding_for_provider(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
) -> AppResult<Option<AgentThreadBindingRecord>> {
    conn.query_row(
        "SELECT ecky_thread_id, provider, external_thread_id, external_title, external_cwd,
                bootstrap_version, created_at, updated_at
         FROM agent_thread_bindings
         WHERE ecky_thread_id = ?1 AND provider = ?2",
        params![ecky_thread_id, provider],
        |row| {
            Ok(AgentThreadBindingRecord {
                ecky_thread_id: row.get(0)?,
                provider: row.get(1)?,
                external_thread_id: row.get(2)?,
                external_title: row.get(3)?,
                external_cwd: row.get(4)?,
                bootstrap_version: row.get::<_, i64>(5)? as u32,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .optional()
    .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn get_binding(
    conn: &Connection,
    ecky_thread_id: &str,
) -> AppResult<Option<CodexTakeoverBinding>> {
    Ok(
        get_agent_binding_for_provider(conn, ecky_thread_id, CODEX_PROVIDER_ID)?.map(|binding| {
            CodexTakeoverBinding {
                ecky_thread_id: binding.ecky_thread_id,
                codex_thread_id: binding.external_thread_id,
                label: binding.external_title,
                cwd: binding.external_cwd,
                bootstrap_version: binding.bootstrap_version,
                created_at: binding.created_at,
                updated_at: binding.updated_at,
            }
        }),
    )
}

pub fn get_binding_by_codex_thread_id(
    conn: &Connection,
    codex_thread_id: &str,
) -> AppResult<Option<CodexTakeoverBinding>> {
    let ecky_thread_id = conn
        .query_row(
            "SELECT ecky_thread_id FROM agent_thread_bindings
             WHERE provider = ?1 AND external_thread_id = ?2",
            params![CODEX_PROVIDER_ID, codex_thread_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    ecky_thread_id
        .map(|thread_id| get_binding(conn, &thread_id))
        .transpose()
        .map(Option::flatten)
}

pub fn ensure_external_thread_available(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
    external_thread_id: &str,
) -> AppResult<()> {
    if let Some(existing) = get_agent_binding_for_provider(conn, ecky_thread_id, provider)? {
        if existing.external_thread_id != external_thread_id {
            return Err(AppError::conflict(format!(
                "Ecky thread {ecky_thread_id} already owns {provider} conversation {}; provider mode switching preserves that ownership.",
                existing.external_thread_id
            )));
        }
    }
    let owner: Option<String> = conn
        .query_row(
            "SELECT ecky_thread_id FROM agent_thread_bindings
             WHERE provider = ?1 AND external_thread_id = ?2",
            params![provider, external_thread_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    if let Some(owner) = owner.filter(|owner| owner != ecky_thread_id) {
        return Err(AppError::conflict(format!(
            "{} conversation {external_thread_id} is already owned by Ecky thread {owner}.",
            provider
        )));
    }
    Ok(())
}

pub fn upsert_agent_binding(
    conn: &Connection,
    binding: &AgentThreadBindingRecord,
) -> AppResult<AgentThreadBindingRecord> {
    ensure_external_thread_available(
        conn,
        &binding.ecky_thread_id,
        &binding.provider,
        &binding.external_thread_id,
    )?;
    conn.execute(
        "INSERT INTO agent_thread_bindings (
            ecky_thread_id, provider, external_thread_id, external_title,
            external_cwd, bootstrap_version, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(ecky_thread_id, provider) DO UPDATE SET
            external_title = excluded.external_title,
            external_cwd = excluded.external_cwd,
            bootstrap_version = excluded.bootstrap_version,
            updated_at = excluded.updated_at",
        params![
            binding.ecky_thread_id,
            binding.provider,
            binding.external_thread_id,
            binding.external_title,
            binding.external_cwd,
            binding.bootstrap_version,
            binding.created_at,
            binding.updated_at,
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    conn.execute(
        "INSERT OR IGNORE INTO agent_thread_binding_lineage (
            ecky_thread_id, provider, external_thread_id, activated_at,
            superseded_at, superseded_reason
         ) VALUES (?1, ?2, ?3, ?4, NULL, NULL)",
        params![
            binding.ecky_thread_id,
            binding.provider,
            binding.external_thread_id,
            binding.created_at
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    get_agent_binding_for_provider(conn, &binding.ecky_thread_id, &binding.provider)?.ok_or_else(
        || {
            AppError::persistence(format!(
                "Agent binding for Ecky thread {} disappeared after save.",
                binding.ecky_thread_id
            ))
        },
    )
}

pub fn rotate_agent_binding(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
    new_external_thread_id: &str,
    reason: &str,
    now: i64,
) -> AppResult<AgentThreadBindingRecord> {
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    if let Some(existing) = get_agent_binding_for_provider(&transaction, ecky_thread_id, provider)?
    {
        transaction.execute(
            "UPDATE agent_thread_binding_lineage
             SET superseded_at = ?4, superseded_reason = ?5
             WHERE ecky_thread_id = ?1 AND provider = ?2 AND external_thread_id = ?3 AND superseded_at IS NULL",
            params![ecky_thread_id, provider, existing.external_thread_id, now, reason],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    }

    transaction
        .execute(
            "UPDATE agent_thread_bindings
         SET external_thread_id = ?3, updated_at = ?4
         WHERE ecky_thread_id = ?1 AND provider = ?2",
            params![ecky_thread_id, provider, new_external_thread_id, now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;

    transaction
        .execute(
            "INSERT OR IGNORE INTO agent_thread_binding_lineage (
            ecky_thread_id, provider, external_thread_id, activated_at,
            superseded_at, superseded_reason
         ) VALUES (?1, ?2, ?3, ?4, NULL, NULL)",
            params![ecky_thread_id, provider, new_external_thread_id, now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;

    let binding = get_agent_binding_for_provider(&transaction, ecky_thread_id, provider)?
        .ok_or_else(|| {
            AppError::persistence(format!(
                "Agent binding for Ecky thread {} disappeared after rotate.",
                ecky_thread_id
            ))
        })?;
    transaction
        .commit()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(binding)
}

pub fn writer_conflict_rotated_for_queue(conn: &Connection, queue_id: &str) -> AppResult<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_thread_binding_lineage
         WHERE provider = ?1 AND superseded_reason = ?2)",
        params![CODEX_PROVIDER_ID, format!("writer-conflict:{queue_id}")],
        |row| row.get(0),
    )
    .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn bind_owned_thread(
    conn: &Connection,
    ecky_thread_id: &str,
    external_thread_id: &str,
    ecky_title: &str,
    cwd: &str,
    now: i64,
) -> AppResult<CodexTakeoverBinding> {
    let saved = upsert_agent_binding(
        conn,
        &AgentThreadBindingRecord {
            ecky_thread_id: ecky_thread_id.to_string(),
            provider: CODEX_PROVIDER_ID.to_string(),
            external_thread_id: external_thread_id.to_string(),
            external_title: ecky_title.to_string(),
            external_cwd: cwd.to_string(),
            bootstrap_version: CODEX_BOOTSTRAP_VERSION,
            created_at: now,
            updated_at: now,
        },
    )?;
    Ok(CodexTakeoverBinding {
        ecky_thread_id: saved.ecky_thread_id,
        codex_thread_id: saved.external_thread_id,
        label: saved.external_title,
        cwd: saved.external_cwd,
        bootstrap_version: saved.bootstrap_version,
        created_at: saved.created_at,
        updated_at: saved.updated_at,
    })
}

pub fn list_binding_lineage(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
) -> AppResult<Vec<AgentThreadBindingLineageRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT ecky_thread_id, provider, external_thread_id, activated_at,
                    superseded_at, superseded_reason
             FROM agent_thread_binding_lineage
             WHERE ecky_thread_id = ?1 AND provider = ?2
             ORDER BY activated_at ASC, external_thread_id ASC",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map(params![ecky_thread_id, provider], |row| {
            Ok(AgentThreadBindingLineageRecord {
                ecky_thread_id: row.get(0)?,
                provider: row.get(1)?,
                external_thread_id: row.get(2)?,
                activated_at: row.get(3)?,
                superseded_at: row.get(4)?,
                superseded_reason: row.get(5)?,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

fn is_finished_provider_message_status(status: &str) -> bool {
    matches!(status, "success" | "error" | "interrupted" | "discarded")
}

pub fn is_realtime_delegation(content: &str) -> bool {
    let content = content.trim_matches([' ', '\t', '\r', '\n']);
    content.starts_with("<realtime_delegation>")
        && content.ends_with("</realtime_delegation>")
        && content.contains("<input>")
        && content.contains("</input>")
}

// Apply before LIMIT so legacy transport packets cannot fill a public history page.
// Keep this predicate aligned with is_realtime_delegation; retain raw legacy rows.
const PUBLIC_PROVIDER_MESSAGE_SQL: &str = "NOT (
    provider = 'codex' AND role = 'user'
    AND trim(content, ' ' || char(9) || char(13) || char(10))
        GLOB '<realtime_delegation>*</realtime_delegation>'
    AND instr(content, '<input>') > 0 AND instr(content, '</input>') > 0
)";

pub fn persist_finished_provider_messages(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
    external_thread_id: &str,
    messages: &[CodexDialogueMessage],
) -> AppResult<usize> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let mut persisted = 0;
    let mut activity_at = 0;
    let mut first_user_title = None;
    for message in messages
        .iter()
        .filter(|message| is_finished_provider_message_status(&message.status))
    {
        let content = public_provider_content(&message.role, message.content.clone());
        if provider == CODEX_PROVIDER_ID && message.role == "user" && is_realtime_delegation(&content) {
            continue;
        }
        let changed = tx
            .execute(
                "INSERT INTO agent_provider_messages (
                    id, ecky_thread_id, provider, external_thread_id,
                    role, content, attachments_json, status, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    content = excluded.content,
                    attachments_json = CASE
                        WHEN excluded.attachments_json = '[]'
                        THEN agent_provider_messages.attachments_json
                        ELSE excluded.attachments_json
                    END,
                    status = excluded.status,
                    created_at = excluded.created_at
                 WHERE agent_provider_messages.ecky_thread_id = excluded.ecky_thread_id
                   AND agent_provider_messages.provider = excluded.provider
                   AND agent_provider_messages.external_thread_id = excluded.external_thread_id
                   AND (
                       agent_provider_messages.content != excluded.content
                       OR (excluded.attachments_json != '[]'
                           AND agent_provider_messages.attachments_json != excluded.attachments_json)
                       OR agent_provider_messages.status != excluded.status
                       OR agent_provider_messages.created_at != excluded.created_at
                   )",
                params![
                    message.id,
                    ecky_thread_id,
                    provider,
                    external_thread_id,
                    message.role,
                    content,
                    serde_json::to_string(&message.attachments)
                        .map_err(|error| AppError::persistence(error.to_string()))?,
                    message.status,
                    message.timestamp
                ],
            )
            .map_err(|error| AppError::persistence(error.to_string()))?;
        persisted += changed;
        if changed > 0 {
            activity_at = activity_at.max(message.timestamp);
            first_user_title = first_user_title.or_else(|| {
                (message.role == "user")
                    .then(|| provider_thread_title(&content))
                    .flatten()
            });
        }
    }
    tx.execute(
        "UPDATE threads
         SET updated_at = MAX(updated_at + 1, ?2),
             title = CASE
                 WHEN title = 'Untitled design' AND ?3 IS NOT NULL THEN ?3
                 ELSE title
             END
         WHERE id = ?1 AND ?4 > 0",
        params![
            ecky_thread_id,
            activity_at,
            first_user_title,
            persisted as i64
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    tx.commit()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(persisted)
}

fn provider_thread_title(content: &str) -> Option<String> {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = normalized.chars().take(80).collect::<String>();
    (!title.is_empty()).then_some(title)
}

fn public_provider_content(role: &str, content: String) -> String {
    if role == "user" {
        crate::provider_turn::unwrap_user_message(&content).unwrap_or(content)
    } else {
        content
    }
}

pub fn persist_provider_turn_user_input(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
    external_thread_id: &str,
    turn_id: &str,
    content: &str,
    attachments: &[Attachment],
    timestamp: i64,
) -> AppResult<CodexDialogueMessage> {
    let prefix = format!("codex:{external_thread_id}:{turn_id}:user:");
    let mut stmt = conn
        .prepare(
            "SELECT id
             FROM agent_provider_messages
             WHERE ecky_thread_id = ?1
               AND provider = ?2
               AND external_thread_id = ?3
               AND role = 'user'
               AND substr(id, 1, length(?4)) = ?4",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let ids = stmt
        .query_map(
            params![ecky_thread_id, provider, external_thread_id, prefix],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| AppError::persistence(error.to_string()))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let next_ordinal = ids
        .iter()
        .filter_map(|id| id.strip_prefix(&prefix))
        .filter_map(|ordinal| ordinal.parse::<usize>().ok())
        .max()
        .map_or(0, |ordinal| ordinal + 1);
    let message = CodexDialogueMessage {
        id: format!("{prefix}{next_ordinal}"),
        role: "user".to_string(),
        content: content.to_string(),
        status: "success".to_string(),
        timestamp,
        attachments: attachments.to_vec(),
        provider_event_kind: None,
    };
    persist_finished_provider_messages(
        conn,
        ecky_thread_id,
        provider,
        external_thread_id,
        std::slice::from_ref(&message),
    )?;
    Ok(message)
}

pub fn list_provider_messages(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
    limit: usize,
) -> AppResult<Vec<CodexDialogueMessage>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, role, content, attachments_json, status, created_at
             FROM (
                 SELECT id, role, content, attachments_json, status, created_at
                 FROM agent_provider_messages
                 WHERE ecky_thread_id = ?1 AND provider = ?2
                   AND {PUBLIC_PROVIDER_MESSAGE_SQL}
                 ORDER BY created_at DESC, id DESC
                 LIMIT ?3
             ) recent
             ORDER BY created_at ASC, id ASC"
        ))
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map(params![ecky_thread_id, provider, limit as i64], |row| {
            let role: String = row.get(1)?;
            let content: String = row.get(2)?;
            Ok(CodexDialogueMessage {
                id: row.get(0)?,
                role: role.clone(),
                content: public_provider_content(&role, content),
                attachments: decode_queue_attachments(row.get(3)?)?,
                status: row.get(4)?,
                timestamp: row.get(5)?,
                provider_event_kind: None,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

fn encode_provider_message_cursor(timestamp: i64, id: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{timestamp}\n{id}"))
}

fn decode_provider_message_cursor(cursor: &str) -> AppResult<(i64, String)> {
    let decoded = URL_SAFE_NO_PAD.decode(cursor).map_err(|error| {
        AppError::validation(format!("Invalid provider message cursor: {error}"))
    })?;
    let decoded = String::from_utf8(decoded).map_err(|error| {
        AppError::validation(format!("Invalid provider message cursor: {error}"))
    })?;
    let (timestamp, id) = decoded
        .split_once('\n')
        .ok_or_else(|| AppError::validation("Invalid provider message cursor payload."))?;
    let timestamp = timestamp.parse::<i64>().map_err(|error| {
        AppError::validation(format!(
            "Invalid provider message cursor timestamp: {error}"
        ))
    })?;
    Ok((timestamp, id.to_string()))
}

pub fn provider_message_page(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
    cursor: Option<&str>,
) -> AppResult<CodexMessagePage> {
    const PAGE_SIZE: usize = 30;
    let boundary = cursor.map(decode_provider_message_cursor).transpose()?;
    let boundary_timestamp = boundary.as_ref().map(|(timestamp, _)| *timestamp);
    let boundary_id = boundary.as_ref().map(|(_, id)| id.as_str());
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, role, content, attachments_json, status, created_at
             FROM agent_provider_messages
             WHERE ecky_thread_id = ?1 AND provider = ?2
               AND {PUBLIC_PROVIDER_MESSAGE_SQL}
               AND (?3 IS NULL OR created_at < ?3 OR (created_at = ?3 AND id < ?4))
             ORDER BY created_at DESC, id DESC
             LIMIT 31"
        ))
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map(
            params![ecky_thread_id, provider, boundary_timestamp, boundary_id],
            |row| {
                let role: String = row.get(1)?;
                let content: String = row.get(2)?;
                Ok(CodexDialogueMessage {
                    id: row.get(0)?,
                    role: role.clone(),
                    content: public_provider_content(&role, content),
                    attachments: decode_queue_attachments(row.get(3)?)?,
                    status: row.get(4)?,
                    timestamp: row.get(5)?,
                    provider_event_kind: None,
                })
            },
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let mut messages = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let has_more = messages.len() > PAGE_SIZE;
    messages.truncate(PAGE_SIZE);
    let next_cursor = has_more.then(|| {
        messages
            .last()
            .map(|message| encode_provider_message_cursor(message.timestamp, &message.id))
    });
    messages.reverse();
    Ok(CodexMessagePage {
        messages,
        next_cursor: next_cursor.flatten(),
        backwards_cursor: None,
    })
}

pub fn refresh_binding_metadata(
    conn: &Connection,
    binding: &CodexTakeoverBinding,
    ecky_title: &str,
    cwd: &str,
    now: i64,
) -> AppResult<CodexTakeoverBinding> {
    let saved = upsert_agent_binding(
        conn,
        &AgentThreadBindingRecord {
            ecky_thread_id: binding.ecky_thread_id.clone(),
            provider: CODEX_PROVIDER_ID.to_string(),
            external_thread_id: binding.codex_thread_id.clone(),
            external_title: ecky_title.to_string(),
            external_cwd: cwd.to_string(),
            // Bootstrap version records delivered instructions, not desired
            // metadata. Preserve it until thread/resume succeeds.
            bootstrap_version: binding.bootstrap_version,
            created_at: binding.created_at,
            updated_at: now,
        },
    )?;
    Ok(CodexTakeoverBinding {
        ecky_thread_id: saved.ecky_thread_id,
        codex_thread_id: saved.external_thread_id,
        label: saved.external_title,
        cwd: saved.external_cwd,
        bootstrap_version: saved.bootstrap_version,
        created_at: saved.created_at,
        updated_at: saved.updated_at,
    })
}

pub fn build_provider_handoff_summary(
    canonical_summary: &str,
    messages: &[crate::contracts::CodexDialogueMessage],
) -> String {
    build_provider_handoff_summary_for("CODEX", canonical_summary, messages)
}

pub fn build_provider_handoff_summary_for(
    provider_label: &str,
    canonical_summary: &str,
    messages: &[crate::contracts::CodexDialogueMessage],
) -> String {
    const MAX_HANDOFF_CHARS: usize = 6_000;
    const MAX_MESSAGE_CHARS: usize = 700;
    let mut sections = Vec::new();
    if !canonical_summary.trim().is_empty() {
        sections.push(canonical_summary.trim().to_string());
    }
    let dialogue = messages
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter_map(|message| {
            let content = crate::context::compact_text(&message.content, MAX_MESSAGE_CHARS);
            if content.is_empty() {
                return None;
            }
            let role = if message.role == "assistant" {
                format!("{} ASSISTANT", provider_label.to_uppercase())
            } else {
                format!("{} USER", provider_label.to_uppercase())
            };
            Some(format!("{role}: {content}"))
        })
        .collect::<Vec<_>>();
    if !dialogue.is_empty() {
        sections.push(format!(
            "LATEST PROVIDER HANDOFF (canonical context for API/MCP switching):\n{}",
            dialogue.join("\n")
        ));
    }
    let joined = sections.join("\n\n");
    if joined.chars().count() <= MAX_HANDOFF_CHARS {
        joined
    } else {
        joined
            .chars()
            .take(MAX_HANDOFF_CHARS.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

pub fn enqueue_prompt(
    conn: &Connection,
    ecky_thread_id: &str,
    prompt_text: &str,
    now: i64,
) -> AppResult<CodexQueuedPrompt> {
    enqueue_prompt_with_attachments(conn, ecky_thread_id, prompt_text, &[], now)
}

pub fn enqueue_prompt_with_attachments(
    conn: &Connection,
    ecky_thread_id: &str,
    prompt_text: &str,
    attachments: &[Attachment],
    now: i64,
) -> AppResult<CodexQueuedPrompt> {
    if prompt_text.trim().is_empty() && attachments.is_empty() {
        return Err(AppError::validation(
            "Codex queued prompt must include text or attachments.",
        ));
    }
    if get_agent_binding_for_provider(conn, ecky_thread_id, CODEX_PROVIDER_ID)?.is_none() {
        return Err(AppError::not_found(format!(
            "Ecky thread {ecky_thread_id} has no Codex takeover binding."
        )));
    }
    let item = CodexQueuedPrompt {
        id: uuid::Uuid::new_v4().to_string(),
        ecky_thread_id: ecky_thread_id.to_string(),
        prompt_text: prompt_text.to_string(),
        attachments: attachments.to_vec(),
        status: "queued".to_string(),
        can_cancel: true,
        error: None,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO agent_prompt_queue (
            id, ecky_thread_id, provider, prompt_text, attachments_json, status, error, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', NULL, ?6, ?6)",
        params![
            item.id,
            item.ecky_thread_id,
            CODEX_PROVIDER_ID,
            item.prompt_text,
            serde_json::to_string(&item.attachments)
                .map_err(|error| AppError::persistence(error.to_string()))?,
            now
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(item)
}

pub fn list_queue(conn: &Connection, ecky_thread_id: &str) -> AppResult<Vec<CodexQueuedPrompt>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, ecky_thread_id, prompt_text, attachments_json, status, error, created_at, updated_at, dispatch_started
             FROM agent_prompt_queue
             WHERE ecky_thread_id = ?1 AND provider = ?2
             ORDER BY created_at ASC, id ASC",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map(params![ecky_thread_id, CODEX_PROVIDER_ID], |row| {
            let status: String = row.get(4)?;
            let dispatch_started: i64 = row.get(8)?;
            Ok(CodexQueuedPrompt {
                id: row.get(0)?,
                ecky_thread_id: row.get(1)?,
                prompt_text: row.get(2)?,
                attachments: decode_queue_attachments(row.get(3)?)?,
                status: status.clone(),
                error: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                can_cancel: status != "sending" || dispatch_started == 0,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn pending_queue_bindings(conn: &Connection, now: i64) -> AppResult<Vec<CodexTakeoverBinding>> {
    let mut stmt = conn
        .prepare(
            "SELECT b.ecky_thread_id, b.external_thread_id, b.external_title,
                    b.external_cwd, b.bootstrap_version, b.created_at, b.updated_at
             FROM agent_thread_bindings b
             JOIN agent_prompt_queue q ON q.id = (
                 SELECT head.id
                 FROM agent_prompt_queue head
                 WHERE head.ecky_thread_id = b.ecky_thread_id AND head.provider = b.provider
                 ORDER BY head.created_at ASC, head.id ASC
                 LIMIT 1
             )
             WHERE b.provider = ?1 AND q.status = 'queued' AND q.updated_at <= ?2
             ORDER BY q.created_at ASC, q.id ASC",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map(params![CODEX_PROVIDER_ID, now], |row| {
            Ok(CodexTakeoverBinding {
                ecky_thread_id: row.get(0)?,
                codex_thread_id: row.get(1)?,
                label: row.get(2)?,
                cwd: row.get(3)?,
                bootstrap_version: row.get::<_, i64>(4)? as u32,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn recover_stale_sending(conn: &Connection, now: i64) -> AppResult<usize> {
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let failed_closed = transaction
        .execute(
            "UPDATE agent_prompt_queue
             SET status = 'failed',
                 error = 'Previous Ecky process exited while Agy delivery was active. Ecky could not verify provider process state; the agent may still be running. Automatic replay disabled to prevent duplicate model work.',
                 updated_at = ?1
             WHERE status = 'sending' AND provider = 'agy'",
            [now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let recovered_codex = transaction
        .execute(
            "UPDATE agent_prompt_queue
             SET status = 'queued', error = NULL, updated_at = ?1
             WHERE status = 'sending' AND provider = 'codex'",
            [now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    transaction
        .commit()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(failed_closed + recovered_codex)
}

pub fn recover_retryable_failures(conn: &Connection, now: i64) -> AppResult<usize> {
    conn.execute(
        "UPDATE agent_prompt_queue
         SET status = 'queued', updated_at = ?1
         WHERE status = 'failed'
           AND coalesce(failure_phase, 'provider-delivery') = 'provider-delivery'
           AND (
               lower(coalesce(error, '')) LIKE '%already has an active writer%'
               OR lower(coalesce(error, '')) LIKE '%already has an active or pending turn%'
               OR (
                   lower(coalesce(error, '')) LIKE '%codex desktop ipc%'
                   AND lower(coalesce(error, '')) LIKE '%timed out%'
               )
               OR lower(coalesce(error, '')) LIKE '%client-disconnected%'
           )",
        [now],
    )
    .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn fail_classifier_queue_item(
    conn: &Connection,
    id: &str,
    error: &str,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE agent_prompt_queue SET status = 'failed', error = ?2, failure_phase = 'jev-classifier', updated_at = ?3 WHERE id = ?1",
        params![id, format!("{JEV_CLASSIFIER_ERROR_PREFIX} {error}"), now],
    )
    .map_err(|db_error| AppError::persistence(db_error.to_string()))?;
    Ok(())
}

pub fn is_retryable_delivery_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    is_active_writer_error(&error)
        || error.contains("already has an active or pending turn")
        || error.contains("client-disconnected")
}

pub fn is_active_writer_error(error: &str) -> bool {
    error
        .to_ascii_lowercase()
        .contains("already has an active writer")
}

pub fn defer_queue_item(conn: &Connection, id: &str, error: &str, retry_at: i64) -> AppResult<()> {
    let changed = conn
        .execute(
            "UPDATE agent_prompt_queue
             SET status = 'queued', error = ?2, updated_at = ?3
             WHERE id = ?1 AND status IN ('queued', 'sending')",
            params![id, error, retry_at],
        )
        .map_err(|db_error| AppError::persistence(db_error.to_string()))?;
    if changed == 1 {
        Ok(())
    } else {
        Err(AppError::conflict(format!(
            "Codex queue item {id} cannot be deferred."
        )))
    }
}

pub fn record_bootstrap_version(
    conn: &Connection,
    ecky_thread_id: &str,
    provider: &str,
    external_thread_id: &str,
    bootstrap_version: u32,
    now: i64,
) -> AppResult<bool> {
    conn.execute(
        "UPDATE agent_thread_bindings
         SET bootstrap_version = ?4, updated_at = ?5
         WHERE ecky_thread_id = ?1 AND provider = ?2 AND external_thread_id = ?3",
        params![
            ecky_thread_id,
            provider,
            external_thread_id,
            bootstrap_version,
            now,
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn queue_head(conn: &Connection, ecky_thread_id: &str) -> AppResult<Option<CodexQueuedPrompt>> {
    conn.query_row(
        "SELECT id, ecky_thread_id, prompt_text, attachments_json, status, error, created_at, updated_at, dispatch_started
         FROM agent_prompt_queue
         WHERE ecky_thread_id = ?1 AND provider = ?2
         ORDER BY created_at ASC, id ASC
         LIMIT 1",
        params![ecky_thread_id, CODEX_PROVIDER_ID],
        |row| {
            let status: String = row.get(4)?;
            let dispatch_started: i64 = row.get(8)?;
            Ok(CodexQueuedPrompt {
                id: row.get(0)?,
                ecky_thread_id: row.get(1)?,
                prompt_text: row.get(2)?,
                attachments: decode_queue_attachments(row.get(3)?)?,
                status: status.clone(),
                error: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                can_cancel: status != "sending" || dispatch_started == 0,
            })
        },
    )
    .optional()
    .map_err(|error| AppError::persistence(error.to_string()))
}

fn decode_queue_attachments(value: String) -> rusqlite::Result<Vec<Attachment>> {
    serde_json::from_str(&value)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}

pub fn claim_queue_item(conn: &Connection, id: &str, now: i64) -> AppResult<bool> {
    let changed = conn
        .execute(
            "UPDATE agent_prompt_queue
             SET status = 'sending', error = NULL, failure_phase = NULL, dispatch_started = 0, updated_at = ?2
             WHERE id = ?1 AND status = 'queued'",
            params![id, now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(changed == 1)
}

pub fn begin_queue_delivery(conn: &Connection, id: &str, now: i64) -> AppResult<bool> {
    let changed = conn
        .execute(
            "UPDATE agent_prompt_queue SET dispatch_started = 1, updated_at = ?2
             WHERE id = ?1 AND status = 'sending' AND dispatch_started = 0",
            params![id, now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(changed == 1)
}

pub fn mark_queue_sending(conn: &Connection, id: &str, now: i64) -> AppResult<()> {
    if claim_queue_item(conn, id, now)? {
        Ok(())
    } else {
        Err(AppError::conflict(format!(
            "Codex queue item {id} is no longer queued."
        )))
    }
}

pub fn complete_queue_item(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM agent_prompt_queue WHERE id = ?1 AND status = 'sending'",
        [id],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(())
}

pub fn fail_queue_item(conn: &Connection, id: &str, error: &str, now: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE agent_prompt_queue
         SET status = 'failed', error = ?2, failure_phase = 'provider-delivery', updated_at = ?3
         WHERE id = ?1",
        params![id, error, now],
    )
    .map_err(|db_error| AppError::persistence(db_error.to_string()))?;
    Ok(())
}

pub fn retry_queue_item(
    conn: &Connection,
    ecky_thread_id: &str,
    id: &str,
    now: i64,
) -> AppResult<()> {
    let head = queue_head(conn, ecky_thread_id)?
        .ok_or_else(|| AppError::not_found(format!("Codex queue item {id} was not found.")))?;
    if head.id != id {
        return Err(AppError::conflict(format!(
            "Codex queue item {id} cannot overtake queue head {}.",
            head.id
        )));
    }
    let changed = conn
        .execute(
            "UPDATE agent_prompt_queue
             SET status = 'queued', error = NULL, failure_phase = NULL, dispatch_started = 0, updated_at = ?3
             WHERE id = ?1 AND ecky_thread_id = ?2 AND status = 'failed'",
            params![id, ecky_thread_id, now],
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    if changed == 1 {
        Ok(())
    } else {
        Err(AppError::conflict(format!(
            "Codex queue item {id} is not failed."
        )))
    }
}

pub fn remove_queue_item(conn: &Connection, ecky_thread_id: &str, id: &str) -> AppResult<()> {
    let status: Option<(String, bool, String)> = conn
        .query_row(
            "SELECT status, dispatch_started, provider FROM agent_prompt_queue WHERE id = ?1 AND ecky_thread_id = ?2",
            params![id, ecky_thread_id],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0, row.get(2)?)),
        )
        .optional()
        .map_err(|error| AppError::persistence(error.to_string()))?;
    match status {
        Some((status, false, provider)) if status == "sending" && provider == CODEX_PROVIDER_ID => {
            conn.execute(
                "DELETE FROM agent_prompt_queue WHERE id = ?1 AND ecky_thread_id = ?2 AND status = 'sending' AND dispatch_started = 0",
                params![id, ecky_thread_id],
            )
            .map_err(|error| AppError::persistence(error.to_string()))?;
            Ok(())
        }
        Some((status, _, _)) if status != "sending" => {
            conn.execute(
                "DELETE FROM agent_prompt_queue WHERE id = ?1 AND ecky_thread_id = ?2",
                params![id, ecky_thread_id],
            )
            .map_err(|error| AppError::persistence(error.to_string()))?;
            Ok(())
        }
        Some(_) => Err(AppError::conflict(format!(
            "Codex queue item {id} has crossed dispatch boundary. Use STOP after turn starts."
        ))),
        None => Err(AppError::not_found(format!(
            "Codex queue item {id} was not found."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_delegation_is_internal_only_for_complete_native_envelopes() {
        assert!(is_realtime_delegation(" \n<realtime_delegation>\n<input>Move mount.</input>\n<transcript_delta>user: Hello</transcript_delta>\n</realtime_delegation>\n"));
        assert!(!is_realtime_delegation("Move mount."));
        assert!(!is_realtime_delegation("Explain <realtime_delegation><input>text</input></realtime_delegation>"));
        assert!(!is_realtime_delegation("<realtime_delegation><input>unfinished"));
        assert!(!is_realtime_delegation("<realtime_delegation>example</realtime_delegation>"));
    }

    #[test]
    fn durable_provider_history_accepts_only_terminal_messages() {
        assert!(is_finished_provider_message_status("success"));
        assert!(is_finished_provider_message_status("error"));
        assert!(is_finished_provider_message_status("interrupted"));
        assert!(!is_finished_provider_message_status("pending"));
        assert!(!is_finished_provider_message_status("working"));
    }

    #[test]
    fn provider_history_cursor_round_trips_timestamp_and_id() {
        let cursor = encode_provider_message_cursor(42, "codex:item-7");
        assert_eq!(
            decode_provider_message_cursor(&cursor).unwrap(),
            (42, "codex:item-7".to_string())
        );
    }

    #[test]
    fn queue_cancel_and_delivery_start_are_atomic_boundary() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads VALUES ('ecky-thread', 'Thread', 1, 1)",
            [],
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        upsert_agent_binding(
            &conn,
            &AgentThreadBindingRecord {
                ecky_thread_id: "ecky-thread".into(),
                provider: CODEX_PROVIDER_ID.into(),
                external_thread_id: "codex-thread".into(),
                external_title: "Thread".into(),
                external_cwd: "/tmp".into(),
                bootstrap_version: CODEX_BOOTSTRAP_VERSION,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let cancelled = enqueue_prompt(&conn, "ecky-thread", "cancel while routing", 2).unwrap();
        assert!(claim_queue_item(&conn, &cancelled.id, 3).unwrap());
        assert!(list_queue(&conn, "ecky-thread").unwrap()[0].can_cancel);
        remove_queue_item(&conn, "ecky-thread", &cancelled.id).unwrap();
        assert!(!begin_queue_delivery(&conn, &cancelled.id, 4).unwrap());

        let delivered = enqueue_prompt(&conn, "ecky-thread", "cross delivery boundary", 5).unwrap();
        assert!(claim_queue_item(&conn, &delivered.id, 6).unwrap());
        assert!(begin_queue_delivery(&conn, &delivered.id, 7).unwrap());
        assert!(!list_queue(&conn, "ecky-thread").unwrap()[0].can_cancel);
        assert!(remove_queue_item(&conn, "ecky-thread", &delivered.id).is_err());
    }

    #[test]
    fn classifier_retry_ownership_uses_persisted_phase_not_error_prefix() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads VALUES ('ecky-thread', 'Thread', 1, 1)",
            [],
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        upsert_agent_binding(
            &conn,
            &AgentThreadBindingRecord {
                ecky_thread_id: "ecky-thread".into(),
                provider: CODEX_PROVIDER_ID.into(),
                external_thread_id: "codex-thread".into(),
                external_title: "Thread".into(),
                external_cwd: "/tmp".into(),
                bootstrap_version: CODEX_BOOTSTRAP_VERSION,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();

        let classifier = enqueue_prompt(&conn, "ecky-thread", "classify", 2).unwrap();
        fail_classifier_queue_item(
            &conn,
            &classifier.id,
            "client-disconnected; already has an active writer",
            3,
        )
        .unwrap();
        conn.execute(
            "UPDATE agent_prompt_queue SET error = 'client-disconnected; already has an active writer' WHERE id = ?1",
            [&classifier.id],
        )
        .unwrap();
        assert_eq!(recover_retryable_failures(&conn, 4).unwrap(), 0);

        let delivery = enqueue_prompt(&conn, "ecky-thread", "deliver", 5).unwrap();
        fail_queue_item(&conn, &delivery.id, "client-disconnected", 6).unwrap();
        assert_eq!(recover_retryable_failures(&conn, 7).unwrap(), 1);
        assert_eq!(
            list_queue(&conn, "ecky-thread").unwrap()[0].id,
            classifier.id
        );
    }

    #[test]
    fn durable_provider_history_round_trips_user_attachments() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads (id, title, created_at, updated_at)
             VALUES ('ecky-thread', 'Untitled design', 1, 1)",
            [],
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        upsert_agent_binding(
            &conn,
            &AgentThreadBindingRecord {
                ecky_thread_id: "ecky-thread".to_string(),
                provider: CODEX_PROVIDER_ID.to_string(),
                external_thread_id: "codex-thread".to_string(),
                external_title: "Thread".to_string(),
                external_cwd: "/tmp".to_string(),
                bootstrap_version: CODEX_BOOTSTRAP_VERSION,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let attachment = Attachment {
            path: "/tmp/reference.png".to_string(),
            name: "reference.png".to_string(),
            explanation: "Face reference".to_string(),
            data_url: None,
            kind: crate::contracts::AttachmentKind::Image,
        };

        persist_provider_turn_user_input(
            &conn,
            "ecky-thread",
            CODEX_PROVIDER_ID,
            "codex-thread",
            "turn-1",
            "Use this image.",
            std::slice::from_ref(&attachment),
            10,
        )
        .unwrap();

        let wrapped = crate::provider_turn::ProviderTurnPolicy::prompt_based()
            .wrap_user_message("Use this image.");
        conn.execute(
            "UPDATE agent_provider_messages SET content = ?1 WHERE id = 'codex:codex-thread:turn-1:user:0'",
            [&wrapped],
        )
        .unwrap();

        let leaked_title = provider_thread_title(&wrapped).unwrap();
        conn.execute(
            "UPDATE threads SET title = ?1, updated_at = updated_at + 1 WHERE id = 'ecky-thread'",
            [leaked_title],
        )
        .unwrap();
        backfill_provider_thread_projection(&conn).unwrap();

        let page = provider_message_page(&conn, "ecky-thread", CODEX_PROVIDER_ID, None).unwrap();
        assert_eq!(page.messages[0].attachments, vec![attachment]);
        assert_eq!(page.messages[0].content, "Use this image.");
        let (title, created_at, updated_at): (String, i64, i64) = conn
            .query_row(
                "SELECT title, created_at, updated_at FROM threads WHERE id = 'ecky-thread'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(title, "Use this image.");
        assert!(updated_at > created_at);

        conn.execute(
            "UPDATE threads SET title = 'Untitled design', updated_at = created_at
             WHERE id = 'ecky-thread'",
            [],
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        let (title, created_at, updated_at): (String, i64, i64) = conn
            .query_row(
                "SELECT title, created_at, updated_at FROM threads WHERE id = 'ecky-thread'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(title, "Use this image.");
        assert!(updated_at > created_at);
    }

    #[test]
    fn metadata_refresh_preserves_undelivered_bootstrap_version() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads (id, title, created_at, updated_at)
             VALUES ('ecky-thread', 'Untitled design', 1, 1)",
            [],
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        let old = upsert_agent_binding(
            &conn,
            &AgentThreadBindingRecord {
                ecky_thread_id: "ecky-thread".to_string(),
                provider: CODEX_PROVIDER_ID.to_string(),
                external_thread_id: "codex-thread".to_string(),
                external_title: "Old title".to_string(),
                external_cwd: "/old".to_string(),
                bootstrap_version: CODEX_BOOTSTRAP_VERSION - 1,
                created_at: 10,
                updated_at: 10,
            },
        )
        .unwrap();
        let binding = CodexTakeoverBinding {
            ecky_thread_id: old.ecky_thread_id,
            codex_thread_id: old.external_thread_id,
            label: old.external_title,
            cwd: old.external_cwd,
            bootstrap_version: old.bootstrap_version,
            created_at: old.created_at,
            updated_at: old.updated_at,
        };

        let refreshed = refresh_binding_metadata(&conn, &binding, "New title", "/new", 20)
            .expect("refresh metadata");
        assert_eq!(refreshed.label, "New title");
        assert_eq!(refreshed.cwd, "/new");
        assert_eq!(refreshed.bootstrap_version, CODEX_BOOTSTRAP_VERSION - 1);

        assert!(record_bootstrap_version(
            &conn,
            "ecky-thread",
            CODEX_PROVIDER_ID,
            "codex-thread",
            CODEX_BOOTSTRAP_VERSION,
            21,
        )
        .unwrap());
        assert_eq!(
            get_binding(&conn, "ecky-thread")
                .unwrap()
                .unwrap()
                .bootstrap_version,
            CODEX_BOOTSTRAP_VERSION
        );
    }
}
