use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{AppError, AppResult};

const PYTHON_SITE_PACKAGES: &str = "lib/python3.12/site-packages";
const OCP_PACKAGE: &str = "OCP";
const OCP_DYLIBS: &str = ".dylibs";
const OCP_INSTALL_NAME_PREFIX: &str = "/DLC/OCP/.dylibs";

pub const REQUIRED_OCCT_HEADERS: &[&str] = &[
    "BRepAlgoAPI_Common.hxx",
    "BRepAlgoAPI_Cut.hxx",
    "BRepAlgoAPI_Fuse.hxx",
    "BRepPrimAPI_MakeBox.hxx",
    "BRepPrimAPI_MakeCylinder.hxx",
    "BRepPrimAPI_MakePrism.hxx",
    "BRepPrimAPI_MakeSphere.hxx",
    "BRepBuilderAPI_Transform.hxx",
    "BRepBuilderAPI_MakeEdge.hxx",
    "BRepBuilderAPI_MakeFace.hxx",
    "BRepBuilderAPI_MakePolygon.hxx",
    "BRepBuilderAPI_MakeWire.hxx",
    "BRep_Builder.hxx",
    "BRepMesh_IncrementalMesh.hxx",
    "STEPControl_Writer.hxx",
    "StlAPI_Writer.hxx",
    "TopoDS_Compound.hxx",
    "TopoDS_Shape.hxx",
    "TopoDS_Wire.hxx",
    "gp_Ax2.hxx",
    "gp_Circ.hxx",
    "gp_Dir.hxx",
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
    "TKBRep",
    "TKTopAlgo",
    "TKBO",
    "TKBool",
    "TKPrim",
    "TKMesh",
    "TKDESTEP",
    "TKDESTL",
];

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
    }

    pub fn blocker_summary(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.ocp_root.is_none() {
            blockers.push("OCP package directory missing".to_string());
        }
        if self.dylib_dir.is_none() {
            blockers.push("OCP .dylibs directory missing".to_string());
        }
        if self.include_dir.is_none() {
            blockers.push("OCCT include directory missing".to_string());
        }
        if !self.missing_headers.is_empty() {
            blockers.push(format!(
                "OCCT headers missing: {}",
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
    let site_packages = runtime_root.join(PYTHON_SITE_PACKAGES);
    let ocp_root = existing_dir(site_packages.join(OCP_PACKAGE));
    let dylib_dir = ocp_root
        .as_ref()
        .and_then(|root| existing_dir(root.join(OCP_DYLIBS)));
    let include_dir = find_include_dir(&runtime_root, ocp_root.as_deref());

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
                .map(|dir| find_versioned_dylib(dir, lib).is_none())
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
        .arg("-Wl,-headerpad_max_install_names")
        .arg("-I")
        .arg(layout.include_dir()?)
        .arg(&source_path)
        .args(&dylib_paths);
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

fn find_include_dir(runtime_root: &Path, ocp_root: Option<&Path>) -> Option<PathBuf> {
    [
        runtime_root.join("include").join("opencascade"),
        runtime_root.join("include"),
        runtime_root
            .join("Library")
            .join("include")
            .join("opencascade"),
    ]
    .into_iter()
    .chain(ocp_root.into_iter().flat_map(|root| {
        [
            root.join("include"),
            root.join("include").join("opencascade"),
        ]
    }))
    .find(|candidate| {
        REQUIRED_OCCT_HEADERS
            .iter()
            .any(|header| candidate.join(header).is_file())
    })
}

fn find_versioned_dylib(dir: &Path, lib: &str) -> Option<PathBuf> {
    let prefix = format!("lib{}.", lib);
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with(&prefix) && name.ends_with(".dylib"))
                .unwrap_or(false)
        })
}

fn required_dylib_paths(dir: &Path) -> AppResult<Vec<PathBuf>> {
    REQUIRED_OCCT_LIBS
        .iter()
        .map(|lib| {
            find_versioned_dylib(dir, lib).ok_or_else(|| {
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

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{label}-{}", uuid::Uuid::new_v4()))
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, "").expect("write");
    }

    #[test]
    fn inspect_runtime_reports_ocp_libs_without_headers() {
        let root = temp_root("ocp-no-headers");
        let dylib_dir = root
            .join(PYTHON_SITE_PACKAGES)
            .join(OCP_PACKAGE)
            .join(OCP_DYLIBS);
        for lib in REQUIRED_OCCT_LIBS {
            touch(&dylib_dir.join(format!("lib{lib}.7.8.1.dylib")));
        }

        let layout = inspect_build123d_ocp_runtime(&root);

        assert!(layout.ocp_root.is_some());
        assert!(layout.dylib_dir.is_some());
        assert!(layout.include_dir.is_none());
        assert_eq!(layout.missing_libs, Vec::<String>::new());
        assert_eq!(layout.missing_headers.len(), REQUIRED_OCCT_HEADERS.len());
        assert!(!layout.can_compile_native_shim());
        assert!(layout
            .blocker_summary()
            .iter()
            .any(|blocker| blocker.contains("OCCT include directory missing")));
    }

    #[test]
    fn inspect_runtime_accepts_matching_headers_and_dylibs() {
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
            touch(&dylib_dir.join(format!("lib{lib}.7.8.1.dylib")));
        }

        let layout = inspect_build123d_ocp_runtime(&root);

        assert_eq!(layout.include_dir.as_deref(), Some(include_dir.as_path()));
        assert!(layout.missing_headers.is_empty());
        assert!(layout.missing_libs.is_empty());
        assert!(layout.can_compile_native_shim());
    }

    #[test]
    fn native_box_probe_blocks_before_compile_when_headers_missing() {
        let root = temp_root("ocp-blocked-probe");
        let dylib_dir = root
            .join(PYTHON_SITE_PACKAGES)
            .join(OCP_PACKAGE)
            .join(OCP_DYLIBS);
        for lib in REQUIRED_OCCT_LIBS {
            touch(&dylib_dir.join(format!("lib{lib}.7.8.1.dylib")));
        }
        let layout = inspect_build123d_ocp_runtime(&root);

        let outcome =
            run_native_box_export_probe(&layout, temp_root("ocp-probe-out")).expect("probe");

        let NativeExportOutcome::Blocked { blockers } = outcome else {
            panic!("expected blocked outcome");
        };
        assert!(blockers
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
    fn live_bundled_build123d_runtime_can_export_when_headers_are_available() {
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
