//! Persistent search metadata for immutable component sources. Normal reads
//! touch only this database; source parsing belongs to capture and recovery.

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::component_package_runtime::ExtractedComponentSearchResult;
use crate::contracts::{AppError, AppResult};

pub(crate) const CAPTURE_SCHEMA_VERSION: u32 = 5;

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComponentIndexPhase {
    #[default]
    Ready,
    Indexing,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComponentIndexStatus {
    pub phase: ComponentIndexPhase,
    pub completed: u32,
    pub total: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(crate) struct ComponentLibraryIndex(Connection);

fn persistence(error: impl std::fmt::Display) -> AppError {
    AppError::persistence(format!("Component library index: {error}"))
}

fn search_text(entry: &ExtractedComponentSearchResult, extra: &str) -> String {
    format!(
        "{} {} {} {} {} {} {} {}",
        entry.name,
        entry.one_liner,
        entry.param_keys.join(" "),
        entry.tags.join(" "),
        entry
            .ports
            .iter()
            .flat_map(|port| [port.port_id.as_str(), port.type_id.as_str()])
            .collect::<Vec<_>>()
            .join(" "),
        entry.thread_id.as_deref().unwrap_or_default(),
        entry.thread_title.as_deref().unwrap_or_default(),
        extra,
    )
    .to_lowercase()
}

impl ComponentLibraryIndex {
    pub(crate) fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path).map_err(persistence)?;
        conn.busy_timeout(Duration::from_secs(3))
            .map_err(persistence)?;
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(persistence)?;
        if version > 3 {
            return Err(persistence(format!(
                "unsupported schema version {version}; upgrade Ecky before opening this catalog"
            )));
        }
        if version == 0 {
            conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS component_entries (
               id INTEGER PRIMARY KEY, entry_key TEXT NOT NULL UNIQUE,
               thread_id TEXT, origin TEXT NOT NULL, metadata TEXT NOT NULL,
               search_text TEXT NOT NULL, extra_search TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS component_entries_thread ON component_entries(thread_id);
             CREATE VIRTUAL TABLE IF NOT EXISTS component_search USING fts5(
               search_text, content='component_entries', content_rowid='id', tokenize='trigram'
             );
             CREATE TRIGGER IF NOT EXISTS component_search_insert AFTER INSERT ON component_entries BEGIN
               INSERT INTO component_search(rowid, search_text) VALUES(new.id, new.search_text);
             END;
             CREATE TRIGGER IF NOT EXISTS component_search_delete AFTER DELETE ON component_entries BEGIN
               INSERT INTO component_search(component_search, rowid, search_text) VALUES('delete', old.id, old.search_text);
             END;
             CREATE TRIGGER IF NOT EXISTS component_search_update AFTER UPDATE ON component_entries BEGIN
               INSERT INTO component_search(component_search, rowid, search_text) VALUES('delete', old.id, old.search_text);
               INSERT INTO component_search(rowid, search_text) VALUES(new.id, new.search_text);
             END;
             CREATE TABLE IF NOT EXISTS component_captures (
               thread_id TEXT PRIMARY KEY, message_id TEXT NOT NULL, schema_version INTEGER NOT NULL,
               diagnostics TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS component_index_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             PRAGMA user_version=1;"
        ).map_err(persistence)?;
        }
        if version < 2 {
            crate::catalog::initialize(&conn)?;
            let rows = conn
                .prepare("SELECT metadata FROM component_entries")
                .map_err(persistence)?
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(persistence)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(persistence)?;
            for row in rows {
                crate::catalog::upsert_component(
                    &conn,
                    &serde_json::from_str(&row).map_err(persistence)?,
                )?;
            }
            conn.pragma_update(None, "user_version", 2)
                .map_err(persistence)?;
        }
        if version < 3 {
            conn.execute_batch("CREATE TABLE IF NOT EXISTS installed_package_headers (package_id TEXT NOT NULL,version TEXT NOT NULL,header TEXT NOT NULL,package_digest TEXT NOT NULL,PRIMARY KEY(package_id,version));
                CREATE TABLE IF NOT EXISTS cairn_metadata_cache (url TEXT PRIMARY KEY,etag TEXT,value TEXT NOT NULL);
                PRAGMA user_version=3;").map_err(persistence)?;
        }
        Ok(Self(conn))
    }

    pub(crate) fn retain_threads(&mut self, threads: &[String]) -> AppResult<()> {
        let tx = self.0.transaction().map_err(persistence)?;
        tx.execute_batch("CREATE TEMP TABLE current_threads (id TEXT PRIMARY KEY);")
            .map_err(persistence)?;
        for id in threads {
            tx.execute("INSERT INTO current_threads VALUES (?1)", [id])
                .map_err(persistence)?;
        }
        tx.execute_batch("DELETE FROM catalog_documents WHERE (json_extract(body,'$.kind')='model' OR json_extract(body,'$.reference.origin')='local') AND json_extract(body, '$.reference.threadId') IS NOT NULL AND json_extract(body, '$.reference.threadId') NOT IN (SELECT id FROM current_threads);
            DELETE FROM component_entries WHERE origin='local' AND thread_id NOT IN (SELECT id FROM current_threads);
            DELETE FROM component_captures WHERE thread_id NOT IN (SELECT id FROM current_threads);
            DROP TABLE current_threads;").map_err(persistence)?;
        tx.commit().map_err(persistence)
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.0
    }

    pub(crate) fn packages_imported(&self) -> AppResult<bool> {
        Ok(self
            .0
            .query_row(
                "SELECT 1 FROM component_index_state WHERE key='packageHeadersImported'",
                [],
                |_| Ok(()),
            )
            .optional()
            .map_err(persistence)?
            .is_some())
    }
    pub(crate) fn mark_packages_imported(&self) -> AppResult<()> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO component_index_state VALUES('packageHeadersImported','1')",
                [],
            )
            .map_err(persistence)?;
        Ok(())
    }

    pub(crate) fn needs_bootstrap(&self) -> AppResult<bool> {
        Ok(self
            .0
            .query_row(
                "SELECT 1 FROM component_index_state WHERE key = 'legacyImported'",
                [],
                |_| Ok(()),
            )
            .optional()
            .map_err(persistence)?
            .is_none())
    }

    pub(crate) fn bootstrap(
        &mut self,
        entries: &[ExtractedComponentSearchResult],
    ) -> AppResult<()> {
        let tx = self.0.transaction().map_err(persistence)?;
        for entry in entries {
            Self::write_entry(&tx, entry, "")?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO component_index_state VALUES ('legacyImported', '1')",
            [],
        )
        .map_err(persistence)?;
        tx.commit().map_err(persistence)
    }

    fn write_entry(
        conn: &Connection,
        entry: &ExtractedComponentSearchResult,
        extra: &str,
    ) -> AppResult<()> {
        let key = entry
            .component_id
            .clone()
            .unwrap_or_else(|| format!("{}:{}", entry.origin, entry.name));
        let metadata = serde_json::to_string(entry).map_err(persistence)?;
        conn.execute(
            "INSERT INTO component_entries(entry_key, thread_id, origin, metadata, search_text, extra_search)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(entry_key) DO UPDATE SET thread_id=excluded.thread_id, origin=excluded.origin,
               metadata=excluded.metadata, search_text=excluded.search_text, extra_search=excluded.extra_search",
            params![key, entry.thread_id, entry.origin, metadata, search_text(entry, extra), extra],
        ).map_err(persistence)?;
        crate::catalog::upsert_component(conn, entry)?;
        Ok(())
    }

    pub(crate) fn upsert(
        &self,
        entry: &ExtractedComponentSearchResult,
        extra: &str,
    ) -> AppResult<()> {
        Self::write_entry(&self.0, entry, extra)
    }

    pub(crate) fn publish_capture(
        &mut self,
        thread_id: &str,
        message_id: &str,
        entries: &[(ExtractedComponentSearchResult, String)],
        diagnostics: &[String],
        model: &crate::catalog::CatalogDocument,
    ) -> AppResult<()> {
        let tx = self.0.transaction().map_err(persistence)?;
        tx.execute("DELETE FROM catalog_documents WHERE json_extract(body, '$.reference.threadId')=?1 AND json_extract(body, '$.kind')='component' AND json_extract(body,'$.reference.origin')='local'", [thread_id]).map_err(persistence)?;
        tx.execute(
            "DELETE FROM component_entries WHERE thread_id = ?1 AND origin = 'local'",
            [thread_id],
        )
        .map_err(persistence)?;
        tx.execute("DELETE FROM catalog_documents WHERE json_extract(body, '$.reference.threadId')=?1 AND json_extract(body, '$.kind')='model'", [thread_id]).map_err(persistence)?;
        crate::catalog::upsert(&tx, model)?;
        for (entry, extra) in entries {
            Self::write_entry(&tx, entry, extra)?;
        }
        tx.execute(
            "INSERT INTO component_captures VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(thread_id) DO UPDATE SET message_id=excluded.message_id,
               schema_version=excluded.schema_version, diagnostics=excluded.diagnostics",
            params![
                thread_id,
                message_id,
                CAPTURE_SCHEMA_VERSION,
                serde_json::to_string(diagnostics).map_err(persistence)?
            ],
        )
        .map_err(persistence)?;
        tx.commit().map_err(persistence)
    }

    pub(crate) fn capture_diagnostics(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> AppResult<Option<Vec<String>>> {
        self.0.query_row(
            "SELECT diagnostics FROM component_captures WHERE thread_id=?1 AND message_id=?2 AND schema_version=?3",
            params![thread_id, message_id, CAPTURE_SCHEMA_VERSION], |row| row.get::<_, String>(0),
        ).optional().map_err(persistence)?.map(|raw| serde_json::from_str(&raw).map_err(persistence)).transpose()
    }

    pub(crate) fn diagnostics(&self) -> AppResult<Vec<String>> {
        let mut stmt = self
            .0
            .prepare("SELECT message_id, diagnostics FROM component_captures ORDER BY thread_id")
            .map_err(persistence)?;
        let mut diagnostics = Vec::new();
        for row in stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(persistence)?
        {
            let (message, raw) = row.map_err(persistence)?;
            let items: Vec<String> = serde_json::from_str(&raw).map_err(persistence)?;
            diagnostics.extend(
                items
                    .into_iter()
                    .map(|item| format!("Version {message}: {item}")),
            );
        }
        Ok(diagnostics)
    }

    pub(crate) fn record_failure(
        &self,
        thread_id: &str,
        message_id: &str,
        diagnostic: &str,
    ) -> AppResult<()> {
        self.0
            .execute(
                "INSERT INTO component_captures VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(thread_id) DO UPDATE SET message_id=excluded.message_id,
               schema_version=excluded.schema_version, diagnostics=excluded.diagnostics",
                params![
                    thread_id,
                    message_id,
                    CAPTURE_SCHEMA_VERSION,
                    serde_json::to_string(&[diagnostic]).map_err(persistence)?
                ],
            )
            .map_err(persistence)?;
        Ok(())
    }

    pub(crate) fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> AppResult<Vec<ExtractedComponentSearchResult>> {
        let needle = query.trim().to_lowercase();
        let (predicate, value) = if needle.chars().count() >= 3 {
            (
                "id IN (SELECT rowid FROM component_search WHERE component_search MATCH ?1)",
                format!("\"{}\"", needle.replace('"', "\"\"")),
            )
        } else {
            ("instr(search_text, ?1) > 0", needle)
        };
        let sql = format!("SELECT metadata FROM component_entries WHERE {predicate}
            ORDER BY CASE origin WHEN 'builtin' THEN 0 WHEN 'extracted' THEN 1 ELSE 2 END, entry_key LIMIT ?2");
        let mut stmt = self.0.prepare(&sql).map_err(persistence)?;
        let rows = stmt
            .query_map(
                params![value, i64::try_from(limit).unwrap_or(i64::MAX)],
                |row| row.get::<_, String>(0),
            )
            .map_err(persistence)?;
        rows.map(|row| serde_json::from_str(&row.map_err(persistence)?).map_err(persistence))
            .collect()
    }

    pub(crate) fn latest_revision(&self, component_id: &str) -> AppResult<Option<String>> {
        let raw = self
            .0
            .query_row(
                "SELECT metadata FROM component_entries WHERE entry_key=?1",
                [component_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(persistence)?;
        raw.map(|raw| {
            serde_json::from_str::<ExtractedComponentSearchResult>(&raw).map_err(persistence)
        })
        .transpose()
        .map(|entry| entry.and_then(|entry| entry.revision_digest))
    }

    pub(crate) fn set_thread_title(&self, thread_id: &str, title: &str) -> AppResult<()> {
        let mut stmt = self
            .0
            .prepare("SELECT metadata, extra_search FROM component_entries WHERE thread_id=?1")
            .map_err(persistence)?;
        let rows = stmt
            .query_map([thread_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(persistence)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence)?;
        for (raw, extra) in rows {
            let mut entry: ExtractedComponentSearchResult =
                serde_json::from_str(&raw).map_err(persistence)?;
            if entry.thread_title.as_deref() == Some(title) {
                continue;
            }
            entry.thread_title = Some(title.to_string());
            // Existing dependency terms are retained when provenance changes.
            self.upsert(&entry, &extra)?;
        }
        let mut stmt=self.0.prepare("SELECT body FROM catalog_documents WHERE json_extract(body,'$.kind')='model' AND json_extract(body,'$.reference.threadId')=?1").map_err(persistence)?;
        let models = stmt
            .query_map([thread_id], |row| row.get::<_, String>(0))
            .map_err(persistence)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence)?;
        for body in models {
            let mut document: crate::catalog::CatalogDocument =
                serde_json::from_str(&body).map_err(persistence)?;
            if document.name != title {
                document.name = title.into();
                crate::catalog::upsert(&self.0, &document)?;
            }
        }
        Ok(())
    }

    pub(crate) fn status(&self) -> AppResult<ComponentIndexStatus> {
        let raw = self
            .0
            .query_row(
                "SELECT value FROM component_index_state WHERE key='status'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(persistence)?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(persistence))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    pub(crate) fn set_status(&self, status: &ComponentIndexStatus) -> AppResult<()> {
        self.0
            .execute(
                "INSERT INTO component_index_state VALUES ('status', ?1)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [serde_json::to_string(status).map_err(persistence)?],
            )
            .map_err(persistence)?;
        Ok(())
    }
}

/// Coalesces recovery requests and exposes completion without polling history or source storage.
#[derive(Default)]
pub struct ComponentIndexWorker {
    running: std::sync::atomic::AtomicBool,
    requested: std::sync::atomic::AtomicU8,
    idle: tokio::sync::Notify,
}
impl ComponentIndexWorker {
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Acquire)
            || self.requested.load(std::sync::atomic::Ordering::Acquire) != 0
    }
    pub(crate) fn request(&self, force: bool) -> bool {
        self.requested.fetch_max(
            if force { 2 } else { 1 },
            std::sync::atomic::Ordering::AcqRel,
        );
        !self.running.swap(true, std::sync::atomic::Ordering::AcqRel)
    }
    pub(crate) fn take_request(&self) -> bool {
        self.requested.swap(0, std::sync::atomic::Ordering::AcqRel) == 2
    }
    pub(crate) fn finish_iteration(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.running.store(false, Ordering::Release);
        if self.requested.load(Ordering::Acquire) != 0
            && self
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return true;
        }
        self.idle.notify_waiters();
        false
    }
    pub async fn wait_idle(&self) {
        loop {
            let notified = self.idle.notified();
            if !self.is_running() {
                return;
            }
            notified.await;
        }
    }
}
