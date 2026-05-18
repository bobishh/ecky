use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::models::{AppError, AppResult};

const PYTHON_SITE_PACKAGES: &str = "lib/python3.12/site-packages";
const OCP_PACKAGE: &str = "OCP";
const OCP_DYLIBS: &str = ".dylibs";
const OCP_INSTALL_NAME_PREFIX: &str = "/DLC/OCP/.dylibs";
const ECKY_OCCT_ROOT: &str = "ECKY_OCCT_ROOT";
const OCCT_RUNTIME_SUBDIR: &str = "runtime/occt";
const OCCT_MANIFEST_FILE: &str = "manifest.json";

pub const REQUIRED_OCCT_HEADERS: &[&str] = &[
    "BRepAlgoAPI_Common.hxx",
    "BRepAlgoAPI_Cut.hxx",
    "BRepAlgoAPI_Fuse.hxx",
    "Bnd_Box.hxx",
    "BRepAdaptor_Curve.hxx",
    "BRepAdaptor_Surface.hxx",
    "BRepBndLib.hxx",
    "BRepGProp.hxx",
    "BRepFilletAPI_MakeChamfer.hxx",
    "BRepFilletAPI_MakeFillet.hxx",
    "BRepBuilderAPI_GTransform.hxx",
    "BRepPrimAPI_MakeBox.hxx",
    "BRepPrimAPI_MakeCone.hxx",
    "BRepPrimAPI_MakeCylinder.hxx",
    "BRepPrimAPI_MakePrism.hxx",
    "BRepPrimAPI_MakeRevol.hxx",
    "BRepPrimAPI_MakeSphere.hxx",
    "BRepBuilderAPI_Transform.hxx",
    "BRepBuilderAPI_MakeEdge.hxx",
    "BRepBuilderAPI_MakeFace.hxx",
    "BRepBuilderAPI_MakePolygon.hxx",
    "BRepBuilderAPI_MakeWire.hxx",
    "BRep_Builder.hxx",
    "BRepMesh_IncrementalMesh.hxx",
    "BRepOffsetAPI_MakeOffset.hxx",
    "BRepOffsetAPI_MakeOffsetShape.hxx",
    "BRepOffsetAPI_MakePipeShell.hxx",
    "BRepOffsetAPI_MakeThickSolid.hxx",
    "BRepOffsetAPI_ThruSections.hxx",
    "BRepOffset_Mode.hxx",
    "BRepTools.hxx",
    "GeomAbs_JoinType.hxx",
    "GeomAbs_SurfaceType.hxx",
    "GProp_GProps.hxx",
    "GC_MakeArcOfCircle.hxx",
    "Geom_BezierCurve.hxx",
    "Geom_BSplineCurve.hxx",
    "Geom_TrimmedCurve.hxx",
    "GeomAPI_PointsToBSpline.hxx",
    "IFSelect_ReturnStatus.hxx",
    "STEPControl_Writer.hxx",
    "StlAPI_Writer.hxx",
    "TColgp_Array1OfPnt.hxx",
    "TopAbs_ShapeEnum.hxx",
    "TopExp_Explorer.hxx",
    "TopoDS.hxx",
    "TopoDS_Compound.hxx",
    "TopoDS_Edge.hxx",
    "TopoDS_Face.hxx",
    "TopoDS_Shape.hxx",
    "TopoDS_Wire.hxx",
    "TopTools_ListOfShape.hxx",
    "gp_Ax1.hxx",
    "gp_Ax2.hxx",
    "gp_Circ.hxx",
    "gp_Dir.hxx",
    "gp_GTrsf.hxx",
    "gp_Pnt.hxx",
    "gp_Trsf.hxx",
    "gp_Vec.hxx",
];

pub const REQUIRED_OCCT_LIBS: &[&str] = &[
    "TKernel",
    "TKMath",
    "TKG2d",
    "TKG3d",
    "TKGeomBase",
    "TKGeomAlgo",
    "TKBRep",
    "TKTopAlgo",
    "TKBO",
    "TKBool",
    "TKPrim",
    "TKOffset",
    "TKFillet",
    "TKMesh",
    "TKDE",
    "TKDESTEP",
    "TKDESTL",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OcctManifest {
    schema_version: Option<String>,
    platform: Option<String>,
    arch: Option<String>,
    occt_version: Option<String>,
    abi_tag: Option<String>,
    include_dir: Option<String>,
    lib_dir: Option<String>,
    required_headers: Option<Vec<String>>,
    required_libraries: Option<Vec<String>>,
    library_hashes: Option<HashMap<String, String>>,
}

impl OcctManifest {
    fn required_headers(&self) -> Option<&[String]> {
        self.required_headers.as_deref()
    }

    fn required_libraries(&self) -> Option<&[String]> {
        self.required_libraries.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectOcctSdkLayout {
    pub runtime_root: PathBuf,
    pub ocp_root: Option<PathBuf>,
    pub dylib_dir: Option<PathBuf>,
    pub include_dir: Option<PathBuf>,
    pub missing_headers: Vec<String>,
    pub missing_libs: Vec<String>,
    pub install_name_prefix: &'static str,
}

impl DirectOcctSdkLayout {
    pub fn can_compile_native_shim(&self) -> bool {
        self.ocp_root.is_some()
            && self.dylib_dir.is_some()
            && self.include_dir.is_some()
            && self.missing_headers.is_empty()
            && self.missing_libs.is_empty()
            && self.manifest_blockers().is_empty()
    }

    pub fn blocker_summary(&self) -> Vec<String> {
        let mut blockers = self.manifest_blockers();
        if !blockers.is_empty() {
            blockers.extend(self.legacy_blockers());
            return blockers;
        }
        let mut legacy = self.legacy_blockers();
        blockers.append(&mut legacy);
        blockers
    }

    fn manifest_blockers(&self) -> Vec<String> {
        let Some(manifest_path) = manifest_path(&self.runtime_root) else {
            return Vec::new();
        };
        if !manifest_path.is_file() {
            return Vec::new();
        }
        match load_manifest(&manifest_path) {
            Ok(manifest) => manifest_blockers_with_layout(&manifest_path, &manifest, self),
            Err(err) => vec![err.to_string()],
        }
    }

    fn legacy_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.ocp_root.is_none() {
            blockers.push(format!(
                "OCP package directory missing; runtime root: '{}'; checked package candidates: {}",
                self.runtime_root.display(),
                self.describe_ocp_root_candidates()
            ));
        }
        if self.dylib_dir.is_none() {
            let dylib_path = self
                .ocp_root
                .as_ref()
                .map(|root| root.join(OCP_DYLIBS))
                .unwrap_or_else(|| {
                    self.runtime_root
                        .join(PYTHON_SITE_PACKAGES)
                        .join(OCP_PACKAGE)
                        .join(OCP_DYLIBS)
                });
            blockers.push(format!(
                "OCP .dylibs directory missing at '{}'; runtime root: '{}'",
                dylib_path.display(),
                self.runtime_root.display()
            ));
        }
        if self.include_dir.is_none() {
            blockers.push(format!(
                "OCCT include directory missing or empty; runtime root: '{}'; checked include candidates: {}; run `npm run occt:prepare` from the repo root",
                self.runtime_root.display(),
                self.describe_include_dir_candidates()
            ));
        }
        if let Some(include_dir) = self
            .include_dir
            .as_ref()
            .filter(|_| !self.missing_headers.is_empty())
        {
            blockers.push(format!(
                "OCCT headers missing in selected include directory '{}': {}",
                include_dir.display(),
                self.missing_headers.join(", ")
            ));
        }
        if !self.missing_libs.is_empty() {
            blockers.push(format!(
                "OCCT dylibs missing: {}",
                self.missing_libs.join(", ")
            ));
        }
        blockers
    }

    fn include_dir(&self) -> AppResult<&Path> {
        self.include_dir.as_deref().ok_or_else(|| {
            AppError::validation("Direct OCCT native shim blocked: OCCT include directory missing.")
        })
    }

    fn dylib_dir(&self) -> AppResult<&Path> {
        self.dylib_dir.as_deref().ok_or_else(|| {
            AppError::validation("Direct OCCT native shim blocked: OCP dylib directory missing.")
        })
    }

    fn describe_include_dir_candidates(&self) -> String {
        include_dir_candidates(&self.runtime_root, self.ocp_root.as_deref())
            .iter()
            .map(|candidate| {
                let status = if Some(candidate) == self.include_dir.as_ref() {
                    format!(
                        "selected with {}/{} required OCCT headers",
                        required_header_count(candidate),
                        REQUIRED_OCCT_HEADERS.len()
                    )
                } else if !candidate.is_dir() {
                    "missing".to_string()
                } else {
                    let count = required_header_count(candidate);
                    if count == 0 {
                        "present without required OCCT headers".to_string()
                    } else {
                        format!(
                            "present with {count}/{} required OCCT headers",
                            REQUIRED_OCCT_HEADERS.len()
                        )
                    }
                };
                format!("{status} '{}'", candidate.display())
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn describe_ocp_root_candidates(&self) -> String {
        ocp_root_candidates(&self.runtime_root)
            .iter()
            .map(|candidate| {
                let status = if Some(candidate) == self.ocp_root.as_ref() {
                    "selected"
                } else if candidate.is_dir() {
                    "present"
                } else {
                    "missing"
                };
                format!("{status} '{}'", candidate.display())
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeExportOutcome {
    Blocked {
        blockers: Vec<String>,
    },
    Exported {
        step_path: PathBuf,
        stl_path: PathBuf,
    },
}

pub fn inspect_build123d_ocp_runtime(runtime_root: impl AsRef<Path>) -> DirectOcctSdkLayout {
    let runtime_root = runtime_root.as_ref().to_path_buf();
    if let Some(occt_root) = ecky_occt_root_from_env() {
        let manifest_root = occt_root.join(OCCT_RUNTIME_SUBDIR);
        if let Some(layout) = inspect_occt_manifest_runtime(manifest_root, true) {
            return layout;
        }
    }

    if let Some(manifest_root) = discover_runtime_occt_root(&runtime_root) {
        if let Some(layout) = inspect_occt_manifest_runtime(manifest_root, false) {
            return layout;
        }
    }

    let ocp_root = find_ocp_root(&runtime_root);
    let dylib_dir = ocp_root
        .as_ref()
        .and_then(|root| existing_dir(root.join(OCP_DYLIBS)));
    let include_dir = find_include_dir(&include_dir_candidates(&runtime_root, ocp_root.as_deref()));

    let missing_headers = REQUIRED_OCCT_HEADERS
        .iter()
        .filter(|header| {
            include_dir
                .as_ref()
                .map(|dir| !dir.join(header).is_file())
                .unwrap_or(true)
        })
        .map(|header| (*header).to_string())
        .collect::<Vec<_>>();
    let missing_libs = REQUIRED_OCCT_LIBS
        .iter()
        .filter(|lib| {
            dylib_dir
                .as_ref()
                .map(|dir| find_versioned_library_path(dir, lib).is_none())
                .unwrap_or(true)
        })
        .map(|lib| (*lib).to_string())
        .collect::<Vec<_>>();

    DirectOcctSdkLayout {
        runtime_root,
        ocp_root,
        dylib_dir,
        include_dir,
        missing_headers,
        missing_libs,
        install_name_prefix: OCP_INSTALL_NAME_PREFIX,
    }
}

pub fn bundled_build123d_runtime_root_from_repo(repo_root: impl AsRef<Path>) -> PathBuf {
    repo_root.as_ref().join(".dist").join("build123d-runtime")
}

pub fn bundled_occt_runtime_root_from_repo(repo_root: impl AsRef<Path>) -> PathBuf {
    repo_root
        .as_ref()
        .join(".dist")
        .join("runtime")
        .join("occt")
}

pub fn run_native_box_export_probe(
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    let output_dir = output_dir.as_ref();
    run_native_export_source(
        layout,
        output_dir,
        "direct_occt_box_probe.cpp",
        "direct_occt_box_probe",
        output_dir.join("box.step"),
        output_dir.join("box.stl"),
        native_box_export_probe_source(&output_dir.join("box.step"), &output_dir.join("box.stl")),
    )
}

pub fn run_native_export_source(
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
    source_file_name: &str,
    exe_file_name: &str,
    step_path: PathBuf,
    stl_path: PathBuf,
    source: String,
) -> AppResult<NativeExportOutcome> {
    let blockers = layout.blocker_summary();
    if !blockers.is_empty() {
        return Ok(NativeExportOutcome::Blocked { blockers });
    }

    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir).map_err(|err| {
        AppError::validation(format!(
            "Direct OCCT native shim could not create output dir '{}': {}",
            output_dir.display(),
            err
        ))
    })?;

    let source_path = output_dir.join(source_file_name);
    let exe_path = output_dir.join(exe_file_name);
    fs::write(&source_path, source).map_err(|err| {
        AppError::validation(format!(
            "Direct OCCT native shim could not write '{}': {}",
            source_path.display(),
            err
        ))
    })?;

    let compiler = std::env::var_os("CXX").unwrap_or_else(|| OsString::from("c++"));
    let mut command = Command::new(compiler);
    let dylib_paths = required_dylib_paths(layout.dylib_dir()?)?;
    command
        .arg("-std=c++17")
        .arg("-I")
        .arg(layout.include_dir()?)
        .arg(&source_path)
        .args(&dylib_paths);
    #[cfg(not(target_os = "windows"))]
    {
        command.arg("-Wl,-headerpad_max_install_names");
    }
    for rpath in runtime_rpath_dirs(layout) {
        command.arg("-Wl,-rpath");
        command.arg("-Wl,".to_string() + &rpath.to_string_lossy());
    }
    command.arg("-o").arg(&exe_path);

    let output = command.output().map_err(|err| {
        AppError::validation(format!(
            "Direct OCCT native shim compiler failed to start: {}",
            err
        ))
    })?;
    if !output.status.success() {
        return Err(AppError::with_details(
            crate::models::AppErrorCode::Validation,
            "Direct OCCT native shim compile failed.",
            format!(
                "stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    rewrite_probe_install_names(&exe_path, &dylib_paths)?;

    let run = Command::new(&exe_path).output().map_err(|err| {
        AppError::validation(format!(
            "Direct OCCT native shim probe failed to start '{}': {}",
            exe_path.display(),
            err
        ))
    })?;
    if !run.status.success() {
        return Err(AppError::with_details(
            crate::models::AppErrorCode::Validation,
            "Direct OCCT native shim probe failed.",
            format!(
                "stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            ),
        ));
    }

    Ok(NativeExportOutcome::Exported {
        step_path,
        stl_path,
    })
}

fn existing_dir(path: PathBuf) -> Option<PathBuf> {
    path.is_dir().then_some(path)
}

fn ecky_occt_root_from_env() -> Option<PathBuf> {
    std::env::var_os(ECKY_OCCT_ROOT)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn discover_runtime_occt_root(runtime_root: &Path) -> Option<PathBuf> {
    let candidate = runtime_root.join(OCCT_RUNTIME_SUBDIR);
    candidate
        .join(OCCT_MANIFEST_FILE)
        .is_file()
        .then_some(candidate)
        .or_else(|| {
            runtime_root
                .parent()
                .map(|parent| parent.join("runtime").join("occt"))
                .filter(|candidate| candidate.join(OCCT_MANIFEST_FILE).is_file())
        })
}

fn manifest_path(runtime_root: &Path) -> Option<PathBuf> {
    let manifest_file = runtime_root.join(OCCT_MANIFEST_FILE);
    manifest_file.is_file().then_some(manifest_file)
}

fn load_manifest(path: &Path) -> AppResult<OcctManifest> {
    let raw = fs::read_to_string(path).map_err(|err| {
        AppError::validation(format!(
            "Could not read OCCT manifest '{}': {}",
            path.display(),
            err
        ))
    })?;
    let manifest: OcctManifest = serde_json::from_str(&raw).map_err(|err| {
        AppError::validation(format!(
            "Invalid OCCT manifest '{}': {}",
            path.display(),
            err
        ))
    })?;
    Ok(manifest)
}

fn inspect_occt_manifest_runtime(
    manifest_root: PathBuf,
    strict: bool,
) -> Option<DirectOcctSdkLayout> {
    let manifest_path = manifest_root.join(OCCT_MANIFEST_FILE);
    let manifest = match load_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(_err) => {
            if strict {
                return Some(DirectOcctSdkLayout {
                    runtime_root: manifest_root.clone(),
                    ocp_root: Some(manifest_root),
                    dylib_dir: None,
                    include_dir: None,
                    missing_headers: Vec::new(),
                    missing_libs: Vec::new(),
                    install_name_prefix: OCP_INSTALL_NAME_PREFIX,
                });
            }
            return None;
        }
    };

    let include_dir = manifest
        .include_dir
        .as_deref()
        .map(|dir| resolve_relative_path(&manifest_root, dir))
        .filter(|dir| dir.is_dir());
    let dylib_dir = manifest
        .lib_dir
        .as_deref()
        .map(|dir| resolve_relative_path(&manifest_root, dir))
        .filter(|dir| dir.is_dir());
    let required_headers: Vec<&str> = manifest
        .required_headers()
        .map(|values| values.iter().map(String::as_str).collect())
        .unwrap_or_else(|| REQUIRED_OCCT_HEADERS.to_vec());
    let required_libs: Vec<&str> = manifest
        .required_libraries()
        .map(|values| values.iter().map(String::as_str).collect())
        .unwrap_or_else(|| REQUIRED_OCCT_LIBS.to_vec());

    let missing_headers = required_headers
        .iter()
        .filter(|header| {
            include_dir
                .as_ref()
                .map(|dir| !dir.join(header).is_file())
                .unwrap_or(true)
        })
        .map(|header| (*header).to_string())
        .collect::<Vec<_>>();
    let missing_libs = required_libs
        .iter()
        .filter(|lib| {
            dylib_dir
                .as_ref()
                .map(|dir| find_versioned_library_path(dir, lib).is_none())
                .unwrap_or(true)
        })
        .map(|lib| (*lib).to_string())
        .collect::<Vec<_>>();

    let layout = DirectOcctSdkLayout {
        runtime_root: manifest_root.clone(),
        ocp_root: Some(manifest_root),
        include_dir,
        dylib_dir,
        missing_headers,
        missing_libs,
        install_name_prefix: OCP_INSTALL_NAME_PREFIX,
    };

    if strict || layout.can_compile_native_shim() {
        Some(layout)
    } else if manifest_blockers(&manifest_path, &manifest).is_empty() {
        Some(layout)
    } else {
        None
    }
}

fn resolve_relative_path(root: &Path, value: &str) -> PathBuf {
    let value = Path::new(value);
    if value.is_absolute() {
        value.to_path_buf()
    } else {
        root.join(value)
    }
}

fn manifest_blockers(manifest_path: &Path, manifest: &OcctManifest) -> Vec<String> {
    manifest_blockers_with_layout(
        manifest_path,
        manifest,
        &DirectOcctSdkLayout {
            runtime_root: manifest_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
            ocp_root: Some(
                manifest_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf(),
            ),
            dylib_dir: None,
            include_dir: None,
            missing_headers: Vec::new(),
            missing_libs: Vec::new(),
            install_name_prefix: OCP_INSTALL_NAME_PREFIX,
        },
    )
}

fn manifest_blockers_with_layout(
    manifest_path: &Path,
    manifest: &OcctManifest,
    layout: &DirectOcctSdkLayout,
) -> Vec<String> {
    let mut blockers = Vec::new();

    let missing_fields: Vec<&str> = [
        ("schemaVersion", manifest.schema_version.as_ref().is_some()),
        ("platform", manifest.platform.as_ref().is_some()),
        ("arch", manifest.arch.as_ref().is_some()),
        ("occtVersion", manifest.occt_version.as_ref().is_some()),
        ("abiTag", manifest.abi_tag.as_ref().is_some()),
        ("includeDir", manifest.include_dir.as_ref().is_some()),
        ("libDir", manifest.lib_dir.as_ref().is_some()),
        (
            "requiredHeaders",
            manifest
                .required_headers
                .as_ref()
                .is_some_and(|required_headers| !required_headers.is_empty()),
        ),
        (
            "requiredLibraries",
            manifest
                .required_libraries
                .as_ref()
                .is_some_and(|required_libraries| !required_libraries.is_empty()),
        ),
        ("libraryHashes", manifest.library_hashes.as_ref().is_some()),
    ]
    .iter()
    .filter_map(|(name, present)| (!*present).then_some(*name))
    .collect();

    if !missing_fields.is_empty() {
        blockers.push(
            serde_json::json!({
                "kind": "manifestMissingFields",
                "manifest": manifest_path.display().to_string(),
                "missingFields": missing_fields,
            })
            .to_string(),
        );
    }

    if let (Some(platform), Some(arch)) = (manifest.platform.as_deref(), manifest.arch.as_deref()) {
        if platform != current_runtime_platform() || arch != std::env::consts::ARCH {
            blockers.push(
                serde_json::json!({
                    "kind": "platformMismatch",
                    "manifest": manifest_path.display().to_string(),
                    "manifestPlatform": platform,
                    "manifestArch": arch,
                    "runtimePlatform": current_runtime_platform(),
                    "runtimeArch": std::env::consts::ARCH,
                })
                .to_string(),
            );
        }
    }

    if let Some(manifest_abi) = manifest.abi_tag.as_deref() {
        let runtime_abi = current_runtime_abi_tag();
        if manifest_abi != runtime_abi {
            blockers.push(
                serde_json::json!({
                    "kind": "abiMismatch",
                    "manifest": manifest_path.display().to_string(),
                    "manifestAbiTag": manifest_abi,
                    "runtimeAbiTag": runtime_abi,
                })
                .to_string(),
            );
        }
    }

    if !layout.missing_headers.is_empty() {
        blockers.push(
            serde_json::json!({
                "kind": "missingHeaders",
                "manifest": manifest_path.display().to_string(),
                "required": required_manifest_headers(manifest),
                "missing": layout.missing_headers,
            })
            .to_string(),
        );
    }
    if !layout.missing_libs.is_empty() {
        blockers.push(
            serde_json::json!({
                "kind": "missingLibraries",
                "manifest": manifest_path.display().to_string(),
                "required": required_manifest_libraries(manifest),
                "missing": layout.missing_libs,
            })
            .to_string(),
        );
    }

    blockers
}

fn required_manifest_headers(manifest: &OcctManifest) -> Vec<String> {
    manifest
        .required_headers()
        .map(|headers| headers.iter().map(|header| header.to_string()).collect())
        .unwrap_or_else(|| {
            REQUIRED_OCCT_HEADERS
                .iter()
                .map(|header| (*header).to_string())
                .collect()
        })
}

fn required_manifest_libraries(manifest: &OcctManifest) -> Vec<String> {
    manifest
        .required_libraries()
        .map(|libraries| libraries.iter().map(|lib| lib.to_string()).collect())
        .unwrap_or_else(|| {
            REQUIRED_OCCT_LIBS
                .iter()
                .map(|lib| (*lib).to_string())
                .collect()
        })
}

fn current_runtime_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        std::env::consts::OS
    }
}

fn current_runtime_abi_tag() -> &'static str {
    #[cfg(all(target_os = "windows", target_env = "gnu"))]
    {
        "windows-gnu"
    }
    #[cfg(all(target_os = "windows", target_env = "msvc"))]
    {
        "windows-msvc"
    }
    #[cfg(target_os = "linux")]
    {
        if cfg!(target_env = "musl") {
            "linux-musl"
        } else {
            "linux-gnu"
        }
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "unknown"
    }
}

fn include_dir_candidates(runtime_root: &Path, ocp_root: Option<&Path>) -> Vec<PathBuf> {
    let runtime_candidates = [
        runtime_root.join("include").join("opencascade"),
        runtime_root.join("include"),
        runtime_root
            .join("Library")
            .join("include")
            .join("opencascade"),
    ];
    runtime_candidates
        .into_iter()
        .chain(ocp_root.into_iter().flat_map(|root| {
            [
                root.join("include"),
                root.join("include").join("opencascade"),
            ]
        }))
        .collect()
}

fn ocp_root_candidates(runtime_root: &Path) -> Vec<PathBuf> {
    site_packages_dir_candidates(runtime_root)
        .into_iter()
        .map(|site_packages| site_packages.join(OCP_PACKAGE))
        .collect()
}

fn site_packages_dir_candidates(runtime_root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![runtime_root.join(PYTHON_SITE_PACKAGES)];
    let lib_dir = runtime_root.join("lib");
    if let Ok(entries) = fs::read_dir(&lib_dir) {
        let mut dynamic = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name.starts_with("python"))
                        .unwrap_or(false)
            })
            .map(|path| path.join("site-packages"))
            .collect::<Vec<_>>();
        dynamic.sort();
        candidates.extend(dynamic);
    }
    candidates.dedup();
    candidates
}

fn find_ocp_root(runtime_root: &Path) -> Option<PathBuf> {
    ocp_root_candidates(runtime_root)
        .into_iter()
        .find_map(existing_dir)
}

fn required_header_count(candidate: &Path) -> usize {
    REQUIRED_OCCT_HEADERS
        .iter()
        .filter(|header| candidate.join(header).is_file())
        .count()
}

fn find_include_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    let mut best_candidate = None;
    let mut best_count = 0usize;
    for candidate in candidates {
        if !candidate.is_dir() {
            continue;
        }
        let count = required_header_count(candidate);
        if count > best_count {
            best_count = count;
            best_candidate = Some(candidate.clone());
        }
    }
    best_candidate
}

fn library_file_extensions() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["dylib"]
    }
    #[cfg(target_os = "linux")]
    {
        &["so"]
    }
    #[cfg(target_os = "windows")]
    {
        &["lib", "dll"]
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        &["dylib", "so"]
    }
}

fn find_versioned_library_path(dir: &Path, lib: &str) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| is_library_candidate(name, lib))
        })
        .min_by_key(|path| {
            if cfg!(windows) {
                let is_import_lib = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".lib"));
                usize::from(!is_import_lib)
            } else {
                0
            }
        })
}

fn is_library_candidate(file_name: &str, lib: &str) -> bool {
    let prefixes = ["", "lib"];
    let extensions = library_file_extensions();
    prefixes.iter().any(|prefix| {
        extensions.iter().any(|ext| {
            let primary = format!("{prefix}{lib}.{ext}");
            let versioned_prefix = format!("{prefix}{lib}.");
            file_name == primary
                || file_name.starts_with(&format!("{primary}."))
                    && file_name.ends_with(&format!(".{ext}"))
                || file_name.starts_with(&versioned_prefix)
                    && file_name.ends_with(&format!(".{ext}"))
        })
    })
}

fn required_dylib_paths(dir: &Path) -> AppResult<Vec<PathBuf>> {
    REQUIRED_OCCT_LIBS
        .iter()
        .map(|lib| {
            find_versioned_library_path(dir, lib).ok_or_else(|| {
                AppError::validation(format!(
                    "Direct OCCT native shim missing OCP dylib for `{}` in '{}'.",
                    lib,
                    dir.display()
                ))
            })
        })
        .collect()
}

fn runtime_rpath_dirs(layout: &DirectOcctSdkLayout) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(dylib_dir) = &layout.dylib_dir {
            dirs.push(dylib_dir.clone());
        }
        let vtk_dir = layout
            .runtime_root
            .join(PYTHON_SITE_PACKAGES)
            .join("vtkmodules")
            .join(OCP_DYLIBS);
        if vtk_dir.is_dir() {
            dirs.push(vtk_dir);
        }
    }
    dirs
}

#[cfg(target_os = "macos")]
fn rewrite_probe_install_names(exe_path: &Path, dylib_paths: &[PathBuf]) -> AppResult<()> {
    for dylib_path in dylib_paths {
        let Some(file_name) = dylib_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let old_name = format!("{OCP_INSTALL_NAME_PREFIX}/{file_name}");
        let output = Command::new("install_name_tool")
            .arg("-change")
            .arg(&old_name)
            .arg(dylib_path)
            .arg(exe_path)
            .output()
            .map_err(|err| {
                AppError::validation(format!(
                    "Direct OCCT native shim install-name rewrite failed to start: {}",
                    err
                ))
            })?;
        if !output.status.success() {
            return Err(AppError::with_details(
                crate::models::AppErrorCode::Validation,
                "Direct OCCT native shim install-name rewrite failed.",
                format!(
                    "rewrite: {} -> {}\nstdout: {}\nstderr: {}",
                    old_name,
                    dylib_path.display(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn rewrite_probe_install_names(_exe_path: &Path, _dylib_paths: &[PathBuf]) -> AppResult<()> {
    Ok(())
}

fn native_box_export_probe_source(step_path: &Path, stl_path: &Path) -> String {
    format!(
        r#"#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <STEPControl_Writer.hxx>
#include <StlAPI_Writer.hxx>
#include <TopoDS_Shape.hxx>

int main() {{
    TopoDS_Shape shape = BRepPrimAPI_MakeBox(10.0, 20.0, 30.0).Shape();
    STEPControl_Writer step_writer;
    step_writer.Transfer(shape, STEPControl_AsIs);
    if (step_writer.Write("{}") != IFSelect_RetDone) {{
        return 2;
    }}
    BRepMesh_IncrementalMesh mesh(shape, 0.2);
    StlAPI_Writer stl_writer;
    if (!stl_writer.Write(shape, "{}")) {{
        return 3;
    }}
    return 0;
}}
"#,
        step_path.to_string_lossy(),
        stl_path.to_string_lossy()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{self, json};
    use std::env;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{label}-{}", uuid::Uuid::new_v4()))
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, "").expect("write");
    }

    fn sdk_env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct EnvVarGuard {
        key: &'static str,
        value: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let value = value.to_string();
            let previous = env::var(key).ok();
            env::set_var(key, &value);
            Self {
                key,
                value: previous,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.value.take() {
                env::set_var(self.key, previous);
            } else {
                env::remove_var(self.key);
            }
        }
    }

    fn touch_library_file(dir: &Path, lib: &str) {
        let ext = library_file_extensions().first().copied().unwrap_or("so");
        touch(&dir.join(format!("lib{lib}.{ext}")));
    }

    fn write_manifest(
        runtime_root: &Path,
        include_dir: &str,
        lib_dir: &str,
        platform: &str,
        arch: &str,
        abi_tag: &str,
        required_headers: &[&str],
        required_libraries: &[&str],
    ) {
        let manifest = json!({
            "schemaVersion": "1",
            "platform": platform,
            "arch": arch,
            "occtVersion": "7.8.1",
            "abiTag": abi_tag,
            "includeDir": include_dir,
            "libDir": lib_dir,
            "requiredHeaders": required_headers,
            "requiredLibraries": required_libraries,
            "libraryHashes": { "TKernel": "dummy" },
        });
        let manifest_path = runtime_root.join(OCCT_MANIFEST_FILE);
        fs::create_dir_all(runtime_root).expect("mkdir manifest root");
        fs::write(
            manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .expect("write manifest");
    }

    fn write_headers(target: &Path, headers: &[&str]) {
        for header in headers {
            touch(&target.join(header));
        }
    }

    fn write_executable_script(path: &Path, contents: &str) {
        touch(path);
        fs::write(path, contents).expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(path).expect("script metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("script permissions");
        }
    }

    #[test]
    fn inspect_runtime_reports_ocp_libs_without_headers() {
        let _lock = sdk_env_lock();
        let root = temp_root("ocp-no-headers");
        let empty_include_dir = root.join("include").join("opencascade");
        fs::create_dir_all(&empty_include_dir).expect("mkdir empty include");
        let dylib_dir = root
            .join(PYTHON_SITE_PACKAGES)
            .join(OCP_PACKAGE)
            .join(OCP_DYLIBS);
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&dylib_dir, lib);
        }

        let layout = inspect_build123d_ocp_runtime(&root);

        assert!(layout.ocp_root.is_some());
        assert!(layout.dylib_dir.is_some());
        assert!(layout.include_dir.is_none());
        assert_eq!(layout.missing_libs, Vec::<String>::new());
        assert_eq!(layout.missing_headers.len(), REQUIRED_OCCT_HEADERS.len());
        assert!(!layout.can_compile_native_shim());
        let blockers = layout.blocker_summary();
        let include_blocker = blockers
            .iter()
            .find(|blocker| blocker.contains("OCCT include directory missing or empty"))
            .expect("include blocker");
        assert!(include_blocker.contains(&root.display().to_string()));
        assert!(include_blocker.contains(&empty_include_dir.display().to_string()));
        assert!(include_blocker.contains("present without required OCCT headers"));
        assert!(include_blocker.contains("checked include candidates"));
        assert!(blockers
            .iter()
            .any(|blocker| blocker.contains("npm run occt:prepare")));
        assert!(!blockers
            .iter()
            .any(|blocker| blocker.contains("OCCT headers missing")));
    }

    #[test]
    fn inspect_runtime_accepts_matching_headers_and_dylibs() {
        let _lock = sdk_env_lock();
        let root = temp_root("ocp-ready");
        let include_dir = root.join("include").join("opencascade");
        let dylib_dir = root
            .join(PYTHON_SITE_PACKAGES)
            .join(OCP_PACKAGE)
            .join(OCP_DYLIBS);
        for header in REQUIRED_OCCT_HEADERS {
            touch(&include_dir.join(header));
        }
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&dylib_dir, lib);
        }

        let layout = inspect_build123d_ocp_runtime(&root);

        assert_eq!(layout.include_dir.as_deref(), Some(include_dir.as_path()));
        assert!(layout.missing_headers.is_empty());
        assert!(layout.missing_libs.is_empty());
        assert!(layout.can_compile_native_shim());
    }

    #[test]
    fn inspect_runtime_reports_specific_headers_when_include_dir_exists() {
        let _lock = sdk_env_lock();
        let root = temp_root("ocp-partial-headers");
        let include_dir = root.join("include").join("opencascade");
        let dylib_dir = root
            .join(PYTHON_SITE_PACKAGES)
            .join(OCP_PACKAGE)
            .join(OCP_DYLIBS);
        for header in REQUIRED_OCCT_HEADERS.iter().skip(1) {
            touch(&include_dir.join(header));
        }
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&dylib_dir, lib);
        }

        let layout = inspect_build123d_ocp_runtime(&root);

        assert_eq!(layout.include_dir.as_deref(), Some(include_dir.as_path()));
        assert_eq!(
            layout.missing_headers,
            vec![REQUIRED_OCCT_HEADERS[0].to_string()]
        );
        let blockers = layout.blocker_summary();
        let header_blocker = blockers
            .iter()
            .find(|blocker| blocker.contains("OCCT headers missing"))
            .expect("header blocker");
        assert!(header_blocker.contains(&include_dir.display().to_string()));
        assert!(header_blocker.contains(REQUIRED_OCCT_HEADERS[0]));
    }

    #[test]
    fn inspect_runtime_prefers_full_header_dir_over_partial_earlier_candidate() {
        let _lock = sdk_env_lock();
        let root = temp_root("ocp-full-later");
        let partial_include_dir = root.join("include").join("opencascade");
        let ocp_root = root
            .join("lib")
            .join("python3.12")
            .join("site-packages")
            .join(OCP_PACKAGE);
        let full_include_dir = ocp_root.join("include");
        let dylib_dir = ocp_root.join(OCP_DYLIBS);

        touch(&partial_include_dir.join(REQUIRED_OCCT_HEADERS[0]));
        for header in REQUIRED_OCCT_HEADERS {
            touch(&full_include_dir.join(header));
        }
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&dylib_dir, lib);
        }

        let layout = inspect_build123d_ocp_runtime(&root);

        assert_eq!(layout.ocp_root.as_deref(), Some(ocp_root.as_path()));
        assert_eq!(
            layout.include_dir.as_deref(),
            Some(full_include_dir.as_path())
        );
        assert!(layout.missing_headers.is_empty(), "{layout:?}");
        assert!(layout.can_compile_native_shim(), "{layout:?}");
    }

    #[test]
    fn inspect_runtime_discovers_non_default_python_minor_site_packages() {
        let _lock = sdk_env_lock();
        let root = temp_root("ocp-python313");
        let ocp_root = root
            .join("lib")
            .join("python3.13")
            .join("site-packages")
            .join(OCP_PACKAGE);
        let include_dir = ocp_root.join("include").join("opencascade");
        let dylib_dir = ocp_root.join(OCP_DYLIBS);
        for header in REQUIRED_OCCT_HEADERS {
            touch(&include_dir.join(header));
        }
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&dylib_dir, lib);
        }

        let layout = inspect_build123d_ocp_runtime(&root);

        assert_eq!(layout.ocp_root.as_deref(), Some(ocp_root.as_path()));
        assert_eq!(layout.dylib_dir.as_deref(), Some(dylib_dir.as_path()));
        assert!(layout.missing_libs.is_empty(), "{layout:?}");
        assert!(layout.missing_headers.is_empty(), "{layout:?}");
        assert!(layout.can_compile_native_shim(), "{layout:?}");
    }

    #[test]
    fn native_box_probe_blocks_before_compile_when_headers_missing() {
        let _lock = sdk_env_lock();
        let root = temp_root("ocp-blocked-probe");
        let dylib_dir = root
            .join(PYTHON_SITE_PACKAGES)
            .join(OCP_PACKAGE)
            .join(OCP_DYLIBS);
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&dylib_dir, lib);
        }
        let layout = inspect_build123d_ocp_runtime(&root);

        let outcome =
            run_native_box_export_probe(&layout, temp_root("ocp-probe-out")).expect("probe");

        let NativeExportOutcome::Blocked { blockers } = outcome else {
            panic!("expected blocked outcome");
        };
        assert!(blockers
            .iter()
            .any(|blocker| blocker.contains("OCCT include directory missing")));
        assert!(!blockers
            .iter()
            .any(|blocker| blocker.contains("OCCT headers missing")));
    }

    #[test]
    fn probe_source_targets_step_and_stl_exports() {
        let source =
            native_box_export_probe_source(Path::new("/tmp/box.step"), Path::new("/tmp/box.stl"));

        assert!(source.contains("BRepPrimAPI_MakeBox"));
        assert!(source.contains("STEPControl_Writer"));
        assert!(source.contains("StlAPI_Writer"));
        assert!(source.contains("/tmp/box.step"));
        assert!(source.contains("/tmp/box.stl"));
    }

    #[test]
    fn native_export_compile_failure_preserves_raw_stdout_and_stderr_details() {
        let _lock = sdk_env_lock();
        let root = temp_root("native-compile-raw-error");
        let include_dir = root.join("include");
        let dylib_dir = root.join("lib");
        write_headers(&include_dir, REQUIRED_OCCT_HEADERS);
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&dylib_dir, lib);
        }
        let compiler_path = root.join("fake-cxx");
        write_executable_script(
            &compiler_path,
            "#!/bin/sh\necho raw compiler stdout\necho raw compiler stderr >&2\nexit 42\n",
        );
        let _guard = EnvVarGuard::set("CXX", compiler_path.to_string_lossy().as_ref());
        let layout = DirectOcctSdkLayout {
            runtime_root: root.clone(),
            ocp_root: Some(root.clone()),
            dylib_dir: Some(dylib_dir),
            include_dir: Some(include_dir),
            missing_headers: Vec::new(),
            missing_libs: Vec::new(),
            install_name_prefix: OCP_INSTALL_NAME_PREFIX,
        };

        let err = run_native_export_source(
            &layout,
            root.join("out"),
            "broken.cpp",
            "broken",
            root.join("model.step"),
            root.join("preview.stl"),
            "int main(){return 0;}".to_string(),
        )
        .expect_err("compiler failure should surface raw details");

        assert_eq!(err.message, "Direct OCCT native shim compile failed.");
        let details = err.details.as_deref().expect("raw details");
        assert!(details.contains("raw compiler stdout"), "{details}");
        assert!(details.contains("raw compiler stderr"), "{details}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn inspect_runtime_prefers_occt_manifest_from_env() {
        let _lock = sdk_env_lock();
        let env_root = temp_root("manifest-env");
        let manifest_root = env_root.join(OCCT_RUNTIME_SUBDIR);
        let include_dir = manifest_root.join("include");
        let lib_dir = manifest_root.join("lib");
        write_headers(&include_dir, REQUIRED_OCCT_HEADERS);
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&lib_dir, lib);
        }
        write_manifest(
            &manifest_root,
            "include",
            "lib",
            current_runtime_platform(),
            std::env::consts::ARCH,
            current_runtime_abi_tag(),
            REQUIRED_OCCT_HEADERS,
            REQUIRED_OCCT_LIBS,
        );
        let _guard = EnvVarGuard::set(ECKY_OCCT_ROOT, env_root.to_string_lossy().as_ref());

        let layout = inspect_build123d_ocp_runtime(&temp_root("ignored"));

        assert_eq!(layout.ocp_root.as_deref(), Some(manifest_root.as_path()));
        assert_eq!(layout.include_dir.as_deref(), Some(include_dir.as_path()));
        assert_eq!(layout.dylib_dir.as_deref(), Some(lib_dir.as_path()));
        assert!(layout.can_compile_native_shim());
    }

    #[test]
    fn inspect_runtime_manifest_blocks_missing_library() {
        let _lock = sdk_env_lock();
        let env_root = temp_root("manifest-missing-lib");
        let manifest_root = env_root.join(OCCT_RUNTIME_SUBDIR);
        let include_dir = manifest_root.join("include");
        let lib_dir = manifest_root.join("lib");
        write_headers(&include_dir, REQUIRED_OCCT_HEADERS);
        for lib in REQUIRED_OCCT_LIBS.iter().skip(1) {
            touch_library_file(&lib_dir, lib);
        }
        write_manifest(
            &manifest_root,
            "include",
            "lib",
            current_runtime_platform(),
            std::env::consts::ARCH,
            current_runtime_abi_tag(),
            REQUIRED_OCCT_HEADERS,
            REQUIRED_OCCT_LIBS,
        );
        let _guard = EnvVarGuard::set(ECKY_OCCT_ROOT, env_root.to_string_lossy().as_ref());

        let layout = inspect_build123d_ocp_runtime(&temp_root("ignored"));
        assert_eq!(layout.missing_libs, vec![REQUIRED_OCCT_LIBS[0].to_string()]);
        let blocker = layout
            .blocker_summary()
            .into_iter()
            .find_map(|blocker| serde_json::from_str::<serde_json::Value>(&blocker).ok())
            .expect("invalid blocker payload");
        assert_eq!(blocker["kind"], "missingLibraries");
        assert_eq!(blocker["missing"][0].as_str(), Some(REQUIRED_OCCT_LIBS[0]));
    }

    #[test]
    fn inspect_runtime_manifest_platform_mismatch_reports_blocker() {
        let _lock = sdk_env_lock();
        let env_root = temp_root("manifest-platform-mismatch");
        let manifest_root = env_root.join(OCCT_RUNTIME_SUBDIR);
        let include_dir = manifest_root.join("include");
        let lib_dir = manifest_root.join("lib");
        write_headers(&include_dir, REQUIRED_OCCT_HEADERS);
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&lib_dir, lib);
        }
        write_manifest(
            &manifest_root,
            "include",
            "lib",
            "unsupported-platform",
            std::env::consts::ARCH,
            current_runtime_abi_tag(),
            REQUIRED_OCCT_HEADERS,
            REQUIRED_OCCT_LIBS,
        );
        let _guard = EnvVarGuard::set(ECKY_OCCT_ROOT, env_root.to_string_lossy().as_ref());

        let blockers = inspect_build123d_ocp_runtime(&temp_root("ignored")).blocker_summary();
        let blocker = blockers
            .into_iter()
            .find_map(|blocker| serde_json::from_str::<serde_json::Value>(&blocker).ok())
            .expect("invalid blocker payload");
        assert_eq!(blocker["kind"], "platformMismatch");
        assert_eq!(blocker["manifestPlatform"], "unsupported-platform");
        assert_eq!(blocker["runtimePlatform"], current_runtime_platform());
    }

    #[test]
    fn inspect_runtime_manifest_missing_required_fields_report_blocker_payload() {
        let _lock = sdk_env_lock();
        let env_root = temp_root("manifest-missing-fields");
        let manifest_root = env_root.join(OCCT_RUNTIME_SUBDIR);
        fs::create_dir_all(&manifest_root).expect("mkdir manifest root");
        fs::write(manifest_root.join(OCCT_MANIFEST_FILE), "{}").expect("write blank manifest");
        let _guard = EnvVarGuard::set(ECKY_OCCT_ROOT, env_root.to_string_lossy().as_ref());

        let blockers = inspect_build123d_ocp_runtime(&temp_root("ignored"))
            .blocker_summary()
            .into_iter()
            .filter_map(|blocker| serde_json::from_str::<serde_json::Value>(&blocker).ok())
            .filter(|json| {
                json.get("kind").and_then(|kind| kind.as_str()) == Some("manifestMissingFields")
            })
            .collect::<Vec<_>>();

        assert_eq!(blockers.len(), 1);
        let blocker = &blockers[0];
        assert_eq!(
            blocker
                .get("missingFields")
                .and_then(|fields| fields.as_array())
                .expect("missingFields")
                .iter()
                .map(|value| value.as_str().expect("field must be string").to_string())
                .collect::<Vec<_>>(),
            vec![
                "schemaVersion".to_string(),
                "platform".to_string(),
                "arch".to_string(),
                "occtVersion".to_string(),
                "abiTag".to_string(),
                "includeDir".to_string(),
                "libDir".to_string(),
                "requiredHeaders".to_string(),
                "requiredLibraries".to_string(),
                "libraryHashes".to_string()
            ]
        );
    }

    #[test]
    fn inspect_runtime_falls_back_to_ocp_when_manifest_blocked() {
        let _lock = sdk_env_lock();
        let root = temp_root("manifest-fallback");
        let manifest_root = root.join(OCCT_RUNTIME_SUBDIR);
        let manifest_include_dir = manifest_root.join("include");
        let manifest_lib_dir = manifest_root.join("lib");
        write_headers(&manifest_include_dir, REQUIRED_OCCT_HEADERS);
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&manifest_lib_dir, lib);
        }
        write_manifest(
            &manifest_root,
            "include",
            "lib",
            "unsupported-platform",
            std::env::consts::ARCH,
            current_runtime_abi_tag(),
            REQUIRED_OCCT_HEADERS,
            REQUIRED_OCCT_LIBS,
        );

        let ocp_root = root
            .join("lib")
            .join("python3.12")
            .join("site-packages")
            .join(OCP_PACKAGE);
        let include_dir = ocp_root.join("include").join("opencascade");
        let dylib_dir = ocp_root.join(OCP_DYLIBS);
        write_headers(&include_dir, REQUIRED_OCCT_HEADERS);
        for lib in REQUIRED_OCCT_LIBS {
            touch_library_file(&dylib_dir, lib);
        }

        let layout = inspect_build123d_ocp_runtime(&root);

        assert_eq!(layout.ocp_root.as_deref(), Some(ocp_root.as_path()));
        assert_eq!(layout.include_dir.as_deref(), Some(include_dir.as_path()));
        assert!(layout.can_compile_native_shim());
        assert!(layout.missing_headers.is_empty());
        assert!(layout.missing_libs.is_empty());
    }

    #[test]
    fn live_bundled_build123d_runtime_can_export_when_headers_are_available() {
        let _lock = sdk_env_lock();
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }

        let layout = inspect_build123d_ocp_runtime(&runtime_root);

        assert!(layout.ocp_root.is_some(), "{layout:?}");
        assert!(layout.dylib_dir.is_some(), "{layout:?}");
        assert_eq!(layout.missing_libs, Vec::<String>::new());
        assert_eq!(layout.install_name_prefix, OCP_INSTALL_NAME_PREFIX);

        let outcome =
            run_native_box_export_probe(&layout, temp_root("live-ocp-probe")).expect("probe");
        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            } = outcome
            else {
                panic!("expected native box export once OCCT headers are bundled");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked native probe without complete SDK layout");
            };
            assert!(!blockers.is_empty());
        }
    }
}
