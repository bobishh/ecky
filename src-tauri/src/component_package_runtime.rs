use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use base64::{engine::general_purpose, Engine as _};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::models::{
    component_package_header, validate_component_package, validate_component_package_header,
    validate_design_params, validate_ui_spec, AppError, AppResult, ComponentDefinition,
    ComponentPackage, ComponentPackageHeader, ComponentParam, ComponentParamKind, DesignParams,
    InstalledAssemblyComponentSource, InstalledAssemblySource, InstalledComponentPackage,
    InstalledComponentSource, ParamValue, ParsedParamsResult, PathResolver, UiField, UiSpec,
};

pub const COMPONENT_PACKAGE_FILE_NAME: &str = "ecky-package.json";
pub const COMPONENT_PACKAGE_HEADER_FILE_NAME: &str = "ecky-header.json";
pub const COMPONENT_PACKAGE_PAYLOAD_FILE_NAME: &str = "ecky-payload.b64";
const COMPONENT_LIBRARY_DIR_NAME: &str = "component-library";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DerivedComponentSourceContract {
    pub params: Vec<ComponentParam>,
    pub ui_spec: UiSpec,
    pub initial_params: DesignParams,
}

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
    validate_component_source_refs(project_dir, &package)?;
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

pub fn resolve_installed_component_source(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
    component_id: &str,
) -> AppResult<InstalledComponentSource> {
    let (package_dir, package) = load_installed_package(app, package_id, version)?;
    resolve_component_source_from_package(package_id, version, &package_dir, &package, component_id)
}

pub fn resolve_installed_component_assembly(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
    assembly_id: &str,
) -> AppResult<InstalledAssemblySource> {
    let (package_dir, package) = load_installed_package(app, package_id, version)?;
    let assembly = package
        .assemblies
        .iter()
        .find(|assembly| assembly.assembly_id == assembly_id)
        .cloned()
        .ok_or_else(|| {
            AppError::not_found(format!(
                "Installed component package '{}@{}' does not contain assemblyId '{}'.",
                package_id, version, assembly_id
            ))
        })?;
    let components = assembly
        .components
        .iter()
        .map(|component_ref| {
            Ok(InstalledAssemblyComponentSource {
                instance_id: component_ref.instance_id.clone(),
                component_id: component_ref.component_id.clone(),
                placement_frame: None,
                installed_source: resolve_component_source_from_package(
                    package_id,
                    version,
                    &package_dir,
                    &package,
                    &component_ref.component_id,
                )?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(InstalledAssemblySource {
        package_id: package.package_id.clone(),
        version: package.version.clone(),
        package_display_name: package.display_name.clone(),
        package_dir: package_dir.to_string_lossy().to_string(),
        assembly,
        port_types: package.port_types.clone(),
        mate_types: package.mate_types.clone(),
        components,
        mate_results: Vec::new(),
    })
}

fn load_installed_package(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
) -> AppResult<(PathBuf, ComponentPackage)> {
    let package_dir = component_package_install_dir(app, package_id, version)?;
    let package = read_component_package_manifest(&package_dir)?;
    Ok((package_dir, package))
}

fn resolve_component_source_from_package(
    package_id: &str,
    version: &str,
    package_dir: &Path,
    package: &ComponentPackage,
    component_id: &str,
) -> AppResult<InstalledComponentSource> {
    let mut component = package
        .components
        .iter()
        .find(|component| component.component_id == component_id)
        .cloned()
        .ok_or_else(|| {
            AppError::not_found(format!(
                "Installed component package '{}@{}' does not contain componentId '{}'.",
                package_id, version, component_id
            ))
        })?;
    let source_ref = component.source_ref.as_deref().ok_or_else(|| {
        AppError::validation(format!(
            "Installed component '{}@{}:{}' is missing sourceRef.",
            package_id, version, component_id
        ))
    })?;
    let relative_source_path = safe_archive_path(source_ref).map_err(|_| {
        AppError::validation(format!(
            "Installed component '{}@{}:{}' sourceRef '{}' must be a safe package-local relative path.",
            package_id, version, component_id, source_ref
        ))
    })?;
    let source_path = package_dir.join(relative_source_path);
    if !source_path.is_file() {
        return Err(AppError::not_found(format!(
            "Installed component '{}@{}:{}' source file '{}' was not found in '{}'.",
            package_id,
            version,
            component_id,
            source_ref,
            package_dir.display()
        )));
    }
    maybe_backfill_component_contract_from_source(&mut component, &source_path)?;

    Ok(InstalledComponentSource {
        package_id: package.package_id.clone(),
        version: package.version.clone(),
        package_display_name: package.display_name.clone(),
        package_dir: package_dir.to_string_lossy().to_string(),
        component,
        port_types: package.port_types.clone(),
        mate_types: package.mate_types.clone(),
        source_path: source_path.to_string_lossy().to_string(),
    })
}

pub(crate) fn derive_component_source_contract_from_path(
    source_path: &Path,
) -> AppResult<DerivedComponentSourceContract> {
    let source = fs::read_to_string(source_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read reusable component source '{}' for param derivation: {}",
            source_path.display(),
            err
        ))
    })?;
    let parsed = crate::commands::design::parse_macro_params(source);
    let derived = DerivedComponentSourceContract {
        params: component_params_from_parsed_params(&parsed),
        ui_spec: UiSpec {
            fields: parsed.fields,
        },
        initial_params: parsed.params,
    };
    validate_ui_spec(&derived.ui_spec)?;
    validate_design_params(&derived.initial_params, &derived.ui_spec)?;
    Ok(derived)
}

fn maybe_backfill_component_contract_from_source(
    component: &mut ComponentDefinition,
    source_path: &Path,
) -> AppResult<()> {
    if !source_path_supports_param_derivation(source_path) {
        return Ok(());
    }
    let derived = derive_component_source_contract_from_path(source_path)?;
    if component.params.is_empty() {
        component.params = derived.params.clone();
    }
    if component.ui_spec.fields.is_empty() {
        component.ui_spec = derived.ui_spec.clone();
    }
    if component.initial_params.is_empty() {
        component.initial_params = derived.initial_params;
    }
    Ok(())
}

fn source_path_supports_param_derivation(source_path: &Path) -> bool {
    source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "ecky" | "py" | "fcmacro"))
        .unwrap_or(false)
}

pub(crate) fn component_params_from_parsed_params(
    parsed: &ParsedParamsResult,
) -> Vec<ComponentParam> {
    let mut params = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for field in &parsed.fields {
        if seen.insert(field.key().to_string()) {
            params.push(component_param_from_field(field));
        }
    }

    for (key, value) in &parsed.params {
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(param) = component_param_from_value(key, value) {
            params.push(param);
        }
    }

    params
}

pub(crate) fn component_params_from_ui_contract(
    ui_spec: &UiSpec,
    initial_params: &DesignParams,
) -> Vec<ComponentParam> {
    component_params_from_parsed_params(&ParsedParamsResult {
        fields: ui_spec.fields.clone(),
        params: initial_params.clone(),
    })
}

fn component_param_from_field(field: &UiField) -> ComponentParam {
    ComponentParam {
        key: field.key().to_string(),
        label: component_param_label(field.key(), field.label()),
        kind: match field {
            UiField::Range { .. } | UiField::Number { .. } => ComponentParamKind::Number,
            UiField::Select { .. } => ComponentParamKind::Choice,
            UiField::Checkbox { .. } => ComponentParamKind::Boolean,
            UiField::Image { .. } => ComponentParamKind::Text,
        },
        unit: None,
    }
}

fn component_param_from_value(key: &str, value: &ParamValue) -> Option<ComponentParam> {
    let kind = match value {
        ParamValue::Number(_) => ComponentParamKind::Number,
        ParamValue::String(_) => ComponentParamKind::Text,
        ParamValue::Boolean(_) => ComponentParamKind::Boolean,
        ParamValue::Null => return None,
    };
    Some(ComponentParam {
        key: key.to_string(),
        label: component_param_label(key, ""),
        kind,
        unit: None,
    })
}

pub(crate) fn component_param_label(key: &str, label: &str) -> String {
    let trimmed = label.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    key.split(['_', '-', '.'])
        .filter(|token| !token.is_empty())
        .map(|token| {
            let mut chars = token.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

fn validate_component_source_refs(project_dir: &Path, package: &ComponentPackage) -> AppResult<()> {
    for component in &package.components {
        let Some(source_ref) = component.source_ref.as_deref() else {
            continue;
        };
        let relative_path = safe_archive_path(source_ref).map_err(|_| {
            AppError::validation(format!(
                "Component package component '{}' sourceRef '{}' must be a safe package-local relative path.",
                component.component_id, source_ref
            ))
        })?;
        let source_path = project_dir.join(relative_path);
        if !source_path.is_file() {
            return Err(AppError::validation(format!(
                "Component package component '{}' sourceRef '{}' was not found under project dir '{}'.",
                component.component_id,
                source_ref,
                project_dir.display()
            )));
        }
    }
    Ok(())
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

pub(crate) fn safe_archive_path(entry_name: &str) -> AppResult<PathBuf> {
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
