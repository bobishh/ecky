use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::direct_occt::{
    OcctArg, OcctCommand, OcctKeyword, OcctOp, OcctParameterKind, OcctPlan, OcctSlot,
};
use super::direct_occt_runner;
use super::direct_occt_sdk::{run_native_export_source, DirectOcctSdkLayout, NativeExportOutcome};
use crate::ecky_core_ir::{
    CoreEdgeAxis, CoreEdgeBound, CoreEdgeSelectorClause, CoreFaceAreaRank, CoreFaceSelectorClause,
    CoreParameterValue, CoreProgram, CoreSelectorPayload,
};
use crate::models::{AppError, AppResult, DesignParams, ParamValue, PathResolver};

const PREVIEW_MESH_LINEAR_DEFLECTION_MM: f64 = 0.05;
const PREVIEW_MESH_ANGULAR_DEFLECTION_RAD: f64 = 0.15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectOcctExport {
    pub step_path: PathBuf,
    pub stl_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EdgeSelectorKind {
    TargetIds(Vec<String>),
    Clauses(Vec<CoreEdgeSelectorClause>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EdgeSelector {
    kind: EdgeSelectorKind,
    created_by_slot_index: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ShellFaceSelectorKind {
    TargetIds(Vec<String>),
    Clauses(Vec<CoreFaceSelectorClause>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellFaceSelector {
    kind: ShellFaceSelectorKind,
    created_by_slot_index: Option<u64>,
}

pub fn export_core_program_step_stl(
    program: &CoreProgram,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    export_core_program_step_stl_with_params(program, &DesignParams::new(), layout, output_dir)
}

pub fn export_core_program_step_stl_with_params(
    program: &CoreProgram,
    parameters: &DesignParams,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    let parameters = effective_program_parameters(program, parameters);
    let plan = super::direct_occt::plan_core_program_with_params(program, &parameters)?;
    export_plan_step_stl_with_params(&plan, &parameters, layout, output_dir)
}

pub fn export_core_program_step_stl_with_params_runner_first(
    program: &CoreProgram,
    parameters: &DesignParams,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
    app: &dyn PathResolver,
) -> AppResult<NativeExportOutcome> {
    let parameters = effective_program_parameters(program, parameters);
    let plan = super::direct_occt::plan_core_program_with_params(program, &parameters)?;
    let resolved_plan = resolve_plan_parameters(&plan, &parameters)?;
    if let Some(outcome) =
        direct_occt_runner::run_plan_step_stl_if_available(&resolved_plan, &output_dir, app)?
    {
        return Ok(outcome);
    }
    export_plan_step_stl_with_params(&plan, &parameters, layout, output_dir)
}

pub fn export_plan_step_stl(
    plan: &OcctPlan,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    export_plan_step_stl_with_params(plan, &DesignParams::new(), layout, output_dir)
}

pub fn export_plan_step_stl_with_params(
    plan: &OcctPlan,
    parameters: &DesignParams,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    let output_dir = output_dir.as_ref();
    let step_path = output_dir.join("model.step");
    let stl_path = output_dir.join("preview.stl");
    let source = emit_plan_export_source_with_params(plan, parameters, &step_path, &stl_path)?;
    let outcome = run_native_export_source(
        layout,
        output_dir,
        "direct_occt_executor.cpp",
        "direct_occt_executor",
        step_path,
        stl_path,
        source,
    )?;
    // Scan for per-part binary STL files emitted by the generated C++.
    let mut part_stl_paths = Vec::new();
    if plan.parts.len() > 1 {
        let parts_dir = output_dir.join("parts");
        if parts_dir.is_dir() {
            for part in &plan.parts {
                let key = if part.key.trim().is_empty() {
                    "body".to_string()
                } else {
                    part.key.clone()
                };
                let candidate = parts_dir.join(format!("{}.stl", key));
                if candidate.is_file() {
                    part_stl_paths.push((key, candidate));
                }
            }
        }
    }
    if let NativeExportOutcome::Exported {
        step_path,
        stl_path,
        ..
    } = outcome
    {
        Ok(NativeExportOutcome::Exported {
            step_path,
            stl_path,
            part_stl_paths,
        })
    } else {
        Ok(outcome)
    }
}

pub fn emit_plan_export_source(
    plan: &OcctPlan,
    step_path: &Path,
    stl_path: &Path,
) -> AppResult<String> {
    emit_plan_export_source_with_params(plan, &DesignParams::new(), step_path, stl_path)
}

pub fn emit_plan_export_source_with_params(
    plan: &OcctPlan,
    parameters: &DesignParams,
    step_path: &Path,
    stl_path: &Path,
) -> AppResult<String> {
    let plan = resolve_plan_parameters(plan, parameters)?;
    if plan.parts.is_empty() {
        return Err(AppError::validation(
            "Direct OCCT executor needs at least one part.",
        ));
    }

    let mut vars = BTreeMap::new();
    let mut body = String::new();
    let mut part_roots = Vec::with_capacity(plan.parts.len());
    let mut part_topology_roots = Vec::with_capacity(plan.parts.len());
    for part in &plan.parts {
        for command in &part.commands {
            emit_command(&mut body, &mut vars, &part.key, command)?;
        }

        let root_var = vars.get(&part.root).cloned().ok_or_else(|| {
            AppError::validation(format!(
                "Direct OCCT executor could not find root slot {:?} for part `{}`.",
                part.root, part.key
            ))
        })?;
        part_roots.push(root_var.clone());
        let topology_slots = part
            .commands
            .iter()
            .filter(|command| occt_command_outputs_shape(command.op))
            .map(|command| (command.output.0, slot_var(command.output)))
            .collect::<Vec<_>>();
        part_topology_roots.push((
            part.key.clone(),
            part.label.clone(),
            root_var,
            topology_slots,
        ));
    }

    let root_var = if part_roots.len() == 1 {
        part_roots.pop().expect("checked non-empty")
    } else {
        emit_top_level_compound(&mut body, &part_roots);
        "model_compound_shape".to_string()
    };
    let topology_path = step_path.with_file_name("topology.json");
    let topology_writer_source = direct_occt_topology_writer_source();
    let stl_writer_source = direct_occt_stl_writer_source();
    let topology_writer_calls = direct_occt_topology_writer_calls(&part_topology_roots);

    Ok(format!(
        r#"#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepClass_FaceClassifier.hxx>
#include <Bnd_Box.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepBndLib.hxx>
#include <BRepGProp.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepBuilderAPI_GTransform.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepPrimAPI_MakeTorus.hxx>
#include <BRepPrimAPI_MakeWedge.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRep_Builder.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepOffsetAPI_MakeOffsetShape.hxx>
#include <BRepOffsetAPI_MakeOffset.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <BRepGProp_Face.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepOffset_Mode.hxx>
#include <BRep_Tool.hxx>
#include <BRepTools.hxx>
#include <GeomAbs_JoinType.hxx>
#include <GeomAbs_SurfaceType.hxx>
#include <GProp_GProps.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GCE2d_MakeSegment.hxx>
#include <Geom_BezierCurve.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <GeomAPI_PointsToBSpline.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Writer.hxx>
#include <StlAPI_Reader.hxx>
#include <TColgp_Array1OfPnt.hxx>
#include <TopAbs_Orientation.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopAbs_State.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Wire.hxx>
#include <TopTools_ListOfShape.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Elips.hxx>
#include <gp_Dir.hxx>
#include <gp_GTrsf.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Pnt.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <sstream>
#include <string>
#include <vector>
#include <filesystem>
namespace fs = std::filesystem;

{topology_writer_source}
{stl_writer_source}

int main() {{
{body}    TopoDS_Shape shape = {root_var};
    STEPControl_Writer step_writer;
    step_writer.Transfer(shape, STEPControl_AsIs);
    if (step_writer.Write("{step_path}") != IFSelect_RetDone) {{
        return 2;
    }}
    std::ofstream topology_file("{topology_path}");
    if (!topology_file) {{
        return 4;
    }}
    topology_file << "{{\"parts\":[";
    bool first_topology_part = true;
{topology_writer_calls}    topology_file << "]}}\n";
    if (!topology_file.good()) {{
        return 4;
    }}
    BRepMesh_IncrementalMesh mesh(shape, {PREVIEW_MESH_LINEAR_DEFLECTION_MM}, false, {PREVIEW_MESH_ANGULAR_DEFLECTION_RAD}, true);
    if (!write_binary_stl_mesh(shape, "{stl_path}")) {{
        std::cerr << "Direct OCCT preview STL write failed: shape produced no triangulated faces.\n";
        return 3;
    }}
{per_part_stl_writes}    return 0;
}}
"#,
        step_path = step_path.to_string_lossy(),
        topology_path = topology_path.to_string_lossy(),
        stl_path = stl_path.to_string_lossy(),
        per_part_stl_writes = direct_occt_per_part_stl_writes(&part_topology_roots, step_path),
    ))
}

/// Emit C++ that writes each part's root shape to `parts/{key}.stl` as binary
/// STL, so multipart export has distinct per-part geometry instead of
/// duplicating the merged preview mesh.
fn direct_occt_per_part_stl_writes(
    part_roots: &[(String, String, String, Vec<(u64, String)>)],
    step_path: &Path,
) -> String {
    if part_roots.len() <= 1 {
        return String::new();
    }
    let parts_dir = step_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("parts");
    let mut calls = format!(
        "    fs::create_directories(\"{}\");\n",
        parts_dir.to_string_lossy()
    );
    for (key, _label, root_var, _topology_slots) in part_roots {
        let part_stl = parts_dir.join(format!("{}.stl", key));
        let root = root_var;
        let path = part_stl.to_string_lossy();
        calls.push_str(&format!(
            "    if (!write_binary_stl_mesh({root}, \"{path}\")) {{\n\
        std::cerr << \"Direct OCCT per-part STL write failed for part '{key}' ({path}).\\n\";\n\
        return 5;\n\
    }}\n"
        ));
    }
    calls
}

fn direct_occt_topology_writer_calls(
    part_roots: &[(String, String, String, Vec<(u64, String)>)],
) -> String {
    part_roots
        .iter()
        .map(|(key, label, root_var, topology_slots)| {
            let label = if label.trim().is_empty() { key } else { label };
            let slot_indexes = topology_slots
                .iter()
                .map(|(slot_index, _)| slot_index.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let slot_shapes = topology_slots
                .iter()
                .map(|(_, slot_var)| slot_var.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "    write_part_faces(topology_file, {}, {}, {}, std::vector<std::uint64_t>{{{}}}, std::vector<TopoDS_Shape>{{{}}}, first_topology_part);\n",
                cpp_string_literal(key),
                cpp_string_literal(label),
                root_var,
                slot_indexes,
                slot_shapes
            )
        })
        .collect::<String>()
}

fn direct_occt_stl_writer_source() -> &'static str {
    r#"bool write_ascii_stl_mesh(const TopoDS_Shape& shape, const char* path) {
    std::ofstream out(path);
    if (!out) {
        return false;
    }
    out << "solid ecky\n";
    bool wrote_triangle = false;
    for (TopExp_Explorer face_explorer(shape, TopAbs_FACE); face_explorer.More(); face_explorer.Next()) {
        TopoDS_Face face = TopoDS::Face(face_explorer.Current());
        TopLoc_Location location;
        Handle(Poly_Triangulation) triangulation = BRep_Tool::Triangulation(face, location);
        if (triangulation.IsNull()) {
            continue;
        }
        gp_Trsf transform = location.Transformation();
        const Poly_Array1OfTriangle& tri_arr = triangulation->Triangles();
        for (Standard_Integer triangle_index = tri_arr.Lower(); triangle_index <= tri_arr.Upper(); ++triangle_index) {
            Standard_Integer n1 = 0;
            Standard_Integer n2 = 0;
            Standard_Integer n3 = 0;
            tri_arr(triangle_index).Get(n1, n2, n3);
            gp_Pnt p1 = welder.weld(triangulation->Node(n1).Transformed(transform));
            gp_Pnt p2 = welder.weld(triangulation->Node(n2).Transformed(transform));
            gp_Pnt p3 = welder.weld(triangulation->Node(n3).Transformed(transform));
            if (face.Orientation() == TopAbs_REVERSED) {
                std::swap(p2, p3);
            }
            gp_Vec edge_a(p1, p2);
            gp_Vec edge_b(p1, p3);
            gp_Vec normal = edge_a.Crossed(edge_b);
            if (normal.SquareMagnitude() <= 1.0e-18) {
                continue;
            }
            triangles.push_back({p1, p2, p3});
        }
    }
    if (triangles.empty()) {
        return false;
    }
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        return false;
    }
    // 80-byte header (blank) + 4-byte little-endian triangle count + 50 bytes
    // per triangle (12 normal + 3*12 vertices + 2 attribute).
    std::string header(80, '\0');
    out.write(header.data(), 80);
    std::uint32_t count = static_cast<std::uint32_t>(triangles.size());
    out.write(reinterpret_cast<const char*>(&count), 4);
    auto write_float = [&](float value) {
        out.write(reinterpret_cast<const char*>(&value), 4);
    };
    for (const auto& tri : triangles) {
        gp_Vec edge_a(tri.p1, tri.p2);
        gp_Vec edge_b(tri.p1, tri.p3);
        gp_Vec normal = edge_a.Crossed(edge_b);
        if (normal.SquareMagnitude() > 1.0e-18) {
            normal.Normalize();
        }
        write_float(static_cast<float>(normal.X()));
        write_float(static_cast<float>(normal.Y()));
        write_float(static_cast<float>(normal.Z()));
        write_float(static_cast<float>(tri.p1.X()));
        write_float(static_cast<float>(tri.p1.Y()));
        write_float(static_cast<float>(tri.p1.Z()));
        write_float(static_cast<float>(tri.p2.X()));
        write_float(static_cast<float>(tri.p2.Y()));
        write_float(static_cast<float>(tri.p2.Z()));
        write_float(static_cast<float>(tri.p3.X()));
        write_float(static_cast<float>(tri.p3.Y()));
        write_float(static_cast<float>(tri.p3.Z()));
        std::uint16_t attr = 0;
        out.write(reinterpret_cast<const char*>(&attr), 2);
    }
    return out.good();
}
"#
}

fn cpp_string_literal(value: &str) -> String {
    let mut escaped = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\x{:02x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

fn occt_command_outputs_shape(op: OcctOp) -> bool {
    !matches!(op, OcctOp::Plane | OcctOp::Location | OcctOp::PathFrame)
}

fn direct_occt_topology_writer_source() -> &'static str {
    r#"void write_json_string(std::ofstream& out, const std::string& value) {
    out << '"';
    for (char ch : value) {
        switch (ch) {
            case '\\': out << "\\\\"; break;
            case '"': out << "\\\""; break;
            case '\n': out << "\\n"; break;
            case '\r': out << "\\r"; break;
            case '\t': out << "\\t"; break;
            default: out << ch; break;
        }
    }
    out << '"';
}

void write_json_number(std::ofstream& out, double value) {
    if (std::isfinite(value)) {
        out << std::setprecision(17) << value;
    } else {
        out << 0;
    }
}

std::int64_t direct_occt_originating_slot_index(
    const std::vector<std::uint64_t>& slot_indexes,
    const std::vector<TopoDS_Shape>& slot_shapes,
    TopAbs_ShapeEnum shape_kind,
    const TopoDS_Shape& target
) {
    std::size_t slot_count = std::min(slot_indexes.size(), slot_shapes.size());
    for (std::size_t slot_index = 0; slot_index < slot_count; ++slot_index) {
        TopTools_IndexedMapOfShape slot_map;
        TopExp::MapShapes(slot_shapes[slot_index], shape_kind, slot_map);
        for (int shape_ordinal = 1; shape_ordinal <= slot_map.Extent(); ++shape_ordinal) {
            if (slot_map.FindKey(shape_ordinal).IsSame(target)) {
                return static_cast<std::int64_t>(slot_indexes[slot_index]);
            }
        }
    }
    return -1;
}

std::string direct_occt_format_coordinate(double value) {
    if (!std::isfinite(value) || std::abs(value) < 0.0005) {
        return "0";
    }
    std::ostringstream out;
    out << std::fixed << std::setprecision(3) << value;
    std::string text = out.str();
    while (!text.empty() && text.back() == '0') {
        text.pop_back();
    }
    if (!text.empty() && text.back() == '.') {
        text.pop_back();
    }
    if (text.empty() || text == "-0") {
        return "0";
    }
    return text;
}

std::string direct_occt_point_signature(const gp_Pnt& point) {
    return direct_occt_format_coordinate(point.X()) + "-" +
           direct_occt_format_coordinate(point.Y()) + "-" +
           direct_occt_format_coordinate(point.Z());
}

std::string direct_occt_edge_signature(const gp_Pnt& start, const gp_Pnt& end) {
    std::string first = direct_occt_point_signature(start);
    std::string second = direct_occt_point_signature(end);
    if (second < first) {
        std::swap(first, second);
    }
    return first + "_" + second;
}

std::string direct_occt_edge_target_id(
    const std::string& part_id,
    int edge_index,
    const TopoDS_Edge& edge
) {
    try {
        BRepAdaptor_Curve curve(edge);
        double first_param = curve.FirstParameter();
        double last_param = curve.LastParameter();
        if (std::isfinite(first_param) && std::isfinite(last_param)) {
            gp_Pnt start = curve.Value(first_param);
            gp_Pnt end = curve.Value(last_param);
            return part_id + ":edge:" + std::to_string(edge_index) + ":" +
                   direct_occt_edge_signature(start, end);
        }
    } catch (...) {
    }
    return part_id + ":edge:" + std::to_string(edge_index);
}

std::string direct_occt_face_target_id(
    const std::string& part_id,
    int face_index,
    const TopoDS_Face& face
) {
    try {
        GProp_GProps props;
        BRepGProp::SurfaceProperties(face, props);
        gp_Pnt center = props.CentreOfMass();
        double area = props.Mass();
        return part_id + ":face:" + std::to_string(face_index) + ":" +
               direct_occt_point_signature(center) + ":" +
               direct_occt_format_coordinate(area);
    } catch (...) {
    }
    return part_id + ":face:" + std::to_string(face_index);
}

std::string direct_occt_stable_target_suffix(const std::string& payload) {
    std::size_t first_colon = payload.find(':');
    if (first_colon == std::string::npos) {
        return payload;
    }
    bool numeric_prefix = first_colon > 0 &&
        std::all_of(payload.begin(), payload.begin() + static_cast<long>(first_colon), [](char ch) {
            return ch >= '0' && ch <= '9';
        });
    if (!numeric_prefix) {
        return payload;
    }
    return payload.substr(first_colon + 1);
}

std::string direct_occt_stable_edge_target_id(const std::string& target_id) {
    const std::string marker = ":edge:";
    std::size_t marker_pos = target_id.find(marker);
    if (marker_pos == std::string::npos) {
        return target_id;
    }
    std::string prefix = target_id.substr(0, marker_pos);
    std::size_t node_marker_pos = prefix.find(":node:");
    std::size_t stable_node_marker_pos = prefix.find(":stable-node-key:");
    if (node_marker_pos != std::string::npos) {
        prefix = prefix.substr(0, node_marker_pos);
    } else if (stable_node_marker_pos != std::string::npos) {
        prefix = prefix.substr(0, stable_node_marker_pos);
    }
    std::string payload = target_id.substr(marker_pos + marker.size());
    return prefix + marker + direct_occt_stable_target_suffix(payload);
}

std::string direct_occt_stable_face_target_id(const std::string& target_id) {
    const std::string marker = ":face:";
    std::size_t marker_pos = target_id.find(marker);
    if (marker_pos == std::string::npos) {
        return target_id;
    }
    std::string prefix = target_id.substr(0, marker_pos);
    std::size_t node_marker_pos = prefix.find(":node:");
    std::size_t stable_node_marker_pos = prefix.find(":stable-node-key:");
    if (node_marker_pos != std::string::npos) {
        prefix = prefix.substr(0, node_marker_pos);
    } else if (stable_node_marker_pos != std::string::npos) {
        prefix = prefix.substr(0, stable_node_marker_pos);
    }
    std::string payload = target_id.substr(marker_pos + marker.size());
    return prefix + marker + direct_occt_stable_target_suffix(payload);
}

void write_part_faces(
    std::ofstream& out,
    const std::string& part_id,
    const std::string& part_label,
    const TopoDS_Shape& part_shape,
    const std::vector<std::uint64_t>& slot_indexes,
    const std::vector<TopoDS_Shape>& slot_shapes,
    bool& first_part
) {
    if (!first_part) {
        out << ",";
    }
    first_part = false;
    out << "{\"partId\":";
    write_json_string(out, part_id);
    out << ",\"label\":";
    write_json_string(out, part_label);
    out << ",\"edges\":[";

    bool first_edge = true;
    TopTools_IndexedMapOfShape edge_map;
    TopExp::MapShapes(part_shape, TopAbs_EDGE, edge_map);
    for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
        try {
            int edge_index = edge_ordinal - 1;
            TopoDS_Edge edge = TopoDS::Edge(edge_map.FindKey(edge_ordinal));
            BRepAdaptor_Curve curve(edge);
            double first_param = curve.FirstParameter();
            double last_param = curve.LastParameter();
            if (!std::isfinite(first_param) || !std::isfinite(last_param)) {
                continue;
            }
            gp_Pnt start = curve.Value(first_param);
            gp_Pnt end = curve.Value(last_param);

            if (!first_edge) {
                out << ",";
            }
            first_edge = false;
            out << "{\"edgeIndex\":" << edge_index;
            std::int64_t edge_originating_slot_index = direct_occt_originating_slot_index(
                slot_indexes,
                slot_shapes,
                TopAbs_EDGE,
                edge
            );
            if (edge_originating_slot_index >= 0) {
                out << ",\"originatingSlotIndex\":" << edge_originating_slot_index;
            }
            out << ",\"label\":";
            std::ostringstream edge_label;
            edge_label << part_label << ".Edge" << (edge_index + 1);
            write_json_string(out, edge_label.str());
            out << ",\"start\":{\"x\":";
            write_json_number(out, start.X());
            out << ",\"y\":";
            write_json_number(out, start.Y());
            out << ",\"z\":";
            write_json_number(out, start.Z());
            out << "},\"end\":{\"x\":";
            write_json_number(out, end.X());
            out << ",\"y\":";
            write_json_number(out, end.Y());
            out << ",\"z\":";
            write_json_number(out, end.Z());
            out << "}}";
        } catch (...) {
        }
    }

    out << "],\"faces\":[";

    bool first_face = true;
    int face_index = 0;
    for (TopExp_Explorer explorer(part_shape, TopAbs_FACE); explorer.More(); explorer.Next(), ++face_index) {
        TopoDS_Face face = TopoDS::Face(explorer.Current());
        GProp_GProps props;
        BRepGProp::SurfaceProperties(face, props);
        gp_Pnt center = props.CentreOfMass();
        double area = props.Mass();

        double normal_x = 0.0;
        double normal_y = 0.0;
        double normal_z = 0.0;
        try {
            double u_min = 0.0;
            double u_max = 0.0;
            double v_min = 0.0;
            double v_max = 0.0;
            BRepTools::UVBounds(face, u_min, u_max, v_min, v_max);
            if (std::isfinite(u_min) && std::isfinite(u_max) && std::isfinite(v_min) && std::isfinite(v_max)) {
                BRepAdaptor_Surface surface(face);
                gp_Pnt surface_point;
                gp_Vec du;
                gp_Vec dv;
                surface.D1((u_min + u_max) / 2.0, (v_min + v_max) / 2.0, surface_point, du, dv);
                gp_Vec normal = du.Crossed(dv);
                if (normal.Magnitude() > 1.0e-9) {
                    normal.Normalize();
                    normal_x = normal.X();
                    normal_y = normal.Y();
                    normal_z = normal.Z();
                }
            }
        } catch (...) {
            normal_x = 0.0;
            normal_y = 0.0;
            normal_z = 0.0;
        }

        if (!first_face) {
            out << ",";
        }
        first_face = false;
        out << "{\"faceIndex\":" << face_index;
        std::int64_t face_originating_slot_index = direct_occt_originating_slot_index(
            slot_indexes,
            slot_shapes,
            TopAbs_FACE,
            face
        );
        if (face_originating_slot_index >= 0) {
            out << ",\"originatingSlotIndex\":" << face_originating_slot_index;
        }
        out << ",\"label\":";
        std::ostringstream face_label;
        face_label << part_label << ".Face" << (face_index + 1);
        write_json_string(out, face_label.str());
        out << ",\"center\":{\"x\":";
        write_json_number(out, center.X());
        out << ",\"y\":";
        write_json_number(out, center.Y());
        out << ",\"z\":";
        write_json_number(out, center.Z());
        out << "},\"normal\":[";
        write_json_number(out, normal_x);
        out << ",";
        write_json_number(out, normal_y);
        out << ",";
        write_json_number(out, normal_z);
        out << "],\"area\":";
        write_json_number(out, area);
        out << "}";
    }

    out << "]}";
}
"#
}

fn emit_command(
    body: &mut String,
    vars: &mut BTreeMap<OcctSlot, String>,
    part_key: &str,
    command: &OcctCommand,
) -> AppResult<()> {
    let var = slot_var(command.output);
    if !command.keywords.is_empty()
        && !matches!(
            command.op,
            OcctOp::Box
                | OcctOp::Sphere
                | OcctOp::Cylinder
                | OcctOp::Cone
                | OcctOp::Torus
                | OcctOp::Wedge
                | OcctOp::Profile
                | OcctOp::Plane
                | OcctOp::Location
                | OcctOp::PathFrame
                | OcctOp::Place
                | OcctOp::ClipBox
                | OcctOp::Fillet
                | OcctOp::Chamfer
                | OcctOp::Shell
                | OcctOp::Draft
                | OcctOp::Bspline
                | OcctOp::Sweep
        )
    {
        return Err(AppError::validation(format!(
            "Direct OCCT executor does not support `{}` keyword arguments yet.",
            op_name(command.op)
        )));
    }
    match command.op {
        OcctOp::Box => {
            let args = box_args(&command.args, &command.keywords)?;
            emit_box_operation(body, &var, args.width, args.depth, args.height, args.align);
        }
        OcctOp::Sphere => {
            let args = sphere_args(&command.args, &command.keywords)?;
            emit_sphere_operation(body, &var, args.radius, args.align);
        }
        OcctOp::Cylinder => {
            let args = cylinder_args(&command.args, &command.keywords)?;
            emit_cylinder_operation(body, &var, args.radius, args.height, args.align);
        }
        OcctOp::Cone => {
            let args = cone_args(&command.args, &command.keywords)?;
            emit_cone_operation(
                body,
                &var,
                args.radius1,
                args.radius2,
                args.height,
                args.align,
            );
        }
        OcctOp::Torus => {
            let args = torus_args(&command.args, &command.keywords)?;
            emit_torus_operation(body, &var, args.major, args.minor, args.align);
        }
        OcctOp::Circle => {
            let [radius] = numeric_args(&command.args)?;
            body.push_str(&format!(
                "    gp_Circ {var}_circle(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {radius});\n    TopoDS_Wire {var}_wire = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge({var}_circle).Edge()).Wire();\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n"
            ));
        }
        OcctOp::Wedge => {
            let args = wedge_args(&command.args, &command.keywords)?;
            emit_wedge_operation(body, &var, args.dims, args.align);
        }
        OcctOp::Slot => {
            let [length, width] = numeric_args(&command.args)?;
            let r = width / 2.0;
            let half = (length - width) / 2.0;
            body.push_str(&format!(
                "    BRepBuilderAPI_MakeWire {var}_wire_builder;\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({nh}, {nr}, 0), gp_Pnt({half}, {nr}, 0)).Edge());\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({half}, {nr}, 0), gp_Pnt({hr}, 0, 0), gp_Pnt({half}, {r}, 0)).Value()).Edge());\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({half}, {r}, 0), gp_Pnt({nh}, {r}, 0)).Edge());\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({nh}, {r}, 0), gp_Pnt({nhr}, 0, 0), gp_Pnt({nh}, {nr}, 0)).Value()).Edge());\n    TopoDS_Wire {var}_wire = {var}_wire_builder.Wire();\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n",
                nh = -half,
                nr = -r,
                hr = half + r,
                nhr = -half - r,
            ));
        }
        OcctOp::SlotArc => {
            let [radius, start_deg, end_deg, width] = numeric_args(&command.args)?;
            let r = width / 2.0;
            let ro = radius + r;
            let ri = radius - r;
            let a0 = start_deg.to_radians();
            let a1 = end_deg.to_radians();
            let am = (a0 + a1) / 2.0;
            let pt = |rad: f64, ang: f64| (rad * ang.cos(), rad * ang.sin());
            let (ox0, oy0) = pt(ro, a0);
            let (oxm, oym) = pt(ro, am);
            let (ox1, oy1) = pt(ro, a1);
            let (ix0, iy0) = pt(ri, a0);
            let (ixm, iym) = pt(ri, am);
            let (ix1, iy1) = pt(ri, a1);
            let cap1x = radius * a1.cos() - r * a1.sin();
            let cap1y = radius * a1.sin() + r * a1.cos();
            let cap0x = radius * a0.cos() + r * a0.sin();
            let cap0y = radius * a0.sin() - r * a0.cos();
            body.push_str(&format!(
                "    BRepBuilderAPI_MakeWire {var}_wire_builder;\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({ox0}, {oy0}, 0), gp_Pnt({oxm}, {oym}, 0), gp_Pnt({ox1}, {oy1}, 0)).Value()).Edge());\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({ox1}, {oy1}, 0), gp_Pnt({cap1x}, {cap1y}, 0), gp_Pnt({ix1}, {iy1}, 0)).Value()).Edge());\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({ix1}, {iy1}, 0), gp_Pnt({ixm}, {iym}, 0), gp_Pnt({ix0}, {iy0}, 0)).Value()).Edge());\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({ix0}, {iy0}, 0), gp_Pnt({cap0x}, {cap0y}, 0), gp_Pnt({ox0}, {oy0}, 0)).Value()).Edge());\n    TopoDS_Wire {var}_wire = {var}_wire_builder.Wire();\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n"
            ));
        }
        OcctOp::Ellipse => {
            let [rx, ry] = numeric_args(&command.args)?;
            body.push_str(&format!(
                "    gp_Ax2 {var}_axes = (static_cast<double>({rx}) >= static_cast<double>({ry})) ? gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1), gp_Dir(1, 0, 0)) : gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1), gp_Dir(0, 1, 0));\n    gp_Elips {var}_ellipse({var}_axes, std::max(static_cast<double>({rx}), static_cast<double>({ry})), std::min(static_cast<double>({rx}), static_cast<double>({ry})));\n    TopoDS_Wire {var}_wire = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge({var}_ellipse).Edge()).Wire();\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n"
            ));
        }
        OcctOp::Rectangle => {
            let [width, height] = numeric_args(&command.args)?;
            let half_width = width / 2.0;
            let half_height = height / 2.0;
            let points = [
                [-half_width, -half_height],
                [half_width, -half_height],
                [half_width, half_height],
                [-half_width, half_height],
            ];
            emit_polygon_face(body, &var, &points)?;
        }
        OcctOp::RoundedRectangle => {
            let [width, height, radius] = numeric_prefix_args::<3>(&command.args)?;
            emit_rounded_rectangle_face(body, &var, width, height, radius);
        }
        OcctOp::RoundedPolygon => {
            let points = point2_list_arg(&command.args, 0)?;
            let radius = numeric_arg(&command.args, 1)?;
            emit_rounded_polygon_face(body, &var, &points, radius)?;
        }
        OcctOp::Polygon => {
            let points = point2_list_arg(&command.args, 0)?;
            emit_polygon_face(body, &var, &points)?;
        }
        OcctOp::Profile => {
            let profile = profile_refs(&command.args, &command.keywords)?;
            emit_profile_face(body, &var, profile)?;
        }
        OcctOp::MakeFace => {
            let inputs = ref_args(&command.args)?;
            if inputs.len() != 1 {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `make-face` expects one wire, got {}.",
                    inputs.len()
                )));
            }
            emit_make_face_operation(body, &var, slot_var(inputs[0]));
        }
        OcctOp::ImportStl => {
            let path = stringish_arg(&command.args, 0, "import-stl path")?;
            emit_import_stl_operation(body, &var, &path);
        }
        OcctOp::Extrude => {
            let profile = ref_arg(&command.args, 0)?;
            let distance = numeric_arg(&command.args, 1)?;
            body.push_str(&format!(
                "    TopoDS_Shape {var} = BRepPrimAPI_MakePrism({}, gp_Vec(0, 0, {distance})).Shape();\n",
                slot_var(profile)
            ));
        }
        OcctOp::Revolve => {
            let profile = ref_arg(&command.args, 0)?;
            let angle_radians = numeric_arg(&command.args, 1)?.to_radians();
            body.push_str(&format!(
                "    gp_Trsf {var}_profile_trsf;\n    {var}_profile_trsf.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0)), 1.5707963267948966);\n    TopoDS_Shape {var}_profile = BRepBuilderAPI_Transform({}, {var}_profile_trsf, true).Shape();\n    TopoDS_Shape {var} = BRepPrimAPI_MakeRevol({var}_profile, gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {angle_radians}).Shape();\n",
                slot_var(profile)
            ));
        }
        OcctOp::Loft => {
            let distance = numeric_arg(&command.args, 0)?;
            let profiles = ref_args_after(&command.args, 1)?;
            emit_loft_operation(body, &var, distance, profiles)?;
        }
        OcctOp::Sweep => {
            let profile = ref_arg(&command.args, 0)?;
            let path = ref_arg(&command.args, 1)?;
            let frenet = optional_bool_keyword(&command.keywords, &["frenet"]).unwrap_or(false);
            emit_sweep_operation(body, &var, slot_var(profile), slot_var(path), frenet);
        }
        OcctOp::Twist => {
            let height = numeric_arg(&command.args, 0)?;
            let angle_radians = numeric_arg(&command.args, 1)?.to_radians();
            let profile = ref_arg(&command.args, 2)?;
            emit_twist_operation(body, &var, height, angle_radians, slot_var(profile));
        }
        OcctOp::Taper => {
            let (height, scale_x, scale_y, profile) = taper_args(&command.args)?;
            emit_taper_operation(body, &var, height, scale_x, scale_y, slot_var(profile));
        }
        OcctOp::Draft => {
            let angle_radians = numeric_arg(&command.args, 0)?.to_radians();
            let input = ref_arg(&command.args, 1)?;
            let neutral_z =
                optional_numeric_keyword(&command.keywords, &["neutral-z", "neutral_z"])
                    .unwrap_or(0.0);
            emit_draft_operation(body, &var, slot_var(input), angle_radians, neutral_z);
        }
        OcctOp::Path => {
            let points = point3_sequence_args(&command.args)?;
            emit_path_wire(body, &var, &points)?;
        }
        OcctOp::HelixPath => {
            let [radius, pitch, height] = numeric_prefix_args::<3>(&command.args)?;
            let lefthand = bool_arg(&command.args, 3)?;
            emit_helix_path_wire(body, &var, radius, pitch, height, lefthand)?;
        }
        OcctOp::BezierPath => {
            let points = point3_sequence_args(&command.args)?;
            emit_bezier_path_wire(body, &var, &points)?;
        }
        OcctOp::Bspline => {
            let args = bspline_args(&command.args, &command.keywords)?;
            emit_bspline_shape(body, &var, &args)?;
        }
        OcctOp::Plane => {
            let args = plane_args(&command.args, &command.keywords)?;
            emit_plane_operation(body, &var, args.origin, args.x_axis, args.normal);
        }
        OcctOp::Location => {
            let args = location_args(&command.args, &command.keywords)?;
            emit_location_operation(
                body,
                &var,
                args.frame.map(slot_var),
                args.offset,
                args.rotate,
            );
        }
        OcctOp::PathFrame => {
            let args = path_frame_args(&command.args, &command.keywords)?;
            emit_path_frame_operation(body, &var, slot_var(args.path), args.at, args.up);
        }
        OcctOp::Place => {
            let args = place_args(&command.args, &command.keywords)?;
            emit_place_operation(
                body,
                &var,
                slot_var(args.frame),
                slot_var(args.shape),
                args.offset,
                args.rotate,
            );
        }
        OcctOp::ClipBox => {
            let args = clip_box_args(&command.args, &command.keywords)?;
            emit_clip_box_operation(body, &var, slot_var(args.shape), args.x, args.y, args.z);
        }
        OcctOp::LinearArray => {
            let count = count_arg(&command.args, 0, "linear-array count")?;
            let [x, y, z] = numeric_args(&command.args[1..4])?;
            let input = ref_arg(&command.args, 4)?;
            emit_linear_array_operation(body, &var, slot_var(input), count, [x, y, z]);
        }
        OcctOp::RadialArray => {
            let count = count_arg(&command.args, 0, "radial-array count")?;
            let step_degrees = numeric_arg(&command.args, 1)?;
            let radius = numeric_arg(&command.args, 2)?;
            let input = ref_arg(&command.args, 3)?;
            emit_radial_array_operation(body, &var, slot_var(input), count, step_degrees, radius);
        }
        OcctOp::GridArray => {
            let rows = count_arg(&command.args, 0, "grid-array rows")?;
            let cols = count_arg(&command.args, 1, "grid-array cols")?;
            let dx = numeric_arg(&command.args, 2)?;
            let dy = numeric_arg(&command.args, 3)?;
            let input = ref_arg(&command.args, 4)?;
            emit_grid_array_operation(body, &var, slot_var(input), rows, cols, dx, dy);
        }
        OcctOp::ArcArray => {
            let count = count_arg(&command.args, 0, "arc-array count")?;
            let radius = numeric_arg(&command.args, 1)?;
            let start_degrees = numeric_arg(&command.args, 2)?;
            let end_degrees = numeric_arg(&command.args, 3)?;
            let input = ref_arg(&command.args, 4)?;
            emit_arc_array_operation(
                body,
                &var,
                slot_var(input),
                count,
                radius,
                start_degrees,
                end_degrees,
            );
        }
        OcctOp::Offset => {
            let amount = numeric_arg(&command.args, 0)?;
            let input = ref_arg(&command.args, 1)?;
            emit_offset_operation(body, &var, slot_var(input), amount);
        }
        OcctOp::Fillet => {
            let radius = positive_radius_arg(&command.args, 0, "fillet")?;
            let input = ref_arg(&command.args, 1)?;
            let selector = edge_selector(&command.keywords, "fillet")?;
            let to_radius = optional_numeric_keyword(&command.keywords, &["to-radius", "to_radius"]);
            emit_edge_radius_operation(
                body,
                &var,
                "fillet",
                "BRepFilletAPI_MakeFillet",
                slot_var(input),
                radius,
                to_radius,
                part_key,
                selector.as_ref(),
            );
        }
        OcctOp::Chamfer => {
            let distance = positive_radius_arg(&command.args, 0, "chamfer")?;
            let input = ref_arg(&command.args, 1)?;
            let selector = edge_selector(&command.keywords, "chamfer")?;
            emit_edge_radius_operation(
                body,
                &var,
                "chamfer",
                "BRepFilletAPI_MakeChamfer",
                slot_var(input),
                distance,
                None,
                part_key,
                selector.as_ref(),
            );
        }
        OcctOp::Shell => {
            let thickness = positive_radius_arg(&command.args, 0, "shell")?;
            let input = ref_arg(&command.args, 1)?;
            let selector = shell_face_selector(&command.keywords, "shell")?;
            emit_shell_operation(
                body,
                &var,
                slot_var(input),
                thickness,
                part_key,
                selector.as_ref(),
            );
        }
        OcctOp::Translate => {
            let [x, y, z] = numeric_prefix_args::<3>(&command.args)?;
            let input = ref_arg(&command.args, 3)?;
            body.push_str(&format!(
                "    gp_Trsf {var}_trsf;\n    {var}_trsf.SetTranslation(gp_Vec({x}, {y}, {z}));\n    TopoDS_Shape {var} = BRepBuilderAPI_Transform({}, {var}_trsf, true).Shape();\n",
                slot_var(input)
            ));
        }
        OcctOp::Rotate => {
            let [x, y, z] = numeric_prefix_args::<3>(&command.args)?;
            let input = ref_arg(&command.args, 3)?;
            let x = x.to_radians();
            let y = y.to_radians();
            let z = z.to_radians();
            body.push_str(&format!(
                "    gp_Trsf {var}_trsf_x;\n    {var}_trsf_x.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0)), {x});\n    TopoDS_Shape {var}_after_x = BRepBuilderAPI_Transform({}, {var}_trsf_x, true).Shape();\n    gp_Trsf {var}_trsf_y;\n    {var}_trsf_y.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 1, 0)), {y});\n    TopoDS_Shape {var}_after_y = BRepBuilderAPI_Transform({var}_after_x, {var}_trsf_y, true).Shape();\n    gp_Trsf {var}_trsf_z;\n    {var}_trsf_z.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {z});\n    TopoDS_Shape {var} = BRepBuilderAPI_Transform({var}_after_y, {var}_trsf_z, true).Shape();\n",
                slot_var(input)
            ));
        }
        OcctOp::Scale => {
            let ([x, y, z], input) = scale_args(&command.args)?;
            body.push_str(&format!(
                "    gp_GTrsf {var}_gtrsf;\n    {var}_gtrsf.SetValue(1, 1, {x});\n    {var}_gtrsf.SetValue(2, 2, {y});\n    {var}_gtrsf.SetValue(3, 3, {z});\n    TopoDS_Shape {var} = BRepBuilderAPI_GTransform({}, {var}_gtrsf, true).Shape();\n",
                slot_var(input)
            ));
        }
        OcctOp::Mirror => {
            let axis = stringish_arg(&command.args, 0, "mirror axis")?;
            let offset = numeric_arg(&command.args, 1)?;
            let input = ref_arg(&command.args, 2)?;
            emit_mirror_operation(body, &var, slot_var(input), &axis, offset)?;
        }
        OcctOp::Compound => {
            let inputs = ref_args(&command.args)?;
            if inputs.is_empty() {
                return Err(AppError::validation(
                    "Direct OCCT executor `compound` requires at least one operand.",
                ));
            }
            body.push_str(&format!(
                "    BRep_Builder {var}_builder;\n    TopoDS_Compound {var}_compound;\n    {var}_builder.MakeCompound({var}_compound);\n"
            ));
            for input in inputs {
                body.push_str(&format!(
                    "    {var}_builder.Add({var}_compound, {});\n",
                    slot_var(input)
                ));
            }
            body.push_str(&format!("    TopoDS_Shape {var} = {var}_compound;\n"));
        }
        OcctOp::Union => emit_boolean_fold(
            body,
            &var,
            "union",
            "BRepAlgoAPI_Fuse",
            ref_args(&command.args)?,
        )?,
        OcctOp::Difference => emit_boolean_fold(
            body,
            &var,
            "difference",
            "BRepAlgoAPI_Cut",
            ref_args(&command.args)?,
        )?,
        OcctOp::Intersection => emit_boolean_fold(
            body,
            &var,
            "intersection",
            "BRepAlgoAPI_Common",
            ref_args(&command.args)?,
        )?,
    }
    vars.insert(command.output, var);
    Ok(())
}

fn emit_top_level_compound(body: &mut String, part_roots: &[String]) {
    body.push_str(
        "    BRep_Builder model_compound_builder;\n    TopoDS_Compound model_compound;\n    model_compound_builder.MakeCompound(model_compound);\n",
    );
    for root in part_roots {
        body.push_str(&format!(
            "    model_compound_builder.Add(model_compound, {root});\n"
        ));
    }
    body.push_str("    TopoDS_Shape model_compound_shape = model_compound;\n");
}

fn numeric_args<const N: usize>(args: &[OcctArg]) -> AppResult<[f64; N]> {
    if args.len() != N {
        return Err(AppError::validation(format!(
            "Direct OCCT executor expected {N} numeric argument(s), got {}.",
            args.len()
        )));
    }
    let mut values = [0.0_f64; N];
    for (index, arg) in args.iter().enumerate() {
        let OcctArg::Number(value) = arg else {
            return Err(AppError::validation(format!(
                "Direct OCCT executor expected literal number at arg {index}, got {:?}.",
                arg
            )));
        };
        values[index] = *value;
    }
    Ok(values)
}

fn numeric_arg(args: &[OcctArg], index: usize) -> AppResult<f64> {
    match args.get(index) {
        Some(OcctArg::Number(value)) => Ok(*value),
        Some(other) => Err(AppError::validation(format!(
            "Direct OCCT executor expected literal number at arg {index}, got {:?}.",
            other
        ))),
        None => Err(AppError::validation(format!(
            "Direct OCCT executor expected literal number at arg {index}, got no argument."
        ))),
    }
}

fn bool_arg(args: &[OcctArg], index: usize) -> AppResult<bool> {
    match args.get(index) {
        Some(OcctArg::Boolean(value)) => Ok(*value),
        Some(other) => Err(AppError::validation(format!(
            "Direct OCCT executor expected literal boolean at arg {index}, got {:?}.",
            other
        ))),
        None => Err(AppError::validation(format!(
            "Direct OCCT executor expected literal boolean at arg {index}, got no argument."
        ))),
    }
}

fn stringish_arg(args: &[OcctArg], index: usize, label: &str) -> AppResult<String> {
    match args.get(index) {
        Some(OcctArg::Text(value)) | Some(OcctArg::Symbol(value)) => Ok(value.clone()),
        Some(other) => Err(AppError::validation(format!(
            "Direct OCCT executor `{label}` expects text or symbol, got {:?}.",
            other
        ))),
        None => Err(AppError::validation(format!(
            "Direct OCCT executor `{label}` got no argument."
        ))),
    }
}

fn edge_selector(keywords: &[OcctKeyword], op_name: &str) -> AppResult<Option<EdgeSelector>> {
    let created_by_slot_index = selector_created_by_slot_index(keywords, op_name, "edges")?;
    let Some(keyword) = selector_keyword(keywords, op_name, "edges")? else {
        return Ok(None);
    };
    match keyword.selector_payload() {
        Some(CoreSelectorPayload::EdgeAll) => {
            if created_by_slot_index.is_some() {
                Ok(Some(EdgeSelector {
                    kind: EdgeSelectorKind::Clauses(Vec::new()),
                    created_by_slot_index,
                }))
            } else {
                Ok(None)
            }
        }
        Some(CoreSelectorPayload::EdgeTargetIds(target_ids)) => Ok(Some(EdgeSelector {
            kind: EdgeSelectorKind::TargetIds(target_ids.clone()),
            created_by_slot_index,
        })),
        Some(CoreSelectorPayload::EdgeTag(tag_name)) => Ok(Some(EdgeSelector {
            kind: EdgeSelectorKind::TargetIds(vec![format!("tag:{tag_name}")]),
            created_by_slot_index,
        })),
        Some(CoreSelectorPayload::EdgeClauses(clauses)) => Ok(Some(EdgeSelector {
            kind: EdgeSelectorKind::Clauses(clauses.clone()),
            created_by_slot_index,
        })),
        Some(CoreSelectorPayload::FaceTag(tag_name)) => Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name} :edges` got face selector tag `{tag_name}`.",
        ))),
        Some(CoreSelectorPayload::FaceTargetIds(target_ids)) => Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name} :edges` got face selector payload {:?}.",
            target_ids
        ))),
        Some(CoreSelectorPayload::FaceClauses(clauses)) => Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name} :edges` got face selector clauses {:?}.",
            clauses
        ))),
        None => Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name} :edges` requires typed selector payload.",
        ))),
    }
}

fn shell_face_selector(
    keywords: &[OcctKeyword],
    op_name: &str,
) -> AppResult<Option<ShellFaceSelector>> {
    let created_by_slot_index = selector_created_by_slot_index(keywords, op_name, "faces")?;
    let Some(keyword) = selector_keyword(keywords, op_name, "faces")? else {
        return Ok(None);
    };
    match keyword.selector_payload() {
        Some(CoreSelectorPayload::FaceTargetIds(target_ids)) => Ok(Some(ShellFaceSelector {
            kind: ShellFaceSelectorKind::TargetIds(target_ids.clone()),
            created_by_slot_index,
        })),
        Some(CoreSelectorPayload::FaceTag(tag_name)) => Ok(Some(ShellFaceSelector {
            kind: ShellFaceSelectorKind::TargetIds(vec![format!("tag:{tag_name}")]),
            created_by_slot_index,
        })),
        Some(CoreSelectorPayload::FaceClauses(clauses)) => Ok(Some(ShellFaceSelector {
            kind: ShellFaceSelectorKind::Clauses(clauses.clone()),
            created_by_slot_index,
        })),
        Some(
            CoreSelectorPayload::EdgeAll
            | CoreSelectorPayload::EdgeClauses(_)
            | CoreSelectorPayload::EdgeTag(_),
        ) => Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name} :faces` got edge selector payload.",
        ))),
        Some(CoreSelectorPayload::EdgeTargetIds(target_ids)) => Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name} :faces` got edge selector payload {:?}.",
            target_ids
        ))),
        None => Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name} :faces` requires typed selector payload.",
        ))),
    }
}

fn selector_created_by_slot_index(
    keywords: &[OcctKeyword],
    op_name: &str,
    selector_keyword_name: &str,
) -> AppResult<Option<u64>> {
    let mut created_by_slot_index = None;
    let mut saw_selector_keyword = false;

    for keyword in keywords {
        match keyword.name.as_str() {
            name if name == selector_keyword_name => saw_selector_keyword = true,
            "created-by" => {
                if created_by_slot_index.is_some() {
                    return Err(AppError::validation(format!(
                        "Direct OCCT executor `{op_name}` got duplicate `:created-by` keywords.",
                    )));
                }
                let slot = match keyword.source_arg() {
                    OcctArg::Ref(slot) => slot.0,
                    other => {
                        return Err(AppError::validation(format!(
                            "Direct OCCT executor `{op_name} :created-by` requires a build shape reference, got {:?}.",
                            other
                        )))
                    }
                };
                created_by_slot_index = Some(slot);
            }
            _ => {}
        }
    }

    if created_by_slot_index.is_some() && !saw_selector_keyword {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name} :created-by` requires `:{selector_keyword_name}`.",
        )));
    }

    Ok(created_by_slot_index)
}

fn optional_numeric_keyword(keywords: &[OcctKeyword], names: &[&str]) -> Option<f64> {
    keywords.iter().find_map(|keyword| {
        if names.contains(&keyword.name.as_str()) {
            if let OcctArg::Number(value) = keyword.source_arg() {
                return Some(*value);
            }
        }
        None
    })
}

fn selector_keyword<'a>(
    keywords: &'a [OcctKeyword],
    op_name: &str,
    keyword_name: &str,
) -> AppResult<Option<&'a OcctKeyword>> {
    let mut selector = None;
    for keyword in keywords {
        match keyword.name.as_str() {
            name if name == keyword_name => {
                if selector.replace(keyword).is_some() {
                    return Err(AppError::validation(format!(
                        "Direct OCCT executor `{op_name}` got duplicate `:{keyword_name}` keywords.",
                    )));
                }
            }
            "created-by" => {}
            // Tapered fillet radius rides alongside the edge selector.
            "to-radius" | "to_radius" => {}
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `{op_name}` does not support keyword `:{other}`.",
                )));
            }
        }
    }
    Ok(selector)
}

fn positive_radius_arg(args: &[OcctArg], index: usize, op_name: &str) -> AppResult<f64> {
    let radius = numeric_arg(args, index)?;
    if radius <= 0.0 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name}` expects a positive radius, got {radius}."
        )));
    }
    Ok(radius)
}

fn count_arg(args: &[OcctArg], index: usize, label: &str) -> AppResult<usize> {
    let value = numeric_arg(args, index)?;
    if !value.is_finite() {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `{label}` expects a finite count, got {value}."
        )));
    }
    Ok(value.round().max(1.0) as usize)
}

fn effective_program_parameters(program: &CoreProgram, overrides: &DesignParams) -> DesignParams {
    let mut parameters = DesignParams::new();
    for parameter in &program.parameters {
        parameters.insert(
            parameter.key.clone(),
            param_value_from_core_default(&parameter.default_value),
        );
    }
    for (key, value) in overrides {
        parameters.insert(key.clone(), value.clone());
    }
    parameters
}

fn param_value_from_core_default(value: &CoreParameterValue) -> ParamValue {
    match value {
        CoreParameterValue::Number(value) => ParamValue::Number(*value),
        CoreParameterValue::Boolean(value) => ParamValue::Boolean(*value),
        CoreParameterValue::Text(value)
        | CoreParameterValue::Choice(value)
        | CoreParameterValue::Image(value) => ParamValue::String(value.clone()),
    }
}

fn resolve_plan_parameters(plan: &OcctPlan, parameters: &DesignParams) -> AppResult<OcctPlan> {
    let mut env = BTreeMap::new();
    for parameter in &plan.parameters {
        let Some(value) = parameters.get(&parameter.key) else {
            return Err(AppError::validation(format!(
                "Direct OCCT executor missing runtime parameter `{}`.",
                parameter.key
            )));
        };
        validate_parameter_value(&parameter.key, parameter.kind, value)?;
        env.insert(parameter.key.clone(), occt_arg_from_param_value(value)?);
    }

    let mut resolved = plan.clone();
    for part in &mut resolved.parts {
        for command in &mut part.commands {
            for arg in &mut command.args {
                *arg = resolve_occt_arg(arg, &env)?;
            }
            for keyword in &mut command.keywords {
                *keyword.source_arg_mut() = resolve_occt_arg(keyword.source_arg(), &env)?;
            }
        }
    }
    Ok(resolved)
}

fn validate_parameter_value(
    key: &str,
    kind: OcctParameterKind,
    value: &ParamValue,
) -> AppResult<()> {
    let valid = matches!(
        (kind, value),
        (OcctParameterKind::Number, ParamValue::Number(_))
            | (OcctParameterKind::Boolean, ParamValue::Boolean(_))
            | (OcctParameterKind::Text, ParamValue::String(_))
            | (OcctParameterKind::Choice, ParamValue::String(_))
            | (OcctParameterKind::Choice, ParamValue::Number(_))
            | (OcctParameterKind::Image, ParamValue::String(_))
    );
    if valid {
        return Ok(());
    }
    Err(AppError::validation(format!(
        "Direct OCCT executor parameter `{key}` expected {}, got {}.",
        parameter_kind_name(kind),
        value.kind()
    )))
}

fn parameter_kind_name(kind: OcctParameterKind) -> &'static str {
    match kind {
        OcctParameterKind::Number => "number",
        OcctParameterKind::Boolean => "boolean",
        OcctParameterKind::Text => "text",
        OcctParameterKind::Choice => "choice",
        OcctParameterKind::Image => "image",
    }
}

fn occt_arg_from_param_value(value: &ParamValue) -> AppResult<OcctArg> {
    match value {
        ParamValue::Number(value) => Ok(OcctArg::Number(*value)),
        ParamValue::Boolean(value) => Ok(OcctArg::Boolean(*value)),
        ParamValue::String(value) => Ok(OcctArg::Text(value.clone())),
        ParamValue::Null => Err(AppError::validation(
            "Direct OCCT executor does not support null runtime parameters.",
        )),
    }
}

fn resolve_occt_arg(arg: &OcctArg, env: &BTreeMap<String, OcctArg>) -> AppResult<OcctArg> {
    match arg {
        OcctArg::Param(key) => env.get(key).cloned().ok_or_else(|| {
            AppError::validation(format!(
                "Direct OCCT executor could not resolve runtime parameter `{key}`."
            ))
        }),
        OcctArg::List(items) => Ok(OcctArg::List(
            items
                .iter()
                .map(|item| resolve_occt_arg(item, env))
                .collect::<AppResult<Vec<_>>>()?,
        )),
        other => Ok(other.clone()),
    }
}

fn numeric_prefix_args<const N: usize>(args: &[OcctArg]) -> AppResult<[f64; N]> {
    if args.len() < N {
        return Err(AppError::validation(format!(
            "Direct OCCT executor expected at least {N} numeric argument(s), got {}.",
            args.len()
        )));
    }
    numeric_args(&args[..N])
}

fn ref_arg(args: &[OcctArg], index: usize) -> AppResult<OcctSlot> {
    match args.get(index) {
        Some(OcctArg::Ref(slot)) => Ok(*slot),
        Some(other) => Err(AppError::validation(format!(
            "Direct OCCT executor expected shape reference at arg {index}, got {:?}.",
            other
        ))),
        None => Err(AppError::validation(format!(
            "Direct OCCT executor expected shape reference at arg {index}, got no argument."
        ))),
    }
}

fn ref_args(args: &[OcctArg]) -> AppResult<Vec<OcctSlot>> {
    args.iter()
        .enumerate()
        .map(|(index, arg)| match arg {
            OcctArg::Ref(slot) => Ok(*slot),
            other => Err(AppError::validation(format!(
                "Direct OCCT executor expected shape reference at arg {index}, got {:?}.",
                other
            ))),
        })
        .collect()
}

fn ref_args_after(args: &[OcctArg], start: usize) -> AppResult<Vec<OcctSlot>> {
    if args.len() <= start {
        return Err(AppError::validation(format!(
            "Direct OCCT executor expected shape reference at arg {start}, got no argument."
        )));
    }
    args.iter()
        .enumerate()
        .skip(start)
        .map(|(index, arg)| match arg {
            OcctArg::Ref(slot) => Ok(*slot),
            other => Err(AppError::validation(format!(
                "Direct OCCT executor expected shape reference at arg {index}, got {:?}.",
                other
            ))),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProfileRefs {
    outer: Vec<OcctSlot>,
    holes: Vec<OcctSlot>,
}

fn profile_refs(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<ProfileRefs> {
    let mut outer = Vec::new();
    let mut holes = Vec::new();

    if keywords.is_empty() {
        outer.extend(ref_args(args)?);
    } else {
        if !args.is_empty() {
            return Err(AppError::validation(
                "Direct OCCT executor `profile` does not mix positional loops with keyword loops.",
            ));
        }
        for keyword in keywords {
            match keyword.name.as_str() {
                "outer" => {
                    outer.extend(ref_collection_arg(keyword.source_arg(), "profile :outer")?)
                }
                "holes" => {
                    holes.extend(ref_collection_arg(keyword.source_arg(), "profile :holes")?)
                }
                other => {
                    return Err(AppError::validation(format!(
                        "Direct OCCT executor `profile` does not recognize `:{other}`."
                    )));
                }
            }
        }
    }

    if outer.is_empty() {
        return Err(AppError::validation(
            "Direct OCCT executor `profile` needs at least one outer loop.",
        ));
    }
    Ok(ProfileRefs { outer, holes })
}

fn ref_collection_arg(arg: &OcctArg, label: &str) -> AppResult<Vec<OcctSlot>> {
    match arg {
        OcctArg::Ref(slot) => Ok(vec![*slot]),
        OcctArg::List(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| match item {
                OcctArg::Ref(slot) => Ok(*slot),
                other => Err(AppError::validation(format!(
                    "Direct OCCT executor `{label}` expected shape reference at index {index}, got {:?}.",
                    other
                ))),
            })
            .collect(),
        other => Err(AppError::validation(format!(
            "Direct OCCT executor `{label}` expected shape reference or reference list, got {:?}.",
            other
        ))),
    }
}

#[derive(Debug, Clone, Copy)]
enum AxisAlign {
    Min,
    Center,
    Max,
}

#[derive(Debug, Clone, Copy)]
struct BoxArgs {
    width: f64,
    depth: f64,
    height: f64,
    align: [AxisAlign; 3],
}

#[derive(Debug, Clone, Copy)]
struct SphereArgs {
    radius: f64,
    align: [AxisAlign; 3],
}

#[derive(Debug, Clone, Copy)]
struct CylinderArgs {
    radius: f64,
    height: f64,
    align: [AxisAlign; 3],
}

#[derive(Debug, Clone, Copy)]
struct ConeArgs {
    radius1: f64,
    radius2: f64,
    height: f64,
    align: [AxisAlign; 3],
}

#[derive(Debug, Clone, Copy)]
struct TorusArgs {
    major: f64,
    minor: f64,
    align: [AxisAlign; 3],
}

#[derive(Debug, Clone, Copy)]
struct WedgeArgs {
    dims: [f64; 7],
    align: [AxisAlign; 3],
}

fn box_args(args: &[OcctArg], keywords: &[super::direct_occt::OcctKeyword]) -> AppResult<BoxArgs> {
    let [width, depth, height] = numeric_args(args)?;
    let mut align = [AxisAlign::Center, AxisAlign::Center, AxisAlign::Min];
    for keyword in keywords {
        match keyword.name.as_str() {
            "align" => align = align_tuple_arg(keyword.source_arg(), "box :align")?,
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `box` does not recognize `:{other}`."
                )));
            }
        }
    }
    Ok(BoxArgs {
        width,
        depth,
        height,
        align,
    })
}

fn sphere_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<SphereArgs> {
    let [radius] = numeric_args(args)?;
    let mut align = [AxisAlign::Center, AxisAlign::Center, AxisAlign::Center];
    apply_align_keywords("sphere", keywords, &mut align)?;
    Ok(SphereArgs { radius, align })
}

fn cylinder_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<CylinderArgs> {
    let [radius, height] = numeric_prefix_args::<2>(args)?;
    let mut align = [AxisAlign::Center, AxisAlign::Center, AxisAlign::Min];
    apply_align_keywords("cylinder", keywords, &mut align)?;
    Ok(CylinderArgs {
        radius,
        height,
        align,
    })
}

fn cone_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<ConeArgs> {
    let [radius1, radius2, height] = numeric_prefix_args::<3>(args)?;
    let mut align = [AxisAlign::Center, AxisAlign::Center, AxisAlign::Min];
    apply_align_keywords("cone", keywords, &mut align)?;
    Ok(ConeArgs {
        radius1,
        radius2,
        height,
        align,
    })
}

fn torus_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<TorusArgs> {
    let [major, minor] = numeric_prefix_args::<2>(args)?;
    let mut align = [AxisAlign::Center, AxisAlign::Center, AxisAlign::Center];
    apply_align_keywords("torus", keywords, &mut align)?;
    Ok(TorusArgs {
        major,
        minor,
        align,
    })
}

fn wedge_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<WedgeArgs> {
    let dims = numeric_prefix_args::<7>(args)?;
    let mut align = [AxisAlign::Center, AxisAlign::Center, AxisAlign::Center];
    apply_align_keywords("wedge", keywords, &mut align)?;
    Ok(WedgeArgs { dims, align })
}

fn apply_align_keywords(
    op: &str,
    keywords: &[super::direct_occt::OcctKeyword],
    align: &mut [AxisAlign; 3],
) -> AppResult<()> {
    for keyword in keywords {
        match keyword.name.as_str() {
            "align" => *align = align_tuple_arg(keyword.source_arg(), &format!("{op} :align"))?,
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `{op}` does not recognize `:{other}`."
                )));
            }
        }
    }
    Ok(())
}

fn align_tuple_arg(arg: &OcctArg, context: &str) -> AppResult<[AxisAlign; 3]> {
    let OcctArg::List(items) = arg else {
        return Err(AppError::validation(format!(
            "{context} expects `(x y z)` with `min`, `center`, or `max`."
        )));
    };
    if items.len() != 3 {
        return Err(AppError::validation(format!(
            "{context} expects exactly three axis symbols."
        )));
    }
    Ok([
        parse_align_axis(&items[0], context)?,
        parse_align_axis(&items[1], context)?,
        parse_align_axis(&items[2], context)?,
    ])
}

fn parse_align_axis(arg: &OcctArg, context: &str) -> AppResult<AxisAlign> {
    let symbol = match arg {
        OcctArg::Symbol(value) | OcctArg::Text(value) => value.as_str(),
        _ => {
            return Err(AppError::validation(format!(
                "{context} expects `min`, `center`, or `max` symbols."
            )));
        }
    };
    match symbol {
        "min" => Ok(AxisAlign::Min),
        "center" => Ok(AxisAlign::Center),
        "max" => Ok(AxisAlign::Max),
        other => Err(AppError::validation(format!(
            "{context} expects `min`, `center`, or `max`, got `{other}`."
        ))),
    }
}

fn axis_align_offset(size: f64, align: AxisAlign) -> f64 {
    match align {
        AxisAlign::Min => 0.0,
        AxisAlign::Center => -size * 0.5,
        AxisAlign::Max => -size,
    }
}

fn centered_axis_align_offset(size: f64, align: AxisAlign) -> f64 {
    match align {
        AxisAlign::Min => size * 0.5,
        AxisAlign::Center => 0.0,
        AxisAlign::Max => -size * 0.5,
    }
}

#[derive(Debug, Clone, Copy)]
struct PlaneArgs {
    origin: [f64; 3],
    x_axis: [f64; 3],
    normal: [f64; 3],
}

fn plane_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<PlaneArgs> {
    if !args.is_empty() {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `plane` expects keyword arguments only, got {} positional argument(s).",
            args.len()
        )));
    }
    let mut origin = [0.0, 0.0, 0.0];
    let mut x_axis = [1.0, 0.0, 0.0];
    let mut normal = [0.0, 0.0, 1.0];
    for keyword in keywords {
        match keyword.name.as_str() {
            "origin" => origin = point3_like_arg(keyword.source_arg(), "plane :origin")?,
            "x" => x_axis = point3_like_arg(keyword.source_arg(), "plane :x")?,
            "normal" => normal = point3_like_arg(keyword.source_arg(), "plane :normal")?,
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `plane` does not recognize `:{other}`."
                )));
            }
        }
    }
    Ok(PlaneArgs {
        origin,
        x_axis,
        normal,
    })
}

#[derive(Debug, Clone, Copy)]
struct LocationArgs {
    frame: Option<OcctSlot>,
    offset: [f64; 3],
    rotate: [f64; 3],
}

fn location_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<LocationArgs> {
    if args.len() > 1 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `location` expects zero or one frame reference, got {} argument(s).",
            args.len()
        )));
    }
    let frame = if args.is_empty() {
        None
    } else {
        Some(ref_arg(args, 0)?)
    };
    let mut offset = [0.0, 0.0, 0.0];
    let mut rotate = [0.0, 0.0, 0.0];
    for keyword in keywords {
        match keyword.name.as_str() {
            "offset" => offset = point3_like_arg(keyword.source_arg(), "location :offset")?,
            "rotate" => rotate = point3_like_arg(keyword.source_arg(), "location :rotate")?,
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `location` does not recognize `:{other}`."
                )));
            }
        }
    }
    Ok(LocationArgs {
        frame,
        offset,
        rotate,
    })
}

#[derive(Debug, Clone, Copy)]
struct PathFrameArgs {
    path: OcctSlot,
    at: f64,
    up: [f64; 3],
}

fn path_frame_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<PathFrameArgs> {
    if args.len() != 1 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `path-frame` expects one path reference, got {} argument(s).",
            args.len()
        )));
    }
    let path = ref_arg(args, 0)?;
    let mut at = 1.0;
    let mut up = [0.0, 0.0, 1.0];
    for keyword in keywords {
        match keyword.name.as_str() {
            "at" => at = path_frame_anchor_arg(keyword.source_arg())?,
            "up" => up = point3_like_arg(keyword.source_arg(), "path-frame :up")?,
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `path-frame` does not recognize `:{other}`."
                )));
            }
        }
    }
    Ok(PathFrameArgs { path, at, up })
}

fn path_frame_anchor_arg(arg: &OcctArg) -> AppResult<f64> {
    match arg {
        OcctArg::Number(value) => Ok(value.clamp(0.0, 1.0)),
        OcctArg::Symbol(symbol) | OcctArg::Text(symbol) if symbol == "start" => Ok(0.0),
        OcctArg::Symbol(symbol) | OcctArg::Text(symbol) if symbol == "end" => Ok(1.0),
        other => Err(AppError::validation(format!(
            "Direct OCCT executor `path-frame :at` expects `start`, `end`, or a numeric 0..1 anchor, got {:?}.",
            other
        ))),
    }
}

#[derive(Debug, Clone, Copy)]
struct PlaceArgs {
    frame: OcctSlot,
    shape: OcctSlot,
    offset: [f64; 3],
    rotate: [f64; 3],
}

fn place_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<PlaceArgs> {
    if args.len() != 2 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `place` expects frame and shape references, got {} argument(s).",
            args.len()
        )));
    }
    let frame = ref_arg(args, 0)?;
    let shape = ref_arg(args, 1)?;
    let mut offset = [0.0, 0.0, 0.0];
    let mut rotate = [0.0, 0.0, 0.0];
    for keyword in keywords {
        match keyword.name.as_str() {
            "offset" => offset = point3_like_arg(keyword.source_arg(), "place :offset")?,
            "rotate" => rotate = point3_like_arg(keyword.source_arg(), "place :rotate")?,
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `place` does not recognize `:{other}`."
                )));
            }
        }
    }
    Ok(PlaceArgs {
        frame,
        shape,
        offset,
        rotate,
    })
}

fn point3_like_arg(arg: &OcctArg, label: &str) -> AppResult<[f64; 3]> {
    match arg {
        OcctArg::Point3(point) => Ok(*point),
        OcctArg::List(items) if items.len() == 3 => numeric_args::<3>(items),
        other => Err(AppError::validation(format!(
            "Direct OCCT executor `{label}` expects a 3D point, got {:?}.",
            other
        ))),
    }
}

#[derive(Debug, Clone, Copy)]
struct ClipBoxArgs {
    shape: OcctSlot,
    x: [f64; 2],
    y: [f64; 2],
    z: [f64; 2],
}

fn clip_box_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<ClipBoxArgs> {
    if args.len() != 1 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `clip-box` expects one shape reference, got {} argument(s).",
            args.len()
        )));
    }
    let shape = ref_arg(args, 0)?;
    let mut x = None;
    let mut y = None;
    let mut z = None;
    for keyword in keywords {
        match keyword.name.as_str() {
            "x" => x = Some(axis_range_arg(keyword.source_arg(), "clip-box :x")?),
            "y" => y = Some(axis_range_arg(keyword.source_arg(), "clip-box :y")?),
            "z" => z = Some(axis_range_arg(keyword.source_arg(), "clip-box :z")?),
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `clip-box` does not recognize `:{other}`."
                )));
            }
        }
    }
    Ok(ClipBoxArgs {
        shape,
        x: x.ok_or_else(|| AppError::validation("Direct OCCT executor `clip-box` requires `:x`."))?,
        y: y.ok_or_else(|| AppError::validation("Direct OCCT executor `clip-box` requires `:y`."))?,
        z: z.ok_or_else(|| AppError::validation("Direct OCCT executor `clip-box` requires `:z`."))?,
    })
}

fn axis_range_arg(arg: &OcctArg, label: &str) -> AppResult<[f64; 2]> {
    let [a, b] = match arg {
        OcctArg::Point2(point) => *point,
        OcctArg::List(items) if items.len() == 2 => numeric_args::<2>(items)?,
        other => {
            return Err(AppError::validation(format!(
                "Direct OCCT executor `{label}` expects a `(min max)` numeric range, got {:?}.",
                other
            )));
        }
    };
    if (a - b).abs() <= 1.0e-12 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `{label}` must not be zero width."
        )));
    }
    Ok([a.min(b), a.max(b)])
}

fn scale_args(args: &[OcctArg]) -> AppResult<([f64; 3], OcctSlot)> {
    match args.len() {
        2 => {
            let factor = numeric_arg(args, 0)?;
            let input = ref_arg(args, 1)?;
            Ok(([factor, factor, factor], input))
        }
        3 => {
            let [x, y] = numeric_args(&args[..2])?;
            let input = ref_arg(args, 2)?;
            Ok(([x, y, 1.0], input))
        }
        4 => {
            let [x, y, z] = numeric_args(&args[..3])?;
            let input = ref_arg(args, 3)?;
            Ok(([x, y, z], input))
        }
        _ => Err(AppError::validation(format!(
            "Direct OCCT executor `scale` expects one to three factors and a shape, got {} argument(s).",
            args.len()
        ))),
    }
}

fn taper_args(args: &[OcctArg]) -> AppResult<(f64, f64, f64, OcctSlot)> {
    match args.len() {
        3 => {
            let height = numeric_arg(args, 0)?;
            let scale = numeric_arg(args, 1)?;
            let profile = ref_arg(args, 2)?;
            Ok((height, scale, scale, profile))
        }
        4 => {
            let height = numeric_arg(args, 0)?;
            let scale_x = numeric_arg(args, 1)?;
            let scale_y = numeric_arg(args, 2)?;
            let profile = ref_arg(args, 3)?;
            Ok((height, scale_x, scale_y, profile))
        }
        _ => Err(AppError::validation(format!(
            "Direct OCCT executor `taper` expects height, scale, profile or height, scale-x, scale-y, profile, got {} argument(s).",
            args.len()
        ))),
    }
}

fn point2_list_arg(args: &[OcctArg], index: usize) -> AppResult<Vec<[f64; 2]>> {
    point2_list_arg_min(args, index, 3, "polygon")
}

fn point2_list_arg_min(
    args: &[OcctArg],
    index: usize,
    min_points: usize,
    op_name: &str,
) -> AppResult<Vec<[f64; 2]>> {
    let Some(arg) = args.get(index) else {
        return Err(AppError::validation(format!(
            "Direct OCCT executor expected 2D point list at arg {index}, got no argument."
        )));
    };
    let OcctArg::List(items) = arg else {
        return Err(AppError::validation(format!(
            "Direct OCCT executor expected 2D point list at arg {index}, got {:?}.",
            arg
        )));
    };
    let points = items
        .iter()
        .enumerate()
        .map(|(point_index, item)| match item {
            OcctArg::Point2(point) => Ok(*point),
            OcctArg::List(values) if values.len() == 2 => {
                let [x, y] = numeric_args::<2>(values)?;
                Ok([x, y])
            }
            other => Err(AppError::validation(format!(
                "Direct OCCT executor expected 2D point at polygon index {point_index}, got {:?}.",
                other
            ))),
        })
        .collect::<AppResult<Vec<_>>>()?;
    if points.len() < min_points {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name}` requires at least {min_points} points."
        )));
    }
    Ok(points)
}

#[derive(Debug, Clone, PartialEq)]
struct BsplineArgs {
    points: Vec<[f64; 2]>,
    closed: bool,
    tangents: Option<Vec<[f64; 2]>>,
    tangent_scalars: Option<Vec<f64>>,
}

fn bspline_args(
    args: &[OcctArg],
    keywords: &[super::direct_occt::OcctKeyword],
) -> AppResult<BsplineArgs> {
    let mut closed = if args.len() > 1 {
        bool_arg(args, 1)?
    } else {
        false
    };
    let mut tangents = None;
    let mut tangent_scalars = None;

    for keyword in keywords {
        if keyword.selector_payload().is_some() {
            return Err(AppError::validation(
                "Direct OCCT executor `bspline` keywords expect arg values only.",
            ));
        }
        match keyword.name.as_str() {
            "closed" => closed = bool_occt_arg(keyword.source_arg(), "bspline :closed")?,
            "tangents" => {
                let value = point2_list_occt_arg(keyword.source_arg(), 2, "bspline :tangents")?;
                tangents = Some(value);
            }
            "tangent_scalars" | "tangent-scalars" => {
                tangent_scalars = Some(number_list_occt_arg(
                    keyword.source_arg(),
                    "bspline :tangent-scalars",
                )?);
            }
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor `bspline` does not recognize `:{other}`."
                )))
            }
        }
    }

    let points = point2_list_arg_min(args, 0, 2, "bspline")?;
    if let Some(tangents) = &tangents {
        if tangents.len() != 2 && tangents.len() != points.len() {
            return Err(AppError::validation(format!(
                "Direct OCCT executor `bspline` `:tangents` expects 2 entries or one per point ({}).",
                points.len()
            )));
        }
    }
    if let Some(scalars) = &tangent_scalars {
        if scalars.len() != 2 && scalars.len() != points.len() {
            return Err(AppError::validation(format!(
                "Direct OCCT executor `bspline` `:tangent-scalars` expects 2 entries or one per point ({}).",
                points.len()
            )));
        }
    }
    if points.len() < 3 && tangents.is_none() {
        return Err(AppError::validation(
            "Direct OCCT executor `bspline` requires at least three points unless tangents are supplied.",
        ));
    }

    Ok(BsplineArgs {
        points,
        closed,
        tangents,
        tangent_scalars,
    })
}

fn bool_occt_arg(arg: &OcctArg, label: &str) -> AppResult<bool> {
    match arg {
        OcctArg::Boolean(value) => Ok(*value),
        other => Err(AppError::validation(format!(
            "Direct OCCT executor expected boolean for {label}, got {:?}.",
            other
        ))),
    }
}

fn number_list_occt_arg(arg: &OcctArg, label: &str) -> AppResult<Vec<f64>> {
    if let OcctArg::Point2(point) = arg {
        return Ok(point.to_vec());
    }
    if let OcctArg::Point3(point) = arg {
        return Ok(point.to_vec());
    }
    let OcctArg::List(items) = arg else {
        return Err(AppError::validation(format!(
            "Direct OCCT executor expected number list for {label}, got {:?}.",
            arg
        )));
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            OcctArg::Number(value) => Ok(*value),
            other => Err(AppError::validation(format!(
                "Direct OCCT executor expected number at {label}[{index}], got {:?}.",
                other
            ))),
        })
        .collect()
}

fn point2_list_occt_arg(arg: &OcctArg, min_points: usize, label: &str) -> AppResult<Vec<[f64; 2]>> {
    let OcctArg::List(items) = arg else {
        return Err(AppError::validation(format!(
            "Direct OCCT executor expected 2D point list for {label}, got {:?}.",
            arg
        )));
    };
    let points = items
        .iter()
        .enumerate()
        .map(|(point_index, item)| match item {
            OcctArg::Point2(point) => Ok(*point),
            OcctArg::List(values) if values.len() == 2 => {
                let [x, y] = numeric_args::<2>(values)?;
                Ok([x, y])
            }
            other => Err(AppError::validation(format!(
                "Direct OCCT executor expected 2D point at {label}[{point_index}], got {:?}.",
                other
            ))),
        })
        .collect::<AppResult<Vec<_>>>()?;
    if points.len() < min_points {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `{label}` requires at least {min_points} points."
        )));
    }
    Ok(points)
}

fn point3_sequence_args(args: &[OcctArg]) -> AppResult<Vec<[f64; 3]>> {
    let items = if args.len() == 1 {
        match &args[0] {
            OcctArg::List(items) => items.as_slice(),
            _ => args,
        }
    } else {
        args
    };
    let points = items
        .iter()
        .enumerate()
        .map(|(point_index, item)| match item {
            OcctArg::Point3(point) => Ok(*point),
            OcctArg::List(values) if values.len() == 3 => {
                let [x, y, z] = numeric_args::<3>(values)?;
                Ok([x, y, z])
            }
            other => Err(AppError::validation(format!(
                "Direct OCCT executor expected 3D point at path index {point_index}, got {:?}.",
                other
            ))),
        })
        .collect::<AppResult<Vec<_>>>()?;
    if points.len() < 2 {
        return Err(AppError::validation(
            "Direct OCCT executor `path` requires at least two points.",
        ));
    }
    Ok(points)
}

fn emit_created_by_subshape_filter(
    var: &str,
    label: &str,
    shape_kind: &str,
    created_by_var: &str,
    candidate_var: &str,
    created_by_slot_index: Option<u64>,
) -> String {
    if created_by_slot_index.is_none() {
        return String::new();
    }
    format!(
        "        bool {var}_created_by_{label}_matches = false;\n        TopTools_IndexedMapOfShape {var}_created_by_{label}_map;\n        TopExp::MapShapes({created_by_var}, {shape_kind}, {var}_created_by_{label}_map);\n        for (int {var}_created_by_{label}_ordinal = 1; {var}_created_by_{label}_ordinal <= {var}_created_by_{label}_map.Extent(); ++{var}_created_by_{label}_ordinal) {{\n            if ({var}_created_by_{label}_map.FindKey({var}_created_by_{label}_ordinal).IsSame({candidate_var})) {{\n                {var}_created_by_{label}_matches = true;\n                break;\n            }}\n        }}\n        if (!{var}_created_by_{label}_matches) {{\n            continue;\n        }}\n",
    )
}

fn emit_edge_radius_operation(
    body: &mut String,
    var: &str,
    label: &str,
    builder_type: &str,
    input_var: String,
    radius: f64,
    to_radius: Option<f64>,
    part_key: &str,
    selector: Option<&EdgeSelector>,
) {
    // Tapered (variable-radius) fillet uses OCCT's two-radius `Add(r1, r2, edge)`;
    // uniform stays `Add(r, edge)`.
    let radius_args = match to_radius {
        Some(r2) => format!("{radius}, {r2}"),
        None => format!("{radius}"),
    };
    if let Some(EdgeSelector {
        kind: EdgeSelectorKind::TargetIds(target_ids),
        created_by_slot_index,
    }) = selector
    {
        let created_by_filter = emit_created_by_subshape_filter(
            var,
            "edge",
            "TopAbs_EDGE",
            &slot_var(OcctSlot(
                created_by_slot_index.as_ref().copied().unwrap_or_default(),
            )),
            &format!("{var}_edge"),
            *created_by_slot_index,
        );
        let edge_no_match_message = cpp_string_literal(&format!(
            "Direct OCCT edge selector target ids did not match current topology for part `{part_key}`."
        ));
        let edge_ambiguous_message = cpp_string_literal(&format!(
            "Direct OCCT edge selector stable target id ambiguously matched current topology for part `{part_key}`."
        ));
        let target_id_vector = if target_ids.is_empty() {
            "{}".to_string()
        } else {
            format!(
                "{{{}}}",
                target_ids
                    .iter()
                    .map(|target_id| cpp_string_literal(target_id))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        body.push_str(&format!(
            "    {builder_type} {var}_{label}({input_var});\n    std::vector<std::string> {var}_target_ids = {target_id_vector};\n    std::vector<std::string> {var}_edge_target_ids;\n    std::vector<std::string> {var}_edge_stable_ids;\n    std::map<std::string, int> {var}_stable_counts;\n    TopTools_IndexedMapOfShape {var}_edge_map;\n    TopExp::MapShapes({input_var}, TopAbs_EDGE, {var}_edge_map);\n    for (int {var}_edge_ordinal = 1; {var}_edge_ordinal <= {var}_edge_map.Extent(); ++{var}_edge_ordinal) {{\n        int {var}_edge_index = {var}_edge_ordinal - 1;\n        TopoDS_Edge {var}_edge = TopoDS::Edge({var}_edge_map.FindKey({var}_edge_ordinal));\n{created_by_filter}        std::string {var}_target_id = direct_occt_edge_target_id({part_id}, {var}_edge_index, {var}_edge);\n        std::string {var}_stable_id = direct_occt_stable_edge_target_id({var}_target_id);\n        {var}_edge_target_ids.push_back({var}_target_id);\n        {var}_edge_stable_ids.push_back({var}_stable_id);\n        {var}_stable_counts[{var}_stable_id] += 1;\n    }}\n    std::vector<std::string> {var}_matched_target_ids;\n    std::vector<int> {var}_matched_edge_indexes;\n    for (const std::string& {var}_requested_target_id : {var}_target_ids) {{\n        bool {var}_matched = false;\n        for (std::size_t {var}_candidate_index = 0; {var}_candidate_index < {var}_edge_target_ids.size(); ++{var}_candidate_index) {{\n            if ({var}_edge_target_ids[{var}_candidate_index] != {var}_requested_target_id) {{\n                continue;\n            }}\n            if (std::find({var}_matched_edge_indexes.begin(), {var}_matched_edge_indexes.end(), static_cast<int>({var}_candidate_index)) == {var}_matched_edge_indexes.end()) {{\n                {var}_matched_edge_indexes.push_back(static_cast<int>({var}_candidate_index));\n            }}\n            {var}_matched_target_ids.push_back({var}_requested_target_id);\n            {var}_matched = true;\n            break;\n        }}\n        if ({var}_matched) {{\n            continue;\n        }}\n        std::string {var}_requested_stable_id = direct_occt_stable_edge_target_id({var}_requested_target_id);\n        if ({var}_stable_counts[{var}_requested_stable_id] > 1) {{ std::cerr << {edge_ambiguous_message} << \" requested=\" << {var}_requested_target_id << std::endl; return 7; }}\n        for (std::size_t {var}_candidate_index = 0; {var}_candidate_index < {var}_edge_stable_ids.size(); ++{var}_candidate_index) {{\n            if ({var}_edge_stable_ids[{var}_candidate_index] != {var}_requested_stable_id) {{\n                continue;\n            }}\n            if (std::find({var}_matched_edge_indexes.begin(), {var}_matched_edge_indexes.end(), static_cast<int>({var}_candidate_index)) == {var}_matched_edge_indexes.end()) {{\n                {var}_matched_edge_indexes.push_back(static_cast<int>({var}_candidate_index));\n            }}\n            {var}_matched_target_ids.push_back({var}_requested_target_id);\n            {var}_matched = true;\n            break;\n        }}\n        if (!{var}_matched) {{ std::cerr << {edge_no_match_message} << \" requested=\" << {var}_requested_target_id << std::endl; return 4; }}\n    }}\n    if ({var}_matched_target_ids.size() != {var}_target_ids.size()) {{ std::cerr << {edge_ambiguous_message} << std::endl; return 7; }}\n    if ({var}_matched_edge_indexes.empty()) {{ std::cerr << {edge_no_match_message} << std::endl; return 4; }}\n    for (int {var}_edge_ordinal = 1; {var}_edge_ordinal <= {var}_edge_map.Extent(); ++{var}_edge_ordinal) {{\n        int {var}_edge_index = {var}_edge_ordinal - 1;\n        if (std::find({var}_matched_edge_indexes.begin(), {var}_matched_edge_indexes.end(), {var}_edge_index) == {var}_matched_edge_indexes.end()) {{\n            continue;\n        }}\n        {var}_{label}.Add({radius_args}, TopoDS::Edge({var}_edge_map.FindKey({var}_edge_ordinal)));\n    }}\n    TopoDS_Shape {var} = {var}_{label}.Shape();\n",
            part_id = cpp_string_literal(part_key)
        ));
    } else if let Some(EdgeSelector {
        kind: EdgeSelectorKind::Clauses(clauses),
        created_by_slot_index,
    }) = selector
    {
        let created_by_filter = emit_created_by_subshape_filter(
            var,
            "edge",
            "TopAbs_EDGE",
            &slot_var(OcctSlot(
                created_by_slot_index.as_ref().copied().unwrap_or_default(),
            )),
            &format!("{var}_edge"),
            *created_by_slot_index,
        );
        let clause_checks = clauses
            .iter()
            .map(|clause| match clause {
                CoreEdgeSelectorClause::Axis(axis) => {
                    let (span, other_a, other_b) = match axis {
                        CoreEdgeAxis::X => (
                            format!("{var}_edge_xmax - {var}_edge_xmin"),
                            format!("{var}_edge_ymax - {var}_edge_ymin"),
                            format!("{var}_edge_zmax - {var}_edge_zmin"),
                        ),
                        CoreEdgeAxis::Y => (
                            format!("{var}_edge_ymax - {var}_edge_ymin"),
                            format!("{var}_edge_xmax - {var}_edge_xmin"),
                            format!("{var}_edge_zmax - {var}_edge_zmin"),
                        ),
                        CoreEdgeAxis::Z => (
                            format!("{var}_edge_zmax - {var}_edge_zmin"),
                            format!("{var}_edge_xmax - {var}_edge_xmin"),
                            format!("{var}_edge_ymax - {var}_edge_ymin"),
                        ),
                    };
                    format!(
                        "        {var}_edge_matches = {var}_edge_matches && ({span}) > {var}_edge_tol && ({other_a}) <= {var}_edge_tol && ({other_b}) <= {var}_edge_tol;\n"
                    )
                }
                CoreEdgeSelectorClause::Boundary { axis, bound } => {
                    let (shape_bound, edge_min, edge_max) = match (axis, bound) {
                        (CoreEdgeAxis::X, CoreEdgeBound::Min) => (
                            format!("{var}_shape_xmin"),
                            format!("{var}_edge_xmin"),
                            format!("{var}_edge_xmax"),
                        ),
                        (CoreEdgeAxis::X, CoreEdgeBound::Max) => (
                            format!("{var}_shape_xmax"),
                            format!("{var}_edge_xmin"),
                            format!("{var}_edge_xmax"),
                        ),
                        (CoreEdgeAxis::Y, CoreEdgeBound::Min) => (
                            format!("{var}_shape_ymin"),
                            format!("{var}_edge_ymin"),
                            format!("{var}_edge_ymax"),
                        ),
                        (CoreEdgeAxis::Y, CoreEdgeBound::Max) => (
                            format!("{var}_shape_ymax"),
                            format!("{var}_edge_ymin"),
                            format!("{var}_edge_ymax"),
                        ),
                        (CoreEdgeAxis::Z, CoreEdgeBound::Min) => (
                            format!("{var}_shape_zmin"),
                            format!("{var}_edge_zmin"),
                            format!("{var}_edge_zmax"),
                        ),
                        (CoreEdgeAxis::Z, CoreEdgeBound::Max) => (
                            format!("{var}_shape_zmax"),
                            format!("{var}_edge_zmin"),
                            format!("{var}_edge_zmax"),
                        ),
                    };
                    format!(
                        "        {var}_edge_matches = {var}_edge_matches && std::abs({edge_min} - {shape_bound}) <= {var}_edge_tol && std::abs({edge_max} - {shape_bound}) <= {var}_edge_tol;\n"
                    )
                }
            })
            .collect::<String>();
        body.push_str(&format!(
            "    {builder_type} {var}_{label}({input_var});\n    Bnd_Box {var}_shape_box;\n    BRepBndLib::Add({input_var}, {var}_shape_box);\n    Standard_Real {var}_shape_xmin, {var}_shape_ymin, {var}_shape_zmin, {var}_shape_xmax, {var}_shape_ymax, {var}_shape_zmax;\n    {var}_shape_box.Get({var}_shape_xmin, {var}_shape_ymin, {var}_shape_zmin, {var}_shape_xmax, {var}_shape_ymax, {var}_shape_zmax);\n    Standard_Real {var}_edge_tol = std::max({var}_shape_xmax - {var}_shape_xmin, std::max({var}_shape_ymax - {var}_shape_ymin, std::max({var}_shape_zmax - {var}_shape_zmin, 1.0))) * 1.0e-6;\n    TopTools_IndexedMapOfShape {var}_edge_map;\n    TopExp::MapShapes({input_var}, TopAbs_EDGE, {var}_edge_map);\n    std::vector<int> {var}_matched_edge_indexes;\n    for (int {var}_edge_ordinal = 1; {var}_edge_ordinal <= {var}_edge_map.Extent(); ++{var}_edge_ordinal) {{\n        int {var}_edge_index = {var}_edge_ordinal - 1;\n        TopoDS_Edge {var}_edge = TopoDS::Edge({var}_edge_map.FindKey({var}_edge_ordinal));\n{created_by_filter}        Bnd_Box {var}_edge_box;\n        BRepBndLib::Add({var}_edge, {var}_edge_box);\n        Standard_Real {var}_edge_xmin, {var}_edge_ymin, {var}_edge_zmin, {var}_edge_xmax, {var}_edge_ymax, {var}_edge_zmax;\n        {var}_edge_box.Get({var}_edge_xmin, {var}_edge_ymin, {var}_edge_zmin, {var}_edge_xmax, {var}_edge_ymax, {var}_edge_zmax);\n        bool {var}_edge_matches = true;\n{clause_checks}        if ({var}_edge_matches) {{\n            {var}_matched_edge_indexes.push_back({var}_edge_index);\n        }}\n    }}\n    if ({var}_matched_edge_indexes.empty()) {{ return 4; }}\n    for (int {var}_edge_ordinal = 1; {var}_edge_ordinal <= {var}_edge_map.Extent(); ++{var}_edge_ordinal) {{\n        int {var}_edge_index = {var}_edge_ordinal - 1;\n        if (std::find({var}_matched_edge_indexes.begin(), {var}_matched_edge_indexes.end(), {var}_edge_index) == {var}_matched_edge_indexes.end()) {{\n            continue;\n        }}\n        {var}_{label}.Add({radius_args}, TopoDS::Edge({var}_edge_map.FindKey({var}_edge_ordinal)));\n    }}\n    TopoDS_Shape {var} = {var}_{label}.Shape();\n"
        ));
    } else {
        body.push_str(&format!(
            "    {builder_type} {var}_{label}({input_var});\n    TopTools_IndexedMapOfShape {var}_edge_map;\n    TopExp::MapShapes({input_var}, TopAbs_EDGE, {var}_edge_map);\n    int {var}_edge_count = 0;\n    for (int {var}_edge_ordinal = 1; {var}_edge_ordinal <= {var}_edge_map.Extent(); ++{var}_edge_ordinal) {{\n        {var}_{label}.Add({radius_args}, TopoDS::Edge({var}_edge_map.FindKey({var}_edge_ordinal)));\n        ++{var}_edge_count;\n    }}\n    if ({var}_edge_count == 0) {{ return 4; }}\n    TopoDS_Shape {var} = {var}_{label}.Shape();\n"
        ));
    }
}

fn emit_shell_operation(
    body: &mut String,
    var: &str,
    input_var: String,
    thickness: f64,
    part_key: &str,
    selector: Option<&ShellFaceSelector>,
) {
    let offset = -thickness.abs();
    if let Some(ShellFaceSelector {
        kind: ShellFaceSelectorKind::TargetIds(target_ids),
        created_by_slot_index,
    }) = selector
    {
        let created_by_filter = emit_created_by_subshape_filter(
            var,
            "face",
            "TopAbs_FACE",
            &slot_var(OcctSlot(
                created_by_slot_index.as_ref().copied().unwrap_or_default(),
            )),
            &format!("{var}_face"),
            *created_by_slot_index,
        );
        let face_no_match_message = cpp_string_literal(&format!(
            "Direct OCCT shell face selector target ids did not match current topology for part `{part_key}`."
        ));
        let face_ambiguous_message = cpp_string_literal(&format!(
            "Direct OCCT shell face selector stable target id ambiguously matched current topology for part `{part_key}`."
        ));
        let target_id_vector = if target_ids.is_empty() {
            "std::vector<std::string>{}".to_string()
        } else {
            format!(
                "std::vector<std::string>{{{}}}",
                target_ids
                    .iter()
                    .map(|target_id| format!("{:?}", target_id))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        body.push_str(&format!(
            "    TopTools_ListOfShape {var}_closing_faces;\n    std::vector<std::string> {var}_target_ids = {target_id_vector};\n    std::vector<TopoDS_Face> {var}_faces;\n    std::vector<std::string> {var}_face_target_ids;\n    std::vector<std::string> {var}_face_stable_ids;\n    std::map<std::string, int> {var}_stable_counts;\n    int {var}_face_index = 0;\n    for (TopExp_Explorer {var}_face_explorer({input_var}, TopAbs_FACE); {var}_face_explorer.More(); {var}_face_explorer.Next(), ++{var}_face_index) {{\n        TopoDS_Face {var}_face = TopoDS::Face({var}_face_explorer.Current());\n{created_by_filter}        std::string {var}_target_id = direct_occt_face_target_id({:?}, {var}_face_index, {var}_face);\n        std::string {var}_stable_id = direct_occt_stable_face_target_id({var}_target_id);\n        {var}_faces.push_back({var}_face);\n        {var}_face_target_ids.push_back({var}_target_id);\n        {var}_face_stable_ids.push_back({var}_stable_id);\n        {var}_stable_counts[{var}_stable_id] += 1;\n    }}\n    std::vector<std::string> {var}_matched_target_ids;\n    std::vector<int> {var}_matched_face_indexes;\n    for (const std::string& {var}_requested_target_id : {var}_target_ids) {{\n        bool {var}_matched = false;\n        for (std::size_t {var}_candidate_index = 0; {var}_candidate_index < {var}_face_target_ids.size(); ++{var}_candidate_index) {{\n            if ({var}_face_target_ids[{var}_candidate_index] != {var}_requested_target_id) {{\n                continue;\n            }}\n            if (std::find({var}_matched_face_indexes.begin(), {var}_matched_face_indexes.end(), static_cast<int>({var}_candidate_index)) == {var}_matched_face_indexes.end()) {{\n                {var}_closing_faces.Append({var}_faces[{var}_candidate_index]);\n                {var}_matched_face_indexes.push_back(static_cast<int>({var}_candidate_index));\n            }}\n            {var}_matched_target_ids.push_back({var}_requested_target_id);\n            {var}_matched = true;\n            break;\n        }}\n        if ({var}_matched) {{\n            continue;\n        }}\n        std::string {var}_requested_stable_id = direct_occt_stable_face_target_id({var}_requested_target_id);\n        if ({var}_stable_counts[{var}_requested_stable_id] > 1) {{ std::cerr << {face_ambiguous_message} << \" requested=\" << {var}_requested_target_id << std::endl; return 11; }}\n        for (std::size_t {var}_candidate_index = 0; {var}_candidate_index < {var}_face_stable_ids.size(); ++{var}_candidate_index) {{\n            if ({var}_face_stable_ids[{var}_candidate_index] != {var}_requested_stable_id) {{\n                continue;\n            }}\n            if (std::find({var}_matched_face_indexes.begin(), {var}_matched_face_indexes.end(), static_cast<int>({var}_candidate_index)) == {var}_matched_face_indexes.end()) {{\n                {var}_closing_faces.Append({var}_faces[{var}_candidate_index]);\n                {var}_matched_face_indexes.push_back(static_cast<int>({var}_candidate_index));\n            }}\n            {var}_matched_target_ids.push_back({var}_requested_target_id);\n            {var}_matched = true;\n            break;\n        }}\n        if (!{var}_matched) {{ std::cerr << {face_no_match_message} << \" requested=\" << {var}_requested_target_id << std::endl; return 10; }}\n    }}\n    if ({var}_matched_target_ids.size() != {var}_target_ids.size()) {{ std::cerr << {face_ambiguous_message} << std::endl; return 11; }}\n    if ({var}_matched_face_indexes.empty()) {{ std::cerr << {face_no_match_message} << std::endl; return 10; }}\n    BRepOffsetAPI_MakeThickSolid {var}_thick;\n    {var}_thick.MakeThickSolidByJoin({input_var}, {var}_closing_faces, {offset}, 0.05, BRepOffset_Skin, false, false, GeomAbs_Intersection, true);\n    TopoDS_Shape {var} = {var}_thick.Shape();\n",
            part_key
        ));
        return;
    }
    if let Some(ShellFaceSelector {
        kind: ShellFaceSelectorKind::Clauses(clauses),
        created_by_slot_index,
    }) = selector
    {
        let created_by_filter = emit_created_by_subshape_filter(
            var,
            "face",
            "TopAbs_FACE",
            &slot_var(OcctSlot(
                created_by_slot_index.as_ref().copied().unwrap_or_default(),
            )),
            &format!("{var}_face"),
            *created_by_slot_index,
        );
        let clause_checks = clauses
            .iter()
            .map(|clause| {
                match clause {
                    CoreFaceSelectorClause::Boundary { axis, bound } => {
                        let axis_name = match axis {
                            CoreEdgeAxis::X => "x",
                            CoreEdgeAxis::Y => "y",
                            CoreEdgeAxis::Z => "z",
                        };
                        let bound_name = match bound {
                            CoreEdgeBound::Min => "min",
                            CoreEdgeBound::Max => "max",
                        };
                        format!(
                            "        {var}_matches = {var}_matches && std::abs({var}_face_{axis_name}min - {var}_{axis_name}{bound_name}) <= {var}_tol && std::abs({var}_face_{axis_name}max - {var}_{axis_name}{bound_name}) <= {var}_tol;\n"
                        )
                    }
                    CoreFaceSelectorClause::Planar => format!(
                        "        {var}_matches = {var}_matches && {var}_is_planar;\n"
                    ),
                    CoreFaceSelectorClause::Normal(axis) => {
                        let axis_name = match axis {
                            CoreEdgeAxis::X => "x",
                            CoreEdgeAxis::Y => "y",
                            CoreEdgeAxis::Z => "z",
                        };
                        format!(
                            "        {var}_matches = {var}_matches && {var}_is_planar && ({var}_face_{axis_name}max - {var}_face_{axis_name}min) <= {var}_tol;\n"
                        )
                    }
                    CoreFaceSelectorClause::Area(rank) => {
                        let _ = rank;
                        String::new()
                    }
                }
            })
            .collect::<String>();
        let area_filters = clauses
            .iter()
            .filter_map(|clause| match clause {
                CoreFaceSelectorClause::Area(rank) => Some(match rank {
                    CoreFaceAreaRank::Min => {
                        format!(
                            "    Standard_Real {var}_target_area = 1.0e100;\n    for (int {var}_index : {var}_candidate_indexes) {{\n        {var}_target_area = std::min({var}_target_area, {var}_face_areas[static_cast<std::size_t>({var}_index)]);\n    }}\n    std::vector<int> {var}_area_filtered;\n    for (int {var}_index : {var}_candidate_indexes) {{\n        if (std::abs({var}_face_areas[static_cast<std::size_t>({var}_index)] - {var}_target_area) <= {var}_area_tol) {{\n            {var}_area_filtered.push_back({var}_index);\n        }}\n    }}\n    {var}_candidate_indexes = {var}_area_filtered;\n"
                        )
                    }
                    CoreFaceAreaRank::Max => {
                        format!(
                            "    Standard_Real {var}_target_area = -1.0e100;\n    for (int {var}_index : {var}_candidate_indexes) {{\n        {var}_target_area = std::max({var}_target_area, {var}_face_areas[static_cast<std::size_t>({var}_index)]);\n    }}\n    std::vector<int> {var}_area_filtered;\n    for (int {var}_index : {var}_candidate_indexes) {{\n        if (std::abs({var}_face_areas[static_cast<std::size_t>({var}_index)] - {var}_target_area) <= {var}_area_tol) {{\n            {var}_area_filtered.push_back({var}_index);\n        }}\n    }}\n    {var}_candidate_indexes = {var}_area_filtered;\n"
                        )
                    }
                }),
                _ => None,
            })
            .collect::<String>();
        body.push_str(&format!(
            "    TopTools_ListOfShape {var}_closing_faces;\n    Bnd_Box {var}_shape_box;\n    BRepBndLib::Add({input_var}, {var}_shape_box);\n    Standard_Real {var}_xmin, {var}_ymin, {var}_zmin, {var}_xmax, {var}_ymax, {var}_zmax;\n    {var}_shape_box.Get({var}_xmin, {var}_ymin, {var}_zmin, {var}_xmax, {var}_ymax, {var}_zmax);\n    Standard_Real {var}_tol = std::max({var}_xmax - {var}_xmin, std::max({var}_ymax - {var}_ymin, std::max({var}_zmax - {var}_zmin, 1.0))) * 1.0e-6;\n    Standard_Real {var}_area_tol = 1.0e-6;\n    std::vector<TopoDS_Face> {var}_faces;\n    std::vector<Standard_Real> {var}_face_areas;\n    std::vector<int> {var}_candidate_indexes;\n    for (TopExp_Explorer {var}_face_explorer({input_var}, TopAbs_FACE); {var}_face_explorer.More(); {var}_face_explorer.Next()) {{\n        TopoDS_Face {var}_face = TopoDS::Face({var}_face_explorer.Current());\n{created_by_filter}        BRepAdaptor_Surface {var}_surface({var}_face);\n        bool {var}_is_planar = {var}_surface.GetType() == GeomAbs_Plane;\n        Bnd_Box {var}_face_box;\n        BRepBndLib::Add({var}_face, {var}_face_box);\n        Standard_Real {var}_face_xmin, {var}_face_ymin, {var}_face_zmin, {var}_face_xmax, {var}_face_ymax, {var}_face_zmax;\n        {var}_face_box.Get({var}_face_xmin, {var}_face_ymin, {var}_face_zmin, {var}_face_xmax, {var}_face_ymax, {var}_face_zmax);\n        GProp_GProps {var}_props;\n        BRepGProp::SurfaceProperties({var}_face, {var}_props);\n        Standard_Real {var}_area = {var}_props.Mass();\n        bool {var}_matches = true;\n{clause_checks}        if ({var}_matches) {{\n            {var}_faces.push_back({var}_face);\n            {var}_face_areas.push_back({var}_area);\n            {var}_candidate_indexes.push_back(static_cast<int>({var}_faces.size()) - 1);\n        }}\n    }}\n    if ({var}_candidate_indexes.empty()) {{ return 10; }}\n{area_filters}    if ({var}_candidate_indexes.empty()) {{ return 10; }}\n    for (int {var}_index : {var}_candidate_indexes) {{\n        {var}_closing_faces.Append({var}_faces[static_cast<std::size_t>({var}_index)]);\n    }}\n    BRepOffsetAPI_MakeThickSolid {var}_thick;\n    {var}_thick.MakeThickSolidByJoin({input_var}, {var}_closing_faces, {offset}, 0.05, BRepOffset_Skin, false, false, GeomAbs_Intersection, true);\n    TopoDS_Shape {var} = {var}_thick.Shape();\n"
        ));
        return;
    }
    body.push_str(&format!(
        "    TopTools_ListOfShape {var}_closing_faces;\n    Standard_Real {var}_top_z = -1.0e100;\n    for (TopExp_Explorer {var}_face_explorer({input_var}, TopAbs_FACE); {var}_face_explorer.More(); {var}_face_explorer.Next()) {{\n        TopoDS_Face {var}_face = TopoDS::Face({var}_face_explorer.Current());\n        BRepAdaptor_Surface {var}_surface({var}_face);\n        if ({var}_surface.GetType() != GeomAbs_Plane) {{ continue; }}\n        Bnd_Box {var}_face_box;\n        BRepBndLib::Add({var}_face, {var}_face_box);\n        Standard_Real {var}_xmin, {var}_ymin, {var}_zmin, {var}_xmax, {var}_ymax, {var}_zmax;\n        {var}_face_box.Get({var}_xmin, {var}_ymin, {var}_zmin, {var}_xmax, {var}_ymax, {var}_zmax);\n        if ({var}_zmax > {var}_top_z + 1.0e-7) {{\n            {var}_closing_faces.Clear();\n            {var}_top_z = {var}_zmax;\n        }}\n        if (std::abs({var}_zmax - {var}_top_z) <= 1.0e-7) {{\n            {var}_closing_faces.Append({var}_face);\n        }}\n    }}\n    TopoDS_Shape {var};\n    if ({var}_closing_faces.IsEmpty()) {{\n        BRepOffsetAPI_MakeOffsetShape {var}_offset;\n        {var}_offset.PerformByJoin({input_var}, {offset}, 0.05, BRepOffset_Skin, false, false, GeomAbs_Intersection, true);\n        TopoDS_Shape {var}_inner = {var}_offset.Shape();\n        {var} = BRepAlgoAPI_Cut({input_var}, {var}_inner).Shape();\n    }} else {{\n        BRepOffsetAPI_MakeThickSolid {var}_thick;\n        {var}_thick.MakeThickSolidByJoin({input_var}, {var}_closing_faces, {offset}, 0.05, BRepOffset_Skin, false, false, GeomAbs_Intersection, true);\n        {var} = {var}_thick.Shape();\n    }}\n"
    ));
}

fn emit_loft_operation(
    body: &mut String,
    var: &str,
    distance: f64,
    profiles: Vec<OcctSlot>,
) -> AppResult<()> {
    if profiles.len() < 2 {
        return Err(AppError::validation(
            "Direct OCCT executor `loft` requires at least two profiles.",
        ));
    }
    let denominator = (profiles.len() - 1) as f64;
    body.push_str(&format!(
        "    BRepOffsetAPI_ThruSections {var}_loft(true, false, 1.0e-6);\n"
    ));
    for (index, profile) in profiles.iter().enumerate() {
        let z = distance * index as f64 / denominator;
        let input_var = slot_var(*profile);
        body.push_str(&format!(
            "    gp_Trsf {var}_section_{index}_trsf;\n    {var}_section_{index}_trsf.SetTranslation(gp_Vec(0, 0, {z}));\n    TopoDS_Shape {var}_section_{index}_shape = BRepBuilderAPI_Transform({input_var}, {var}_section_{index}_trsf, true).Shape();\n    TopoDS_Wire {var}_section_{index}_wire;\n    for (TopExp_Explorer {var}_section_{index}_wire_explorer({var}_section_{index}_shape, TopAbs_WIRE); {var}_section_{index}_wire_explorer.More(); {var}_section_{index}_wire_explorer.Next()) {{\n        {var}_section_{index}_wire = TopoDS::Wire({var}_section_{index}_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_section_{index}_wire.IsNull()) {{ return 5; }}\n    {var}_loft.AddWire({var}_section_{index}_wire);\n"
        ));
    }
    body.push_str(&format!(
        "    {var}_loft.Build();\n    if (!{var}_loft.IsDone()) {{ return 6; }}\n    TopoDS_Shape {var} = {var}_loft.Shape();\n"
    ));
    Ok(())
}

fn emit_path_wire(body: &mut String, var: &str, points: &[[f64; 3]]) -> AppResult<()> {
    if points.len() < 2 {
        return Err(AppError::validation(
            "Direct OCCT executor path wire requires at least two points.",
        ));
    }
    body.push_str(&format!("    BRepBuilderAPI_MakePolygon {var}_path;\n"));
    for [x, y, z] in points {
        body.push_str(&format!("    {var}_path.Add(gp_Pnt({x}, {y}, {z}));\n"));
    }
    body.push_str(&format!(
        "    TopoDS_Wire {var}_wire = {var}_path.Wire();\n    TopoDS_Shape {var} = {var}_wire;\n"
    ));
    Ok(())
}

fn emit_helix_path_wire(
    body: &mut String,
    var: &str,
    radius: f64,
    pitch: f64,
    height: f64,
    lefthand: bool,
) -> AppResult<()> {
    if !(radius.is_finite() && radius > 0.0) {
        return Err(AppError::validation(
            "Direct OCCT executor `helix-path` radius must be positive and finite.",
        ));
    }
    if !(pitch.is_finite() && pitch > 0.0) {
        return Err(AppError::validation(
            "Direct OCCT executor `helix-path` pitch must be positive and finite.",
        ));
    }
    if !(height.is_finite() && height > 0.0) {
        return Err(AppError::validation(
            "Direct OCCT executor `helix-path` height must be positive and finite.",
        ));
    }
    let turns = height / pitch;
    let end_angle = if lefthand {
        -2.0 * std::f64::consts::PI * turns
    } else {
        2.0 * std::f64::consts::PI * turns
    };
    body.push_str(&format!(
        "    Handle(Geom_CylindricalSurface) {var}_surface = new Geom_CylindricalSurface(gp_Ax3(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {radius});\n    Handle(Geom2d_TrimmedCurve) {var}_curve2d = GCE2d_MakeSegment(gp_Pnt2d(0, 0), gp_Pnt2d({end_angle}, {height})).Value();\n    TopoDS_Edge {var}_edge = BRepBuilderAPI_MakeEdge({var}_curve2d, {var}_surface).Edge();\n    TopoDS_Wire {var}_wire = BRepBuilderAPI_MakeWire({var}_edge).Wire();\n    TopoDS_Shape {var} = {var}_wire;\n"
    ));
    Ok(())
}

fn emit_bezier_path_wire(body: &mut String, var: &str, points: &[[f64; 3]]) -> AppResult<()> {
    if points.len() < 4 || (points.len() - 1) % 3 != 0 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `bezier-path` expects 3n+1 control points (4, 7, 10, ...), got {}.",
            points.len()
        )));
    }
    body.push_str(&format!(
        "    BRepBuilderAPI_MakeWire {var}_wire_builder;\n"
    ));
    for (segment_index, start) in (0..points.len() - 1).step_by(3).enumerate() {
        body.push_str(&format!(
            "    TColgp_Array1OfPnt {var}_segment_{segment_index}_poles(1, 4);\n"
        ));
        for local_index in 0..4 {
            let [x, y, z] = points[start + local_index];
            let pole_index = local_index + 1;
            body.push_str(&format!(
                "    {var}_segment_{segment_index}_poles.SetValue({pole_index}, gp_Pnt({x}, {y}, {z}));\n"
            ));
        }
        body.push_str(&format!(
            "    Handle(Geom_BezierCurve) {var}_segment_{segment_index}_curve = new Geom_BezierCurve({var}_segment_{segment_index}_poles);\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge({var}_segment_{segment_index}_curve).Edge());\n"
        ));
    }
    body.push_str(&format!(
        "    TopoDS_Wire {var}_wire = {var}_wire_builder.Wire();\n    TopoDS_Shape {var} = {var}_wire;\n"
    ));
    Ok(())
}

fn emit_bspline_shape(body: &mut String, var: &str, args: &BsplineArgs) -> AppResult<()> {
    let first = args.points[0];
    let last = *args.points.last().expect("checked non-empty");
    body.push_str(&format!(
        "    BRepBuilderAPI_MakeWire {var}_wire_builder;\n"
    ));
    if let Some(tangents) = &args.tangents {
        let first_tangent = tangents[0];
        let last_tangent = *tangents.last().expect("checked non-empty");
        let first_scale = args
            .tangent_scalars
            .as_ref()
            .and_then(|items| items.first())
            .copied()
            .unwrap_or(1.0);
        let last_scale = args
            .tangent_scalars
            .as_ref()
            .and_then(|items| items.last())
            .copied()
            .unwrap_or(1.0);
        let p1 = [
            first[0] + first_tangent[0] * first_scale,
            first[1] + first_tangent[1] * first_scale,
        ];
        let p2 = [
            last[0] - last_tangent[0] * last_scale,
            last[1] - last_tangent[1] * last_scale,
        ];
        let poles = [first, p1, p2, last];
        body.push_str(&format!("    TColgp_Array1OfPnt {var}_poles(1, 4);\n"));
        for (index, [x, y]) in poles.iter().enumerate() {
            let pole_index = index + 1;
            body.push_str(&format!(
                "    {var}_poles.SetValue({pole_index}, gp_Pnt({x}, {y}, 0));\n"
            ));
        }
        body.push_str(&format!(
            "    Handle(Geom_BezierCurve) {var}_curve = new Geom_BezierCurve({var}_poles);\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge({var}_curve).Edge());\n"
        ));
    } else {
        body.push_str(&format!(
            "    TColgp_Array1OfPnt {var}_poles(1, {});\n",
            args.points.len()
        ));
        for (index, [x, y]) in args.points.iter().enumerate() {
            let pole_index = index + 1;
            body.push_str(&format!(
                "    {var}_poles.SetValue({pole_index}, gp_Pnt({x}, {y}, 0));\n"
            ));
        }
        body.push_str(&format!(
            "    GeomAPI_PointsToBSpline {var}_bspline_builder({var}_poles, 3, 8, GeomAbs_C2, 1.0e-4);\n    Handle(Geom_BSplineCurve) {var}_curve = {var}_bspline_builder.Curve();\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge({var}_curve).Edge());\n"
        ));
    }
    if args.closed && distance2(first, last) > 1.0e-9 {
        body.push_str(&format!(
            "    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({}, {}, 0), gp_Pnt({}, {}, 0)).Edge());\n",
            last[0], last[1], first[0], first[1]
        ));
    }
    body.push_str(&format!(
        "    TopoDS_Wire {var}_wire = {var}_wire_builder.Wire();\n"
    ));
    if args.closed {
        body.push_str(&format!(
            "    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n"
        ));
    } else {
        body.push_str(&format!("    TopoDS_Shape {var} = {var}_wire;\n"));
    }
    Ok(())
}

fn emit_plane_operation(
    body: &mut String,
    var: &str,
    origin: [f64; 3],
    x_axis: [f64; 3],
    normal: [f64; 3],
) {
    let [origin_x, origin_y, origin_z] = origin;
    let [x_hint_x, x_hint_y, x_hint_z] = x_axis;
    let [normal_x, normal_y, normal_z] = normal;
    body.push_str(&format!(
        "    gp_Pnt {var}_origin({origin_x}, {origin_y}, {origin_z});\n    gp_Vec {var}_z({normal_x}, {normal_y}, {normal_z});\n    if ({var}_z.Magnitude() <= 1.0e-9) {{ return 22; }}\n    {var}_z.Normalize();\n    gp_Vec {var}_x_hint({x_hint_x}, {x_hint_y}, {x_hint_z});\n    if ({var}_x_hint.Magnitude() <= 1.0e-9) {{ return 23; }}\n    gp_Vec {var}_x = {var}_x_hint - {var}_z.Multiplied({var}_x_hint.Dot({var}_z));\n    if ({var}_x.Magnitude() <= 1.0e-9) {{ return 24; }}\n    {var}_x.Normalize();\n    gp_Vec {var}_y = {var}_z.Crossed({var}_x);\n    if ({var}_y.Magnitude() <= 1.0e-9) {{ return 25; }}\n    {var}_y.Normalize();\n    {var}_x = {var}_y.Crossed({var}_z);\n    {var}_x.Normalize();\n    gp_Trsf {var};\n    {var}.SetValues(\n        {var}_x.X(), {var}_y.X(), {var}_z.X(), {var}_origin.X(),\n        {var}_x.Y(), {var}_y.Y(), {var}_z.Y(), {var}_origin.Y(),\n        {var}_x.Z(), {var}_y.Z(), {var}_z.Z(), {var}_origin.Z());\n"
    ));
}

fn emit_box_operation(
    body: &mut String,
    var: &str,
    width: f64,
    depth: f64,
    height: f64,
    align: [AxisAlign; 3],
) {
    let tx = axis_align_offset(width, align[0]);
    let ty = axis_align_offset(depth, align[1]);
    let tz = axis_align_offset(height, align[2]);
    body.push_str(&format!(
        "    TopoDS_Shape {var}_raw = BRepPrimAPI_MakeBox({width}, {depth}, {height}).Shape();\n"
    ));
    if tx == 0.0 && ty == 0.0 && tz == 0.0 {
        body.push_str(&format!("    TopoDS_Shape {var} = {var}_raw;\n"));
        return;
    }
    body.push_str(&format!(
        "    gp_Trsf {var}_align_trsf;\n    {var}_align_trsf.SetTranslation(gp_Vec({tx}, {ty}, {tz}));\n    TopoDS_Shape {var} = BRepBuilderAPI_Transform({var}_raw, {var}_align_trsf, true).Shape();\n"
    ));
}

fn emit_aligned_shape(body: &mut String, var: &str, raw_expr: &str, tx: f64, ty: f64, tz: f64) {
    body.push_str(&format!("    TopoDS_Shape {var}_raw = {raw_expr};\n"));
    if tx == 0.0 && ty == 0.0 && tz == 0.0 {
        body.push_str(&format!("    TopoDS_Shape {var} = {var}_raw;\n"));
        return;
    }
    body.push_str(&format!(
        "    gp_Trsf {var}_align_trsf;\n    {var}_align_trsf.SetTranslation(gp_Vec({tx}, {ty}, {tz}));\n    TopoDS_Shape {var} = BRepBuilderAPI_Transform({var}_raw, {var}_align_trsf, true).Shape();\n"
    ));
}

fn emit_sphere_operation(body: &mut String, var: &str, radius: f64, align: [AxisAlign; 3]) {
    let span = radius * 2.0;
    let tx = centered_axis_align_offset(span, align[0]);
    let ty = centered_axis_align_offset(span, align[1]);
    let tz = centered_axis_align_offset(span, align[2]);
    emit_aligned_shape(
        body,
        var,
        &format!("BRepPrimAPI_MakeSphere({radius}).Shape()"),
        tx,
        ty,
        tz,
    );
}

fn emit_cylinder_operation(
    body: &mut String,
    var: &str,
    radius: f64,
    height: f64,
    align: [AxisAlign; 3],
) {
    let span = radius * 2.0;
    let tx = centered_axis_align_offset(span, align[0]);
    let ty = centered_axis_align_offset(span, align[1]);
    let tz = axis_align_offset(height, align[2]);
    emit_aligned_shape(
        body,
        var,
        &format!("BRepPrimAPI_MakeCylinder({radius}, {height}).Shape()"),
        tx,
        ty,
        tz,
    );
}

fn emit_cone_operation(
    body: &mut String,
    var: &str,
    radius1: f64,
    radius2: f64,
    height: f64,
    align: [AxisAlign; 3],
) {
    let span = radius1.max(radius2) * 2.0;
    let tx = centered_axis_align_offset(span, align[0]);
    let ty = centered_axis_align_offset(span, align[1]);
    let tz = axis_align_offset(height, align[2]);
    emit_aligned_shape(
        body,
        var,
        &format!("BRepPrimAPI_MakeCone({radius1}, {radius2}, {height}).Shape()"),
        tx,
        ty,
        tz,
    );
}

fn emit_torus_operation(
    body: &mut String,
    var: &str,
    major: f64,
    minor: f64,
    align: [AxisAlign; 3],
) {
    let span = (major + minor) * 2.0;
    let tx = centered_axis_align_offset(span, align[0]);
    let ty = centered_axis_align_offset(span, align[1]);
    let tz = centered_axis_align_offset(minor * 2.0, align[2]);
    emit_aligned_shape(
        body,
        var,
        &format!("BRepPrimAPI_MakeTorus({major}, {minor}).Shape()"),
        tx,
        ty,
        tz,
    );
}

fn emit_draft_operation(
    body: &mut String,
    var: &str,
    input_var: String,
    angle_radians: f64,
    neutral_z: f64,
) {
    // MVP: taper every vertical (side) face about the z = neutral plane, pulling +Z.
    // Faces whose normal is perpendicular to the pull direction are the side walls.
    body.push_str(&format!(
        "    BRepOffsetAPI_DraftAngle {var}_draft({input_var});\n    gp_Dir {var}_pull(0, 0, 1);\n    gp_Pln {var}_neutral(gp_Pnt(0, 0, {neutral_z}), {var}_pull);\n    for (TopExp_Explorer {var}_fx({input_var}, TopAbs_FACE); {var}_fx.More(); {var}_fx.Next()) {{\n        TopoDS_Face {var}_f = TopoDS::Face({var}_fx.Current());\n        BRepGProp_Face {var}_props({var}_f);\n        Standard_Real {var}_u1, {var}_u2, {var}_v1, {var}_v2;\n        {var}_props.Bounds({var}_u1, {var}_u2, {var}_v1, {var}_v2);\n        gp_Pnt {var}_pnt; gp_Vec {var}_nrm;\n        {var}_props.Normal(({var}_u1 + {var}_u2) / 2.0, ({var}_v1 + {var}_v2) / 2.0, {var}_pnt, {var}_nrm);\n        if ({var}_nrm.Magnitude() < 1.0e-12) {{ continue; }}\n        gp_Dir {var}_nd({var}_nrm);\n        if (std::abs({var}_nd.Z()) < 1.0e-6) {{\n            {var}_draft.Add({var}_f, {var}_pull, {angle_radians}, {var}_neutral);\n        }}\n    }}\n    {var}_draft.Build();\n    if (!{var}_draft.IsDone()) {{ std::cerr << \"draft failed\" << std::endl; return 12; }}\n    TopoDS_Shape {var} = {var}_draft.Shape();\n"
    ));
}

fn emit_wedge_operation(body: &mut String, var: &str, dims: [f64; 7], align: [AxisAlign; 3]) {
    let [dx, dy, dz, xmin, zmin, xmax, zmax] = dims;
    let tx = axis_align_offset(dx, align[0]);
    let ty = axis_align_offset(dy, align[1]);
    let tz = axis_align_offset(dz, align[2]);
    emit_aligned_shape(
        body,
        var,
        &format!("BRepPrimAPI_MakeWedge({dx}, {dy}, {dz}, {xmin}, {zmin}, {xmax}, {zmax}).Shape()"),
        tx,
        ty,
        tz,
    );
}

fn emit_location_operation(
    body: &mut String,
    var: &str,
    frame_var: Option<String>,
    offset: [f64; 3],
    rotate: [f64; 3],
) {
    match frame_var {
        Some(frame_var) => body.push_str(&format!("    gp_Trsf {var} = {frame_var};\n")),
        None => body.push_str(&format!("    gp_Trsf {var};\n")),
    }
    emit_local_transform_multiply(body, var, offset, rotate);
}

fn emit_path_frame_operation(
    body: &mut String,
    var: &str,
    path_var: String,
    at: f64,
    up: [f64; 3],
) {
    let [up_x, up_y, up_z] = up;
    let at_literal = format!("{at:.17}");
    body.push_str(&format!(
        "    std::vector<TopoDS_Edge> {var}_edges;\n    std::vector<double> {var}_edge_lengths;\n    double {var}_total_length = 0.0;\n    for (TopExp_Explorer {var}_edge_explorer({path_var}, TopAbs_EDGE); {var}_edge_explorer.More(); {var}_edge_explorer.Next()) {{\n        TopoDS_Edge {var}_candidate_edge = TopoDS::Edge({var}_edge_explorer.Current());\n        GProp_GProps {var}_edge_props;\n        BRepGProp::LinearProperties({var}_candidate_edge, {var}_edge_props);\n        double {var}_edge_length = std::max(0.0, {var}_edge_props.Mass());\n        {var}_edges.push_back({var}_candidate_edge);\n        {var}_edge_lengths.push_back({var}_edge_length);\n        {var}_total_length += {var}_edge_length;\n    }}\n    if ({var}_edges.empty() || {var}_total_length <= 1.0e-9) {{ return 17; }}\n    Standard_Real {var}_anchor = std::min(1.0, std::max(0.0, {at_literal}));\n    double {var}_target_length = {var}_anchor * {var}_total_length;\n    std::size_t {var}_edge_index = {var}_edges.size() - 1;\n    double {var}_local_t = 1.0;\n    double {var}_walked_length = 0.0;\n    for (std::size_t {var}_candidate = 0; {var}_candidate < {var}_edges.size(); ++{var}_candidate) {{\n        double {var}_length = {var}_edge_lengths[{var}_candidate];\n        if ({var}_target_length <= {var}_walked_length + {var}_length || {var}_candidate + 1 == {var}_edges.size()) {{\n            {var}_edge_index = {var}_candidate;\n            {var}_local_t = {var}_length <= 1.0e-9 ? 0.0 : ({var}_target_length - {var}_walked_length) / {var}_length;\n            {var}_local_t = std::min(1.0, std::max(0.0, {var}_local_t));\n            break;\n        }}\n        {var}_walked_length += {var}_length;\n    }}\n    TopoDS_Edge {var}_edge = {var}_edges[{var}_edge_index];\n    BRepAdaptor_Curve {var}_curve({var}_edge);\n    Standard_Real {var}_first = {var}_curve.FirstParameter();\n    Standard_Real {var}_last = {var}_curve.LastParameter();\n    Standard_Real {var}_param = {var}_first + ({var}_last - {var}_first) * {var}_local_t;\n    gp_Pnt {var}_origin;\n    gp_Vec {var}_derivative;\n    {var}_curve.D1({var}_param, {var}_origin, {var}_derivative);\n    if ({var}_derivative.Magnitude() <= 1.0e-9) {{ return 18; }}\n    gp_Vec {var}_z = {var}_derivative;\n    {var}_z.Normalize();\n    gp_Vec {var}_up({up_x}, {up_y}, {up_z});\n    if ({var}_up.Magnitude() <= 1.0e-9) {{ return 19; }}\n    gp_Vec {var}_x = {var}_up - {var}_z.Multiplied({var}_up.Dot({var}_z));\n    if ({var}_x.Magnitude() <= 1.0e-9) {{\n        gp_Vec {var}_fallback(0, 0, 1);\n        {var}_x = {var}_fallback - {var}_z.Multiplied({var}_fallback.Dot({var}_z));\n    }}\n    if ({var}_x.Magnitude() <= 1.0e-9) {{\n        gp_Vec {var}_fallback_y(0, 1, 0);\n        {var}_x = {var}_fallback_y - {var}_z.Multiplied({var}_fallback_y.Dot({var}_z));\n    }}\n    if ({var}_x.Magnitude() <= 1.0e-9) {{ return 20; }}\n    {var}_x.Normalize();\n    gp_Vec {var}_y = {var}_z.Crossed({var}_x);\n    if ({var}_y.Magnitude() <= 1.0e-9) {{ return 21; }}\n    {var}_y.Normalize();\n    {var}_x = {var}_y.Crossed({var}_z);\n    {var}_x.Normalize();\n    gp_Trsf {var};\n    {var}.SetValues(\n        {var}_x.X(), {var}_y.X(), {var}_z.X(), {var}_origin.X(),\n        {var}_x.Y(), {var}_y.Y(), {var}_z.Y(), {var}_origin.Y(),\n        {var}_x.Z(), {var}_y.Z(), {var}_z.Z(), {var}_origin.Z());\n"
    ));
}

fn emit_place_operation(
    body: &mut String,
    var: &str,
    frame_var: String,
    shape_var: String,
    offset: [f64; 3],
    rotate: [f64; 3],
) {
    body.push_str(&format!("    gp_Trsf {var}_place_trsf = {frame_var};\n"));
    emit_local_transform_multiply(body, &format!("{var}_place_trsf"), offset, rotate);
    body.push_str(&format!(
        "    TopoDS_Shape {var} = BRepBuilderAPI_Transform({shape_var}, {var}_place_trsf, true).Shape();\n"
    ));
}

fn emit_local_transform_multiply(
    body: &mut String,
    trsf_var: &str,
    offset: [f64; 3],
    rotate: [f64; 3],
) {
    let [x, y, z] = offset;
    let rx = rotate[0].to_radians();
    let ry = rotate[1].to_radians();
    let rz = rotate[2].to_radians();
    body.push_str(&format!(
        "    gp_Trsf {trsf_var}_offset_trsf;\n    {trsf_var}_offset_trsf.SetTranslation(gp_Vec({x}, {y}, {z}));\n    {trsf_var}.Multiply({trsf_var}_offset_trsf);\n    gp_Trsf {trsf_var}_rot_x;\n    {trsf_var}_rot_x.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0)), {rx});\n    {trsf_var}.Multiply({trsf_var}_rot_x);\n    gp_Trsf {trsf_var}_rot_y;\n    {trsf_var}_rot_y.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 1, 0)), {ry});\n    {trsf_var}.Multiply({trsf_var}_rot_y);\n    gp_Trsf {trsf_var}_rot_z;\n    {trsf_var}_rot_z.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {rz});\n    {trsf_var}.Multiply({trsf_var}_rot_z);\n"
    ));
}

fn emit_clip_box_operation(
    body: &mut String,
    var: &str,
    shape_var: String,
    x: [f64; 2],
    y: [f64; 2],
    z: [f64; 2],
) {
    body.push_str(&format!(
        "    TopoDS_Shape {var}_clip_box = BRepPrimAPI_MakeBox(gp_Pnt({}, {}, {}), gp_Pnt({}, {}, {})).Shape();\n    TopoDS_Shape {var} = BRepAlgoAPI_Common({shape_var}, {var}_clip_box).Shape();\n",
        x[0], y[0], z[0], x[1], y[1], z[1]
    ));
}

fn emit_mirror_operation(
    body: &mut String,
    var: &str,
    shape_var: String,
    axis: &str,
    offset: f64,
) -> AppResult<()> {
    let (point, normal) = match axis.to_ascii_lowercase().as_str() {
        "x" => ([offset, 0.0, 0.0], [1.0, 0.0, 0.0]),
        "y" => ([0.0, offset, 0.0], [0.0, 1.0, 0.0]),
        "z" => ([0.0, 0.0, offset], [0.0, 0.0, 1.0]),
        other => {
            return Err(AppError::validation(format!(
                "Direct OCCT executor `mirror` unsupported axis `{other}`. Use `x`, `y`, or `z`."
            )));
        }
    };
    body.push_str(&format!(
        "    gp_Trsf {var}_mirror_trsf;\n    {var}_mirror_trsf.SetMirror(gp_Ax2(gp_Pnt({}, {}, {}), gp_Dir({}, {}, {})));\n    TopoDS_Shape {var} = BRepBuilderAPI_Transform({shape_var}, {var}_mirror_trsf, true).Shape();\n",
        point[0], point[1], point[2], normal[0], normal[1], normal[2]
    ));
    Ok(())
}

fn emit_taper_operation(
    body: &mut String,
    var: &str,
    height: f64,
    scale_x: f64,
    scale_y: f64,
    profile_var: String,
) {
    body.push_str(&format!(
        "    TopoDS_Wire {var}_base_wire;\n    for (TopExp_Explorer {var}_base_wire_explorer({profile_var}, TopAbs_WIRE); {var}_base_wire_explorer.More(); {var}_base_wire_explorer.Next()) {{\n        {var}_base_wire = TopoDS::Wire({var}_base_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_base_wire.IsNull()) {{ return 26; }}\n    gp_GTrsf {var}_top_scale;\n    {var}_top_scale.SetValue(1, 1, {scale_x});\n    {var}_top_scale.SetValue(2, 2, {scale_y});\n    {var}_top_scale.SetValue(3, 3, 1.0);\n    TopoDS_Shape {var}_top_scaled = BRepBuilderAPI_GTransform({profile_var}, {var}_top_scale, true).Shape();\n    gp_Trsf {var}_top_translate;\n    {var}_top_translate.SetTranslation(gp_Vec(0, 0, {height}));\n    TopoDS_Shape {var}_top_shape = BRepBuilderAPI_Transform({var}_top_scaled, {var}_top_translate, true).Shape();\n    TopoDS_Wire {var}_top_wire;\n    for (TopExp_Explorer {var}_top_wire_explorer({var}_top_shape, TopAbs_WIRE); {var}_top_wire_explorer.More(); {var}_top_wire_explorer.Next()) {{\n        {var}_top_wire = TopoDS::Wire({var}_top_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_top_wire.IsNull()) {{ return 27; }}\n    BRepOffsetAPI_ThruSections {var}_taper(true, false, 1.0e-6);\n    {var}_taper.AddWire({var}_base_wire);\n    {var}_taper.AddWire({var}_top_wire);\n    {var}_taper.Build();\n    if (!{var}_taper.IsDone()) {{ return 28; }}\n    TopoDS_Shape {var} = {var}_taper.Shape();\n"
    ));
}

fn emit_array_compound_prelude(body: &mut String, var: &str, label: &str) {
    body.push_str(&format!(
        "    BRep_Builder {var}_{label}_builder;\n    TopoDS_Compound {var}_{label}_compound;\n    {var}_{label}_builder.MakeCompound({var}_{label}_compound);\n"
    ));
}

fn emit_array_compound_finish(body: &mut String, var: &str, label: &str) {
    body.push_str(&format!(
        "    TopoDS_Shape {var} = {var}_{label}_compound;\n"
    ));
}

fn emit_linear_array_operation(
    body: &mut String,
    var: &str,
    shape_var: String,
    count: usize,
    step: [f64; 3],
) {
    emit_array_compound_prelude(body, var, "linear_array");
    for index in 0..count {
        let [x, y, z] = [
            step[0] * index as f64,
            step[1] * index as f64,
            step[2] * index as f64,
        ];
        body.push_str(&format!(
            "    gp_Trsf {var}_linear_array_{index}_trsf;\n    {var}_linear_array_{index}_trsf.SetTranslation(gp_Vec({x}, {y}, {z}));\n    TopoDS_Shape {var}_linear_array_{index}_shape = BRepBuilderAPI_Transform({shape_var}, {var}_linear_array_{index}_trsf, true).Shape();\n    {var}_linear_array_builder.Add({var}_linear_array_compound, {var}_linear_array_{index}_shape);\n"
        ));
    }
    emit_array_compound_finish(body, var, "linear_array");
}

fn emit_radial_array_operation(
    body: &mut String,
    var: &str,
    shape_var: String,
    count: usize,
    step_degrees: f64,
    radius: f64,
) {
    emit_array_compound_prelude(body, var, "radial_array");
    for index in 0..count {
        let angle = (step_degrees * index as f64).to_radians();
        body.push_str(&format!(
            "    gp_Trsf {var}_radial_array_{index}_translate;\n    {var}_radial_array_{index}_translate.SetTranslation(gp_Vec({radius}, 0, 0));\n    gp_Trsf {var}_radial_array_{index}_rotate;\n    {var}_radial_array_{index}_rotate.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {angle});\n    {var}_radial_array_{index}_rotate.Multiply({var}_radial_array_{index}_translate);\n    TopoDS_Shape {var}_radial_array_{index}_shape = BRepBuilderAPI_Transform({shape_var}, {var}_radial_array_{index}_rotate, true).Shape();\n    {var}_radial_array_builder.Add({var}_radial_array_compound, {var}_radial_array_{index}_shape);\n"
        ));
    }
    emit_array_compound_finish(body, var, "radial_array");
}

fn emit_grid_array_operation(
    body: &mut String,
    var: &str,
    shape_var: String,
    rows: usize,
    cols: usize,
    dx: f64,
    dy: f64,
) {
    emit_array_compound_prelude(body, var, "grid_array");
    for row in 0..rows {
        for col in 0..cols {
            let index = row * cols + col;
            let x = dx * col as f64;
            let y = dy * row as f64;
            body.push_str(&format!(
                "    gp_Trsf {var}_grid_array_{index}_trsf;\n    {var}_grid_array_{index}_trsf.SetTranslation(gp_Vec({x}, {y}, 0));\n    TopoDS_Shape {var}_grid_array_{index}_shape = BRepBuilderAPI_Transform({shape_var}, {var}_grid_array_{index}_trsf, true).Shape();\n    {var}_grid_array_builder.Add({var}_grid_array_compound, {var}_grid_array_{index}_shape);\n"
            ));
        }
    }
    emit_array_compound_finish(body, var, "grid_array");
}

fn emit_arc_array_operation(
    body: &mut String,
    var: &str,
    shape_var: String,
    count: usize,
    radius: f64,
    start_degrees: f64,
    end_degrees: f64,
) {
    emit_array_compound_prelude(body, var, "arc_array");
    let denominator = count.saturating_sub(1).max(1) as f64;
    for index in 0..count {
        let angle = (start_degrees + (end_degrees - start_degrees) * index as f64 / denominator)
            .to_radians();
        body.push_str(&format!(
            "    gp_Trsf {var}_arc_array_{index}_translate;\n    {var}_arc_array_{index}_translate.SetTranslation(gp_Vec({radius}, 0, 0));\n    gp_Trsf {var}_arc_array_{index}_rotate;\n    {var}_arc_array_{index}_rotate.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {angle});\n    {var}_arc_array_{index}_rotate.Multiply({var}_arc_array_{index}_translate);\n    TopoDS_Shape {var}_arc_array_{index}_shape = BRepBuilderAPI_Transform({shape_var}, {var}_arc_array_{index}_rotate, true).Shape();\n    {var}_arc_array_builder.Add({var}_arc_array_compound, {var}_arc_array_{index}_shape);\n"
        ));
    }
    emit_array_compound_finish(body, var, "arc_array");
}

fn emit_profile_face(body: &mut String, var: &str, profile: ProfileRefs) -> AppResult<()> {
    if profile.outer.is_empty() {
        return Err(AppError::validation(
            "Direct OCCT executor `profile` needs at least one outer loop.",
        ));
    }
    body.push_str(&format!(
        "    std::vector<TopoDS_Wire> {var}_outer_wires;\n    std::vector<TopoDS_Face> {var}_outer_faces;\n    std::vector<double> {var}_outer_areas;\n    std::vector<std::vector<TopoDS_Wire>> {var}_hole_wires;\n"
    ));
    for (index, outer) in profile.outer.iter().enumerate() {
        let outer_var = slot_var(*outer);
        body.push_str(&format!(
            "    TopoDS_Wire {var}_outer_{index}_wire;\n    for (TopExp_Explorer {var}_outer_{index}_wire_explorer({outer_var}, TopAbs_WIRE); {var}_outer_{index}_wire_explorer.More(); {var}_outer_{index}_wire_explorer.Next()) {{\n        {var}_outer_{index}_wire = TopoDS::Wire({var}_outer_{index}_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_outer_{index}_wire.IsNull()) {{ return 12; }}\n    BRepBuilderAPI_MakeFace {var}_outer_{index}_face_builder({var}_outer_{index}_wire);\n    if (!{var}_outer_{index}_face_builder.IsDone()) {{ return 29; }}\n    TopoDS_Face {var}_outer_{index}_face = TopoDS::Face({var}_outer_{index}_face_builder.Shape());\n    GProp_GProps {var}_outer_{index}_props;\n    BRepGProp::SurfaceProperties({var}_outer_{index}_face, {var}_outer_{index}_props);\n    {var}_outer_wires.push_back({var}_outer_{index}_wire);\n    {var}_outer_faces.push_back({var}_outer_{index}_face);\n    {var}_outer_areas.push_back(std::abs({var}_outer_{index}_props.Mass()));\n    {var}_hole_wires.emplace_back();\n"
        ));
    }
    for (index, hole) in profile.holes.iter().enumerate() {
        let hole_var = slot_var(*hole);
        body.push_str(&format!(
            "    TopoDS_Wire {var}_hole_{index}_wire;\n    for (TopExp_Explorer {var}_hole_{index}_wire_explorer({hole_var}, TopAbs_WIRE); {var}_hole_{index}_wire_explorer.More(); {var}_hole_{index}_wire_explorer.Next()) {{\n        {var}_hole_{index}_wire = TopoDS::Wire({var}_hole_{index}_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_hole_{index}_wire.IsNull()) {{ return 13; }}\n    gp_Pnt {var}_hole_{index}_sample;\n    bool {var}_hole_{index}_sample_found = false;\n    for (TopExp_Explorer {var}_hole_{index}_edge_explorer({var}_hole_{index}_wire, TopAbs_EDGE); {var}_hole_{index}_edge_explorer.More(); {var}_hole_{index}_edge_explorer.Next()) {{\n        BRepAdaptor_Curve {var}_hole_{index}_curve(TopoDS::Edge({var}_hole_{index}_edge_explorer.Current()));\n        double {var}_hole_{index}_first = {var}_hole_{index}_curve.FirstParameter();\n        double {var}_hole_{index}_last = {var}_hole_{index}_curve.LastParameter();\n        if (!std::isfinite({var}_hole_{index}_first) || !std::isfinite({var}_hole_{index}_last)) {{\n            continue;\n        }}\n        {var}_hole_{index}_sample = {var}_hole_{index}_curve.Value(({var}_hole_{index}_first + {var}_hole_{index}_last) / 2.0);\n        {var}_hole_{index}_sample_found = true;\n        break;\n    }}\n    if (!{var}_hole_{index}_sample_found) {{ return 30; }}\n    bool {var}_hole_{index}_matched = false;\n    std::size_t {var}_hole_{index}_outer_index = 0;\n    double {var}_hole_{index}_outer_area = 0.0;\n    for (std::size_t {var}_hole_{index}_candidate = 0; {var}_hole_{index}_candidate < {var}_outer_faces.size(); ++{var}_hole_{index}_candidate) {{\n        BRepClass_FaceClassifier {var}_hole_{index}_classifier({var}_outer_faces[{var}_hole_{index}_candidate], {var}_hole_{index}_sample, 1.0e-7);\n        TopAbs_State {var}_hole_{index}_state = {var}_hole_{index}_classifier.State();\n        if ({var}_hole_{index}_state != TopAbs_IN && {var}_hole_{index}_state != TopAbs_ON) {{\n            continue;\n        }}\n        double {var}_hole_{index}_candidate_area = {var}_outer_areas[{var}_hole_{index}_candidate];\n        if (!{var}_hole_{index}_matched || {var}_hole_{index}_candidate_area < {var}_hole_{index}_outer_area) {{\n            {var}_hole_{index}_matched = true;\n            {var}_hole_{index}_outer_index = {var}_hole_{index}_candidate;\n            {var}_hole_{index}_outer_area = {var}_hole_{index}_candidate_area;\n        }}\n    }}\n    if (!{var}_hole_{index}_matched) {{ return 31; }}\n    {var}_hole_wires[{var}_hole_{index}_outer_index].push_back({var}_hole_{index}_wire);\n"
        ));
    }
    body.push_str(&format!(
        "    std::vector<TopoDS_Shape> {var}_faces;\n    for (std::size_t {var}_outer_index = 0; {var}_outer_index < {var}_outer_wires.size(); ++{var}_outer_index) {{\n        BRepBuilderAPI_MakeFace {var}_face_builder({var}_outer_wires[{var}_outer_index]);\n        if (!{var}_face_builder.IsDone()) {{ return 32; }}\n        for (const auto& {var}_hole_wire : {var}_hole_wires[{var}_outer_index]) {{\n            {var}_face_builder.Add(TopoDS::Wire({var}_hole_wire.Reversed()));\n        }}\n        {var}_faces.push_back({var}_face_builder.Shape());\n    }}\n    TopoDS_Shape {var};\n    if ({var}_faces.size() == 1) {{\n        {var} = {var}_faces.front();\n    }} else {{\n        BRep_Builder {var}_profile_builder;\n        TopoDS_Compound {var}_profile_compound;\n        {var}_profile_builder.MakeCompound({var}_profile_compound);\n        for (const auto& {var}_face : {var}_faces) {{\n            {var}_profile_builder.Add({var}_profile_compound, {var}_face);\n        }}\n        {var} = {var}_profile_compound;\n    }}\n"
    ));
    Ok(())
}

fn emit_make_face_operation(body: &mut String, var: &str, input_var: String) {
    body.push_str(&format!(
        "    BRepBuilderAPI_MakeWire {var}_make_face_wire_builder;\n    bool {var}_make_face_has_profile_edge = false;\n    for (TopExp_Explorer {var}_make_face_wire_explorer({input_var}, TopAbs_WIRE); {var}_make_face_wire_explorer.More(); {var}_make_face_wire_explorer.Next()) {{\n        {var}_make_face_wire_builder.Add(TopoDS::Wire({var}_make_face_wire_explorer.Current()));\n        {var}_make_face_has_profile_edge = true;\n    }}\n    if (!{var}_make_face_has_profile_edge) {{\n        for (TopExp_Explorer {var}_make_face_edge_explorer({input_var}, TopAbs_EDGE); {var}_make_face_edge_explorer.More(); {var}_make_face_edge_explorer.Next()) {{\n            {var}_make_face_wire_builder.Add(TopoDS::Edge({var}_make_face_edge_explorer.Current()));\n            {var}_make_face_has_profile_edge = true;\n        }}\n    }}\n    if (!{var}_make_face_has_profile_edge) {{ return 14; }}\n    if (!{var}_make_face_wire_builder.IsDone()) {{ return 33; }}\n    BRepBuilderAPI_MakeFace {var}_make_face_face({var}_make_face_wire_builder.Wire());\n    if (!{var}_make_face_face.IsDone()) {{ return 34; }}\n    TopoDS_Shape {var} = {var}_make_face_face.Shape();\n"
    ));
}

fn emit_import_stl_operation(body: &mut String, var: &str, path: &str) {
    let path = format!("{path:?}");
    body.push_str(&format!(
        "    StlAPI_Reader {var}_reader;\n    TopoDS_Shape {var};\n    if (!{var}_reader.Read({var}, {path}.c_str())) {{ return 17; }}\n"
    ));
}

fn emit_offset_operation(body: &mut String, var: &str, input_var: String, amount: f64) {
    body.push_str(&format!(
        "    TopoDS_Wire {var}_offset_input_wire;\n    for (TopExp_Explorer {var}_offset_input_wire_explorer({input_var}, TopAbs_WIRE); {var}_offset_input_wire_explorer.More(); {var}_offset_input_wire_explorer.Next()) {{\n        {var}_offset_input_wire = TopoDS::Wire({var}_offset_input_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_offset_input_wire.IsNull()) {{ return 15; }}\n    BRepOffsetAPI_MakeOffset {var}_offset({var}_offset_input_wire, GeomAbs_Arc, false);\n    {var}_offset.Perform({amount});\n    TopoDS_Shape {var}_offset_shape = {var}_offset.Shape();\n    TopoDS_Wire {var}_offset_wire;\n    for (TopExp_Explorer {var}_offset_wire_explorer({var}_offset_shape, TopAbs_WIRE); {var}_offset_wire_explorer.More(); {var}_offset_wire_explorer.Next()) {{\n        {var}_offset_wire = TopoDS::Wire({var}_offset_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_offset_wire.IsNull()) {{ return 16; }}\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_offset_wire).Shape();\n"
    ));
}

fn emit_sweep_operation(
    body: &mut String,
    var: &str,
    profile_var: String,
    path_var: String,
    frenet: bool,
) {
    // `frenet` selects the Frenet trihedron (Standard_True) for helical spines so
    // the section stays radial; generic sweeps keep corrected-Frenet
    // (Standard_False) to match build123d's default. Without an explicit SetMode
    // a curved spine has no trihedron.
    let mode = if frenet {
        "Standard_True"
    } else {
        "Standard_False"
    };
    body.push_str(&format!(
        "    TopoDS_Wire {var}_profile_wire;\n    for (TopExp_Explorer {var}_profile_wire_explorer({profile_var}, TopAbs_WIRE); {var}_profile_wire_explorer.More(); {var}_profile_wire_explorer.Next()) {{\n        {var}_profile_wire = TopoDS::Wire({var}_profile_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_profile_wire.IsNull()) {{ return 7; }}\n    TopoDS_Wire {var}_path_wire;\n    for (TopExp_Explorer {var}_path_wire_explorer({path_var}, TopAbs_WIRE); {var}_path_wire_explorer.More(); {var}_path_wire_explorer.Next()) {{\n        {var}_path_wire = TopoDS::Wire({var}_path_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_path_wire.IsNull()) {{ return 8; }}\n    BRepOffsetAPI_MakePipeShell {var}_pipe({var}_path_wire);\n    {var}_pipe.SetMode({mode});\n    {var}_pipe.Add({var}_profile_wire);\n    {var}_pipe.Build();\n    if (!{var}_pipe.IsDone()) {{ return 9; }}\n    {var}_pipe.MakeSolid();\n    TopoDS_Shape {var} = {var}_pipe.Shape();\n"
    ));
}

fn optional_bool_keyword(keywords: &[OcctKeyword], names: &[&str]) -> Option<bool> {
    keywords.iter().find_map(|keyword| {
        if names.contains(&keyword.name.as_str()) {
            if let OcctArg::Boolean(value) = keyword.source_arg() {
                return Some(*value);
            }
        }
        None
    })
}

fn emit_twist_operation(
    body: &mut String,
    var: &str,
    height: f64,
    angle_radians: f64,
    profile_var: String,
) {
    let segments = 12usize;
    body.push_str(&format!(
        "    BRepOffsetAPI_ThruSections {var}_twist(true, false, 1.0e-6);\n"
    ));
    for index in 0..=segments {
        let ratio = index as f64 / segments as f64;
        let z = height * ratio;
        let angle = angle_radians * ratio;
        body.push_str(&format!(
            "    gp_Trsf {var}_section_{index}_rot;\n    {var}_section_{index}_rot.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {angle});\n    TopoDS_Shape {var}_section_{index}_rotated = BRepBuilderAPI_Transform({profile_var}, {var}_section_{index}_rot, true).Shape();\n    gp_Trsf {var}_section_{index}_trsf;\n    {var}_section_{index}_trsf.SetTranslation(gp_Vec(0, 0, {z}));\n    TopoDS_Shape {var}_section_{index}_shape = BRepBuilderAPI_Transform({var}_section_{index}_rotated, {var}_section_{index}_trsf, true).Shape();\n    TopoDS_Wire {var}_section_{index}_wire;\n    for (TopExp_Explorer {var}_section_{index}_wire_explorer({var}_section_{index}_shape, TopAbs_WIRE); {var}_section_{index}_wire_explorer.More(); {var}_section_{index}_wire_explorer.Next()) {{\n        {var}_section_{index}_wire = TopoDS::Wire({var}_section_{index}_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_section_{index}_wire.IsNull()) {{ return 10; }}\n    {var}_twist.AddWire({var}_section_{index}_wire);\n"
        ));
    }
    body.push_str(&format!(
        "    {var}_twist.Build();\n    if (!{var}_twist.IsDone()) {{ return 11; }}\n    TopoDS_Shape {var} = {var}_twist.Shape();\n"
    ));
}

fn emit_polygon_face(body: &mut String, var: &str, points: &[[f64; 2]]) -> AppResult<()> {
    if points.len() < 3 {
        return Err(AppError::validation(
            "Direct OCCT executor polygon face requires at least three points.",
        ));
    }
    body.push_str(&format!("    BRepBuilderAPI_MakePolygon {var}_polygon;\n"));
    for [x, y] in points {
        body.push_str(&format!("    {var}_polygon.Add(gp_Pnt({x}, {y}, 0));\n"));
    }
    body.push_str(&format!(
        "    {var}_polygon.Close();\n    TopoDS_Wire {var}_wire = {var}_polygon.Wire();\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n"
    ));
    Ok(())
}

fn emit_rounded_rectangle_face(body: &mut String, var: &str, width: f64, height: f64, radius: f64) {
    body.push_str(&format!(
        "    Standard_Real {var}_w = {width};\n    Standard_Real {var}_h = {height};\n    Standard_Real {var}_radius = {radius};\n    Standard_Real {var}_r = std::min(std::abs({var}_radius), std::min(std::abs({var}_w) / 2.0, std::abs({var}_h) / 2.0));\n    Standard_Real {var}_x0 = -{var}_w / 2.0;\n    Standard_Real {var}_y0 = -{var}_h / 2.0;\n    Standard_Real {var}_x1 = {var}_w / 2.0;\n    Standard_Real {var}_y1 = {var}_h / 2.0;\n    TopoDS_Shape {var};\n    if ({var}_r <= 1.0e-12) {{\n        BRepBuilderAPI_MakePolygon {var}_polygon;\n        {var}_polygon.Add(gp_Pnt({var}_x0, {var}_y0, 0));\n        {var}_polygon.Add(gp_Pnt({var}_x1, {var}_y0, 0));\n        {var}_polygon.Add(gp_Pnt({var}_x1, {var}_y1, 0));\n        {var}_polygon.Add(gp_Pnt({var}_x0, {var}_y1, 0));\n        {var}_polygon.Close();\n        TopoDS_Wire {var}_wire = {var}_polygon.Wire();\n        {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n    }} else {{\n        Standard_Real {var}_arc_mid = {var}_r * std::sqrt(0.5);\n        BRepBuilderAPI_MakeWire {var}_wire_builder;\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({var}_x0 + {var}_r, {var}_y0, 0), gp_Pnt({var}_x1 - {var}_r, {var}_y0, 0)).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({var}_x1 - {var}_r, {var}_y0, 0), gp_Pnt({var}_x1 - {var}_r + {var}_arc_mid, {var}_y0 + {var}_r - {var}_arc_mid, 0), gp_Pnt({var}_x1, {var}_y0 + {var}_r, 0)).Value()).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({var}_x1, {var}_y0 + {var}_r, 0), gp_Pnt({var}_x1, {var}_y1 - {var}_r, 0)).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({var}_x1, {var}_y1 - {var}_r, 0), gp_Pnt({var}_x1 - {var}_r + {var}_arc_mid, {var}_y1 - {var}_r + {var}_arc_mid, 0), gp_Pnt({var}_x1 - {var}_r, {var}_y1, 0)).Value()).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({var}_x1 - {var}_r, {var}_y1, 0), gp_Pnt({var}_x0 + {var}_r, {var}_y1, 0)).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({var}_x0 + {var}_r, {var}_y1, 0), gp_Pnt({var}_x0 + {var}_r - {var}_arc_mid, {var}_y1 - {var}_r + {var}_arc_mid, 0), gp_Pnt({var}_x0, {var}_y1 - {var}_r, 0)).Value()).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({var}_x0, {var}_y1 - {var}_r, 0), gp_Pnt({var}_x0, {var}_y0 + {var}_r, 0)).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({var}_x0, {var}_y0 + {var}_r, 0), gp_Pnt({var}_x0 + {var}_r - {var}_arc_mid, {var}_y0 + {var}_r - {var}_arc_mid, 0), gp_Pnt({var}_x0 + {var}_r, {var}_y0, 0)).Value()).Edge());\n        TopoDS_Wire {var}_wire = {var}_wire_builder.Wire();\n        {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n    }}\n"
    ));
}

#[derive(Debug, Clone, Copy)]
struct RoundedCorner {
    p_in: [f64; 2],
    p_out: [f64; 2],
    mid: [f64; 2],
    rounded: bool,
}

fn emit_rounded_polygon_face(
    body: &mut String,
    var: &str,
    points: &[[f64; 2]],
    radius: f64,
) -> AppResult<()> {
    let corners = rounded_polygon_corners(points, radius)?;
    if corners.iter().all(|corner| !corner.rounded) {
        return emit_polygon_face(body, var, points);
    }

    body.push_str(&format!(
        "    BRepBuilderAPI_MakeWire {var}_wire_builder;\n"
    ));
    for index in 0..corners.len() {
        let current = corners[index];
        let next = corners[(index + 1) % corners.len()];
        if distance2(current.p_out, next.p_in) > 1.0e-9 {
            body.push_str(&format!(
                "    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({}, {}, 0), gp_Pnt({}, {}, 0)).Edge());\n",
                current.p_out[0], current.p_out[1], next.p_in[0], next.p_in[1]
            ));
        }
        if next.rounded {
            body.push_str(&format!(
                "    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({}, {}, 0), gp_Pnt({}, {}, 0), gp_Pnt({}, {}, 0)).Value()).Edge());\n",
                next.p_in[0],
                next.p_in[1],
                next.mid[0],
                next.mid[1],
                next.p_out[0],
                next.p_out[1]
            ));
        }
    }
    body.push_str(&format!(
        "    TopoDS_Wire {var}_wire = {var}_wire_builder.Wire();\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n"
    ));
    Ok(())
}

fn rounded_polygon_corners(points: &[[f64; 2]], radius: f64) -> AppResult<Vec<RoundedCorner>> {
    let points = normalize_closed_points(points)?;
    if points.len() < 3 {
        return Err(AppError::validation(
            "Direct OCCT executor `rounded-polygon` requires at least three points.",
        ));
    }
    let requested_radius = radius.abs();
    if requested_radius <= 1.0e-12 {
        return Ok(points
            .iter()
            .map(|point| RoundedCorner {
                p_in: *point,
                p_out: *point,
                mid: *point,
                rounded: false,
            })
            .collect());
    }

    let count = points.len();
    let mut corners = Vec::with_capacity(count);
    for index in 0..count {
        let prev = points[(index + count - 1) % count];
        let curr = points[index];
        let next = points[(index + 1) % count];
        let in_vec = sub2(prev, curr);
        let out_vec = sub2(next, curr);
        let len_in = length2(in_vec);
        let len_out = length2(out_vec);
        if len_in <= 1.0e-12 || len_out <= 1.0e-12 {
            return Err(AppError::validation(
                "Direct OCCT executor `rounded-polygon` got a zero-length edge.",
            ));
        }
        let in_dir = mul2(in_vec, 1.0 / len_in);
        let out_dir = mul2(out_vec, 1.0 / len_out);
        let dot = dot2(in_dir, out_dir).clamp(-1.0, 1.0);
        let theta = dot.acos();
        let tan_half = if theta > 1.0e-12 {
            (theta / 2.0).tan()
        } else {
            0.0
        };
        let bisector = add2(in_dir, out_dir);
        let bisector_len = length2(bisector);
        if tan_half <= 1.0e-12 || bisector_len <= 1.0e-12 {
            corners.push(RoundedCorner {
                p_in: curr,
                p_out: curr,
                mid: curr,
                rounded: false,
            });
            continue;
        }
        let corner_radius = requested_radius.min(len_in.min(len_out) * tan_half);
        if corner_radius <= 1.0e-12 {
            corners.push(RoundedCorner {
                p_in: curr,
                p_out: curr,
                mid: curr,
                rounded: false,
            });
            continue;
        }
        let tangent = corner_radius / tan_half;
        let bisector = mul2(bisector, 1.0 / bisector_len);
        let center_dist = corner_radius / (theta / 2.0).sin();
        let p_in = add2(curr, mul2(in_dir, tangent));
        let p_out = add2(curr, mul2(out_dir, tangent));
        let center = add2(curr, mul2(bisector, center_dist));
        let mid_dir = sub2(curr, center);
        let mid_len = length2(mid_dir);
        if mid_len <= 1.0e-12 {
            corners.push(RoundedCorner {
                p_in: curr,
                p_out: curr,
                mid: curr,
                rounded: false,
            });
            continue;
        }
        let mid = add2(center, mul2(mid_dir, corner_radius / mid_len));
        corners.push(RoundedCorner {
            p_in,
            p_out,
            mid,
            rounded: true,
        });
    }
    Ok(corners)
}

fn normalize_closed_points(points: &[[f64; 2]]) -> AppResult<Vec<[f64; 2]>> {
    if points.len() < 3 {
        return Err(AppError::validation(
            "Direct OCCT executor `rounded-polygon` requires at least three points.",
        ));
    }
    let mut points = points.to_vec();
    if points.len() >= 2 && distance2(points[0], *points.last().expect("non-empty")) <= 1.0e-12 {
        points.pop();
    }
    if points.len() < 3 {
        return Err(AppError::validation(
            "Direct OCCT executor `rounded-polygon` requires at least three points.",
        ));
    }
    Ok(points)
}

fn sub2(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [left[0] - right[0], left[1] - right[1]]
}

fn add2(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [left[0] + right[0], left[1] + right[1]]
}

fn mul2(point: [f64; 2], scalar: f64) -> [f64; 2] {
    [point[0] * scalar, point[1] * scalar]
}

fn dot2(left: [f64; 2], right: [f64; 2]) -> f64 {
    left[0] * right[0] + left[1] * right[1]
}

fn length2(point: [f64; 2]) -> f64 {
    (point[0] * point[0] + point[1] * point[1]).sqrt()
}

fn distance2(left: [f64; 2], right: [f64; 2]) -> f64 {
    length2(sub2(left, right))
}

fn emit_boolean_fold(
    body: &mut String,
    var: &str,
    op_name: &str,
    api_name: &str,
    inputs: Vec<OcctSlot>,
) -> AppResult<()> {
    if inputs.len() < 2 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `{op_name}` requires at least two operands."
        )));
    }
    let mut iter = inputs.into_iter();
    let first = iter.next().expect("checked non-empty");
    body.push_str(&format!("    TopoDS_Shape {var} = {};\n", slot_var(first)));
    for input in iter {
        body.push_str(&format!(
            "    {var} = {api_name}({var}, {}).Shape();\n",
            slot_var(input)
        ));
    }
    Ok(())
}

fn slot_var(slot: OcctSlot) -> String {
    format!("shape_{}", slot.0)
}

fn op_name(op: OcctOp) -> &'static str {
    match op {
        OcctOp::Box => "box",
        OcctOp::Sphere => "sphere",
        OcctOp::Cylinder => "cylinder",
        OcctOp::Cone => "cone",
        OcctOp::Torus => "torus",
        OcctOp::Wedge => "wedge",
        OcctOp::Ellipse => "ellipse",
        OcctOp::Slot => "slot-overall",
        OcctOp::SlotArc => "slot-arc",
        OcctOp::Circle => "circle",
        OcctOp::Rectangle => "rectangle",
        OcctOp::RoundedRectangle => "rounded-rect",
        OcctOp::RoundedPolygon => "rounded-polygon",
        OcctOp::Polygon => "polygon",
        OcctOp::Profile => "profile",
        OcctOp::MakeFace => "make-face",
        OcctOp::ImportStl => "import-stl",
        OcctOp::Extrude => "extrude",
        OcctOp::Revolve => "revolve",
        OcctOp::Loft => "loft",
        OcctOp::Sweep => "sweep",
        OcctOp::Twist => "twist",
        OcctOp::Taper => "taper",
        OcctOp::Draft => "draft",
        OcctOp::Offset => "offset",
        OcctOp::Path => "path",
        OcctOp::HelixPath => "helix-path",
        OcctOp::BezierPath => "bezier-path",
        OcctOp::Bspline => "bspline",
        OcctOp::Plane => "plane",
        OcctOp::Location => "location",
        OcctOp::PathFrame => "path-frame",
        OcctOp::Place => "place",
        OcctOp::ClipBox => "clip-box",
        OcctOp::LinearArray => "linear-array",
        OcctOp::RadialArray => "radial-array",
        OcctOp::GridArray => "grid-array",
        OcctOp::ArcArray => "arc-array",
        OcctOp::Union => "union",
        OcctOp::Difference => "difference",
        OcctOp::Intersection => "intersection",
        OcctOp::Fillet => "fillet",
        OcctOp::Chamfer => "chamfer",
        OcctOp::Shell => "shell",
        OcctOp::Translate => "translate",
        OcctOp::Rotate => "rotate",
        OcctOp::Scale => "scale",
        OcctOp::Mirror => "mirror",
        OcctOp::Compound => "compound",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecky_cad_host::direct_occt_sdk::{
        bundled_build123d_runtime_root_from_repo, inspect_build123d_ocp_runtime,
    };
    use crate::models::PathResolver;

    struct TestResolver;

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            temp_root("direct-occt-executor-config")
        }

        fn app_data_dir(&self) -> PathBuf {
            temp_root("direct-occt-executor-data")
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn compile(source: &str) -> CoreProgram {
        crate::ecky_scheme::compile_to_core_program(source).expect("compile")
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{label}-{}", uuid::Uuid::new_v4()))
    }

    fn text_font_fixture() -> Option<&'static str> {
        [
            "/System/Library/Fonts/Supplemental/Arial Black.ttf",
            "/System/Library/Fonts/Supplemental/Impact.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
            "C:/Windows/Fonts/arial.ttf",
        ]
        .into_iter()
        .find(|path| Path::new(path).is_file())
    }

    fn write_ascii_stl_fixture(path: &Path) {
        std::fs::write(
            path,
            "solid fixture\n  facet normal 0 0 1\n    outer loop\n      vertex 0 0 0\n      vertex 10 0 0\n      vertex 0 10 0\n    endloop\n  endfacet\nendsolid fixture\n",
        )
        .expect("write stl fixture");
    }

    #[test]
    fn emits_box_plan_as_native_occt_source() {
        let program = compile("(model (part body (box 10 20 30)))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepPrimAPI_MakeBox(10"));
        assert!(source.contains("20"));
        assert!(source.contains("30"));
        assert!(source.contains("STEPControl_Writer"));
        assert!(source.contains("write_binary_stl_mesh"));
        assert!(source.contains("BRepMesh_IncrementalMesh mesh(shape, 0.05, false, 0.15, true);"));
        assert!(source.contains("/tmp/model.step"));
        assert!(source.contains("/tmp/preview.stl"));
    }

    #[test]
    fn emits_box_align_plan_as_native_occt_source() {
        let plan = OcctPlan {
            parameters: Vec::new(),
            parts: vec![super::super::direct_occt::OcctPartPlan {
                key: "body".to_string(),
                label: "Body".to_string(),
                root: OcctSlot(1),
                commands: vec![OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Box,
                    args: vec![
                        OcctArg::Number(10.0),
                        OcctArg::Number(20.0),
                        OcctArg::Number(30.0),
                    ],
                    keywords: vec![OcctKeyword::arg(
                        "align".to_string(),
                        OcctArg::List(vec![
                            OcctArg::Symbol("min".to_string()),
                            OcctArg::Symbol("center".to_string()),
                            OcctArg::Symbol("max".to_string()),
                        ]),
                    )],
                }],
            }],
        };

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepPrimAPI_MakeBox(10"), "{source}");
        assert!(
            source.contains("SetTranslation(gp_Vec(0, -10, -30))"),
            "{source}"
        );
    }

    #[test]
    fn emits_round_primitive_align_plan_as_native_occt_source() {
        let plan = OcctPlan {
            parameters: Vec::new(),
            parts: vec![super::super::direct_occt::OcctPartPlan {
                key: "body".to_string(),
                label: "Body".to_string(),
                root: OcctSlot(3),
                commands: vec![
                    OcctCommand {
                        output: OcctSlot(1),
                        op: OcctOp::Cylinder,
                        args: vec![OcctArg::Number(2.0), OcctArg::Number(10.0)],
                        keywords: vec![OcctKeyword::arg(
                            "align".to_string(),
                            OcctArg::List(vec![
                                OcctArg::Symbol("min".to_string()),
                                OcctArg::Symbol("center".to_string()),
                                OcctArg::Symbol("max".to_string()),
                            ]),
                        )],
                    },
                    OcctCommand {
                        output: OcctSlot(2),
                        op: OcctOp::Sphere,
                        args: vec![OcctArg::Number(3.0)],
                        keywords: vec![OcctKeyword::arg(
                            "align".to_string(),
                            OcctArg::List(vec![
                                OcctArg::Symbol("max".to_string()),
                                OcctArg::Symbol("center".to_string()),
                                OcctArg::Symbol("min".to_string()),
                            ]),
                        )],
                    },
                    OcctCommand {
                        output: OcctSlot(3),
                        op: OcctOp::Cone,
                        args: vec![
                            OcctArg::Number(4.0),
                            OcctArg::Number(1.0),
                            OcctArg::Number(12.0),
                        ],
                        keywords: vec![OcctKeyword::arg(
                            "align".to_string(),
                            OcctArg::List(vec![
                                OcctArg::Symbol("center".to_string()),
                                OcctArg::Symbol("min".to_string()),
                                OcctArg::Symbol("center".to_string()),
                            ]),
                        )],
                    },
                ],
            }],
        };

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(
            source.contains("BRepPrimAPI_MakeCylinder(2, 10"),
            "{source}"
        );
        assert!(
            source.contains("shape_1_align_trsf.SetTranslation(gp_Vec(2, 0, -10))"),
            "{source}"
        );
        assert!(source.contains("BRepPrimAPI_MakeSphere(3"), "{source}");
        assert!(
            source.contains("shape_2_align_trsf.SetTranslation(gp_Vec(-3, 0, 3))"),
            "{source}"
        );
        assert!(source.contains("BRepPrimAPI_MakeCone(4, 1, 12"), "{source}");
        assert!(
            source.contains("shape_3_align_trsf.SetTranslation(gp_Vec(0, 4, -6))"),
            "{source}"
        );
    }

    #[test]
    fn emits_topology_report_writer_for_native_occt_faces() {
        let program = compile("(model (part body (box 10 20 30)))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepGProp::SurfaceProperties"), "{source}");
        assert!(source.contains("TopExp_Explorer"), "{source}");
        assert!(source.contains("TopAbs_FACE"), "{source}");
        assert!(source.contains("/tmp/topology.json"), "{source}");
        assert!(
            source.contains("write_part_faces(topology_file"),
            "{source}"
        );
    }

    #[test]
    fn emits_topology_report_writer_for_native_occt_edges() {
        let program = compile("(model (part body (box 10 20 30)))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("TopAbs_EDGE"), "{source}");
        assert!(source.contains("\\\"edges\\\""), "{source}");
        assert!(source.contains("\\\"edgeIndex\\\""), "{source}");
        assert!(source.contains("BRepAdaptor_Curve curve(edge)"), "{source}");
    }

    #[test]
    fn emits_originating_slot_indexes_in_topology_report() {
        let plan = OcctPlan {
            parameters: Vec::new(),
            parts: vec![super::super::direct_occt::OcctPartPlan {
                key: "body".to_string(),
                label: "Body".to_string(),
                root: OcctSlot(2),
                commands: vec![
                    OcctCommand {
                        output: OcctSlot(1),
                        op: OcctOp::Box,
                        args: vec![
                            OcctArg::Number(10.0),
                            OcctArg::Number(20.0),
                            OcctArg::Number(30.0),
                        ],
                        keywords: Vec::new(),
                    },
                    OcctCommand {
                        output: OcctSlot(2),
                        op: OcctOp::Translate,
                        args: vec![
                            OcctArg::Number(0.0),
                            OcctArg::Number(0.0),
                            OcctArg::Number(5.0),
                            OcctArg::Ref(OcctSlot(1)),
                        ],
                        keywords: Vec::new(),
                    },
                ],
            }],
        };

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("\\\"originatingSlotIndex\\\""), "{source}");
        assert!(
            source.contains("std::vector<std::uint64_t>{1, 2}"),
            "{source}"
        );
        assert!(
            source.contains("std::vector<TopoDS_Shape>{shape_1, shape_2}"),
            "{source}"
        );
        assert!(
            source.contains("direct_occt_originating_slot_index("),
            "{source}"
        );
    }

    #[test]
    fn emits_multi_part_model_as_top_level_compound() {
        let program = compile(
            r#"
            (model
              (part base (box 10 20 3))
              (part peg (translate 0 0 3 (cylinder 2 8))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepPrimAPI_MakeBox(10"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakeCylinder(2"), "{source}");
        assert!(
            source.contains("model_compound_builder.MakeCompound"),
            "{source}"
        );
        assert!(
            source.contains("model_compound_builder.Add(model_compound"),
            "{source}"
        );
        assert!(
            source.contains("TopoDS_Shape shape = model_compound_shape"),
            "{source}"
        );
    }

    #[test]
    fn emits_sketch_surface_ops_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (union
                  (extrude (circle 10) 5)
                  (translate 24 0 0 (extrude (rectangle 8 12) 3))
                  (translate -24 0 0
                    (extrude (polygon ((0 0) (8 0) (8 6) (0 6))) 2)))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("gp_Circ"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakePolygon"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakeFace"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
        assert!(source.contains("gp_Vec(0, 0, 5"), "{source}");
    }

    #[test]
    fn emits_rounded_rectangle_as_native_occt_wire_face() {
        let program = compile("(model (part body (extrude (rounded_rect 20 10 2) 5)))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("GC_MakeArcOfCircle"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakeWire"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakeFace"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
    }

    #[test]
    fn rounded_rectangle_arcs_use_trimmed_corner_midpoints() {
        let mut source = String::new();

        emit_rounded_rectangle_face(&mut source, "rr", 20.0, 10.0, 2.0);

        assert!(
            source.contains("Standard_Real rr_arc_mid = rr_r * std::sqrt(0.5);"),
            "{source}"
        );
        assert!(
            source.contains(
                "GC_MakeArcOfCircle(gp_Pnt(rr_x1 - rr_r, rr_y0, 0), gp_Pnt(rr_x1 - rr_r + rr_arc_mid, rr_y0 + rr_r - rr_arc_mid, 0), gp_Pnt(rr_x1, rr_y0 + rr_r, 0))"
            ),
            "{source}"
        );
        assert!(
            !source.contains(
                "GC_MakeArcOfCircle(gp_Pnt(rr_x1 - rr_r, rr_y0, 0), gp_Pnt(rr_x1, rr_y0, 0), gp_Pnt(rr_x1, rr_y0 + rr_r, 0))"
            ),
            "{source}"
        );
    }

    #[test]
    fn emits_rounded_polygon_as_native_occt_wire_face() {
        let program = compile(
            "(model (part body (extrude (rounded-polygon ((0 0) (20 0) (20 10) (0 10)) 2) 5)))",
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("GC_MakeArcOfCircle"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakeWire"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakeFace"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
    }

    #[test]
    fn rounded_polygon_rejects_zero_length_edges_before_native_compile() {
        let program = compile(
            "(model (part body (extrude (rounded-polygon ((0 0) (20 0) (20 0) (0 10)) 2) 5)))",
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("zero edge");
        let message = err.to_string();

        assert!(message.contains("rounded-polygon"), "{message}");
        assert!(message.contains("zero-length edge"), "{message}");
    }

    #[test]
    fn path_rejects_less_than_two_points_before_native_compile() {
        let program = compile("(model (part body (sweep (circle 5) (path ((0 0 0))))))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("short path");
        let message = err.to_string();

        assert!(message.contains("path"), "{message}");
        assert!(message.contains("at least two points"), "{message}");
    }

    #[test]
    fn bezier_path_rejects_non_cubic_control_count_before_native_compile() {
        let program = compile(
            r#"
            (model
              (part body
                (sweep
                  (circle 2)
                  (bezier-path ((0 0 0) (4 0 0) (8 4 0) (12 4 0) (16 8 0))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("bad bezier point count");
        let message = err.to_string();

        assert!(message.contains("bezier-path"), "{message}");
        assert!(message.contains("3n+1"), "{message}");
    }

    #[test]
    fn emits_revolve_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (revolve
                  (polygon ((8 0) (12 0) (12 20) (8 20)))
                  360)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepPrimAPI_MakeRevol"), "{source}");
        assert!(
            source.contains("SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0))"),
            "{source}"
        );
        assert!(
            source.contains("gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1))"),
            "{source}"
        );
        assert!(source.contains("6.283185307179586"), "{source}");
    }

    #[test]
    fn emits_loft_as_native_occt_source() {
        let program = compile("(model (part body (loft 30 (circle 10) (rounded-rect 12 8 2))))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepOffsetAPI_ThruSections"), "{source}");
        assert!(source.contains(".AddWire("), "{source}");
        assert!(source.contains("_loft.Build()"), "{source}");
        assert!(source.contains("gp_Vec(0, 0, 30"), "{source}");
    }

    #[test]
    fn emits_sweep_path_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (sweep
                  (circle 5)
                  (path ((0 0 0) (0 0 24))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepOffsetAPI_MakePipeShell"), "{source}");
        assert!(source.contains("gp_Pnt(0, 0, 24"), "{source}");
        assert!(source.contains("_pipe.MakeSolid()"), "{source}");
        assert!(source.contains("_pipe.Shape()"), "{source}");
    }

    #[test]
    fn emits_bezier_path_as_native_occt_wire() {
        let program = compile(
            r#"
            (model
              (part body
                (sweep
                  (circle 2)
                  (bezier-path ((0 0 0) (8 0 0) (8 8 12) (16 8 12))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("Geom_BezierCurve"), "{source}");
        assert!(source.contains("TColgp_Array1OfPnt"), "{source}");
        assert!(source.contains("BRepOffsetAPI_MakePipeShell"), "{source}");
        assert!(source.contains("gp_Pnt(16, 8, 12"), "{source}");
    }

    #[test]
    fn emits_bspline_profile_as_native_occt_face() {
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (bspline ((0 6) (5 2) (6 -4) (0 -6) (-6 -4) (-5 2)) #t)
                  4)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("GeomAPI_PointsToBSpline"), "{source}");
        assert!(source.contains("Geom_BSplineCurve"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakeFace"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
    }

    #[test]
    fn emits_open_bspline_with_tangents_as_native_occt_wire() {
        let program = compile(
            r#"
            (model
              (part cup
                (make-face
                  (union
                    (bspline ((30 10) (69 105)) #f
                      :tangents ((1 0.5) (0.7 1))
                      :tangent-scalars (1.75 1))
                    (path (30 10 0) (40 0 0) (0 0 0) (0 105 0) (69 105 0))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("Geom_BezierCurve"), "{source}");
        assert!(source.contains("TopoDS_Shape shape_"), "{source}");
        assert!(source.contains("_make_face_face"), "{source}");
    }

    #[test]
    fn emits_helical_ridge_as_native_occt_sweep() {
        let program = compile(
            r#"
            (model
              (part body
                (helical-ridge
                  :radius 20
                  :pitch 6
                  :height 30
                  :base-width 2
                  :crest-width 1
                  :depth 1.5)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepBuilderAPI_MakePolygon"), "{source}");
        assert!(source.contains("gp_Pnt(20"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakeFace"), "{source}");
        assert!(source.contains("BRepOffsetAPI_MakePipeShell"), "{source}");
    }

    #[test]
    fn emits_import_stl_as_native_occt_source() {
        let stl_path =
            std::env::temp_dir().join(format!("ecky-import-stl-{}.stl", uuid::Uuid::new_v4()));
        write_ascii_stl_fixture(&stl_path);
        let program = compile(&format!(
            "(model (part body (import-stl {:?})))",
            stl_path.to_string_lossy()
        ));
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("StlAPI_Reader"), "{source}");
        assert!(source.contains("import-stl"), "{source}");
        let _ = std::fs::remove_file(stl_path);
    }

    #[test]
    fn emits_twist_as_native_occt_source() {
        let program = compile("(model (part body (twist 24 90 (circle 5))))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepOffsetAPI_ThruSections"), "{source}");
        assert!(source.contains("1.5707963267948966"), "{source}");
        assert!(source.contains("gp_Vec(0, 0, 24"), "{source}");
        assert!(source.contains("_twist.Build()"), "{source}");
    }

    #[test]
    fn emits_profile_holes_as_native_occt_face() {
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (profile :outer (circle 10) :holes (circle 3))
                  4)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepBuilderAPI_MakeFace"), "{source}");
        assert!(source.contains("_hole_wires["), "{source}");
        assert!(source.contains(".Add(TopoDS::Wire("), "{source}");
        assert!(source.contains(".Reversed())"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
    }

    #[test]
    fn emits_positional_multi_outer_profile_as_native_occt_compound() {
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (profile
                    (polygon ((0 0) (24 0) (24 24) (0 24)))
                    (translate 36 0 0 (polygon ((0 0) (12 0) (12 12) (0 12)))))
                  4)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("std::vector<TopoDS_Wire>"), "{source}");
        assert!(source.contains("_profile_builder.MakeCompound"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
    }

    #[test]
    fn emits_multi_outer_profile_holes_as_native_occt_compound() {
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (profile
                    :outer ((polygon ((0 0) (24 0) (24 24) (0 24)))
                            (translate 40 0 0 (polygon ((0 0) (12 0) (12 12) (0 12)))))
                    :holes ((polygon ((8 8) (16 8) (16 16) (8 16)))))
                  4)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepClass_FaceClassifier"), "{source}");
        assert!(source.contains("_hole_wires["), "{source}");
        assert!(source.contains("_profile_builder.MakeCompound"), "{source}");
    }

    #[test]
    fn emits_make_face_as_native_occt_face() {
        let program = compile(
            "(model (part body (extrude (make-face (polygon ((0 0) (8 0) (8 6) (0 6)))) 4)))",
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("_make_face_face"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
    }

    #[test]
    fn emits_offset_as_native_occt_face() {
        let program = compile("(model (part body (extrude (offset 2 (circle 10)) 4)))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepOffsetAPI_MakeOffset"), "{source}");
        assert!(source.contains("_offset.Perform(2"), "{source}");
        assert!(source.contains("BRepBuilderAPI_MakeFace"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
    }

    #[test]
    fn emits_path_frame_place_as_native_occt_transform() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape rail (path ((0 0 0) (0 0 20))))
                  (shape peg (cylinder 2 6))
                  (shape end-frame (path-frame rail :at end))
                  (result (place end-frame peg :offset (0 0 -3) :rotate (0 0 45))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepAdaptor_Curve"), "{source}");
        assert!(source.contains("gp_Trsf"), "{source}");
        assert!(source.contains("SetValues("), "{source}");
        assert!(source.contains("BRepBuilderAPI_Transform"), "{source}");
        assert!(
            source.contains("SetTranslation(gp_Vec(0, 0, -3))"),
            "{source}"
        );
        assert!(
            source.contains(
                "SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), 0.7853981633974483"
            ),
            "{source}"
        );
    }

    #[test]
    fn emits_plane_location_place_clip_box_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape base (plane :origin (0 0 4) :normal (0 0 1)))
                  (shape loc (location base :offset (5 0 0) :rotate (0 0 90)))
                  (shape peg (box 2 4 6))
                  (shape placed (place loc peg))
                  (result
                    (clip-box placed :x (0 10) :y (-5 5) :z (0 12))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("SetValues("), "{source}");
        assert!(
            source.contains("SetTranslation(gp_Vec(5, 0, 0))"),
            "{source}"
        );
        assert!(
            source.contains(
                "SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), 1.5707963267948966"
            ),
            "{source}"
        );
        assert!(source.contains("BRepPrimAPI_MakeBox(gp_Pnt(0"), "{source}");
        assert!(source.contains("BRepAlgoAPI_Common"), "{source}");
    }

    #[test]
    fn emits_array_ops_as_native_occt_compounds() {
        let program = compile(
            r#"
            (model
              (part body
                (compound
                  (linear-array 3 10 0 0 (box 2 2 2))
                  (radial-array 4 90 20 (cylinder 2 5))
                  (grid-array 2 3 8 9 (sphere 2))
                  (arc-array 5 30 0 180 (cone 2 1 4)))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("_linear_array_builder"), "{source}");
        assert!(source.contains("SetTranslation(gp_Vec(10"), "{source}");
        assert!(source.contains("_radial_array_builder"), "{source}");
        assert!(source.contains("1.5707963267948966"), "{source}");
        assert!(source.contains("_grid_array_builder"), "{source}");
        assert!(source.contains("gp_Vec(16, 9, 0)"), "{source}");
        assert!(source.contains("_arc_array_builder"), "{source}");
        assert!(source.contains("3.141592653589793"), "{source}");
    }

    #[test]
    fn emits_rotate_and_scale_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (union
                  (rotate 90 0 45 (box 1 2 3))
                  (translate 10 0 0
                    (scale 1 2 3 (box 2 2 2))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(
            source.contains(
                "SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0)), 1.5707963267948966"
            ),
            "{source}"
        );
        assert!(
            source.contains(
                "SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), 0.7853981633974483"
            ),
            "{source}"
        );
        assert!(source.contains("BRepBuilderAPI_GTransform"), "{source}");
        assert!(source.contains("SetValue(1, 1, 1"), "{source}");
        assert!(source.contains("SetValue(2, 2, 2"), "{source}");
        assert!(source.contains("SetValue(3, 3, 3"), "{source}");
    }

    #[test]
    fn emits_mirror_taper_and_offset_rounded_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (compound
                  (mirror "y" 2 (box 4 5 6))
                  (translate 14 0 0
                    (taper 12 0.55 0.8 (rounded-rect 8 6 1)))
                  (translate 28 0 0
                    (extrude (offset-rounded 1.5 (circle 5)) 4)))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(
            source.contains("SetMirror(gp_Ax2(gp_Pnt(0, 2, 0)"),
            "{source}"
        );
        assert!(source.contains("gp_Dir(0, 1, 0)"), "{source}");
        assert!(source.contains("BRepOffsetAPI_ThruSections"), "{source}");
        assert!(source.contains("BRepBuilderAPI_GTransform"), "{source}");
        assert!(source.contains("SetValue(1, 1, 0.55"), "{source}");
        assert!(source.contains("SetValue(2, 2, 0.8"), "{source}");
        assert!(source.contains("BRepOffsetAPI_MakeOffset"), "{source}");
        assert!(source.contains(".Perform(1.5"), "{source}");
    }

    #[test]
    fn emits_fillet_and_chamfer_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (union
                  (fillet 0.5 (box 10 10 10))
                  (translate 20 0 0
                    (chamfer 0.75 (box 10 10 10))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepFilletAPI_MakeFillet"), "{source}");
        assert!(source.contains("BRepFilletAPI_MakeChamfer"), "{source}");
        assert!(source.contains("TopExp_Explorer"), "{source}");
        assert!(source.contains("TopAbs_EDGE"), "{source}");
        assert!(source.contains("TopoDS::Edge"), "{source}");
        assert!(source.contains(".Add(0.5"), "{source}");
        assert!(source.contains(".Add(0.75"), "{source}");
    }

    #[test]
    fn emits_exact_target_id_selector_for_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "target-id:body:edge:0:0-0-0_0-0-10"
                  (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(
            source.contains("std::vector<std::string>") && source.contains("_target_ids"),
            "{source}"
        );
        assert!(source.contains("\"body:edge:0:0-0-0_0-0-10\""), "{source}");
        assert!(
            source.contains("direct_occt_edge_target_id(\"body\""),
            "{source}"
        );
        assert!(
            source.contains(
                "Direct OCCT edge selector target ids did not match current topology for part `body`."
            ),
            "{source}"
        );
    }

    #[test]
    fn emits_stable_edge_target_id_selector_for_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "target-id:body:edge:0-0-0_0-0-10"
                  (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("\"body:edge:0-0-0_0-0-10\""), "{source}");
        assert!(
            source.contains("direct_occt_stable_edge_target_id"),
            "{source}"
        );
    }

    #[test]
    fn emits_durable_edge_target_id_selector_for_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "target-id:body:node:42:edge:0:0-0-0_0-0-10"
                  (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(
            source.contains("\"body:node:42:edge:0:0-0-0_0-0-10\""),
            "{source}"
        );
        assert!(
            source.contains("direct_occt_stable_edge_target_id"),
            "{source}"
        );
    }

    #[test]
    fn emits_exact_target_id_selector_for_native_occt_source_from_payload_when_value_is_bad() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "target-id:body:edge:0:0-0-0_0-0-10"
                  (box 10 10 10))))
            "#,
        );
        let mut plan =
            crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let fillet = plan.parts[0]
            .commands
            .iter_mut()
            .find(|command| command.op == OcctOp::Fillet)
            .expect("fillet");
        *fillet.keywords[0].source_arg_mut() = OcctArg::Number(7.0);

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("\"body:edge:0:0-0-0_0-0-10\""), "{source}");
    }

    #[test]
    fn rejects_exact_edge_selector_without_payload_even_if_text_present() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5
                  :edges "target-id:body:edge:0:0-0-0_0-0-10"
                  (box 10 10 10))))
            "#,
        );
        let mut plan =
            crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let fillet = plan.parts[0]
            .commands
            .iter_mut()
            .find(|command| command.op == OcctOp::Fillet)
            .expect("fillet");
        fillet.keywords[0].set_selector_payload(None);

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("missing payload should fail");

        assert!(
            err.message.contains("requires typed selector payload"),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn emits_exact_face_target_id_selector_for_native_occt_shell_source() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "target-id:body:face:0:0-0-10:400"
                  (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(
            source.contains("std::vector<std::string>") && source.contains("_target_ids"),
            "{source}"
        );
        assert!(source.contains("\"body:face:0:0-0-10:400\""), "{source}");
        assert!(
            source.contains("direct_occt_face_target_id(\"body\""),
            "{source}"
        );
        assert!(source.contains("BRepOffsetAPI_MakeThickSolid"), "{source}");
        assert!(
            source.contains(
                "Direct OCCT shell face selector target ids did not match current topology for part `body`."
            ),
            "{source}"
        );
    }

    #[test]
    fn emits_stable_face_target_id_selector_for_native_occt_shell_source() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "target-id:body:face:0-0-10:400"
                  (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("\"body:face:0-0-10:400\""), "{source}");
        assert!(
            source.contains("direct_occt_stable_face_target_id"),
            "{source}"
        );
    }

    #[test]
    fn emits_durable_face_target_id_selector_for_native_occt_shell_source() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "target-id:body:node:42:face:0:0-0-10:400"
                  (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(
            source.contains("\"body:node:42:face:0:0-0-10:400\""),
            "{source}"
        );
        assert!(
            source.contains("direct_occt_stable_face_target_id"),
            "{source}"
        );
    }

    #[test]
    fn emits_exact_face_target_id_selector_for_native_occt_shell_source_from_payload_when_value_is_bad(
    ) {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "target-id:body:face:0:0-0-10:400"
                  (box 10 10 10))))
            "#,
        );
        let mut plan =
            crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let shell = plan.parts[0]
            .commands
            .iter_mut()
            .find(|command| command.op == OcctOp::Shell)
            .expect("shell");
        *shell.keywords[0].source_arg_mut() = OcctArg::Number(7.0);

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("\"body:face:0:0-0-10:400\""), "{source}");
    }

    #[test]
    fn rejects_exact_face_selector_without_payload_even_if_text_present() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8
                  :faces "target-id:body:face:0:0-0-10:400"
                  (box 10 10 10))))
            "#,
        );
        let mut plan =
            crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let shell = plan.parts[0]
            .commands
            .iter_mut()
            .find(|command| command.op == OcctOp::Shell)
            .expect("shell");
        shell.keywords[0].set_selector_payload(None);

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("missing payload should fail");

        assert!(
            err.message.contains("requires typed selector payload"),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn emits_coarse_edge_selector_for_native_occt_exact_path() {
        let program = compile(
            r#"
            (model
                (part body
                (fillet 0.5 :edges "top" (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("coarse selector should emit");

        assert!(source.contains("edge_tol"), "{source}");
        assert!(source.contains("edge_matches"), "{source}");
    }

    #[test]
    fn emits_coarse_edge_selector_payload_for_native_occt_exact_path_when_value_is_bad() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5 :edges "top" (box 10 10 10))))
            "#,
        );
        let mut plan =
            crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let fillet = plan.parts[0]
            .commands
            .iter_mut()
            .find(|command| command.op == OcctOp::Fillet)
            .expect("fillet");
        *fillet.keywords[0].source_arg_mut() = OcctArg::Number(7.0);

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("coarse selector should emit from payload");

        assert!(source.contains("edge_tol"), "{source}");
        assert!(source.contains("edge_matches"), "{source}");
    }

    #[test]
    fn emits_created_by_filter_for_native_occt_edge_selector_source() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape blank (box 10 10 10))
                  (shape pocket (box 4 4 4))
                  (shape solid (difference blank pocket))
                  (result
                    (fillet 0.5
                      :edges "left+vertical"
                      :created-by pocket
                      solid)))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("created_by_edge_map"), "{source}");
        assert!(source.contains("created_by_edge_matches"), "{source}");
        assert!(source.contains("TopExp::MapShapes(shape_"), "{source}");
        assert!(source.contains("TopAbs_EDGE"), "{source}");
        assert!(source.contains(".IsSame("), "{source}");
    }

    #[test]
    fn rejects_unknown_edge_selector_with_shared_selector_help() {
        let err = crate::ecky_scheme::compile_to_core_program(
            r#"
            (model
              (part body
                (fillet 0.5 :edges "side" (box 10 10 10))))
            "#,
        )
        .expect_err("unknown selector should fail");

        assert!(
            err.message.contains("expected selector payload"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn rejects_face_target_id_in_native_occt_edge_selector() {
        let program = compile(
            r#"
            (model
              (part body
                (fillet 0.5 :edges "target-id:body:edge:0:0-0-0_10-0-0" (box 10 10 10))))
            "#,
        );
        let mut plan =
            crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let fillet = plan.parts[0]
            .commands
            .iter_mut()
            .find(|command| command.op == OcctOp::Fillet)
            .expect("fillet");
        fillet.keywords[0].set_selector_payload(Some(CoreSelectorPayload::FaceTargetIds(vec![
            "body:face:0:0-0-10:100".to_string(),
        ])));

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("wrong target kind should fail");

        assert!(
            err.message
                .contains("got face selector payload [\"body:face:0:0-0-10:100\"]"),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn emits_coarse_face_selector_for_native_occt_shell_source() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8 :faces "top" (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("coarse face selector should emit");
        assert!(
            source.contains("Bnd_Box")
                && source.contains("TopTools_ListOfShape")
                && source.contains("BRepBndLib::Add")
                && source.contains("std::abs"),
            "unexpected source: {}",
            source
        );
    }

    #[test]
    fn emits_richer_face_selector_for_native_occt_shell_source() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8 :faces "planar+normal-z+area-max" (box 10 10 10))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("richer face selector should emit");
        assert!(
            source.contains("BRepGProp::SurfaceProperties")
                && source.contains("GeomAbs_Plane")
                && source.contains("std::max")
                && source.contains("std::abs"),
            "unexpected source: {}",
            source
        );
    }

    #[test]
    fn emits_created_by_filter_for_native_occt_shell_selector_source() {
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape blank (box 10 10 10))
                  (shape pocket (box 4 4 4))
                  (shape solid (difference blank pocket))
                  (result
                    (shell 0.8
                      :faces "planar+normal-z+area-max"
                      :created-by pocket
                      solid)))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("created_by_face_map"), "{source}");
        assert!(source.contains("created_by_face_matches"), "{source}");
        assert!(source.contains("TopExp::MapShapes(shape_"), "{source}");
        assert!(source.contains("TopAbs_FACE"), "{source}");
        assert!(source.contains(".IsSame("), "{source}");
    }

    #[test]
    fn rejects_edge_target_id_in_native_occt_face_selector() {
        let program = compile(
            r#"
            (model
              (part body
                (shell 0.8 :faces "target-id:body:face:0:0-0-10:100" (box 10 10 10))))
            "#,
        );
        let mut plan =
            crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let shell = plan.parts[0]
            .commands
            .iter_mut()
            .find(|command| command.op == OcctOp::Shell)
            .expect("shell");
        shell.keywords[0].set_selector_payload(Some(CoreSelectorPayload::EdgeTargetIds(vec![
            "body:edge:0:0-0-0_10-0-0".to_string(),
        ])));

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("wrong target kind should fail");

        assert!(
            err.message
                .contains("got edge selector payload [\"body:edge:0:0-0-0_10-0-0\"]"),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn emits_shell_as_native_occt_source() {
        let program = compile("(model (part body (shell 0.8 (box 10 10 10))))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepOffsetAPI_MakeThickSolid"), "{source}");
        assert!(source.contains("MakeThickSolidByJoin"), "{source}");
        assert!(source.contains("BRepOffsetAPI_MakeOffsetShape"), "{source}");
        assert!(source.contains("TopAbs_FACE"), "{source}");
        assert!(source.contains("BRepAdaptor_Surface"), "{source}");
        assert!(source.contains("GeomAbs_Plane"), "{source}");
        assert!(source.contains("-0.8"), "{source}");
    }

    #[test]
    fn emits_supported_solid_ops_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (intersection
                  (union
                    (sphere 6)
                    (translate 10 0 0 (cylinder 3 14))
                    (translate -10 0 0 (cone 5 2 16))
                    (compound
                      (box 2 3 4)
                      (translate 0 4 0 (sphere 1))))
                  (difference
                    (box 30 30 30)
                    (translate 5 0 0 (cylinder 2 40))))))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let source = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepPrimAPI_MakeSphere(6"), "{source}");
        assert!(
            source.contains("BRepPrimAPI_MakeCylinder(3, 14"),
            "{source}"
        );
        assert!(source.contains("BRepPrimAPI_MakeCone(5, 2, 16"), "{source}");
        assert!(
            source.contains("SetTranslation(gp_Vec(10, 0, 0))"),
            "{source}"
        );
        assert!(source.contains("BRep_Builder"), "{source}");
        assert!(source.contains("MakeCompound"), "{source}");
        assert!(source.contains("BRepAlgoAPI_Fuse"), "{source}");
        assert!(source.contains("BRepAlgoAPI_Cut"), "{source}");
        assert!(source.contains("BRepAlgoAPI_Common"), "{source}");
    }

    #[test]
    fn emits_runtime_parameters_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (params (number width 10))
              (part body (box width 1 1)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let params = DesignParams::from([("width".to_string(), ParamValue::Number(42.0))]);

        let source = emit_plan_export_source_with_params(
            &plan,
            &params,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepPrimAPI_MakeBox(42"), "{source}");
    }

    #[test]
    fn fills_core_parameter_defaults_for_native_occt_source() {
        let program = compile(
            r#"
            (model
              (params (number width 10))
              (part body (box width 1 1)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");
        let params = effective_program_parameters(&program, &DesignParams::new());

        let source = emit_plan_export_source_with_params(
            &plan,
            &params,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect("source");

        assert!(source.contains("BRepPrimAPI_MakeBox(10"), "{source}");
    }

    #[test]
    fn reports_missing_runtime_parameters_by_name() {
        let program = compile(
            r#"
            (model
              (params (number width 10))
              (part body (box width 1 1)))
            "#,
        );
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("missing runtime parameter");

        let message = err.to_string();
        assert!(message.contains("width"), "{message}");
        assert!(message.contains("missing runtime parameter"), "{message}");
    }

    #[test]
    fn live_executor_exports_core_ir_box_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile("(model (part body (box 10 20 30)))");

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-exec"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT box export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_supported_solid_ops_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (intersection
                  (union
                    (box 20 20 20)
                    (translate 12 0 0 (sphere 8))
                    (translate -12 0 0 (cone 5 2 16))
                    (compound
                      (translate 0 12 0 (cylinder 3 18))
                      (translate 0 -12 0 (sphere 3))))
                  (difference
                    (box 40 40 40)
                    (translate 10 0 0 (cylinder 4 50))))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-solid-ops"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT solid ops export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_multi_part_compound_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part base (box 20 14 3))
              (part post (translate 0 0 3 (cylinder 3 12))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-multipart"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT multipart export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_extruded_sketches_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (union
                  (extrude (circle 10) 8)
                  (translate 24 0 0 (extrude (rectangle 10 14) 5))
                  (translate 48 0 0 (extrude (rounded-rect 14 10 2) 4))
                  (translate -24 0 0
                    (extrude (polygon ((0 0) (10 0) (6 8) (0 6))) 4))
                  (translate -48 0 0
                    (extrude (rounded-polygon ((0 0) (12 0) (9 8) (2 7)) 1.5) 4)))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-extrude"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT extrude export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_profile_holes_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (profile :outer (circle 12) :holes (circle 4))
                  5)))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-profile-hole"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT profile-hole export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_multi_outer_profile_holes_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (profile
                    :outer ((polygon ((0 0) (24 0) (24 24) (0 24)))
                            (translate 40 0 0 (polygon ((0 0) (12 0) (12 12) (0 12)))))
                    :holes ((polygon ((8 8) (16 8) (16 16) (8 16)))))
                  5)))
            "#,
        );

        let outcome = export_core_program_step_stl(
            &program,
            &layout,
            temp_root("direct-occt-profile-multi-outer-hole"),
        )
        .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT multi-outer profile export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_runner_first_exports_multi_outer_profile_holes_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let Some(runner) =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &TestResolver,
                true,
            )
        else {
            return;
        };
        if !runner.is_file() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (profile
                    :outer ((polygon ((0 0) (24 0) (24 24) (0 24)))
                            (translate 40 0 0 (polygon ((0 0) (12 0) (12 12) (0 12)))))
                    :holes ((polygon ((8 8) (16 8) (16 16) (8 16)))))
                  5)))
            "#,
        );
        let output_dir = temp_root("direct-occt-runner-profile-multi-outer-hole");

        let outcome = export_core_program_step_stl_with_params_runner_first(
            &program,
            &DesignParams::new(),
            &layout,
            &output_dir,
            &TestResolver,
        )
        .expect("export");

        let NativeExportOutcome::Exported {
            step_path,
            stl_path,
        ..
        } = outcome
        else {
            panic!("expected direct OCCT runner-first export");
        };
        assert!(
            output_dir.join("plan.json").is_file(),
            "missing runner plan"
        );
        assert!(
            output_dir.join("topology.json").is_file(),
            "missing topology"
        );
        assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
        assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
        assert!(
            std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
            "STEP export too small"
        );
        assert!(
            std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
            "STL export too small"
        );
    }

    #[test]
    fn live_runner_first_exports_text_profile_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let Some(runner) =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &TestResolver,
                true,
            )
        else {
            return;
        };
        if !runner.is_file() {
            return;
        }
        let Some(font_path) = text_font_fixture() else {
            return;
        };
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(r#"(model (part body (extrude (text "II" 12) 4)))"#);
        let output_dir = temp_root("direct-occt-runner-text-profile");
        let previous_font = std::env::var_os("ECKYCAD_FONT_PATH");
        unsafe {
            std::env::set_var("ECKYCAD_FONT_PATH", font_path);
        }
        let outcome = export_core_program_step_stl_with_params_runner_first(
            &program,
            &DesignParams::new(),
            &layout,
            &output_dir,
            &TestResolver,
        );
        match previous_font {
            Some(previous_font) => unsafe {
                std::env::set_var("ECKYCAD_FONT_PATH", previous_font);
            },
            None => unsafe {
                std::env::remove_var("ECKYCAD_FONT_PATH");
            },
        }
        let outcome = outcome.expect("export");

        let NativeExportOutcome::Exported {
            step_path,
            stl_path,
        ..
        } = outcome
        else {
            panic!("expected direct OCCT runner-first text export");
        };
        assert!(
            output_dir.join("plan.json").is_file(),
            "missing runner plan"
        );
        assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
        assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
    }

    #[test]
    fn live_runner_first_exports_import_stl_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let Some(runner) =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &TestResolver,
                true,
            )
        else {
            return;
        };
        if !runner.is_file() {
            return;
        }
        let stl_path = temp_root("direct-occt-import-stl-fixture").with_extension("stl");
        write_ascii_stl_fixture(&stl_path);
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(&format!(
            "(model (part body (import-stl {:?})))",
            stl_path.to_string_lossy()
        ));
        let output_dir = temp_root("direct-occt-runner-import-stl");

        let outcome = export_core_program_step_stl_with_params_runner_first(
            &program,
            &DesignParams::new(),
            &layout,
            &output_dir,
            &TestResolver,
        )
        .expect("export");

        let NativeExportOutcome::Exported {
            step_path,
            stl_path: preview_stl_path,
        ..
        } = outcome
        else {
            panic!("expected direct OCCT runner-first import-stl export");
        };
        assert!(
            output_dir.join("plan.json").is_file(),
            "missing runner plan"
        );
        assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
        assert!(
            preview_stl_path.is_file(),
            "missing STL export: {preview_stl_path:?}"
        );
        let _ = std::fs::remove_file(stl_path);
    }

    #[test]
    fn live_runner_first_exports_helical_ridge_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let Some(runner) =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &TestResolver,
                true,
            )
        else {
            return;
        };
        if !runner.is_file() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (helical-ridge
                  :radius 20
                  :pitch 6
                  :height 30
                  :base-width 2
                  :crest-width 1
                  :depth 1.5)))
            "#,
        );
        let output_dir = temp_root("direct-occt-runner-helical-ridge");

        let outcome = export_core_program_step_stl_with_params_runner_first(
            &program,
            &DesignParams::new(),
            &layout,
            &output_dir,
            &TestResolver,
        )
        .expect("export");

        let NativeExportOutcome::Exported {
            step_path,
            stl_path,
        ..
        } = outcome
        else {
            panic!("expected direct OCCT runner-first helical-ridge export");
        };
        assert!(
            output_dir.join("plan.json").is_file(),
            "missing runner plan"
        );
        assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
        assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
    }

    // Regression: a `clip-box` over a `helical-ridge` used to collapse to an
    // empty shape on the native runner (BRepAlgoAPI_Common silently emptied the
    // faceted swept solid), so threads silently vanished from any part that
    // clipped them before a fuse/difference. The Cut-based clip must keep the
    // thread: the fused result has far more facets than the bare cylinder.
    #[test]
    fn live_runner_first_keeps_clipped_helical_ridge_thread_when_runner_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let Some(runner) =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &TestResolver,
                true,
            )
        else {
            return;
        };
        if !runner.is_file() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part carrier
                (build
                  (shape body (cylinder 31.8 20 96))
                  (shape thread_raw
                    (translate 0 0 -0.8
                      (helical-ridge
                        :radius 31.5 :pitch 6.25 :height 21.6
                        :base-width 1.45 :crest-width 0.84 :depth 1.42)))
                  (shape thread
                    (clip-box thread_raw :x (-35 35) :y (-35 35) :z (0 20)))
                  (result (fuse body thread)))))
            "#,
        );
        let output_dir = temp_root("direct-occt-runner-clipped-helical-ridge");

        let outcome = export_core_program_step_stl_with_params_runner_first(
            &program,
            &DesignParams::new(),
            &layout,
            &output_dir,
            &TestResolver,
        )
        .expect("export");

        let NativeExportOutcome::Exported { stl_path, .. } = outcome else {
            panic!("expected direct OCCT runner-first clipped helical-ridge export");
        };
        let stl_bytes = std::fs::read(&stl_path).expect("read stl");
        // Binary STL: triangle count is at bytes 80-84 (u32 LE).
        assert!(stl_bytes.len() >= 84);
        let facets = u32::from_le_bytes([stl_bytes[80], stl_bytes[81], stl_bytes[82], stl_bytes[83]]) as usize;
        // A bare r=31.8 96-segment cylinder is ~380 facets. With the clipped
        // thread fused on it the count is far higher; if the clip emptied the
        // thread we would fall back to roughly the bare cylinder.
        assert!(
            facets > 1000,
            "clipped helical-ridge thread missing from native export: only {facets} facets"
        );
    }

    #[test]
    fn live_executor_exports_svg_profile_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let svg_path = std::env::temp_dir().join(format!(
            "ecky-direct-occt-live-svg-profile-{}.svg",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &svg_path,
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path fill="#000" d="M 1 1 L 9 1 L 9 9 L 1 9 Z"/></svg>"##,
        )
        .expect("write svg");
        let program = compile(&format!(
            r#"(model (part body (extrude (svg "{}" 10 10 "contain") 4)))"#,
            svg_path.display()
        ));

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-svg-profile"))
                .expect("export");
        let _ = std::fs::remove_file(&svg_path);

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT SVG profile export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_bspline_profile_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (bspline ((0 6) (5 2) (6 -4) (0 -6) (-6 -4) (-5 2)) #t)
                  4)))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-bspline"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT bspline export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_offset_sketch_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (extrude
                  (offset 2 (rounded-rect 16 10 1.5))
                  4)))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-offset"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT offset export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_path_frame_place_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape rail (path ((0 0 0) (6 0 8) (0 0 18))))
                  (shape peg (cylinder 2 6))
                  (shape end-frame (path-frame rail :at end))
                  (result (place end-frame peg :offset (0 0 -3))))))
            "#,
        );

        let outcome = export_core_program_step_stl(
            &program,
            &layout,
            temp_root("direct-occt-path-frame-place"),
        )
        .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT path-frame/place export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_plane_location_clip_box_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (build
                  (shape base (plane :origin (0 0 4) :normal (0 0 1)))
                  (shape loc (location base :offset (5 0 0) :rotate (0 0 90)))
                  (shape peg (box 2 4 6))
                  (shape placed (place loc peg))
                  (result
                    (clip-box placed :x (0 10) :y (-5 5) :z (0 12))))))
            "#,
        );

        let outcome = export_core_program_step_stl(
            &program,
            &layout,
            temp_root("direct-occt-plane-location-clip"),
        )
        .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT plane/location/clip export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_array_ops_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (compound
                  (linear-array 3 8 0 0 (box 2 2 2))
                  (radial-array 4 90 14 (cylinder 1.5 4))
                  (grid-array 2 2 6 6 (sphere 1.5))
                  (arc-array 4 22 0 180 (cone 2 1 4)))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-arrays"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT array export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_revolved_sketch_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (revolve
                  (polygon ((8 0) (12 0) (12 20) (8 20)))
                  360)))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-revolve"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT revolve export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_loft_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (loft 30
                  (circle 10)
                  (rounded-rect 14 10 2)
                  (circle 4))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-loft"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT loft export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_sweep_path_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (sweep
                  (circle 4)
                  (path ((0 0 0) (8 0 10) (0 0 24))))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-sweep"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT sweep export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_bezier_sweep_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (sweep
                  (circle 2)
                  (bezier-path ((0 0 0) (8 0 0) (8 8 14) (16 8 14))))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-bezier-sweep"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT bezier sweep export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_twist_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (twist 32 120
                  (rounded-rect 12 8 1.5))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-twist"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT twist export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_mirror_taper_and_offset_rounded_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (compound
                  (mirror "x" 0 (box 4 5 6))
                  (translate 14 0 0
                    (taper 12 0.55 0.8 (rounded-rect 8 6 1)))
                  (translate 28 0 0
                    (extrude (offset-rounded 1.5 (circle 5)) 4)))))
            "#,
        );

        let outcome = export_core_program_step_stl(
            &program,
            &layout,
            temp_root("direct-occt-mirror-taper-offset-rounded"),
        )
        .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT mirror/taper/offset-rounded export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_rotated_scaled_solid_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (union
                  (rotate 90 0 45 (box 1 2 3))
                  (translate 10 0 0
                    (scale 1 2 3 (box 2 2 2))))))
            "#,
        );

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-transform"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT transform export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_fillet_chamfer_solid_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (union
                  (fillet 0.5 (box 10 10 10))
                  (translate 20 0 0
                    (chamfer 0.75 (box 10 10 10))))))
            "#,
        );

        let outcome = export_core_program_step_stl(
            &program,
            &layout,
            temp_root("direct-occt-fillet-chamfer"),
        )
        .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT fillet/chamfer export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_shell_solid_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile("(model (part body (shell 0.8 (box 10 10 10))))");

        let outcome =
            export_core_program_step_stl(&program, &layout, temp_root("direct-occt-shell"))
                .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT shell export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_sampled_radial_loft_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (sampled-radial-loft
                  (theta z fz)
                  :height 40
                  :z-steps 6
                  :theta-steps 24
                  :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                  :z-map (+ z (* fz 2)))))
            "#,
        );

        let outcome = export_core_program_step_stl(
            &program,
            &layout,
            temp_root("direct-occt-sampled-radial-loft"),
        )
        .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT sampled radial loft export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    #[test]
    fn live_executor_exports_shell_sampled_radial_loft_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        let program = compile(
            r#"
            (model
              (part body
                (shell 2
                  (sampled-radial-loft
                    (theta z fz)
                    :height 40
                    :z-steps 6
                    :theta-steps 24
                    :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                    :z-map (+ z (* fz 2))))))
            "#,
        );

        let outcome = export_core_program_step_stl(
            &program,
            &layout,
            temp_root("direct-occt-shell-sampled-radial-loft"),
        )
        .expect("export");

        if layout.can_compile_native_shim() {
            let NativeExportOutcome::Exported {
                step_path,
                stl_path,
            ..
            } = outcome
            else {
                panic!("expected direct OCCT shell sampled radial loft export");
            };
            assert!(step_path.is_file(), "missing STEP export: {step_path:?}");
            assert!(stl_path.is_file(), "missing STL export: {stl_path:?}");
            assert!(
                std::fs::metadata(&step_path).expect("step metadata").len() > 1024,
                "STEP export too small"
            );
            assert!(
                std::fs::metadata(&stl_path).expect("stl metadata").len() > 512,
                "STL export too small"
            );
        } else {
            let NativeExportOutcome::Blocked { blockers } = outcome else {
                panic!("expected blocked outcome without complete SDK");
            };
            assert!(!blockers.is_empty());
        }
    }

    // ===== Multipart native export regression tests =====
    //
    // The native direct-occt backend used to write a single ASCII `preview.stl`
    // for the whole model and point every part's viewerAsset at it. That broke
    // multipart export: the zip duplicated one merged mesh, and 3MF export
    // choked on ASCII bytes read as a triangle count ("failed to fill whole
    // buffer"). These tests pin the corrected behavior: binary STL + per-part
    // files with distinct geometry.

    #[test]
    fn live_native_preview_stl_is_binary_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let Some(runner) =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &TestResolver,
                true,
            )
        else {
            return;
        };
        if !runner.is_file() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        if !layout.can_compile_native_shim() {
            return;
        }
        let program = compile("(model (part body (box 10 20 30)))");
        let output_dir = temp_root("direct-occt-binary-stl");
        let outcome = export_core_program_step_stl_with_params_runner_first(
            &program,
            &DesignParams::new(),
            &layout,
            &output_dir,
            &TestResolver,
        )
        .expect("export");
        let NativeExportOutcome::Exported { stl_path, .. } = outcome else {
            panic!("expected exported outcome");
        };
        let bytes = std::fs::read(&stl_path).expect("read stl");
        assert!(
            !bytes.starts_with(b"solid"),
            "native preview STL must be BINARY, not ASCII. First 20 bytes: {:?}",
            String::from_utf8_lossy(&bytes[..20.min(bytes.len())])
        );
        // Binary STL: 80-byte header + 4-byte triangle count + 50 bytes per triangle.
        assert!(
            bytes.len() >= 84,
            "binary STL too small: {} bytes",
            bytes.len()
        );
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        assert_eq!(
            bytes.len(),
            84 + count * 50,
            "binary STL length mismatch: header says {} triangles, file is {} bytes",
            count,
            bytes.len()
        );
        assert!(count > 0, "binary STL has zero triangles");
    }

    #[test]
    fn live_native_multi_part_writes_distinct_per_part_binary_stl_when_runtime_ready() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root = bundled_build123d_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let Some(runner) =
            crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(
                &TestResolver,
                true,
            )
        else {
            return;
        };
        if !runner.is_file() {
            return;
        }
        let layout = inspect_build123d_ocp_runtime(&runtime_root);
        if !layout.can_compile_native_shim() {
            return;
        }
        // Two parts with completely different geometry and bounding boxes.
        let program = compile(
            r#"
            (model
              (part base (box 100 100 5))
              (part peg (translate 50 50 5 (cylinder 10 20))))
            "#,
        );
        let output_dir = temp_root("direct-occt-per-part-stl");
        let outcome = export_core_program_step_stl_with_params_runner_first(
            &program,
            &DesignParams::new(),
            &layout,
            &output_dir,
            &TestResolver,
        )
        .expect("export");
        let NativeExportOutcome::Exported {
            stl_path,
            part_stl_paths,
            ..
        } = outcome
        else {
            panic!("expected exported outcome");
        };
        // Merged preview must still exist and be binary.
        assert!(stl_path.is_file(), "missing merged preview STL: {stl_path:?}");
        // Must have exactly 2 per-part STL files.
        assert_eq!(
            part_stl_paths.len(),
            2,
            "expected 2 per-part STL paths, got {:?}",
            part_stl_paths
        );
        let (key0, path0) = &part_stl_paths[0];
        let (key1, path1) = &part_stl_paths[1];
        assert_eq!(key0, "base");
        assert_eq!(key1, "peg");
        assert!(path0.is_file(), "missing part STL: {path0:?}");
        assert!(path1.is_file(), "missing part STL: {path1:?}");
        assert_ne!(
            path0, path1,
            "both part STLs must point to DISTINCT files"
        );

        // Each per-part file must be a valid binary STL.
        for (key, path) in &part_stl_paths {
            let bytes = std::fs::read(path).expect("read part stl");
            assert!(
                !bytes.starts_with(b"solid"),
                "part '{key}' STL must be BINARY, not ASCII"
            );
            assert!(bytes.len() >= 84, "part '{key}' STL too small");
            let count =
                u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
            assert_eq!(
                bytes.len(),
                84 + count * 50,
                "part '{key}' STL length mismatch"
            );
        }

        // The two parts MUST have distinct triangle counts (base slab vs peg
        // cylinder — completely different tessellation densities).
        let bytes0 = std::fs::read(&path0).unwrap();
        let bytes1 = std::fs::read(&path1).unwrap();
        let count0 = u32::from_le_bytes([bytes0[80], bytes0[81], bytes0[82], bytes0[83]]);
        let count1 = u32::from_le_bytes([bytes1[80], bytes1[81], bytes1[82], bytes1[83]]);
        assert_ne!(
            count0, count1,
            "base and peg must have distinct triangle counts, both were {count0}"
        );
    }
}
