use std::path::Path;

use tauri::AppHandle;

use crate::component_package_runtime;
use crate::models::{
    AppResult, ComponentPackage, ComponentPackageHeader, InstalledComponentPackage, PathResolver,
};

#[tauri::command]
#[specta::specta]
pub async fn read_component_package_manifest(project_dir: String) -> AppResult<ComponentPackage> {
    component_package_runtime::read_component_package_manifest(Path::new(&project_dir))
}

#[tauri::command]
#[specta::specta]
pub async fn write_component_package_manifest(
    project_dir: String,
    component_package: ComponentPackage,
) -> AppResult<String> {
    let path = component_package_runtime::write_component_package_manifest(
        Path::new(&project_dir),
        &component_package,
    )?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn write_component_package_archive(
    project_dir: String,
    archive_path: String,
) -> AppResult<()> {
    component_package_runtime::write_component_package_archive(
        Path::new(&project_dir),
        Path::new(&archive_path),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn read_component_package_from_archive(
    archive_path: String,
) -> AppResult<ComponentPackage> {
    component_package_runtime::read_component_package_from_archive(Path::new(&archive_path))
}

#[tauri::command]
#[specta::specta]
pub async fn read_component_package_header_from_archive(
    archive_path: String,
) -> AppResult<ComponentPackageHeader> {
    component_package_runtime::read_component_package_header_from_archive(Path::new(&archive_path))
}

#[tauri::command]
#[specta::specta]
pub async fn extract_component_package_archive(
    archive_path: String,
    target_dir: String,
) -> AppResult<ComponentPackage> {
    component_package_runtime::extract_component_package_archive(
        Path::new(&archive_path),
        Path::new(&target_dir),
    )
}

pub async fn install_component_package_archive_for_app(
    app: &dyn PathResolver,
    archive_path: String,
) -> AppResult<InstalledComponentPackage> {
    component_package_runtime::install_component_package_archive(app, Path::new(&archive_path))
}

#[tauri::command]
#[specta::specta]
pub async fn install_component_package_archive(
    archive_path: String,
    app: AppHandle,
) -> AppResult<InstalledComponentPackage> {
    install_component_package_archive_for_app(&app, archive_path).await
}

pub async fn list_installed_component_package_headers_for_app(
    app: &dyn PathResolver,
) -> AppResult<Vec<ComponentPackageHeader>> {
    component_package_runtime::list_installed_component_package_headers(app)
}

#[tauri::command]
#[specta::specta]
pub async fn list_installed_component_package_headers(
    app: AppHandle,
) -> AppResult<Vec<ComponentPackageHeader>> {
    list_installed_component_package_headers_for_app(&app).await
}
