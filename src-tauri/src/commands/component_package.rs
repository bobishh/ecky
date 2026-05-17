use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use csgrs::float_types::parry3d::na::{Matrix3, Vector3};
use tauri::{AppHandle, State};

use crate::component_package_runtime;
use crate::models::{
    validate_component_package, AppError, AppResult, AppState, ArtifactBundle,
    ArtifactBundleComponentPackageRequest, AssemblyDefinition, AssemblyMate, AssemblyOperation,
    AssemblyOutputMode, ComponentDefinition, ComponentInterfaceValue, ComponentPackage,
    ComponentPackageHeader, ComponentPort, DesignParams, ExportPartInput, GeometryBackend,
    InstalledAssemblyComponentControls, InstalledAssemblyComponentSource,
    InstalledAssemblyControls, InstalledAssemblyMateResult, InstalledAssemblyOperationResult,
    InstalledAssemblyOutputRuntime, InstalledAssemblyRuntime, InstalledAssemblySource,
    InstalledComponentControls, InstalledComponentPackage, InstalledComponentRuntime,
    InstalledComponentSource, MacroDialect, ModelManifest, OperationKind, PathResolver, PortFrame,
    PortReference, SourceLanguage,
};
use crate::topology_target_ids::{is_stable_topology_target_id, portable_topology_target_id};

const FRAME_EPSILON: f64 = 1.0e-6;

#[derive(Clone, Debug)]
struct RigidFrame {
    origin: Vector3<f64>,
    basis: Matrix3<f64>,
}

#[derive(Clone, Debug)]
struct AssemblySolveResult {
    placement_frames: BTreeMap<String, PortFrame>,
    mate_results: Vec<InstalledAssemblyMateResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AssemblyOutputPlan {
    mode: AssemblyOutputPlanMode,
    fuse_group_by_instance: BTreeMap<String, String>,
    cut_group_by_instance: BTreeMap<String, CutGroupMembership>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssemblyOutputPlanMode {
    None,
    Joined,
    FusedSolid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CutGroupMembership {
    group_id: String,
    role: CutGroupRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CutGroupRole {
    Base,
    Tool,
}

impl RigidFrame {
    fn identity() -> Self {
        Self {
            origin: Vector3::new(0.0, 0.0, 0.0),
            basis: Matrix3::identity(),
        }
    }

    fn from_port_frame(frame: &PortFrame, label: &str) -> Result<Self, String> {
        let x_axis = normalize_frame_axis(frame.x_axis, label, "xAxis")?;
        let y_axis = normalize_frame_axis(frame.y_axis, label, "yAxis")?;
        let z_axis = normalize_frame_axis(frame.z_axis, label, "zAxis")?;
        if x_axis.dot(&y_axis).abs() > FRAME_EPSILON
            || x_axis.dot(&z_axis).abs() > FRAME_EPSILON
            || y_axis.dot(&z_axis).abs() > FRAME_EPSILON
        {
            return Err(format!("{label} frame axes must be orthogonal."));
        }
        if x_axis.cross(&y_axis).dot(&z_axis) <= FRAME_EPSILON {
            return Err(format!(
                "{label} frame axes must form a right-handed basis."
            ));
        }
        Ok(Self {
            origin: Vector3::new(frame.origin[0], frame.origin[1], frame.origin[2]),
            basis: Matrix3::from_columns(&[x_axis, y_axis, z_axis]),
        })
    }

    fn compose(&self, other: &Self) -> Self {
        Self {
            origin: self.origin + self.basis * other.origin,
            basis: self.basis * other.basis,
        }
    }

    fn inverse(&self) -> Self {
        let basis = self.basis.transpose();
        Self {
            origin: -(basis * self.origin),
            basis,
        }
    }

    fn approx_eq(&self, other: &Self) -> bool {
        (self.origin - other.origin).norm() <= FRAME_EPSILON
            && (self.basis - other.basis).norm() <= FRAME_EPSILON
    }

    fn into_port_frame(self) -> PortFrame {
        let x_axis = self.basis.column(0);
        let y_axis = self.basis.column(1);
        let z_axis = self.basis.column(2);
        PortFrame {
            origin: [self.origin.x, self.origin.y, self.origin.z],
            x_axis: [x_axis[0], x_axis[1], x_axis[2]],
            y_axis: [y_axis[0], y_axis[1], y_axis[2]],
            z_axis: [z_axis[0], z_axis[1], z_axis[2]],
        }
    }
}

fn normalize_frame_axis(
    axis: [f64; 3],
    label: &str,
    axis_name: &str,
) -> Result<Vector3<f64>, String> {
    let axis = Vector3::new(axis[0], axis[1], axis[2]);
    let norm = axis.norm();
    if norm <= FRAME_EPSILON {
        return Err(format!("{label} frame {axis_name} must be non-zero."));
    }
    Ok(axis / norm)
}

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

#[derive(Clone, Debug)]
struct PackagedComponentSource {
    source_ref: String,
    copy_from: PathBuf,
    source_language: Option<SourceLanguage>,
    geometry_backend: Option<GeometryBackend>,
    macro_dialect: Option<MacroDialect>,
}

#[tauri::command]
#[specta::specta]
pub async fn write_artifact_bundle_component_package_project(
    project_dir: String,
    request: ArtifactBundleComponentPackageRequest,
) -> AppResult<ComponentPackage> {
    write_artifact_bundle_component_package_project_impl(Path::new(&project_dir), request)
}

fn write_artifact_bundle_component_package_project_impl(
    project_dir: &Path,
    request: ArtifactBundleComponentPackageRequest,
) -> AppResult<ComponentPackage> {
    let packaged_source = resolve_packaged_component_source(&request)?;
    let derived_component_contract =
        resolve_packaged_component_contract(&request, &packaged_source)?;
    let ports = normalize_packaged_component_ports(
        &request.ports,
        &request.artifact_bundle,
        should_normalize_packaged_topology_ids(&packaged_source, &request.artifact_bundle),
    )?;
    let runtime_target_ids = artifact_bundle_target_ids(&request.artifact_bundle)?;
    let known_port_type_ids = request
        .port_types
        .iter()
        .map(|port_type| port_type.type_id.as_str())
        .collect::<HashSet<_>>();
    for port in &request.ports {
        if !known_port_type_ids.contains(port.type_id.as_str()) {
            return Err(AppError::validation(format!(
                "Artifact bundle component port '{}' references unknown port typeId '{}'.",
                port.port_id, port.type_id
            )));
        }
        for target_id in &port.target_ids {
            if !runtime_target_ids.contains(target_id) {
                return Err(AppError::validation(format!(
                    "Artifact bundle component port '{}' references unknown runtime targetId '{}'.",
                    port.port_id, target_id
                )));
            }
        }
    }

    let package = ComponentPackage {
        schema_version: crate::models::COMPONENT_PACKAGE_SCHEMA_VERSION,
        package_id: request.package_id,
        version: request.version,
        display_name: request.display_name,
        visibility: crate::models::PackageVisibility::Source,
        tags: request.tags,
        port_types: request.port_types,
        mate_types: Vec::new(),
        components: vec![ComponentDefinition {
            component_id: request.component_id,
            version: request.component_version,
            display_name: request.component_display_name,
            source_ref: Some(packaged_source.source_ref.clone()),
            source_language: packaged_source.source_language,
            geometry_backend: packaged_source.geometry_backend,
            macro_dialect: packaged_source.macro_dialect,
            sketches: Vec::new(),
            keepouts: Vec::new(),
            fusion_zones: Vec::new(),
            params: derived_component_contract.params,
            ui_spec: derived_component_contract.ui_spec,
            initial_params: derived_component_contract.initial_params,
            ports,
        }],
        assemblies: Vec::new(),
    };
    validate_component_package(&package)?;

    let target_path = project_dir.join(
        component_package_runtime::safe_archive_path(&packaged_source.source_ref).map_err(|_| {
            AppError::validation(format!(
                "Artifact bundle component sourceRef '{}' must be a safe package-local relative path.",
                packaged_source.source_ref
            ))
        })?,
    );
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::persistence(format!(
                "Failed to create artifact bundle component source directory '{}': {}",
                parent.display(),
                err
            ))
        })?;
    }
    if packaged_source.copy_from != target_path {
        fs::copy(&packaged_source.copy_from, &target_path).map_err(|err| {
            AppError::persistence(format!(
                "Failed to copy component package source from '{}' to '{}': {}",
                packaged_source.copy_from.display(),
                target_path.display(),
                err
            ))
        })?;
    }
    component_package_runtime::write_component_package_manifest(project_dir, &package)?;
    Ok(package)
}

fn normalize_packaged_component_ports(
    ports: &[ComponentPort],
    artifact_bundle: &ArtifactBundle,
    prefer_stable_topology_ids: bool,
) -> AppResult<Vec<ComponentPort>> {
    if !prefer_stable_topology_ids {
        return Ok(ports.to_vec());
    }

    let manifest = read_model_manifest_from_path(Path::new(&artifact_bundle.manifest_path))?;
    let mut preferred_target_ids = manifest
        .selection_targets
        .iter()
        .flat_map(|target| {
            let Some(preferred) = preferred_packaged_target_id(target) else {
                return Vec::new();
            };
            target
                .target_id
                .iter()
                .chain(target.durable_target_id.iter())
                .chain(target.canonical_target_id.iter())
                .chain(target.alias_ids.iter())
                .map(|target_id| (target_id.clone(), preferred.clone()))
                .collect::<Vec<_>>()
        })
        .chain(artifact_bundle.edge_targets.iter().flat_map(|target| {
            target
                .durable_target_id
                .iter()
                .map(|durable_target_id| (durable_target_id.clone(), target.target_id.clone()))
                .chain(
                    target
                        .canonical_target_id
                        .iter()
                        .map(|canonical_target_id| {
                            (canonical_target_id.clone(), target.target_id.clone())
                        })
                        .chain(
                            target
                                .alias_ids
                                .iter()
                                .map(|alias_id| (alias_id.clone(), target.target_id.clone())),
                        ),
                )
                .collect::<Vec<_>>()
        }))
        .chain(artifact_bundle.face_targets.iter().flat_map(|target| {
            target
                .durable_target_id
                .iter()
                .map(|durable_target_id| (durable_target_id.clone(), target.target_id.clone()))
                .chain(
                    target
                        .canonical_target_id
                        .iter()
                        .map(|canonical_target_id| {
                            (canonical_target_id.clone(), target.target_id.clone())
                        })
                        .chain(
                            target
                                .alias_ids
                                .iter()
                                .map(|alias_id| (alias_id.clone(), target.target_id.clone())),
                        ),
                )
                .collect::<Vec<_>>()
        }))
        .collect::<HashMap<_, _>>();

    for target in &manifest.selection_targets {
        let Some(preferred) = preferred_packaged_target_id(target) else {
            continue;
        };
        let manifest_portable_ids = target
            .target_id
            .iter()
            .chain(target.durable_target_id.iter())
            .chain(target.canonical_target_id.iter())
            .chain(target.alias_ids.iter())
            .filter_map(|target_id| portable_topology_target_id(target_id))
            .collect::<HashSet<_>>();
        if manifest_portable_ids.is_empty() {
            continue;
        }
        match target.kind {
            crate::models::SelectionTargetKind::Edge => {
                for edge_target in &artifact_bundle.edge_targets {
                    if portable_topology_target_id(&edge_target.target_id)
                        .is_some_and(|portable| manifest_portable_ids.contains(&portable))
                    {
                        preferred_target_ids
                            .insert(edge_target.target_id.clone(), preferred.clone());
                    }
                }
            }
            crate::models::SelectionTargetKind::Face => {
                for face_target in &artifact_bundle.face_targets {
                    if portable_topology_target_id(&face_target.target_id)
                        .is_some_and(|portable| manifest_portable_ids.contains(&portable))
                    {
                        preferred_target_ids
                            .insert(face_target.target_id.clone(), preferred.clone());
                    }
                }
            }
            _ => {}
        }
    }

    Ok(ports
        .iter()
        .cloned()
        .map(|mut port| {
            port.target_ids = port
                .target_ids
                .into_iter()
                .map(|target_id| {
                    preferred_target_ids
                        .get(&target_id)
                        .cloned()
                        .unwrap_or(target_id)
                })
                .collect();
            port
        })
        .collect())
}

fn should_normalize_packaged_topology_ids(
    packaged_source: &PackagedComponentSource,
    artifact_bundle: &ArtifactBundle,
) -> bool {
    let source_ref = packaged_source.source_ref.to_ascii_lowercase();
    source_ref.ends_with(".step")
        || source_ref.ends_with(".stp")
        || artifact_bundle.geometry_backend == GeometryBackend::EckyRust
        || artifact_bundle
            .edge_targets
            .iter()
            .any(|target| target.durable_target_id.is_some())
        || artifact_bundle
            .face_targets
            .iter()
            .any(|target| target.durable_target_id.is_some())
}

fn preferred_packaged_target_id(target: &crate::models::SelectionTarget) -> Option<String> {
    target
        .durable_target_id
        .clone()
        .or_else(|| {
            target
                .alias_ids
                .iter()
                .filter(|alias_id| is_stable_topology_target_id(alias_id))
                .min_by_key(|alias_id| alias_id.len())
                .cloned()
        })
        .or_else(|| target.target_id.clone())
}

fn read_model_manifest_from_path(path: &Path) -> AppResult<ModelManifest> {
    let data = fs::read_to_string(path).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read model manifest '{}': {}",
            path.display(),
            err
        ))
    })?;
    serde_json::from_str(&data).map_err(|err| {
        AppError::validation(format!(
            "Failed to parse model manifest '{}': {}",
            path.display(),
            err
        ))
    })
}

fn resolve_packaged_component_source(
    request: &ArtifactBundleComponentPackageRequest,
) -> AppResult<PackagedComponentSource> {
    let requested_source_ref = request
        .source_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested_extension = requested_source_ref
        .and_then(|value| Path::new(value).extension().and_then(|ext| ext.to_str()))
        .map(|value| value.to_ascii_lowercase());
    if requested_source_ref.is_some() && requested_extension.is_none() {
        return Err(AppError::validation(
            "Artifact bundle component sourceRef must include a supported extension: .ecky, .py, .FCMacro, or .step.",
        ));
    }
    let macro_source = available_bundle_macro_source(&request.artifact_bundle)?;
    let step_source = available_bundle_step_source(&request.artifact_bundle)?;

    match requested_extension.as_deref() {
        Some("ecky") | Some("py") | Some("fcmacro") => {
            let Some((copy_from, actual_extension)) = macro_source else {
                return Err(AppError::validation(
                    "Artifact bundle component package requested source text, but artifact bundle has no reusable macro source.",
                ));
            };
            if requested_extension.as_deref() != Some(actual_extension.as_str()) {
                return Err(AppError::validation(format!(
                    "Artifact bundle component sourceRef extension '.{}' does not match bundle source extension '.{}'.",
                    requested_extension.unwrap_or_default(),
                    actual_extension
                )));
            }
            let source_ref = requested_source_ref.expect("checked").to_string();
            return Ok(PackagedComponentSource {
                source_ref,
                copy_from,
                source_language: Some(request.artifact_bundle.source_language),
                geometry_backend: Some(request.artifact_bundle.geometry_backend),
                macro_dialect: macro_dialect_for_source_language(
                    request.artifact_bundle.source_language,
                ),
            });
        }
        Some("step") | Some("stp") => {
            let Some(copy_from) = step_source else {
                return Err(AppError::validation(
                    "Artifact bundle component package requested STEP source, but artifact bundle has no STEP export artifact.",
                ));
            };
            return Ok(PackagedComponentSource {
                source_ref: requested_source_ref.expect("checked").to_string(),
                copy_from,
                source_language: None,
                geometry_backend: None,
                macro_dialect: None,
            });
        }
        Some(other) => {
            return Err(AppError::validation(format!(
                "Artifact bundle component sourceRef extension '.{}' is unsupported. Expected .ecky, .py, .FCMacro, or .step.",
                other
            )));
        }
        None => {}
    }

    if let Some((copy_from, extension)) = macro_source {
        return Ok(PackagedComponentSource {
            source_ref: format!("components/{}/source.{}", request.component_id, extension),
            copy_from,
            source_language: Some(request.artifact_bundle.source_language),
            geometry_backend: Some(request.artifact_bundle.geometry_backend),
            macro_dialect: macro_dialect_for_source_language(
                request.artifact_bundle.source_language,
            ),
        });
    }
    if let Some(copy_from) = step_source {
        return Ok(PackagedComponentSource {
            source_ref: format!("components/{}/source.step", request.component_id),
            copy_from,
            source_language: None,
            geometry_backend: None,
            macro_dialect: None,
        });
    }

    Err(AppError::validation(
        "Artifact bundle component package requires either a reusable macro source or a STEP export artifact.",
    ))
}

fn resolve_packaged_component_contract(
    request: &ArtifactBundleComponentPackageRequest,
    packaged_source: &PackagedComponentSource,
) -> AppResult<component_package_runtime::DerivedComponentSourceContract> {
    let mut derived = component_package_runtime::DerivedComponentSourceContract {
        params: Vec::new(),
        ui_spec: crate::models::UiSpec::default(),
        initial_params: DesignParams::new(),
    };
    if !request.ui_spec.fields.is_empty() || !request.initial_params.is_empty() {
        crate::models::validate_ui_spec(&request.ui_spec)?;
        crate::models::validate_design_params(&request.initial_params, &request.ui_spec)?;
        derived.ui_spec = request.ui_spec.clone();
        derived.initial_params = request.initial_params.clone();
        derived.params = component_package_runtime::component_params_from_ui_contract(
            &derived.ui_spec,
            &derived.initial_params,
        );
    } else if packaged_source.source_language.is_some() {
        derived = component_package_runtime::derive_component_source_contract_from_path(
            &packaged_source.copy_from,
        )?;
    }
    if !request.params.is_empty() {
        derived.params = request.params.clone();
    }
    Ok(derived)
}

fn available_bundle_macro_source(bundle: &ArtifactBundle) -> AppResult<Option<(PathBuf, String)>> {
    let Some(path) = bundle.macro_path.as_deref() else {
        return Ok(None);
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let source_path = PathBuf::from(trimmed);
    if !source_path.is_file() {
        return Err(AppError::not_found(format!(
            "Artifact bundle macro source '{}' was not found.",
            trimmed
        )));
    }
    let extension = source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| {
            AppError::validation(format!(
                "Artifact bundle macro source '{}' must have a supported extension.",
                trimmed
            ))
        })?;
    if !matches!(extension.as_str(), "ecky" | "py" | "fcmacro") {
        return Ok(None);
    }
    Ok(Some((source_path, extension)))
}

fn available_bundle_step_source(bundle: &ArtifactBundle) -> AppResult<Option<PathBuf>> {
    let Some(step_artifact) = bundle
        .export_artifacts
        .iter()
        .find(|artifact| artifact.format.eq_ignore_ascii_case("step"))
    else {
        return Ok(None);
    };
    let path = PathBuf::from(step_artifact.path.trim());
    if !path.is_file() {
        return Err(AppError::not_found(format!(
            "Artifact bundle STEP export '{}' was not found.",
            step_artifact.path
        )));
    }
    Ok(Some(path))
}

fn artifact_bundle_target_ids(bundle: &ArtifactBundle) -> AppResult<HashSet<String>> {
    let manifest = read_artifact_bundle_manifest(bundle)?;
    Ok(manifest
        .selection_targets
        .iter()
        .flat_map(|target| {
            target
                .target_id
                .iter()
                .cloned()
                .chain(target.alias_ids.iter().cloned())
        })
        .chain(bundle.edge_targets.iter().flat_map(|target| {
            std::iter::once(target.target_id.clone()).chain(target.alias_ids.iter().cloned())
        }))
        .chain(bundle.face_targets.iter().flat_map(|target| {
            std::iter::once(target.target_id.clone()).chain(target.alias_ids.iter().cloned())
        }))
        .collect())
}

fn read_artifact_bundle_manifest(bundle: &ArtifactBundle) -> AppResult<ModelManifest> {
    let raw = fs::read_to_string(bundle.manifest_path.trim()).map_err(|err| {
        AppError::persistence(format!(
            "Failed to read artifact bundle manifest '{}': {}",
            bundle.manifest_path, err
        ))
    })?;
    let manifest: ModelManifest = serde_json::from_str(&raw).map_err(|err| {
        AppError::parse(format!(
            "Failed to parse artifact bundle manifest '{}': {}",
            bundle.manifest_path, err
        ))
    })?;
    Ok(manifest)
}

fn macro_dialect_for_source_language(source_language: SourceLanguage) -> Option<MacroDialect> {
    Some(match source_language {
        SourceLanguage::EckyIrV0 => MacroDialect::EckyIrV0,
        SourceLanguage::Build123d => MacroDialect::Build123d,
        SourceLanguage::LegacyPython => MacroDialect::Legacy,
    })
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

pub async fn resolve_installed_component_source_for_app(
    app: &dyn PathResolver,
    package_id: String,
    version: String,
    component_id: String,
) -> AppResult<InstalledComponentSource> {
    component_package_runtime::resolve_installed_component_source(
        app,
        &package_id,
        &version,
        &component_id,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn resolve_installed_component_source(
    package_id: String,
    version: String,
    component_id: String,
    app: AppHandle,
) -> AppResult<InstalledComponentSource> {
    resolve_installed_component_source_for_app(&app, package_id, version, component_id).await
}

pub async fn resolve_installed_component_controls_for_app(
    app: &dyn PathResolver,
    package_id: String,
    version: String,
    component_id: String,
    parameters: DesignParams,
) -> AppResult<InstalledComponentControls> {
    let installed_source = component_package_runtime::resolve_installed_component_source(
        app,
        &package_id,
        &version,
        &component_id,
    )?;
    let merged_parameters =
        merge_component_render_parameters(&installed_source.component, &parameters);
    Ok(InstalledComponentControls {
        installed_source,
        parameters: merged_parameters,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn resolve_installed_component_controls(
    package_id: String,
    version: String,
    component_id: String,
    parameters: DesignParams,
    app: AppHandle,
) -> AppResult<InstalledComponentControls> {
    resolve_installed_component_controls_for_app(
        &app,
        package_id,
        version,
        component_id,
        parameters,
    )
    .await
}

pub async fn resolve_installed_component_assembly_for_app(
    app: &dyn PathResolver,
    package_id: String,
    version: String,
    assembly_id: String,
) -> AppResult<InstalledAssemblySource> {
    let mut assembly_source = component_package_runtime::resolve_installed_component_assembly(
        app,
        &package_id,
        &version,
        &assembly_id,
    )?;
    let solve = solve_installed_assembly(&assembly_source);
    for component in &mut assembly_source.components {
        component.placement_frame = solve.placement_frames.get(&component.instance_id).cloned();
    }
    assembly_source.mate_results = solve.mate_results;
    Ok(assembly_source)
}

#[tauri::command]
#[specta::specta]
pub async fn resolve_installed_component_assembly(
    package_id: String,
    version: String,
    assembly_id: String,
    app: AppHandle,
) -> AppResult<InstalledAssemblySource> {
    resolve_installed_component_assembly_for_app(&app, package_id, version, assembly_id).await
}

pub async fn resolve_installed_component_assembly_controls_for_app(
    app: &dyn PathResolver,
    package_id: String,
    version: String,
    assembly_id: String,
    instance_parameters: BTreeMap<String, DesignParams>,
) -> AppResult<InstalledAssemblyControls> {
    let assembly_source =
        resolve_installed_component_assembly_for_app(app, package_id, version, assembly_id).await?;
    let mates_solved = assembly_source
        .mate_results
        .iter()
        .all(|result| result.solved);
    let components = assembly_source
        .components
        .iter()
        .map(|component| {
            let overrides = instance_parameters
                .get(&component.instance_id)
                .cloned()
                .unwrap_or_default();
            let parameters = merge_component_render_parameters(
                &component.installed_source.component,
                &overrides,
            );
            InstalledAssemblyComponentControls {
                instance_id: component.instance_id.clone(),
                component_id: component.component_id.clone(),
                parameters,
                placement_frame: component.placement_frame.clone(),
                installed_source: component.installed_source.clone(),
            }
        })
        .collect();

    Ok(InstalledAssemblyControls {
        package_id: assembly_source.package_id,
        version: assembly_source.version,
        package_display_name: assembly_source.package_display_name,
        package_dir: assembly_source.package_dir,
        assembly: assembly_source.assembly,
        port_types: assembly_source.port_types,
        mate_types: assembly_source.mate_types,
        components,
        mate_results: assembly_source.mate_results,
        mates_solved,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn resolve_installed_component_assembly_controls(
    package_id: String,
    version: String,
    assembly_id: String,
    instance_parameters: BTreeMap<String, DesignParams>,
    app: AppHandle,
) -> AppResult<InstalledAssemblyControls> {
    resolve_installed_component_assembly_controls_for_app(
        &app,
        package_id,
        version,
        assembly_id,
        instance_parameters,
    )
    .await
}

pub async fn render_installed_component_source_for_app(
    app: &dyn PathResolver,
    state: &AppState,
    package_id: String,
    version: String,
    component_id: String,
    parameters: DesignParams,
) -> AppResult<InstalledComponentRuntime> {
    let installed_source = component_package_runtime::resolve_installed_component_source(
        app,
        &package_id,
        &version,
        &component_id,
    )?;
    let merged_parameters =
        merge_component_render_parameters(&installed_source.component, &parameters);
    render_resolved_component_source(app, state, installed_source, merged_parameters).await
}

#[tauri::command]
#[specta::specta]
pub async fn render_installed_component_source(
    package_id: String,
    version: String,
    component_id: String,
    parameters: DesignParams,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<InstalledComponentRuntime> {
    render_installed_component_source_for_app(
        &app,
        &state,
        package_id,
        version,
        component_id,
        parameters,
    )
    .await
}

pub async fn render_installed_component_assembly_for_app(
    app: &dyn PathResolver,
    state: &AppState,
    package_id: String,
    version: String,
    assembly_id: String,
    instance_parameters: BTreeMap<String, DesignParams>,
) -> AppResult<InstalledAssemblyRuntime> {
    let assembly_source =
        resolve_installed_component_assembly_for_app(app, package_id, version, assembly_id).await?;
    let placement_frames = assembly_source
        .components
        .iter()
        .filter_map(|component| {
            component
                .placement_frame
                .clone()
                .map(|placement| (component.instance_id.clone(), placement))
        })
        .collect::<BTreeMap<_, _>>();
    let mate_results = assembly_source.mate_results.clone();
    let mates_solved = mate_results.iter().all(|result| result.solved);
    let output_plan = plan_installed_assembly_output(&assembly_source.assembly);
    let mut components = Vec::new();

    for component in &assembly_source.components {
        let overrides = instance_parameters
            .get(&component.instance_id)
            .cloned()
            .unwrap_or_default();
        let parameters =
            merge_component_render_parameters(&component.installed_source.component, &overrides);
        components.push(crate::models::InstalledAssemblyComponentRuntime {
            instance_id: component.instance_id.clone(),
            component_id: component.component_id.clone(),
            parameters: parameters.clone(),
            placement_frame: placement_frames.get(&component.instance_id).cloned(),
            runtime: render_resolved_component_source(
                app,
                state,
                component.installed_source.clone(),
                parameters,
            )
            .await?,
        });
    }
    let output_runtime = render_installed_assembly_output_runtime(
        app,
        state,
        &assembly_source,
        &components,
        mates_solved,
    )
    .await?;
    let operation_results = evaluate_installed_assembly_operations(
        &assembly_source,
        &output_plan,
        mates_solved,
        output_runtime.is_some(),
    );
    let operations_applied = operation_results.iter().all(|result| result.applied);
    let mut warnings = mate_results
        .iter()
        .filter_map(|result| result.warning.clone())
        .collect::<Vec<_>>();
    warnings.extend(
        operation_results
            .iter()
            .filter_map(|result| result.warning.clone()),
    );
    warnings.extend(installed_assembly_pending_warnings(
        &assembly_source.assembly,
        mates_solved,
        operations_applied,
    ));

    Ok(InstalledAssemblyRuntime {
        package_id: assembly_source.package_id,
        version: assembly_source.version,
        package_display_name: assembly_source.package_display_name,
        package_dir: assembly_source.package_dir,
        assembly: assembly_source.assembly,
        port_types: assembly_source.port_types,
        mate_types: assembly_source.mate_types,
        components,
        mate_results,
        mates_solved,
        operation_results,
        operations_applied,
        output_runtime,
        warnings,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn render_installed_component_assembly(
    package_id: String,
    version: String,
    assembly_id: String,
    instance_parameters: BTreeMap<String, DesignParams>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<InstalledAssemblyRuntime> {
    render_installed_component_assembly_for_app(
        &app,
        &state,
        package_id,
        version,
        assembly_id,
        instance_parameters,
    )
    .await
}

pub async fn export_installed_component_assembly_3mf_for_app(
    app: &dyn PathResolver,
    state: &AppState,
    package_id: String,
    version: String,
    assembly_id: String,
    instance_parameters: BTreeMap<String, DesignParams>,
    target_path: String,
    model_name: Option<String>,
) -> AppResult<()> {
    let assembly_runtime = render_installed_component_assembly_for_app(
        app,
        state,
        package_id,
        version,
        assembly_id,
        instance_parameters,
    )
    .await?;
    ensure_assembly_runtime_can_export_placed_parts(&assembly_runtime, "3MF")?;
    let parts = build_installed_assembly_export_parts(&assembly_runtime)?;
    crate::commands::render::export_multipart_3mf_impl(
        &parts,
        &target_path,
        model_name.unwrap_or_else(|| assembly_runtime.assembly.display_name.clone()),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn export_installed_component_assembly_3mf(
    package_id: String,
    version: String,
    assembly_id: String,
    instance_parameters: BTreeMap<String, DesignParams>,
    target_path: String,
    model_name: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    export_installed_component_assembly_3mf_for_app(
        &app,
        &state,
        package_id,
        version,
        assembly_id,
        instance_parameters,
        target_path,
        model_name,
    )
    .await
}

pub async fn export_installed_component_assembly_multipart_stl_zip_for_app(
    app: &dyn PathResolver,
    state: &AppState,
    package_id: String,
    version: String,
    assembly_id: String,
    instance_parameters: BTreeMap<String, DesignParams>,
    target_path: String,
    model_name: Option<String>,
) -> AppResult<()> {
    let assembly_runtime = render_installed_component_assembly_for_app(
        app,
        state,
        package_id,
        version,
        assembly_id,
        instance_parameters,
    )
    .await?;
    ensure_assembly_runtime_can_export_placed_parts(&assembly_runtime, "STL zip")?;
    let parts = build_installed_assembly_export_parts(&assembly_runtime)?;
    crate::commands::render::export_multipart_stl_zip_impl(
        &parts,
        &target_path,
        model_name.unwrap_or_else(|| assembly_runtime.assembly.display_name.clone()),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn export_installed_component_assembly_multipart_stl_zip(
    package_id: String,
    version: String,
    assembly_id: String,
    instance_parameters: BTreeMap<String, DesignParams>,
    target_path: String,
    model_name: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    export_installed_component_assembly_multipart_stl_zip_for_app(
        &app,
        &state,
        package_id,
        version,
        assembly_id,
        instance_parameters,
        target_path,
        model_name,
    )
    .await
}

fn ensure_assembly_runtime_can_export_placed_parts(
    assembly_runtime: &InstalledAssemblyRuntime,
    format_label: &str,
) -> AppResult<()> {
    match assembly_runtime.assembly.output.mode {
        AssemblyOutputMode::SeparateParts | AssemblyOutputMode::JoinedAssembly => {}
        _ => {
            return Err(crate::models::AppError::validation(format!(
                "Installed assembly '{}@{}:{}' output mode '{:?}' is not supported for placed {} export.",
                assembly_runtime.package_id,
                assembly_runtime.version,
                assembly_runtime.assembly.assembly_id,
                assembly_runtime.assembly.output.mode,
                format_label,
            )))
        }
    }
    if assembly_runtime.assembly.mates.is_empty() || assembly_runtime.mates_solved {
        return Ok(());
    }
    let detail = assembly_runtime
        .mate_results
        .iter()
        .find(|result| !result.solved)
        .and_then(|result| result.warning.clone())
        .unwrap_or_else(|| {
            format!(
                "Assembly '{}' still has unsolved mates.",
                assembly_runtime.assembly.assembly_id
            )
        });
    Err(crate::models::AppError::validation(format!(
        "Installed assembly '{}@{}:{}' cannot export placed {}: {}",
        assembly_runtime.package_id,
        assembly_runtime.version,
        assembly_runtime.assembly.assembly_id,
        format_label,
        detail
    )))
}

fn build_installed_assembly_export_parts(
    assembly_runtime: &InstalledAssemblyRuntime,
) -> AppResult<Vec<ExportPartInput>> {
    let mut parts = Vec::new();
    for component in &assembly_runtime.components {
        let placement_frame = component.placement_frame.clone().or_else(|| {
            if assembly_runtime.assembly.mates.is_empty() {
                Some(PortFrame::identity())
            } else {
                None
            }
        });
        if component.runtime.artifact_bundle.viewer_assets.is_empty() {
            let preview_path = component.runtime.artifact_bundle.preview_stl_path.trim();
            if preview_path.is_empty() {
                return Err(crate::models::AppError::validation(format!(
                    "Installed assembly '{}@{}:{}' component instance '{}' has no exportable viewer assets or preview STL.",
                    assembly_runtime.package_id,
                    assembly_runtime.version,
                    assembly_runtime.assembly.assembly_id,
                    component.instance_id
                )));
            }
            parts.push(ExportPartInput {
                label: format!("{} / {}", component.instance_id, component.component_id),
                path: preview_path.to_string(),
                object_name: Some(format!(
                    "{} / {}",
                    component.instance_id, component.component_id
                )),
                part_id: Some(format!("{}:preview", component.instance_id)),
                display_color: None,
                placement_frame: placement_frame.clone(),
            });
            continue;
        }
        for asset in &component.runtime.artifact_bundle.viewer_assets {
            parts.push(ExportPartInput {
                label: format!("{} / {}", component.instance_id, asset.label),
                path: asset.path.clone(),
                object_name: Some(format!("{} / {}", component.instance_id, asset.object_name)),
                part_id: Some(format!("{}:{}", component.instance_id, asset.part_id)),
                display_color: None,
                placement_frame: placement_frame.clone(),
            });
        }
    }
    Ok(parts)
}

async fn render_installed_assembly_output_runtime(
    app: &dyn PathResolver,
    state: &AppState,
    assembly_source: &InstalledAssemblySource,
    components: &[crate::models::InstalledAssemblyComponentRuntime],
    mates_solved: bool,
) -> AppResult<Option<InstalledAssemblyOutputRuntime>> {
    let output_plan = plan_installed_assembly_output(&assembly_source.assembly);
    if output_plan.mode == AssemblyOutputPlanMode::None {
        return Ok(None);
    }
    if !assembly_source.assembly.mates.is_empty() && !mates_solved {
        return Ok(None);
    }
    if assembly_source.assembly.operations.iter().any(|operation| {
        installed_assembly_operation_preflight_warning(
            assembly_source,
            &output_plan,
            operation,
            mates_solved,
        )
        .is_some()
    }) {
        return Ok(None);
    }

    let parts =
        build_installed_assembly_output_step_inputs(assembly_source, components, &output_plan)?;
    let (artifact_bundle, model_manifest) = crate::freecad::assemble_step_parts(
        app,
        crate::services::render::configured_freecad_cmd(state).as_deref(),
        &assembly_source.assembly.assembly_id,
        &assembly_source.assembly.display_name,
        &parts,
    )?;
    let runtime_cache_dir = crate::freecad::runtime_cache_dir(app)?;
    crate::freecad::evict_cache_if_needed(&runtime_cache_dir);
    Ok(Some(InstalledAssemblyOutputRuntime {
        artifact_bundle,
        model_manifest,
    }))
}

fn plan_installed_assembly_output(assembly: &AssemblyDefinition) -> AssemblyOutputPlan {
    match assembly.output.mode {
        AssemblyOutputMode::JoinedAssembly if assembly.operations.is_empty() => {
            AssemblyOutputPlan {
                mode: AssemblyOutputPlanMode::Joined,
                fuse_group_by_instance: BTreeMap::new(),
                cut_group_by_instance: BTreeMap::new(),
            }
        }
        AssemblyOutputMode::JoinedAssembly => match derive_pure_fuse_group_map(assembly) {
            Some(fuse_group_by_instance) => AssemblyOutputPlan {
                mode: AssemblyOutputPlanMode::Joined,
                fuse_group_by_instance,
                cut_group_by_instance: BTreeMap::new(),
            },
            None => match derive_pure_cut_group_map(assembly) {
                Some(cut_group_by_instance) => AssemblyOutputPlan {
                    mode: AssemblyOutputPlanMode::Joined,
                    fuse_group_by_instance: BTreeMap::new(),
                    cut_group_by_instance,
                },
                None => AssemblyOutputPlan {
                    mode: AssemblyOutputPlanMode::None,
                    fuse_group_by_instance: BTreeMap::new(),
                    cut_group_by_instance: BTreeMap::new(),
                },
            },
        },
        AssemblyOutputMode::FusedSolid => match derive_pure_fuse_group_map(assembly) {
            Some(fuse_group_by_instance)
                if pure_fuse_groups_cover_whole_assembly(assembly, &fuse_group_by_instance) =>
            {
                AssemblyOutputPlan {
                    mode: AssemblyOutputPlanMode::FusedSolid,
                    fuse_group_by_instance,
                    cut_group_by_instance: BTreeMap::new(),
                }
            }
            _ => AssemblyOutputPlan {
                mode: AssemblyOutputPlanMode::None,
                fuse_group_by_instance: BTreeMap::new(),
                cut_group_by_instance: BTreeMap::new(),
            },
        },
        _ => AssemblyOutputPlan {
            mode: AssemblyOutputPlanMode::None,
            fuse_group_by_instance: BTreeMap::new(),
            cut_group_by_instance: BTreeMap::new(),
        },
    }
}

fn derive_pure_fuse_group_map(assembly: &AssemblyDefinition) -> Option<BTreeMap<String, String>> {
    if assembly.operations.is_empty() {
        return Some(BTreeMap::new());
    }
    let mut parent = HashMap::<String, String>::new();

    for operation in &assembly.operations {
        if operation.kind != crate::models::OperationKind::Fuse
            || !operation.port_refs.is_empty()
            || !operation.params.is_empty()
        {
            return None;
        }
        let mut targets = operation.target_instance_ids.iter();
        let first = targets.next()?.clone();
        parent.entry(first.clone()).or_insert_with(|| first.clone());
        for target in targets {
            parent
                .entry(target.clone())
                .or_insert_with(|| target.clone());
            union_instance_group(&mut parent, &first, target);
        }
    }

    let mut members_by_root = BTreeMap::<String, Vec<String>>::new();
    let keys = parent.keys().cloned().collect::<Vec<_>>();
    for instance_id in keys {
        let root = find_instance_group_root(&mut parent, &instance_id);
        members_by_root.entry(root).or_default().push(instance_id);
    }

    let mut fuse_group_by_instance = BTreeMap::new();
    for (group_index, members) in members_by_root.values().enumerate() {
        if members.len() < 2 {
            continue;
        }
        let group_id = format!("fuse-group-{}", group_index + 1);
        for instance_id in members {
            fuse_group_by_instance.insert(instance_id.clone(), group_id.clone());
        }
    }

    Some(fuse_group_by_instance)
}

fn derive_pure_cut_group_map(
    assembly: &AssemblyDefinition,
) -> Option<BTreeMap<String, CutGroupMembership>> {
    if assembly.operations.is_empty() {
        return Some(BTreeMap::new());
    }

    let mut cut_group_by_instance = BTreeMap::new();

    for (group_index, operation) in assembly.operations.iter().enumerate() {
        if operation.kind != OperationKind::Cut
            || !operation.port_refs.is_empty()
            || !operation.params.is_empty()
        {
            return None;
        }
        let mut targets = operation.target_instance_ids.iter();
        let base_instance_id = targets.next()?.clone();
        let group_id = format!("cut-group-{}", group_index + 1);
        if cut_group_by_instance
            .insert(
                base_instance_id,
                CutGroupMembership {
                    group_id: group_id.clone(),
                    role: CutGroupRole::Base,
                },
            )
            .is_some()
        {
            return None;
        }
        for target_instance_id in targets {
            if cut_group_by_instance
                .insert(
                    target_instance_id.clone(),
                    CutGroupMembership {
                        group_id: group_id.clone(),
                        role: CutGroupRole::Tool,
                    },
                )
                .is_some()
            {
                return None;
            }
        }
    }

    Some(cut_group_by_instance)
}

fn pure_fuse_groups_cover_whole_assembly(
    assembly: &AssemblyDefinition,
    fuse_group_by_instance: &BTreeMap<String, String>,
) -> bool {
    if assembly.components.is_empty() || fuse_group_by_instance.len() != assembly.components.len() {
        return false;
    }
    if fuse_group_by_instance
        .values()
        .collect::<HashSet<_>>()
        .len()
        != 1
    {
        return false;
    }
    assembly
        .components
        .iter()
        .all(|component| fuse_group_by_instance.contains_key(&component.instance_id))
}

fn find_instance_group_root(parent: &mut HashMap<String, String>, instance_id: &str) -> String {
    let current = parent
        .get(instance_id)
        .cloned()
        .unwrap_or_else(|| instance_id.to_string());
    if current == instance_id {
        return current;
    }
    let root = find_instance_group_root(parent, &current);
    parent.insert(instance_id.to_string(), root.clone());
    root
}

fn union_instance_group(parent: &mut HashMap<String, String>, left: &str, right: &str) {
    let left_root = find_instance_group_root(parent, left);
    let right_root = find_instance_group_root(parent, right);
    if left_root != right_root {
        parent.insert(right_root, left_root);
    }
}

fn installed_assembly_operation_fusion_zone_ids_by_instance(
    assembly_source: &InstalledAssemblySource,
    operation: &AssemblyOperation,
) -> Result<BTreeMap<String, String>, String> {
    let mut zone_ids_by_instance = BTreeMap::new();

    for instance_id in &operation.target_instance_ids {
        let Some(component) = assembly_source
            .components
            .iter()
            .find(|component| component.instance_id == *instance_id)
        else {
            return Err(format!(
                "Assembly '{}' operation '{}' references unknown target instance '{}'.",
                assembly_source.assembly.assembly_id, operation.operation_id, instance_id
            ));
        };
        let Some(zone) = component
            .installed_source
            .component
            .fusion_zones
            .iter()
            .find(|zone| zone.allowed_ops.contains(&operation.kind))
        else {
            return Err(format!(
                "Assembly '{}' operation '{}' target instance '{}' is missing {:?}-capable fusion zone.",
                assembly_source.assembly.assembly_id,
                operation.operation_id,
                instance_id,
                operation.kind
            ));
        };
        zone_ids_by_instance.insert(instance_id.clone(), zone.zone_id.clone());
    }

    Ok(zone_ids_by_instance)
}

fn installed_assembly_operation_preflight_warning(
    assembly_source: &InstalledAssemblySource,
    output_plan: &AssemblyOutputPlan,
    operation: &AssemblyOperation,
    mates_solved: bool,
) -> Option<String> {
    if !assembly_source.assembly.mates.is_empty() && !mates_solved {
        return Some(format!(
            "Assembly '{}' operation '{}' is waiting on solved mates.",
            assembly_source.assembly.assembly_id, operation.operation_id
        ));
    }

    match assembly_source.assembly.output.mode {
        AssemblyOutputMode::SeparateParts => {
            return Some(format!(
                "Assembly '{}' output mode 'SeparateParts' does not execute operation '{}'.",
                assembly_source.assembly.assembly_id, operation.operation_id
            ));
        }
        AssemblyOutputMode::MoldedSolid => {
            return Some(format!(
                "Assembly '{}' output mode 'MoldedSolid' is not supported for operation '{}'.",
                assembly_source.assembly.assembly_id, operation.operation_id
            ));
        }
        _ => {}
    }

    if !matches!(
        operation.kind,
        OperationKind::Fuse | OperationKind::Cut | OperationKind::Blend | OperationKind::Mold
    ) {
        return Some(format!(
            "Assembly '{}' operation '{}' kind '{:?}' does not support fusion-zone execution.",
            assembly_source.assembly.assembly_id, operation.operation_id, operation.kind
        ));
    }

    if let Err(warning) =
        installed_assembly_operation_fusion_zone_ids_by_instance(assembly_source, operation)
    {
        return Some(warning);
    }

    match operation.kind {
        OperationKind::Cut => {
            let target_group_ids = operation
                .target_instance_ids
                .iter()
                .filter_map(|instance_id| output_plan.cut_group_by_instance.get(instance_id))
                .map(|membership| membership.group_id.as_str())
                .collect::<HashSet<_>>();

            if target_group_ids.len() != 1 {
                return Some(format!(
                    "Assembly '{}' operation '{}' does not resolve to one cut group.",
                    assembly_source.assembly.assembly_id, operation.operation_id
                ));
            }
        }
        OperationKind::Fuse | OperationKind::Blend | OperationKind::Mold => {
            let Some(fuse_group_map) = derive_pure_fuse_group_map(&assembly_source.assembly) else {
                return Some(format!(
                    "Assembly '{}' operation '{}' is not a supported pure Fuse group.",
                    assembly_source.assembly.assembly_id, operation.operation_id
                ));
            };

            let target_group_ids = operation
                .target_instance_ids
                .iter()
                .filter_map(|instance_id| fuse_group_map.get(instance_id))
                .collect::<HashSet<_>>();

            if target_group_ids.len() != 1 {
                return Some(format!(
                    "Assembly '{}' operation '{}' does not resolve to one fuse group.",
                    assembly_source.assembly.assembly_id, operation.operation_id
                ));
            }

            if assembly_source.assembly.output.mode == AssemblyOutputMode::FusedSolid
                && !pure_fuse_groups_cover_whole_assembly(
                    &assembly_source.assembly,
                    &fuse_group_map,
                )
            {
                return Some(format!(
                    "Assembly '{}' fused output requires pure Fuse groups to cover the whole assembly; operation '{}' is partial.",
                    assembly_source.assembly.assembly_id, operation.operation_id
                ));
            }
        }
        _ => {}
    }

    if output_plan.mode == AssemblyOutputPlanMode::None {
        return Some(format!(
            "Assembly '{}' operation '{}' does not have an executable output runtime yet.",
            assembly_source.assembly.assembly_id, operation.operation_id
        ));
    }

    None
}

fn installed_assembly_operation_group_id(
    output_plan: &AssemblyOutputPlan,
    operation: &AssemblyOperation,
) -> Option<String> {
    match operation.kind {
        OperationKind::Fuse => operation
            .target_instance_ids
            .iter()
            .find_map(|instance_id| output_plan.fuse_group_by_instance.get(instance_id))
            .cloned(),
        OperationKind::Cut => operation
            .target_instance_ids
            .iter()
            .find_map(|instance_id| output_plan.cut_group_by_instance.get(instance_id))
            .map(|membership| membership.group_id.clone()),
        _ => None,
    }
}

fn evaluate_installed_assembly_operations(
    assembly_source: &InstalledAssemblySource,
    output_plan: &AssemblyOutputPlan,
    mates_solved: bool,
    output_runtime_ready: bool,
) -> Vec<InstalledAssemblyOperationResult> {
    let fuse_group_map = derive_pure_fuse_group_map(&assembly_source.assembly);

    assembly_source
        .assembly
        .operations
        .iter()
        .map(|operation| {
            let mut result = InstalledAssemblyOperationResult {
                operation_id: operation.operation_id.clone(),
                applied: false,
                group_id: None,
                fusion_zone_ids_by_instance: BTreeMap::new(),
                warning: None,
            };

            if let Some(warning) = installed_assembly_operation_preflight_warning(
                assembly_source,
                output_plan,
                operation,
                mates_solved,
            ) {
                result.warning = Some(warning);
                return result;
            }

            if !output_runtime_ready {
                result.warning = Some(format!(
                    "Assembly '{}' operation '{}' does not have an executable output runtime yet.",
                    assembly_source.assembly.assembly_id, operation.operation_id
                ));
                return result;
            }

            let Ok(fusion_zone_ids_by_instance) =
                installed_assembly_operation_fusion_zone_ids_by_instance(
                    assembly_source,
                    operation,
                )
            else {
                result.warning = Some(format!(
                    "Assembly '{}' operation '{}' does not have an executable output runtime yet.",
                    assembly_source.assembly.assembly_id, operation.operation_id
                ));
                return result;
            };
            result.applied = true;
            result.group_id = installed_assembly_operation_group_id(output_plan, operation)
                .or_else(|| {
                    fuse_group_map.as_ref().and_then(|fuse_group_map| {
                        operation
                            .target_instance_ids
                            .iter()
                            .find_map(|instance_id| fuse_group_map.get(instance_id))
                            .cloned()
                    })
                });
            result.fusion_zone_ids_by_instance = fusion_zone_ids_by_instance;
            result
        })
        .collect()
}

fn build_installed_assembly_output_step_inputs(
    assembly_source: &InstalledAssemblySource,
    components: &[crate::models::InstalledAssemblyComponentRuntime],
    output_plan: &AssemblyOutputPlan,
) -> AppResult<Vec<crate::freecad::AssemblyStepPartInput>> {
    let mut parts = Vec::new();

    for component in components {
        let step_path = component
            .runtime
            .artifact_bundle
            .export_artifacts
            .iter()
            .find(|artifact| {
                artifact.role == "primary"
                    && matches!(artifact.format.as_str(), "step" | "stp")
                    && !artifact.path.trim().is_empty()
            })
            .map(|artifact| artifact.path.clone())
            .ok_or_else(|| {
                crate::models::AppError::validation(format!(
                    "Installed assembly '{}@{}:{}' component instance '{}' has no primary STEP export for joined assembly output.",
                    assembly_source.package_id,
                    assembly_source.version,
                    assembly_source.assembly.assembly_id,
                    component.instance_id
                ))
            })?;
        let placement_frame = component.placement_frame.clone().or_else(|| {
            if assembly_source.assembly.mates.is_empty() {
                Some(PortFrame::identity())
            } else {
                None
            }
        });
        if placement_frame.is_none() {
            return Err(crate::models::AppError::validation(format!(
                "Installed assembly '{}@{}:{}' component instance '{}' is missing placement frame for joined assembly output.",
                assembly_source.package_id,
                assembly_source.version,
                assembly_source.assembly.assembly_id,
                component.instance_id
            )));
        }
        parts.push(crate::freecad::AssemblyStepPartInput {
            instance_id: component.instance_id.clone(),
            object_name: component.component_id.clone(),
            label: format!("{} / {}", component.instance_id, component.component_id),
            step_path,
            fuse_group_id: output_plan
                .fuse_group_by_instance
                .get(&component.instance_id)
                .cloned(),
            cut_group_id: output_plan
                .cut_group_by_instance
                .get(&component.instance_id)
                .map(|membership| membership.group_id.clone()),
            cut_role: output_plan
                .cut_group_by_instance
                .get(&component.instance_id)
                .map(|membership| match membership.role {
                    CutGroupRole::Base => "base".to_string(),
                    CutGroupRole::Tool => "tool".to_string(),
                }),
            placement_frame,
        });
    }

    Ok(parts)
}

fn installed_assembly_pending_warnings(
    assembly: &AssemblyDefinition,
    mates_solved: bool,
    operations_applied: bool,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if !mates_solved {
        warnings.push(format!(
            "Assembly '{}' mates are not solved yet; component runtimes are returned as separate parts.",
            assembly.assembly_id
        ));
    }
    if !operations_applied {
        warnings.push(format!(
            "Assembly '{}' operations are not applied yet; component runtimes are returned as separate parts.",
            assembly.assembly_id
        ));
    }
    warnings
}

fn solve_installed_assembly(assembly_source: &InstalledAssemblySource) -> AssemblySolveResult {
    let components_by_instance = assembly_source
        .components
        .iter()
        .map(|component| (component.instance_id.as_str(), component))
        .collect::<HashMap<_, _>>();
    let mut adjacency = HashMap::<&str, Vec<(usize, bool)>>::new();
    for (mate_index, mate) in assembly_source.assembly.mates.iter().enumerate() {
        adjacency
            .entry(mate.a.instance_id.as_str())
            .or_default()
            .push((mate_index, true));
        adjacency
            .entry(mate.b.instance_id.as_str())
            .or_default()
            .push((mate_index, false));
    }

    let mut placements = HashMap::<String, RigidFrame>::new();
    let mut checked_mates = HashSet::<usize>::new();
    let mut mate_results = Vec::new();

    for component in &assembly_source.components {
        if placements.contains_key(&component.instance_id) {
            continue;
        }
        if !adjacency.contains_key(component.instance_id.as_str()) {
            placements.insert(component.instance_id.clone(), RigidFrame::identity());
            continue;
        }
        placements.insert(component.instance_id.clone(), RigidFrame::identity());
        let mut queue = VecDeque::from([component.instance_id.clone()]);
        while let Some(instance_id) = queue.pop_front() {
            let Some(current_placement) = placements.get(&instance_id).cloned() else {
                continue;
            };
            let mates = adjacency
                .get(instance_id.as_str())
                .cloned()
                .unwrap_or_default();
            for (mate_index, current_is_a) in mates {
                if !checked_mates.insert(mate_index) {
                    continue;
                }
                let mate = &assembly_source.assembly.mates[mate_index];
                let mut mate_result = InstalledAssemblyMateResult {
                    mate_id: mate.mate_id.clone(),
                    solved: false,
                    required_clearance: None,
                    available_clearance: None,
                    warning: None,
                };
                let (current_ref, other_ref) = if current_is_a {
                    (&mate.a, &mate.b)
                } else {
                    (&mate.b, &mate.a)
                };
                let Some(current_component) =
                    components_by_instance.get(current_ref.instance_id.as_str())
                else {
                    mate_result.warning = Some(format!(
                        "Assembly '{}' mate '{}' cannot be solved: instance '{}' is missing from resolved components.",
                        assembly_source.assembly.assembly_id, mate.mate_id, current_ref.instance_id
                    ));
                    mate_results.push(mate_result);
                    continue;
                };
                let Some(other_component) =
                    components_by_instance.get(other_ref.instance_id.as_str())
                else {
                    mate_result.warning = Some(format!(
                        "Assembly '{}' mate '{}' cannot be solved: instance '{}' is missing from resolved components.",
                        assembly_source.assembly.assembly_id, mate.mate_id, other_ref.instance_id
                    ));
                    mate_results.push(mate_result);
                    continue;
                };
                let Some(current_port) =
                    find_component_port(*current_component, current_ref.port_id.as_str())
                else {
                    mate_result.warning = Some(format!(
                        "Assembly '{}' mate '{}' cannot be solved: port '{}.{}' is missing from resolved component metadata.",
                        assembly_source.assembly.assembly_id,
                        mate.mate_id,
                        current_ref.instance_id,
                        current_ref.port_id
                    ));
                    mate_results.push(mate_result);
                    continue;
                };
                let Some(other_port) =
                    find_component_port(*other_component, other_ref.port_id.as_str())
                else {
                    mate_result.warning = Some(format!(
                        "Assembly '{}' mate '{}' cannot be solved: port '{}.{}' is missing from resolved component metadata.",
                        assembly_source.assembly.assembly_id,
                        mate.mate_id,
                        other_ref.instance_id,
                        other_ref.port_id
                    ));
                    mate_results.push(mate_result);
                    continue;
                };
                let (source_ref, target_ref, source_port, target_port) = if current_is_a {
                    (&mate.a, &mate.b, current_port, other_port)
                } else {
                    (&mate.a, &mate.b, other_port, current_port)
                };
                match validate_mate_clearance(
                    &assembly_source.assembly.assembly_id,
                    mate,
                    source_ref,
                    target_ref,
                    source_port,
                    target_port,
                ) {
                    Ok((required_clearance, available_clearance)) => {
                        mate_result.required_clearance = required_clearance;
                        mate_result.available_clearance = available_clearance;
                    }
                    Err((required_clearance, available_clearance, warning)) => {
                        mate_result.required_clearance = required_clearance;
                        mate_result.available_clearance = available_clearance;
                        mate_result.warning = Some(warning);
                        mate_results.push(mate_result);
                        continue;
                    }
                }
                let Some(current_frame) = current_port.frame.as_ref() else {
                    mate_result.warning = Some(format!(
                        "Assembly '{}' mate '{}' cannot be solved: port '{}.{}' is missing frame.",
                        assembly_source.assembly.assembly_id,
                        mate.mate_id,
                        current_ref.instance_id,
                        current_ref.port_id
                    ));
                    mate_results.push(mate_result);
                    continue;
                };
                let Some(other_frame) = other_port.frame.as_ref() else {
                    mate_result.warning = Some(format!(
                        "Assembly '{}' mate '{}' cannot be solved: port '{}.{}' is missing frame.",
                        assembly_source.assembly.assembly_id,
                        mate.mate_id,
                        other_ref.instance_id,
                        other_ref.port_id
                    ));
                    mate_results.push(mate_result);
                    continue;
                };
                let current_frame = match RigidFrame::from_port_frame(
                    current_frame,
                    &format!("port '{}.{}'", current_ref.instance_id, current_ref.port_id),
                ) {
                    Ok(frame) => frame,
                    Err(message) => {
                        mate_result.warning = Some(format!(
                            "Assembly '{}' mate '{}' cannot be solved: {}",
                            assembly_source.assembly.assembly_id, mate.mate_id, message
                        ));
                        mate_results.push(mate_result);
                        continue;
                    }
                };
                let other_frame = match RigidFrame::from_port_frame(
                    other_frame,
                    &format!("port '{}.{}'", other_ref.instance_id, other_ref.port_id),
                ) {
                    Ok(frame) => frame,
                    Err(message) => {
                        mate_result.warning = Some(format!(
                            "Assembly '{}' mate '{}' cannot be solved: {}",
                            assembly_source.assembly.assembly_id, mate.mate_id, message
                        ));
                        mate_results.push(mate_result);
                        continue;
                    }
                };
                let derived_other = current_placement
                    .compose(&current_frame)
                    .compose(&other_frame.inverse());
                if let Some(existing_other) = placements.get(&other_ref.instance_id) {
                    if !existing_other.approx_eq(&derived_other) {
                        mate_result.warning = Some(format!(
                            "Assembly '{}' mate '{}' cannot be solved: port frames conflict with existing placement for instance '{}'.",
                            assembly_source.assembly.assembly_id,
                            mate.mate_id,
                            other_ref.instance_id
                        ));
                    } else {
                        mate_result.solved = true;
                    }
                    mate_results.push(mate_result);
                    continue;
                }
                placements.insert(other_ref.instance_id.clone(), derived_other);
                queue.push_back(other_ref.instance_id.clone());
                mate_result.solved = true;
                mate_results.push(mate_result);
            }
        }
    }

    let placement_frames = assembly_source
        .components
        .iter()
        .filter_map(|component| {
            placements
                .remove(&component.instance_id)
                .map(|placement| (component.instance_id.clone(), placement.into_port_frame()))
        })
        .collect::<BTreeMap<_, _>>();

    AssemblySolveResult {
        placement_frames,
        mate_results,
    }
}

fn find_component_port<'a>(
    component: &'a InstalledAssemblyComponentSource,
    port_id: &str,
) -> Option<&'a ComponentPort> {
    component
        .installed_source
        .component
        .ports
        .iter()
        .find(|port| port.port_id == port_id)
}

fn validate_mate_clearance(
    assembly_id: &str,
    mate: &AssemblyMate,
    source_ref: &PortReference,
    target_ref: &PortReference,
    source_port: &ComponentPort,
    target_port: &ComponentPort,
) -> Result<(Option<f64>, Option<f64>), (Option<f64>, Option<f64>, String)> {
    let source_clearance = match numeric_component_interface_param(
        &source_port.params,
        "clearance",
        &format!("port '{}.{}'", source_ref.instance_id, source_ref.port_id),
    ) {
        Ok(value) => value,
        Err(message) => return Err((None, None, with_mate_context(assembly_id, mate, message))),
    };
    let target_clearance = match numeric_component_interface_param(
        &target_port.params,
        "clearance",
        &format!("port '{}.{}'", target_ref.instance_id, target_ref.port_id),
    ) {
        Ok(value) => value,
        Err(message) => {
            return Err((
                source_clearance,
                None,
                with_mate_context(assembly_id, mate, message),
            ));
        }
    };
    let required_clearance = match numeric_component_interface_param(
        &mate.params,
        "clearance",
        &format!("mate '{}'", mate.mate_id),
    ) {
        Ok(value) => value.or(source_clearance),
        Err(message) => {
            return Err((
                source_clearance,
                target_clearance,
                with_mate_context(assembly_id, mate, message),
            ));
        }
    };
    if let Some(required_clearance) = required_clearance {
        let Some(available_clearance) = target_clearance else {
            return Err((
                Some(required_clearance),
                None,
                format!(
                    "Assembly '{}' mate '{}' cannot be solved: target port '{}.{}' is missing numeric clearance for required clearance {}.",
                    assembly_id,
                    mate.mate_id,
                    target_ref.instance_id,
                    target_ref.port_id,
                    required_clearance
                ),
            ));
        };
        if available_clearance + FRAME_EPSILON < required_clearance {
            return Err((
                Some(required_clearance),
                Some(available_clearance),
                format!(
                    "Assembly '{}' mate '{}' cannot be solved: target port '{}.{}' clearance {} is below required clearance {} from source port '{}.{}'.",
                    assembly_id,
                    mate.mate_id,
                    target_ref.instance_id,
                    target_ref.port_id,
                    available_clearance,
                    required_clearance,
                    source_ref.instance_id,
                    source_ref.port_id
                ),
            ));
        }
        return Ok((Some(required_clearance), Some(available_clearance)));
    }
    Ok((None, None))
}

fn numeric_component_interface_param(
    params: &BTreeMap<String, ComponentInterfaceValue>,
    key: &str,
    label: &str,
) -> Result<Option<f64>, String> {
    match params.get(key) {
        None => Ok(None),
        Some(ComponentInterfaceValue::Number(value)) if value.is_finite() => Ok(Some(*value)),
        Some(ComponentInterfaceValue::Number(_)) => {
            Err(format!("{label} param '{key}' must be finite."))
        }
        Some(_) => Err(format!("{label} param '{key}' must be numeric.")),
    }
}

fn with_mate_context(assembly_id: &str, mate: &AssemblyMate, message: String) -> String {
    format!(
        "Assembly '{}' mate '{}' cannot be solved: {}",
        assembly_id, mate.mate_id, message
    )
}

async fn render_resolved_component_source(
    app: &dyn PathResolver,
    state: &AppState,
    installed_source: InstalledComponentSource,
    parameters: DesignParams,
) -> AppResult<InstalledComponentRuntime> {
    let artifact_bundle = crate::services::render::render_model_source(
        Path::new(&installed_source.source_path),
        installed_source.component.source_language,
        installed_source.component.macro_dialect.clone(),
        installed_source.component.geometry_backend,
        &parameters,
        None,
        state,
        app,
    )
    .await?;
    let model_manifest = crate::model_runtime::read_model_manifest(app, &artifact_bundle.model_id)?;
    validate_rendered_component_port_targets(&installed_source, &artifact_bundle, &model_manifest)?;
    let model_manifest = populate_component_feature_graph_ports(
        &installed_source,
        &artifact_bundle,
        &model_manifest,
    )?;
    let model_manifest = crate::model_runtime::write_model_manifest(
        app,
        &artifact_bundle.model_id,
        &model_manifest,
    )?;
    Ok(InstalledComponentRuntime {
        installed_source,
        parameters,
        artifact_bundle,
        model_manifest,
    })
}

fn merge_component_render_parameters(
    component: &ComponentDefinition,
    overrides: &DesignParams,
) -> DesignParams {
    let mut merged = component.initial_params.clone();
    for (key, value) in overrides {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

pub(crate) fn validate_rendered_component_port_targets(
    installed_source: &InstalledComponentSource,
    artifact_bundle: &crate::models::ArtifactBundle,
    model_manifest: &crate::models::ModelManifest,
) -> AppResult<()> {
    let runtime_target_ids = model_manifest
        .selection_targets
        .iter()
        .flat_map(|target| {
            target
                .target_id
                .iter()
                .cloned()
                .chain(target.durable_target_id.iter().cloned())
                .chain(target.canonical_target_id.iter().cloned())
                .chain(target.alias_ids.iter().cloned())
        })
        .chain(artifact_bundle.edge_targets.iter().flat_map(|target| {
            std::iter::once(target.target_id.clone())
                .chain(target.durable_target_id.iter().cloned())
                .chain(target.canonical_target_id.iter().cloned())
                .chain(target.alias_ids.iter().cloned())
        }))
        .chain(artifact_bundle.face_targets.iter().flat_map(|target| {
            std::iter::once(target.target_id.clone())
                .chain(target.durable_target_id.iter().cloned())
                .chain(target.canonical_target_id.iter().cloned())
                .chain(target.alias_ids.iter().cloned())
        }))
        .collect::<HashSet<_>>();
    let is_step_source = Path::new(installed_source.source_path.trim())
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "step" | "stp"))
        .unwrap_or(false);
    let runtime_portable_topology_target_ids = if is_step_source {
        runtime_target_ids
            .iter()
            .filter_map(|target_id| portable_topology_target_id(target_id))
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };

    for port in &installed_source.component.ports {
        for target_id in &port.target_ids {
            let literal_match = runtime_target_ids.contains(target_id);
            let portable_match = is_step_source
                && portable_topology_target_id(target_id)
                    .map(|portable| runtime_portable_topology_target_ids.contains(&portable))
                    .unwrap_or(false);
            if !literal_match && !portable_match {
                return Err(crate::models::AppError::validation(format!(
                    "Installed component '{}@{}:{}' port '{}' targetId '{}' was not found in rendered runtime topology.",
                    installed_source.package_id,
                    installed_source.version,
                    installed_source.component.component_id,
                    port.port_id,
                    target_id
                )));
            }
        }
    }

    Ok(())
}

fn populate_component_feature_graph_ports(
    installed_source: &InstalledComponentSource,
    _artifact_bundle: &crate::models::ArtifactBundle,
    model_manifest: &crate::models::ModelManifest,
) -> AppResult<crate::models::ModelManifest> {
    let mut manifest = model_manifest.clone();
    let Some(feature_graph) = manifest.feature_graph.as_mut() else {
        return Ok(manifest);
    };

    let target_id_map = component_feature_port_target_id_map(installed_source, model_manifest);
    let target_kind_map = component_feature_port_target_kind_map(model_manifest);

    for component_port in &installed_source.component.ports {
        let resolved_target_ids = component_port
            .target_ids
            .iter()
            .filter_map(|target_id| target_id_map.get(target_id).cloned())
            .collect::<Vec<_>>();

        let target_node_index = if component_port.target_ids.is_empty() {
            (feature_graph.nodes.len() == 1).then_some(0)
        } else {
            feature_graph.nodes.iter().position(|node| {
                node.output_refs.iter().any(|output_ref| {
                    output_ref
                        .target_ids
                        .iter()
                        .any(|target_id| resolved_target_ids.contains(target_id))
                })
            })
        };

        let Some(target_node_index) = target_node_index else {
            continue;
        };

        let targets_fully_resolved = !component_port.target_ids.is_empty()
            && resolved_target_ids.len() == component_port.target_ids.len();
        let feature_port = crate::models::FeaturePort {
            port_id: component_port.port_id.clone(),
            type_id: component_port.type_id.clone(),
            target_ids: resolved_target_ids,
            frame: component_port.frame.clone(),
            interfaces: component_port.interfaces.clone(),
            params: component_port.params.clone(),
            source_ref: Some(component_feature_port_source_ref(
                installed_source,
                component_port,
            )),
            confidence: targets_fully_resolved.then_some(1.0),
            target_role: component_feature_port_target_role(
                &component_port.target_ids,
                &target_id_map,
                &target_kind_map,
            ),
        };

        let node = &mut feature_graph.nodes[target_node_index];
        node.ports
            .retain(|existing| existing.port_id != feature_port.port_id);
        node.ports.push(feature_port);
    }

    Ok(manifest)
}

fn component_feature_port_target_id_map(
    installed_source: &InstalledComponentSource,
    model_manifest: &crate::models::ModelManifest,
) -> HashMap<String, String> {
    let is_step_source = Path::new(installed_source.source_path.trim())
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "step" | "stp"))
        .unwrap_or(false);

    let mut target_id_map = HashMap::new();
    let mut portable_target_id_map = HashMap::new();

    for target in &model_manifest.selection_targets {
        let Some(preferred) = target.target_id.clone().or_else(|| {
            target
                .durable_target_id
                .clone()
                .or_else(|| target.canonical_target_id.clone())
                .or_else(|| target.alias_ids.first().cloned())
        }) else {
            continue;
        };

        for target_id in target
            .target_id
            .iter()
            .chain(target.durable_target_id.iter())
            .chain(target.canonical_target_id.iter())
            .chain(target.alias_ids.iter())
        {
            target_id_map.insert(target_id.clone(), preferred.clone());
            if is_step_source {
                if let Some(portable) = portable_topology_target_id(target_id) {
                    portable_target_id_map.insert(portable, preferred.clone());
                }
            }
        }
    }

    if is_step_source {
        for component_port in &installed_source.component.ports {
            for target_id in &component_port.target_ids {
                if target_id_map.contains_key(target_id) {
                    continue;
                }
                if let Some(preferred) = portable_topology_target_id(target_id)
                    .and_then(|portable| portable_target_id_map.get(&portable).cloned())
                {
                    target_id_map.insert(target_id.clone(), preferred);
                }
            }
        }
    }

    target_id_map
}

fn component_feature_port_target_kind_map(
    model_manifest: &crate::models::ModelManifest,
) -> HashMap<String, crate::models::SelectionTargetKind> {
    model_manifest
        .selection_targets
        .iter()
        .flat_map(|target| {
            target
                .target_id
                .iter()
                .chain(target.durable_target_id.iter())
                .chain(target.canonical_target_id.iter())
                .chain(target.alias_ids.iter())
                .map(|target_id| (target_id.clone(), target.kind.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn component_feature_port_target_role(
    source_target_ids: &[String],
    target_id_map: &HashMap<String, String>,
    target_kind_map: &HashMap<String, crate::models::SelectionTargetKind>,
) -> Option<String> {
    let mut role = None;
    for source_target_id in source_target_ids {
        let resolved_target_id = target_id_map.get(source_target_id)?;
        let next_role = selection_target_kind_role(target_kind_map.get(resolved_target_id)?)?;
        if role.is_some_and(|existing| existing != next_role) {
            return None;
        }
        role = Some(next_role);
    }
    role.map(str::to_string)
}

fn selection_target_kind_role(kind: &crate::models::SelectionTargetKind) -> Option<&'static str> {
    match kind {
        crate::models::SelectionTargetKind::Object => Some("object"),
        crate::models::SelectionTargetKind::Edge => Some("edge"),
        crate::models::SelectionTargetKind::Face => Some("face"),
        crate::models::SelectionTargetKind::Part | crate::models::SelectionTargetKind::Group => {
            None
        }
    }
}

fn component_feature_port_source_ref(
    installed_source: &InstalledComponentSource,
    component_port: &ComponentPort,
) -> crate::models::SourceRef {
    let base = installed_source
        .component
        .source_ref
        .as_deref()
        .map(str::trim)
        .filter(|source_ref| !source_ref.is_empty())
        .map(|source_ref| source_ref.trim_end_matches('/').to_string())
        .unwrap_or_else(|| {
            format!(
                "packages/{}/{}/components/{}",
                installed_source.package_id,
                installed_source.version,
                installed_source.component.component_id
            )
        });

    crate::models::SourceRef {
        source_id: Some(format!(
            "{}@{}:{}",
            installed_source.package_id,
            installed_source.version,
            installed_source.component.component_id
        )),
        path: Some(format!("{}/ports/{}", base, component_port.port_id)),
        start_byte: None,
        end_byte: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::models::{
        ArtifactBundle, AssemblyComponentRef, AssemblyMate, AssemblyOutput, AssemblyOutputMode,
        ComponentDefinition, ComponentFusionZone, ComponentInterfaceValue, ComponentKeepoutVolume,
        ComponentParam, ComponentParamKind, ComponentPort, EngineKind, GeometryBackend,
        InstalledComponentSource, KeepoutVolumeKind, MacroDialect, ParamValue, ParsedParamsResult,
        PortReference, PortTypeDefinition, SelectOption, SelectValue, SourceLanguage, UiField,
        ViewerEdgePoint, ViewerEdgeTarget, ViewerFaceTarget,
    };
    use std::fs;
    use std::path::PathBuf;

    struct TestResolver {
        root: PathBuf,
    }

    impl crate::models::PathResolver for TestResolver {
        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn sample_manifest_value(alias_ids: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": crate::models::MODEL_RUNTIME_SCHEMA_VERSION,
            "modelId": "generated-abc123",
            "sourceKind": "generated",
            "engineKind": "freecad",
            "sourceLanguage": "legacyPython",
            "geometryBackend": "freecad",
            "document": {
                "documentName": "Doc",
                "documentLabel": "Doc",
                "sourcePath": null,
                "objectCount": 1,
                "warnings": [],
            },
            "parts": [{
                "partId": "part-shell",
                "freecadObjectName": "Shell",
                "label": "Shell",
                "kind": "Part::Feature",
                "semanticRole": "body",
                "viewerAssetPath": "/tmp/node-shell.stl",
                "viewerNodeIds": ["node-shell"],
                "parameterKeys": ["radius"],
                "editable": true,
                "bounds": null,
                "volume": null,
                "area": null,
            }],
            "parameterGroups": [],
            "controlPrimitives": [],
            "controlRelations": [],
            "controlViews": [],
            "advisories": [],
            "selectionTargets": [{
                "targetId": "target-shell",
                "aliasIds": alias_ids,
                "partId": "part-shell",
                "viewerNodeId": "node-shell",
                "label": "Shell",
                "kind": "object",
                "editable": true,
                "parameterKeys": [],
                "primitiveIds": [],
                "viewIds": [],
            }],
            "measurementAnnotations": [],
            "warnings": [],
            "enrichmentState": {
                "status": "none",
                "proposals": [],
            },
        })
    }

    fn sample_manifest(alias_ids: &[&str]) -> crate::models::ModelManifest {
        serde_json::from_value(sample_manifest_value(alias_ids)).expect("sample manifest")
    }

    fn sample_artifact_bundle(manifest_path: &str) -> ArtifactBundle {
        ArtifactBundle {
            schema_version: crate::models::MODEL_RUNTIME_SCHEMA_VERSION,
            model_id: "generated-abc123".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: EngineKind::Freecad,
            source_language: SourceLanguage::LegacyPython,
            geometry_backend: GeometryBackend::Freecad,
            content_hash: "hash".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: manifest_path.to_string(),
            macro_path: Some("/tmp/model.py".to_string()),
            preview_stl_path: "/tmp/model.stl".to_string(),
            viewer_assets: Vec::new(),
            edge_targets: vec![ViewerEdgeTarget {
                target_id: "alias-edge".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: vec!["legacy-edge".to_string()],
                part_id: "part-shell".to_string(),
                viewer_node_id: "node-shell".to_string(),
                label: "Shell edge".to_string(),
                editable: true,
                start: ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                end: ViewerEdgePoint {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            }],
            face_targets: vec![ViewerFaceTarget {
                target_id: "alias-face".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: vec!["legacy-face".to_string()],
                part_id: "part-shell".to_string(),
                viewer_node_id: "node-shell".to_string(),
                label: "Shell face".to_string(),
                editable: true,
                center: ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                normal: Some([0.0, 0.0, 1.0]),
                area: Some(10.0),
            }],
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        }
    }

    fn test_port(port_id: &str, frame: Option<PortFrame>) -> ComponentPort {
        ComponentPort {
            port_id: port_id.to_string(),
            type_id: "mechanical.dovetail.rail.v1".to_string(),
            target_ids: Vec::new(),
            frame,
            params: Default::default(),
            interfaces: Vec::new(),
            compatible_with: Vec::new(),
            allowed_ops: Vec::new(),
        }
    }

    fn test_component(
        component_id: &str,
        port_id: &str,
        frame: Option<PortFrame>,
    ) -> ComponentDefinition {
        let initial_params = if component_id == "frame-rail" {
            DesignParams::from([("mount_spacing".to_string(), ParamValue::Number(64.0))])
        } else {
            DesignParams::new()
        };
        ComponentDefinition {
            component_id: component_id.to_string(),
            version: "1.0.0".to_string(),
            display_name: component_id.to_string(),
            source_ref: Some(format!("components/{component_id}/source.ecky")),
            source_language: Some(SourceLanguage::EckyIrV0),
            geometry_backend: Some(GeometryBackend::Freecad),
            macro_dialect: Some(MacroDialect::EckyIrV0),
            sketches: Vec::new(),
            keepouts: vec![ComponentKeepoutVolume {
                keepout_id: "keepout".to_string(),
                label: "Keepout".to_string(),
                kind: KeepoutVolumeKind::Box,
                frame: Some(PortFrame::identity()),
                size: Some([1.0, 1.0, 1.0]),
                radius: None,
                height: None,
            }],
            fusion_zones: vec![ComponentFusionZone {
                zone_id: "zone".to_string(),
                surface_ref: "surface".to_string(),
                allowed_ops: Vec::new(),
                max_blend_radius: None,
                keepout_ids: vec!["keepout".to_string()],
            }],
            params: vec![ComponentParam {
                key: "clearance".to_string(),
                label: "Clearance".to_string(),
                kind: ComponentParamKind::Number,
                unit: Some("mm".to_string()),
            }],
            ui_spec: crate::models::UiSpec::default(),
            initial_params,
            ports: vec![test_port(port_id, frame)],
        }
    }

    fn test_installed_component_source(
        instance_id: &str,
        component_id: &str,
        port_id: &str,
        frame: Option<PortFrame>,
    ) -> InstalledAssemblyComponentSource {
        InstalledAssemblyComponentSource {
            instance_id: instance_id.to_string(),
            component_id: component_id.to_string(),
            placement_frame: None,
            installed_source: InstalledComponentSource {
                package_id: "bike.bottle-holder-kit".to_string(),
                version: "0.1.0".to_string(),
                package_display_name: "Bike Bottle Holder Kit".to_string(),
                package_dir: "/tmp/pkg".to_string(),
                component: test_component(component_id, port_id, frame),
                port_types: vec![PortTypeDefinition {
                    type_id: "mechanical.dovetail.rail.v1".to_string(),
                    display_name: "Rail".to_string(),
                    base: None,
                    interfaces: Vec::new(),
                    compatible_with: Vec::new(),
                    allowed_ops: Vec::new(),
                    params: Vec::new(),
                }],
                mate_types: Vec::new(),
                source_path: format!("/tmp/pkg/components/{component_id}/source.ecky"),
            },
        }
    }

    fn test_assembly_source(cage_frame: Option<PortFrame>) -> InstalledAssemblySource {
        InstalledAssemblySource {
            package_id: "bike.bottle-holder-kit".to_string(),
            version: "0.1.0".to_string(),
            package_display_name: "Bike Bottle Holder Kit".to_string(),
            package_dir: "/tmp/pkg".to_string(),
            assembly: AssemblyDefinition {
                assembly_id: "bottle-holder".to_string(),
                display_name: "Bottle Holder".to_string(),
                components: vec![
                    AssemblyComponentRef {
                        instance_id: "rail".to_string(),
                        component_id: "frame-rail".to_string(),
                    },
                    AssemblyComponentRef {
                        instance_id: "cage".to_string(),
                        component_id: "bottle-cage".to_string(),
                    },
                ],
                mates: vec![AssemblyMate {
                    mate_id: "rail-into-cage".to_string(),
                    type_id: "linear_insert".to_string(),
                    a: PortReference {
                        instance_id: "rail".to_string(),
                        port_id: "dovetail_rail".to_string(),
                    },
                    b: PortReference {
                        instance_id: "cage".to_string(),
                        port_id: "dovetail_slot".to_string(),
                    },
                    params: Default::default(),
                }],
                operations: Vec::new(),
                output: AssemblyOutput {
                    mode: AssemblyOutputMode::SeparateParts,
                },
            },
            port_types: Vec::new(),
            mate_types: Vec::new(),
            mate_results: Vec::new(),
            components: vec![
                test_installed_component_source(
                    "rail",
                    "frame-rail",
                    "dovetail_rail",
                    Some(PortFrame {
                        origin: [10.0, 0.0, 5.0],
                        x_axis: [1.0, 0.0, 0.0],
                        y_axis: [0.0, 1.0, 0.0],
                        z_axis: [0.0, 0.0, 1.0],
                    }),
                ),
                test_installed_component_source("cage", "bottle-cage", "dovetail_slot", cage_frame),
            ],
        }
    }

    #[test]
    fn component_params_from_parsed_params_maps_supported_ui_field_kinds() {
        let parsed = ParsedParamsResult {
            fields: vec![
                UiField::Number {
                    key: "amp".to_string(),
                    label: "Amplitude".to_string(),
                    min: Some(0.0),
                    max: Some(5.0),
                    step: Some(0.5),
                    min_from: None,
                    max_from: None,
                    frozen: false,
                },
                UiField::Select {
                    key: "profile".to_string(),
                    label: "Profile".to_string(),
                    options: vec![SelectOption {
                        label: "Bulb".to_string(),
                        value: SelectValue::String("bulb".to_string()),
                    }],
                    frozen: false,
                },
                UiField::Checkbox {
                    key: "vents".to_string(),
                    label: "Vents".to_string(),
                    frozen: false,
                },
                UiField::Image {
                    key: "reference".to_string(),
                    label: "Reference".to_string(),
                    frozen: false,
                },
            ],
            params: [
                ("amp".to_string(), ParamValue::Number(2.0)),
                (
                    "profile".to_string(),
                    ParamValue::String("bulb".to_string()),
                ),
                ("vents".to_string(), ParamValue::Boolean(true)),
                ("reference".to_string(), ParamValue::String(String::new())),
            ]
            .into_iter()
            .collect(),
        };

        assert_eq!(
            crate::component_package_runtime::component_params_from_parsed_params(&parsed),
            vec![
                ComponentParam {
                    key: "amp".to_string(),
                    label: "Amplitude".to_string(),
                    kind: ComponentParamKind::Number,
                    unit: None,
                },
                ComponentParam {
                    key: "profile".to_string(),
                    label: "Profile".to_string(),
                    kind: ComponentParamKind::Choice,
                    unit: None,
                },
                ComponentParam {
                    key: "vents".to_string(),
                    label: "Vents".to_string(),
                    kind: ComponentParamKind::Boolean,
                    unit: None,
                },
                ComponentParam {
                    key: "reference".to_string(),
                    label: "Reference".to_string(),
                    kind: ComponentParamKind::Text,
                    unit: None,
                },
            ]
        );
    }

    #[test]
    fn component_param_label_humanizes_empty_ui_labels() {
        assert_eq!(
            crate::component_package_runtime::component_param_label("base_diameter", ""),
            "Base Diameter"
        );
        assert_eq!(
            crate::component_package_runtime::component_param_label("top-hole.d", "   "),
            "Top Hole D"
        );
    }

    #[test]
    fn merge_component_render_parameters_uses_component_defaults_then_overrides() {
        let component = test_component("frame-rail", "dovetail_rail", Some(PortFrame::identity()));
        let merged = merge_component_render_parameters(
            &component,
            &DesignParams::from([("mount_spacing".to_string(), ParamValue::Number(72.0))]),
        );
        assert_eq!(merged.get("mount_spacing"), Some(&ParamValue::Number(72.0)));
    }

    #[test]
    fn merge_component_render_parameters_keeps_component_defaults_when_no_override() {
        let component = test_component("frame-rail", "dovetail_rail", Some(PortFrame::identity()));
        let merged = merge_component_render_parameters(&component, &DesignParams::new());
        assert_eq!(merged.get("mount_spacing"), Some(&ParamValue::Number(64.0)));
    }

    #[test]
    fn solve_installed_assembly_placements_aligns_exact_port_frames() {
        let source = test_assembly_source(Some(PortFrame {
            origin: [-10.0, 0.0, 5.0],
            x_axis: [-1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            z_axis: [0.0, 0.0, -1.0],
        }));

        let solve = solve_installed_assembly(&source);

        assert!(solve.mate_results.iter().all(|result| result.solved));
        assert!(solve
            .mate_results
            .iter()
            .all(|result| result.warning.is_none()));
        assert_eq!(solve.mate_results.len(), 1);
        assert!(solve.mate_results[0].solved);
        assert_eq!(solve.mate_results[0].warning, None);
        assert_eq!(
            solve.placement_frames.get("rail"),
            Some(&PortFrame::identity())
        );
        assert_eq!(
            solve.placement_frames.get("cage"),
            Some(&PortFrame {
                origin: [0.0, 0.0, 10.0],
                x_axis: [-1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
                z_axis: [0.0, 0.0, -1.0],
            })
        );
    }

    #[test]
    fn solve_installed_assembly_placements_reports_missing_frame() {
        let source = test_assembly_source(None);

        let solve = solve_installed_assembly(&source);

        assert!(solve.mate_results.iter().any(|result| !result.solved));
        assert_eq!(solve.mate_results.len(), 1);
        assert!(!solve.mate_results[0].solved);
        assert!(solve.mate_results[0]
            .warning
            .as_deref()
            .unwrap_or_default()
            .contains("port 'cage.dovetail_slot' is missing frame"));
    }

    #[test]
    fn solve_installed_assembly_reports_clearance_rule_failure() {
        let mut source = test_assembly_source(Some(PortFrame {
            origin: [-10.0, 0.0, 5.0],
            x_axis: [-1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            z_axis: [0.0, 0.0, -1.0],
        }));
        source.components[0].installed_source.component.ports[0]
            .params
            .insert(
                "clearance".to_string(),
                ComponentInterfaceValue::Number(0.6),
            );
        source.components[1].installed_source.component.ports[0]
            .params
            .insert(
                "clearance".to_string(),
                ComponentInterfaceValue::Number(0.4),
            );
        source.assembly.mates[0].params.insert(
            "clearance".to_string(),
            ComponentInterfaceValue::Number(0.5),
        );

        let solve = solve_installed_assembly(&source);

        assert!(solve.mate_results.iter().any(|result| !result.solved));
        assert_eq!(solve.mate_results.len(), 1);
        assert_eq!(solve.mate_results[0].required_clearance, Some(0.5));
        assert_eq!(solve.mate_results[0].available_clearance, Some(0.4));
        assert!(solve.mate_results[0]
            .warning
            .as_deref()
            .unwrap_or_default()
            .contains("clearance"));
    }

    #[test]
    fn plan_installed_assembly_output_supports_joined_without_operations() {
        let mut source = test_assembly_source(Some(PortFrame::identity()));
        source.assembly.output.mode = AssemblyOutputMode::JoinedAssembly;
        source.assembly.operations.clear();

        assert_eq!(
            plan_installed_assembly_output(&source.assembly),
            AssemblyOutputPlan {
                mode: AssemblyOutputPlanMode::Joined,
                fuse_group_by_instance: BTreeMap::new(),
                cut_group_by_instance: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn plan_installed_assembly_output_supports_full_instance_pure_fuse() {
        let mut source = test_assembly_source(Some(PortFrame::identity()));
        source.assembly.output.mode = AssemblyOutputMode::FusedSolid;
        source.assembly.operations = vec![crate::models::AssemblyOperation {
            operation_id: "fuse-all".to_string(),
            kind: crate::models::OperationKind::Fuse,
            target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
            port_refs: Vec::new(),
            params: Default::default(),
        }];

        let plan = plan_installed_assembly_output(&source.assembly);
        assert_eq!(plan.mode, AssemblyOutputPlanMode::FusedSolid);
        assert_eq!(
            plan.fuse_group_by_instance,
            BTreeMap::from([
                ("cage".to_string(), "fuse-group-1".to_string()),
                ("rail".to_string(), "fuse-group-1".to_string()),
            ])
        );
    }

    #[test]
    fn plan_installed_assembly_output_supports_joined_partial_pure_fuse_groups() {
        let mut source = test_assembly_source(Some(PortFrame::identity()));
        source.assembly.output.mode = AssemblyOutputMode::JoinedAssembly;
        source.assembly.components.push(AssemblyComponentRef {
            instance_id: "spacer".to_string(),
            component_id: "spacer".to_string(),
        });
        source.components.push(test_installed_component_source(
            "spacer",
            "spacer",
            "free_slot",
            Some(PortFrame::identity()),
        ));
        source.assembly.operations = vec![crate::models::AssemblyOperation {
            operation_id: "fuse-holder".to_string(),
            kind: crate::models::OperationKind::Fuse,
            target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
            port_refs: Vec::new(),
            params: Default::default(),
        }];

        let plan = plan_installed_assembly_output(&source.assembly);
        assert_eq!(plan.mode, AssemblyOutputPlanMode::Joined);
        assert_eq!(
            plan.fuse_group_by_instance,
            BTreeMap::from([
                ("cage".to_string(), "fuse-group-1".to_string()),
                ("rail".to_string(), "fuse-group-1".to_string()),
            ])
        );
        assert_eq!(plan.cut_group_by_instance, BTreeMap::new());
    }

    #[test]
    fn plan_installed_assembly_output_rejects_partial_fuse_targets() {
        let mut source = test_assembly_source(Some(PortFrame::identity()));
        source.assembly.output.mode = AssemblyOutputMode::FusedSolid;
        source.assembly.operations = vec![crate::models::AssemblyOperation {
            operation_id: "fuse-one".to_string(),
            kind: crate::models::OperationKind::Fuse,
            target_instance_ids: vec!["rail".to_string()],
            port_refs: Vec::new(),
            params: Default::default(),
        }];

        assert_eq!(
            plan_installed_assembly_output(&source.assembly),
            AssemblyOutputPlan {
                mode: AssemblyOutputPlanMode::None,
                fuse_group_by_instance: BTreeMap::new(),
                cut_group_by_instance: BTreeMap::new(),
            }
        );
    }

    #[test]
    fn plan_installed_assembly_output_supports_joined_cut_groups() {
        let mut source = test_assembly_source(Some(PortFrame::identity()));
        source.assembly.output.mode = AssemblyOutputMode::JoinedAssembly;
        source.assembly.mates.clear();
        source.assembly.operations = vec![crate::models::AssemblyOperation {
            operation_id: "cut-slot".to_string(),
            kind: crate::models::OperationKind::Cut,
            target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
            port_refs: Vec::new(),
            params: Default::default(),
        }];

        let plan = plan_installed_assembly_output(&source.assembly);

        assert_eq!(plan.mode, AssemblyOutputPlanMode::Joined);
        assert_eq!(plan.fuse_group_by_instance, BTreeMap::new());
        assert_eq!(
            plan.cut_group_by_instance,
            BTreeMap::from([
                (
                    "cage".to_string(),
                    CutGroupMembership {
                        group_id: "cut-group-1".to_string(),
                        role: CutGroupRole::Tool,
                    },
                ),
                (
                    "rail".to_string(),
                    CutGroupMembership {
                        group_id: "cut-group-1".to_string(),
                        role: CutGroupRole::Base,
                    },
                ),
            ])
        );
    }

    #[test]
    fn installed_assembly_operation_preflight_requires_fuse_capable_zone() {
        let mut source = test_assembly_source(Some(PortFrame::identity()));
        source.assembly.output.mode = AssemblyOutputMode::JoinedAssembly;
        source.assembly.operations = vec![crate::models::AssemblyOperation {
            operation_id: "fuse-holder".to_string(),
            kind: crate::models::OperationKind::Fuse,
            target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
            port_refs: Vec::new(),
            params: Default::default(),
        }];
        source.components[0].installed_source.component.fusion_zones[0].allowed_ops =
            vec![crate::models::OperationKind::Fuse];
        source.components[1].installed_source.component.fusion_zones[0].allowed_ops =
            vec![crate::models::OperationKind::Blend];

        let warning = installed_assembly_operation_preflight_warning(
            &source,
            &plan_installed_assembly_output(&source.assembly),
            &source.assembly.operations[0],
            true,
        );

        assert!(warning
            .as_deref()
            .unwrap_or_default()
            .contains("missing Fuse-capable fusion zone"));
        assert!(warning.as_deref().unwrap_or_default().contains("cage"));
    }

    #[test]
    fn installed_assembly_operation_fusion_zone_ids_map_targets() {
        let mut source = test_assembly_source(Some(PortFrame::identity()));
        source.assembly.output.mode = AssemblyOutputMode::JoinedAssembly;
        source.assembly.operations = vec![crate::models::AssemblyOperation {
            operation_id: "fuse-holder".to_string(),
            kind: crate::models::OperationKind::Fuse,
            target_instance_ids: vec!["rail".to_string(), "cage".to_string()],
            port_refs: Vec::new(),
            params: Default::default(),
        }];
        source.components[0].installed_source.component.fusion_zones[0].allowed_ops =
            vec![crate::models::OperationKind::Fuse];
        source.components[1].installed_source.component.fusion_zones[0].allowed_ops =
            vec![crate::models::OperationKind::Fuse];

        let zone_ids = installed_assembly_operation_fusion_zone_ids_by_instance(
            &source,
            &source.assembly.operations[0],
        )
        .expect("fusion zone ids");

        assert_eq!(
            zone_ids,
            BTreeMap::from([
                ("cage".to_string(), "zone".to_string()),
                ("rail".to_string(), "zone".to_string()),
            ])
        );
    }

    #[test]
    fn artifact_bundle_target_ids_include_manifest_alias_ids() {
        let root = std::env::temp_dir().join(format!(
            "ecky-component-package-target-ids-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let manifest_path = root.join("manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&sample_manifest_value(&["legacy-shell"]))
                .expect("manifest json"),
        )
        .expect("manifest write");

        let target_ids =
            artifact_bundle_target_ids(&sample_artifact_bundle(&manifest_path.to_string_lossy()))
                .expect("target ids");

        assert!(target_ids.contains("target-shell"));
        assert!(target_ids.contains("legacy-shell"));
        assert!(target_ids.contains("alias-edge"));
        assert!(target_ids.contains("legacy-edge"));
        assert!(target_ids.contains("alias-face"));
        assert!(target_ids.contains("legacy-face"));
    }

    #[test]
    fn validate_rendered_component_port_targets_accepts_manifest_alias_ids() {
        let mut installed = InstalledComponentSource {
            package_id: "pkg".to_string(),
            version: "1.0.0".to_string(),
            package_display_name: "Pkg".to_string(),
            package_dir: "/tmp/pkg".to_string(),
            component: test_component("frame-rail", "dovetail_rail", Some(PortFrame::identity())),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.dovetail.rail.v1".to_string(),
                display_name: "Rail".to_string(),
                base: None,
                interfaces: Vec::new(),
                compatible_with: Vec::new(),
                allowed_ops: Vec::new(),
                params: Vec::new(),
            }],
            mate_types: Vec::new(),
            source_path: "/tmp/pkg/component.ecky".to_string(),
        };
        installed.component.ports[0].target_ids = vec!["legacy-shell".to_string()];

        validate_rendered_component_port_targets(
            &installed,
            &sample_artifact_bundle("/tmp/manifest.json"),
            &sample_manifest(&["legacy-shell"]),
        )
        .expect("alias target id should resolve");
    }

    #[test]
    fn validate_rendered_component_port_targets_accepts_viewer_alias_ids() {
        let mut installed = InstalledComponentSource {
            package_id: "pkg".to_string(),
            version: "1.0.0".to_string(),
            package_display_name: "Pkg".to_string(),
            package_dir: "/tmp/pkg".to_string(),
            component: test_component("frame-rail", "dovetail_rail", Some(PortFrame::identity())),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.dovetail.rail.v1".to_string(),
                display_name: "Rail".to_string(),
                base: None,
                interfaces: Vec::new(),
                compatible_with: Vec::new(),
                allowed_ops: Vec::new(),
                params: Vec::new(),
            }],
            mate_types: Vec::new(),
            source_path: "/tmp/pkg/component.ecky".to_string(),
        };
        installed.component.ports[0].target_ids = vec!["legacy-edge".to_string()];

        validate_rendered_component_port_targets(
            &installed,
            &sample_artifact_bundle("/tmp/manifest.json"),
            &sample_manifest(&[]),
        )
        .expect("viewer alias target id should resolve");
    }

    #[test]
    fn populate_component_feature_graph_ports_attaches_matching_target_ports() {
        let mut installed = InstalledComponentSource {
            package_id: "pkg".to_string(),
            version: "1.0.0".to_string(),
            package_display_name: "Pkg".to_string(),
            package_dir: "/tmp/pkg".to_string(),
            component: test_component("frame-rail", "dovetail_rail", Some(PortFrame::identity())),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.dovetail.rail.v1".to_string(),
                display_name: "Rail".to_string(),
                base: None,
                interfaces: Vec::new(),
                compatible_with: Vec::new(),
                allowed_ops: Vec::new(),
                params: Vec::new(),
            }],
            mate_types: Vec::new(),
            source_path: "/tmp/pkg/component.ecky".to_string(),
        };
        installed.component.ports[0].target_ids = vec!["target-shell".to_string()];
        installed.component.ports[0].interfaces = vec!["mount".to_string()];
        installed.component.ports[0].params = BTreeMap::from([(
            "clearanceMm".to_string(),
            ComponentInterfaceValue::Number(0.3),
        )]);
        let mut manifest = sample_manifest(&[]);
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "part:part-shell".to_string(),
                kind: "part".to_string(),
                label: "Shell".to_string(),
                source_ref: None,
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "part:part-shell".to_string(),
                    output_id: "selectionTargets".to_string(),
                    target_ids: vec!["target-shell".to_string()],
                }],
                ports: Vec::new(),
            }],
        });

        let populated = populate_component_feature_graph_ports(
            &installed,
            &sample_artifact_bundle("/tmp/manifest.json"),
            &manifest,
        )
        .expect("populate ports");

        crate::models::validate_model_manifest(&populated).expect("valid manifest");
        let port = &populated
            .feature_graph
            .as_ref()
            .expect("feature graph")
            .nodes[0]
            .ports[0];
        assert_eq!(port.port_id, "dovetail_rail");
        assert_eq!(port.target_ids, vec!["target-shell".to_string()]);
        assert_eq!(port.interfaces, vec!["mount".to_string()]);
        assert_eq!(
            port.source_ref
                .as_ref()
                .and_then(|source_ref| source_ref.path.as_deref()),
            Some("components/frame-rail/source.ecky/ports/dovetail_rail")
        );
        assert_eq!(port.confidence, Some(1.0));
        assert_eq!(port.target_role.as_deref(), Some("object"));
        assert_eq!(
            port.params.get("clearanceMm"),
            Some(&ComponentInterfaceValue::Number(0.3))
        );
    }

    #[test]
    fn populate_component_feature_graph_ports_resolves_step_portable_ids_to_manifest_targets() {
        let mut installed = InstalledComponentSource {
            package_id: "pkg".to_string(),
            version: "1.0.0".to_string(),
            package_display_name: "Pkg".to_string(),
            package_dir: "/tmp/pkg".to_string(),
            component: test_component("frame-rail", "dovetail_rail", Some(PortFrame::identity())),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.dovetail.rail.v1".to_string(),
                display_name: "Rail".to_string(),
                base: None,
                interfaces: Vec::new(),
                compatible_with: Vec::new(),
                allowed_ops: Vec::new(),
                params: Vec::new(),
            }],
            mate_types: Vec::new(),
            source_path: "/tmp/pkg/component.step".to_string(),
        };
        installed.component.ports[0].target_ids = vec!["part:face:9:0-0-1:10".to_string()];
        let mut manifest = sample_manifest(&[]);
        manifest.selection_targets[0].target_id = Some("runtime-face-id".to_string());
        manifest.selection_targets[0].canonical_target_id =
            Some("part:face:0:0-0-1:10".to_string());
        manifest.selection_targets[0].kind = crate::models::SelectionTargetKind::Face;
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "part:part-shell".to_string(),
                kind: "part".to_string(),
                label: "Shell".to_string(),
                source_ref: None,
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "part:part-shell".to_string(),
                    output_id: "selectionTargets".to_string(),
                    target_ids: vec!["runtime-face-id".to_string()],
                }],
                ports: Vec::new(),
            }],
        });

        let populated = populate_component_feature_graph_ports(
            &installed,
            &sample_artifact_bundle("/tmp/manifest.json"),
            &manifest,
        )
        .expect("populate ports");

        crate::models::validate_model_manifest(&populated).expect("valid manifest");
        let port = &populated
            .feature_graph
            .as_ref()
            .expect("feature graph")
            .nodes[0]
            .ports[0];
        assert_eq!(port.target_ids, vec!["runtime-face-id".to_string()]);
        assert_eq!(
            port.source_ref
                .as_ref()
                .and_then(|source_ref| source_ref.path.as_deref()),
            Some("components/frame-rail/source.ecky/ports/dovetail_rail")
        );
        assert_eq!(port.confidence, Some(1.0));
        assert_eq!(port.target_role.as_deref(), Some("face"));
    }

    #[test]
    fn populate_component_feature_graph_ports_leaves_mixed_target_roles_empty() {
        let mut installed = InstalledComponentSource {
            package_id: "pkg".to_string(),
            version: "1.0.0".to_string(),
            package_display_name: "Pkg".to_string(),
            package_dir: "/tmp/pkg".to_string(),
            component: test_component("frame-rail", "dovetail_rail", Some(PortFrame::identity())),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.dovetail.rail.v1".to_string(),
                display_name: "Rail".to_string(),
                base: None,
                interfaces: Vec::new(),
                compatible_with: Vec::new(),
                allowed_ops: Vec::new(),
                params: Vec::new(),
            }],
            mate_types: Vec::new(),
            source_path: "/tmp/pkg/component.ecky".to_string(),
        };
        installed.component.ports[0].target_ids =
            vec!["target-edge".to_string(), "target-face".to_string()];
        let mut manifest = sample_manifest(&[]);
        let mut edge_target = manifest.selection_targets[0].clone();
        edge_target.target_id = Some("target-edge".to_string());
        edge_target.kind = crate::models::SelectionTargetKind::Edge;
        let mut face_target = manifest.selection_targets[0].clone();
        face_target.target_id = Some("target-face".to_string());
        face_target.kind = crate::models::SelectionTargetKind::Face;
        manifest.selection_targets = vec![edge_target, face_target];
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![crate::models::FeatureNode {
                feature_id: "part:part-shell".to_string(),
                kind: "part".to_string(),
                label: "Shell".to_string(),
                source_ref: None,
                dependency_ids: Vec::new(),
                output_refs: vec![crate::models::FeatureOutputRef {
                    feature_id: "part:part-shell".to_string(),
                    output_id: "selectionTargets".to_string(),
                    target_ids: vec!["target-edge".to_string(), "target-face".to_string()],
                }],
                ports: Vec::new(),
            }],
        });

        let populated = populate_component_feature_graph_ports(
            &installed,
            &sample_artifact_bundle("/tmp/manifest.json"),
            &manifest,
        )
        .expect("populate ports");

        crate::models::validate_model_manifest(&populated).expect("valid manifest");
        let port = &populated
            .feature_graph
            .as_ref()
            .expect("feature graph")
            .nodes[0]
            .ports[0];
        assert_eq!(
            port.target_ids,
            vec!["target-edge".to_string(), "target-face".to_string()]
        );
        assert_eq!(port.confidence, Some(1.0));
        assert_eq!(port.target_role, None);
    }

    #[test]
    fn populate_component_feature_graph_ports_skips_targetless_ports_on_multi_part_graph() {
        let installed = InstalledComponentSource {
            package_id: "pkg".to_string(),
            version: "1.0.0".to_string(),
            package_display_name: "Pkg".to_string(),
            package_dir: "/tmp/pkg".to_string(),
            component: test_component("frame-rail", "dovetail_rail", Some(PortFrame::identity())),
            port_types: Vec::new(),
            mate_types: Vec::new(),
            source_path: "/tmp/pkg/component.ecky".to_string(),
        };
        let mut manifest = sample_manifest(&[]);
        manifest.parts.push(crate::models::PartBinding {
            part_id: "part-lid".to_string(),
            freecad_object_name: "Lid".to_string(),
            label: "Lid".to_string(),
            kind: "Part::Feature".to_string(),
            semantic_role: Some("lid".to_string()),
            viewer_asset_path: Some("/tmp/node-lid.stl".to_string()),
            viewer_node_ids: vec!["node-lid".to_string()],
            parameter_keys: Vec::new(),
            editable: true,
            bounds: None,
            volume: None,
            area: None,
        });
        manifest.feature_graph = Some(crate::models::FeatureGraph {
            nodes: vec![
                crate::models::FeatureNode {
                    feature_id: "part:part-shell".to_string(),
                    kind: "part".to_string(),
                    label: "Shell".to_string(),
                    source_ref: None,
                    dependency_ids: Vec::new(),
                    output_refs: Vec::new(),
                    ports: Vec::new(),
                },
                crate::models::FeatureNode {
                    feature_id: "part:part-lid".to_string(),
                    kind: "part".to_string(),
                    label: "Lid".to_string(),
                    source_ref: None,
                    dependency_ids: Vec::new(),
                    output_refs: Vec::new(),
                    ports: Vec::new(),
                },
            ],
        });

        let populated = populate_component_feature_graph_ports(
            &installed,
            &sample_artifact_bundle("/tmp/manifest.json"),
            &manifest,
        )
        .expect("populate ports");

        assert!(populated
            .feature_graph
            .as_ref()
            .expect("feature graph")
            .nodes
            .iter()
            .all(|node| node.ports.is_empty()));
    }

    #[test]
    fn normalize_packaged_component_ports_prefers_public_viewer_target_ids_for_alias_input() {
        let root = std::env::temp_dir().join(format!(
            "ecky-component-package-normalize-viewer-alias-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let manifest_path = root.join("manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&sample_manifest_value(&[])).expect("manifest json"),
        )
        .expect("manifest write");

        let normalized = normalize_packaged_component_ports(
            &[ComponentPort {
                port_id: "anchor".to_string(),
                type_id: "mechanical.anchor.v1".to_string(),
                target_ids: vec!["legacy-edge".to_string()],
                frame: None,
                params: Default::default(),
                interfaces: Vec::new(),
                compatible_with: Vec::new(),
                allowed_ops: Vec::new(),
            }],
            &sample_artifact_bundle(&manifest_path.to_string_lossy()),
            true,
        )
        .expect("viewer alias target id should normalize");

        assert_eq!(normalized[0].target_ids, vec!["alias-edge".to_string()]);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn normalize_packaged_component_ports_prefers_public_viewer_target_ids_for_canonical_input() {
        let root = std::env::temp_dir().join(format!(
            "ecky-component-package-normalize-viewer-canonical-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let manifest_path = root.join("manifest.json");
        let mut manifest_value = sample_manifest_value(&[]);
        manifest_value["selectionTargets"][0]["canonicalTargetId"] =
            serde_json::Value::String("canonical-shell".to_string());
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest_value).expect("manifest json"),
        )
        .expect("manifest write");

        let mut bundle = sample_artifact_bundle(&manifest_path.to_string_lossy());
        bundle.edge_targets[0].canonical_target_id = Some("canonical-edge".to_string());

        let normalized = normalize_packaged_component_ports(
            &[ComponentPort {
                port_id: "anchor".to_string(),
                type_id: "mechanical.anchor.v1".to_string(),
                target_ids: vec!["canonical-edge".to_string()],
                frame: None,
                params: Default::default(),
                interfaces: Vec::new(),
                compatible_with: Vec::new(),
                allowed_ops: Vec::new(),
            }],
            &bundle,
            true,
        )
        .expect("viewer canonical target id should normalize");

        assert_eq!(normalized[0].target_ids, vec!["alias-edge".to_string()]);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn normalize_packaged_component_ports_prefers_public_viewer_target_ids_for_durable_input() {
        let root = std::env::temp_dir().join(format!(
            "ecky-component-package-normalize-viewer-durable-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let manifest_path = root.join("manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&sample_manifest_value(&[])).expect("manifest json"),
        )
        .expect("manifest write");

        let mut bundle = sample_artifact_bundle(&manifest_path.to_string_lossy());
        bundle.edge_targets[0].durable_target_id = Some("durable-edge".to_string());

        let normalized = normalize_packaged_component_ports(
            &[ComponentPort {
                port_id: "anchor".to_string(),
                type_id: "mechanical.anchor.v1".to_string(),
                target_ids: vec!["durable-edge".to_string()],
                frame: None,
                params: Default::default(),
                interfaces: Vec::new(),
                compatible_with: Vec::new(),
                allowed_ops: Vec::new(),
            }],
            &bundle,
            true,
        )
        .expect("viewer durable target id should normalize");

        assert_eq!(normalized[0].target_ids, vec!["alias-edge".to_string()]);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn write_artifact_bundle_component_package_project_round_trips_build123d_exact_target_ids_through_step(
    ) {
        let root = std::env::temp_dir().join(format!(
            "ecky-component-package-build123d-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let resolver = TestResolver { root: root.clone() };
        let build123d_capability = crate::runtime_capabilities::probe_build123d_runtime(&resolver);
        let freecad_capability =
            crate::runtime_capabilities::probe_freecad_runtime(None, &resolver);
        if !build123d_capability.available || !freecad_capability.available {
            let _ = fs::remove_dir_all(&root);
            return;
        }

        let source = r#"(model
                (part body
                  (box 10.246912 7.135791 3.864209)))"#;
        let bundle = crate::build123d::render_model_with_sources(
            &crate::ecky_ir::lower_to_build123d(source).expect("lower"),
            Some(source),
            &DesignParams::new(),
            &resolver,
            SourceLanguage::EckyIrV0,
        )
        .expect("build123d render");
        let edge_target_id = bundle
            .edge_targets
            .first()
            .map(|target| target.target_id.clone())
            .expect("edge target");
        let face_target_id = bundle
            .face_targets
            .first()
            .map(|target| target.target_id.clone())
            .expect("face target");
        let build123d_manifest = read_model_manifest_from_path(Path::new(&bundle.manifest_path))
            .expect("bundle manifest");
        let expected_edge_target_id = build123d_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == crate::models::SelectionTargetKind::Edge)
            .and_then(|target| target.target_id.clone())
            .expect("edge target");
        let expected_face_target_id = build123d_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == crate::models::SelectionTargetKind::Face)
            .and_then(|target| target.target_id.clone())
            .expect("face target");
        let project_dir = root.join("pkg");
        fs::create_dir_all(&project_dir).expect("project dir");

        let package = write_artifact_bundle_component_package_project_impl(
            &project_dir,
            ArtifactBundleComponentPackageRequest {
                package_id: "pkg.build123d".to_string(),
                version: "1.0.0".to_string(),
                display_name: "Build123d Pkg".to_string(),
                tags: Vec::new(),
                component_id: "body".to_string(),
                component_version: "1.0.0".to_string(),
                component_display_name: "Body".to_string(),
                source_ref: Some("components/body/source.step".to_string()),
                artifact_bundle: bundle,
                port_types: vec![PortTypeDefinition {
                    type_id: "mechanical.anchor.v1".to_string(),
                    display_name: "Anchor".to_string(),
                    base: None,
                    interfaces: Vec::new(),
                    compatible_with: Vec::new(),
                    allowed_ops: Vec::new(),
                    params: Vec::new(),
                }],
                params: Vec::new(),
                ui_spec: crate::models::UiSpec::default(),
                initial_params: DesignParams::new(),
                ports: vec![ComponentPort {
                    port_id: "anchor".to_string(),
                    type_id: "mechanical.anchor.v1".to_string(),
                    target_ids: vec![edge_target_id, face_target_id],
                    frame: None,
                    params: Default::default(),
                    interfaces: Vec::new(),
                    compatible_with: Vec::new(),
                    allowed_ops: Vec::new(),
                }],
            },
        )
        .expect("package write");

        assert_eq!(package.components.len(), 1);
        assert_eq!(package.components[0].ports.len(), 1);
        assert_eq!(package.components[0].ports[0].target_ids.len(), 2);
        assert_eq!(
            package.components[0].ports[0].target_ids,
            vec![expected_edge_target_id, expected_face_target_id]
        );
        let source_path = project_dir.join("components/body/source.step");
        assert!(source_path.is_file());

        let imported_bundle =
            crate::freecad::import_step(source_path.to_string_lossy().as_ref(), None, &resolver)
                .expect("import packaged step");
        let imported_manifest =
            read_model_manifest_from_path(Path::new(&imported_bundle.manifest_path))
                .expect("imported manifest");
        let installed_source = InstalledComponentSource {
            package_id: package.package_id.clone(),
            version: package.version.clone(),
            package_display_name: package.display_name.clone(),
            package_dir: project_dir.to_string_lossy().to_string(),
            component: package.components[0].clone(),
            port_types: package.port_types.clone(),
            mate_types: package.mate_types.clone(),
            source_path: source_path.to_string_lossy().to_string(),
        };

        validate_rendered_component_port_targets(
            &installed_source,
            &imported_bundle,
            &imported_manifest,
        )
        .expect("round-trip target ids");

        let _ = fs::remove_dir_all(&root);
    }
}
