use std::collections::BTreeMap;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::contracts::{AppError, AppResult};
use crate::provider_turn::ProviderTurnIntent;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationResult {
    pub thread_id: String,
    pub provider: String,
    pub request_id: String,
    pub message_id: Option<String>,
    pub intent: String,
    pub action_probabilities: BTreeMap<String, f64>,
    pub accepted_at: i64,
}

pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS jev_classification_results (
            thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            request_id TEXT NOT NULL,
            message_id TEXT,
            intent TEXT NOT NULL,
            action_probabilities_json TEXT NOT NULL DEFAULT '{}',
            accepted_at INTEGER NOT NULL,
            PRIMARY KEY(thread_id, provider, request_id)
        );
        CREATE INDEX IF NOT EXISTS idx_jev_classification_results_thread
            ON jev_classification_results(thread_id, accepted_at);",
    )?;
    let columns = conn
        .prepare("PRAGMA table_info(jev_classification_results)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns
        .iter()
        .any(|column| column == "action_probabilities_json")
    {
        conn.execute("ALTER TABLE jev_classification_results ADD COLUMN action_probabilities_json TEXT NOT NULL DEFAULT '{}'", [])?;
    }
    Ok(())
}

pub fn save_accepted(
    conn: &Connection,
    thread_id: &str,
    provider: &str,
    request_id: &str,
    message_id: Option<&str>,
    intent: ProviderTurnIntent,
    action_probabilities: &BTreeMap<String, f64>,
    accepted_at: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO jev_classification_results
            (thread_id, provider, request_id, message_id, intent, action_probabilities_json, accepted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(thread_id, provider, request_id) DO UPDATE SET
            message_id = COALESCE(excluded.message_id, jev_classification_results.message_id),
            intent = excluded.intent,
            action_probabilities_json = excluded.action_probabilities_json,
            accepted_at = excluded.accepted_at",
        params![
            thread_id,
            provider,
            request_id,
            message_id,
            intent.as_str(),
            serde_json::to_string(action_probabilities).map_err(|error| AppError::persistence(error.to_string()))?,
            accepted_at
        ],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(())
}

pub fn bind_message(
    conn: &Connection,
    thread_id: &str,
    provider: &str,
    request_id: &str,
    message_id: &str,
) -> AppResult<()> {
    conn.execute(
        "UPDATE jev_classification_results SET message_id = ?4
         WHERE thread_id = ?1 AND provider = ?2 AND request_id = ?3",
        params![thread_id, provider, request_id, message_id],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(())
}

pub fn discard_unbound(
    conn: &Connection,
    thread_id: &str,
    provider: &str,
    request_id: &str,
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM jev_classification_results
         WHERE thread_id = ?1 AND provider = ?2 AND request_id = ?3 AND message_id IS NULL",
        params![thread_id, provider, request_id],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(())
}

pub fn list_for_thread(conn: &Connection, thread_id: &str) -> AppResult<Vec<ClassificationResult>> {
    let mut stmt = conn
        .prepare(
            "SELECT thread_id, provider, request_id, message_id, intent, action_probabilities_json, accepted_at
             FROM jev_classification_results WHERE thread_id = ?1
             ORDER BY accepted_at ASC, request_id ASC",
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let rows = stmt
        .query_map([thread_id], |row| {
            Ok(ClassificationResult {
                thread_id: row.get(0)?,
                provider: row.get(1)?,
                request_id: row.get(2)?,
                message_id: row.get(3)?,
                intent: row.get(4)?,
                action_probabilities: serde_json::from_str(&row.get::<_, String>(5)?).map_err(
                    |error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    },
                )?,
                accepted_at: row.get(6)?,
            })
        })
        .map_err(|error| AppError::persistence(error.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| AppError::persistence(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_route_is_sidecar_bound_to_message_without_changing_message_payload() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY);
            INSERT INTO threads (id) VALUES ('thread-1');",
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        assert!(list_for_thread(&conn, "thread-1").unwrap().is_empty());
        save_accepted(
            &conn,
            "thread-1",
            "codex",
            "queue-1",
            None,
            ProviderTurnIntent::Modify,
            &BTreeMap::from([("modify".to_string(), 0.93), ("clarify".to_string(), 0.07)]),
            10,
        )
        .unwrap();
        assert_eq!(
            list_for_thread(&conn, "thread-1").unwrap()[0].message_id,
            None
        );
        bind_message(&conn, "thread-1", "codex", "queue-1", "codex:user:1").unwrap();
        let result = &list_for_thread(&conn, "thread-1").unwrap()[0];
        assert_eq!(result.message_id.as_deref(), Some("codex:user:1"));
        assert_eq!(result.intent, "modify");
        assert_eq!(result.action_probabilities["modify"], 0.93);
        discard_unbound(&conn, "thread-1", "codex", "queue-1").unwrap();
        assert_eq!(list_for_thread(&conn, "thread-1").unwrap().len(), 1);
        save_accepted(
            &conn,
            "thread-1",
            "codex",
            "queue-2",
            None,
            ProviderTurnIntent::Clarify,
            &BTreeMap::from([("clarify".to_string(), 1.0)]),
            11,
        )
        .unwrap();
        discard_unbound(&conn, "thread-1", "codex", "queue-2").unwrap();
        assert_eq!(list_for_thread(&conn, "thread-1").unwrap().len(), 1);
    }
}
