use rusqlite::OptionalExtension;
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager, State};

use crate::contracts::{AppError, AppResult, Config};
use crate::models::AppState;
#[cfg(test)]
use crate::thread_source_binding::ThreadSourceBinding;

pub(crate) fn persist_config_transaction(
    config_dir: &Path,
    mut config: Config,
    state: &AppState,
) -> AppResult<crate::config_store::SaveOutcome> {
    crate::mcp::runtime::ensure_primary_agent_id(&mut config);
    let state_config = state.config.clone();
    let state_status = state.config_persistence_status.clone();
    crate::config_store::save_config_transaction(config_dir, config, move |normalized, outcome| {
        let mut config_guard = state_config
            .lock()
            .map_err(|_| crate::contracts::AppError::persistence("config state update failed"))?;
        let mut status_guard = state_status
            .lock()
            .map_err(|_| crate::contracts::AppError::persistence("config status update failed"))?;
        *config_guard = normalized.clone();
        *status_guard = crate::models::ConfigPersistenceStatus {
            cleanup_pending: outcome.cleanup_pending,
            warnings: outcome.warnings.clone(),
        };
        Ok(outcome.clone())
    })
}

fn open_path_in_system_editor(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        command_status(macos_editor_command(path).status()?)
    }

    #[cfg(target_os = "windows")]
    {
        command_status(
            Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(path)
                .status()?,
        )
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        command_status(Command::new("xdg-open").arg(path).status()?)
    }
}

fn open_path_in_default_app(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        command_status(Command::new("open").arg(path).status()?)
    }
    #[cfg(target_os = "windows")]
    {
        command_status(
            Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(path)
                .status()?,
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        command_status(Command::new("xdg-open").arg(path).status()?)
    }
}

#[cfg(target_os = "macos")]
fn macos_editor_command(path: &Path) -> Command {
    let mut command = Command::new("open");
    // `open <file>` depends on a LaunchServices association. `.ecky` commonly
    // has none, while `-t` deliberately routes text source to the user's text
    // editor without teaching macOS a fake application association.
    command.arg("-t").arg(path);
    command
}

fn reveal_path_in_file_manager(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        command_status(Command::new("open").arg(path).status()?)
    }

    #[cfg(target_os = "windows")]
    {
        command_status(Command::new("explorer").arg(path).status()?)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        command_status(Command::new("xdg-open").arg(path).status()?)
    }
}

fn command_status(status: std::process::ExitStatus) -> std::io::Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "launcher exited with {status}"
        )))
    }
}

/// Resolve the exact stored paths for OPEN FILE. The binding is authoritative
/// after backfill, so title and projects-root changes cannot redirect it.
#[cfg(test)]
fn resolve_editor_paths(binding: &ThreadSourceBinding) -> (PathBuf, PathBuf) {
    (
        PathBuf::from(&binding.folder_path),
        PathBuf::from(&binding.source_path),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn get_config(state: State<'_, AppState>) -> AppResult<Config> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
#[specta::specta]
pub async fn save_config(
    config: Config,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    let config_dir = crate::models::PathResolver::try_app_config_dir(&app)?;
    let saved = persist_config_transaction(&config_dir, config, state.inner())?;
    let state_for_sync = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::mcp::runtime::sync_auto_agent_supervisors(state_for_sync);
    });
    for warning in saved.warnings {
        state.push_log(warning);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn list_agent_models(cmd: String) -> AppResult<crate::contracts::AgentModelList> {
    crate::llm::list_agent_models(&cmd)
        .await
        .map_err(crate::contracts::AppError::provider)
}

#[tauri::command]
#[specta::specta]
pub async fn list_provider_models(
    provider: String,
    state: State<'_, AppState>,
) -> AppResult<crate::contracts::AgentModelList> {
    match provider.as_str() {
        "codex" => Ok(crate::contracts::AgentModelList {
            models: state.codex_app_server.list_models().await?,
            is_live: true,
        }),
        "agy" => crate::llm::list_agent_models("agy")
            .await
            .map_err(crate::contracts::AppError::provider),
        _ => Err(AppError::validation(format!(
            "Unsupported provider model catalog '{provider}'."
        ))),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn list_models(
    provider: String,
    api_key: String,
    base_url: String,
) -> AppResult<Vec<String>> {
    crate::llm::list_models(&provider, &api_key, &base_url)
        .await
        .map_err(crate::contracts::AppError::provider)
}

#[tauri::command]
#[specta::specta]
pub async fn get_design_system_prompt(
    provider: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let _ = provider;
    let (source_language, geometry_backend) = {
        let config = state.config.lock().unwrap();
        (
            config.default_source_language,
            config.default_geometry_backend,
        )
    };
    Ok(crate::commands::generation::design_system_prompt(
        source_language,
        geometry_backend,
        None,
    ))
}

#[tauri::command]
#[specta::specta]
pub async fn get_app_logs(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::contracts::AppLogEntry>> {
    let logs = state.app_logs.lock().unwrap();
    Ok(logs.iter().cloned().collect())
}

#[tauri::command]
#[specta::specta]
pub async fn export_ecky_mcp_skill_zip(target_path: String) -> AppResult<()> {
    let skill_dir = resolve_ecky_mcp_skill_dir()?;
    export_ecky_mcp_skill_zip_impl(&skill_dir, Path::new(&target_path))
}

fn resolve_ecky_mcp_skill_dir() -> AppResult<PathBuf> {
    if let Some(path) = env::var_os("ECKY_MCP_SKILL_DIR").map(PathBuf::from) {
        if is_ecky_mcp_skill_dir(&path) {
            return Ok(path);
        }
    }

    let mut candidates = Vec::new();
    // Prefer the repo-owned, generated skill when present (dev/source checkout).
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../skills/ecky-mcp"));
    if let Some(codex_home) = env::var_os("CODEX_HOME").map(PathBuf::from) {
        candidates.push(codex_home.join("skills").join("ecky-mcp"));
    }
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        candidates.push(home.join(".codex-personal").join("skills").join("ecky-mcp"));
        candidates.push(home.join(".codex").join("skills").join("ecky-mcp"));
    }

    candidates
        .into_iter()
        .find(|candidate| is_ecky_mcp_skill_dir(candidate))
        .ok_or_else(|| {
            crate::contracts::AppError::validation(
                "Ecky MCP skill is not installed. Install it under CODEX_HOME/skills/ecky-mcp or ~/.codex-personal/skills/ecky-mcp.",
            )
        })
}

fn is_ecky_mcp_skill_dir(path: &Path) -> bool {
    path.join("SKILL.md").is_file()
        && fs::read_to_string(path.join("SKILL.md"))
            .map(|content| content.contains("name: ecky-mcp"))
            .unwrap_or(false)
}

fn export_ecky_mcp_skill_zip_impl(skill_dir: &Path, target_path: &Path) -> AppResult<()> {
    if target_path.as_os_str().is_empty() {
        return Err(crate::contracts::AppError::validation(
            "Export path is required for Ecky MCP skill zip.",
        ));
    }
    if !is_ecky_mcp_skill_dir(skill_dir) {
        return Err(crate::contracts::AppError::validation(format!(
            "Ecky MCP skill directory is invalid: {}",
            skill_dir.display()
        )));
    }
    if let Some(parent) = target_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| {
            crate::contracts::AppError::persistence(format!(
                "Failed to create export directory '{}': {}",
                parent.display(),
                err
            ))
        })?;
    }

    let file = fs::File::create(target_path).map_err(|err| {
        crate::contracts::AppError::persistence(format!(
            "Failed to create Ecky MCP skill zip '{}': {}",
            target_path.display(),
            err
        ))
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let skill_root_name = skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ecky-mcp");

    for path in collect_skill_files(skill_dir)? {
        let rel = path.strip_prefix(skill_dir).map_err(|err| {
            crate::contracts::AppError::persistence(format!(
                "Failed to resolve skill archive path '{}': {}",
                path.display(),
                err
            ))
        })?;
        let archive_name = Path::new(skill_root_name).join(rel);
        let archive_name = archive_name.to_string_lossy().replace('\\', "/");
        zip.start_file(&archive_name, options).map_err(|err| {
            crate::contracts::AppError::persistence(format!(
                "Failed to write skill archive entry '{}': {}",
                archive_name, err
            ))
        })?;
        let mut source = fs::File::open(&path).map_err(|err| {
            crate::contracts::AppError::persistence(format!(
                "Failed to open skill file '{}': {}",
                path.display(),
                err
            ))
        })?;
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).map_err(|err| {
            crate::contracts::AppError::persistence(format!(
                "Failed to read skill file '{}': {}",
                path.display(),
                err
            ))
        })?;
        zip.write_all(&bytes).map_err(|err| {
            crate::contracts::AppError::persistence(format!(
                "Failed to write skill file '{}': {}",
                archive_name, err
            ))
        })?;
    }

    zip.finish().map_err(|err| {
        crate::contracts::AppError::persistence(format!(
            "Failed to finalize Ecky MCP skill zip '{}': {}",
            target_path.display(),
            err
        ))
    })?;
    Ok(())
}

fn collect_skill_files(skill_dir: &Path) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_skill_files_inner(skill_dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_skill_files_inner(dir: &Path, files: &mut Vec<PathBuf>) -> AppResult<()> {
    let entries = fs::read_dir(dir).map_err(|err| {
        crate::contracts::AppError::persistence(format!(
            "Failed to read skill directory '{}': {}",
            dir.display(),
            err
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            crate::contracts::AppError::persistence(format!(
                "Failed to read skill directory entry '{}': {}",
                dir.display(),
                err
            ))
        })?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if file_name == "__pycache__" || file_name == ".DS_Store" {
            continue;
        }
        let metadata = entry.metadata().map_err(|err| {
            crate::contracts::AppError::persistence(format!(
                "Failed to inspect skill path '{}': {}",
                path.display(),
                err
            ))
        })?;
        if metadata.is_dir() {
            collect_skill_files_inner(&path, files)?;
        } else if metadata.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        Asset, AutoAgent, Config, EngineKind, GeometryBackend, McpConfig, SourceLanguage,
        VoiceConfig,
    };
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::ZipArchive;

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("ecky-skill-export-{name}-{unique}"))
    }

    fn config(selected: &str) -> Config {
        Config {
            engines: vec![],
            selected_engine_id: selected.into(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: vec![],
            assets: vec![],
            microwave: None,
            voice: VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: false,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            jev_classifier: Default::default(),
            default_engine_kind: EngineKind::EckyIrV0,
            default_source_language: SourceLanguage::EckyIrV0,
            default_geometry_backend: GeometryBackend::Build123d,
            max_generation_attempts: 3,
            max_verify_attempts: 2,
            projects_root: None,
        }
    }

    fn state(config: Config) -> AppState {
        AppState::new(
            config,
            None,
            rusqlite::Connection::open_in_memory().unwrap(),
        )
    }

    #[test]
    fn bdd_save_transaction_commits_normalized_config_to_disk_and_memory() {
        let root = temp_dir("save-normalized");
        let state = state(config("old"));
        let mut requested = config("new");
        requested.mcp.auto_agents.push(AutoAgent {
            id: "agent".into(),
            label: "Agent".into(),
            cmd: "secret-command".into(),
            model: None,
            args: vec![],
            enabled: true,
            start_on_demand: true,
        });

        persist_config_transaction(&root, requested, &state).unwrap();

        let memory = state.config.lock().unwrap().clone();
        assert!(!memory.mcp.auto_agents[0].start_on_demand);
        let disk =
            crate::config_store::load_config(&root, config("fallback"), |config, _| Ok(config))
                .unwrap()
                .config;
        assert_eq!(disk, memory);
        let status = state.config_persistence_status.lock().unwrap().clone();
        assert!(!status.cleanup_pending);
        assert!(status
            .warnings
            .iter()
            .any(|warning| { warning.contains("CONFIG_DEPRECATED_START_ON_DEMAND_DROPPED") }));
        assert!(!format!("{status:?}").contains("secret-command"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_tauri_camel_case_config_payload_persists_edn_only() {
        let root = temp_dir("tauri-camel-save");
        let state = state(config("old"));
        let payload = serde_json::to_string(&config("camel-case-save")).unwrap();
        assert!(payload.contains("selectedEngineId"));
        assert!(!payload.contains("selected_engine_id"));
        let request: Config = serde_json::from_str(&payload).unwrap();

        persist_config_transaction(&root, request, &state).unwrap();

        assert!(root.join(crate::config_store::CONFIG_EDN_FILE).is_file());
        assert!(!root.join(crate::config_store::CONFIG_JSON_FILE).exists());
        let reopened =
            crate::config_store::load_config(&root, config("fallback"), |config, _| Ok(config))
                .unwrap()
                .config;
        assert_eq!(reopened, *state.config.lock().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_tauri_camel_case_start_on_demand_normalizes_with_only_static_warning() {
        let root = temp_dir("tauri-start-on-demand");
        let state = state(config("old"));
        let mut legacy = config("legacy-agent");
        legacy.mcp.auto_agents.push(AutoAgent {
            id: "agent".into(),
            label: "Agent".into(),
            cmd: "secret-command".into(),
            model: None,
            args: vec!["secret-arg".into()],
            enabled: true,
            start_on_demand: true,
        });
        let payload = serde_json::to_string(&legacy).unwrap();
        assert!(payload.contains("startOnDemand"));
        let request: Config = serde_json::from_str(&payload).unwrap();

        persist_config_transaction(&root, request, &state).unwrap();

        let status = state.config_persistence_status.lock().unwrap().clone();
        assert_eq!(
            status.warnings,
            vec!["CONFIG_DEPRECATED_START_ON_DEMAND_DROPPED: mcp.autoAgents[].startOnDemand"]
        );
        assert!(!state.config.lock().unwrap().mcp.auto_agents[0].start_on_demand);
        let edn = fs::read_to_string(root.join(crate::config_store::CONFIG_EDN_FILE)).unwrap();
        assert!(!edn.contains("start-on-demand"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_save_persistence_failure_keeps_old_disk_and_memory() {
        let root = temp_dir("save-failure");
        let old = config("old");
        crate::config_store::save_config(&root, old.clone()).unwrap();
        let state = state(old.clone());
        let mut invalid = config("new");
        invalid.assets.push(Asset {
            id: "bad".into(),
            name: "bad".into(),
            path: "relative-sentinel-secret".into(),
            format: "PNG".into(),
        });

        let error = persist_config_transaction(&root, invalid, &state).unwrap_err();

        assert!(!error.to_string().contains("sentinel-secret"));
        assert_eq!(*state.config.lock().unwrap(), old);
        let disk =
            crate::config_store::load_config(&root, config("fallback"), |config, _| Ok(config))
                .unwrap()
                .config;
        assert_eq!(disk, old);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_concurrent_save_transactions_leave_disk_equal_to_memory() {
        let root = temp_dir("save-concurrent");
        let state = state(config("old"));
        let first_root = root.clone();
        let first_state = state.clone();
        let first = std::thread::spawn(move || {
            persist_config_transaction(&first_root, config("one"), &first_state)
        });
        let second_root = root.clone();
        let second_state = state.clone();
        let second = std::thread::spawn(move || {
            persist_config_transaction(&second_root, config("two"), &second_state)
        });
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();

        let disk =
            crate::config_store::load_config(&root, config("fallback"), |config, _| Ok(config))
                .unwrap()
                .config;
        assert_eq!(disk, *state.config.lock().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bdd_save_cleanup_pending_updates_safe_status_and_warning() {
        let root = temp_dir("save-cleanup");
        fs::create_dir_all(root.join(crate::config_store::CONFIG_JSON_FILE)).unwrap();
        let state = state(config("old"));

        let outcome = persist_config_transaction(&root, config("new"), &state).unwrap();

        assert!(outcome.cleanup_pending);
        let status = state.config_persistence_status.lock().unwrap().clone();
        assert!(status.cleanup_pending);
        assert_eq!(status.warnings, vec!["config.json: cleanup-pending"]);

        fs::remove_dir(root.join(crate::config_store::CONFIG_JSON_FILE)).unwrap();
        fs::write(
            root.join(crate::config_store::CONFIG_JSON_FILE),
            "{stale-invalid-json",
        )
        .unwrap();
        let retried = persist_config_transaction(&root, config("newer"), &state).unwrap();
        assert!(!retried.cleanup_pending);
        let recovered = state.config_persistence_status.lock().unwrap().clone();
        assert!(!recovered.cleanup_pending);
        assert!(recovered.warnings.is_empty());
        assert!(!root.join(crate::config_store::CONFIG_JSON_FILE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_ecky_mcp_skill_zip_packages_skill_root() {
        let root = temp_dir("ok");
        let skill_dir = root.join("ecky-mcp");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: ecky-mcp\ndescription: test\n---\n",
        )
        .unwrap();
        fs::write(
            skill_dir.join("references").join("tool-catalog.md"),
            "tools",
        )
        .unwrap();
        let archive_path = root.join("export").join("ecky-mcp.zip");

        export_ecky_mcp_skill_zip_impl(&skill_dir, &archive_path).unwrap();

        let file = fs::File::open(&archive_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert!(archive.by_name("ecky-mcp/SKILL.md").is_ok());
        assert!(archive
            .by_name("ecky-mcp/references/tool-catalog.md")
            .is_ok());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_ecky_mcp_skill_zip_rejects_missing_skill() {
        let root = temp_dir("missing");
        fs::create_dir_all(&root).unwrap();
        let err = export_ecky_mcp_skill_zip_impl(&root, &root.join("out.zip")).unwrap_err();

        assert!(err.message.contains("invalid"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_editor_paths_bound_ignores_renamed_title_or_changed_root() {
        let binding = ThreadSourceBinding {
            thread_id: "thread-1".to_string(),
            folder_path: "/stored/root/bracket".to_string(),
            source_path: "/stored/root/bracket/model.ecky".to_string(),
            source_digest: "sha256:deadbeef".to_string(),
            created_at: 0,
            updated_at: 0,
        };

        // Derived inputs reflect a renamed title and a changed projectsRoot,
        // but a stored binding is the source of truth and must win.
        let (folder, source) = resolve_editor_paths(&binding);

        assert_eq!(folder, PathBuf::from("/stored/root/bracket"));
        assert_eq!(source, PathBuf::from("/stored/root/bracket/model.ecky"));
    }

    #[test]
    fn new_blank_reuses_only_latest_thread_with_zero_messages() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, updated_at INTEGER NOT NULL,
                deleted_at INTEGER, status TEXT, finalized_at INTEGER
             );
             CREATE TABLE messages (
                id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, status TEXT NOT NULL,
                deleted_at INTEGER
             );
             CREATE TABLE thread_source_bindings (
                thread_id TEXT PRIMARY KEY, folder_path TEXT NOT NULL,
                source_path TEXT NOT NULL, source_digest TEXT NOT NULL,
                created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
             );
             INSERT INTO threads VALUES
                ('empty-old', 'Empty old', 10, NULL, 'active', NULL),
                ('has-code', 'Has code', 30, NULL, 'active', NULL),
                ('empty-new', 'Empty new', 20, NULL, 'active', NULL);
             INSERT INTO thread_source_bindings VALUES
                ('empty-old', '/p/old', '/p/old/model.ecky', 'a', 1, 1),
                ('has-code', '/p/code', '/p/code/model.ecky', 'b', 1, 1),
                ('empty-new', '/p/new', '/p/new/model.ecky', 'c', 1, 1);
             INSERT INTO messages VALUES ('message-1', 'has-code', 'success', NULL);",
        )
        .unwrap();

        assert_eq!(
            latest_empty_bound_thread_id(&conn).unwrap().as_deref(),
            Some("empty-new")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bdd_macos_unknown_ecky_extension_uses_text_editor_launch_mode() {
        let command = macos_editor_command(Path::new("/tmp/model.ecky"));
        assert_eq!(command.get_program(), "open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![
                std::ffi::OsStr::new("-t"),
                std::ffi::OsStr::new("/tmp/model.ecky")
            ],
        );
    }
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEditorLink {
    pub slug: String,
    pub folder: String,
    pub file: String,
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceDocument {
    pub thread_id: String,
    pub slug: String,
    pub folder: String,
    pub file: String,
    pub source: String,
}

fn imported_cad_file_for_message(
    conn: &rusqlite::Connection,
    document: &ProjectSourceDocument,
    message_id: Option<&str>,
) -> AppResult<Option<PathBuf>> {
    let Some(message_id) = message_id else {
        return Ok(None);
    };
    let Some((_, Some(manifest), owner_thread_id)) =
        crate::db::get_message_runtime_and_thread(conn, message_id)
            .map_err(|error| AppError::persistence(error.to_string()))?
    else {
        return Ok(None);
    };
    if owner_thread_id != document.thread_id
        || !matches!(
            manifest.source_kind,
            crate::contracts::ModelSourceKind::ImportedFcstd
                | crate::contracts::ModelSourceKind::ImportedStep
        )
    {
        return Ok(None);
    }
    let Some(file_name) = manifest
        .document
        .source_path
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
    else {
        return Ok(None);
    };
    let candidate = Path::new(&document.folder).join(file_name);
    Ok(candidate.is_file().then_some(candidate))
}

fn latest_empty_bound_thread_id(conn: &rusqlite::Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT t.id
         FROM threads t
         JOIN thread_source_bindings b ON b.thread_id = t.id
         WHERE t.deleted_at IS NULL
           AND COALESCE(t.status, 'active') = 'active'
           AND t.finalized_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM messages m
               WHERE m.thread_id = t.id
                 AND m.deleted_at IS NULL
                 AND m.status != 'discarded'
           )
         ORDER BY t.updated_at DESC, t.id DESC
         LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

#[tauri::command]
#[specta::specta]
pub async fn open_or_create_blank_design_thread(
    title: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectSourceDocument> {
    let title = title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Untitled design".to_string());
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    let reusable_thread_id = latest_empty_bound_thread_id(&conn)
        .map_err(|err| crate::contracts::AppError::persistence(err.to_string()))?;
    if let Some(thread_id) = reusable_thread_id {
        let document =
            ensure_project_source_document(&app, &conn, projects_root.as_deref(), &thread_id)?;
        if crate::thread_source_binding::is_blank_thread_source(&document.source) {
            if document.source != crate::thread_source_binding::DEFAULT_THREAD_SOURCE {
                let binding = crate::thread_source_binding::get_binding(&conn, &thread_id)
                    .map_err(|err| crate::contracts::AppError::persistence(err.to_string()))?
                    .ok_or_else(|| {
                        crate::contracts::AppError::not_found(format!(
                            "Source binding for design thread '{thread_id}' not found."
                        ))
                    })?;
                crate::thread_source_binding::migrate_legacy_blank_source(&conn, &binding)?;
                return ensure_project_source_document(
                    &app,
                    &conn,
                    projects_root.as_deref(),
                    &thread_id,
                );
            }
            return Ok(document);
        }
    }

    let thread_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    crate::db::create_or_update_thread(&conn, &thread_id, &title, now, None)
        .map_err(|err| crate::contracts::AppError::persistence(err.to_string()))?;
    crate::thread_source_binding::bind_new_thread(
        &app,
        &conn,
        projects_root.as_deref(),
        &thread_id,
        &title,
    )?;
    ensure_project_source_document(&app, &conn, projects_root.as_deref(), &thread_id)
}

/// Read the authoritative bound `model.ecky` for a design thread.
/// Missing legacy bindings are created before the file is read.
#[tauri::command]
#[specta::specta]
pub async fn get_project_source(
    thread_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectSourceDocument> {
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    ensure_project_source_document(&app, &conn, projects_root.as_deref(), &thread_id)
}

/// Discover imported mesh nodes from the authoritative bound `model.ecky`.
/// Returned paths point at raw source assets; previewing them does not render
/// or solidify the model.
#[tauri::command]
#[specta::specta]
pub async fn list_external_shape_sources(
    thread_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::external_shapes::ExternalShapeSource>> {
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    let document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &thread_id)?;
    let sources = crate::external_shapes::discover_bound_external_shapes(
        &document.source,
        Path::new(&document.folder),
    )?;
    let scope = app.asset_protocol_scope();
    for source in sources.iter().filter(|source| source.exists) {
        scope.allow_file(&source.path).map_err(|error| {
            crate::contracts::AppError::validation(format!(
                "Bound external shape cannot be exposed to Viewer '{}': {error}",
                source.path
            ))
        })?;
    }
    Ok(sources)
}

#[tauri::command]
#[specta::specta]
pub async fn apply_external_shape_plane_crop(
    request: crate::external_shapes::ApplyExternalShapePlaneCropRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::external_shapes::ApplyExternalShapePlaneCropResult> {
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    let document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &request.thread_id)?;
    let result = crate::external_shapes::apply_plane_crop_to_source(
        &document.source,
        Path::new(&document.folder),
        &request,
    )?;
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_external_shape_plane_crop(
    request: crate::external_shapes::RemoveExternalShapePlaneCropRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::external_shapes::RemoveExternalShapePlaneCropResult> {
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    let document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &request.thread_id)?;
    let result = crate::external_shapes::remove_plane_crop_from_source(
        &document.source,
        Path::new(&document.folder),
        &request,
    )?;
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub async fn preview_external_shape_surface_trim_path(
    request: crate::surface_trim_external_shapes::SurfaceTrimPathPreviewRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::surface_trim_external_shapes::SurfaceTrimPathPreviewResponse> {
    crate::surface_trim_external_shapes::require_surface_trim_schema_version(
        request.schema_version,
    )?;
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    validate_surface_trim_target_message(
        &conn,
        &request.thread_id,
        request.target_message_id.as_deref(),
    )?;
    let document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &request.thread_id)?;
    let source_path = crate::surface_trim_source::resolve_surface_trim_source_path(
        &document.source,
        Path::new(&document.folder),
        request.node_id,
        &request.expected_source_digest,
        &request.expected_mesh_content_digest,
    )?;
    let path = crate::surface_trim_external_shapes::surface_trim_path(
        &source_path,
        &request.from_anchor,
        &request.to_anchor,
        request.path_mode,
    )?;
    Ok(
        crate::surface_trim_external_shapes::SurfaceTrimPathPreviewResponse {
            preview_id: request.preview_id,
            path,
        },
    )
}

#[tauri::command]
#[specta::specta]
pub async fn preview_external_shape_surface_trim_loop(
    request: crate::surface_trim_external_shapes::SurfaceTrimLoopPreviewRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::surface_trim_external_shapes::SurfaceTrimLoopPreviewResponse> {
    crate::surface_trim_external_shapes::require_surface_trim_schema_version(
        request.schema_version,
    )?;
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    validate_surface_trim_target_message(
        &conn,
        &request.thread_id,
        request.target_message_id.as_deref(),
    )?;
    let document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &request.thread_id)?;
    let source_path = crate::surface_trim_source::resolve_surface_trim_source_path(
        &document.source,
        Path::new(&document.folder),
        request.node_id,
        &request.expected_source_digest,
        &request.expected_mesh_content_digest,
    )?;
    crate::surface_trim_external_shapes::preview_surface_trim_loop(
        &source_path,
        &request.loop_anchors,
        request.path_mode,
        request.preview_id,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn preview_external_shape_surface_trim_region(
    request: crate::surface_trim_external_shapes::SurfaceTrimRegionPreviewRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::surface_trim_external_shapes::SurfaceTrimRegionPreviewResponse> {
    crate::surface_trim_external_shapes::require_surface_trim_schema_version(
        request.schema_version,
    )?;
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    validate_surface_trim_target_message(
        &conn,
        &request.thread_id,
        request.target_message_id.as_deref(),
    )?;
    let document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &request.thread_id)?;
    let source_path = crate::surface_trim_source::resolve_surface_trim_source_path(
        &document.source,
        Path::new(&document.folder),
        request.node_id,
        &request.expected_source_digest,
        &request.expected_mesh_content_digest,
    )?;
    let preview = crate::surface_trim_external_shapes::preview_surface_trim_region(
        &source_path,
        &request.loop_anchors,
        &request.keep_seed,
        request.path_mode,
    )?;
    let loop_anchors = request
        .loop_anchors
        .iter()
        .map(
            |anchor| crate::surface_trim_runtime::CanonicalSurfaceTrimAnchor {
                triangle_index: anchor.triangle_index,
                barycentric: anchor.barycentric,
            },
        )
        .collect::<Vec<_>>();
    let keep_seed = crate::surface_trim_runtime::CanonicalSurfaceTrimAnchor {
        triangle_index: request.keep_seed.triangle_index,
        barycentric: request.keep_seed.barycentric,
    };
    let runtime = crate::surface_trim_runtime::execute_surface_trim(
        &source_path,
        &request.expected_mesh_content_digest,
        &loop_anchors,
        &keep_seed,
        request.path_mode,
        request.cap_mode,
    )?;
    let cap_triangle_count = runtime
        .cap_reports
        .iter()
        .map(|report| report.added_triangle_count as usize)
        .sum::<usize>();
    let cap_preview = if cap_triangle_count == 0 {
        None
    } else {
        let cap_start = runtime
            .triangles
            .len()
            .checked_sub(cap_triangle_count)
            .ok_or_else(|| {
                AppError::internal("Surface trim cap report exceeds runtime triangle output.")
            })?;
        Some(crate::surface_trim_external_shapes::SurfaceTrimCapPreview {
            vertices: runtime.vertices.clone(),
            triangles: runtime.triangles[cap_start..].to_vec(),
        })
    };
    Ok(
        crate::surface_trim_external_shapes::SurfaceTrimRegionPreviewResponse {
            preview_id: request.preview_id,
            preview,
            topology: runtime.diagnostics,
            cap_reports: runtime.cap_reports,
            cap_preview,
        },
    )
}

#[tauri::command]
#[specta::specta]
pub async fn apply_external_shape_surface_trim(
    request: crate::surface_trim_source::ApplySurfaceTrimRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::surface_trim_source::ApplySurfaceTrimResult> {
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    validate_surface_trim_target_message(
        &conn,
        &request.thread_id,
        request.target_message_id.as_deref(),
    )?;
    let document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &request.thread_id)?;
    let result = crate::surface_trim_source::apply_surface_trim_to_source(
        &document.source,
        Path::new(&document.folder),
        &request,
    )?;
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_external_shape_surface_trim(
    request: crate::surface_trim_source::RemoveSurfaceTrimRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::surface_trim_source::RemoveSurfaceTrimResult> {
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    validate_surface_trim_target_message(
        &conn,
        &request.thread_id,
        request.target_message_id.as_deref(),
    )?;
    let document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &request.thread_id)?;
    let result =
        crate::surface_trim_source::remove_surface_trim_from_source(&document.source, &request)?;
    Ok(result)
}

fn validate_surface_trim_target_message(
    conn: &rusqlite::Connection,
    thread_id: &str,
    target_message_id: Option<&str>,
) -> AppResult<()> {
    let Some(message_id) = target_message_id else {
        return Ok(());
    };
    let owner = crate::db::get_visible_message_thread_id(conn, message_id).map_err(|error| {
        AppError::internal(format!(
            "Failed to validate surface trim target snapshot: {error}"
        ))
    })?;
    match owner.as_deref() {
        Some(owner_thread_id) if owner_thread_id == thread_id => Ok(()),
        Some(owner_thread_id) => Err(AppError::conflict(format!(
            "Surface trim target message '{}' belongs to thread '{}', not '{}'.",
            message_id, owner_thread_id, thread_id
        ))),
        None => Err(AppError::conflict(format!(
            "Surface trim target message '{}' is missing or no longer visible.",
            message_id
        ))),
    }
}

/// Atomically replace the one bound source used by editor, renderer, and diff.
/// The project watcher observes the changed bytes and appends their version;
/// no second commit/finalize action exists.
#[tauri::command]
#[specta::specta]
pub async fn save_project_source(
    thread_id: String,
    source: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectSourceDocument> {
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let conn = state.db.lock().await;
    let mut document =
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &thread_id)?;
    crate::project_mirror::write_bound_source(Path::new(&document.file), &source)?;
    document.source = source;
    Ok(document)
}

fn ensure_project_source_document(
    app: &dyn crate::models::PathResolver,
    conn: &rusqlite::Connection,
    projects_root: Option<&str>,
    thread_id: &str,
) -> AppResult<ProjectSourceDocument> {
    let binding = match crate::thread_source_binding::get_binding(conn, thread_id)
        .map_err(|err| crate::contracts::AppError::persistence(err.to_string()))?
    {
        Some(binding) => binding,
        None => {
            if crate::db::get_thread_title(conn, thread_id)
                .map_err(|err| crate::contracts::AppError::persistence(err.to_string()))?
                .is_none()
            {
                return Err(crate::contracts::AppError::not_found(format!(
                    "Design thread '{thread_id}' not found."
                )));
            } else {
                let target = crate::services::target::resolve_editable_target(
                    conn,
                    app,
                    Some(thread_id.to_string()),
                    None,
                )?;
                crate::thread_source_binding::prepare_editor_source(
                    app,
                    conn,
                    projects_root,
                    &target.thread_id,
                    &target.design_output.title,
                    &target.design_output.macro_code,
                    &target.message_id,
                    target.model_id().as_deref(),
                )?
            }
        }
    };
    let folder = PathBuf::from(&binding.folder_path);
    let file = PathBuf::from(&binding.source_path);
    let (_, slug) = crate::thread_source_binding::stored_folder_export_args(&folder)?;
    let source = fs::read_to_string(&file).map_err(|err| {
        crate::contracts::AppError::persistence(format!(
            "Failed to read bound source '{}': {}",
            file.display(),
            err
        ))
    })?;
    Ok(ProjectSourceDocument {
        thread_id: binding.thread_id,
        slug,
        folder: folder.to_string_lossy().to_string(),
        file: file.to_string_lossy().to_string(),
        source,
    })
}

/// "Open in editor": mirror the active macro to its project folder (unless
/// the folder carries unapplied external edits) and open `model.ecky` with
/// the system editor. The folder watcher picks edits up as new versions.
#[tauri::command]
#[specta::specta]
pub async fn open_project_in_editor(
    thread_id: Option<String>,
    _message_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectEditorLink> {
    let thread_id = thread_id.ok_or_else(|| {
        crate::contracts::AppError::validation("Source file requires a design thread.")
    })?;
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let document = {
        let conn = state.db.lock().await;
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &thread_id)?
    };
    let link = ProjectEditorLink {
        slug: document.slug,
        folder: document.folder,
        file: document.file,
    };
    let file = Path::new(&link.file);
    open_path_in_system_editor(file).map_err(|err| {
        crate::contracts::AppError::internal(format!(
            "Failed to open '{}' in the system editor: {}",
            file.display(),
            err
        ))
    })?;

    Ok(link)
}

/// Reveal the exact persisted source folder without also opening model.ecky.
#[tauri::command]
#[specta::specta]
pub async fn reveal_project_folder(
    thread_id: Option<String>,
    _message_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectEditorLink> {
    let thread_id = thread_id.ok_or_else(|| {
        crate::contracts::AppError::validation("Source folder requires a design thread.")
    })?;
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let document = {
        let conn = state.db.lock().await;
        ensure_project_source_document(&app, &conn, projects_root.as_deref(), &thread_id)?
    };
    let link = ProjectEditorLink {
        slug: document.slug,
        folder: document.folder,
        file: document.file,
    };
    let folder = Path::new(&link.folder);
    reveal_path_in_file_manager(folder).map_err(|err| {
        crate::contracts::AppError::internal(format!(
            "Failed to reveal '{}' in the file manager: {}",
            folder.display(),
            err
        ))
    })?;
    Ok(link)
}

/// Open the copied FCStd/STEP source associated with one imported message.
#[tauri::command]
#[specta::specta]
pub async fn open_imported_cad_source(
    thread_id: Option<String>,
    message_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectEditorLink> {
    let thread_id = thread_id.ok_or_else(|| {
        crate::contracts::AppError::validation("CAD source requires a design thread.")
    })?;
    let message_id = message_id.ok_or_else(|| {
        crate::contracts::AppError::validation("CAD source requires an imported message.")
    })?;
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let (document, cad_file) = {
        let conn = state.db.lock().await;
        let document =
            ensure_project_source_document(&app, &conn, projects_root.as_deref(), &thread_id)?;
        let cad_file = imported_cad_file_for_message(&conn, &document, Some(&message_id))?
            .ok_or_else(|| {
                crate::contracts::AppError::not_found(format!(
                    "Imported CAD source for message '{}' was not found.",
                    message_id
                ))
            })?;
        (document, cad_file)
    };
    open_path_in_default_app(&cad_file).map_err(|err| {
        crate::contracts::AppError::internal(format!(
            "Failed to open imported CAD source '{}': {}",
            cad_file.display(),
            err
        ))
    })?;
    Ok(ProjectEditorLink {
        slug: document.slug,
        folder: document.folder,
        file: cad_file.to_string_lossy().to_string(),
    })
}
