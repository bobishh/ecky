use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::direct_occt::{OcctArg, OcctCommand, OcctOp, OcctParameterKind, OcctPlan, OcctSlot};
use super::direct_occt_sdk::{run_native_export_source, DirectOcctSdkLayout, NativeExportOutcome};
use crate::ecky_core_ir::{CoreParameterValue, CoreProgram};
use crate::models::{AppError, AppResult, DesignParams, ParamValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectOcctExport {
    pub step_path: PathBuf,
    pub stl_path: PathBuf,
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
    run_native_export_source(
        layout,
        output_dir,
        "direct_occt_executor.cpp",
        "direct_occt_executor",
        step_path,
        stl_path,
        source,
    )
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
            emit_command(&mut body, &mut vars, command)?;
        }

        let root_var = vars.get(&part.root).cloned().ok_or_else(|| {
            AppError::validation(format!(
                "Direct OCCT executor could not find root slot {:?} for part `{}`.",
                part.root, part.key
            ))
        })?;
        part_roots.push(root_var.clone());
        part_topology_roots.push((part.key.clone(), part.label.clone(), root_var));
    }

    let root_var = if part_roots.len() == 1 {
        part_roots.pop().expect("checked non-empty")
    } else {
        emit_top_level_compound(&mut body, &part_roots);
        "model_compound_shape".to_string()
    };
    let topology_path = step_path.with_file_name("topology.json");
    let topology_writer_source = direct_occt_topology_writer_source();
    let topology_writer_calls = direct_occt_topology_writer_calls(&part_topology_roots);

    Ok(format!(
        r#"#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
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
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRep_Builder.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepOffsetAPI_MakeOffsetShape.hxx>
#include <BRepOffsetAPI_MakeOffset.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepOffset_Mode.hxx>
#include <BRepTools.hxx>
#include <GeomAbs_JoinType.hxx>
#include <GeomAbs_SurfaceType.hxx>
#include <GProp_GProps.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <Geom_BezierCurve.hxx>
#include <Geom_BSplineCurve.hxx>
#include <GeomAPI_PointsToBSpline.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <STEPControl_Writer.hxx>
#include <StlAPI_Writer.hxx>
#include <TColgp_Array1OfPnt.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Wire.hxx>
#include <TopTools_ListOfShape.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Circ.hxx>
#include <gp_Dir.hxx>
#include <gp_GTrsf.hxx>
#include <gp_Pnt.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <algorithm>
#include <cmath>
#include <fstream>
#include <iomanip>
#include <sstream>
#include <string>
#include <vector>

{topology_writer_source}

int main() {{
{body}    TopoDS_Shape shape = {root_var};
    STEPControl_Writer step_writer;
    step_writer.Transfer(shape, STEPControl_AsIs);
    if (step_writer.Write("{}") != IFSelect_RetDone) {{
        return 2;
    }}
    std::ofstream topology_file("{}");
    if (!topology_file) {{
        return 4;
    }}
    topology_file << "{{\"parts\":[";
    bool first_topology_part = true;
{topology_writer_calls}    topology_file << "]}}\n";
    if (!topology_file.good()) {{
        return 4;
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
        topology_path.to_string_lossy(),
        stl_path.to_string_lossy()
    ))
}

fn direct_occt_topology_writer_calls(part_roots: &[(String, String, String)]) -> String {
    part_roots
        .iter()
        .map(|(key, label, root_var)| {
            let label = if label.trim().is_empty() { key } else { label };
            format!(
                "    write_part_faces(topology_file, {}, {}, {}, first_topology_part);\n",
                cpp_string_literal(key),
                cpp_string_literal(label),
                root_var
            )
        })
        .collect::<String>()
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

void write_part_faces(
    std::ofstream& out,
    const std::string& part_id,
    const std::string& part_label,
    const TopoDS_Shape& part_shape,
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
    int edge_index = 0;
    for (TopExp_Explorer explorer(part_shape, TopAbs_EDGE); explorer.More(); explorer.Next(), ++edge_index) {
        try {
            TopoDS_Edge edge = TopoDS::Edge(explorer.Current());
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
    command: &OcctCommand,
) -> AppResult<()> {
    let var = slot_var(command.output);
    if !command.keywords.is_empty()
        && !matches!(
            command.op,
            OcctOp::Profile
                | OcctOp::Plane
                | OcctOp::Location
                | OcctOp::PathFrame
                | OcctOp::Place
                | OcctOp::ClipBox
        )
    {
        return Err(AppError::validation(format!(
            "Direct OCCT executor does not support `{}` keyword arguments yet.",
            op_name(command.op)
        )));
    }
    match command.op {
        OcctOp::Box => {
            let [width, depth, height] = numeric_args(&command.args)?;
            body.push_str(&format!(
                "    TopoDS_Shape {var} = BRepPrimAPI_MakeBox({width}, {depth}, {height}).Shape();\n"
            ));
        }
        OcctOp::Sphere => {
            let radius = numeric_prefix_args::<1>(&command.args)?[0];
            body.push_str(&format!(
                "    TopoDS_Shape {var} = BRepPrimAPI_MakeSphere({radius}).Shape();\n"
            ));
        }
        OcctOp::Cylinder => {
            let [radius, height] = numeric_prefix_args::<2>(&command.args)?;
            body.push_str(&format!(
                "    TopoDS_Shape {var} = BRepPrimAPI_MakeCylinder({radius}, {height}).Shape();\n"
            ));
        }
        OcctOp::Cone => {
            let [radius1, radius2, height] = numeric_prefix_args::<3>(&command.args)?;
            body.push_str(&format!(
                "    TopoDS_Shape {var} = BRepPrimAPI_MakeCone({radius1}, {radius2}, {height}).Shape();\n"
            ));
        }
        OcctOp::Circle => {
            let [radius] = numeric_args(&command.args)?;
            body.push_str(&format!(
                "    gp_Circ {var}_circle(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), {radius});\n    TopoDS_Wire {var}_wire = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge({var}_circle).Edge()).Wire();\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n"
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
            emit_sweep_operation(body, &var, slot_var(profile), slot_var(path));
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
        OcctOp::Path => {
            let points = point3_sequence_args(&command.args)?;
            emit_path_wire(body, &var, &points)?;
        }
        OcctOp::BezierPath => {
            let points = point3_sequence_args(&command.args)?;
            emit_bezier_path_wire(body, &var, &points)?;
        }
        OcctOp::Bspline => {
            let points = point2_list_arg(&command.args, 0)?;
            emit_bspline_face(body, &var, &points)?;
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
            emit_edge_radius_operation(
                body,
                &var,
                "fillet",
                "BRepFilletAPI_MakeFillet",
                slot_var(input),
                radius,
            );
        }
        OcctOp::Chamfer => {
            let distance = positive_radius_arg(&command.args, 0, "chamfer")?;
            let input = ref_arg(&command.args, 1)?;
            emit_edge_radius_operation(
                body,
                &var,
                "chamfer",
                "BRepFilletAPI_MakeChamfer",
                slot_var(input),
                distance,
            );
        }
        OcctOp::Shell => {
            let thickness = positive_radius_arg(&command.args, 0, "shell")?;
            let input = ref_arg(&command.args, 1)?;
            emit_shell_operation(body, &var, slot_var(input), thickness);
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
                keyword.value = resolve_occt_arg(&keyword.value, &env)?;
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
                "outer" => outer.extend(ref_collection_arg(&keyword.value, "profile :outer")?),
                "holes" => holes.extend(ref_collection_arg(&keyword.value, "profile :holes")?),
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
            "origin" => origin = point3_like_arg(&keyword.value, "plane :origin")?,
            "x" => x_axis = point3_like_arg(&keyword.value, "plane :x")?,
            "normal" => normal = point3_like_arg(&keyword.value, "plane :normal")?,
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
            "offset" => offset = point3_like_arg(&keyword.value, "location :offset")?,
            "rotate" => rotate = point3_like_arg(&keyword.value, "location :rotate")?,
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
            "at" => at = path_frame_anchor_arg(&keyword.value)?,
            "up" => up = point3_like_arg(&keyword.value, "path-frame :up")?,
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
            "offset" => offset = point3_like_arg(&keyword.value, "place :offset")?,
            "rotate" => rotate = point3_like_arg(&keyword.value, "place :rotate")?,
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
            "x" => x = Some(axis_range_arg(&keyword.value, "clip-box :x")?),
            "y" => y = Some(axis_range_arg(&keyword.value, "clip-box :y")?),
            "z" => z = Some(axis_range_arg(&keyword.value, "clip-box :z")?),
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
    if points.len() < 3 {
        return Err(AppError::validation(
            "Direct OCCT executor `polygon` requires at least three points.",
        ));
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

fn emit_edge_radius_operation(
    body: &mut String,
    var: &str,
    label: &str,
    builder_type: &str,
    input_var: String,
    radius: f64,
) {
    body.push_str(&format!(
        "    {builder_type} {var}_{label}({input_var});\n    int {var}_edge_count = 0;\n    for (TopExp_Explorer {var}_edge_explorer({input_var}, TopAbs_EDGE); {var}_edge_explorer.More(); {var}_edge_explorer.Next()) {{\n        {var}_{label}.Add({radius}, TopoDS::Edge({var}_edge_explorer.Current()));\n        ++{var}_edge_count;\n    }}\n    if ({var}_edge_count == 0) {{ return 4; }}\n    TopoDS_Shape {var} = {var}_{label}.Shape();\n"
    ));
}

fn emit_shell_operation(body: &mut String, var: &str, input_var: String, thickness: f64) {
    let offset = -thickness.abs();
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

fn emit_bspline_face(body: &mut String, var: &str, points: &[[f64; 2]]) -> AppResult<()> {
    if points.len() < 3 {
        return Err(AppError::validation(
            "Direct OCCT executor `bspline` requires at least three 2D points.",
        ));
    }
    body.push_str(&format!(
        "    TColgp_Array1OfPnt {var}_poles(1, {});\n",
        points.len()
    ));
    for (index, [x, y]) in points.iter().enumerate() {
        let pole_index = index + 1;
        body.push_str(&format!(
            "    {var}_poles.SetValue({pole_index}, gp_Pnt({x}, {y}, 0));\n"
        ));
    }
    let first = points[0];
    let last = *points.last().expect("checked non-empty");
    body.push_str(&format!(
        "    GeomAPI_PointsToBSpline {var}_bspline_builder({var}_poles, 3, 8, GeomAbs_C2, 1.0e-4);\n    Handle(Geom_BSplineCurve) {var}_curve = {var}_bspline_builder.Curve();\n    BRepBuilderAPI_MakeWire {var}_wire_builder;\n    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge({var}_curve).Edge());\n"
    ));
    if distance2(first, last) > 1.0e-9 {
        body.push_str(&format!(
            "    {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({}, {}, 0), gp_Pnt({}, {}, 0)).Edge());\n",
            last[0], last[1], first[0], first[1]
        ));
    }
    body.push_str(&format!(
        "    TopoDS_Wire {var}_wire = {var}_wire_builder.Wire();\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n"
    ));
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
        "    std::vector<TopoDS_Edge> {var}_edges;\n    for (TopExp_Explorer {var}_edge_explorer({path_var}, TopAbs_EDGE); {var}_edge_explorer.More(); {var}_edge_explorer.Next()) {{\n        {var}_edges.push_back(TopoDS::Edge({var}_edge_explorer.Current()));\n    }}\n    if ({var}_edges.empty()) {{ return 17; }}\n    Standard_Real {var}_anchor = std::min(1.0, std::max(0.0, {at_literal}));\n    int {var}_edge_count = static_cast<int>({var}_edges.size());\n    int {var}_edge_index = static_cast<int>(std::floor({var}_anchor * {var}_edge_count));\n    Standard_Real {var}_local_t = {var}_anchor * {var}_edge_count - {var}_edge_index;\n    if ({var}_edge_index >= {var}_edge_count) {{\n        {var}_edge_index = {var}_edge_count - 1;\n        {var}_local_t = 1.0;\n    }}\n    TopoDS_Edge {var}_edge = {var}_edges[static_cast<std::size_t>({var}_edge_index)];\n    BRepAdaptor_Curve {var}_curve({var}_edge);\n    Standard_Real {var}_first = {var}_curve.FirstParameter();\n    Standard_Real {var}_last = {var}_curve.LastParameter();\n    Standard_Real {var}_param = {var}_first + ({var}_last - {var}_first) * {var}_local_t;\n    gp_Pnt {var}_origin;\n    gp_Vec {var}_derivative;\n    {var}_curve.D1({var}_param, {var}_origin, {var}_derivative);\n    if ({var}_derivative.Magnitude() <= 1.0e-9) {{ return 18; }}\n    gp_Vec {var}_z = {var}_derivative;\n    {var}_z.Normalize();\n    gp_Vec {var}_up({up_x}, {up_y}, {up_z});\n    if ({var}_up.Magnitude() <= 1.0e-9) {{ return 19; }}\n    gp_Vec {var}_x = {var}_up - {var}_z.Multiplied({var}_up.Dot({var}_z));\n    if ({var}_x.Magnitude() <= 1.0e-9) {{\n        gp_Vec {var}_fallback(0, 0, 1);\n        {var}_x = {var}_fallback - {var}_z.Multiplied({var}_fallback.Dot({var}_z));\n    }}\n    if ({var}_x.Magnitude() <= 1.0e-9) {{\n        gp_Vec {var}_fallback_y(0, 1, 0);\n        {var}_x = {var}_fallback_y - {var}_z.Multiplied({var}_fallback_y.Dot({var}_z));\n    }}\n    if ({var}_x.Magnitude() <= 1.0e-9) {{ return 20; }}\n    {var}_x.Normalize();\n    gp_Vec {var}_y = {var}_z.Crossed({var}_x);\n    if ({var}_y.Magnitude() <= 1.0e-9) {{ return 21; }}\n    {var}_y.Normalize();\n    {var}_x = {var}_y.Crossed({var}_z);\n    {var}_x.Normalize();\n    gp_Trsf {var};\n    {var}.SetValues(\n        {var}_x.X(), {var}_y.X(), {var}_z.X(), {var}_origin.X(),\n        {var}_x.Y(), {var}_y.Y(), {var}_z.Y(), {var}_origin.Y(),\n        {var}_x.Z(), {var}_y.Z(), {var}_z.Z(), {var}_origin.Z());\n"
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
    if profile.outer.len() != 1 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor `profile` expects exactly one outer loop, got {}.",
            profile.outer.len()
        )));
    }
    let outer_var = slot_var(profile.outer[0]);
    body.push_str(&format!(
        "    TopoDS_Wire {var}_outer_wire;\n    for (TopExp_Explorer {var}_outer_wire_explorer({outer_var}, TopAbs_WIRE); {var}_outer_wire_explorer.More(); {var}_outer_wire_explorer.Next()) {{\n        {var}_outer_wire = TopoDS::Wire({var}_outer_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_outer_wire.IsNull()) {{ return 12; }}\n    BRepBuilderAPI_MakeFace {var}_profile_face({var}_outer_wire);\n"
    ));
    for (index, hole) in profile.holes.iter().enumerate() {
        let hole_var = slot_var(*hole);
        body.push_str(&format!(
            "    TopoDS_Wire {var}_hole_{index}_wire;\n    for (TopExp_Explorer {var}_hole_{index}_wire_explorer({hole_var}, TopAbs_WIRE); {var}_hole_{index}_wire_explorer.More(); {var}_hole_{index}_wire_explorer.Next()) {{\n        {var}_hole_{index}_wire = TopoDS::Wire({var}_hole_{index}_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_hole_{index}_wire.IsNull()) {{ return 13; }}\n    {var}_profile_face.Add({var}_hole_{index}_wire);\n"
        ));
    }
    body.push_str(&format!(
        "    TopoDS_Shape {var} = {var}_profile_face.Shape();\n"
    ));
    Ok(())
}

fn emit_make_face_operation(body: &mut String, var: &str, input_var: String) {
    body.push_str(&format!(
        "    TopoDS_Wire {var}_make_face_wire;\n    for (TopExp_Explorer {var}_make_face_wire_explorer({input_var}, TopAbs_WIRE); {var}_make_face_wire_explorer.More(); {var}_make_face_wire_explorer.Next()) {{\n        {var}_make_face_wire = TopoDS::Wire({var}_make_face_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_make_face_wire.IsNull()) {{ return 14; }}\n    BRepBuilderAPI_MakeFace {var}_make_face_face({var}_make_face_wire);\n    TopoDS_Shape {var} = {var}_make_face_face.Shape();\n"
    ));
}

fn emit_offset_operation(body: &mut String, var: &str, input_var: String, amount: f64) {
    body.push_str(&format!(
        "    TopoDS_Wire {var}_offset_input_wire;\n    for (TopExp_Explorer {var}_offset_input_wire_explorer({input_var}, TopAbs_WIRE); {var}_offset_input_wire_explorer.More(); {var}_offset_input_wire_explorer.Next()) {{\n        {var}_offset_input_wire = TopoDS::Wire({var}_offset_input_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_offset_input_wire.IsNull()) {{ return 15; }}\n    BRepOffsetAPI_MakeOffset {var}_offset({var}_offset_input_wire, GeomAbs_Arc, false);\n    {var}_offset.Perform({amount});\n    TopoDS_Shape {var}_offset_shape = {var}_offset.Shape();\n    TopoDS_Wire {var}_offset_wire;\n    for (TopExp_Explorer {var}_offset_wire_explorer({var}_offset_shape, TopAbs_WIRE); {var}_offset_wire_explorer.More(); {var}_offset_wire_explorer.Next()) {{\n        {var}_offset_wire = TopoDS::Wire({var}_offset_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_offset_wire.IsNull()) {{ return 16; }}\n    TopoDS_Shape {var} = BRepBuilderAPI_MakeFace({var}_offset_wire).Shape();\n"
    ));
}

fn emit_sweep_operation(body: &mut String, var: &str, profile_var: String, path_var: String) {
    body.push_str(&format!(
        "    TopoDS_Wire {var}_profile_wire;\n    for (TopExp_Explorer {var}_profile_wire_explorer({profile_var}, TopAbs_WIRE); {var}_profile_wire_explorer.More(); {var}_profile_wire_explorer.Next()) {{\n        {var}_profile_wire = TopoDS::Wire({var}_profile_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_profile_wire.IsNull()) {{ return 7; }}\n    TopoDS_Wire {var}_path_wire;\n    for (TopExp_Explorer {var}_path_wire_explorer({path_var}, TopAbs_WIRE); {var}_path_wire_explorer.More(); {var}_path_wire_explorer.Next()) {{\n        {var}_path_wire = TopoDS::Wire({var}_path_wire_explorer.Current());\n        break;\n    }}\n    if ({var}_path_wire.IsNull()) {{ return 8; }}\n    BRepOffsetAPI_MakePipeShell {var}_pipe({var}_path_wire);\n    {var}_pipe.Add({var}_profile_wire);\n    {var}_pipe.Build();\n    if (!{var}_pipe.IsDone()) {{ return 9; }}\n    {var}_pipe.MakeSolid();\n    TopoDS_Shape {var} = {var}_pipe.Shape();\n"
    ));
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
        "    Standard_Real {var}_w = {width};\n    Standard_Real {var}_h = {height};\n    Standard_Real {var}_radius = {radius};\n    Standard_Real {var}_r = std::min(std::abs({var}_radius), std::min(std::abs({var}_w) / 2.0, std::abs({var}_h) / 2.0));\n    Standard_Real {var}_x0 = -{var}_w / 2.0;\n    Standard_Real {var}_y0 = -{var}_h / 2.0;\n    Standard_Real {var}_x1 = {var}_w / 2.0;\n    Standard_Real {var}_y1 = {var}_h / 2.0;\n    TopoDS_Shape {var};\n    if ({var}_r <= 1.0e-12) {{\n        BRepBuilderAPI_MakePolygon {var}_polygon;\n        {var}_polygon.Add(gp_Pnt({var}_x0, {var}_y0, 0));\n        {var}_polygon.Add(gp_Pnt({var}_x1, {var}_y0, 0));\n        {var}_polygon.Add(gp_Pnt({var}_x1, {var}_y1, 0));\n        {var}_polygon.Add(gp_Pnt({var}_x0, {var}_y1, 0));\n        {var}_polygon.Close();\n        TopoDS_Wire {var}_wire = {var}_polygon.Wire();\n        {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n    }} else {{\n        BRepBuilderAPI_MakeWire {var}_wire_builder;\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({var}_x0 + {var}_r, {var}_y0, 0), gp_Pnt({var}_x1 - {var}_r, {var}_y0, 0)).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({var}_x1 - {var}_r, {var}_y0, 0), gp_Pnt({var}_x1, {var}_y0, 0), gp_Pnt({var}_x1, {var}_y0 + {var}_r, 0)).Value()).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({var}_x1, {var}_y0 + {var}_r, 0), gp_Pnt({var}_x1, {var}_y1 - {var}_r, 0)).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({var}_x1, {var}_y1 - {var}_r, 0), gp_Pnt({var}_x1, {var}_y1, 0), gp_Pnt({var}_x1 - {var}_r, {var}_y1, 0)).Value()).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({var}_x1 - {var}_r, {var}_y1, 0), gp_Pnt({var}_x0 + {var}_r, {var}_y1, 0)).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({var}_x0 + {var}_r, {var}_y1, 0), gp_Pnt({var}_x0, {var}_y1, 0), gp_Pnt({var}_x0, {var}_y1 - {var}_r, 0)).Value()).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt({var}_x0, {var}_y1 - {var}_r, 0), gp_Pnt({var}_x0, {var}_y0 + {var}_r, 0)).Edge());\n        {var}_wire_builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt({var}_x0, {var}_y0 + {var}_r, 0), gp_Pnt({var}_x0, {var}_y0, 0), gp_Pnt({var}_x0 + {var}_r, {var}_y0, 0)).Value()).Edge());\n        TopoDS_Wire {var}_wire = {var}_wire_builder.Wire();\n        {var} = BRepBuilderAPI_MakeFace({var}_wire).Shape();\n    }}\n"
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
        OcctOp::Circle => "circle",
        OcctOp::Rectangle => "rectangle",
        OcctOp::RoundedRectangle => "rounded-rect",
        OcctOp::RoundedPolygon => "rounded-polygon",
        OcctOp::Polygon => "polygon",
        OcctOp::Profile => "profile",
        OcctOp::MakeFace => "make-face",
        OcctOp::Extrude => "extrude",
        OcctOp::Revolve => "revolve",
        OcctOp::Loft => "loft",
        OcctOp::Sweep => "sweep",
        OcctOp::Twist => "twist",
        OcctOp::Taper => "taper",
        OcctOp::Offset => "offset",
        OcctOp::Path => "path",
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

    fn compile(source: &str) -> CoreProgram {
        crate::ecky_scheme::compile_to_core_program(source).expect("compile")
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecky-{label}-{}", uuid::Uuid::new_v4()))
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
        assert!(source.contains("StlAPI_Writer"));
        assert!(source.contains("/tmp/model.step"));
        assert!(source.contains("/tmp/preview.stl"));
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
        assert!(source.contains("_profile_face.Add"), "{source}");
        assert!(source.contains("BRepPrimAPI_MakePrism"), "{source}");
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
}
