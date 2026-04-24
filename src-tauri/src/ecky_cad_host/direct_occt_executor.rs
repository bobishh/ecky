use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::direct_occt::{OcctArg, OcctOp, OcctPlan, OcctSlot};
use super::direct_occt_sdk::{run_native_export_source, DirectOcctSdkLayout, NativeExportOutcome};
use crate::ecky_core_ir::CoreProgram;
use crate::models::{AppError, AppResult};

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
    let plan = super::direct_occt::plan_core_program(program)?;
    export_plan_step_stl(&plan, layout, output_dir)
}

pub fn export_plan_step_stl(
    plan: &OcctPlan,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    let output_dir = output_dir.as_ref();
    let step_path = output_dir.join("model.step");
    let stl_path = output_dir.join("preview.stl");
    let source = emit_plan_export_source(plan, &step_path, &stl_path)?;
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
    if !plan.parameters.is_empty() {
        return Err(AppError::validation(
            "Direct OCCT executor does not support runtime parameters yet.",
        ));
    }
    if plan.parts.len() != 1 {
        return Err(AppError::validation(format!(
            "Direct OCCT executor first tranche supports one part, got {}.",
            plan.parts.len()
        )));
    }

    let part = &plan.parts[0];
    let mut vars = BTreeMap::new();
    let mut body = String::new();
    for command in &part.commands {
        let var = slot_var(command.output);
        if !command.keywords.is_empty() {
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
                emit_polygon_face(&mut body, &var, &points)?;
            }
            OcctOp::Polygon => {
                let points = point2_list_arg(&command.args, 0)?;
                emit_polygon_face(&mut body, &var, &points)?;
            }
            OcctOp::Extrude => {
                let profile = ref_arg(&command.args, 0)?;
                let distance = numeric_arg(&command.args, 1)?;
                body.push_str(&format!(
                    "    TopoDS_Shape {var} = BRepPrimAPI_MakePrism({}, gp_Vec(0, 0, {distance})).Shape();\n",
                    slot_var(profile)
                ));
            }
            OcctOp::Translate => {
                let [x, y, z] = numeric_prefix_args::<3>(&command.args)?;
                let input = ref_arg(&command.args, 3)?;
                body.push_str(&format!(
                    "    gp_Trsf {var}_trsf;\n    {var}_trsf.SetTranslation(gp_Vec({x}, {y}, {z}));\n    TopoDS_Shape {var} = BRepBuilderAPI_Transform({}, {var}_trsf, true).Shape();\n",
                    slot_var(input)
                ));
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
                &mut body,
                &var,
                "union",
                "BRepAlgoAPI_Fuse",
                ref_args(&command.args)?,
            )?,
            OcctOp::Difference => emit_boolean_fold(
                &mut body,
                &var,
                "difference",
                "BRepAlgoAPI_Cut",
                ref_args(&command.args)?,
            )?,
            OcctOp::Intersection => emit_boolean_fold(
                &mut body,
                &var,
                "intersection",
                "BRepAlgoAPI_Common",
                ref_args(&command.args)?,
            )?,
            other => {
                return Err(AppError::validation(format!(
                    "Direct OCCT executor does not support `{}` yet.",
                    op_name(other)
                )));
            }
        }
        vars.insert(command.output, var);
    }

    let root_var = vars.get(&part.root).ok_or_else(|| {
        AppError::validation(format!(
            "Direct OCCT executor could not find root slot {:?}.",
            part.root
        ))
    })?;

    Ok(format!(
        r#"#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRep_Builder.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <STEPControl_Writer.hxx>
#include <StlAPI_Writer.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Wire.hxx>
#include <gp_Ax2.hxx>
#include <gp_Circ.hxx>
#include <gp_Dir.hxx>
#include <gp_Pnt.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>

int main() {{
{body}    TopoDS_Shape shape = {root_var};
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
    ))
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
        OcctOp::Circle => "circle",
        OcctOp::Rectangle => "rectangle",
        OcctOp::Polygon => "polygon",
        OcctOp::Extrude => "extrude",
        OcctOp::Revolve => "revolve",
        OcctOp::Union => "union",
        OcctOp::Difference => "difference",
        OcctOp::Intersection => "intersection",
        OcctOp::Fillet => "fillet",
        OcctOp::Chamfer => "chamfer",
        OcctOp::Shell => "shell",
        OcctOp::Translate => "translate",
        OcctOp::Rotate => "rotate",
        OcctOp::Scale => "scale",
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
    fn emits_supported_solid_ops_as_native_occt_source() {
        let program = compile(
            r#"
            (model
              (part body
                (intersection
                  (union
                    (sphere 6)
                    (translate 10 0 0 (cylinder 3 14))
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
    fn reports_unsupported_executor_ops_by_name() {
        let program = compile("(model (part body (rotate 0 0 45 (box 1 1 1))))");
        let plan = crate::ecky_cad_host::direct_occt::plan_core_program(&program).expect("plan");

        let err = emit_plan_export_source(
            &plan,
            Path::new("/tmp/model.step"),
            Path::new("/tmp/preview.stl"),
        )
        .expect_err("rotate unsupported");

        let message = err.to_string();
        assert!(message.contains("rotate"), "{message}");
        assert!(message.contains("Direct OCCT executor"), "{message}");
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
                  (translate -24 0 0
                    (extrude (polygon ((0 0) (10 0) (6 8) (0 6))) 4)))))
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
}
