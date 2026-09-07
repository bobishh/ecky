use super::{
    clear_session_thread_render_preview_durable, handle_macro_preview_render,
    handle_verify_generated_model, now_secs, AgentContext,
};
use crate::contracts::{AppError, AppResult};
use crate::db;
use crate::mcp::contracts::{AgentIdentityOverride, MacroReplaceRequest};
use crate::models::{AppState, PathResolver};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::OptionalExtension;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const PROJECT_FOLDER_DEBOUNCE: Duration = Duration::from_millis(800);
pub const PROJECT_FOLDER_FALLBACK_POLL: Duration = Duration::from_secs(1);

fn active_project_folder_render() -> &'static Mutex<Option<Arc<AtomicBool>>> {
    static ACTIVE: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

fn project_folder_apply_lock() -> &'static tokio::sync::Mutex<()> {
    static APPLY_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    APPLY_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

struct ActiveProjectFolderRenderGuard {
    cancellation: Arc<AtomicBool>,
}

impl ActiveProjectFolderRenderGuard {
    fn activate(cancellation: Arc<AtomicBool>) -> Self {
        active_project_folder_render()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .replace(cancellation.clone());
        Self { cancellation }
    }
}

impl Drop for ActiveProjectFolderRenderGuard {
    fn drop(&mut self) {
        let mut active = active_project_folder_render()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(token, &self.cancellation))
        {
            active.take();
        }
    }
}

pub(crate) fn cancel_active_project_folder_render() -> bool {
    let active = active_project_folder_render()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    active.is_some_and(|cancellation| {
        cancellation.store(true, Ordering::Release);
        true
    })
}

fn is_project_source_path(path: &std::path::Path) -> bool {
    path.file_name().and_then(|name| name.to_str())
        == Some(crate::project_mirror::PROJECT_SOURCE_FILE_NAME)
}

fn declared_separated_print_layout(parameters: &crate::contracts::DesignParams) -> bool {
    matches!(
        parameters.get("assembly-preview"),
        Some(crate::contracts::ParamValue::Boolean(false))
    )
}

fn only_nonblocking_disconnected_issues(
    result: &crate::contracts::StructuralVerificationResult,
    separated_print_layout: bool,
) -> bool {
    only_nonblocking_disconnected_codes(
        result.issues.iter().map(|issue| issue.code.as_str()),
        separated_print_layout,
    ) && result.authored_verify_checks.iter().all(|check| {
        matches!(
            check.status,
            crate::contracts::AuthoredVerifyCheckStatus::Passed
                | crate::contracts::AuthoredVerifyCheckStatus::Skipped
        ) || (check.status == crate::contracts::AuthoredVerifyCheckStatus::Failed
            && check.severity == crate::contracts::AuthoredVerifySeverity::Warning)
    })
}

fn only_nonblocking_disconnected_codes<'a>(
    codes: impl IntoIterator<Item = &'a str>,
    separated_print_layout: bool,
) -> bool {
    let mut found = false;
    for code in codes {
        found = true;
        match code {
            // A multi-part assembly is disconnected by construction.
            "PREVIEW_STL_DISCONNECTED_COMPONENTS" => {}
            // Disconnected solids inside one printable part are intentional only
            // for an explicitly selected separated print layout.
            "PART_DISCONNECTED" if separated_print_layout => {}
            _ => return false,
        }
    }
    found
}

// --- Filesystem project mirror tools (filesystem-project-mirror T2) ---

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFolderExportRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    /// Folder slug; defaults to a deterministic slug from title + thread id.
    pub slug: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFolderExportResponse {
    pub slug: String,
    pub folder: String,
    pub manifest: crate::project_mirror::ProjectManifest,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFolderStatusRequest {
    pub slug: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFolderApplyRequest {
    #[serde(flatten)]
    pub identity: AgentIdentityOverride,
    pub slug: String,
    /// Retained for wire compatibility. Appends always use the latest head;
    /// no force/conflict gate exists.
    #[serde(default)]
    pub force: bool,
    pub title: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFolderApplyResponse {
    pub state_before: crate::project_mirror::ProjectSyncState,
    /// True when the folder was already clean and nothing was appended.
    pub no_op: bool,
    pub thread_id: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub manifest: crate::project_mirror::ProjectManifest,
}

pub async fn handle_project_folder_export(
    state: &AppState,
    app: &dyn PathResolver,
    req: ProjectFolderExportRequest,
    ctx: &AgentContext,
) -> AppResult<ProjectFolderExportResponse> {
    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let _ = ctx;
    let target = {
        let conn = state.db.lock().await;
        crate::services::target::resolve_editable_target(
            &conn,
            app,
            req.thread_id.clone(),
            req.message_id.clone(),
        )?
    };
    let slug = match req.slug {
        Some(slug) => slug,
        None => crate::project_mirror::project_slug(&target.design_output.title, &target.thread_id),
    };
    let model_id = target
        .artifact_bundle
        .as_ref()
        .map(|bundle| bundle.model_id.clone());
    let projects_root = configured_projects_root(state);
    let stored_binding = {
        let conn = state.db.lock().await;
        crate::thread_source_binding::get_binding(&conn, &target.thread_id)
            .map_err(|err| AppError::persistence(err.to_string()))?
    };
    if let Some(binding) = stored_binding {
        let dir = std::path::PathBuf::from(&binding.folder_path);
        let (root, slug) = crate::thread_source_binding::stored_folder_export_args(&dir)?;
        let (dir, manifest) = crate::project_mirror::export_project_with_lock(
            app,
            &crate::project_mirror::ExportProjectRequest {
                slug: &slug,
                thread_id: &target.thread_id,
                message_id: &target.message_id,
                model_id: model_id.as_deref(),
                source: &target.design_output.macro_code,
                projects_root: root.as_deref(),
            },
            target
                .artifact_bundle
                .as_ref()
                .and_then(|bundle| bundle.component_dependency_lock.as_ref()),
        )?;
        {
            let conn = state.db.lock().await;
            crate::thread_source_binding::index_export(
                &conn,
                &target.thread_id,
                &dir,
                &manifest.source_digest,
            )?;
        }
        return Ok(ProjectFolderExportResponse {
            slug,
            folder: dir.to_string_lossy().to_string(),
            manifest,
        });
    }
    let (dir, manifest) = crate::project_mirror::export_project_with_lock(
        app,
        &crate::project_mirror::ExportProjectRequest {
            slug: &slug,
            thread_id: &target.thread_id,
            message_id: &target.message_id,
            model_id: model_id.as_deref(),
            source: &target.design_output.macro_code,
            projects_root: projects_root.as_deref(),
        },
        target
            .artifact_bundle
            .as_ref()
            .and_then(|bundle| bundle.component_dependency_lock.as_ref()),
    )?;

    // Index the export as this thread's bound source folder (digest-safe:
    // binding row digest == on-disk file digest == manifest digest).
    {
        let conn = state.db.lock().await;
        crate::thread_source_binding::index_export(
            &conn,
            &target.thread_id,
            &dir,
            &manifest.source_digest,
        )?;
    }

    Ok(ProjectFolderExportResponse {
        slug,
        folder: dir.to_string_lossy().to_string(),
        manifest,
    })
}

/// Configured `projectsRoot` override; `None` falls back to `<app_data>/projects`.
fn configured_projects_root(state: &AppState) -> Option<String> {
    state.config.lock().unwrap().projects_root.clone()
}

async fn project_folder_path(
    state: &AppState,
    slug: &str,
    app: &dyn PathResolver,
) -> AppResult<std::path::PathBuf> {
    let binding = {
        let conn = state.db.lock().await;
        crate::thread_source_binding::get_binding_by_folder_name(&conn, slug)
            .map_err(|err| AppError::persistence(err.to_string()))?
    };
    if let Some(binding) = binding {
        return Ok(std::path::PathBuf::from(binding.folder_path));
    }
    let root = configured_projects_root(state);
    crate::project_mirror::project_dir(app, root.as_deref(), slug)
}

async fn project_thread_head(
    state: &AppState,
    slug: &str,
    app: &dyn PathResolver,
) -> AppResult<Option<String>> {
    let dir = project_folder_path(state, slug, app).await?;
    let Some(manifest) = crate::project_mirror::read_manifest(&dir)? else {
        return Ok(None);
    };
    let conn = state.db.lock().await;
    // Head is append order, independent of validation/render status or
    // artifact availability. Resolve identity in SQL; watcher status must not
    // deserialize the thread's CAD payload graph.
    let head = db::get_thread_head_version_id(&conn, &manifest.thread_id)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(head)
}

pub async fn handle_project_folder_status(
    state: &AppState,
    app: &dyn PathResolver,
    req: ProjectFolderStatusRequest,
) -> AppResult<crate::project_mirror::ProjectFolderStatus> {
    let head = project_thread_head(state, &req.slug, app).await?;
    let dir = project_folder_path(state, &req.slug, app).await?;
    let (root, slug) = crate::thread_source_binding::stored_folder_export_args(&dir)?;
    crate::project_mirror::folder_status(app, root.as_deref(), &slug, head.as_deref())
}

pub async fn handle_project_folder_apply(
    state: &AppState,
    app: &dyn PathResolver,
    req: ProjectFolderApplyRequest,
    ctx: &AgentContext,
) -> AppResult<ProjectFolderApplyResponse> {
    use crate::project_mirror::ProjectSyncState;

    let ctx = ctx.with_override(&req.identity);
    let ctx = &ctx;
    let status = handle_project_folder_status(
        state,
        app,
        ProjectFolderStatusRequest {
            slug: req.slug.clone(),
        },
    )
    .await?;

    let manifest = match (&status.state, status.manifest.clone()) {
        (ProjectSyncState::Missing, _) | (_, None) => {
            return Err(AppError::validation(format!(
                "Project folder `{}` has no exported model; run project_folder_export first.",
                req.slug
            )))
        }
        (_, Some(manifest)) => manifest,
    };

    if status.state == ProjectSyncState::Clean {
        return Ok(ProjectFolderApplyResponse {
            state_before: ProjectSyncState::Clean,
            no_op: true,
            thread_id: manifest.thread_id.clone(),
            message_id: manifest.message_id.clone(),
            model_id: manifest.model_id.clone(),
            manifest,
        });
    }

    // `threadAdvanced` and `conflict` are informational mirror states only.
    // A changed file always appends against the current head; `force` remains
    // accepted for old clients but never changes write semantics.
    let _ = req.force;

    let dir = std::path::PathBuf::from(&status.folder);
    let (source, expected_lock) = crate::project_mirror::read_project_apply_input(&dir)?
        .ok_or_else(|| {
            AppError::validation(format!(
                "Project folder `{}` lost its model.ecky during apply.",
                req.slug
            ))
        })?;
    let state_before = status.state;
    let expected_source_digest = crate::project_mirror::source_digest(&source);

    let preview = Box::pin(handle_macro_preview_render(
        state,
        app,
        MacroReplaceRequest {
            identity: AgentIdentityOverride::default(),
            thread_id: Some(manifest.thread_id.clone()),
            message_id: status.thread_head_message_id.clone(),
            macro_code: source.clone(),
            macro_dialect: None,
            ui_spec: None,
            parameters: None,
            post_processing: None,
            geometry_backend: None,
            source_window: None,
        },
        ctx,
    ))
    .await?;
    if !project_source_digest_matches(&dir, &expected_source_digest)? {
        rollback_project_folder_preview(state, ctx, &dir, &manifest, &preview.message_id).await?;
        return Err(AppError::conflict(
            crate::ecky_cad_host::direct_occt_runner::SOURCE_CHANGED_CANCELLATION_MESSAGE,
        ));
    }
    if preview.artifact_bundle.component_dependency_lock != expected_lock {
        rollback_project_folder_preview(state, ctx, &dir, &manifest, &preview.message_id).await?;
        return Err(AppError::validation(format!(
            "Project folder `{}` preview dependency lock differs from canonical ecky.lock.edn; apply refused.",
            req.slug
        )));
    }
    let verification = match Box::pin(handle_verify_generated_model(
        state,
        app,
        &preview.thread_id,
        &preview.message_id,
        &preview.artifact_bundle.model_id,
        "",
    ))
    .await
    {
        Ok(verification) => verification,
        Err(error) => {
            rollback_project_folder_preview(state, ctx, &dir, &manifest, &preview.message_id)
                .await?;
            return Err(error);
        }
    };
    if !project_source_digest_matches(&dir, &expected_source_digest)? {
        rollback_project_folder_preview(state, ctx, &dir, &manifest, &preview.message_id).await?;
        return Err(AppError::conflict(
            crate::ecky_cad_host::direct_occt_runner::SOURCE_CHANGED_CANCELLATION_MESSAGE,
        ));
    }
    let separated_print_layout = declared_separated_print_layout(&preview.initial_params);
    let accepted_nonblocking_disconnected = !verification.result.passed
        && only_nonblocking_disconnected_issues(&verification.result, separated_print_layout);
    if !verification.result.passed && !accepted_nonblocking_disconnected {
        rollback_project_folder_preview(state, ctx, &dir, &manifest, &preview.message_id).await?;
        return Err(AppError::validation(format!(
            "Project folder `{}` preview verification failed: {}",
            req.slug, verification.result.summary
        )));
    }
    let draft = {
        let conn = state.db.lock().await;
        db::get_agent_draft_by_preview_id(&conn, &preview.message_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .ok_or_else(|| AppError::persistence("Applied preview draft disappeared."))?
    };
    let version_id = draft
        .base_message_id
        .clone()
        .ok_or_else(|| AppError::persistence("Applied preview has no durable version."))?;
    if accepted_nonblocking_disconnected || req.title.is_some() || req.version_name.is_some() {
        let conn = state.db.lock().await;
        let version = db::get_thread_message_version(&conn, &preview.thread_id, &version_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .ok_or_else(|| AppError::persistence("Applied durable version disappeared."))?;
        let mut output = version
            .output
            .clone()
            .ok_or_else(|| AppError::persistence("Applied version has no source payload."))?;
        if let Some(title) = req.title.as_ref() {
            output.title = title.clone();
        }
        if let Some(version_name) = req.version_name.as_ref() {
            output.version_name = version_name.clone();
        }
        let status = if accepted_nonblocking_disconnected {
            crate::contracts::MessageStatus::Success
        } else {
            version.status.clone()
        };
        let content = if accepted_nonblocking_disconnected {
            output.response.clone()
        } else {
            version.content.clone()
        };
        db::update_message_status_and_output(
            &conn,
            &version_id,
            db::MessageStatusUpdate {
                status: &status,
                output: Some(&output),
                usage: version.usage.as_ref(),
                artifact_bundle: version.artifact_bundle.as_ref(),
                model_manifest: version.model_manifest.as_ref(),
                structural_verification: version.structural_verification.as_ref(),
                visual_kind: version.visual_kind.as_ref(),
                content: Some(&content),
            },
        )
        .map_err(|error| AppError::persistence(error.to_string()))?;
    }
    clear_session_thread_render_preview_durable(state, &ctx.session_id, &preview.thread_id).await?;

    // Rebase the manifest onto the appended version; the file bytes we
    // applied become the new clean baseline.
    let rebased = crate::project_mirror::ProjectManifest {
        message_id: version_id.clone(),
        model_id: Some(preview.artifact_bundle.model_id.clone()),
        source_digest: crate::project_mirror::source_digest(&source),
        exported_at: now_secs(),
        ..manifest
    };
    crate::project_mirror::write_manifest(&dir, &rebased)?;
    {
        let conn = state.db.lock().await;
        crate::thread_source_binding::index_export(
            &conn,
            &rebased.thread_id,
            &dir,
            &rebased.source_digest,
        )?;
    }

    Ok(ProjectFolderApplyResponse {
        state_before,
        no_op: false,
        thread_id: preview.thread_id,
        message_id: version_id,
        model_id: Some(preview.artifact_bundle.model_id),
        manifest: rebased,
    })
}

fn project_source_digest_matches(dir: &Path, expected_digest: &str) -> AppResult<bool> {
    Ok(crate::project_mirror::read_project_source(dir)?
        .map(|source| crate::project_mirror::source_digest(&source) == expected_digest)
        .unwrap_or(false))
}

async fn discard_project_folder_preview(
    state: &AppState,
    ctx: &AgentContext,
    preview_id: &str,
) -> AppResult<()> {
    let draft_target = {
        let conn = state.db.lock().await;
        db::get_agent_draft_by_preview_id(&conn, preview_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
            .map(|draft| (draft.thread_id, draft.base_message_id))
    };
    if let Some((thread_id, version_id)) = draft_target {
        if let Some(version_id) = version_id {
            let conn = state.db.lock().await;
            db::delete_message(&conn, &version_id)
                .map_err(|error| AppError::persistence(error.to_string()))?;
        }
        clear_session_thread_render_preview_durable(state, &ctx.session_id, &thread_id).await?;
    }
    Ok(())
}

async fn rollback_project_folder_preview(
    state: &AppState,
    ctx: &AgentContext,
    dir: &Path,
    manifest: &crate::project_mirror::ProjectManifest,
    preview_id: &str,
) -> AppResult<()> {
    discard_project_folder_preview(state, ctx, preview_id).await?;
    crate::project_mirror::write_manifest(dir, manifest)?;
    let conn = state.db.lock().await;
    crate::thread_source_binding::index_export(
        &conn,
        &manifest.thread_id,
        dir,
        &manifest.source_digest,
    )?;
    Ok(())
}

// --- Project folder watcher (filesystem-project-mirror T5) ---

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ProjectFolderWatchEvent {
    /// An external edit was observed and is waiting for the settle tick.
    Detected { slug: String, thread_id: String },
    /// A settled external edit was appended and processed.
    Applied {
        slug: String,
        thread_id: String,
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_id: Option<String>,
    },
    /// A settled external edit failed to compile/render. Reported
    /// once per file digest; editing the file again retries.
    ApplyFailed {
        slug: String,
        thread_id: String,
        message_id: String,
        error: String,
    },
}

/// Native filesystem wakeups collapse into a one-slot channel. The bounded
/// fallback poll covers watcher setup failures, newly bound external folders,
/// and backends that miss an event.
pub struct ProjectFolderWatchTransport {
    _watcher: Option<RecommendedWatcher>,
    wake_rx: tokio::sync::mpsc::Receiver<()>,
}

fn project_folder_watch_wait_duration(
    next_settle_deadline: Option<Instant>,
    now: Instant,
) -> Duration {
    next_settle_deadline
        .map(|deadline| {
            deadline
                .saturating_duration_since(now)
                .min(PROJECT_FOLDER_FALLBACK_POLL)
        })
        .unwrap_or(PROJECT_FOLDER_FALLBACK_POLL)
}

impl ProjectFolderWatchTransport {
    pub async fn new(state: &AppState, app: &dyn PathResolver) -> Self {
        let (wake_tx, wake_rx) = tokio::sync::mpsc::channel(1);
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    return;
                };
                if event.paths.iter().any(|path| is_project_source_path(path)) {
                    cancel_active_project_folder_render();
                }
                let _ = wake_tx.try_send(());
            })
            .ok();

        if let Some(watcher) = watcher.as_mut() {
            let mut roots = HashSet::<PathBuf>::new();
            roots.insert(crate::project_mirror::projects_root(
                app,
                configured_projects_root(state).as_deref(),
            ));
            let bindings = {
                let conn = state.db.lock().await;
                crate::thread_source_binding::list_bindings(&conn).unwrap_or_default()
            };
            roots.extend(
                bindings
                    .into_iter()
                    .map(|binding| PathBuf::from(binding.folder_path)),
            );
            for root in roots {
                if root.exists() {
                    let _ = watcher.watch(&root, RecursiveMode::Recursive);
                }
            }
        }

        Self {
            _watcher: watcher,
            wake_rx,
        }
    }

    pub async fn wait_until(&mut self, next_settle_deadline: Option<Instant>) {
        let wait_duration =
            project_folder_watch_wait_duration(next_settle_deadline, Instant::now());
        tokio::select! {
            _ = tokio::time::sleep(wait_duration) => {}
            Some(_) = self.wake_rx.recv() => {}
        }
        while self.wake_rx.try_recv().is_ok() {}
    }
}

struct PendingProjectEdit {
    digest: String,
    observed_at: Instant,
}

async fn rebase_known_project_source(
    state: &AppState,
    dir: &Path,
    manifest: &crate::project_mirror::ProjectManifest,
    source_digest: &str,
) -> AppResult<bool> {
    let known_version = {
        let conn = state.db.lock().await;
        db::get_thread_latest_version(&conn, &manifest.thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    };
    let Some(known_version) = known_version else {
        return Ok(false);
    };
    if known_version.status != crate::contracts::MessageStatus::Success {
        return Ok(false);
    }
    let Some(known_source) = known_version
        .output
        .as_ref()
        .map(|output| &output.macro_code)
    else {
        return Ok(false);
    };
    if crate::project_mirror::source_digest(known_source) != source_digest {
        return Ok(false);
    }

    let model_id = known_version
        .artifact_bundle
        .as_ref()
        .map(|bundle| bundle.model_id.clone())
        .or_else(|| {
            known_version
                .model_manifest
                .as_ref()
                .map(|known_manifest| known_manifest.model_id.clone())
        });
    let rebased = crate::project_mirror::ProjectManifest {
        message_id: known_version.id,
        model_id,
        source_digest: source_digest.to_string(),
        exported_at: now_secs(),
        ..manifest.clone()
    };
    crate::project_mirror::write_manifest(dir, &rebased)?;
    {
        let conn = state.db.lock().await;
        crate::thread_source_binding::index_export(
            &conn,
            &rebased.thread_id,
            dir,
            &rebased.source_digest,
        )?;
    }
    Ok(true)
}

async fn known_failed_project_source(
    state: &AppState,
    thread_id: &str,
    source_digest: &str,
) -> AppResult<Option<String>> {
    let latest = {
        let conn = state.db.lock().await;
        db::get_thread_latest_version(&conn, thread_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    };
    let Some(latest) = latest else {
        return Ok(None);
    };
    if latest.status != crate::contracts::MessageStatus::Error
        || latest
            .agent_origin
            .as_ref()
            .is_none_or(|origin| origin.client_kind != "watcher")
        || latest.output.as_ref().is_none_or(|output| {
            crate::project_mirror::source_digest(&output.macro_code) != source_digest
        })
    {
        return Ok(None);
    }
    Ok(Some(latest.content))
}

async fn repair_non_success_project_manifest(
    state: &AppState,
    dir: &Path,
    manifest: &crate::project_mirror::ProjectManifest,
) -> AppResult<Option<crate::project_mirror::ProjectManifest>> {
    let latest_success = {
        let conn = state.db.lock().await;
        let manifest_status = conn
            .query_row(
                "SELECT status FROM messages
                 WHERE id = ?1 AND thread_id = ?2 AND deleted_at IS NULL",
                [&manifest.message_id, &manifest.thread_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| AppError::persistence(error.to_string()))?;
        if manifest_status.as_deref() == Some("success") {
            return Ok(None);
        }
        let Some(success_id) =
            db::get_latest_successful_message_id_in_thread(&conn, &manifest.thread_id)
                .map_err(|error| AppError::persistence(error.to_string()))?
        else {
            return Ok(None);
        };
        db::get_thread_message_version(&conn, &manifest.thread_id, &success_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    };
    let Some(latest_success) = latest_success else {
        return Ok(None);
    };
    let Some(source) = latest_success
        .output
        .as_ref()
        .map(|output| output.macro_code.as_str())
    else {
        return Ok(None);
    };
    let model_id = latest_success
        .artifact_bundle
        .as_ref()
        .map(|bundle| bundle.model_id.clone())
        .or_else(|| {
            latest_success
                .model_manifest
                .as_ref()
                .map(|known_manifest| known_manifest.model_id.clone())
        });
    let repaired = crate::project_mirror::ProjectManifest {
        message_id: latest_success.id,
        model_id,
        source_digest: crate::project_mirror::source_digest(source),
        exported_at: now_secs(),
        ..manifest.clone()
    };
    crate::project_mirror::write_manifest(dir, &repaired)?;
    {
        let conn = state.db.lock().await;
        crate::thread_source_binding::index_export(
            &conn,
            &repaired.thread_id,
            dir,
            &repaired.source_digest,
        )?;
    }
    Ok(Some(repaired))
}

async fn project_folder_watch_failure(
    state: &AppState,
    thread_id: &str,
) -> AppResult<Option<crate::thread_source_binding::ProjectFolderWatchFailure>> {
    let conn = state.db.lock().await;
    crate::thread_source_binding::get_project_folder_watch_failure(&conn, thread_id)
}

async fn clear_project_folder_watch_failure(state: &AppState, thread_id: &str) -> AppResult<()> {
    let conn = state.db.lock().await;
    crate::thread_source_binding::clear_project_folder_watch_failure(&conn, thread_id)
}

async fn remember_project_folder_watch_failure(
    state: &AppState,
    thread_id: &str,
    source_digest: &str,
    error: &str,
) -> AppResult<()> {
    let conn = state.db.lock().await;
    crate::thread_source_binding::set_project_folder_watch_failure(
        &conn,
        thread_id,
        source_digest,
        error,
        now_secs(),
    )
}

/// Digest debounce state. Same-digest events preserve the first observation
/// time, so event storms cannot postpone apply forever. Failed digests remain
/// memoized until content changes.
pub struct ProjectFolderWatcher {
    pending: HashMap<String, PendingProjectEdit>,
    failed: HashMap<String, String>,
    debounce: Duration,
    recovered_orphans: bool,
}

impl Default for ProjectFolderWatcher {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            failed: HashMap::new(),
            debounce: PROJECT_FOLDER_DEBOUNCE,
            recovered_orphans: false,
        }
    }
}

impl ProjectFolderWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn with_debounce(debounce: Duration) -> Self {
        Self {
            debounce,
            ..Self::default()
        }
    }

    pub fn next_settle_deadline(&self) -> Option<Instant> {
        self.pending
            .values()
            .map(|pending| pending.observed_at + self.debounce)
            .min()
    }

    pub async fn tick(
        &mut self,
        state: &AppState,
        app: &dyn PathResolver,
        ctx: &AgentContext,
    ) -> Vec<ProjectFolderWatchEvent> {
        let mut events = Vec::new();
        if !self.recovered_orphans {
            // Working versions are immutable authoring history. A process restart
            // may interrupt their build, but must never hide or delete them.
            self.recovered_orphans = true;
        }
        let active_thread_ids = state.active_authoring_thread_ids().await;
        let root = configured_projects_root(state);
        let mut folders = std::collections::BTreeMap::new();
        if let Ok(slugs) = crate::project_mirror::list_project_slugs(app, root.as_deref()) {
            for slug in slugs {
                if let Ok(dir) = crate::project_mirror::project_dir(app, root.as_deref(), &slug) {
                    folders.insert(slug, dir);
                }
            }
        }
        let bindings = {
            let conn = state.db.lock().await;
            let mut bindings =
                crate::thread_source_binding::list_bindings(&conn).unwrap_or_default();
            for binding in &mut bindings {
                let Ok(Some(provider_binding)) =
                    crate::services::codex_takeover::get_binding(&conn, &binding.thread_id)
                else {
                    continue;
                };
                let provider_folder = PathBuf::from(provider_binding.cwd);
                if provider_folder == binding.folder_path {
                    continue;
                }
                let Ok(Some(provider_manifest)) =
                    crate::project_mirror::read_manifest(&provider_folder)
                else {
                    continue;
                };
                if provider_manifest.thread_id != binding.thread_id
                    || !provider_folder
                        .join(crate::project_mirror::PROJECT_SOURCE_FILE_NAME)
                        .is_file()
                {
                    continue;
                }
                if let Ok(repaired) = crate::thread_source_binding::index_export(
                    &conn,
                    &binding.thread_id,
                    &provider_folder,
                    &provider_manifest.source_digest,
                ) {
                    *binding = repaired;
                }
            }
            bindings
        };
        let bound_folders = bindings
            .iter()
            .map(|binding| {
                (
                    binding.thread_id.clone(),
                    std::path::PathBuf::from(&binding.folder_path),
                )
            })
            .collect::<HashMap<_, _>>();
        for binding in bindings {
            let dir = std::path::PathBuf::from(binding.folder_path);
            if let Some(slug) = dir
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
            {
                // Stored binding wins over a same-name folder under the
                // currently configured root.
                folders.insert(slug, dir);
            }
        }
        folders.retain(|_, dir| {
            let Ok(Some(manifest)) = crate::project_mirror::read_manifest(dir) else {
                return true;
            };
            bound_folders
                .get(&manifest.thread_id)
                .is_none_or(|bound_folder| bound_folder == dir)
        });
        for (slug, dir) in folders {
            let Ok(Some(mut manifest)) = crate::project_mirror::read_manifest(&dir) else {
                continue;
            };
            if !active_thread_ids.contains(&manifest.thread_id) {
                self.pending.remove(&slug);
                state
                    .project_folder_render_activity
                    .lock()
                    .await
                    .remove(&slug);
                continue;
            }
            match repair_non_success_project_manifest(state, &dir, &manifest).await {
                Ok(Some(repaired)) => manifest = repaired,
                Ok(None) => {}
                Err(error) => {
                    events.push(ProjectFolderWatchEvent::ApplyFailed {
                        slug,
                        thread_id: manifest.thread_id,
                        message_id: manifest.message_id,
                        error: error.to_string(),
                    });
                    continue;
                }
            }
            let Ok(Some(source)) = crate::project_mirror::read_project_source(&dir) else {
                continue;
            };
            let digest = crate::project_mirror::source_digest(&source);
            if digest == manifest.source_digest {
                if let Err(error) =
                    clear_project_folder_watch_failure(state, &manifest.thread_id).await
                {
                    events.push(ProjectFolderWatchEvent::ApplyFailed {
                        slug,
                        thread_id: manifest.thread_id,
                        message_id: manifest.message_id,
                        error: error.to_string(),
                    });
                    continue;
                }
                self.pending.remove(&slug);
                self.failed.remove(&slug);
                state
                    .project_folder_render_activity
                    .lock()
                    .await
                    .remove(&slug);
                continue;
            }
            if self.failed.get(&slug) == Some(&digest) {
                continue;
            }
            let persisted_failure =
                match project_folder_watch_failure(state, &manifest.thread_id).await {
                    Ok(failure) => failure,
                    Err(error) => {
                        events.push(ProjectFolderWatchEvent::ApplyFailed {
                            slug,
                            thread_id: manifest.thread_id,
                            message_id: manifest.message_id,
                            error: error.to_string(),
                        });
                        continue;
                    }
                };
            if let Some(failure) = persisted_failure {
                if failure.source_digest == digest {
                    self.pending.remove(&slug);
                    self.failed.insert(slug.clone(), digest.clone());
                    state
                        .project_folder_render_activity
                        .lock()
                        .await
                        .remove(&slug);
                    continue;
                }
                if let Err(error) =
                    clear_project_folder_watch_failure(state, &manifest.thread_id).await
                {
                    events.push(ProjectFolderWatchEvent::ApplyFailed {
                        slug,
                        thread_id: manifest.thread_id,
                        message_id: manifest.message_id,
                        error: error.to_string(),
                    });
                    continue;
                }
            }
            match rebase_known_project_source(state, &dir, &manifest, &digest).await {
                Ok(true) => {
                    if let Err(error) =
                        clear_project_folder_watch_failure(state, &manifest.thread_id).await
                    {
                        events.push(ProjectFolderWatchEvent::ApplyFailed {
                            slug,
                            thread_id: manifest.thread_id,
                            message_id: manifest.message_id,
                            error: error.to_string(),
                        });
                        continue;
                    }
                    self.pending.remove(&slug);
                    self.failed.remove(&slug);
                    state
                        .project_folder_render_activity
                        .lock()
                        .await
                        .remove(&slug);
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    let error_text = error.to_string();
                    let error_text = match remember_project_folder_watch_failure(
                        state,
                        &manifest.thread_id,
                        &digest,
                        &error_text,
                    )
                    .await
                    {
                        Ok(()) => error_text,
                        Err(persistence_error) => {
                            format!("{error_text}\n{persistence_error}")
                        }
                    };
                    self.pending.remove(&slug);
                    self.failed.insert(slug.clone(), digest.clone());
                    state
                        .project_folder_render_activity
                        .lock()
                        .await
                        .remove(&slug);
                    events.push(ProjectFolderWatchEvent::ApplyFailed {
                        slug,
                        thread_id: manifest.thread_id,
                        message_id: manifest.message_id,
                        error: error_text,
                    });
                    continue;
                }
            }
            match known_failed_project_source(state, &manifest.thread_id, &digest).await {
                Ok(Some(error_text)) => {
                    let error_text = match remember_project_folder_watch_failure(
                        state,
                        &manifest.thread_id,
                        &digest,
                        &error_text,
                    )
                    .await
                    {
                        Ok(()) => error_text,
                        Err(persistence_error) => {
                            format!("{error_text}\n{persistence_error}")
                        }
                    };
                    self.pending.remove(&slug);
                    self.failed.insert(slug.clone(), digest.clone());
                    state
                        .project_folder_render_activity
                        .lock()
                        .await
                        .remove(&slug);
                    events.push(ProjectFolderWatchEvent::ApplyFailed {
                        slug,
                        thread_id: manifest.thread_id,
                        message_id: manifest.message_id,
                        error: error_text,
                    });
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    events.push(ProjectFolderWatchEvent::ApplyFailed {
                        slug,
                        thread_id: manifest.thread_id,
                        message_id: manifest.message_id,
                        error: error.to_string(),
                    });
                    continue;
                }
            }
            let now = Instant::now();
            let settled = self
                .pending
                .get(&slug)
                .filter(|pending| pending.digest == digest)
                .is_some_and(|pending| now.duration_since(pending.observed_at) >= self.debounce);
            if !settled {
                let is_new_digest = self
                    .pending
                    .get(&slug)
                    .is_none_or(|pending| pending.digest != digest);
                if is_new_digest {
                    self.pending.insert(
                        slug.clone(),
                        PendingProjectEdit {
                            digest: digest.clone(),
                            observed_at: now,
                        },
                    );
                }
                if !is_new_digest {
                    continue;
                }
                state.project_folder_render_activity.lock().await.insert(
                    slug.clone(),
                    crate::project_mirror::ProjectFolderRenderActivity {
                        slug: slug.clone(),
                        thread_id: manifest.thread_id.clone(),
                    },
                );
                events.push(ProjectFolderWatchEvent::Detected {
                    slug,
                    thread_id: manifest.thread_id,
                });
                continue;
            }

            let _apply_guard = project_folder_apply_lock().lock().await;
            match project_folder_watch_failure(state, &manifest.thread_id).await {
                Ok(Some(failure)) if failure.source_digest == digest => {
                    self.pending.remove(&slug);
                    self.failed.insert(slug.clone(), digest.clone());
                    state
                        .project_folder_render_activity
                        .lock()
                        .await
                        .remove(&slug);
                    continue;
                }
                Ok(_) => {}
                Err(error) => {
                    events.push(ProjectFolderWatchEvent::ApplyFailed {
                        slug,
                        thread_id: manifest.thread_id,
                        message_id: manifest.message_id,
                        error: error.to_string(),
                    });
                    continue;
                }
            }
            let Ok(Some(latest_source)) = crate::project_mirror::read_project_source(&dir) else {
                continue;
            };
            if crate::project_mirror::source_digest(&latest_source) != digest {
                self.pending.insert(
                    slug,
                    PendingProjectEdit {
                        digest: crate::project_mirror::source_digest(&latest_source),
                        observed_at: Instant::now(),
                    },
                );
                continue;
            }

            let cancellation = Arc::new(AtomicBool::new(false));
            let _active_render = ActiveProjectFolderRenderGuard::activate(cancellation.clone());
            let _source_cancellation = crate::services::render::register_source_render_cancellation(
                &source,
                cancellation.clone(),
            );
            let applied = handle_project_folder_apply(
                state,
                app,
                ProjectFolderApplyRequest {
                    identity: AgentIdentityOverride::default(),
                    slug: slug.clone(),
                    force: false,
                    title: None,
                    version_name: Some("folder-sync".to_string()),
                },
                ctx,
            )
            .await;
            self.pending.remove(&slug);
            if cancellation.load(Ordering::Acquire) {
                if let Ok(Some(latest_source)) = crate::project_mirror::read_project_source(&dir) {
                    let latest_digest = crate::project_mirror::source_digest(&latest_source);
                    if latest_digest != digest {
                        self.pending.insert(
                            slug.clone(),
                            PendingProjectEdit {
                                digest: latest_digest,
                                observed_at: Instant::now(),
                            },
                        );
                    }
                }
                continue;
            }
            state
                .project_folder_render_activity
                .lock()
                .await
                .remove(&slug);
            match applied {
                Ok(response) => {
                    if let Err(error) =
                        clear_project_folder_watch_failure(state, &response.thread_id).await
                    {
                        events.push(ProjectFolderWatchEvent::ApplyFailed {
                            slug,
                            thread_id: response.thread_id,
                            message_id: response.message_id,
                            error: error.to_string(),
                        });
                        continue;
                    }
                    self.failed.remove(&slug);
                    events.push(ProjectFolderWatchEvent::Applied {
                        slug,
                        thread_id: response.thread_id,
                        message_id: response.message_id,
                        model_id: response.model_id,
                    });
                }
                Err(err) => {
                    // Stale/conflict folders are not auto-resolved; like a
                    // compile failure they are reported once per digest.
                    let error_text = err.to_string();
                    let error_text = match remember_project_folder_watch_failure(
                        state,
                        &manifest.thread_id,
                        &digest,
                        &error_text,
                    )
                    .await
                    {
                        Ok(()) => error_text,
                        Err(persistence_error) => {
                            format!("{error_text}\n{persistence_error}")
                        }
                    };
                    self.failed.insert(slug.clone(), digest);
                    events.push(ProjectFolderWatchEvent::ApplyFailed {
                        slug,
                        thread_id: manifest.thread_id,
                        message_id: manifest.message_id,
                        error: error_text,
                    });
                }
            }
        }
        events
    }
}

pub fn project_folder_watcher_context() -> AgentContext {
    AgentContext {
        session_id: "project-folder-watcher".to_string(),
        client_kind: "watcher".to_string(),
        host_label: "Ecky".to_string(),
        agent_label: "folder-sync".to_string(),
        llm_model_id: None,
        llm_model_label: None,
    }
}

#[cfg(test)]
mod verification_gate_tests {
    use super::{
        cancel_active_project_folder_render, declared_separated_print_layout,
        is_project_source_path, only_nonblocking_disconnected_codes,
        project_folder_watch_wait_duration, ActiveProjectFolderRenderGuard,
        PROJECT_FOLDER_DEBOUNCE, PROJECT_FOLDER_FALLBACK_POLL,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn model_source_change_cancels_only_the_active_watcher_render() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let guard = ActiveProjectFolderRenderGuard::activate(cancellation.clone());

        assert!(is_project_source_path(std::path::Path::new("model.ecky")));
        assert!(!is_project_source_path(std::path::Path::new(
            "manifest.json"
        )));
        assert!(cancel_active_project_folder_render());
        assert!(cancellation.load(Ordering::Acquire));

        drop(guard);
        assert!(!cancel_active_project_folder_render());
    }

    #[test]
    fn pending_edit_deadline_preempts_fallback_poll() {
        let now = Instant::now();

        assert_eq!(
            project_folder_watch_wait_duration(Some(now + PROJECT_FOLDER_DEBOUNCE), now),
            PROJECT_FOLDER_DEBOUNCE,
        );
        assert_eq!(
            project_folder_watch_wait_duration(None, now),
            PROJECT_FOLDER_FALLBACK_POLL,
        );
    }

    #[test]
    fn separated_print_layout_requires_explicit_false_boolean() {
        let mut parameters = crate::contracts::DesignParams::new();
        assert!(!declared_separated_print_layout(&parameters));
        parameters.insert(
            "assembly-preview".to_string(),
            crate::contracts::ParamValue::Boolean(true),
        );
        assert!(!declared_separated_print_layout(&parameters));
        parameters.insert(
            "assembly-preview".to_string(),
            crate::contracts::ParamValue::Boolean(false),
        );
        assert!(declared_separated_print_layout(&parameters));
    }

    #[test]
    fn separated_print_layout_never_waives_real_structural_failures() {
        assert!(only_nonblocking_disconnected_codes(
            ["PREVIEW_STL_DISCONNECTED_COMPONENTS"],
            false,
        ));
        assert!(!only_nonblocking_disconnected_codes(
            ["PART_DISCONNECTED"],
            false,
        ));
        assert!(only_nonblocking_disconnected_codes(
            ["PART_DISCONNECTED", "PREVIEW_STL_DISCONNECTED_COMPONENTS"],
            true,
        ));
        assert!(!only_nonblocking_disconnected_codes(
            ["PART_DISCONNECTED", "PREVIEW_STL_MISSING"],
            true,
        ));
        assert!(!only_nonblocking_disconnected_codes(
            std::iter::empty(),
            true,
        ));
    }
}
