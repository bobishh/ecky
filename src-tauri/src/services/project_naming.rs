//! One-time neutral naming migration, run before providers, MCP and watchers.
//! Persist intent before moving a directory so interrupted moves are recoverable.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::contracts::{AppError, AppResult};
use crate::project_mirror::{self, PROJECT_SOURCE_FILE_NAME};
use crate::thread_source_binding::{self, ThreadSourceBinding};

#[derive(Debug, Default)]
pub struct ProjectNamingReport {
    pub renamed: usize,
    pub errors: Vec<String>,
    /// Starting source consumers is unsafe while a moved folder is unindexed.
    pub blocked: bool,
}

pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_folder_relocations (
            thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
            project_id TEXT NOT NULL,
            old_folder TEXT NOT NULL,
            new_folder TEXT NOT NULL
        );",
    )
}

#[derive(Debug)]
struct Relocation {
    thread_id: String,
    project_id: String,
    old_folder: PathBuf,
    new_folder: PathBuf,
}

fn filesystem_error(error: std::io::Error) -> AppError {
    AppError::persistence(format!("Project folder relocation failed: {error}"))
}

fn require_resolved_process_leases(conn: &Connection, thread_id: &str) -> AppResult<()> {
    let unresolved: bool = conn.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM agent_provider_process_leases l
            JOIN agent_prompt_queue q ON q.id = l.queue_id
            WHERE q.ecky_thread_id = ?1
        )",
        [thread_id],
        |row| row.get(0),
    )?;
    if unresolved {
        return Err(AppError::validation(format!(
            "Project '{thread_id}' has an unresolved provider process lease; resolve its delivery before folder migration."
        )));
    }
    Ok(())
}

fn neutral_folder(binding: &ThreadSourceBinding) -> AppResult<Option<PathBuf>> {
    let old = Path::new(&binding.folder_path);
    let neutral = project_mirror::project_slug("", &binding.thread_id);
    let legacy = neutral.replacen("ecky-design-", "untitled-design-", 1);
    if old.file_name().and_then(|name| name.to_str()) != Some(&legacy) {
        return Ok(None);
    }
    let parent = old.parent().filter(|_| old.is_absolute()).ok_or_else(|| {
        AppError::validation(format!(
            "Project folder '{}' is not absolute.",
            old.display()
        ))
    })?;
    if Path::new(&binding.source_path) != old.join(PROJECT_SOURCE_FILE_NAME) {
        return Err(AppError::validation(format!(
            "Project '{}' does not bind its folder's model.ecky.",
            binding.thread_id
        )));
    }
    Ok(Some(parent.join(neutral)))
}

fn validate_folder(folder: &Path, thread_id: &str, project_id: Option<&str>) -> AppResult<String> {
    let metadata = std::fs::symlink_metadata(folder).map_err(|error| {
        AppError::persistence(format!(
            "Cannot inspect project '{}': {error}",
            folder.display()
        ))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::validation(format!(
            "Project '{}' is not a regular directory.",
            folder.display()
        )));
    }
    let source = folder.join(PROJECT_SOURCE_FILE_NAME);
    if !std::fs::symlink_metadata(&source).is_ok_and(|metadata| metadata.is_file()) {
        return Err(AppError::validation(format!(
            "Project source '{}' is not a regular file.",
            source.display()
        )));
    }
    let manifest = project_mirror::read_manifest(folder)?.ok_or_else(|| {
        AppError::validation(format!(
            "Project '{}' has no ownership manifest.",
            folder.display()
        ))
    })?;
    if manifest.thread_id != thread_id || project_id.is_some_and(|id| id != manifest.project_id) {
        return Err(AppError::validation(format!(
            "Project '{}' has a different owner.",
            folder.display()
        )));
    }
    Ok(manifest.project_id)
}

/// Unlike std::fs::rename on Unix, never replace even an empty destination.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rename_exclusive(old: &Path, new: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let old = CString::new(old.as_os_str().as_bytes())?;
    let new = CString::new(new.as_os_str().as_bytes())?;
    // SAFETY: Both arguments are live NUL-terminated paths. The kernel performs
    // the same-filesystem rename atomically and refuses existing destinations.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(old.as_ptr(), new.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            old.as_ptr(),
            libc::AT_FDCWD,
            new.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
fn rename_exclusive(old: &Path, new: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let old = old
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let new = new
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: Both arguments are live NUL-terminated paths. No replace flag.
    let result = unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(old.as_ptr(), new.as_ptr(), 0)
    };
    if result != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn rename_exclusive(_old: &Path, _new: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Atomic exclusive project rename is unavailable on this platform.",
    ))
}

fn complete_relocation(conn: &Connection, relocation: &Relocation) -> AppResult<()> {
    let binding = thread_source_binding::get_binding(conn, &relocation.thread_id)?
        .ok_or_else(|| AppError::not_found("Relocating project lost its source binding."))?;
    let stored = Path::new(&binding.folder_path);
    if stored != relocation.old_folder && stored != relocation.new_folder {
        return Err(AppError::validation(
            "Relocating project has a different bound folder.",
        ));
    }
    if Path::new(&binding.source_path) != stored.join(PROJECT_SOURCE_FILE_NAME) {
        return Err(AppError::validation(
            "Relocating project source path changed independently.",
        ));
    }
    if relocation.old_folder.parent() != relocation.new_folder.parent()
        || neutral_folder(&ThreadSourceBinding {
            folder_path: relocation.old_folder.to_string_lossy().into_owned(),
            source_path: relocation
                .old_folder
                .join(PROJECT_SOURCE_FILE_NAME)
                .to_string_lossy()
                .into_owned(),
            ..binding.clone()
        })?
        .as_deref()
            != Some(relocation.new_folder.as_path())
    {
        return Err(AppError::validation(
            "Project relocation does not match neutral naming policy.",
        ));
    }
    match (
        relocation
            .old_folder
            .try_exists()
            .map_err(filesystem_error)?,
        relocation
            .new_folder
            .try_exists()
            .map_err(filesystem_error)?,
    ) {
        (true, false) => {
            require_resolved_process_leases(conn, &relocation.thread_id)?;
            validate_folder(
                &relocation.old_folder,
                &relocation.thread_id,
                Some(&relocation.project_id),
            )?;
            rename_exclusive(&relocation.old_folder, &relocation.new_folder).map_err(|error| {
                AppError::persistence(format!(
                    "Cannot move '{}' to '{}': {error}",
                    relocation.old_folder.display(),
                    relocation.new_folder.display()
                ))
            })?;
        }
        (false, true) => {}
        _ => {
            return Err(AppError::validation(format!(
            "Cannot relocate '{}': expected exactly one owned source directory; destination '{}'.",
            relocation.old_folder.display(),
            relocation.new_folder.display()
        )))
        }
    }
    validate_folder(
        &relocation.new_folder,
        &relocation.thread_id,
        Some(&relocation.project_id),
    )?;
    #[cfg(unix)]
    std::fs::File::open(relocation.new_folder.parent().expect("validated parent"))
        .and_then(|parent| parent.sync_all())
        .map_err(filesystem_error)?;

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE thread_source_bindings SET folder_path = ?2, source_path = ?3 WHERE thread_id = ?1",
        params![
            relocation.thread_id,
            relocation.new_folder.to_string_lossy(),
            relocation
                .new_folder
                .join(PROJECT_SOURCE_FILE_NAME)
                .to_string_lossy()
        ],
    )?;
    tx.execute("UPDATE agent_thread_bindings SET external_cwd = ?3, bootstrap_version = 0 WHERE ecky_thread_id = ?1 AND external_cwd = ?2", params![relocation.thread_id, relocation.old_folder.to_string_lossy(), relocation.new_folder.to_string_lossy()])?;
    tx.execute(
        "DELETE FROM project_folder_relocations WHERE thread_id = ?1",
        [&relocation.thread_id],
    )?;
    tx.commit()?;
    Ok(())
}

fn attempt_relocation(
    conn: &Connection,
    relocation: &Relocation,
    report: &mut ProjectNamingReport,
) {
    match complete_relocation(conn, relocation) {
        Ok(()) => report.renamed += 1,
        Err(error) => {
            // After a successful move, no consumer may recreate the old folder
            // while its binding still points there. Recovery must finish first.
            report.blocked |= !relocation.old_folder.is_dir();
            report.errors.push(error.to_string());
        }
    }
}

pub fn reconcile_project_names(conn: &Connection) -> ProjectNamingReport {
    let mut report = ProjectNamingReport::default();
    let result = (|| -> AppResult<()> {
        if !conn.is_autocommit() {
            return Err(AppError::validation(
                "Project relocation requires a committed startup connection.",
            ));
        }
        ensure_schema(conn)?;
        let pending = {
            let mut statement = conn.prepare("SELECT thread_id, project_id, old_folder, new_folder FROM project_folder_relocations ORDER BY thread_id")?;
            let rows = statement.query_map([], |row| {
                Ok(Relocation {
                    thread_id: row.get(0)?,
                    project_id: row.get(1)?,
                    old_folder: PathBuf::from(row.get::<_, String>(2)?),
                    new_folder: PathBuf::from(row.get::<_, String>(3)?),
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let pending_ids = pending
            .iter()
            .map(|item| item.thread_id.clone())
            .collect::<std::collections::HashSet<_>>();
        for relocation in pending {
            attempt_relocation(conn, &relocation, &mut report);
        }
        for binding in thread_source_binding::list_bindings(conn)? {
            if pending_ids.contains(&binding.thread_id) {
                continue;
            }
            let planned = (|| -> AppResult<Option<Relocation>> {
                let Some(new_folder) = neutral_folder(&binding)? else {
                    return Ok(None);
                };
                require_resolved_process_leases(conn, &binding.thread_id)?;
                let old_folder = PathBuf::from(&binding.folder_path);
                let project_id = validate_folder(&old_folder, &binding.thread_id, None)?;
                let relocation = Relocation {
                    thread_id: binding.thread_id.clone(),
                    project_id,
                    old_folder,
                    new_folder,
                };
                conn.execute("INSERT INTO project_folder_relocations (thread_id, project_id, old_folder, new_folder) VALUES (?1, ?2, ?3, ?4)", params![relocation.thread_id, relocation.project_id, relocation.old_folder.to_string_lossy(), relocation.new_folder.to_string_lossy()])?;
                Ok(Some(relocation))
            })();
            match planned {
                Ok(Some(relocation)) => attempt_relocation(conn, &relocation, &mut report),
                Ok(None) => {}
                Err(error) => report.errors.push(error.to_string()),
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        report.blocked = true;
        report.errors.push(error.to_string());
    }
    report
}
