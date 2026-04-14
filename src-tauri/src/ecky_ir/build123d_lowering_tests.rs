use super::super::edge_ops::{
    chamfer_mesh, detect_feature_edges, fillet_mesh, filter_edges, EdgeSelector,
};
use super::super::shared::IrMesh;
use super::lower_to_build123d;

#[test]
fn lower_to_build123d_minimal_extrude() {
    let src = r#"(model (part body (extrude (rounded_rect 30 20 4) 10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("from build123d import *"), "missing import");
    assert!(
        code.contains("RectangleRounded(30.0, 20.0, 4.0)"),
        "rounded_rect"
    );
    assert!(code.contains("extrude("), "extrude call");
    assert!(code.contains(r#"("body","#), "part entry");
    assert!(code.contains("_ecky_parts"), "_ecky_parts assignment");
}

#[test]
fn lower_to_build123d_difference() {
    let src = r#"(model (part shell (difference (cylinder 10 20) (cylinder 8 20))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Cylinder(10.0, 20.0,"), "outer cylinder");
    assert!(code.contains("Cylinder(8.0, 20.0,"), "inner cylinder");
    assert!(code.contains(" - "), "difference operator");
    assert!(code.contains(r#"("shell","#), "part entry");
}

#[test]
fn lower_to_build123d_param_refs() {
    let src = r#"(model (params (number width 30) (number height 20)) (part body (extrude (rounded_rect width height 4) 10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(r#"float(params.get("width", 30.0))"#),
        "width param: {}",
        code
    );
    assert!(
        code.contains(r#"float(params.get("height", 20.0))"#),
        "height param: {}",
        code
    );
}

#[test]
fn lower_to_build123d_let_local_number_shadows_param() {
    let src = r#"(model
        (params (number width 30))
        (part body
          (let ((width 12))
            (extrude (circle width) 5))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_width = 12.0"), "local binding: {}", code);
    assert!(code.contains("Circle(_width)"), "local use: {}", code);
    assert!(
        !code.contains(r#"float(params.get("width", 30.0))"#),
        "should not use param after shadowing: {}",
        code
    );
}

#[test]
fn lower_to_build123d_let_local_bool_drives_if() {
    let src = r#"(model
        (params (toggle cap #f))
        (part body
          (let ((cap #t))
            (if cap (sphere 10) (cylinder 10 20)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_cap = True"), "local bool: {}", code);
    assert!(
        code.contains("if _cap:"),
        "conditional gate uses local: {}",
        code
    );
    assert!(
        code.contains("else:"),
        "conditional has else block: {}",
        code
    );
}

#[test]
fn lower_to_build123d_let_local_geometry_shadows_param_name() {
    let src = r#"(model
        (params (number body 30))
        (part shell
          (let ((body (box 10 10 10)))
            (translate 1 2 3 body))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_body = _v"), "geom alias: {}", code);
    assert!(
        code.contains("_ecky_apply_transform(Pos(1.0, 2.0, 3.0), _body)"),
        "geom use: {}",
        code
    );
}

#[test]
fn lower_to_build123d_nested_let_prefers_nearest_binding() {
    let src = r#"(model
        (part body
          (let ((r 10))
            (let ((r 4))
              (extrude (circle r) 5)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_r = 10.0"), "outer binding: {}", code);
    assert!(code.contains("_r = 4.0"), "inner binding: {}", code);
    assert!(code.contains("Circle(_r)"), "inner binding used: {}", code);
}

#[test]
fn lower_to_build123d_parallel_let_rejects_same_frame_dependency() {
    let src = r#"(model
        (part body
          (let ((a 2) (b (+ a 1)))
            (extrude (circle b) 5))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(
        err.to_string().contains("Unknown symbol `a`"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_nested_let_allows_sequential_dependency() {
    let src = r#"(model
        (part body
          (let ((a 2))
            (let ((b (+ a 1)))
              (extrude (circle b) 5)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_a = 2.0"), "outer: {}", code);
    assert!(code.contains("_b = (_a + 1.0)"), "inner: {}", code);
}

#[test]
fn lower_to_build123d_numeric_expressions() {
    let src = r#"(model (params (number w 40)) (part body (extrude (circle (/ w 2)) 5)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(r#"float(params.get("w", 40.0)) / 2.0"#),
        "division: {}",
        code
    );
}

#[test]
fn lower_to_build123d_polygon_accepts_let_wrapped_point_lists() {
    let src = r#"(model
        (part body
          (extrude
            (polygon
              (list
                (let ((i 1))
                  (list i (+ i 1)))
                (let ((i 3))
                  (list i (+ i 1)))))
            5)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Polygon("), "polygon: {}", code);
    assert!(code.contains("(1.0, (1.0 + 1.0))"), "first point: {}", code);
    assert!(
        code.contains("(3.0, (3.0 + 1.0))"),
        "second point: {}",
        code
    );
}

#[test]
fn lower_to_build123d_path_accepts_let_wrapped_point_lists() {
    let src = r#"(model
        (part rail
          (path
            (list
              (let ((i 1))
                (list i 0 (+ i 1)))
              (let ((i 3))
                (list i 0 (+ i 1)))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Polyline("), "path: {}", code);
    assert!(
        code.contains("(1.0, 0.0, (1.0 + 1.0))"),
        "first point: {}",
        code
    );
    assert!(
        code.contains("(3.0, 0.0, (3.0 + 1.0))"),
        "second point: {}",
        code
    );
}

#[test]
fn lower_to_build123d_polygon_accepts_build_bound_generated_point_lists() {
    let src = r#"(model
        (part body
          (build
            (shape pts
              ((let ((i 0))
                 (list i (+ i 10)))
               (let ((i 2))
                 (list i (+ i 10)))
               (let ((i 4))
                 (list i (+ i 10)))))
            (result (extrude (polygon pts) 5)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Polygon("), "polygon: {}", code);
    assert!(
        code.contains("(0.0, (0.0 + 10.0))"),
        "first point: {}",
        code
    );
    assert!(
        code.contains("(2.0, (2.0 + 10.0))"),
        "second point: {}",
        code
    );
    assert!(
        code.contains("(4.0, (4.0 + 10.0))"),
        "third point: {}",
        code
    );
}

#[test]
fn lower_to_build123d_bezier_path_accepts_let_bound_generated_point_lists() {
    let src = r#"(model
        (part body
          (let ((pts
                  ((let ((x 0) (z 0))
                     (list x 0 z))
                   (let ((x 10) (z 5))
                     (list x 0 z))
                   (let ((x 20) (z 10))
                     (list x 0 z))
                   (let ((x 30) (z 15))
                     (list x 0 z)))))
            (sweep (circle 2) (bezier-path pts)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Bezier("), "bezier: {}", code);
    assert!(code.contains("(0.0, 0.0, 0.0)"), "first point: {}", code);
    assert!(code.contains("(10.0, 0.0, 5.0)"), "second point: {}", code);
    assert!(code.contains("(30.0, 0.0, 15.0)"), "last point: {}", code);
}

#[test]
fn lower_to_build123d_bspline_accepts_build_bound_generated_point_lists() {
    let src = r#"(model
        (part body
          (build
            (shape pts
              ((let ((x 0) (y 0))
                 (list x y))
               (let ((x 5) (y 2))
                 (list x y))
               (let ((x 10) (y 0))
                 (list x y))))
            (result (bspline pts #f)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("Spline([(0.0, 0.0), (5.0, 2.0), (10.0, 0.0)], periodic=False)"),
        "bspline: {}",
        code
    );
}

#[test]
fn lower_to_build123d_polygon_reports_wrong_bound_list_kind() {
    let src = r#"(model
        (part body
          (let ((pts ((list 0 (list 0 0)) (list 1 (list 10 0)))))
            (extrude (polygon pts) 5))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(
        err.to_string()
            .contains("CAD op `polygon` expected 2D point list")
            && err.to_string().contains("pair list"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_bspline_reports_wrong_bound_list_kind() {
    let src = r#"(model
        (part body
          (let ((pts ((0 0 0) (5 2 1) (10 0 0))))
            (bspline pts #f))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(
        err.to_string()
            .contains("CAD op `bspline` expected 2D point list")
            && err.to_string().contains("3D point list"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_translate_rotate() {
    let src = r#"(model (part body (translate 5 0 0 (rotate 0 0 45 (box 10 10 10)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Box(10.0, 10.0, 10.0,"), "box");
    assert!(
        code.contains("_ecky_apply_transform(Rot(0.0, 0.0, 45.0),"),
        "rotate helper"
    );
    assert!(
        code.contains("_ecky_apply_transform(Pos(5.0, 0.0, 0.0),"),
        "translate helper"
    );
}

#[test]
fn lower_to_build123d_unsupported_node_returns_error() {
    let src =
        r#"(model (part body (wall-pattern (:mode ribs :depth 1) (shell 2 (cylinder 10 20)))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(
        err.to_string().contains("not yet supported"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_rejects_mutable_def() {
    let src = r#"(model (part body (def body (box 10 10 10))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(
        err.to_string().contains("not supported by Ecky IR v0"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_union_three_parts() {
    let src = r#"(model (part compound (union (sphere 5) (cylinder 3 10) (box 4 4 4))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Sphere(5.0)"), "sphere");
    assert!(code.contains("Cylinder(3.0, 10.0,"), "cylinder");
    assert!(code.contains("Box(4.0, 4.0, 4.0,"), "box");
    assert!(code.contains("_ecky_fuse_many("), "fuse helper: {}", code);
}

#[test]
fn lower_to_build123d_shell_cylinder() {
    let src = r#"(model (part body (shell 2 (cylinder 10 20))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Cylinder(10.0, 20.0,"), "outer cylinder");
    assert!(code.contains("10.0 - 2.0"), "inner radius: {}", code);
    assert!(code.contains(" - "), "difference");
}

#[test]
fn lower_to_build123d_shell_extrude() {
    let src = r#"(model (part body (shell 1.5 (extrude (circle 12) 20))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Circle(12.0)"), "circle");
    assert!(code.contains("offset("), "offset for inner sketch");
    assert!(code.contains("extrude("), "extrude");
    assert!(
        code.contains("= _ecky_face("),
        "shell extrude should coerce sketches to faces: {}",
        code
    );
    assert!(code.contains(" - "), "difference");
}

#[test]
fn lower_to_build123d_revolve() {
    let src = r#"(model (part body (revolve (polygon ((10 0) (14 0) (14 20) (10 20))) 360)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Polygon("), "polygon");
    assert!(code.contains("Rot(90, 0, 0)"), "rotation to XZ");
    assert!(code.contains("revolve("), "revolve call");
    assert!(code.contains("revolution_arc=360.0"), "full revolution");
}

#[test]
fn lower_to_build123d_loft() {
    let src = r#"(model (part body (loft 30 (circle 20) (circle 10))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Circle(20.0)"), "bottom");
    assert!(code.contains("Circle(10.0)"), "top");
    assert!(
        code.contains("Pos(0, 0, (30.0) * 1)"),
        "height positioning: {}",
        code
    );
    assert!(code.contains("loft("), "loft call");
}

#[test]
fn lower_to_build123d_sweep() {
    let src = r#"(model (part body (sweep (circle 5) (path (0 0 0) (0 0 30)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Circle(5.0)"), "section");
    assert!(code.contains("Polyline("), "path");
    assert!(code.contains("_ecky_face("), "face coercion");
    assert!(code.contains("sweep("), "sweep call");
}

#[test]
fn lower_to_build123d_sweep_profile_with_holes() {
    let src = r#"(model
        (part body
          (sweep
            (profile
              (:outer ((-20 0) (20 0) (20 12) (-20 12)))
              (:holes (((10 14) (15 14) (14 9) (9 9)) ((-15 14) (-10 14) (-9 9) (-14 9)))))
            (bezier-path ((0 0 0) (30 0 0) (60 0 10) (90 0 10))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Bezier("), "path: {}", code);
    assert!(code.contains("Polygon("), "profile loops: {}", code);
    assert!(
        !code.contains("Wire(Polygon("),
        "raw loop fallback: {}",
        code
    );
    assert!(code.contains(" - "), "hole subtraction: {}", code);
    assert!(code.contains("sweep("), "sweep call: {}", code);
}

#[test]
fn lower_to_build123d_build_shape_result_clip_and_place() {
    let src = r#"(model
        (part body
          (build
            (shape rail (bezier-path ((0 0 0) (10 0 0) (20 0 10) (30 0 10))))
            (shape peg (cylinder 2 6))
            (shape end-frame (path-frame rail :at end))
            (shape placed-peg (place end-frame peg :offset (0 0 -3)))
            (shape clipped (clip-box placed-peg :x (20 40) :y (-5 5) :z (-10 20)))
            (result (compound clipped)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains(".location_at("), "path-frame: {}", code);
    assert!(code.contains("_ecky_place("), "place helper: {}", code);
    assert!(
        code.contains("_ecky_clip_box("),
        "clip-box helper: {}",
        code
    );
    assert!(
        code.contains("_ecky_compound("),
        "compound helper: {}",
        code
    );
}

#[test]
fn lower_to_build123d_path_frame_param_anchor_uses_numeric_expr() {
    let src = r#"(model
        (params (number t 0.25))
        (part body
          (build
            (shape rail (path (0 0 0) (0 0 10)))
            (shape frame (path-frame rail :at t))
            (shape peg (box 2 2 2))
            (result (place frame peg)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(r#"float(params.get("t", 0.25))"#),
        "numeric :at should use lowered param expr: {}",
        code
    );
    assert!(
        !code.contains(r#"_ecky_path_frame(_v0, "t", None)"#),
        "symbol anchor should not lower as string sentinel: {}",
        code
    );
}

#[test]
fn lower_to_build123d_build_rejects_missing_result() {
    let src = r#"(model
        (part body
          (build
            (shape body (box 10 10 10)))))"#;
    let err = lower_to_build123d(src).expect_err("missing result");
    assert!(err
        .to_string()
        .contains("`build` requires exactly one `(result ...)`"));
}

#[test]
fn lower_to_build123d_build_rejects_rebinding() {
    let src = r#"(model
        (part body
          (build
            (shape body (box 10 10 10))
            (shape body (cylinder 2 8))
            (result body))))"#;
    let err = lower_to_build123d(src).expect_err("rebind");
    assert!(err.to_string().contains("cannot rebind shape `body`"));
}

#[test]
fn lower_to_build123d_place_rejects_unknown_keyword() {
    let src = r#"(model
        (part body
          (build
            (shape rail (path (0 0 0) (0 0 10)))
            (shape frame (path-frame rail))
            (shape peg (box 2 2 2))
            (result (place frame peg :spin (0 0 0))))))"#;
    let err = lower_to_build123d(src).expect_err("unknown place keyword");
    assert!(
        err.to_string()
            .contains("`place` does not recognize option `:spin`"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_clip_box_rejects_unknown_keyword() {
    let src = r#"(model
        (part body
          (clip-box (box 10 10 10)
            :x (-5 5)
            :y (-5 5)
            :z (-5 5)
            :padding 2)))"#;
    let err = lower_to_build123d(src).expect_err("unknown clip-box keyword");
    assert!(
        err.to_string()
            .contains("`clip-box` does not recognize option `:padding`"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_make_face_union_of_wires_uses_direct_face_helper() {
    let src = r#"(model
        (part body
          (extrude
            (make-face
              (union
                (path (0 0 0) (10 0 0))
                (path (10 0 0) (10 10 0))
                (path (10 10 0) (0 10 0))
                (path (0 10 0) (0 0 0))))
            5)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("_ecky_face_from_wires("),
        "wire fast path: {}",
        code
    );
}

#[test]
fn lower_to_build123d_extrude_rejects_path_operand() {
    let src = r#"(model (part body (extrude (path (0 0 0) (0 0 30)) 5)))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(err.to_string().contains("2D sketch"), "unexpected: {}", err);
}

#[test]
fn lower_to_build123d_revolve_rejects_path_operand() {
    let src = r#"(model (part body (revolve (path (0 0 0) (0 0 30)) 360)))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(err.to_string().contains("2D sketch"), "unexpected: {}", err);
}

#[test]
fn lower_to_build123d_sweep_rejects_non_path_second_operand() {
    let src = r#"(model (part body (sweep (circle 5) (circle 10))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(err.to_string().contains("3D path"), "unexpected: {}", err);
}

#[test]
fn lower_to_build123d_mirror() {
    let src = r#"(model (part body (mirror x 0 (box 10 10 10))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("mirror("), "mirror call");
    assert!(code.contains("Plane.YZ"), "YZ plane for x-axis mirror");
}

#[test]
fn lower_to_build123d_transformed_sketch_stays_valid_for_3d_ops() {
    let src = r#"(model
        (part body
          (union
            (extrude (translate 1 2 0 (circle 5)) 10)
            (revolve (rotate 0 0 15 (polygon ((10 0) (12 0) (12 5) (10 5)))) 90)
            (sweep (rotate 0 0 30 (circle 2)) (path (0 0 0) (0 0 10))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("_ecky_apply_transform(Pos(1.0, 2.0, 0.0),"),
        "translated sketch: {}",
        code
    );
    assert!(
        code.contains("Rot(0.0, 0.0, 15.0)"),
        "rotated revolve sketch: {}",
        code
    );
    assert!(
        code.contains("Rot(0.0, 0.0, 30.0)"),
        "rotated sweep sketch: {}",
        code
    );
    assert!(code.contains("sweep("), "sweep: {}", code);
}

#[test]
fn lower_to_build123d_if_conditional() {
    let src =
        r#"(model (params (toggle cap #t)) (part body (if cap (sphere 10) (cylinder 10 20))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Sphere(10.0)"), "then branch");
    assert!(code.contains("Cylinder(10.0, 20.0,"), "else branch");
    assert!(code.contains("if "), "conditional");
    assert!(code.contains("else:"), "else");
    assert!(code.contains("params.get(\"cap\""), "param ref");
}

#[test]
fn lower_to_build123d_if_rejects_mismatched_kinds() {
    let src = r#"(model (part body (if #t (circle 10) (sphere 10))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(
        err.to_string().contains("matching branch kinds"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_linear_array() {
    let src = r#"(model (part body (linear-array 4 10 0 0 (box 5 5 5))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Box(5.0, 5.0, 5.0,"), "base box");
    assert!(code.contains("for __ecky_la_i in range(1, 4)"), "loop");
    assert!(code.contains("Pos(10.0 * __ecky_la_i"), "positioning");
}

#[test]
fn lower_to_build123d_linear_array_temp_names_do_not_collide_with_let_locals() {
    let src = r#"(model
        (part body
          (let ((b1 10))
            (translate (+ b1 5)
              0
              0
              (linear-array 3 8 0 0 (box 5 5 5))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_b1 = 10.0"), "let local: {}", code);
    assert!(
        code.contains("for __ecky_la_i in range(1, 3)"),
        "reserved loop var: {}",
        code
    );
    assert!(
        code.contains("Pos((_b1 + 5.0), 0.0, 0.0)"),
        "let local should survive array lowering: {}",
        code
    );
}

#[test]
fn lower_to_build123d_profile_with_holes() {
    let src =
        r#"(model (part body (extrude (profile (:outer (circle 20)) (:holes (circle 10))) 10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Circle(20.0)"), "outer circle");
    assert!(code.contains("Circle(10.0)"), "hole circle");
    assert!(code.contains(" - "), "hole subtraction");
    assert!(code.contains("extrude("), "extrude");
}

#[test]
fn lower_to_build123d_profile_accepts_bspline_outer_loop() {
    let src = r#"(model (part body (extrude (profile (:outer (union (bspline ((0 0) (5 5) (10 0)) #f :tangents ((1 0) (1 0)) :tangent-scalars (1 1)) (path (0 0 0) (10 0 0))))) 5)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Spline("), "bspline outer: {}", code);
    assert!(code.contains("_ecky_face"), "face coercion: {}", code);
}

#[test]
fn lower_to_build123d_profile_accepts_rounded_polygon_outer_loop() {
    let src = r#"(model
        (part body
          (extrude
            (profile (:outer (rounded-polygon ((0 0) (10 0) (10 10) (0 10)) 2)))
            5)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("fillet("), "rounded polygon: {}", code);
    assert!(code.contains("extrude("), "extrude: {}", code);
}

#[test]
fn lower_to_build123d_make_face_accepts_bspline() {
    let src = r#"(model
        (part body
          (make-face
            (union
              (bspline ((0 0) (5 5) (10 0)) #f :tangents ((1 0) (1 0)) :tangent-scalars (1 1))
              (path (0 0 0) (10 0 0))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Spline("), "bspline: {}", code);
    assert!(code.contains("_ecky_face"), "face: {}", code);
}

#[test]
fn lower_to_build123d_polygon() {
    let src = r#"(model (part body (extrude (polygon ((0 0) (10 0) (10 10) (0 10))) 5)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Polygon("), "polygon");
    assert!(code.contains("(0.0, 0.0)"), "point");
    assert!(code.contains("extrude("), "extrude");
}

#[test]
fn lower_to_build123d_xor() {
    let src = r#"(model (part body (xor (box 10 10 10) (translate 5 5 0 (box 10 10 10)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains(" + "), "union for xor");
    assert!(code.contains(" & "), "intersection for xor");
    assert!(code.contains(" - "), "difference for xor");
}

#[test]
fn lower_to_build123d_twist() {
    let src = r#"(model (part body (twist 40 90 (circle 10))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Circle(10.0)"), "sketch");
    assert!(code.contains("Pos(0, 0,"), "height positioning");
    assert!(code.contains("Rot(0, 0,"), "rotation");
    assert!(code.contains("loft("), "loft from sections");
}

#[test]
fn lower_to_build123d_trig_functions() {
    let src = r#"(model (part body (cylinder (sin (deg 45)) 10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("math.sin("), "sin");
    assert!(code.contains("math.radians("), "deg → radians");
    assert!(code.contains("import math"), "math import");
}

#[test]
fn lower_to_build123d_bezier_path() {
    let src = r#"(model (part body (sweep (circle 3) (bezier-path ((0 0 0) (5 10 20) (10 30 40) (8 50 50))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Bezier("), "bezier");
    assert!(code.contains("sweep("), "sweep");
}

#[test]
fn lower_to_build123d_offset() {
    let src = r#"(model (part body (extrude (offset 3 (circle 10)) 5)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("offset("), "offset call");
    assert!(code.contains("amount=3.0"), "offset amount");
}

#[test]
fn lower_to_build123d_offset_openings() {
    let src = r#"(model (part body (extrude (offset 3 :openings (circle 4) (circle 10)) 5)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("openings="), "openings: {}", code);
    assert!(
        code.contains("_ecky_face"),
        "opening face coercion: {}",
        code
    );
}

#[test]
fn lower_to_build123d_radial_array() {
    let src = r#"(model (part body (radial-array 6 60 20 (cylinder 3 10))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Cylinder(3.0, 10.0,"), "base");
    assert!(code.contains("Pos(20.0, 0, 0)"), "radius offset");
    assert!(code.contains("for __ecky_ra_i in range(1, 6)"), "loop");
    assert!(code.contains("Rot(0, 0, 60.0 * __ecky_ra_i)"), "rotation");
}

#[test]
fn lower_to_build123d_shell_revolve() {
    let src =
        r#"(model (part body (shell 2 (revolve (polygon ((10 0) (14 0) (14 20) (10 20))) 360))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Polygon("), "polygon");
    assert!(code.contains("offset("), "solid offset");
    assert!(code.contains("revolve("), "revolve call");
    assert!(
        code.contains("= _ecky_face("),
        "shell revolve should coerce sketches to faces: {}",
        code
    );
    assert!(
        code.contains(".faces().filter_by(GeomType.PLANE)"),
        "solid shell openings: {}",
        code
    );
    assert!(
        !code.contains("= _ecky_cut_many(") && !code.contains(" - _v"),
        "shell revolve should not boolean cut inner revolve anymore: {}",
        code
    );
}

#[test]
fn lower_to_build123d_shell_sweep() {
    let src = r#"(model (part body (shell 2 (sweep (circle 5) (path (0 0 0) (0 0 30))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Circle(5.0)"), "section: {}", code);
    assert!(code.contains("Polyline("), "path: {}", code);
    assert!(code.contains("offset("), "inner sketch offset: {}", code);
    assert!(code.contains("sweep("), "sweep call: {}", code);
    assert!(
        code.contains(" - "),
        "shell sweep still boolean diff: {}",
        code
    );
}

#[test]
fn lower_to_build123d_fillet_all_edges() {
    let src = r#"(model (part body (fillet 2 (box 20 20 10))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Box(20.0, 20.0, 10.0,"), "box");
    assert!(code.contains(".edges()"), "edge selection");
    assert!(code.contains("fillet("), "fillet call");
    assert!(code.contains(", 2.0)"), "radius");
}

#[test]
fn lower_to_build123d_fillet_top_edges() {
    let src = r#"(model (part body (fillet 1.5 :edges top (box 20 20 10))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(".edges().group_by(Axis.Z)[-1]"),
        "top edge selection: {}",
        code
    );
    assert!(code.contains("fillet("), "fillet call");
}

#[test]
fn lower_to_build123d_chamfer_bottom_edges() {
    let src = r#"(model (part body (chamfer 1 :edges bottom (cylinder 10 20))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(".edges().group_by(Axis.Z)[0]"),
        "bottom edge selection: {}",
        code
    );
    assert!(code.contains("chamfer("), "chamfer call");
}

#[test]
fn lower_to_build123d_fillet_vertical_edges() {
    let src = r#"(model (part body (fillet 3 :edges vertical (box 30 30 20))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(".edges().filter_by(Axis.Z)"),
        "vertical edge selection: {}",
        code
    );
}

#[test]
fn detect_feature_edges_box() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let edges = detect_feature_edges(&mesh);
    assert_eq!(edges.len(), 12, "a box has 12 feature edges");
}

#[test]
fn detect_feature_edges_cylinder() {
    let mesh = IrMesh::cylinder(10.0, 20.0, 32, None);
    let edges = detect_feature_edges(&mesh);
    assert!(
        edges.len() >= 32,
        "cylinder should have at least top rim edges, got {}",
        edges.len()
    );
}

#[test]
fn edge_selector_top_box() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let edges = detect_feature_edges(&mesh);
    let top = filter_edges(&edges, EdgeSelector::Top);
    assert_eq!(top.len(), 4, "box top face has 4 edges");
}

#[test]
fn edge_selector_bottom_box() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let edges = detect_feature_edges(&mesh);
    let bottom = filter_edges(&edges, EdgeSelector::Bottom);
    assert_eq!(bottom.len(), 4, "box bottom face has 4 edges");
}

#[test]
fn edge_selector_vertical_box() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let edges = detect_feature_edges(&mesh);
    let vertical = filter_edges(&edges, EdgeSelector::Vertical);
    assert_eq!(vertical.len(), 4, "box has 4 vertical edges");
}

#[test]
fn chamfer_zero_distance_noop() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let chamfered = chamfer_mesh(&mesh, 0.0, EdgeSelector::All).expect("zero chamfer");
    assert_eq!(
        chamfered.polygons.len(),
        mesh.polygons.len(),
        "zero distance should not modify polygon count"
    );
}

#[test]
fn chamfer_increases_polygon_count() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let original_count = mesh.triangulate().polygons.len();
    let chamfered = chamfer_mesh(&mesh, 1.0, EdgeSelector::All).expect("chamfer all");
    assert!(
        chamfered.polygons.len() > original_count,
        "chamfer should add polygons: {} vs {}",
        chamfered.polygons.len(),
        original_count
    );
}

#[test]
fn fillet_increases_polygon_count() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let original_count = mesh.triangulate().polygons.len();
    let filleted = fillet_mesh(&mesh, 1.0, EdgeSelector::All).expect("fillet all");
    let filleted_count = filleted.triangulate().polygons.len();
    assert!(
        filleted_count > original_count,
        "fillet should add polygons: {} vs {}",
        filleted_count,
        original_count
    );
}

#[test]
fn lower_to_build123d_rounded_polygon() {
    let src = r#"(model (part body (rounded-polygon ((0 0) (10 0) (10 10) (0 10)) 2)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Polygon([(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)])"));
    assert!(code.contains("fillet("));
    assert!(code.contains(".vertices(), 2.0)"));
}

#[test]
fn lower_to_build123d_bspline() {
    let src = r#"(model (part body (bspline ((0 0) (5 2) (10 0)) #f)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Spline([(0.0, 0.0), (5.0, 2.0), (10.0, 0.0)], periodic=False)"));
}

#[test]
fn lower_to_build123d_bspline_keyword_properties() {
    let src = r#"(model (part body (bspline ((0 0) (5 2) (10 0)) :closed #f :tangents ((1 0) (1 1) (1 0)) :tangent-scalars ((1 1) (2 2) (1 1)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("periodic=False"), "closed: {}", code);
    assert!(
        code.contains("tangents=[(1.0, 0.0), (1.0, 1.0), (1.0, 0.0)]"),
        "tangents: {}",
        code
    );
    assert!(
        code.contains("tangent_scalars=[1.0, 2.0, 1.0]"),
        "scalars: {}",
        code
    );
}

#[test]
fn lower_to_build123d_bspline_flat_tangent_scalars() {
    let src = r#"(model (part body (bspline ((30 10) (69 105)) #f :tangents ((1 0.5) (0.7 1)) :tangent-scalars (1.75 1))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("tangents=[(1.0, 0.5), (0.7, 1.0)]"),
        "tangents: {}",
        code
    );
    assert!(
        code.contains("tangent_scalars=[1.75, 1.0]"),
        "flat scalars: {}",
        code
    );
}

#[test]
fn lower_to_build123d_bspline_rejects_ambiguous_scalar_pairs() {
    let src =
        r#"(model (part body (bspline ((0 0) (5 2) (10 0)) #f :tangent-scalars ((1 2) (2 2)))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(err.to_string().contains("ambiguous"), "unexpected: {}", err);
}

#[test]
fn lower_to_build123d_bspline_rejects_bad_tangent_count() {
    let src = r#"(model (part body (bspline ((0 0) (5 2) (10 0)) #f :tangents ((1 0)))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(
        err.to_string().contains("`tangents` expects 2 entries"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_bspline_rejects_bad_scalar_count() {
    let src = r#"(model (part body (bspline ((0 0) (5 2) (10 0)) #f :tangent-scalars (1 2 3 4))))"#;
    let err = lower_to_build123d(src).unwrap_err();
    assert!(
        err.to_string()
            .contains("`tangent-scalars` expects 2 entries"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_text_falls_back_to_literal_symbol_string() {
    let src = r#"(model (part body (text axis 10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("Text(\"axis\", font_size=10.0)"),
        "literal symbol: {}",
        code
    );
}

#[test]
fn lower_to_build123d_canonical_cup_contains_expected_snippets() {
    let code = lower_to_build123d(include_str!(
        "../../tests/fixtures/cad/surface/canonical_cup.ecky"
    ))
    .expect("lower");
    assert!(
        code.contains("tangent_scalars=[1.75, 1.0]"),
        "flat tangent scalars: {}",
        code
    );
    assert!(
        !code.contains("Rot(90.0, 0.0, 0.0) * Rot(90"),
        "no pre-rotate drift: {}",
        code
    );
    assert!(
        code.contains("faces().filter_by(GeomType.PLANE)"),
        "solid shell openings: {}",
        code
    );
    assert!(
        code.contains("_ecky_apply_transform(Pos(0.0, 0.0, 10.0),"),
        "translated base plug: {}",
        code
    );
    assert!(code.contains("fillet("), "fillet: {}", code);
}

#[test]
fn lower_to_build123d_text() {
    let src = r#"(model (part body (text "hello" 10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Text(\"hello\", font_size=10.0)"));
}

#[test]
fn lower_to_build123d_svg() {
    let src = r#"(model (part body (svg "logo.svg")))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("import_svg(\"logo.svg\")"));
}

#[test]
fn lower_to_build123d_import_stl() {
    let src = r#"(model (part body (import-stl "base.stl")))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("import_stl(\"base.stl\")"));
}

#[test]
fn lower_to_build123d_shell_twist() {
    let src = r#"(model (part body (shell 2 (twist 40 90 (circle 10)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("offset("));
    assert!(
        code.contains("amount=(-2.0)") || code.contains("amount=-(2.0)"),
        "offset amount: {}",
        code
    );
    assert!(code.contains("loft("));
    assert!(code.contains(" - "));
}

#[test]
fn lower_to_build123d_variadic_loft() {
    let src = r#"(model (part body (loft 50 (circle 10) (rounded_rect 20 20 4) (circle 5))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("loft(["));
    assert!(code.contains("_ecky_apply_transform(Pos(0, 0, (50.0) * 0),"));
    assert!(code.contains("_ecky_apply_transform(Pos(0, 0, (50.0) * 0.5),"));
    assert!(code.contains("_ecky_apply_transform(Pos(0, 0, (50.0) * 1),"));
}
