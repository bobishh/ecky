//! Persistent per-thread source binding (openspec `thread-source-binding`).
//!
//! This module is a thin SQLite **binding index** layered over the existing
//! `project_mirror` service. `filesystem-project-mirror` already owns the
//! folder, the `model.ecky` working copy, the `ecky-project.edn` manifest
//! (which is the recoverable sidecar), the sync-state classifier, and the
//! single folder watcher. Nothing here duplicates that surface: every
//! file/folder/digest/manifest operation delegates to `project_mirror`.
//!
//! What this module adds (and the mirror alone did not provide):
//!   - a fast SQLite lookup row per thread so the UI/agent does not have to
//!     scan the projects root to find a thread's folder, and so a binding
//!     survives even if the folder is temporarily missing;
//!   - immediate binding on blank-thread creation;
//!   - safe backfill/adoption of existing mirror folders (no second folder);
//!   - a single-direction durable-version refresh that preserves dirty external
//!     bytes while version appends remain lossless and never duplicate content.
//!
//! Sync-direction mapping:
//!   - `FileChanged`        -> external edit -> existing watcher applies it
//!     as a new version.
//!   - `ThreadAdvanced`     -> informational head movement. A dirty file is
//!     preserved and can be appended as the next head.
//!   - `Conflict`           -> informational only; no version-write refusal.
//!
//! See `openspec/changes/thread-source-binding`.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use crate::contracts::{AppError, AppResult};
use crate::models::PathResolver;
use crate::project_mirror::{
    self, ProjectFolderStatus, ProjectManifest, ProjectSyncState, PROJECT_SOURCE_FILE_NAME,
};

/// Blank thread source. Zero invented geometry or parameters.
pub const DEFAULT_THREAD_SOURCE: &str = "";

const LEGACY_DEMO_THREAD_SOURCE: &str = "\
(model\n\
  (params\n\
    (param size 20 :label \"Size\" :min 1 :max 200))\n\
  (part body (box size size size)))\n\
";

pub(crate) fn is_blank_thread_source(source: &str) -> bool {
    source == DEFAULT_THREAD_SOURCE || source == LEGACY_DEMO_THREAD_SOURCE
}

pub(crate) fn migrate_legacy_blank_source(
    conn: &Connection,
    binding: &ThreadSourceBinding,
) -> AppResult<()> {
    let source_path = Path::new(&binding.source_path);
    let source = std::fs::read_to_string(source_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read bound source '{}': {}",
            source_path.display(),
            err
        ))
    })?;
    if source != LEGACY_DEMO_THREAD_SOURCE {
        return Ok(());
    }

    project_mirror::write_bound_source(source_path, DEFAULT_THREAD_SOURCE)?;
    let digest = project_mirror::source_digest(DEFAULT_THREAD_SOURCE);
    let folder = Path::new(&binding.folder_path);
    if let Some(mut manifest) = project_mirror::read_manifest(folder)? {
        manifest.source_digest = digest.clone();
        project_mirror::write_manifest(folder, &manifest)?;
    }
    conn.execute(
        "UPDATE thread_source_bindings
         SET source_digest = ?2, updated_at = CAST(strftime('%s','now') AS INTEGER)
         WHERE thread_id = ?1",
        params![binding.thread_id, digest],
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(())
}

/// Create the `thread_source_bindings` index table if missing. Idempotent.
/// Called from `db::init_db`.
pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thread_source_bindings (
            thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
            folder_path TEXT NOT NULL,
            source_path TEXT NOT NULL UNIQUE,
            source_digest TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_thread_source_bindings_folder
         ON thread_source_bindings(folder_path)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS project_folder_watch_failures (
            thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
            source_digest TEXT NOT NULL,
            error TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFolderWatchFailure {
    pub source_digest: String,
    pub error: String,
}

pub fn get_project_folder_watch_failure(
    conn: &Connection,
    thread_id: &str,
) -> AppResult<Option<ProjectFolderWatchFailure>> {
    conn.query_row(
        "SELECT source_digest, error
         FROM project_folder_watch_failures
         WHERE thread_id = ?1",
        [thread_id],
        |row| {
            Ok(ProjectFolderWatchFailure {
                source_digest: row.get(0)?,
                error: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(|error| AppError::persistence(error.to_string()))
}

pub fn set_project_folder_watch_failure(
    conn: &Connection,
    thread_id: &str,
    source_digest: &str,
    error: &str,
    updated_at: u64,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO project_folder_watch_failures (thread_id, source_digest, error, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(thread_id) DO UPDATE SET
             source_digest = excluded.source_digest,
             error = excluded.error,
             updated_at = excluded.updated_at",
        params![thread_id, source_digest, error, updated_at as i64],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(())
}

pub fn clear_project_folder_watch_failure(conn: &Connection, thread_id: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM project_folder_watch_failures WHERE thread_id = ?1",
        [thread_id],
    )
    .map_err(|error| AppError::persistence(error.to_string()))?;
    Ok(())
}

/// A persistent per-thread binding index row. The folder is owned by the
/// project mirror; this row is a fast pointer plus the last-known digest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSourceBinding {
    pub thread_id: String,
    pub folder_path: String,
    pub source_path: String,
    pub source_digest: String,
    pub created_at: u64,
    pub updated_at: u64,
}

pub fn source_digest(source: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(source.as_bytes()))
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Resolve the bound folder for a thread. The slug is delegated to
/// `project_mirror::project_slug` so a binding and a mirror for the same
/// thread always share one folder.
pub fn binding_folder(
    app: &dyn PathResolver,
    configured_root: Option<&str>,
    title: &str,
    thread_id: &str,
) -> AppResult<PathBuf> {
    let slug = project_mirror::project_slug(title, thread_id);
    project_mirror::project_dir(app, configured_root, &slug)
}

/// Resolve the authoritative folder for a thread, and the existing binding
/// row when one exists.
///
/// A stored binding's `folder_path` is the single source of truth: a thread
/// title rename or a `projectsRoot` change must NOT relocate the folder, or
/// it would strand a pending external edit at the old path and silently
/// re-derive a different (possibly empty) folder. When no binding row exists
/// yet, fall back to deriving a fresh folder from the current title +
/// configured root (so a brand-new thread is created under the current root).
fn authoritative_folder(
    app: &dyn PathResolver,
    conn: &Connection,
    configured_root: Option<&str>,
    title: &str,
    thread_id: &str,
) -> AppResult<(PathBuf, Option<ThreadSourceBinding>)> {
    let existing =
        get_binding(conn, thread_id).map_err(|err| AppError::persistence(err.to_string()))?;
    match existing {
        Some(binding) => {
            let folder = PathBuf::from(binding.folder_path.clone());
            Ok((folder, Some(binding)))
        }
        None => {
            let slug = project_mirror::project_slug(title, thread_id);
            let folder = project_mirror::project_dir(app, configured_root, &slug)?;
            Ok((folder, None))
        }
    }
}

/// Derive a `(projects_root, slug)` pair that reconstructs the exact folder
/// when passed back to `project_mirror::export_project` / `folder_status`, so
/// a title rename or `projectsRoot` change never relocates an already-bound
/// folder. The basename must be a safe slug (mirrors `project_dir`'s check);
/// the parent becomes the projects root. Works for both stored binding
/// folders and freshly-derived ones (`projects_root.join(slug)`).
pub(crate) fn stored_folder_export_args(folder: &Path) -> AppResult<(Option<String>, String)> {
    let slug = folder
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .ok_or_else(|| {
            AppError::validation(format!(
                "Bound folder basename for '{}' is not a safe project slug.",
                folder.display()
            ))
        })?;
    let parent = folder.parent().ok_or_else(|| {
        AppError::validation(format!(
            "Bound folder path '{}' has no parent to reuse as the projects root.",
            folder.display()
        ))
    })?;
    Ok((Some(parent.to_string_lossy().to_string()), slug.to_string()))
}

pub fn get_binding(
    conn: &Connection,
    thread_id: &str,
) -> rusqlite::Result<Option<ThreadSourceBinding>> {
    let row = conn
        .query_row(
            "SELECT thread_id, folder_path, source_path, source_digest, created_at, updated_at
             FROM thread_source_bindings WHERE thread_id = ?1",
            params![thread_id],
            |row| {
                Ok(ThreadSourceBinding {
                    thread_id: row.get(0)?,
                    folder_path: row.get(1)?,
                    source_path: row.get(2)?,
                    source_digest: row.get(3)?,
                    created_at: row.get::<_, i64>(4)? as u64,
                    updated_at: row.get::<_, i64>(5)? as u64,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Resolve a compatibility `project_folder_*` slug back to its exact stored
/// binding. Slugs include a stable thread-id suffix, but roots can change;
/// therefore lookup scans binding rows and compares only the final folder
/// component instead of rebuilding a path under current `projectsRoot`.
pub fn get_binding_by_folder_name(
    conn: &Connection,
    folder_name: &str,
) -> rusqlite::Result<Option<ThreadSourceBinding>> {
    let mut statement = conn.prepare(
        "SELECT thread_id, folder_path, source_path, source_digest, created_at, updated_at
         FROM thread_source_bindings",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(ThreadSourceBinding {
            thread_id: row.get(0)?,
            folder_path: row.get(1)?,
            source_path: row.get(2)?,
            source_digest: row.get(3)?,
            created_at: row.get::<_, i64>(4)? as u64,
            updated_at: row.get::<_, i64>(5)? as u64,
        })
    })?;
    for row in rows {
        let binding = row?;
        let matches = Path::new(&binding.folder_path)
            .file_name()
            .and_then(|value| value.to_str())
            == Some(folder_name);
        if matches {
            return Ok(Some(binding));
        }
    }
    Ok(None)
}

pub fn list_bindings(conn: &Connection) -> rusqlite::Result<Vec<ThreadSourceBinding>> {
    let mut statement = conn.prepare(
        "SELECT thread_id, folder_path, source_path, source_digest, created_at, updated_at
         FROM thread_source_bindings ORDER BY thread_id",
    )?;
    let bindings = statement
        .query_map([], |row| {
            Ok(ThreadSourceBinding {
                thread_id: row.get(0)?,
                folder_path: row.get(1)?,
                source_path: row.get(2)?,
                source_digest: row.get(3)?,
                created_at: row.get::<_, i64>(4)? as u64,
                updated_at: row.get::<_, i64>(5)? as u64,
            })
        })?
        .collect();
    bindings
}

/// Upsert a binding index row from a known folder + digest. The mirror
/// manifest must already exist on disk; callers obtain the folder + digest
/// from `project_mirror`.
pub fn upsert_binding_row(
    conn: &Connection,
    thread_id: &str,
    folder: &Path,
    source_digest: &str,
    existing_created_at: Option<u64>,
) -> rusqlite::Result<ThreadSourceBinding> {
    let previous_digest = conn
        .query_row(
            "SELECT source_digest FROM thread_source_bindings WHERE thread_id = ?1",
            [thread_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let now = now_secs();
    let created_at = existing_created_at.unwrap_or(now);
    let source_path = folder.join(PROJECT_SOURCE_FILE_NAME);
    conn.execute(
        "INSERT INTO thread_source_bindings
            (thread_id, folder_path, source_path, source_digest, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(thread_id) DO UPDATE SET
            folder_path = excluded.folder_path,
            source_path = excluded.source_path,
            source_digest = excluded.source_digest,
            updated_at = excluded.updated_at",
        params![
            thread_id,
            folder.to_string_lossy(),
            source_path.to_string_lossy(),
            source_digest,
            created_at as i64,
            now as i64,
        ],
    )?;
    if previous_digest
        .as_deref()
        .is_some_and(|digest| digest != source_digest)
    {
        conn.execute(
            "UPDATE threads
             SET updated_at = MAX(updated_at + 1, CAST(strftime('%s','now') AS INTEGER))
             WHERE id = ?1",
            [thread_id],
        )?;
    }
    Ok(ThreadSourceBinding {
        thread_id: thread_id.to_string(),
        folder_path: folder.to_string_lossy().to_string(),
        source_path: source_path.to_string_lossy().to_string(),
        source_digest: source_digest.to_string(),
        created_at,
        updated_at: now,
    })
}

pub fn delete_binding(conn: &Connection, thread_id: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "DELETE FROM thread_source_bindings WHERE thread_id = ?1",
        [thread_id],
    )?;
    Ok(changed > 0)
}

/// Index an already-exported mirror folder as this thread's bound source.
/// Preserves the existing `created_at` so re-exports do not reset the
/// binding's creation time. Used by the export handler *after*
/// `project_mirror::export_project` has atomically written the folder, so
/// the binding row digest, the on-disk file digest, and the manifest digest
/// always agree (the digest-safe invariant).
pub fn index_export(
    conn: &Connection,
    thread_id: &str,
    folder: &Path,
    source_digest: &str,
) -> rusqlite::Result<ThreadSourceBinding> {
    let existing_created_at = get_binding(conn, thread_id)?.map(|binding| binding.created_at);
    upsert_binding_row(conn, thread_id, folder, source_digest, existing_created_at)
}

// --- Binding lifecycle: creation, backfill, adopt ------------------------

/// Internal: write the source + manifest to a folder via the mirror, then
/// index the result. Used by both bind (new source) and re-export paths.
fn write_and_index(
    app: &dyn PathResolver,
    conn: &Connection,
    configured_root: Option<&str>,
    thread_id: &str,
    title: &str,
    source: &str,
    message_id: &str,
    model_id: Option<&str>,
) -> AppResult<ThreadSourceBinding> {
    let (folder, existing) = authoritative_folder(app, conn, configured_root, title, thread_id)?;
    let existing_created_at = existing.as_ref().map(|b| b.created_at);
    // Reuse the authoritative folder's parent + basename as the export args so
    // a bound thread always writes back to its stored folder (never a second
    // one), while a brand-new binding is created under the configured root.
    let (export_root, slug) = stored_folder_export_args(&folder)?;

    let (folder, manifest) = project_mirror::export_project(
        app,
        &project_mirror::ExportProjectRequest {
            slug: &slug,
            thread_id,
            message_id,
            model_id,
            source,
            projects_root: export_root.as_deref(),
        },
    )?;
    upsert_binding_row(
        conn,
        thread_id,
        &folder,
        &manifest.source_digest,
        existing_created_at,
    )
    .map_err(AppError::from)
}

/// Bind a brand-new blank thread immediately: writes the default source +
/// manifest via the mirror, then indexes the binding. Idempotent: an existing
/// binding is returned unchanged so repeated binds never rewrite the file.
///
/// `message_id` is the placeholder for the blank thread before its first version
/// (empty is allowed; the manifest `messageId` is rebased on the first real
/// append through `refresh_on_version_append`).
pub fn bind_new_thread(
    app: &dyn PathResolver,
    conn: &Connection,
    configured_root: Option<&str>,
    thread_id: &str,
    title: &str,
) -> AppResult<ThreadSourceBinding> {
    if let Some(existing) =
        get_binding(conn, thread_id).map_err(|err| AppError::persistence(err.to_string()))?
    {
        return Ok(existing);
    }
    write_and_index(
        app,
        conn,
        configured_root,
        thread_id,
        title,
        DEFAULT_THREAD_SOURCE,
        "",
        None,
    )
}

/// Backfill/adopt a binding for an existing thread. If a mirror folder with
/// a manifest already exists for this thread, adopt it as-is (no rewrite);
/// otherwise seed it from the provided `existing_source` (or the default).
/// Idempotent.
pub fn backfill_binding(
    app: &dyn PathResolver,
    conn: &Connection,
    configured_root: Option<&str>,
    thread_id: &str,
    title: &str,
    existing_source: Option<&str>,
    message_id: Option<&str>,
    model_id: Option<&str>,
) -> AppResult<ThreadSourceBinding> {
    if let Some(existing) =
        get_binding(conn, thread_id).map_err(|err| AppError::persistence(err.to_string()))?
    {
        return Ok(existing);
    }

    let slug = project_mirror::project_slug(title, thread_id);
    let folder = project_mirror::project_dir(app, configured_root, &slug)?;
    if let Some(manifest) = project_mirror::read_manifest(&folder)? {
        // Adopt an existing mirror folder without rewriting its bytes.
        return upsert_binding_row(conn, thread_id, &folder, &manifest.source_digest, None)
            .map_err(AppError::from);
    }

    let source = existing_source.unwrap_or(DEFAULT_THREAD_SOURCE);
    write_and_index(
        app,
        conn,
        configured_root,
        thread_id,
        title,
        source,
        message_id.unwrap_or(""),
        model_id,
    )
}

/// Ensure OPEN FILE has a bound source, then refresh it only when the working
/// copy still matches Ecky's last manifest. A mismatched file is a pending
/// external edit: preserve it and return the stored binding so the editor
/// opens the exact file the user changed.
pub fn prepare_editor_source(
    app: &dyn PathResolver,
    conn: &Connection,
    configured_root: Option<&str>,
    thread_id: &str,
    title: &str,
    source: &str,
    message_id: &str,
    model_id: Option<&str>,
) -> AppResult<ThreadSourceBinding> {
    let binding = backfill_binding(
        app,
        conn,
        configured_root,
        thread_id,
        title,
        Some(source),
        Some(message_id),
        model_id,
    )?;
    let folder = PathBuf::from(&binding.folder_path);
    let manifest = project_mirror::read_manifest(&folder)?;
    let disk_digest = project_mirror::read_project_source(&folder)?
        .as_deref()
        .map(project_mirror::source_digest);
    let has_pending_external_edit = manifest
        .as_ref()
        .is_some_and(|manifest| disk_digest.as_deref() != Some(manifest.source_digest.as_str()));

    if has_pending_external_edit {
        return Ok(binding);
    }

    write_and_index(
        app,
        conn,
        configured_root,
        thread_id,
        title,
        source,
        message_id,
        model_id,
    )
}

// --- Durable-version mirror refresh -------------------------------------

/// Refresh the bound source after an Ecky-originated version append.
///
/// Dirty external bytes are never overwritten. When the appended durable
/// source exactly matches those bytes, the edit has already entered history:
/// rebase the manifest instead of leaving the watcher to render it again.
/// A different dirty source remains pending and is preserved unchanged.
pub fn refresh_on_version_append(
    app: &dyn PathResolver,
    conn: &Connection,
    configured_root: Option<&str>,
    thread_id: &str,
    title: &str,
    source: &str,
    message_id: &str,
    model_id: Option<&str>,
    thread_head_message_id: Option<&str>,
) -> AppResult<ThreadSourceBinding> {
    let _ = thread_head_message_id; // classification is handled by the caller/watcher.

    let (folder, existing) = authoritative_folder(app, conn, configured_root, title, thread_id)?;
    let manifest = project_mirror::read_manifest(&folder)?;
    let on_disk = project_mirror::read_project_source(&folder)?;
    let disk_digest = on_disk.as_deref().map(project_mirror::source_digest);

    // First append on an unbound thread, or folder with no manifest yet:
    // seed the binding with the appended source as baseline.
    if existing.is_none() || manifest.is_none() {
        return write_and_index(
            app,
            conn,
            configured_root,
            thread_id,
            title,
            source,
            message_id,
            model_id,
        );
    }

    let manifest = manifest.expect("manifest present after the None check");
    // Preserve a different pending external edit. If the appended durable
    // version is byte-identical to the dirty working copy, rebasing is safe
    // and prevents the watcher from treating known source as a new edit after
    // every process restart.
    let clean = disk_digest.as_deref() == Some(manifest.source_digest.as_str());
    let appended_digest = project_mirror::source_digest(source);
    let appended_matches_disk = disk_digest.as_deref() == Some(appended_digest.as_str());
    if !clean && !appended_matches_disk {
        return existing.ok_or_else(|| {
            AppError::persistence(format!(
                "Missing source binding for dirty thread '{thread_id}'."
            ))
        });
    }

    write_and_index(
        app,
        conn,
        configured_root,
        thread_id,
        title,
        source,
        message_id,
        model_id,
    )
}

/// Read-only binding status for the UI/agent: resolves the index row (or the
/// folder it would live in), delegates classification to the mirror, and
/// returns absolute paths + state.
#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceBindingInfo {
    pub state: ProjectSyncState,
    /// Resolved absolute source path. Present even when unbound.
    pub source_path: String,
    /// Resolved absolute folder path.
    pub folder_path: String,
    /// `true` when no binding index row exists yet.
    pub unbound: bool,
    /// Mirror manifest from the folder, when present (the sidecar).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<ProjectManifest>,
    /// Digest of the bytes currently on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_digest: Option<String>,
    /// Thread head message id supplied by the caller for classification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_head_message_id: Option<String>,
}

/// Resolve the binding info for a thread. Optionally backfill/adopt a
/// missing binding before classifying.
pub fn binding_info(
    app: &dyn PathResolver,
    conn: &Connection,
    configured_root: Option<&str>,
    thread_id: &str,
    title: &str,
    thread_head_message_id: Option<&str>,
    backfill_if_missing: bool,
    backfill_source: Option<&str>,
) -> AppResult<SourceBindingInfo> {
    if backfill_if_missing {
        backfill_binding(
            app,
            conn,
            configured_root,
            thread_id,
            title,
            backfill_source,
            thread_head_message_id,
            None,
        )?;
    }

    // The bound folder is authoritative: report the STORED folder + source
    // path so a title rename or `projectsRoot` change does not point the
    // UI/agent at a different folder. Unbound threads still derive from the
    // current title under the configured root.
    let (folder, existing) = authoritative_folder(app, conn, configured_root, title, thread_id)?;
    let (status_root, status_slug) = stored_folder_export_args(&folder)?;
    let source_path = folder.join(PROJECT_SOURCE_FILE_NAME);

    let status: ProjectFolderStatus = project_mirror::folder_status(
        app,
        status_root.as_deref(),
        &status_slug,
        thread_head_message_id,
    )?;

    Ok(SourceBindingInfo {
        state: status.state,
        source_path: source_path.to_string_lossy().to_string(),
        folder_path: status.folder,
        unbound: existing.is_none(),
        manifest: status.manifest,
        file_digest: status.file_digest,
        thread_head_message_id: status.thread_head_message_id,
    })
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::persistence(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_mirror::PROJECT_MANIFEST_FILE_NAME;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
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

    fn temp_resolver(name: &str) -> TestResolver {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        TestResolver {
            root: std::env::temp_dir().join(format!("ecky-binding-{name}-{nonce}")),
        }
    }

    fn temp_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE thread_source_bindings (
                thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
                folder_path TEXT NOT NULL,
                source_path TEXT NOT NULL UNIQUE,
                source_digest TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    fn seed_thread(conn: &Connection, thread_id: &str) {
        conn.execute(
            "INSERT INTO threads (id, title, updated_at) VALUES (?1, ?2, 100)",
            params![thread_id, "Bracket"],
        )
        .unwrap();
    }

    fn folder_of(binding: &ThreadSourceBinding) -> PathBuf {
        PathBuf::from(&binding.folder_path)
    }

    #[test]
    fn bind_new_thread_writes_default_source_manifest_and_index_row() {
        let resolver = temp_resolver("bind");
        let conn = temp_db_with_schema();

        let conn = conn;
        let conn = &conn;
        seed_thread(conn, "thread-1");
        let binding =
            bind_new_thread(&resolver, conn, None, "thread-1", "Film Adapter v2!").expect("bind");

        let folder = folder_of(&binding);
        assert!(folder.join(PROJECT_SOURCE_FILE_NAME).is_file());
        assert!(folder.join(PROJECT_MANIFEST_FILE_NAME).is_file());
        assert_eq!(
            std::fs::read_to_string(folder.join(PROJECT_SOURCE_FILE_NAME)).unwrap(),
            DEFAULT_THREAD_SOURCE
        );
        assert_eq!(binding.thread_id, "thread-1");
        assert_eq!(binding.source_digest, source_digest(DEFAULT_THREAD_SOURCE));
        assert!(binding.source_path.ends_with(PROJECT_SOURCE_FILE_NAME));

        let row = get_binding(conn, "thread-1").unwrap().expect("row");
        assert_eq!(row.source_digest, binding.source_digest);

        // Manifest is the mirror manifest (kebab-case EDN).
        let raw = std::fs::read_to_string(folder.join(PROJECT_MANIFEST_FILE_NAME)).unwrap();
        assert!(raw.contains(":source-digest"), "{raw}");
        assert!(raw.contains(":thread-id"), "{raw}");
    }

    fn temp_db_with_schema() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE thread_source_bindings (
                thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
                folder_path TEXT NOT NULL,
                source_path TEXT NOT NULL UNIQUE,
                source_digest TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn bind_new_thread_is_idempotent_for_same_thread() {
        let resolver = temp_resolver("idem");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");
        let first = bind_new_thread(&resolver, conn, None, "thread-1", "Kit").expect("first");
        let second = bind_new_thread(&resolver, conn, None, "thread-1", "Kit").expect("second");
        assert_eq!(first, second, "second bind returns existing row unchanged");
    }

    #[test]
    fn binding_lookup_by_folder_name_retains_original_root() {
        let resolver = temp_resolver("lookup-folder");
        let conn = temp_db_with_schema();
        seed_thread(&conn, "thread-1");
        let binding = bind_new_thread(&resolver, &conn, None, "thread-1", "Bracket").expect("bind");
        let slug = Path::new(&binding.folder_path)
            .file_name()
            .and_then(|value| value.to_str())
            .expect("slug");

        let found = get_binding_by_folder_name(&conn, slug)
            .unwrap()
            .expect("binding by slug");
        assert_eq!(found.folder_path, binding.folder_path);
        assert_eq!(found.source_path, binding.source_path);
    }

    #[test]
    fn backfill_adopts_existing_mirror_folder_without_rewrite() {
        let resolver = temp_resolver("adopt");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");

        // Simulate an existing mirror folder (e.g. from filesystem-project-mirror)
        // with a hand-written source + manifest.
        let title = "Bracket";
        let slug = project_mirror::project_slug(title, "thread-1");
        let folder = project_mirror::project_dir(&resolver, None, &slug).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let existing_source = "(model (part body (box 7 7 7)))";
        std::fs::write(folder.join(PROJECT_SOURCE_FILE_NAME), existing_source).unwrap();
        let manifest = ProjectManifest {
            schema_version: 1,
            project_id: "proj-existing".into(),
            thread_id: "thread-1".into(),
            message_id: "msg-pre".into(),
            model_id: None,
            source_digest: source_digest(existing_source),
            exported_at: 1234,
        };
        project_mirror::write_manifest(&folder, &manifest).unwrap();

        // Backfill must ADOPT the folder bytes verbatim (not rewrite default).
        let binding = backfill_binding(
            &resolver,
            conn,
            None,
            "thread-1",
            title,
            Some(DEFAULT_THREAD_SOURCE),
            Some("msg-pre"),
            None,
        )
        .expect("backfill");
        assert_eq!(
            std::fs::read_to_string(folder.join(PROJECT_SOURCE_FILE_NAME)).unwrap(),
            existing_source,
            "existing bytes preserved on adopt"
        );
        assert_eq!(binding.source_digest, source_digest(existing_source));
    }

    #[test]
    fn backfill_seeds_default_when_no_folder_exists() {
        let resolver = temp_resolver("seed");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");

        let binding =
            backfill_binding(&resolver, conn, None, "thread-1", "Ghost", None, None, None)
                .expect("backfill");
        let on_disk =
            std::fs::read_to_string(folder_of(&binding).join(PROJECT_SOURCE_FILE_NAME)).unwrap();
        assert_eq!(on_disk, DEFAULT_THREAD_SOURCE);
    }

    #[test]
    fn refresh_on_version_append_creates_binding_on_first_version() {
        let resolver = temp_resolver("refresh-first");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");

        let first = refresh_on_version_append(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            "(model (part body (box 1 1 1)))",
            "msg-1",
            Some("model-1"),
            Some("msg-1"),
        )
        .expect("first version binds");
        assert_eq!(
            first.source_digest,
            source_digest("(model (part body (box 1 1 1)))")
        );
        let manifest = project_mirror::read_manifest(&folder_of(&first))
            .unwrap()
            .expect("manifest");
        assert_eq!(manifest.message_id, "msg-1");
        assert_eq!(manifest.model_id.as_deref(), Some("model-1"));
    }

    #[test]
    fn refresh_on_version_append_writes_clean_file_and_rebases_manifest() {
        let resolver = temp_resolver("refresh-clean");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");
        refresh_on_version_append(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            "(model (part body (box 1 1 1)))",
            "msg-1",
            Some("model-1"),
            Some("msg-1"),
        )
        .unwrap();

        let next = "(model (part body (box 2 2 2)))";
        let refreshed = refresh_on_version_append(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            next,
            "msg-2",
            Some("model-2"),
            Some("msg-2"),
        )
        .expect("clean refresh");
        assert_eq!(refreshed.source_digest, source_digest(next));
        let on_disk =
            std::fs::read_to_string(folder_of(&refreshed).join(PROJECT_SOURCE_FILE_NAME)).unwrap();
        assert_eq!(on_disk, next);
        let manifest = project_mirror::read_manifest(&folder_of(&refreshed))
            .unwrap()
            .expect("manifest");
        assert_eq!(manifest.message_id, "msg-2");
        assert_eq!(manifest.model_id.as_deref(), Some("model-2"));
    }

    #[test]
    fn refresh_on_version_append_preserves_dirty_file_on_digest_mismatch() {
        let resolver = temp_resolver("refresh-guard");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");
        let binding = refresh_on_version_append(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            "(model (part body (box 1 1 1)))",
            "msg-1",
            Some("model-1"),
            Some("msg-1"),
        )
        .unwrap();

        // Simulate an unsynced external edit on disk.
        std::fs::write(
            folder_of(&binding).join(PROJECT_SOURCE_FILE_NAME),
            "(model (part body (box 9 9 9)))",
        )
        .unwrap();

        let refreshed = refresh_on_version_append(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            "(model (part body (box 2 2 2)))",
            "msg-2",
            Some("model-2"),
            Some("msg-1"),
        )
        .expect("dirty file must not turn a durable append into an error");
        assert_eq!(refreshed.source_digest, binding.source_digest);

        // File bytes + binding digest unchanged (no clobber, no version).
        let on_disk =
            std::fs::read_to_string(folder_of(&binding).join(PROJECT_SOURCE_FILE_NAME)).unwrap();
        assert!(on_disk.contains("box 9 9 9"));
        let row = get_binding(conn, "thread-1").unwrap().unwrap();
        assert_eq!(row.source_digest, binding.source_digest);
    }

    #[test]
    fn refresh_on_version_append_rebases_dirty_file_matching_durable_source() {
        let resolver = temp_resolver("refresh-matching-dirty");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");
        let binding = refresh_on_version_append(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            "(model (part body (box 1 1 1)))",
            "msg-1",
            Some("model-1"),
            Some("msg-1"),
        )
        .unwrap();

        let dirty_source = "(model (part body (box 9 9 9)))";
        std::fs::write(
            folder_of(&binding).join(PROJECT_SOURCE_FILE_NAME),
            dirty_source,
        )
        .unwrap();

        let refreshed = refresh_on_version_append(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            dirty_source,
            "msg-2",
            Some("model-2"),
            Some("msg-2"),
        )
        .expect("matching durable source rebases dirty working copy");

        assert_eq!(refreshed.source_digest, source_digest(dirty_source));
        let manifest = project_mirror::read_manifest(&folder_of(&refreshed))
            .unwrap()
            .expect("manifest");
        assert_eq!(manifest.message_id, "msg-2");
        assert_eq!(manifest.model_id.as_deref(), Some("model-2"));
        assert_eq!(manifest.source_digest, source_digest(dirty_source));
        let on_disk =
            std::fs::read_to_string(folder_of(&refreshed).join(PROJECT_SOURCE_FILE_NAME)).unwrap();
        assert_eq!(on_disk, dirty_source);
    }

    #[test]
    fn binding_info_reports_clean_file_changed_unbound() {
        let resolver = temp_resolver("info");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");
        seed_thread(conn, "thread-2");

        // Unbound: no folder, no row.
        let info = binding_info(
            &resolver,
            conn,
            None,
            "thread-2",
            "Ghost",
            Some("msg-1"),
            false,
            None,
        )
        .unwrap();
        assert_eq!(info.state, ProjectSyncState::Missing);
        assert!(info.unbound);

        let binding = bind_new_thread(&resolver, conn, None, "thread-1", "Bracket").unwrap();
        let info = binding_info(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            Some(""),
            false,
            None,
        )
        .unwrap();
        assert_eq!(info.state, ProjectSyncState::Clean);
        assert_eq!(info.source_path, binding.source_path);

        // External edit -> FileChanged.
        std::fs::write(
            folder_of(&binding).join(PROJECT_SOURCE_FILE_NAME),
            "(model (part body (box 9 9 9)))",
        )
        .unwrap();
        let info = binding_info(
            &resolver,
            conn,
            None,
            "thread-1",
            "Bracket",
            Some(""),
            false,
            None,
        )
        .unwrap();
        assert_eq!(info.state, ProjectSyncState::FileChanged);
    }

    #[test]
    fn binding_info_backfill_adopts_then_clean() {
        let resolver = temp_resolver("info-backfill");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");

        // First call backfills + classifies clean.
        let info = binding_info(
            &resolver, conn, None, "thread-1", "Bracket", None, true, None,
        )
        .unwrap();
        assert_eq!(info.state, ProjectSyncState::Clean);
        assert!(!info.unbound);
    }

    #[test]
    fn delete_binding_removes_row_only() {
        let resolver = temp_resolver("delete");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");
        let binding = bind_new_thread(&resolver, conn, None, "thread-1", "Bracket").unwrap();
        assert!(delete_binding(conn, "thread-1").unwrap());
        assert!(get_binding(conn, "thread-1").unwrap().is_none());
        assert!(!delete_binding(conn, "thread-1").unwrap());
        // Folder is owned by the mirror; deleting the index row leaves it.
        assert!(folder_of(&binding).join(PROJECT_SOURCE_FILE_NAME).is_file());
    }

    #[test]
    fn default_thread_source_is_empty_without_invented_geometry() {
        assert!(DEFAULT_THREAD_SOURCE.is_empty());
        assert!(is_blank_thread_source(LEGACY_DEMO_THREAD_SOURCE));
    }

    // --- retained-path / backfill lifecycle (thread-source-binding) --------
    // A stored binding's folder must survive a thread title rename and a
    // `projectsRoot` change; a pending external edit at the old stored path
    // must still block an Ecky commit.

    #[test]
    fn binding_info_retains_stored_folder_after_title_and_root_change() {
        let resolver_a = temp_resolver("retain-a");
        let resolver_b = temp_resolver("retain-b");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");

        // Bind under root A / title A.
        let binding =
            bind_new_thread(&resolver_a, conn, None, "thread-1", "Alpha Title").expect("bind");
        let original_folder = folder_of(&binding);
        let original_source = binding.source_path.clone();
        let original_digest = binding.source_digest.clone();

        // Both the title and the projectsRoot (resolver_b => a different
        // app_data dir) change. binding_info must still report the ORIGINAL
        // stored folder + source path + clean state.
        let info = binding_info(
            &resolver_b,
            conn,
            None,
            "thread-1",
            "Beta Title",
            Some(""),
            false,
            None,
        )
        .expect("info");
        assert_eq!(
            info.folder_path,
            original_folder.to_string_lossy(),
            "folder must be the retained stored path, not a re-derived one"
        );
        assert_eq!(info.source_path, original_source);
        assert_eq!(info.state, ProjectSyncState::Clean);
        assert!(!info.unbound);
        assert_eq!(
            info.manifest.as_ref().expect("manifest").source_digest,
            original_digest
        );

        // No second folder was created under the new title/root.
        let new_root = project_mirror::projects_root(&resolver_b, None);
        let new_slug = project_mirror::project_slug("Beta Title", "thread-1");
        assert!(
            !new_root.join(&new_slug).exists(),
            "no second folder should be created for the new title"
        );
    }

    #[test]
    fn binding_info_retains_exact_path_when_bound_folder_is_missing() {
        let resolver = temp_resolver("missing-bound-folder");
        let conn = temp_db_with_schema();
        seed_thread(&conn, "thread-1");
        let binding = bind_new_thread(&resolver, &conn, None, "thread-1", "Bracket").expect("bind");
        std::fs::remove_dir_all(&binding.folder_path).unwrap();

        let info = binding_info(
            &resolver, &conn, None, "thread-1", "Renamed", None, false, None,
        )
        .expect("binding info");

        assert!(!info.unbound, "SQLite binding survives missing folder");
        assert_eq!(info.source_path, binding.source_path);
        assert_eq!(info.folder_path, binding.folder_path);
        assert_eq!(info.state, ProjectSyncState::Missing);
    }

    #[test]
    fn refresh_on_version_append_after_rename_writes_original_folder_no_second_folder() {
        let resolver_a = temp_resolver("refresh-rename-a");
        let resolver_b = temp_resolver("refresh-rename-b");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");

        let binding = bind_new_thread(&resolver_a, conn, None, "thread-1", "Alpha").expect("bind");
        let original_folder = folder_of(&binding);

        // Append a new source under a NEW title + NEW root. The file at the
        // original folder is still clean, so refresh writes to the ORIGINAL
        // folder and creates no second folder under the new title/root.
        let next = "(model (part body (box 5 5 5)))";
        let refreshed = refresh_on_version_append(
            &resolver_b,
            conn,
            Some("/different/root"),
            "thread-1",
            "Beta Title",
            next,
            "msg-2",
            Some("model-2"),
            Some("msg-2"),
        )
        .expect("clean refresh");

        assert_eq!(
            PathBuf::from(&refreshed.folder_path),
            original_folder,
            "refresh must write to the retained stored folder"
        );
        assert_eq!(
            std::fs::read_to_string(original_folder.join(PROJECT_SOURCE_FILE_NAME)).unwrap(),
            next
        );
        assert_eq!(refreshed.source_digest, source_digest(next));

        // No second folder under the new title/root.
        let new_slug = project_mirror::project_slug("Beta Title", "thread-1");
        assert!(
            !PathBuf::from("/different/root").join(&new_slug).exists(),
            "no second folder should be created for the new title/root"
        );
    }

    #[test]
    fn prepare_editor_source_preserves_pending_edit_at_stored_path_after_rename() {
        let resolver_a = temp_resolver("editor-pending-a");
        let resolver_b = temp_resolver("editor-pending-b");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");

        let binding = bind_new_thread(&resolver_a, conn, None, "thread-1", "Alpha").expect("bind");
        let stored_folder = folder_of(&binding);
        let pending = "(model (part body (box 9 8 7)))";
        std::fs::write(stored_folder.join(PROJECT_SOURCE_FILE_NAME), pending).unwrap();

        let prepared = prepare_editor_source(
            &resolver_b,
            conn,
            Some("/different/root"),
            "thread-1",
            "Beta Title",
            "(model (part body (box 2 2 2)))",
            "msg-2",
            Some("model-2"),
        )
        .expect("prepare editor source");

        assert_eq!(PathBuf::from(&prepared.folder_path), stored_folder);
        assert_eq!(
            std::fs::read_to_string(stored_folder.join(PROJECT_SOURCE_FILE_NAME)).unwrap(),
            pending,
            "opening the editor must preserve the pending external edit"
        );
        let new_slug = project_mirror::project_slug("Beta Title", "thread-1");
        assert!(!PathBuf::from("/different/root").join(new_slug).exists());
    }

    #[test]
    fn unbound_thread_backfills_and_indexes_on_first_open() {
        let resolver = temp_resolver("open-backfill");
        let conn = temp_db_with_schema();
        let conn = &conn;
        seed_thread(conn, "thread-1");

        // Existing-but-unbound thread: no binding yet (the state
        // open_project_in_editor sees on first open of such a thread).
        assert!(get_binding(conn, "thread-1").unwrap().is_none());

        // First open mirrors the macro to a fresh folder, then indexes the
        // binding — the exact sequence open_project_in_editor runs for an
        // unbound thread (derive folder, export, index_export).
        let title = "Bracket";
        let source = "(model (part body (box 3 3 3)))";
        let slug = project_mirror::project_slug(title, "thread-1");
        let folder = project_mirror::project_dir(&resolver, None, &slug).unwrap();
        let (_, manifest) = project_mirror::export_project(
            &resolver,
            &project_mirror::ExportProjectRequest {
                slug: &slug,
                thread_id: "thread-1",
                message_id: "msg-open",
                model_id: None,
                source,
                projects_root: None,
            },
        )
        .unwrap();
        let binding = index_export(conn, "thread-1", &folder, &manifest.source_digest).unwrap();

        assert!(folder.join(PROJECT_SOURCE_FILE_NAME).is_file());
        assert_eq!(binding.source_digest, source_digest(source));
        assert_eq!(PathBuf::from(&binding.folder_path), folder);
        let row = get_binding(conn, "thread-1").unwrap().expect("row indexed");
        assert_eq!(row.thread_id, "thread-1");
        assert_eq!(row.source_digest, manifest.source_digest);
    }

    // silence unused-import warnings for the `temp_conn` helper retained for
    // reference; the schema is provided by `temp_db_with_schema` instead.
    #[allow(dead_code)]
    fn _temp_conn_retained() -> Connection {
        temp_conn()
    }
}
