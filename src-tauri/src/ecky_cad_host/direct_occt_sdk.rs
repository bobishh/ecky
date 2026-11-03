use std::path::{Path, PathBuf};

const ECKY_OCCT_ROOT: &str = "ECKY_OCCT_ROOT";

pub(crate) fn scoped_env_var_os(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(key)
}

pub const REQUIRED_OCCT_HEADERS: &[&str] = &[
    "BRepAlgoAPI_Common.hxx",
    "BRepAlgoAPI_Cut.hxx",
    "BRepAlgoAPI_Fuse.hxx",
    "BRepAlgoAPI_Section.hxx",
    "BRepClass3d_SolidClassifier.hxx",
    "BRepIntCurveSurface_Inter.hxx",
    "Bnd_Box.hxx",
    "BRepAdaptor_Curve.hxx",
    "BRepAdaptor_Surface.hxx",
    "BRepBndLib.hxx",
    "BRepCheck_Analyzer.hxx",
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
    "GCPnts_QuasiUniformDeflection.hxx",
    "GProp_GProps.hxx",
    "GC_MakeArcOfCircle.hxx",
    "Geom_BezierCurve.hxx",
    "Geom_BSplineCurve.hxx",
    "Geom_TrimmedCurve.hxx",
    "GeomAPI_PointsToBSpline.hxx",
    "IFSelect_ReturnStatus.hxx",
    "HLRAlgo_Projector.hxx",
    "HLRBRep_Algo.hxx",
    "HLRBRep_HLRToShape.hxx",
    "ShapeAnalysis_FreeBounds.hxx",
    "STEPControl_Reader.hxx",
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
    "TopTools_HSequenceOfShape.hxx",
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
    "TKHLR",
    "TKBRep",
    "TKTopAlgo",
    "TKShHealing",
    "TKBO",
    "TKBool",
    "TKFeat",
    "TKPrim",
    "TKOffset",
    "TKFillet",
    "TKMesh",
    "TKDE",
    "TKXSBase",
    "TKDESTEP",
    "TKDESTL",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectOcctSdkLayout {
    pub runtime_root: PathBuf,
    pub dylib_dir: Option<PathBuf>,
    pub include_dir: Option<PathBuf>,
    pub missing_headers: Vec<String>,
    pub missing_libs: Vec<String>,
    pub install_name_prefix: &'static str,
}

impl DirectOcctSdkLayout {
    pub fn runtime_complete(&self) -> bool {
        self.dylib_dir.is_some()
            && self.include_dir.is_some()
            && self.missing_headers.is_empty()
            && self.missing_libs.is_empty()
    }

    pub fn blocker_summary(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.include_dir.is_none() {
            blockers.push(format!(
                "OCCT include directory missing under '{}'.",
                self.runtime_root.display()
            ));
        }
        if self.dylib_dir.is_none() {
            blockers.push(format!(
                "OCCT library directory missing under '{}'.",
                self.runtime_root.display()
            ));
        }
        if !self.missing_headers.is_empty() {
            blockers.push(format!(
                "missing OCCT headers: {}",
                self.missing_headers.join(", ")
            ));
        }
        if !self.missing_libs.is_empty() {
            blockers.push(format!(
                "missing OCCT libraries: {}",
                self.missing_libs.join(", ")
            ));
        }
        blockers
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeExportOutcome {
    Exported {
        step_path: PathBuf,
        stl_path: PathBuf,
        part_stl_paths: Vec<(String, PathBuf)>,
        tessellated_step: bool,
        source_mesh_digests: Vec<String>,
    },
    MeshExported {
        stl_path: PathBuf,
        part_stl_paths: Vec<(String, PathBuf)>,
        source_mesh_digests: Vec<String>,
    },
    Blocked {
        blockers: Vec<String>,
    },
}

pub fn inspect_occt_runtime(runtime_root: impl AsRef<Path>) -> DirectOcctSdkLayout {
    let requested_root = runtime_root.as_ref().to_path_buf();
    let runtime_root = scoped_env_var_os(ECKY_OCCT_ROOT)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or(requested_root);
    let include_dir = [
        runtime_root.join("include").join("opencascade"),
        runtime_root.join("include"),
    ]
    .into_iter()
    .find(|path| path.is_dir());
    let dylib_dir = [runtime_root.join("lib"), runtime_root.join("bin")]
        .into_iter()
        .find(|path| path.is_dir());
    let missing_headers = REQUIRED_OCCT_HEADERS
        .iter()
        .filter(|header| {
            include_dir
                .as_ref()
                .is_none_or(|directory| !directory.join(header).is_file())
        })
        .map(|header| (*header).to_string())
        .collect();
    let missing_libs = REQUIRED_OCCT_LIBS
        .iter()
        .filter(|library| {
            dylib_dir
                .as_ref()
                .is_none_or(|directory| !library_exists(directory, library))
        })
        .map(|library| (*library).to_string())
        .collect();

    DirectOcctSdkLayout {
        runtime_root: runtime_root.clone(),
        dylib_dir,
        include_dir,
        missing_headers,
        missing_libs,
        install_name_prefix: "@rpath",
    }
}

fn library_exists(directory: &Path, library: &str) -> bool {
    std::fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .any(|name| {
            name == format!("{library}.dll")
                || name.starts_with(&format!("lib{library}.so"))
                || name.starts_with(&format!("lib{library}.dylib"))
                || (name.starts_with(&format!("lib{library}.")) && name.ends_with(".dylib"))
        })
}

pub fn bundled_occt_runtime_root_from_repo(repo_root: impl AsRef<Path>) -> PathBuf {
    repo_root
        .as_ref()
        .join(".dist")
        .join("runtime")
        .join("occt")
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_cpp_export_surface_is_absent() {
        let source = include_str!("direct_occt_sdk.rs")
            .split_once("#[cfg(test)]")
            .map(|(head, _)| head)
            .unwrap_or_else(|| include_str!("direct_occt_sdk.rs"));
        assert!(!source.contains("run_native_export_source"));
        assert!(!source.contains("Command::new(compiler)"));
    }
}
