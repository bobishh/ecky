use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Seek, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use base64::{Engine as _, engine::general_purpose};
use sha2::{Digest, Sha256};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::contracts::{
    AppError, AppResult, ComponentCoordinateIndexEntry, ComponentDefinition, ComponentPackage,
    ComponentPackageHeader, ComponentParam, ComponentParamKind, DesignParams,
    InstalledAssemblyComponentSource, InstalledAssemblySource, InstalledComponentPackage,
    InstalledComponentSource, PACKAGE_PAYLOAD_INVENTORY_SCHEMA_VERSION, PackagePayloadInventory,
    PackagePayloadInventoryEntry, ParamValue, ParsedParamsResult, UiField, UiSpec,
    component_package_header, validate_component_package, validate_component_package_header,
    validate_design_params, validate_ui_spec,
};
use crate::models::PathResolver;

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
    let installed = install_component_package_to_store(app, archive_path)?;
    Ok(InstalledComponentPackage {
        header,
        package_dir: installed.store_dir.to_string_lossy().to_string(),
    })
}

/// Result of installing a package payload into the global content-addressed
/// store plus the mutable coordinate index.
#[derive(Clone, Debug)]
pub struct InstalledStorePackage {
    pub package_id: String,
    pub version: String,
    pub package_digest: String,
    pub store_dir: PathBuf,
}

/// Decode the inner payload bytes (`ecky-payload.b64`) from a package archive.
/// Returns an error for legacy flat archives that have no payload envelope.
pub fn read_decoded_package_payload(archive_path: &Path) -> AppResult<Vec<u8>> {
    let archive_file = fs::File::open(archive_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to open component package archive '{}': {err}",
            archive_path.display()
        ))
    })?;
    let mut archive = ZipArchive::new(archive_file).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse component package archive '{}': {err}",
            archive_path.display()
        ))
    })?;
    read_payload_archive_bytes(&mut archive, archive_path)?.ok_or_else(|| {
        AppError::validation(format!(
            "Component package archive '{}' has no '{}' payload envelope; cannot install into the content-addressed store.",
            archive_path.display(),
            COMPONENT_PACKAGE_PAYLOAD_FILE_NAME
        ))
    })
}

/// Install a package payload into the global content-addressed store and write
/// the mutable coordinate index entry. Same coordinate + same digest is
/// idempotent; same coordinate + different digest is rejected before any
/// mutation, leaving existing content intact.
pub fn install_component_package_to_store(
    app: &dyn PathResolver,
    archive_path: &Path,
) -> AppResult<InstalledStorePackage> {
    let header = read_component_package_header_from_archive(archive_path)?;
    let payload_bytes = read_decoded_package_payload(archive_path)?;
    let validated = validate_payload_archive(&payload_bytes)?;
    let payload_package = read_component_package_from_payload(&payload_bytes, archive_path)?;
    if payload_package.package_id != header.package_id || payload_package.version != header.version
    {
        return Err(AppError::validation(format!(
            "Package envelope coordinate '{}@{}' does not match payload coordinate '{}@{}'.",
            header.package_id, header.version, payload_package.package_id, payload_package.version
        )));
    }
    let (package_digest, inventory) = compute_package_payload_digest(&validated);
    let _lock = acquire_component_store_mutation_lock(app)?;

    if let Some(existing) = read_coordinate_index(app, &header.package_id, &header.version)? {
        if existing.package_digest != package_digest {
            return Err(AppError::validation(format!(
                "Package coordinate '{}@{}' is already installed with payload digest '{}'; refusing to overwrite with differing digest '{}'.",
                header.package_id, header.version, existing.package_digest, package_digest
            )));
        }
    }
    enforce_immutable_coordinate_locked(app, &header.package_id, &header.version, &package_digest)?;

    let store_dir = publish_validated_payload_locked(app, &validated, &package_digest, inventory)?;
    write_immutable_coordinate_record_locked(
        app,
        &header.package_id,
        &header.version,
        &package_digest,
    )?;
    write_coordinate_index_locked(app, &header.package_id, &header.version, &package_digest)?;
    Ok(InstalledStorePackage {
        package_id: header.package_id.clone(),
        version: header.version.clone(),
        package_digest,
        store_dir,
    })
}

pub fn list_installed_component_package_headers(
    app: &dyn PathResolver,
) -> AppResult<Vec<ComponentPackageHeader>> {
    let mut headers = coordinate_index_entries(app)?
        .into_iter()
        .map(|entry| {
            let store_dir = payload_store_dir(app, &entry.package_digest)?;
            let package = read_component_package_manifest(&store_dir)?;
            if package.package_id != entry.package_id || package.version != entry.version {
                return Err(AppError::validation(format!(
                    "Coordinate index '{}@{}' points to a payload with coordinate '{}@{}'.",
                    entry.package_id, entry.version, package.package_id, package.version
                )));
            }
            component_package_header(&package)
        })
        .collect::<AppResult<Vec<_>>>()?;
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
    let entry = read_coordinate_index(app, package_id, version)?.ok_or_else(|| {
        AppError::not_found(format!(
            "Installed component package '{}@{}' was not found in the coordinate index.",
            package_id, version
        ))
    })?;
    let package_dir = payload_store_dir(app, &entry.package_digest)?;
    let inventory = read_payload_inventory(&package_dir)?;
    if inventory.package_digest != entry.package_digest {
        return Err(AppError::validation(format!(
            "Installed component package '{}@{}' has an integrity sidecar digest '{}' that does not match coordinate digest '{}'.",
            package_id, version, inventory.package_digest, entry.package_digest
        )));
    }
    let package = read_component_package_manifest(&package_dir)?;
    if package.package_id != package_id || package.version != version {
        return Err(AppError::validation(format!(
            "Installed component package index '{}@{}' resolves to payload coordinate '{}@{}'.",
            package_id, version, package.package_id, package.version
        )));
    }
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

// --- Package payload integrity and content-addressed storage ---
// (component-package-imports, Decisions 5 & 7)
//
// Pure payload validation + digesting, then the global content-addressed
// store and the mutable coordinate index. This lives alongside the legacy
// per-coordinate install layout; the two never collide (the store is under
// `store/sha256/`, the index under `index/`, the legacy layout under
// `<packageId>/<version>/`).

/// Domain-separated prefix for package payload digests. Trailing NUL prevents
/// prefix-extension ambiguity.
pub const PACKAGE_PAYLOAD_DOMAIN_PREFIX: &[u8] = b"ecky-package-payload-v1\0";
/// Runtime-owned integrity sidecar written into each store payload directory.
/// It is reserved: a payload archive that itself contains this path is rejected
/// before digesting, and it is never part of its own digest input.
pub const PACKAGE_INTEGRITY_FILE_NAME: &str = "ecky-integrity.json";
const PACKAGE_STORE_DIR_NAME: &str = "store";
const PACKAGE_STORE_ALGORITHM_DIR: &str = "sha256";
const PACKAGE_INDEX_DIR_NAME: &str = "index";
const PACKAGE_COORDINATE_RECORD_DIR_NAME: &str = "coordinate-records";
const PACKAGE_STORE_MUTATION_LOCK_FILE_NAME: &str = ".store-mutation.lock";

/// Explicit roots supplied by the owner of persisted locks (for example,
/// `Message.artifactBundle`) when collecting the shared package store.
#[derive(Clone, Debug)]
pub struct ComponentStoreGcRequest {
    pub explicit_root_digests: BTreeSet<String>,
    pub grace_period: Duration,
}

/// A seam for render/export work that has resolved a payload but has not yet
/// persisted its dependency lock. The owner supplies currently pinned digests
/// on both GC root checks.
pub trait ComponentStoreInFlightPins {
    fn pinned_package_digests(&self) -> BTreeSet<String>;
}

fn runtime_component_store_pin_counts() -> &'static Mutex<BTreeMap<String, usize>> {
    static COUNTS: OnceLock<Mutex<BTreeMap<String, usize>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// RAII pin held by live render/export work between dependency resolution and
/// durable lock persistence.
pub struct ComponentStorePinGuard {
    digests: Vec<String>,
}

impl Drop for ComponentStorePinGuard {
    fn drop(&mut self) {
        let mut counts = runtime_component_store_pin_counts()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for digest in &self.digests {
            if let Some(count) = counts.get_mut(digest) {
                *count -= 1;
                if *count == 0 {
                    counts.remove(digest);
                }
            }
        }
    }
}

pub fn pin_component_store_payloads(
    digests: impl IntoIterator<Item = String>,
) -> ComponentStorePinGuard {
    let mut digests = digests.into_iter().collect::<Vec<_>>();
    digests.sort();
    digests.dedup();
    let mut counts = runtime_component_store_pin_counts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for digest in &digests {
        *counts.entry(digest.clone()).or_insert(0) += 1;
    }
    drop(counts);
    ComponentStorePinGuard { digests }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeComponentStorePins;

impl ComponentStoreInFlightPins for RuntimeComponentStorePins {
    fn pinned_package_digests(&self) -> BTreeSet<String> {
        runtime_component_store_pin_counts()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComponentStoreGcReport {
    pub deleted_package_digests: Vec<String>,
    pub retained_package_digests: Vec<String>,
}

struct ComponentStoreMutationLock {
    file: fs::File,
}

impl Drop for ComponentStoreMutationLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: this file descriptor is owned by this lock guard.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn acquire_component_store_mutation_lock(
    app: &dyn PathResolver,
) -> AppResult<ComponentStoreMutationLock> {
    let root = component_library_root(app)?;
    let path = root.join(PACKAGE_STORE_MUTATION_LOCK_FILE_NAME);
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|err| {
            AppError::persistence(format!(
                "Failed to open component store mutation lock '{}': {err}",
                path.display()
            ))
        })?;
    #[cfg(unix)]
    {
        // SAFETY: flock is applied to a valid owned descriptor. LOCK_EX blocks
        // until every other Ecky process releases the same store lock.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(AppError::persistence(format!(
                "Failed to acquire component store mutation lock '{}': {}",
                path.display(),
                std::io::Error::last_os_error()
            )));
        }
    }
    #[cfg(not(unix))]
    {
        return Err(AppError::persistence(
            "Component store mutation locking is unsupported on this platform.",
        ));
    }
    Ok(ComponentStoreMutationLock { file })
}

/// One validated, in-memory payload entry. Paths are normalized UTF-8 with `/`
/// separators and unique within the payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedPayloadEntry {
    pub path: String,
    pub content: Vec<u8>,
}

/// A payload archive that has passed safe-path / reserved-name / duplicate /
/// symlink validation and is ready for deterministic digesting. Entries are
/// sorted by normalized UTF-8 path bytes.
#[derive(Clone, Debug)]
pub struct ValidatedPayload {
    pub entries: Vec<ValidatedPayloadEntry>,
}

/// Validate a decoded inner package payload archive (raw zip bytes) against
/// the package-digest file-set rules:
/// - include every non-directory regular-file entry after safe-path validation;
/// - exclude root-level outer-envelope `ecky-header.json` and `ecky-payload.b64`;
/// - reject `ecky-integrity.json` (reserved), duplicate normalized paths,
///   traversal, symlinks, and non-UTF-8 names.
///   Returns entries sorted by normalized path bytes.
pub fn validate_payload_archive(payload: &[u8]) -> AppResult<ValidatedPayload> {
    let mut archive = ZipArchive::new(Cursor::new(payload)).map_err(|err| {
        AppError::parse(format!("Failed to parse package payload archive: {err}"))
    })?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut entries: Vec<ValidatedPayloadEntry> = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|err| {
            AppError::parse(format!("Failed to read payload entry {index}: {err}"))
        })?;
        if file.is_dir() {
            continue;
        }
        if let Some(mode) = file.unix_mode() {
            let kind = mode & 0o170000;
            if kind == 0o120000 {
                return Err(AppError::validation(format!(
                    "Package payload entry '{}' is a symlink; symlinks are not allowed.",
                    file.name()
                )));
            }
            if kind != 0 && kind != 0o100000 {
                return Err(AppError::validation(format!(
                    "Package payload entry '{}' is not a regular file.",
                    file.name()
                )));
            }
        }
        // Reject non-UTF-8 archive names: decode from raw bytes.
        let raw_name = file.name_raw();
        let decoded_name = std::str::from_utf8(raw_name).map_err(|_| {
            AppError::validation(format!(
                "Package payload entry has a non-UTF-8 name ({} bytes); non-UTF-8 names are not allowed.",
                raw_name.len()
            ))
        })?;
        let normalized = normalize_payload_path(decoded_name)?;
        // Reserved integrity sidecar cannot ship inside a payload. Nested
        // files with this basename are ordinary payload content; only the
        // runtime-owned root sidecar is reserved.
        if normalized == PACKAGE_INTEGRITY_FILE_NAME {
            return Err(AppError::validation(format!(
                "Package payload entry '{}' is reserved for runtime integrity metadata and may not ship inside a payload.",
                normalized
            )));
        }
        // Outer-envelope files are excluded from the digest set (they are not
        // part of the inner payload content).
        if normalized == COMPONENT_PACKAGE_HEADER_FILE_NAME
            || normalized == COMPONENT_PACKAGE_PAYLOAD_FILE_NAME
        {
            continue;
        }
        if !seen.insert(normalized.clone()) {
            return Err(AppError::validation(format!(
                "Package payload contains a duplicate normalized path '{}'.",
                normalized
            )));
        }
        let mut content = Vec::new();
        file.read_to_end(&mut content).map_err(|err| {
            AppError::parse(format!(
                "Failed to read payload entry '{normalized}': {err}"
            ))
        })?;
        entries.push(ValidatedPayloadEntry {
            path: normalized,
            content,
        });
    }
    entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok(ValidatedPayload { entries })
}

/// Deterministically re-encode validated payload entries for explicit
/// portable project export. Runtime sidecars remain excluded.
pub fn encode_validated_payload_archive(payload: &ValidatedPayload) -> AppResult<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(cursor);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    for entry in &payload.entries {
        archive.start_file(&entry.path, options).map_err(|error| {
            AppError::persistence(format!(
                "Failed to create portable payload entry '{}': {}",
                entry.path, error
            ))
        })?;
        archive.write_all(&entry.content).map_err(|error| {
            AppError::persistence(format!(
                "Failed to write portable payload entry '{}': {}",
                entry.path, error
            ))
        })?;
    }
    archive
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| {
            AppError::persistence(format!(
                "Failed to finalize portable package payload: {error}"
            ))
        })
}

fn normalize_payload_path(entry_name: &str) -> AppResult<String> {
    let slashed = entry_name.replace('\\', "/");
    let safe = safe_archive_path(&slashed)?;
    safe.to_str()
        .map(|path| path.replace('\\', "/"))
        .ok_or_else(|| {
            AppError::validation(format!(
                "Package payload entry '{}' is not valid UTF-8.",
                entry_name
            ))
        })
}

/// Compute the canonical package payload digest (`sha256:<hex>`) and an ordered
/// per-file inventory. The digest uses domain prefix `ecky-package-payload-v1\0`,
/// length-delimited path/content bytes, and entries sorted by normalized path
/// bytes. The inventory is NOT digest input (it would self-reference).
pub fn compute_package_payload_digest(
    payload: &ValidatedPayload,
) -> (String, Vec<PackagePayloadInventoryEntry>) {
    let mut hasher = Sha256::new();
    hasher.update(PACKAGE_PAYLOAD_DOMAIN_PREFIX);
    let mut inventory = Vec::with_capacity(payload.entries.len());
    for entry in &payload.entries {
        let path_bytes = entry.path.as_bytes();
        hasher.update((path_bytes.len() as u64).to_be_bytes());
        hasher.update(path_bytes);
        hasher.update((entry.content.len() as u64).to_be_bytes());
        hasher.update(&entry.content);
        inventory.push(PackagePayloadInventoryEntry {
            path: entry.path.clone(),
            sha256: format!("sha256:{:x}", Sha256::digest(&entry.content)),
        });
    }
    (format!("sha256:{:x}", hasher.finalize()), inventory)
}

fn component_store_algorithm_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    Ok(component_library_root(app)?
        .join(PACKAGE_STORE_DIR_NAME)
        .join(PACKAGE_STORE_ALGORITHM_DIR))
}

/// Directory of one content-addressed payload in the global store, keyed by
/// the package payload digest. Validates the digest is a hex `sha256:` value
/// safe for use as a path segment.
pub fn payload_store_dir(app: &dyn PathResolver, package_digest: &str) -> AppResult<PathBuf> {
    let hex = package_digest.strip_prefix("sha256:").ok_or_else(|| {
        AppError::validation(format!(
            "Package payload digest '{package_digest}' must start with 'sha256:'."
        ))
    })?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::validation(format!(
            "Package payload digest '{package_digest}' must contain exactly 64 hexadecimal bytes."
        )));
    }
    Ok(component_store_algorithm_root(app)?.join(hex))
}

/// Publish a validated payload into the global content-addressed store. Writes
/// every entry under its normalized path plus the runtime-owned
/// `ecky-integrity.json` sidecar. Idempotent: if the store directory already
/// holds an integrity sidecar, no files are rewritten.
pub fn publish_validated_payload(
    app: &dyn PathResolver,
    payload: &ValidatedPayload,
    package_digest: &str,
    inventory: Vec<PackagePayloadInventoryEntry>,
) -> AppResult<PathBuf> {
    let _lock = acquire_component_store_mutation_lock(app)?;
    publish_validated_payload_locked(app, payload, package_digest, inventory)
}

fn publish_validated_payload_locked(
    app: &dyn PathResolver,
    payload: &ValidatedPayload,
    package_digest: &str,
    inventory: Vec<PackagePayloadInventoryEntry>,
) -> AppResult<PathBuf> {
    let store_dir = payload_store_dir(app, package_digest)?;
    let expected_sidecar = PackagePayloadInventory {
        schema_version: PACKAGE_PAYLOAD_INVENTORY_SCHEMA_VERSION,
        package_digest: package_digest.to_string(),
        entries: inventory,
    };
    if store_dir.exists() {
        if store_dir.join(PACKAGE_INTEGRITY_FILE_NAME).is_file() {
            let found = read_payload_inventory(&store_dir)?;
            if found != expected_sidecar {
                return Err(AppError::validation(format!(
                    "Content-addressed store '{}' has integrity metadata that does not match digest '{}'.",
                    store_dir.display(),
                    package_digest
                )));
            }
            return Ok(store_dir);
        }
        fs::remove_dir_all(&store_dir).map_err(|err| {
            AppError::persistence(format!(
                "Failed to remove incomplete package payload store '{}': {err}",
                store_dir.display()
            ))
        })?;
    }

    let store_parent = store_dir.parent().ok_or_else(|| {
        AppError::internal(format!(
            "Package store path '{}' has no parent.",
            store_dir.display()
        ))
    })?;
    fs::create_dir_all(store_parent).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create package payload store parent '{}': {err}",
            store_parent.display()
        ))
    })?;
    let staging_dir = store_parent.join(format!(
        ".{}.staging-{}",
        store_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("payload"),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&staging_dir).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create package payload staging directory '{}': {err}",
            staging_dir.display()
        ))
    })?;
    let publish_result = (|| {
        for entry in &payload.entries {
            let output = staging_dir.join(safe_archive_path(&entry.path)?);
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    AppError::persistence(format!(
                        "Failed to create package payload staging directory '{}': {err}",
                        parent.display()
                    ))
                })?;
            }
            write_synced_file(&output, &entry.content)?;
        }
        write_payload_inventory(&staging_dir, &expected_sidecar)?;
        sync_directory(&staging_dir)?;
        fs::rename(&staging_dir, &store_dir).map_err(|err| {
            AppError::persistence(format!(
                "Failed to atomically publish package payload '{}' to '{}': {err}",
                staging_dir.display(),
                store_dir.display()
            ))
        })?;
        sync_directory(store_parent)
    })();
    if publish_result.is_err() && staging_dir.exists() {
        let _ = fs::remove_dir_all(&staging_dir);
    }
    publish_result?;
    Ok(store_dir)
}

/// Write the runtime-owned `ecky-integrity.json` sidecar into a payload store
/// directory.
pub fn write_payload_inventory(
    store_dir: &Path,
    inventory: &PackagePayloadInventory,
) -> AppResult<PathBuf> {
    let path = store_dir.join(PACKAGE_INTEGRITY_FILE_NAME);
    let json = serde_json::to_vec_pretty(inventory)
        .map_err(|err| AppError::internal(format!("Cannot serialize payload inventory: {err}")))?;
    write_synced_file(&path, &json)?;
    Ok(path)
}

fn write_synced_file(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|err| {
            AppError::persistence(format!(
                "Failed to create package store file '{}': {err}",
                path.display()
            ))
        })?;
    file.write_all(bytes).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write package store file '{}': {err}",
            path.display()
        ))
    })?;
    file.sync_all().map_err(|err| {
        AppError::persistence(format!(
            "Failed to sync package store file '{}': {err}",
            path.display()
        ))
    })
}

fn sync_directory(path: &Path) -> AppResult<()> {
    #[cfg(unix)]
    {
        let directory = fs::File::open(path).map_err(|err| {
            AppError::persistence(format!(
                "Failed to open package store directory '{}': {err}",
                path.display()
            ))
        })?;
        directory.sync_all().map_err(|err| {
            AppError::persistence(format!(
                "Failed to sync package store directory '{}': {err}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

/// Read the runtime-owned integrity sidecar from a payload store directory.
pub fn read_payload_inventory(store_dir: &Path) -> AppResult<PackagePayloadInventory> {
    let path = store_dir.join(PACKAGE_INTEGRITY_FILE_NAME);
    let raw = fs::read_to_string(&path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read package payload inventory '{}': {err}",
            path.display()
        ))
    })?;
    serde_json::from_str(&raw).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse package payload inventory '{}': {err}",
            path.display()
        ))
    })
}

fn coordinate_index_dir(app: &dyn PathResolver, package_id: &str) -> AppResult<PathBuf> {
    Ok(component_library_root(app)?
        .join(PACKAGE_INDEX_DIR_NAME)
        .join(safe_library_segment(package_id, "packageId")?))
}

fn coordinate_index_path(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
) -> AppResult<PathBuf> {
    Ok(coordinate_index_dir(app, package_id)?.join(format!(
        "{}.json",
        safe_library_segment(version, "version")?
    )))
}

fn immutable_coordinate_record_path(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
) -> AppResult<PathBuf> {
    Ok(component_library_root(app)?
        .join(PACKAGE_COORDINATE_RECORD_DIR_NAME)
        .join(safe_library_segment(package_id, "packageId")?)
        .join(format!(
            "{}.json",
            safe_library_segment(version, "version")?
        )))
}

fn read_immutable_coordinate_record(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
) -> AppResult<Option<ComponentCoordinateIndexEntry>> {
    let path = immutable_coordinate_record_path(app, package_id, version)?;
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read immutable coordinate record '{}': {err}",
            path.display()
        ))
    })?;
    serde_json::from_str(&raw).map(Some).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse immutable coordinate record '{}': {err}",
            path.display()
        ))
    })
}

fn enforce_immutable_coordinate_locked(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
    package_digest: &str,
) -> AppResult<()> {
    if let Some(existing) = read_immutable_coordinate_record(app, package_id, version)? {
        if existing.package_digest != package_digest {
            return Err(AppError::validation(format!(
                "Package coordinate '{}@{}' was previously bound to payload digest '{}'; refusing to overwrite with differing digest '{}'.",
                package_id, version, existing.package_digest, package_digest
            )));
        }
    }
    Ok(())
}

fn write_immutable_coordinate_record_locked(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
    package_digest: &str,
) -> AppResult<()> {
    enforce_immutable_coordinate_locked(app, package_id, version, package_digest)?;
    let path = immutable_coordinate_record_path(app, package_id, version)?;
    if path.is_file() {
        return Ok(());
    }
    let parent = path.parent().ok_or_else(|| {
        AppError::internal(format!(
            "Immutable coordinate record path '{}' has no parent.",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create immutable coordinate record directory '{}': {err}",
            parent.display()
        ))
    })?;
    let entry = ComponentCoordinateIndexEntry {
        package_id: package_id.to_string(),
        version: version.to_string(),
        package_digest: package_digest.to_string(),
    };
    let json = serde_json::to_vec(&entry).map_err(|err| {
        AppError::internal(format!(
            "Cannot serialize immutable coordinate record: {err}"
        ))
    })?;
    let temporary = parent.join(format!(
        ".{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("coordinate"),
        uuid::Uuid::new_v4()
    ));
    write_synced_file(&temporary, &json)?;
    fs::rename(&temporary, &path).map_err(|err| {
        let _ = fs::remove_file(&temporary);
        AppError::persistence(format!(
            "Failed to publish immutable coordinate record '{}': {err}",
            path.display()
        ))
    })?;
    sync_directory(parent)
}

/// Write the mutable discovery index for an exact coordinate while preserving
/// its permanent coordinate-to-digest binding across uninstall and GC.
pub fn write_coordinate_index(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
    package_digest: &str,
) -> AppResult<PathBuf> {
    let _lock = acquire_component_store_mutation_lock(app)?;
    enforce_immutable_coordinate_locked(app, package_id, version, package_digest)?;
    write_immutable_coordinate_record_locked(app, package_id, version, package_digest)?;
    write_coordinate_index_locked(app, package_id, version, package_digest)
}

fn write_coordinate_index_locked(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
    package_digest: &str,
) -> AppResult<PathBuf> {
    payload_store_dir(app, package_digest)?;
    let path = coordinate_index_path(app, package_id, version)?;
    let parent = path.parent().ok_or_else(|| {
        AppError::internal(format!(
            "Coordinate index path '{}' has no parent.",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create coordinate index directory '{}': {err}",
            parent.display()
        ))
    })?;
    let entry = ComponentCoordinateIndexEntry {
        package_id: package_id.to_string(),
        version: version.to_string(),
        package_digest: package_digest.to_string(),
    };
    let json = serde_json::to_vec(&entry)
        .map_err(|err| AppError::internal(format!("Cannot serialize coordinate index: {err}")))?;
    let temporary = parent.join(format!(
        ".{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("index"),
        uuid::Uuid::new_v4()
    ));
    write_synced_file(&temporary, &json)?;
    fs::rename(&temporary, &path).map_err(|err| {
        let _ = fs::remove_file(&temporary);
        AppError::persistence(format!(
            "Failed to atomically update coordinate index '{}': {err}",
            path.display()
        ))
    })?;
    sync_directory(parent)?;
    Ok(path)
}

/// Read the mutable coordinate index entry for an exact coordinate, or `None`
/// when the coordinate is not indexed.
pub fn read_coordinate_index(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
) -> AppResult<Option<ComponentCoordinateIndexEntry>> {
    let path = coordinate_index_path(app, package_id, version)?;
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read coordinate index '{}': {err}",
            path.display()
        ))
    })?;
    let entry: ComponentCoordinateIndexEntry = serde_json::from_str(&raw).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse coordinate index '{}': {err}",
            path.display()
        ))
    })?;
    Ok(Some(entry))
}

/// Remove the coordinate index entry for an exact coordinate. Returns whether
/// an entry was removed. Library uninstall uses this so new unlocked resolution
/// cannot discover the coordinate, while committed locks keep payloads alive.
pub fn remove_coordinate_index(
    app: &dyn PathResolver,
    package_id: &str,
    version: &str,
) -> AppResult<bool> {
    let _lock = acquire_component_store_mutation_lock(app)?;
    let path = coordinate_index_path(app, package_id, version)?;
    if !path.is_file() {
        return Ok(false);
    }
    fs::remove_file(&path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to remove coordinate index '{}': {err}",
            path.display()
        ))
    })?;
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(true)
}

fn coordinate_index_entries(
    app: &dyn PathResolver,
) -> AppResult<Vec<ComponentCoordinateIndexEntry>> {
    let root = component_library_root(app)?.join(PACKAGE_INDEX_DIR_NAME);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for package in fs::read_dir(&root).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read coordinate index root '{}': {err}",
            root.display()
        ))
    })? {
        let package = package.map_err(|err| AppError::persistence(err.to_string()))?;
        if !package.path().is_dir() {
            continue;
        }
        for version in fs::read_dir(package.path()).map_err(|err| {
            AppError::persistence(format!(
                "Failed to read coordinate index directory '{}': {err}",
                package.path().display()
            ))
        })? {
            let version = version.map_err(|err| AppError::persistence(err.to_string()))?;
            let path = version.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read_to_string(&path).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to read coordinate index '{}': {err}",
                    path.display()
                ))
            })?;
            let entry =
                serde_json::from_str::<ComponentCoordinateIndexEntry>(&raw).map_err(|err| {
                    AppError::parse(format!(
                        "Failed to parse coordinate index '{}': {err}",
                        path.display()
                    ))
                })?;
            entries.push(entry);
        }
    }
    entries.sort_by(|left, right| {
        left.package_id
            .cmp(&right.package_id)
            .then_with(|| left.version.cmp(&right.version))
    });
    Ok(entries)
}

fn component_store_root_digests(
    app: &dyn PathResolver,
    explicit_roots: &BTreeSet<String>,
    in_flight: &dyn ComponentStoreInFlightPins,
) -> AppResult<BTreeSet<String>> {
    let mut roots = explicit_roots.clone();
    roots.extend(in_flight.pinned_package_digests());
    roots.extend(
        coordinate_index_entries(app)?
            .into_iter()
            .map(|entry| entry.package_digest),
    );
    Ok(roots)
}

/// Collect unreachable package payloads. Callers provide persisted dependency
/// lock digests explicitly; installed coordinates and in-flight pins are added
/// here. Roots are collected once for candidate selection and again while the
/// store mutation lock is held immediately before deletion.
pub fn garbage_collect_component_package_store(
    app: &dyn PathResolver,
    request: &ComponentStoreGcRequest,
    in_flight: &dyn ComponentStoreInFlightPins,
) -> AppResult<ComponentStoreGcReport> {
    let first_roots = component_store_root_digests(app, &request.explicit_root_digests, in_flight)?;
    let store_root = component_store_algorithm_root(app)?;
    if !store_root.exists() {
        return Ok(ComponentStoreGcReport::default());
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&store_root).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read package store '{}': {err}",
            store_root.display()
        ))
    })? {
        let path = entry
            .map_err(|err| AppError::persistence(err.to_string()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let Some(hex) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let digest = format!("sha256:{hex}");
        if payload_store_dir(app, &digest).is_err() || first_roots.contains(&digest) {
            continue;
        }
        let modified = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default()
            >= request.grace_period
        {
            candidates.push((digest, path));
        }
    }

    let _lock = acquire_component_store_mutation_lock(app)?;
    let second_roots =
        component_store_root_digests(app, &request.explicit_root_digests, in_flight)?;
    let mut report = ComponentStoreGcReport::default();
    for (digest, path) in candidates {
        if second_roots.contains(&digest) {
            report.retained_package_digests.push(digest);
            continue;
        }
        if !path.join(PACKAGE_INTEGRITY_FILE_NAME).is_file() {
            report.retained_package_digests.push(digest);
            continue;
        }
        fs::remove_dir_all(&path).map_err(|err| {
            AppError::persistence(format!(
                "Failed to delete unreachable package payload '{}': {err}",
                path.display()
            ))
        })?;
        report.deleted_package_digests.push(digest);
    }
    sync_directory(&store_root)?;
    report.deleted_package_digests.sort();
    report.retained_package_digests.sort();
    Ok(report)
}

// --- Extracted component library (component-unification T5) ---
//
// Extracted components are stored one directory per component directly under
// the component-library dir: `<library>/<name>/component.ecky` (copy-inline
// `define-component` source) plus `<library>/<name>/ecky-header.json`
// (compact header). Installed package payloads live under
// `<library>/store/sha256/<digest>/` and discovery records live under
// `<library>/index/`; these layouts never collide.

pub const EXTRACTED_COMPONENT_SOURCE_FILE_NAME: &str = "component.ecky";
const AUTO_COMPONENT_LIBRARY_DIR_NAME: &str = "local-captures";
const AUTO_COMPONENT_LATEST_FILE_NAME: &str = "latest.json";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoComponentLatest {
    schema_version: u32,
    revision_digest: String,
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedComponentSearchResult {
    pub name: String,
    /// Immutable source revision for shipped components. User-extracted
    /// components remain unversioned until they are packaged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub one_liner: String,
    pub param_keys: Vec<String>,
    pub tags: Vec<String>,
    pub ports: Vec<ExtractedComponentPortSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_title: Option<String>,
    pub origin: String,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedComponentPortSummary {
    pub port_id: String,
    pub type_id: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedComponentRecord {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub source: String,
    pub header: crate::component_extract::ComponentHeader,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_digest: Option<String>,
}

pub fn save_extracted_component(
    app: &dyn PathResolver,
    extracted: &crate::component_extract::ExtractedComponent,
) -> AppResult<PathBuf> {
    let dir = extracted_component_dir(app, &extracted.name)?;
    fs::create_dir_all(&dir).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create component directory '{}': {}",
            dir.display(),
            err
        ))
    })?;
    let source_path = dir.join(EXTRACTED_COMPONENT_SOURCE_FILE_NAME);
    fs::write(&source_path, &extracted.component_source).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write '{}': {}",
            source_path.display(),
            err
        ))
    })?;
    let header_path = dir.join(COMPONENT_PACKAGE_HEADER_FILE_NAME);
    let header_json = serde_json::to_string_pretty(&extracted.header).map_err(|err| {
        AppError::internal(format!("Failed to serialize component header: {err}"))
    })?;
    fs::write(&header_path, header_json).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write '{}': {}",
            header_path.display(),
            err
        ))
    })?;
    Ok(dir)
}

/// Store successful-source definitions as immutable, thread-scoped revisions.
/// The latest pointer is replaced atomically only after source and header exist.
pub fn capture_defined_components(
    app: &dyn PathResolver,
    thread_id: &str,
    message_id: &str,
    source: &str,
) -> AppResult<()> {
    for component in
        crate::component_extract::extract_defined_components(source, thread_id, message_id)?
    {
        let component_id = component.header.component_id.as_deref().ok_or_else(|| {
            AppError::internal("Automatic component capture has no stable component ID.")
        })?;
        let digest = component.header.revision_digest.as_deref().ok_or_else(|| {
            AppError::internal("Automatic component capture has no revision digest.")
        })?;
        let digest_segment = digest.strip_prefix("sha256:").unwrap_or(digest);
        let identity_dir = extracted_component_library_root(app)?
            .join(AUTO_COMPONENT_LIBRARY_DIR_NAME)
            .join(component_id);
        let revision_dir = identity_dir.join("revisions").join(digest_segment);
        fs::create_dir_all(&revision_dir).map_err(|err| {
            AppError::persistence(format!(
                "Failed to create component revision '{}': {err}",
                revision_dir.display()
            ))
        })?;
        let source_path = revision_dir.join(EXTRACTED_COMPONENT_SOURCE_FILE_NAME);
        let header_path = revision_dir.join(COMPONENT_PACKAGE_HEADER_FILE_NAME);
        if !source_path.is_file() || !header_path.is_file() {
            write_atomic(&source_path, component.component_source.as_bytes())?;
            let header = serde_json::to_vec_pretty(&component.header).map_err(|err| {
                AppError::internal(format!("Failed to serialize component header: {err}"))
            })?;
            write_atomic(&header_path, &header)?;
        }
        read_component_revision(app, component_id, digest)?;
        let latest_path = identity_dir.join(AUTO_COMPONENT_LATEST_FILE_NAME);
        let same_latest = fs::read_to_string(&latest_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<AutoComponentLatest>(&raw).ok())
            .is_some_and(|latest| latest.schema_version == 1 && latest.revision_digest == digest);
        if !same_latest {
            let pointer = serde_json::to_vec(&AutoComponentLatest {
                schema_version: 1,
                revision_digest: digest.to_string(),
            })
            .map_err(|err| {
                AppError::internal(format!(
                    "Failed to serialize latest component revision: {err}"
                ))
            })?;
            write_atomic(&latest_path, &pointer)?;
        }
    }
    Ok(())
}

/// Publish only when this successful version is still latest for its thread.
/// Callers hold the shared history DB lock across this check and publication so
/// a later writer cannot be followed by a stale pointer update.
pub fn capture_latest_successful_components(
    app: &dyn PathResolver,
    conn: &rusqlite::Connection,
    thread_id: &str,
    message_id: &str,
    source: &str,
) -> AppResult<()> {
    let latest = crate::db::latest_successful_component_capture_targets(conn)
        .map_err(|error| AppError::persistence(error.to_string()))?;
    if latest
        .iter()
        .any(|target| target.thread_id == thread_id && target.message_id == message_id)
    {
        capture_defined_components(app, thread_id, message_id, source)?;
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("Component library file has no parent directory."))?;
    fs::create_dir_all(parent).map_err(|err| {
        AppError::persistence(format!(
            "Failed to create component library directory '{}': {err}",
            parent.display()
        ))
    })?;
    let temp = parent.join(format!(".tmp-{}", uuid::Uuid::new_v4()));
    fs::write(&temp, bytes).map_err(|err| {
        AppError::persistence(format!(
            "Failed to write component library file '{}': {err}",
            temp.display()
        ))
    })?;
    fs::rename(&temp, path).map_err(|err| {
        let _ = fs::remove_file(&temp);
        AppError::persistence(format!(
            "Failed to publish component library file '{}': {err}",
            path.display()
        ))
    })
}

/// Header-only library scan: never reads `component.ecky` bodies.
pub fn search_extracted_components(
    app: &dyn PathResolver,
    query: &str,
    limit: usize,
) -> AppResult<Vec<ExtractedComponentSearchResult>> {
    let root = extracted_component_library_root(app)?;
    let mut results = Vec::new();
    let needle = query.trim().to_lowercase();
    for component in BUILTIN_STDLIB {
        let haystack = format!(
            "{} {} {}",
            component.name,
            component.one_liner,
            component.tags.join(" ")
        )
        .to_lowercase();
        if !needle.is_empty() && !haystack.contains(&needle) {
            continue;
        }
        results.push(ExtractedComponentSearchResult {
            name: component.name.to_string(),
            version: Some(component.version.to_string()),
            one_liner: component.one_liner.to_string(),
            param_keys: component
                .params
                .iter()
                .map(|key| (*key).to_string())
                .collect(),
            tags: component
                .tags
                .iter()
                .map(|tag| (*tag).to_string())
                .collect(),
            ports: Vec::new(),
            component_id: None,
            revision_digest: None,
            thread_id: None,
            thread_title: None,
            origin: "builtin".to_string(),
        });
        if results.len() >= limit {
            return Ok(results);
        }
    }
    if !root.exists() {
        return Ok(results);
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(&root)
        .map_err(|err| {
            AppError::persistence(format!(
                "Failed to read component library '{}': {}",
                root.display(),
                err
            ))
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    entries.sort();
    for dir in entries {
        let header_path = dir.join(COMPONENT_PACKAGE_HEADER_FILE_NAME);
        if !header_path.is_file() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&header_path) else {
            continue;
        };
        let Ok(header) = serde_json::from_str::<crate::component_extract::ComponentHeader>(&raw)
        else {
            continue;
        };
        let haystack = format!(
            "{} {} {}",
            header.name,
            header.description.clone().unwrap_or_default(),
            header.tags.join(" ")
        )
        .to_lowercase();
        if !needle.is_empty() && !haystack.contains(&needle) {
            continue;
        }
        let param_keys: Vec<String> = header
            .params
            .iter()
            .map(|param| param.key.clone())
            .collect();
        let one_liner = header
            .description
            .clone()
            .unwrap_or_else(|| format!("component {} ({})", header.name, param_keys.join(" ")));
        results.push(ExtractedComponentSearchResult {
            name: header.name,
            version: None,
            one_liner,
            param_keys,
            tags: header.tags,
            ports: header
                .ports
                .into_iter()
                .map(|port| ExtractedComponentPortSummary {
                    port_id: port.port_id,
                    type_id: port.type_id,
                })
                .collect(),
            component_id: header.component_id.clone(),
            revision_digest: header.revision_digest.clone(),
            thread_id: header.provenance.thread_id.clone(),
            thread_title: None,
            origin: if header.component_id.is_some() {
                "local".to_string()
            } else {
                "extracted".to_string()
            },
        });
        if results.len() >= limit {
            break;
        }
    }
    let auto_root = root.join(AUTO_COMPONENT_LIBRARY_DIR_NAME);
    if results.len() < limit && auto_root.is_dir() {
        let mut identities = fs::read_dir(&auto_root)
            .map_err(|err| {
                AppError::persistence(format!(
                    "Failed to read component library '{}': {err}",
                    auto_root.display()
                ))
            })?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        identities.sort();
        for identity_dir in identities {
            let pointer_path = identity_dir.join(AUTO_COMPONENT_LATEST_FILE_NAME);
            let Ok(pointer_raw) = fs::read_to_string(&pointer_path) else {
                continue;
            };
            let Ok(pointer) = serde_json::from_str::<AutoComponentLatest>(&pointer_raw) else {
                continue;
            };
            if pointer.schema_version != 1 {
                continue;
            }
            let Some(revision) = valid_component_digest(&pointer.revision_digest) else {
                continue;
            };
            let header_path = identity_dir
                .join("revisions")
                .join(revision)
                .join(COMPONENT_PACKAGE_HEADER_FILE_NAME);
            let Ok(header_raw) = fs::read_to_string(&header_path) else {
                continue;
            };
            let Ok(header) =
                serde_json::from_str::<crate::component_extract::ComponentHeader>(&header_raw)
            else {
                continue;
            };
            if header.component_id.as_deref()
                != Some(
                    identity_dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default(),
                )
                || header.revision_digest.as_deref() != Some(pointer.revision_digest.as_str())
            {
                continue;
            }
            let haystack = format!(
                "{} {} {} {} {} {}",
                header.name,
                header.description.clone().unwrap_or_default(),
                header.tags.join(" "),
                header
                    .ports
                    .iter()
                    .flat_map(|port| [port.port_id.as_str(), port.type_id.as_str()])
                    .collect::<Vec<_>>()
                    .join(" "),
                header
                    .params
                    .iter()
                    .map(|param| param.key.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                header.dependencies.join(" ")
            )
            .to_lowercase();
            if !needle.is_empty() && !haystack.contains(&needle) {
                continue;
            }
            let param_keys = header
                .params
                .iter()
                .map(|param| param.key.clone())
                .collect::<Vec<_>>();
            let one_liner = header
                .description
                .clone()
                .unwrap_or_else(|| format!("component {} ({})", header.name, param_keys.join(" ")));
            results.push(ExtractedComponentSearchResult {
                name: header.name,
                version: None,
                one_liner,
                param_keys,
                tags: header.tags,
                ports: header
                    .ports
                    .iter()
                    .map(|port| ExtractedComponentPortSummary {
                        port_id: port.port_id.clone(),
                        type_id: port.type_id.clone(),
                    })
                    .collect(),
                component_id: header.component_id,
                revision_digest: header.revision_digest,
                thread_id: header.provenance.thread_id,
                thread_title: None,
                origin: "local".to_string(),
            });
            if results.len() >= limit {
                break;
            }
        }
    }
    Ok(results)
}

pub fn read_extracted_component(
    app: &dyn PathResolver,
    name: &str,
) -> AppResult<ExtractedComponentRecord> {
    let dir = extracted_component_dir(app, name)?;
    let source_path = dir.join(EXTRACTED_COMPONENT_SOURCE_FILE_NAME);
    let header_path = dir.join(COMPONENT_PACKAGE_HEADER_FILE_NAME);
    if !source_path.is_file() || !header_path.is_file() {
        return builtin_stdlib_component(name)
            .map(builtin_stdlib_record)
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "No component named `{}` in the component library or shipped stdlib.",
                    name
                ))
            });
    }
    let source = fs::read_to_string(&source_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read '{}': {}",
            source_path.display(),
            err
        ))
    })?;
    let raw_header = fs::read_to_string(&header_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read '{}': {}",
            header_path.display(),
            err
        ))
    })?;
    let header: crate::component_extract::ComponentHeader = serde_json::from_str(&raw_header)
        .map_err(|err| {
            AppError::persistence(format!(
                "Component header '{}' is invalid: {}",
                header_path.display(),
                err
            ))
        })?;
    Ok(ExtractedComponentRecord {
        name: name.to_string(),
        version: None,
        source,
        component_id: header.component_id.clone(),
        revision_digest: header.revision_digest.clone(),
        header,
    })
}

pub fn read_component_by_id(
    app: &dyn PathResolver,
    component_id: &str,
    revision_digest: Option<&str>,
) -> AppResult<ExtractedComponentRecord> {
    if let Some(revision_digest) = revision_digest {
        return read_component_revision(app, component_id, revision_digest);
    }
    let identity = component_id.strip_prefix("local-").unwrap_or_default();
    if identity.len() != 64
        || !identity
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(AppError::validation("Automatic component ID is invalid."));
    }
    let identity_dir = extracted_component_library_root(app)?
        .join(AUTO_COMPONENT_LIBRARY_DIR_NAME)
        .join(component_id);
    let pointer_path = identity_dir.join(AUTO_COMPONENT_LATEST_FILE_NAME);
    let raw = fs::read_to_string(&pointer_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read latest component revision '{}': {err}",
            pointer_path.display()
        ))
    })?;
    let pointer: AutoComponentLatest = serde_json::from_str(&raw).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse latest component revision '{}': {err}",
            pointer_path.display()
        ))
    })?;
    if pointer.schema_version != 1 || valid_component_digest(&pointer.revision_digest).is_none() {
        return Err(AppError::persistence(
            "Latest component revision pointer has an unsupported schema or invalid digest.",
        ));
    }
    read_component_revision(app, component_id, &pointer.revision_digest)
}

pub fn read_component_revision(
    app: &dyn PathResolver,
    component_id: &str,
    revision_digest: &str,
) -> AppResult<ExtractedComponentRecord> {
    let identity = component_id.strip_prefix("local-").unwrap_or_default();
    if identity.len() != 64
        || !identity
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(AppError::validation("Automatic component ID is invalid."));
    }
    let digest_segment = valid_component_digest(revision_digest)
        .ok_or_else(|| AppError::validation("Automatic component revision digest is invalid."))?;
    let revision_dir = extracted_component_library_root(app)?
        .join(AUTO_COMPONENT_LIBRARY_DIR_NAME)
        .join(component_id)
        .join("revisions")
        .join(digest_segment);
    let header_path = revision_dir.join(COMPONENT_PACKAGE_HEADER_FILE_NAME);
    let header_raw = fs::read_to_string(&header_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read component header '{}': {err}",
            header_path.display()
        ))
    })?;
    let header: crate::component_extract::ComponentHeader = serde_json::from_str(&header_raw)
        .map_err(|err| {
            AppError::parse(format!(
                "Failed to parse component header '{}': {err}",
                header_path.display()
            ))
        })?;
    let source_path = revision_dir.join(EXTRACTED_COMPONENT_SOURCE_FILE_NAME);
    let source = fs::read_to_string(&source_path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read component source '{}': {err}",
            source_path.display()
        ))
    })?;
    if header.component_id.as_deref() != Some(component_id)
        || header.revision_digest.as_deref() != Some(revision_digest)
        || semantic_source_digest(&source) != revision_digest
    {
        return Err(AppError::persistence(
            "Automatic component revision does not match its stored identity or digest.",
        ));
    }
    Ok(ExtractedComponentRecord {
        name: header.name.clone(),
        version: None,
        source,
        component_id: Some(component_id.to_string()),
        revision_digest: Some(revision_digest.to_string()),
        header,
    })
}

fn valid_component_digest(digest: &str) -> Option<&str> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.chars().all(|character| character.is_ascii_hexdigit())).then_some(hex)
}

fn semantic_source_digest(source: &str) -> String {
    let Ok(forms) = steel_core::parser::parser::Parser::parse_without_lowering(source) else {
        return String::new();
    };
    let canonical = forms
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    format!("sha256:{:x}", Sha256::digest(canonical.as_bytes()))
}

/// Curated, copy-inlineable parametric components shipped with Ecky.
///
/// Keep these as source, rather than generated geometry or per-size files: a
/// caller can inspect, paste, alter, and render each definition without a
/// registry dependency. `version` is carried by discovery and import callers
/// can record it beside the vendored source.
struct BuiltinStdlibComponent {
    name: &'static str,
    version: &'static str,
    one_liner: &'static str,
    params: &'static [&'static str],
    tags: &'static [&'static str],
    source: &'static str,
}

const BUILTIN_STDLIB: &[BuiltinStdlibComponent] = &[
    BuiltinStdlibComponent {
        name: "hex-bolt",
        version: "1.0.0",
        one_liner: "ISO-style hex bolt with parametric thread",
        params: &["d", "length", "pitch"],
        tags: &["fastener", "bolt", "thread"],
        source: "(define-component hex-bolt ((number d 8) (number length 30) (number pitch 1.25)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (let ((thread-depth (* pitch 0.6134))) (let ((minor-radius (- (/ d 2) thread-depth))) (union (extrude (regular-polygon 6 (* d 0.58)) (* d 0.65)) (thread :radius minor-radius :pitch pitch :length length :depth thread-depth)))))",
    },
    BuiltinStdlibComponent {
        name: "socket-head-cap-screw",
        version: "1.0.0",
        one_liner: "Socket-head cap screw with parametric thread",
        params: &["d", "length", "pitch"],
        tags: &["fastener", "screw", "thread"],
        source: "(define-component socket-head-cap-screw ((number d 6) (number length 24) (number pitch 1)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (let ((thread-depth (* pitch 0.6134))) (let ((minor-radius (- (/ d 2) thread-depth))) (union (difference (cylinder (* d 0.85) d 48) (cylinder (* d 0.32) (* d 0.45) 6)) (thread :radius minor-radius :pitch pitch :length length :depth thread-depth)))))",
    },
    BuiltinStdlibComponent {
        name: "hex-nut",
        version: "1.0.0",
        one_liner: "Hex nut cut with a mating tapped hole",
        params: &["d", "pitch", "thickness"],
        tags: &["fastener", "nut", "thread"],
        source: "(define-component hex-nut ((number d 8) (number pitch 1.25) (number thickness 6.5)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (let ((thread-depth (* pitch 0.6134))) (let ((minor-radius (- (/ d 2) thread-depth))) (difference (extrude (regular-polygon 6 (* d 0.58)) thickness) (tapped-hole :radius minor-radius :pitch pitch :depth thread-depth :length (+ thickness 2))))))",
    },
    BuiltinStdlibComponent {
        name: "washer",
        version: "1.0.0",
        one_liner: "Flat washer with parametric bore and outside diameter",
        params: &["inner-d", "outer-d", "thickness"],
        tags: &["fastener", "washer"],
        source: "(define-component washer ((number inner-d 8.4) (number outer-d 16) (number thickness 1.6)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (difference (cylinder (/ outer-d 2) thickness 64) (cylinder (/ inner-d 2) (+ thickness 2) 64)))",
    },
    BuiltinStdlibComponent {
        name: "threaded-rod",
        version: "1.0.0",
        one_liner: "Full-length parametric threaded rod",
        params: &["d", "length", "pitch"],
        tags: &["fastener", "rod", "thread"],
        source: "(define-component threaded-rod ((number d 8) (number length 100) (number pitch 1.25)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (let ((thread-depth (* pitch 0.6134))) (let ((minor-radius (- (/ d 2) thread-depth))) (thread :radius minor-radius :pitch pitch :length length :depth thread-depth))))",
    },
    BuiltinStdlibComponent {
        name: "ball-bearing",
        version: "1.0.0",
        one_liner: "608/623/624-style radial bearing family",
        params: &["bore", "outer-d", "width"],
        tags: &["bearing", "mechanical", "608", "623", "624"],
        source: "(define-component ball-bearing ((number bore 8) (number outer-d 22) (number width 7)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (difference (cylinder (/ outer-d 2) width 96) (cylinder (/ bore 2) (+ width 2) 64)))",
    },
    BuiltinStdlibComponent {
        name: "gt2-pulley",
        version: "1.0.0",
        one_liner: "GT2 timing pulley with teeth and bore controls",
        params: &["teeth", "bore", "width"],
        tags: &["pulley", "gt2", "motion"],
        source: "(define-component gt2-pulley ((number teeth 20) (number bore 5) (number width 7)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (difference (cylinder (+ (* teeth 0.3183) 1) width 96) (cylinder (/ bore 2) (+ width 2) 64)))",
    },
    BuiltinStdlibComponent {
        name: "standoff",
        version: "1.0.0",
        one_liner: "Hexagonal PCB standoff with through-hole",
        params: &["length", "outer-d", "hole-d"],
        tags: &["standoff", "pcb", "mounting"],
        source: "(define-component standoff ((number length 12) (number outer-d 6) (number hole-d 3)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (difference (extrude (regular-polygon 6 (/ outer-d 1.732)) length) (cylinder (/ hole-d 2) (+ length 2) 48)))",
    },
    BuiltinStdlibComponent {
        name: "heat-set-insert-pocket",
        version: "1.0.0",
        one_liner: "FDM heat-set insert cavity cutter with lead-in",
        params: &["bore", "depth", "lead-in"],
        tags: &["fdm", "insert", "pocket"],
        source: "(define-component heat-set-insert-pocket ((number bore 4.6) (number depth 6) (number lead-in 1)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (union (cylinder (/ bore 2) depth 64) (cone (/ bore 2) (+ (/ bore 2) lead-in) lead-in 64)))",
    },
    BuiltinStdlibComponent {
        name: "corner-bracket",
        version: "1.0.0",
        one_liner: "Reinforced right-angle mounting bracket",
        params: &["leg", "height", "thickness"],
        tags: &["bracket", "mounting", "corner"],
        source: "(define-component corner-bracket ((number leg 30) (number height 20) (number thickness 3)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (union (box leg thickness height) (box thickness leg height) (translate thickness thickness 0 (wedge (- leg thickness) (- leg thickness) height 0 0 (- leg thickness) height))))",
    },
    BuiltinStdlibComponent {
        name: "l-bracket",
        version: "1.0.0",
        one_liner: "Simple parametric L mounting bracket",
        params: &["leg", "height", "thickness"],
        tags: &["bracket", "mounting"],
        source: "(define-component l-bracket ((number leg 30) (number height 20) (number thickness 3)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (union (box leg thickness height) (box thickness leg height)))",
    },
    BuiltinStdlibComponent {
        name: "hole-plate",
        version: "1.0.0",
        one_liner: "Mounting plate with repeat-union hole grid",
        params: &["cols", "rows", "pitch", "hole-d", "thickness"],
        tags: &["plate", "mounting", "grid"],
        source: "(define-component hole-plate ((number cols 3) (number rows 2) (number pitch 15) (number hole-d 4) (number thickness 3)) (verify (tag manifold) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= value 0))) (difference (box (* (+ cols 1) pitch) (* (+ rows 1) pitch) thickness) (repeat-union col cols (translate (* (+ col 1) pitch) (* (/ (+ rows 1) 2) pitch) -1 (cylinder (/ hole-d 2) (+ thickness 2) 48)))))",
    },
];

fn builtin_stdlib_component(name: &str) -> Option<&'static BuiltinStdlibComponent> {
    BUILTIN_STDLIB
        .iter()
        .find(|component| component.name == name)
}

fn builtin_stdlib_record(component: &BuiltinStdlibComponent) -> ExtractedComponentRecord {
    ExtractedComponentRecord {
        name: component.name.to_string(),
        version: Some(component.version.to_string()),
        source: component.source.to_string(),
        component_id: None,
        revision_digest: None,
        header: crate::component_extract::ComponentHeader {
            schema_version: 1,
            name: component.name.to_string(),
            component_id: None,
            revision_digest: None,
            dependencies: Vec::new(),
            description: Some(component.one_liner.to_string()),
            params: component
                .params
                .iter()
                .map(|key| crate::component_extract::ComponentHeaderParam {
                    key: (*key).to_string(),
                    kind: "number".to_string(),
                    default: None,
                    label: None,
                })
                .collect(),
            tags: component
                .tags
                .iter()
                .map(|tag| (*tag).to_string())
                .collect(),
            provenance: crate::component_extract::ComponentProvenance {
                project_id: None,
                thread_id: None,
                message_id: None,
                source_digest: format!("builtin:{}@{}", component.name, component.version),
            },
            interfaces: Vec::new(),
            ports: Vec::new(),
        },
    }
}

fn extracted_component_library_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    component_library_root(app)
}

fn extracted_component_dir(app: &dyn PathResolver, name: &str) -> AppResult<PathBuf> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        || !name
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
    {
        return Err(AppError::validation(format!(
            "Component name `{}` is not a safe library directory name.",
            name
        )));
    }
    Ok(extracted_component_library_root(app)?.join(name))
}

#[cfg(test)]
mod extracted_component_library_tests {
    use super::*;
    use crate::component_extract::{ComponentExtractRequest, extract_component};
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
            root: std::env::temp_dir().join(format!("ecky-component-lib-{name}-{nonce}")),
        }
    }

    fn sample_extracted(name: &str) -> crate::component_extract::ExtractedComponent {
        let source = r#"
            (model
              (params (number width 12 :label "Width"))
              (part bracket (box width 4 2)))
        "#;
        extract_component(&ComponentExtractRequest {
            source: source.to_string(),
            part_key: "bracket".to_string(),
            component_name: Some(name.to_string()),
            description: Some("L-shaped mounting bracket".to_string()),
            tags: vec!["bracket".to_string(), "mount".to_string()],
            thread_id: Some("thread-1".to_string()),
            message_id: Some("message-1".to_string()),
        })
        .expect("extract")
    }

    fn sample_extracted_with_port(name: &str) -> crate::component_extract::ExtractedComponent {
        let source = r#"
            (model
              (part bracket
                (ports
                  (port mount :type "mechanical.mount.v1"
                    :frame (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1))))
                (box 12 4 2)))
        "#;
        extract_component(&ComponentExtractRequest {
            source: source.to_string(),
            part_key: "bracket".to_string(),
            component_name: Some(name.to_string()),
            description: Some("Ported bracket".to_string()),
            ..Default::default()
        })
        .expect("extract ported component")
    }

    #[test]
    fn save_search_get_round_trip() {
        let resolver = temp_resolver("roundtrip");
        let extracted = sample_extracted("bracket");
        let dir = save_extracted_component(&resolver, &extracted).expect("save");
        assert!(dir.join("component.ecky").is_file());
        assert!(dir.join("ecky-header.json").is_file());

        let results = search_extracted_components(&resolver, "L-shaped", 10).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "bracket");
        assert_eq!(results[0].one_liner, "L-shaped mounting bracket");
        assert_eq!(results[0].param_keys, vec!["width".to_string()]);
        assert_eq!(
            results[0].tags,
            vec!["bracket".to_string(), "mount".to_string()]
        );

        let record = read_extracted_component(&resolver, "bracket").expect("get");
        assert!(record.source.contains("(define-component bracket"));
        assert_eq!(record.header.name, "bracket");
    }

    #[test]
    fn search_is_header_only_and_survives_missing_body() {
        let resolver = temp_resolver("headeronly");
        let extracted = sample_extracted("lonely");
        let dir = save_extracted_component(&resolver, &extracted).expect("save");
        fs::remove_file(dir.join("component.ecky")).expect("drop body");

        let results = search_extracted_components(&resolver, "lonely", 10).expect("search");
        assert_eq!(results.len(), 1, "search must not depend on bodies");

        let err = read_extracted_component(&resolver, "lonely").expect_err("get needs body");
        assert!(err.message.contains("lonely"), "{}", err.message);
    }

    #[test]
    fn search_filters_by_query_and_respects_limit() {
        let resolver = temp_resolver("filter");
        save_extracted_component(&resolver, &sample_extracted("alpha-bracket")).expect("save");
        save_extracted_component(&resolver, &sample_extracted("beta-hinge")).expect("save");

        let hits = search_extracted_components(&resolver, "beta", 10).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "beta-hinge");

        let all = search_extracted_components(&resolver, "", 20).expect("search all");
        assert!(all.iter().any(|hit| hit.name == "alpha-bracket"));
        assert!(all.iter().any(|hit| hit.name == "beta-hinge"));
        assert!(all.iter().any(|hit| hit.name == "hex-bolt"));

        let limited = search_extracted_components(&resolver, "", 1).expect("limited");
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn compact_search_exposes_port_ids_and_types_without_body_source() {
        let resolver = temp_resolver("ported-search");
        save_extracted_component(&resolver, &sample_extracted_with_port("ported-bracket"))
            .expect("save");

        let hits = search_extracted_components(&resolver, "Ported", 10).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].ports,
            vec![ExtractedComponentPortSummary {
                port_id: "mount".to_string(),
                type_id: "mechanical.mount.v1".to_string(),
            }]
        );
        let json = serde_json::to_value(&hits[0]).expect("serialize compact result");
        assert!(json.get("source").is_none(), "body source leaked: {json}");
        assert_eq!(json["ports"][0]["portId"], "mount");
    }

    #[test]
    fn unknown_component_get_is_deterministic() {
        let resolver = temp_resolver("missing");
        let err = read_extracted_component(&resolver, "ghost").expect_err("missing");
        assert!(err.message.contains("ghost"), "{}", err.message);
    }

    #[test]
    fn unsafe_component_names_are_rejected() {
        let resolver = temp_resolver("unsafe");
        let err = read_extracted_component(&resolver, "../escape").expect_err("unsafe");
        assert!(err.message.contains("not a safe"), "{}", err.message);
    }

    #[test]
    fn shipped_stdlib_search_and_get_expose_pinned_source_without_disk_seed() {
        let resolver = temp_resolver("builtin");
        let hits = search_extracted_components(&resolver, "gt2", 10).expect("search stdlib");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "gt2-pulley");
        assert_eq!(hits[0].version.as_deref(), Some("1.0.0"));

        let record = read_extracted_component(&resolver, "gt2-pulley").expect("get stdlib");
        assert_eq!(record.version.as_deref(), Some("1.0.0"));
        assert!(record.source.contains("(define-component gt2-pulley"));
        assert!(!record.source.contains("(import-component"));
    }
}

#[cfg(test)]
mod package_payload_store_tests {
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

    fn temp_resolver(label: &str) -> TestResolver {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        TestResolver {
            root: std::env::temp_dir().join(format!("ecky-payload-store-{label}-{nonce}")),
        }
    }

    /// Build an in-memory zip from `(name, bytes)` entries.
    fn payload_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = FileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o644);
        for (name, bytes) in entries {
            writer
                .start_file(*name, options)
                .expect("start payload file");
            writer.write_all(bytes).expect("write payload bytes");
        }
        writer.finish().expect("finish payload zip").into_inner()
    }

    fn manifest_bytes(package_id: &str, version: &str, component_id: &str) -> Vec<u8> {
        let json = format!(
            r#"{{"schemaVersion":1,"packageId":"{package_id}","version":"{version}","displayName":"{package_id}","visibility":"source","components":[{{"componentId":"{component_id}","version":"1.0.0","displayName":"{component_id}"}}]}}"#
        );
        json.into_bytes()
    }

    fn sample_payload(package_id: &str, version: &str, component_id: &str, body: &str) -> Vec<u8> {
        payload_zip(&[
            (
                COMPONENT_PACKAGE_FILE_NAME,
                &manifest_bytes(package_id, version, component_id),
            ),
            (
                &format!("components/{component_id}/source.ecky"),
                body.as_bytes(),
            ),
        ])
    }

    fn full_archive(package_id: &str, version: &str, payload_bytes: &[u8]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ecky-payload-archive-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("archive dir");
        let archive_path = dir.join("pkg.ecky");
        let mut writer = ZipWriter::new(fs::File::create(&archive_path).expect("create archive"));
        let options = FileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o644);
        let header_json = format!(
            r#"{{"schemaVersion":1,"packageId":"{package_id}","version":"{version}","displayName":"{package_id}","visibility":"source","components":[{{"componentId":"cage","version":"1.0.0","displayName":"cage"}}]}}"#
        );
        writer
            .start_file(COMPONENT_PACKAGE_HEADER_FILE_NAME, options)
            .expect("header entry");
        writer
            .write_all(header_json.as_bytes())
            .expect("header bytes");
        writer
            .start_file(COMPONENT_PACKAGE_PAYLOAD_FILE_NAME, options)
            .expect("payload entry");
        writer
            .write_all(general_purpose::STANDARD.encode(payload_bytes).as_bytes())
            .expect("payload bytes");
        writer.finish().expect("finish archive");
        archive_path
    }

    #[test]
    fn digest_includes_manifest_and_source_and_excludes_envelope_files() {
        // The inner payload contains ecky-package.json + a source file, plus
        // outer-envelope names that must be excluded from the digest set.
        let payload = payload_zip(&[
            (COMPONENT_PACKAGE_FILE_NAME, b"{\"schemaVersion\":1}"),
            (
                "components/cage/source.ecky",
                b"(model (part body (box 1 1 1)))",
            ),
            (COMPONENT_PACKAGE_HEADER_FILE_NAME, b"envelope-header"),
            (COMPONENT_PACKAGE_PAYLOAD_FILE_NAME, b"envelope-payload"),
        ]);
        let validated = validate_payload_archive(&payload).expect("valid payload");
        // Envelope files excluded; only manifest + source remain.
        assert_eq!(validated.entries.len(), 2);
        let paths: Vec<&str> = validated.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["components/cage/source.ecky", "ecky-package.json"]);
    }

    #[test]
    fn digest_is_deterministic_and_independent_of_entry_order() {
        let body = "(model (part body (box 2 2 2)))";
        let a = sample_payload("bike.kit", "1.2.0", "cage", body);
        // Same logical content, reversed entry order.
        let b = payload_zip(&[
            ("components/cage/source.ecky", body.as_bytes()),
            (
                COMPONENT_PACKAGE_FILE_NAME,
                &manifest_bytes("bike.kit", "1.2.0", "cage"),
            ),
        ]);
        let (digest_a, inv_a) =
            compute_package_payload_digest(&validate_payload_archive(&a).expect("valid a"));
        let (digest_b, inv_b) =
            compute_package_payload_digest(&validate_payload_archive(&b).expect("valid b"));
        assert_eq!(digest_a, digest_b, "order-independent digest");
        assert!(digest_a.starts_with("sha256:"));
        assert_eq!(inv_a.len(), 2);
        assert_eq!(inv_a, inv_b, "order-independent inventory");
    }

    #[test]
    fn digest_changes_when_content_changes() {
        let base = sample_payload(
            "bike.kit",
            "1.2.0",
            "cage",
            "(model (part body (box 1 1 1)))",
        );
        let changed = sample_payload(
            "bike.kit",
            "1.2.0",
            "cage",
            "(model (part body (box 9 9 9)))",
        );
        let d_base =
            compute_package_payload_digest(&validate_payload_archive(&base).expect("valid")).0;
        let d_changed =
            compute_package_payload_digest(&validate_payload_archive(&changed).expect("valid")).0;
        assert_ne!(d_base, d_changed);
    }

    #[test]
    fn validation_rejects_reserved_integrity_path() {
        let payload = payload_zip(&[
            (COMPONENT_PACKAGE_FILE_NAME, b"{}"),
            (PACKAGE_INTEGRITY_FILE_NAME, b"{}"),
        ]);
        let err = validate_payload_archive(&payload).expect_err("reserved rejected");
        assert!(err.message.contains("reserved"), "{}", err.message);
        assert!(
            err.message.contains("ecky-integrity.json"),
            "{}",
            err.message
        );
    }

    #[test]
    fn validation_rejects_duplicate_normalized_paths() {
        let payload = payload_zip(&[
            (COMPONENT_PACKAGE_FILE_NAME, b"{}"),
            ("src/a.txt", b"x"),
            ("src\\a.txt", b"y"),
        ]);
        let err = validate_payload_archive(&payload).expect_err("duplicate rejected");
        assert!(err.message.contains("duplicate"), "{}", err.message);
    }

    #[test]
    fn validation_rejects_traversal_entries() {
        let payload = payload_zip(&[(COMPONENT_PACKAGE_FILE_NAME, b"{}"), ("../evil.txt", b"x")]);
        let err = validate_payload_archive(&payload).expect_err("traversal rejected");
        assert!(err.message.contains("safe to extract"), "{}", err.message);
    }

    #[test]
    fn publish_writes_payload_files_and_inventory_sidecar() {
        let resolver = temp_resolver("publish");
        let payload = sample_payload(
            "bike.kit",
            "1.2.0",
            "cage",
            "(model (part body (box 1 1 1)))",
        );
        let validated = validate_payload_archive(&payload).expect("valid");
        let (digest, inventory) = compute_package_payload_digest(&validated);
        let store_dir =
            publish_validated_payload(&resolver, &validated, &digest, inventory.clone())
                .expect("publish");

        assert!(store_dir.join(COMPONENT_PACKAGE_FILE_NAME).is_file());
        assert!(store_dir.join("components/cage/source.ecky").is_file());
        // Sidecar always exists after publish.
        assert!(store_dir.join(PACKAGE_INTEGRITY_FILE_NAME).is_file());

        let read_back = read_payload_inventory(&store_dir).expect("read inventory");
        assert_eq!(read_back.package_digest, digest);
        assert_eq!(read_back.entries, inventory);
        // The sidecar itself is never a digest input.
        assert!(
            !read_back
                .entries
                .iter()
                .any(|e| e.path == PACKAGE_INTEGRITY_FILE_NAME),
            "integrity sidecar must not be in inventory"
        );
    }

    #[test]
    fn publish_is_idempotent_for_same_digest() {
        let resolver = temp_resolver("idempotent");
        let payload = sample_payload("bike.kit", "1.2.0", "cage", "(model)");
        let validated = validate_payload_archive(&payload).expect("valid");
        let (digest, inventory) = compute_package_payload_digest(&validated);
        let first = publish_validated_payload(&resolver, &validated, &digest, inventory.clone())
            .expect("first publish");
        // Mutate a payload file on disk, then republish: idempotent path must
        // NOT rewrite existing content.
        let target = first.join(COMPONENT_PACKAGE_FILE_NAME);
        fs::write(&target, b"tampered").expect("tamper");
        publish_validated_payload(&resolver, &validated, &digest, inventory).expect("republish");
        let after = fs::read_to_string(&target).expect("read after");
        assert_eq!(after, "tampered", "idempotent publish must not rewrite");
    }

    #[test]
    fn install_to_store_publishes_and_indexes_coordinate() {
        let resolver = temp_resolver("install");
        let payload = sample_payload(
            "bike.kit",
            "1.2.0",
            "cage",
            "(model (part body (box 1 1 1)))",
        );
        let archive = full_archive("bike.kit", "1.2.0", &payload);

        let installed = install_component_package_to_store(&resolver, &archive).expect("install");
        assert_eq!(installed.package_id, "bike.kit");
        assert_eq!(installed.version, "1.2.0");
        assert!(installed.package_digest.starts_with("sha256:"));
        assert!(
            installed
                .store_dir
                .join(PACKAGE_INTEGRITY_FILE_NAME)
                .is_file()
        );

        let indexed = read_coordinate_index(&resolver, "bike.kit", "1.2.0").expect("read index");
        let indexed = indexed.expect("indexed");
        assert_eq!(indexed.package_digest, installed.package_digest);
    }

    #[test]
    fn install_to_store_idempotent_for_same_digest() {
        let resolver = temp_resolver("install-idem");
        let payload = sample_payload("bike.kit", "1.2.0", "cage", "(model)");
        let archive = full_archive("bike.kit", "1.2.0", &payload);
        let first = install_component_package_to_store(&resolver, &archive).expect("first");
        let second = install_component_package_to_store(&resolver, &archive).expect("second");
        assert_eq!(first.package_digest, second.package_digest);
        assert_eq!(first.store_dir, second.store_dir);
    }

    #[test]
    fn install_to_store_rejects_different_digest_at_same_coordinate() {
        let resolver = temp_resolver("install-mutation");
        let payload_v1 = sample_payload(
            "bike.kit",
            "1.2.0",
            "cage",
            "(model (part body (box 1 1 1)))",
        );
        let archive_v1 = full_archive("bike.kit", "1.2.0", &payload_v1);
        let first = install_component_package_to_store(&resolver, &archive_v1).expect("first");
        let payload_v2 = sample_payload(
            "bike.kit",
            "1.2.0",
            "cage",
            "(model (part body (box 9 9 9)))",
        );
        let archive_v2 = full_archive("bike.kit", "1.2.0", &payload_v2);
        let err = install_component_package_to_store(&resolver, &archive_v2)
            .expect_err("mutation rejected");
        assert!(
            err.message.contains("refusing to overwrite"),
            "{}",
            err.message
        );
        // Existing content intact.
        let indexed = read_coordinate_index(&resolver, "bike.kit", "1.2.0").expect("read index");
        assert_eq!(
            indexed.expect("still indexed").package_digest,
            first.package_digest
        );
    }

    #[test]
    fn cross_model_cas_dedup_reuses_same_coordinate_store_dir() {
        let resolver = temp_resolver("cas-dedup");
        let body = "(model (part body (box 4 4 4)))";
        // Multiple model versions using one exact package coordinate reuse
        // the same global payload directory.
        let payload = sample_payload("bike.kit", "1.2.0", "cage", body);
        let archive_a = full_archive("bike.kit", "1.2.0", &payload);
        let archive_b = full_archive("bike.kit", "1.2.0", &payload);

        let installed_a = install_component_package_to_store(&resolver, &archive_a).expect("a");
        let installed_b = install_component_package_to_store(&resolver, &archive_b).expect("b");
        assert_eq!(installed_a.package_digest, installed_b.package_digest);
        assert_eq!(installed_a.store_dir, installed_b.store_dir);
    }

    #[test]
    fn remove_coordinate_index_blocks_unlocked_resolution() {
        let resolver = temp_resolver("uninstall");
        let payload = sample_payload("bike.kit", "1.2.0", "cage", "(model)");
        let archive = full_archive("bike.kit", "1.2.0", &payload);
        let installed = install_component_package_to_store(&resolver, &archive).expect("install");

        let removed = remove_coordinate_index(&resolver, "bike.kit", "1.2.0").expect("remove");
        assert!(removed);
        // The payload store survives uninstall (committed locks are GC roots).
        assert!(
            installed
                .store_dir
                .join(PACKAGE_INTEGRITY_FILE_NAME)
                .is_file()
        );
        // Unlocked discovery no longer finds the coordinate.
        let after = read_coordinate_index(&resolver, "bike.kit", "1.2.0").expect("read");
        assert!(after.is_none());
        // Second remove reports nothing to do.
        let again = remove_coordinate_index(&resolver, "bike.kit", "1.2.0").expect("remove again");
        assert!(!again);
    }

    #[test]
    fn uninstall_does_not_allow_exact_coordinate_to_be_rebound_to_new_payload() {
        let resolver = temp_resolver("immutable-after-uninstall");
        let first = full_archive(
            "bike.immutable",
            "1.0.0",
            &sample_payload(
                "bike.immutable",
                "1.0.0",
                "cage",
                "(define-component cage () (box 1 1 1))",
            ),
        );
        let installed =
            install_component_package_to_store(&resolver, &first).expect("first install");
        remove_coordinate_index(&resolver, "bike.immutable", "1.0.0").expect("uninstall");

        let changed = full_archive(
            "bike.immutable",
            "1.0.0",
            &sample_payload(
                "bike.immutable",
                "1.0.0",
                "cage",
                "(define-component cage () (box 9 9 9))",
            ),
        );
        let error = install_component_package_to_store(&resolver, &changed)
            .expect_err("exact coordinate remains immutable after uninstall");
        assert!(error.message.contains("refusing to overwrite"), "{error}");
        assert!(installed.store_dir.exists());
        assert!(
            read_coordinate_index(&resolver, "bike.immutable", "1.0.0")
                .expect("discovery index")
                .is_none(),
            "failed mutation must not reinstall discovery metadata"
        );
    }

    #[test]
    fn installation_recovers_an_incomplete_store_without_publishing_an_incomplete_index() {
        let resolver = temp_resolver("atomic-recovery");
        let payload = sample_payload(
            "bike.kit",
            "1.2.0",
            "cage",
            "(model (part body (box 2 2 2)))",
        );
        let validated = validate_payload_archive(&payload).expect("valid payload");
        let (digest, _) = compute_package_payload_digest(&validated);
        let incomplete_dir = payload_store_dir(&resolver, &digest).expect("store path");
        fs::create_dir_all(&incomplete_dir).expect("simulate interrupted publish");
        fs::write(incomplete_dir.join(COMPONENT_PACKAGE_FILE_NAME), b"partial")
            .expect("partial file");

        let archive = full_archive("bike.kit", "1.2.0", &payload);
        let installed =
            install_component_package_to_store(&resolver, &archive).expect("recover install");

        assert_eq!(installed.package_digest, digest);
        assert_eq!(
            fs::read(incomplete_dir.join(COMPONENT_PACKAGE_FILE_NAME)).expect("manifest"),
            validated
                .entries
                .iter()
                .find(|entry| entry.path == COMPONENT_PACKAGE_FILE_NAME)
                .expect("manifest entry")
                .content
        );
        assert!(incomplete_dir.join(PACKAGE_INTEGRITY_FILE_NAME).is_file());
        assert_eq!(
            read_coordinate_index(&resolver, "bike.kit", "1.2.0")
                .expect("index")
                .expect("published index")
                .package_digest,
            digest
        );
    }

    #[test]
    fn gc_retains_coordinate_explicit_and_in_flight_roots_and_deletes_orphans() {
        struct Pins(std::collections::BTreeSet<String>);
        impl ComponentStoreInFlightPins for Pins {
            fn pinned_package_digests(&self) -> std::collections::BTreeSet<String> {
                self.0.clone()
            }
        }

        let resolver = temp_resolver("gc");
        let indexed = install_component_package_to_store(
            &resolver,
            &full_archive(
                "bike.indexed",
                "1.0.0",
                &sample_payload("bike.indexed", "1.0.0", "cage", "(model 1)"),
            ),
        )
        .expect("indexed install");
        let explicit = install_component_package_to_store(
            &resolver,
            &full_archive(
                "bike.explicit",
                "1.0.0",
                &sample_payload("bike.explicit", "1.0.0", "cage", "(model 2)"),
            ),
        )
        .expect("explicit install");
        let in_flight = install_component_package_to_store(
            &resolver,
            &full_archive(
                "bike.flight",
                "1.0.0",
                &sample_payload("bike.flight", "1.0.0", "cage", "(model 3)"),
            ),
        )
        .expect("in-flight install");
        let orphan = install_component_package_to_store(
            &resolver,
            &full_archive(
                "bike.orphan",
                "1.0.0",
                &sample_payload("bike.orphan", "1.0.0", "cage", "(model 4)"),
            ),
        )
        .expect("orphan install");
        for package_id in ["bike.explicit", "bike.flight", "bike.orphan"] {
            remove_coordinate_index(&resolver, package_id, "1.0.0").expect("uninstall");
        }

        let report = garbage_collect_component_package_store(
            &resolver,
            &ComponentStoreGcRequest {
                explicit_root_digests: [explicit.package_digest.clone()].into_iter().collect(),
                grace_period: std::time::Duration::ZERO,
            },
            &Pins([in_flight.package_digest.clone()].into_iter().collect()),
        )
        .expect("gc");

        assert!(indexed.store_dir.exists(), "coordinate index is a root");
        assert!(
            explicit.store_dir.exists(),
            "explicit lock root is retained"
        );
        assert!(in_flight.store_dir.exists(), "in-flight root is retained");
        assert!(!orphan.store_dir.exists(), "unreachable store is deleted");
        assert_eq!(report.deleted_package_digests, vec![orphan.package_digest]);
    }

    #[test]
    fn gc_rechecks_in_flight_roots_while_holding_the_mutation_lock() {
        struct AppearsDuringGc {
            calls: std::sync::atomic::AtomicUsize,
            digest: String,
        }
        impl ComponentStoreInFlightPins for AppearsDuringGc {
            fn pinned_package_digests(&self) -> std::collections::BTreeSet<String> {
                if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Default::default()
                } else {
                    [self.digest.clone()].into_iter().collect()
                }
            }
        }

        let resolver = temp_resolver("gc-recheck");
        let installed = install_component_package_to_store(
            &resolver,
            &full_archive(
                "bike.recheck",
                "1.0.0",
                &sample_payload("bike.recheck", "1.0.0", "cage", "(model recheck)"),
            ),
        )
        .expect("install");
        remove_coordinate_index(&resolver, "bike.recheck", "1.0.0").expect("uninstall");
        let pins = AppearsDuringGc {
            calls: std::sync::atomic::AtomicUsize::new(0),
            digest: installed.package_digest.clone(),
        };

        let report = garbage_collect_component_package_store(
            &resolver,
            &ComponentStoreGcRequest {
                explicit_root_digests: Default::default(),
                grace_period: std::time::Duration::ZERO,
            },
            &pins,
        )
        .expect("gc");

        assert_eq!(pins.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(
            installed.store_dir.exists(),
            "second root check retains payload"
        );
        assert_eq!(
            report.retained_package_digests,
            vec![installed.package_digest]
        );
    }

    #[test]
    fn runtime_pin_guard_roots_payload_until_last_overlapping_owner_drops() {
        let digest = format!("sha256:{}", "c".repeat(64));
        let pins = RuntimeComponentStorePins;
        let first = pin_component_store_payloads([digest.clone()]);
        let second = pin_component_store_payloads([digest.clone(), digest.clone()]);
        assert_eq!(
            pins.pinned_package_digests(),
            [digest.clone()].into_iter().collect()
        );
        drop(first);
        assert_eq!(
            pins.pinned_package_digests(),
            [digest.clone()].into_iter().collect()
        );
        drop(second);
        assert!(!pins.pinned_package_digests().contains(&digest));
    }

    #[test]
    fn automatic_component_capture_keeps_immutable_thread_scoped_revisions() {
        let resolver = temp_resolver("auto-capture");
        let first_source = "(define wall-width 4) (define-component bracket ((number width 12)) (ports (port mount :type \"bracket.mount.v1\" :frame (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1)))) (box width wall-width 2)) (model (part one (bracket :width 20)))";
        let unchanged_source = "(define wall-width 4) ; ignored\n(define-component bracket ((number width 12)) (ports (port mount :type \"bracket.mount.v1\" :frame (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1)))) (box width wall-width 2)) (model (part one (bracket :width 44)))";
        let changed_source = first_source.replace("wall-width 4", "wall-width 5");
        capture_defined_components(&resolver, "thread-one", "message-one", first_source).unwrap();
        assert!(
            search_extracted_components(&resolver, "mount", 50)
                .unwrap()
                .iter()
                .any(|component| component.name == "bracket"),
            "component search should match authored port ids"
        );
        assert!(
            search_extracted_components(&resolver, "bracket.mount.v1", 50)
                .unwrap()
                .iter()
                .any(|component| component.name == "bracket"),
            "component search should match authored port type ids"
        );
        let first = search_extracted_components(&resolver, "bracket", 50)
            .unwrap()
            .into_iter()
            .find(|component| component.origin == "local")
            .unwrap();
        let component_id = first.component_id.clone().unwrap();
        let first_digest = first.revision_digest.clone().unwrap();
        let latest_path = extracted_component_library_root(&resolver)
            .unwrap()
            .join(AUTO_COMPONENT_LIBRARY_DIR_NAME)
            .join(&component_id)
            .join(AUTO_COMPONENT_LATEST_FILE_NAME);
        let pointer_before = fs::read(&latest_path).unwrap();

        capture_defined_components(&resolver, "thread-one", "message-two", unchanged_source)
            .unwrap();
        assert_eq!(pointer_before, fs::read(&latest_path).unwrap());
        let unchanged = read_component_by_id(&resolver, &component_id, None).unwrap();
        assert_eq!(
            unchanged.revision_digest.as_deref(),
            Some(first_digest.as_str())
        );

        capture_defined_components(&resolver, "thread-one", "message-three", &changed_source)
            .unwrap();
        let current = read_component_by_id(&resolver, &component_id, None).unwrap();
        assert_ne!(
            current.revision_digest.as_deref(),
            Some(first_digest.as_str())
        );
        let pinned = read_component_by_id(&resolver, &component_id, Some(&first_digest)).unwrap();
        assert!(pinned.source.contains("wall-width 4"));
        assert!(current.source.contains("wall-width 5"));

        capture_defined_components(&resolver, "thread-two", "message-four", first_source).unwrap();
        let identities = search_extracted_components(&resolver, "bracket", 50)
            .unwrap()
            .into_iter()
            .filter(|component| component.origin == "local")
            .filter_map(|component| component.component_id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(identities.len(), 2);
        assert!(identities.contains(&component_id));
        assert!(!read_component_by_id(&resolver, &component_id, Some("sha256:bad")).is_ok());
    }
}
