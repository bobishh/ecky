//! Filesystem project mirror: exposes one thread's active macro as a plain
//! folder (`model.ecky` + `ecky-project.json`) so external editors and LLM
//! file skills can author source directly, while threads/versions remain the
//! canonical record. See `openspec/changes/filesystem-project-mirror`.
//!
//! The folder is a mirror, never an alternate database: edits re-enter the
//! app only through compile -> preview -> commit (wired in `mcp::handlers`).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::{AppError, AppResult, PathResolver};

pub const PROJECT_SOURCE_FILE_NAME: &str = "model.ecky";
pub const PROJECT_MANIFEST_FILE_NAME: &str = "ecky-project.json";
const PROJECT_MANIFEST_SCHEMA_VERSION: u32 = 1;
const PROJECTS_DIR_NAME: &str = "projects";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub project_id: String,
    pub thread_id: String,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Digest of the `model.ecky` bytes Ecky last wrote or applied. The only
    /// thing distinguishing "user edited the file" from "clean".
    pub source_digest: String,
    pub exported_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectSyncState {
    /// No `model.ecky` or no manifest in the folder.
    Missing,
    /// File matches the manifest digest and the thread head is still the
    /// bound message.
    Clean,
    /// File was edited externally; thread head unchanged. Safe to apply.
    FileChanged,
    /// Thread gained versions past the binding; folder is stale. Re-export.
    ThreadAdvanced,
    /// Both sides moved. Applying requires an explicit force.
    Conflict,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFolderStatus {
    pub state: ProjectSyncState,
    pub folder: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<ProjectManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_head_message_id: Option<String>,
}

pub fn source_digest(source: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(source.as_bytes()))
}

/// Projects root: `<app_data>/projects` until a config override lands.
pub fn projects_root(app: &dyn PathResolver) -> PathBuf {
    app.app_data_dir().join(PROJECTS_DIR_NAME)
}

/// Deterministic folder slug: human prefix from the title plus a stable
/// thread-id suffix so renames and collisions cannot cross-wire folders.
pub fn project_slug(title: &str, thread_id: &str) -> String {
    let mut prefix: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    prefix.truncate(40);
    let suffix: String = thread_id
        .chars()
        .rev()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if prefix.is_empty() {
        format!("project-{suffix}")
    } else {
        format!("{prefix}-{suffix}")
    }
}

pub fn project_dir(app: &dyn PathResolver, slug: &str) -> AppResult<PathBuf> {
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::validation(format!(
            "Project slug `{slug}` is not a safe directory name."
        )));
    }
    Ok(projects_root(app).join(slug))
}

pub fn read_manifest(dir: &Path) -> AppResult<Option<ProjectManifest>> {
    let path = dir.join(PROJECT_MANIFEST_FILE_NAME);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|err| {
        AppError::persistence(format!("Failed to read '{}': {}", path.display(), err))
    })?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|err| AppError::persistence(format!("Invalid '{}': {}", path.display(), err)))
}

pub fn write_manifest(dir: &Path, manifest: &ProjectManifest) -> AppResult<()> {
    let path = dir.join(PROJECT_MANIFEST_FILE_NAME);
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|err| AppError::internal(format!("Failed to serialize manifest: {err}")))?;
    fs::write(&path, json).map_err(|err| {
        AppError::persistence(format!("Failed to write '{}': {}", path.display(), err))
    })
}

pub fn read_project_source(dir: &Path) -> AppResult<Option<String>> {
    let path = dir.join(PROJECT_SOURCE_FILE_NAME);
    if !path.is_file() {
        return Ok(None);
    }
    fs::read_to_string(&path).map(Some).map_err(|err| {
        AppError::persistence(format!("Failed to read '{}': {}", path.display(), err))
    })
}

pub struct ExportProjectRequest<'a> {
    pub slug: &'a str,
    pub thread_id: &'a str,
    pub message_id: &'a str,
    pub model_id: Option<&'a str>,
    pub source: &'a str,
}

/// Writes/refreshes the mirror folder from a bound version. Keeps the
/// existing projectId across re-exports so external references stay valid.
pub fn export_project(
    app: &dyn PathResolver,
    request: &ExportProjectRequest<'_>,
) -> AppResult<(PathBuf, ProjectManifest)> {
    let dir = project_dir(app, request.slug)?;
    fs::create_dir_all(&dir).map_err(|err| {
        AppError::persistence(format!("Failed to create '{}': {}", dir.display(), err))
    })?;
    let project_id = read_manifest(&dir)?
        .map(|existing| existing.project_id)
        .unwrap_or_else(|| format!("proj-{}", uuid::Uuid::new_v4()));

    let source_path = dir.join(PROJECT_SOURCE_FILE_NAME);
    fs::write(&source_path, request.source).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write '{}': {}",
            source_path.display(),
            err
        ))
    })?;

    let manifest = ProjectManifest {
        schema_version: PROJECT_MANIFEST_SCHEMA_VERSION,
        project_id,
        thread_id: request.thread_id.to_string(),
        message_id: request.message_id.to_string(),
        model_id: request.model_id.map(str::to_string),
        source_digest: source_digest(request.source),
        exported_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0),
    };
    write_manifest(&dir, &manifest)?;
    Ok((dir, manifest))
}

/// Pure classification over (file digest, manifest, thread head).
pub fn classify_sync_state(
    file_digest: Option<&str>,
    manifest: Option<&ProjectManifest>,
    thread_head_message_id: Option<&str>,
) -> ProjectSyncState {
    let (Some(file_digest), Some(manifest)) = (file_digest, manifest) else {
        return ProjectSyncState::Missing;
    };
    let file_changed = file_digest != manifest.source_digest;
    let thread_advanced = thread_head_message_id.is_some_and(|head| head != manifest.message_id);
    match (file_changed, thread_advanced) {
        (false, false) => ProjectSyncState::Clean,
        (true, false) => ProjectSyncState::FileChanged,
        (false, true) => ProjectSyncState::ThreadAdvanced,
        (true, true) => ProjectSyncState::Conflict,
    }
}

/// Read-only folder status; thread head is supplied by the caller (handlers
/// own history lookups).
pub fn folder_status(
    app: &dyn PathResolver,
    slug: &str,
    thread_head_message_id: Option<&str>,
) -> AppResult<ProjectFolderStatus> {
    let dir = project_dir(app, slug)?;
    let manifest = read_manifest(&dir)?;
    let file_digest = read_project_source(&dir)?.map(|source| source_digest(&source));
    let state = classify_sync_state(
        file_digest.as_deref(),
        manifest.as_ref(),
        thread_head_message_id,
    );
    Ok(ProjectFolderStatus {
        state,
        folder: dir.to_string_lossy().to_string(),
        manifest,
        file_digest,
        thread_head_message_id: thread_head_message_id.map(str::to_string),
    })
}

/// Slugs of all project folders under the projects root that look like
/// mirrors (have a manifest). Used by the folder watcher.
pub fn list_project_slugs(app: &dyn PathResolver) -> AppResult<Vec<String>> {
    let root = projects_root(app);
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut slugs = Vec::new();
    let entries = fs::read_dir(&root).map_err(|err| {
        AppError::persistence(format!("Failed to read '{}': {}", root.display(), err))
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.join(PROJECT_MANIFEST_FILE_NAME).is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            slugs.push(name.to_string());
        }
    }
    slugs.sort();
    Ok(slugs)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            root: std::env::temp_dir().join(format!("ecky-project-mirror-{name}-{nonce}")),
        }
    }

    fn sample_request<'a>(slug: &'a str, source: &'a str) -> ExportProjectRequest<'a> {
        ExportProjectRequest {
            slug,
            thread_id: "thread-1",
            message_id: "msg-1",
            model_id: Some("model-1"),
            source,
        }
    }

    #[test]
    fn export_writes_source_and_manifest_round_trip() {
        let resolver = temp_resolver("export");
        let source = "(model (part body (box 1 2 3)))";

        let (dir, manifest) =
            export_project(&resolver, &sample_request("bracket-abc123", source)).expect("export");

        assert_eq!(
            fs::read_to_string(dir.join(PROJECT_SOURCE_FILE_NAME)).expect("source"),
            source
        );
        let reread = read_manifest(&dir).expect("read").expect("manifest");
        assert_eq!(reread, manifest);
        assert_eq!(reread.schema_version, 1);
        assert_eq!(reread.thread_id, "thread-1");
        assert_eq!(reread.source_digest, source_digest(source));
        assert!(reread.project_id.starts_with("proj-"));

        let raw = fs::read_to_string(dir.join(PROJECT_MANIFEST_FILE_NAME)).expect("raw");
        assert!(
            raw.contains("\"sourceDigest\""),
            "camelCase manifest: {raw}"
        );
        assert!(raw.contains("\"threadId\""), "camelCase manifest: {raw}");
    }

    #[test]
    fn re_export_keeps_project_id_and_rebases_digest() {
        let resolver = temp_resolver("reexport");
        let (_, first) = export_project(
            &resolver,
            &sample_request("kit-abc123", "(model (part a (box 1 1 1)))"),
        )
        .expect("export");
        let mut second_request = sample_request("kit-abc123", "(model (part a (box 2 2 2)))");
        second_request.message_id = "msg-2";
        let (_, second) = export_project(&resolver, &second_request).expect("re-export");

        assert_eq!(first.project_id, second.project_id);
        assert_eq!(second.message_id, "msg-2");
        assert_ne!(first.source_digest, second.source_digest);
    }

    #[test]
    fn slug_is_deterministic_and_safe() {
        assert_eq!(
            project_slug("Film Adapter v2!", "thread-12345678"),
            project_slug("Film Adapter v2!", "thread-12345678")
        );
        let slug = project_slug("Película / адаптер", "thread-XYZ99876");
        assert!(
            slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "{slug}"
        );
        assert!(slug.ends_with("XYZ99876"), "{slug}");
        assert!(project_slug("", "thread-1").starts_with("project-"));
    }

    #[test]
    fn classify_covers_the_full_matrix() {
        let manifest = ProjectManifest {
            schema_version: 1,
            project_id: "proj-x".into(),
            thread_id: "thread-1".into(),
            message_id: "msg-1".into(),
            model_id: None,
            source_digest: source_digest("a"),
            exported_at: 0,
        };
        let clean = source_digest("a");
        let edited = source_digest("b");

        assert_eq!(
            classify_sync_state(None, Some(&manifest), Some("msg-1")),
            ProjectSyncState::Missing
        );
        assert_eq!(
            classify_sync_state(Some(&clean), None, Some("msg-1")),
            ProjectSyncState::Missing
        );
        assert_eq!(
            classify_sync_state(Some(&clean), Some(&manifest), Some("msg-1")),
            ProjectSyncState::Clean
        );
        assert_eq!(
            classify_sync_state(Some(&edited), Some(&manifest), Some("msg-1")),
            ProjectSyncState::FileChanged
        );
        assert_eq!(
            classify_sync_state(Some(&clean), Some(&manifest), Some("msg-2")),
            ProjectSyncState::ThreadAdvanced
        );
        assert_eq!(
            classify_sync_state(Some(&edited), Some(&manifest), Some("msg-2")),
            ProjectSyncState::Conflict
        );
        assert_eq!(
            classify_sync_state(Some(&edited), Some(&manifest), None),
            ProjectSyncState::FileChanged
        );
    }

    #[test]
    fn folder_status_reports_missing_then_clean() {
        let resolver = temp_resolver("status");
        let status = folder_status(&resolver, "ghost-abc12345", Some("msg-1")).expect("status");
        assert_eq!(status.state, ProjectSyncState::Missing);

        let source = "(model (part body (box 1 2 3)))";
        export_project(&resolver, &sample_request("live-abc12345", source)).expect("export");
        let status = folder_status(&resolver, "live-abc12345", Some("msg-1")).expect("status");
        assert_eq!(status.state, ProjectSyncState::Clean);
        assert_eq!(
            status.file_digest.as_deref(),
            Some(source_digest(source).as_str())
        );

        fs::write(
            projects_root(&resolver)
                .join("live-abc12345")
                .join(PROJECT_SOURCE_FILE_NAME),
            "(model (part body (box 9 9 9)))",
        )
        .expect("external edit");
        let status = folder_status(&resolver, "live-abc12345", Some("msg-1")).expect("status");
        assert_eq!(status.state, ProjectSyncState::FileChanged);
    }

    #[test]
    fn unsafe_slugs_are_rejected() {
        let resolver = temp_resolver("unsafe");
        let err = folder_status(&resolver, "../escape", None).expect_err("unsafe slug");
        assert!(err.message.contains("not a safe"), "{}", err.message);
    }
}
