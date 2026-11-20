use crate::contracts::{AppError, AppResult};
use crate::models::{AppState, PathResolver};
use crate::puppet::{self, PuppetDocument, PuppetPart};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

fn store(app: &AppHandle) -> PathBuf {
    app.app_data_dir().join("puppets")
}
fn find_obj(root: &Path, depth: u8) -> Option<PathBuf> {
    if depth > 4 {
        return None;
    }
    let mut entries = std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            if let Some(found) = find_obj(&path, depth + 1) {
                return Some(found);
            }
        } else if path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case("obj"))
        {
            return Some(path);
        }
    }
    None
}

#[tauri::command]
#[specta::specta]
pub async fn puppet_import_part(
    source_path: Option<String>,
    capture_run_id: Option<String>,
    metres: bool,
    z_up: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<PuppetPart> {
    let (path, metres, z_up) = match (source_path, capture_run_id) {
        (Some(path), None) => (PathBuf::from(path), metres, z_up),
        (None, Some(id)) => {
            let db = state.db.lock().await;
            let run = crate::capture_runs::get(&db, &id)
                .map_err(|e| AppError::persistence(e.to_string()))?
                .ok_or_else(|| AppError::not_found("Capture run not found."))?;
            let preview = run.mesh_preview.ok_or_else(|| {
                AppError::validation("Finish reconstruction before adding this scan.")
            })?;
            let folder = Path::new(&preview.stl_path)
                .parent()
                .ok_or_else(|| AppError::validation("Capture output folder is missing."))?
                .join("object");
            let path=find_obj(&folder,0).ok_or_else(||AppError::not_found("Original capture OBJ is missing. Import the saved STL, or reconstruct this scan again to retain its textures."))?;
            (path, true, false)
        }
        _ => {
            return Err(AppError::validation(
                "Choose either a part file or one capture run.",
            ))
        }
    };
    let root = store(&app);
    tokio::task::spawn_blocking(move || {
        puppet::import_part(
            &root,
            &path,
            format!("part_{}", uuid::Uuid::new_v4().simple()),
            metres,
            z_up,
        )
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
    .map_err(AppError::validation)
}

#[tauri::command]
#[specta::specta]
pub async fn puppet_preview(document: PuppetDocument, app: AppHandle) -> AppResult<String> {
    let root = store(&app);
    tokio::task::spawn_blocking(move || {
        puppet::preview(&root, &document).map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
    .map_err(AppError::validation)
}

#[tauri::command]
#[specta::specta]
pub async fn puppet_export(
    document: PuppetDocument,
    path: String,
    app: AppHandle,
) -> AppResult<String> {
    let root = store(&app);
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        if !path.to_ascii_lowercase().ends_with(".glb") {
            return Err("Choose a .glb output file.".into());
        }
        let bytes = puppet::export_glb(&root, &document)?;
        puppet::atomic_write(Path::new(&path), &bytes)?;
        Ok(path)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
    .map_err(AppError::validation)
}

#[tauri::command]
#[specta::specta]
pub async fn puppet_save(
    document: PuppetDocument,
    path: String,
    app: AppHandle,
) -> AppResult<String> {
    let root = store(&app);
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        if !path.to_ascii_lowercase().ends_with(".eckypuppet") {
            return Err("Choose an .eckypuppet output file.".into());
        }
        puppet::save(&root, &document, Path::new(&path))?;
        Ok(path)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
    .map_err(AppError::validation)
}

#[tauri::command]
#[specta::specta]
pub async fn puppet_open(path: String, app: AppHandle) -> AppResult<PuppetDocument> {
    let root = store(&app);
    tokio::task::spawn_blocking(move || puppet::open(&root, Path::new(&path)))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .map_err(AppError::validation)
}
