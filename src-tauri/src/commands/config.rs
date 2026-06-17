use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager, State};

use crate::models::{AppResult, AppState, Config};

fn open_path_in_system_editor(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).status()?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .status()?;
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(path).status()?;
        Ok(())
    }
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
    mut config: Config,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    crate::mcp::runtime::ensure_primary_agent_id(&mut config);
    let config_dir = app.path().app_config_dir().unwrap();
    let config_path = config_dir.join("config.json");

    let data = serde_json::to_string_pretty(&config)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;
    fs::write(config_path, data)
        .map_err(|err| crate::models::AppError::persistence(err.to_string()))?;

    {
        let mut state_config = state.config.lock().unwrap();
        *state_config = config;
    }
    let state_for_sync = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::mcp::runtime::sync_auto_agent_supervisors(state_for_sync);
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn list_agent_models(cmd: String) -> AppResult<crate::contracts::AgentModelList> {
    crate::llm::list_agent_models(&cmd)
        .await
        .map_err(crate::models::AppError::provider)
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
        .map_err(crate::models::AppError::provider)
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
            crate::models::AppError::validation(
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
        return Err(crate::models::AppError::validation(
            "Export path is required for Ecky MCP skill zip.",
        ));
    }
    if !is_ecky_mcp_skill_dir(skill_dir) {
        return Err(crate::models::AppError::validation(format!(
            "Ecky MCP skill directory is invalid: {}",
            skill_dir.display()
        )));
    }
    if let Some(parent) = target_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| {
            crate::models::AppError::persistence(format!(
                "Failed to create export directory '{}': {}",
                parent.display(),
                err
            ))
        })?;
    }

    let file = fs::File::create(target_path).map_err(|err| {
        crate::models::AppError::persistence(format!(
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
            crate::models::AppError::persistence(format!(
                "Failed to resolve skill archive path '{}': {}",
                path.display(),
                err
            ))
        })?;
        let archive_name = Path::new(skill_root_name).join(rel);
        let archive_name = archive_name.to_string_lossy().replace('\\', "/");
        zip.start_file(&archive_name, options).map_err(|err| {
            crate::models::AppError::persistence(format!(
                "Failed to write skill archive entry '{}': {}",
                archive_name, err
            ))
        })?;
        let mut source = fs::File::open(&path).map_err(|err| {
            crate::models::AppError::persistence(format!(
                "Failed to open skill file '{}': {}",
                path.display(),
                err
            ))
        })?;
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).map_err(|err| {
            crate::models::AppError::persistence(format!(
                "Failed to read skill file '{}': {}",
                path.display(),
                err
            ))
        })?;
        zip.write_all(&bytes).map_err(|err| {
            crate::models::AppError::persistence(format!(
                "Failed to write skill file '{}': {}",
                archive_name, err
            ))
        })?;
    }

    zip.finish().map_err(|err| {
        crate::models::AppError::persistence(format!(
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
        crate::models::AppError::persistence(format!(
            "Failed to read skill directory '{}': {}",
            dir.display(),
            err
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            crate::models::AppError::persistence(format!(
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
            crate::models::AppError::persistence(format!(
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
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::ZipArchive;

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("ecky-skill-export-{name}-{unique}"))
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
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEditorLink {
    pub slug: String,
    pub folder: String,
    pub file: String,
}

/// "Open in editor": mirror the active macro to its project folder (unless
/// the folder carries unapplied external edits) and open `model.ecky` with
/// the system editor. The folder watcher picks edits up as new versions.
#[tauri::command]
#[specta::specta]
pub async fn open_project_in_editor(
    thread_id: Option<String>,
    message_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectEditorLink> {
    use crate::project_mirror::{self, ProjectSyncState};

    let target = {
        let conn = state.db.lock().await;
        crate::services::target::resolve_editable_target(&conn, &app, thread_id, message_id)?
    };
    let slug = project_mirror::project_slug(&target.design_output.title, &target.thread_id);
    let projects_root = state.config.lock().unwrap().projects_root.clone();
    let dir = project_mirror::project_dir(&app, projects_root.as_deref(), &slug)?;

    let manifest = project_mirror::read_manifest(&dir)?;
    let file_digest = project_mirror::read_project_source(&dir)?
        .map(|source| project_mirror::source_digest(&source));
    let has_unapplied_edits = matches!(
        project_mirror::classify_sync_state(file_digest.as_deref(), manifest.as_ref(), None),
        ProjectSyncState::FileChanged
    );
    if !has_unapplied_edits {
        let model_id = target
            .artifact_bundle
            .as_ref()
            .map(|bundle| bundle.model_id.clone());
        project_mirror::export_project(
            &app,
            &project_mirror::ExportProjectRequest {
                slug: &slug,
                thread_id: &target.thread_id,
                message_id: &target.message_id,
                model_id: model_id.as_deref(),
                source: &target.design_output.macro_code,
                projects_root: projects_root.as_deref(),
            },
        )?;
    }

    let file = dir.join(project_mirror::PROJECT_SOURCE_FILE_NAME);
    open_path_in_system_editor(&file).map_err(|err| {
        crate::models::AppError::internal(format!(
            "Failed to open '{}' in the system editor: {}",
            file.display(),
            err
        ))
    })?;

    Ok(ProjectEditorLink {
        slug,
        folder: dir.to_string_lossy().to_string(),
        file: file.to_string_lossy().to_string(),
    })
}
