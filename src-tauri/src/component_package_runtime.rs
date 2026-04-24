use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use base64::{engine::general_purpose, Engine as _};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::models::{
    component_package_header, validate_component_package, validate_component_package_header,
    AppError, AppResult, ComponentPackage, ComponentPackageHeader, InstalledComponentPackage,
    PathResolver,
};

pub const COMPONENT_PACKAGE_FILE_NAME: &str = "ecky-package.json";
pub const COMPONENT_PACKAGE_HEADER_FILE_NAME: &str = "ecky-header.json";
pub const COMPONENT_PACKAGE_PAYLOAD_FILE_NAME: &str = "ecky-payload.b64";
const COMPONENT_LIBRARY_DIR_NAME: &str = "component-library";

pub fn read_component_package_manifest(project_dir: &Path) -> AppResult<ComponentPackage> {
    let path = project_dir.join(COMPONENT_PACKAGE_FILE_NAME);
    let raw = fs::read_to_string(&path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read component package manifest '{}': {}",
            path.display(),
            err
        ))
    })?;
    let package: ComponentPackage = serde_json::from_str(&raw).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse component package manifest '{}': {}",
            path.display(),
            err
        ))
    })?;
    validate_component_package(&package)?;
    Ok(package)
}

pub fn write_component_package_manifest(
    project_dir: &Path,
    package: &ComponentPackage,
) -> AppResult<PathBuf> {
    validate_component_package(package)?;
    fs::create_dir_all(project_dir).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create component package directory '{}': {}",
            project_dir.display(),
            err
        ))
    })?;
    let path = project_dir.join(COMPONENT_PACKAGE_FILE_NAME);
    let data = serde_json::to_string_pretty(package)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(&path, data).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write component package manifest '{}': {}",
            path.display(),
            err
        ))
    })?;
    Ok(path)
}

pub fn write_component_package_archive(project_dir: &Path, archive_path: &Path) -> AppResult<()> {
    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::persistence(format!(
                "Failed to create component package archive directory '{}': {}",
                parent.display(),
                err
            ))
        })?;
    }

    let archive_file = fs::File::create(archive_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create component package archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    let mut writer = ZipWriter::new(archive_file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let package = read_component_package_manifest(project_dir)?;
    let header = component_package_header(&package)?;
    writer
        .start_file(COMPONENT_PACKAGE_HEADER_FILE_NAME, options)
        .map_err(|err| {
            AppError::persistence(format!(
                "Failed to add component package header to archive '{}': {}",
                archive_path.display(),
                err
            ))
        })?;
    let header_data =
        serde_json::to_vec_pretty(&header).map_err(|err| AppError::persistence(err.to_string()))?;
    writer.write_all(&header_data).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write component package header into archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;

    let payload = build_component_package_payload(project_dir, archive_path)?;
    let encoded_payload = general_purpose::STANDARD.encode(payload);
    writer
        .start_file(COMPONENT_PACKAGE_PAYLOAD_FILE_NAME, options)
        .map_err(|err| {
            AppError::persistence(format!(
                "Failed to add component package payload to archive '{}': {}",
                archive_path.display(),
                err
            ))
        })?;
    writer
        .write_all(encoded_payload.as_bytes())
        .map_err(|err| {
            AppError::persistence(format!(
                "Failed to write component package payload into archive '{}': {}",
                archive_path.display(),
                err
            ))
        })?;

    writer.finish().map_err(|err| {
        AppError::persistence(format!(
            "Failed to finalize component package archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    Ok(())
}

pub fn read_component_package_from_archive(archive_path: &Path) -> AppResult<ComponentPackage> {
    let archive_file = fs::File::open(archive_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to open component package archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    let mut archive = ZipArchive::new(archive_file).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse component package archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    let package = if let Some(payload) = read_payload_archive_bytes(&mut archive, archive_path)? {
        read_component_package_from_payload(&payload, archive_path)?
    } else {
        read_component_package_manifest_entry(&mut archive, archive_path)?
    };
    validate_component_package(&package)?;
    Ok(package)
}

pub fn read_component_package_header_from_archive(
    archive_path: &Path,
) -> AppResult<ComponentPackageHeader> {
    let archive_file = fs::File::open(archive_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to open component package archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    let mut archive = ZipArchive::new(archive_file).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse component package archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    let header_result = {
        match archive.by_name(COMPONENT_PACKAGE_HEADER_FILE_NAME) {
            Ok(mut header_file) => {
                let mut raw = String::new();
                header_file.read_to_string(&mut raw).map_err(|err| {
                    AppError::parse(format!(
                        "Failed to read component package header from archive '{}': {}",
                        archive_path.display(),
                        err
                    ))
                })?;
                let header: ComponentPackageHeader = serde_json::from_str(&raw).map_err(|err| {
                    AppError::parse(format!(
                        "Failed to parse component package header from archive '{}': {}",
                        archive_path.display(),
                        err
                    ))
                })?;
                validate_component_package_header(&header)?;
                Some(header)
            }
            Err(_) => None,
        }
    };
    if let Some(header) = header_result {
        Ok(header)
    } else {
        drop(archive);
        let package = read_component_package_from_archive(archive_path)?;
        component_package_header(&package)
    }
}

pub fn extract_component_package_archive(
    archive_path: &Path,
    target_dir: &Path,
) -> AppResult<ComponentPackage> {
    let archive_file = fs::File::open(archive_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to open component package archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    let mut archive = ZipArchive::new(archive_file).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse component package archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    fs::create_dir_all(target_dir).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create component package extraction directory '{}': {}",
            target_dir.display(),
            err
        ))
    })?;

    let archive_label = archive_path.display().to_string();
    if let Some(payload) = read_payload_archive_bytes(&mut archive, archive_path)? {
        extract_archive_entries(&mut archive, &archive_label, target_dir, true)?;
        let mut payload_archive = ZipArchive::new(Cursor::new(payload)).map_err(|err| {
            AppError::parse(format!(
                "Failed to parse component package payload from archive '{}': {}",
                archive_path.display(),
                err
            ))
        })?;
        extract_archive_entries(
            &mut payload_archive,
            &format!("payload in {}", archive_label),
            target_dir,
            false,
        )?;
    } else {
        extract_archive_entries(&mut archive, &archive_label, target_dir, false)?;
    }

    read_component_package_manifest(target_dir)
}

pub fn install_component_package_archive(
    app: &dyn PathResolver,
    archive_path: &Path,
) -> AppResult<InstalledComponentPackage> {
    let header = read_component_package_header_from_archive(archive_path)?;
    let package_dir = component_package_install_dir(app, &header.package_id, &header.version)?;
    extract_component_package_archive(archive_path, &package_dir)?;
    Ok(InstalledComponentPackage {
        header,
        package_dir: package_dir.to_string_lossy().to_string(),
    })
}

pub fn list_installed_component_package_headers(
    app: &dyn PathResolver,
) -> AppResult<Vec<ComponentPackageHeader>> {
    let root = component_library_root(app)?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut headers = Vec::new();
    for package_entry in fs::read_dir(&root).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read component library directory '{}': {}",
            root.display(),
            err
        ))
    })? {
        let package_entry = package_entry.map_err(|err| {
            AppError::persistence(format!(
                "Failed to read component library entry '{}': {}",
                root.display(),
                err
            ))
        })?;
        let package_path = package_entry.path();
        if !package_path.is_dir() {
            continue;
        }
        for version_entry in fs::read_dir(&package_path).map_err(|err| {
            AppError::persistence(format!(
                "Failed to read component package directory '{}': {}",
                package_path.display(),
                err
            ))
        })? {
            let version_entry = version_entry.map_err(|err| {
                AppError::persistence(format!(
                    "Failed to read component package version entry '{}': {}",
                    package_path.display(),
                    err
                ))
            })?;
            let version_path = version_entry.path();
            if !version_path.is_dir() {
                continue;
            }
            let header_path = version_path.join(COMPONENT_PACKAGE_HEADER_FILE_NAME);
            if !header_path.exists() {
                continue;
            }
            let raw = fs::read_to_string(&header_path).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to read installed component package header '{}': {}",
                    header_path.display(),
                    err
                ))
            })?;
            let header: ComponentPackageHeader = serde_json::from_str(&raw).map_err(|err| {
                AppError::parse(format!(
                    "Failed to parse installed component package header '{}': {}",
                    header_path.display(),
                    err
                ))
            })?;
            validate_component_package_header(&header)?;
            headers.push(header);
        }
    }
    headers.sort_by(|a, b| {
        a.package_id
            .cmp(&b.package_id)
            .then_with(|| a.version.cmp(&b.version))
    });
    Ok(headers)
}

fn component_library_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    let root = app.app_data_dir().join(COMPONENT_LIBRARY_DIR_NAME);
    fs::create_dir_all(&root).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create component library directory '{}': {}",
            root.display(),
            err
        ))
    })?;
    Ok(root)
}

fn component_package_install_dir(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
) -> AppResult<PathBuf> {
    Ok(component_library_root(app)?
        .join(safe_library_segment(package_id, "packageId")?)
        .join(safe_library_segment(version, "version")?))
}

fn collect_package_files(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_package_files_inner(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_package_files_inner(path: &Path, files: &mut Vec<PathBuf>) -> AppResult<()> {
    for entry in fs::read_dir(path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read component package directory '{}': {}",
            path.display(),
            err
        ))
    })? {
        let entry = entry.map_err(|err| {
            AppError::persistence(format!(
                "Failed to read component package directory entry '{}': {}",
                path.display(),
                err
            ))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            AppError::persistence(format!(
                "Failed to inspect component package path '{}': {}",
                path.display(),
                err
            ))
        })?;
        if file_type.is_dir() {
            collect_package_files_inner(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn build_component_package_payload(project_dir: &Path, archive_path: &Path) -> AppResult<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    for path in collect_package_files(project_dir)? {
        let file_name = path.file_name().and_then(|name| name.to_str());
        if matches!(
            file_name,
            Some(COMPONENT_PACKAGE_HEADER_FILE_NAME | COMPONENT_PACKAGE_PAYLOAD_FILE_NAME)
        ) {
            continue;
        }
        let entry_name = archive_entry_name(project_dir, &path)?;
        writer.start_file(entry_name, options).map_err(|err| {
            AppError::persistence(format!(
                "Failed to add file '{}' to component package payload for '{}': {}",
                path.display(),
                archive_path.display(),
                err
            ))
        })?;
        let data = fs::read(&path).map_err(|err| {
            AppError::persistence(format!(
                "Failed to read component package file '{}': {}",
                path.display(),
                err
            ))
        })?;
        writer.write_all(&data).map_err(|err| {
            AppError::persistence(format!(
                "Failed to write file '{}' into component package payload for '{}': {}",
                path.display(),
                archive_path.display(),
                err
            ))
        })?;
    }

    let cursor = writer.finish().map_err(|err| {
        AppError::persistence(format!(
            "Failed to finalize component package payload for '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    Ok(cursor.into_inner())
}

fn read_payload_archive_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    archive_path: &Path,
) -> AppResult<Option<Vec<u8>>> {
    let mut payload = match archive.by_name(COMPONENT_PACKAGE_PAYLOAD_FILE_NAME) {
        Ok(payload) => payload,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => {
            return Err(AppError::parse(format!(
                "Failed to read component package payload from archive '{}': {}",
                archive_path.display(),
                err
            )));
        }
    };
    let mut encoded = String::new();
    payload.read_to_string(&mut encoded).map_err(|err| {
        AppError::parse(format!(
            "Failed to read component package payload from archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    let decoded = general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|err| {
            AppError::parse(format!(
                "Failed to decode component package payload from archive '{}': {}",
                archive_path.display(),
                err
            ))
        })?;
    Ok(Some(decoded))
}

fn read_component_package_from_payload(
    payload: &[u8],
    archive_path: &Path,
) -> AppResult<ComponentPackage> {
    let mut payload_archive = ZipArchive::new(Cursor::new(payload)).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse component package payload from archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    read_component_package_manifest_entry(&mut payload_archive, archive_path)
}

fn read_component_package_manifest_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    archive_path: &Path,
) -> AppResult<ComponentPackage> {
    let mut manifest = archive
        .by_name(COMPONENT_PACKAGE_FILE_NAME)
        .map_err(|err| {
            AppError::validation(format!(
                "Component package archive '{}' is missing '{}': {}",
                archive_path.display(),
                COMPONENT_PACKAGE_FILE_NAME,
                err
            ))
        })?;
    let mut raw = String::new();
    manifest.read_to_string(&mut raw).map_err(|err| {
        AppError::parse(format!(
            "Failed to read component package manifest from archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    let package: ComponentPackage = serde_json::from_str(&raw).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse component package manifest from archive '{}': {}",
            archive_path.display(),
            err
        ))
    })?;
    Ok(package)
}

fn extract_archive_entries<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    archive_label: &str,
    target_dir: &Path,
    skip_payload: bool,
) -> AppResult<()> {
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|err| {
            AppError::parse(format!(
                "Failed to read component package archive entry {} from '{}': {}",
                index, archive_label, err
            ))
        })?;
        let entry_name = entry.name().to_string();
        if skip_payload && entry_name == COMPONENT_PACKAGE_PAYLOAD_FILE_NAME {
            continue;
        }
        let relative_path = safe_archive_path(&entry_name)?;
        let output_path = target_dir.join(relative_path);

        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to create component package directory '{}': {}",
                    output_path.display(),
                    err
                ))
            })?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to create component package directory '{}': {}",
                    parent.display(),
                    err
                ))
            })?;
        }
        let mut output = fs::File::create(&output_path).map_err(|err| {
            AppError::persistence(format!(
                "Failed to create component package file '{}': {}",
                output_path.display(),
                err
            ))
        })?;
        std::io::copy(&mut entry, &mut output).map_err(|err| {
            AppError::persistence(format!(
                "Failed to extract component package file '{}': {}",
                output_path.display(),
                err
            ))
        })?;
    }
    Ok(())
}

fn safe_library_segment(value: &str, label: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('.')
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(AppError::validation(format!(
            "Component package {} '{}' is not safe for local library paths.",
            label, value
        )));
    }
    Ok(trimmed.to_string())
}

fn safe_archive_path(entry_name: &str) -> AppResult<PathBuf> {
    let path = Path::new(entry_name);
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => output.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::validation(format!(
                    "Component package archive entry '{}' is not safe to extract.",
                    entry_name
                )));
            }
        }
    }
    if output.as_os_str().is_empty() {
        return Err(AppError::validation(format!(
            "Component package archive entry '{}' is not safe to extract.",
            entry_name
        )));
    }
    Ok(output)
}

fn archive_entry_name(root: &Path, path: &Path) -> AppResult<String> {
    let relative = path.strip_prefix(root).map_err(|err| {
        AppError::internal(format!(
            "Failed to derive package archive entry for '{}': {}",
            path.display(),
            err
        ))
    })?;
    let entry_name = relative.to_string_lossy().replace('\\', "/");
    if entry_name.is_empty() || entry_name.starts_with("../") || entry_name.contains("/../") {
        return Err(AppError::validation(format!(
            "Component package path '{}' is not safe for archive output.",
            path.display()
        )));
    }
    Ok(entry_name)
}
