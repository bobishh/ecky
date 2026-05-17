use super::super::edge_ops::{
    chamfer_mesh, detect_feature_edges, fillet_mesh, filter_edges, parse_edge_selector_value,
    EdgeAxis, EdgeSelector,
};
use super::super::shared::IrMesh;
use super::{lower_core_program_to_build123d, lower_to_build123d};

fn surface_fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/cad/surface/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

fn example_fixture(name: &str) -> String {
    let path = format!(
        "{}/../model-runtime/examples/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

fn example_fixture_required(name: &str) -> String {
    let path = format!(
        "{}/../model-runtime/examples/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    assert!(
        std::path::Path::new(&path).exists(),
        "missing fixture `{}` at `{}`; add fixture or mark test pending",
        name,
        path
    );
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

fn fixture_part_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = source;
    while let Some(pos) = cursor.find("(part") {
        cursor = &cursor[pos + "(part".len()..];
        let trimmed = cursor.trim_start();
        let skip = cursor.len() - trimmed.len();
        cursor = &cursor[skip..];
        let end = cursor
            .find(|c: char| c.is_whitespace() || c == ')' || c == '(')
            .unwrap_or(cursor.len());
        let name = &cursor[..end];
        if !name.is_empty() && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
        cursor = &cursor[end..];
    }
    names
}

fn assert_tuple_contains_fixture_parts(code: &str, source: &str, fixture_name: &str) {
    let part_names = fixture_part_names(source);
    assert!(
        !part_names.is_empty(),
        "fixture `{}` should declare at least one part",
        fixture_name
    );
    for part_name in part_names {
        let marker = format!(r#"("{}", "#, part_name);
        assert!(
            code.contains(&marker),
            "missing part `{}` in tuple for fixture `{}`: {}",
            part_name,
            fixture_name,
            code
        );
    }
}

fn assert_marker_hits_at_least(code: &str, markers: &[&str], min_hits: usize, label: &str) {
    let hits = markers
        .iter()
        .filter(|marker| code.contains(**marker))
        .count();
    assert!(
        hits >= min_hits,
        "expected at least {} marker hits for {} from {:?}, got {}: {}",
        min_hits,
        label,
        markers,
        hits,
        code
    );
}

#[test]
fn lower_to_build123d_minimal_extrude() {
    let src = r#"(model (part body (extrude (rounded_rect 30 20 4) 10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("from build123d import *"), "missing import");
    assert!(
        code.contains("from _ecky_build123d_helpers import *"),
        "missing helpers import"
    );
    assert!(!code.contains("def _ecky_solid("), "inline helper leaked");
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
    assert!(code.contains("_ecky_cut_many("), "difference helper");
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
    assert!(code.contains("_r_2 = 4.0"), "inner binding: {}", code);
    assert!(
        code.contains("Circle(_r_2)"),
        "inner binding used: {}",
        code
    );
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
fn lower_to_build123d_let_star_allows_sequential_dependency() {
    let src = r#"(model
        (part body
          (let* ((a 2) (b (+ a 1)))
            (extrude (circle b) 5))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_a = 2.0"), "outer: {}", code);
    assert!(code.contains("_b = (_a + 1.0)"), "inner: {}", code);
    assert!(code.contains("Circle(_b)"), "body: {}", code);
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
fn lower_to_build123d_build_shape_param_arithmetic_wall() {
    let src = r#"(model
      (params
        (number duplo_height_blocks 5)
        (number flat_start 48)
        (number ramp_length 192)
        (number flat_end 48))
      (part body
        (build
          (shape dz (* duplo_height_blocks 19.2))
          (shape L (+ flat_start ramp_length flat_end))
          (result (box L 10 dz)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(r#"_dz = (float(params.get("duplo_height_blocks", 5.0)) * 19.2)"#),
        "mul lowering missing: {}",
        code
    );
    assert!(
        code.contains(
            r#"_L = (float(params.get("flat_start", 48.0)) + float(params.get("ramp_length", 192.0)) + float(params.get("flat_end", 48.0)))"#
        ),
        "sum lowering missing: {}",
        code
    );
    assert!(code.contains("Box(_L, 10.0, _dz"), "box missing: {}", code);
}

#[test]
fn lower_to_build123d_hygienic_let_locals_emit_valid_python_identifiers() {
    let src = r#"(model
        (params
          (number width 100)
          (number height 20)
          (number shift 5))
        (part body
          (let ((w (/ width 2))
                (h height))
            (translate shift 0 0
              (extrude
                (polygon ((0 0) ((* 2 w) 0) ((* 2 w) h) (0 h)))
                8)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        !code.contains("_##"),
        "invalid python local emitted: {}",
        code
    );
    assert!(
        !code.contains(" ##"),
        "invalid symbol leaked into python: {}",
        code
    );
    assert!(code.contains("_ecky_polygon("), "polygon missing: {}", code);
    assert!(code.contains("extrude("), "extrude missing: {}", code);
    assert!(
        code.contains("float(params.get(\"shift\", 5.0))"),
        "shift param missing: {}",
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
    assert!(code.contains("_ecky_polygon("), "polygon: {}", code);
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
    assert!(code.contains("_ecky_polygon("), "polygon: {}", code);
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
fn lower_to_build123d_path_accepts_lorenz_point_helper() {
    let src = r#"(model
        (part rail
          (path (lorenz-points 4 0.01 12))))"#;
    let code = crate::ecky_ir::lower_to_build123d(src).expect("lower");
    assert!(code.contains("Polyline("), "path: {}", code);
    assert!(
        code.contains("max((-12.0), min(12.0"),
        "chaotic helper bounds should lower into point expressions: {}",
        code
    );
}

#[test]
fn lower_to_build123d_polygon_accepts_henon_point_helper() {
    let src = r#"(model
        (part body
          (extrude (polygon (henon-points 8 10)) 2)))"#;
    let code = crate::ecky_ir::lower_to_build123d(src).expect("lower");
    assert!(code.contains("_ecky_polygon("), "polygon: {}", code);
    assert!(
        code.contains("max((-10.0), min(10.0"),
        "chaotic helper bounds should lower into point expressions: {}",
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
        err.to_string()
            .contains("not supported by current `.ecky` runtime"),
        "unexpected: {}",
        err
    );
}

#[test]
fn lower_to_build123d_union_three_parts() {
    let src = r#"(model (part compound (union (sphere 5) (cylinder 3 10) (box 4 4 4))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Sphere(5.0,"), "sphere");
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
    assert!(code.contains("_ecky_extrude("), "extrude helper");
    assert!(
        code.contains("_ecky_extrude("),
        "shell extrude should coerce sketches to faces: {}",
        code
    );
    assert!(
        code.contains("_ecky_cut_many(") || code.contains(" - "),
        "difference"
    );
}

#[test]
fn lower_to_build123d_revolve() {
    let src = r#"(model (part body (revolve (polygon ((10 0) (14 0) (14 20) (10 20))) 360)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_ecky_polygon("), "polygon");
    assert!(code.contains("Rot(90, 0, 0)"), "rotation to XZ");
    assert!(code.contains("revolve("), "revolve call");
    assert!(code.contains("revolution_arc=360.0"), "full revolution");
}

#[test]
fn lower_to_build123d_rectangle() {
    let src = r#"(model (part body (extrude (rectangle 24 12) 4)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Rectangle(24.0, 12.0)"), "rectangle: {code}");
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
    assert!(code.contains("_ecky_loft("), "loft helper");
}

#[test]
fn lower_to_build123d_taper_accepts_non_uniform_scale() {
    let src = r#"(model (part body (taper 20 0.5 0.75 (rectangle 20 10))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("_ecky_non_uniform_scale("),
        "non-uniform taper scale: {code}"
    );
    assert!(code.contains("_ecky_loft("), "loft helper: {code}");
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
    assert!(
        code.contains("_ecky_polygon(") || code.contains("Polygon(["),
        "profile loops: {}",
        code
    );
    assert!(
        !code.contains("Wire(_ecky_polygon("),
        "raw loop fallback: {}",
        code
    );
    assert!(
        code.contains("_ecky_face_with_holes("),
        "hole subtraction: {}",
        code
    );
    assert!(code.contains("sweep("), "sweep call: {}", code);
}

#[test]
fn lower_to_build123d_helical_ridge_uses_helix_sweep() {
    let src = r#"(model
        (part thread
          (helical-ridge
            :radius 10
            :pitch 2
            :height 18
            :base-width 1.2
            :crest-width 0.4
            :depth 0.7)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("_ecky_helical_ridge("),
        "helical ridge helper: {}",
        code
    );
    assert!(code.contains("Edge.make_helix("), "helix path: {}", code);
    assert!(code.contains("Polyline("), "trapezoid profile: {}", code);
    assert!(code.contains("sweep("), "sweep call: {}", code);
    assert!(
        code.contains("_ecky_helical_ridge(10.0, 2.0, 18.0, 1.2, 0.4, 0.7"),
        "positional contract: {}",
        code
    );
}

#[test]
fn lower_to_build123d_helical_ridge_female_keeps_path_radius_and_expands_envelope() {
    let src = r#"(model
        (part thread
          (helical-ridge
            :radius 10
            :pitch 2
            :height 18
            :base-width 1.2
            :crest-width 0.4
            :depth 0.7
            :female #t
            :clearance 0.15
            :lefthand #t)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("path_radius = radius"),
        "same path radius: {}",
        code
    );
    assert!(
        code.contains("envelope_clearance = clearance if female else 0.0"),
        "female envelope expansion: {}",
        code
    );
    assert!(
        code.contains("_ecky_helical_ridge(10.0, 2.0, 18.0, 1.2, 0.4, 0.7, female=True, clearance=0.15, lefthand=True)"),
        "female args: {}",
        code
    );
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
    assert!(code.contains("_ecky_path_frame("), "path-frame: {}", code);
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
    assert!(code.contains("Sphere(10.0,"), "then branch");
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
    assert!(code.contains("_ecky_face_with_holes("), "hole subtraction");
    assert!(code.contains("extrude("), "extrude");
}

#[test]
fn lower_core_program_to_build123d_ring_alias_lowers_to_profile_with_hole() {
    let src = r#"(model (part body (extrude (ring 20 10 96) 10)))"#;
    let program = crate::ecky_scheme::compile_to_core_program(src).expect("compile core program");
    let code = lower_core_program_to_build123d(&program).expect("lower core program");
    assert!(code.contains("Circle(20.0)"), "outer circle: {}", code);
    assert!(code.contains("Circle(10.0)"), "inner circle: {}", code);
    assert!(
        code.contains("_ecky_face_with_holes("),
        "ring hole: {}",
        code
    );
    assert!(code.contains("extrude("), "extrude: {}", code);
}

#[test]
fn lower_to_build123d_profile_accepts_keyword_holes_and_circle_segments() {
    let src = r#"(model
        (part body
          (extrude
            (profile :outer (circle 20 96) :holes (circle 10 96))
            10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Circle(20.0)"), "outer circle: {}", code);
    assert!(code.contains("Circle(10.0)"), "hole circle: {}", code);
    assert!(
        code.contains("_ecky_face_with_holes("),
        "profile holes: {}",
        code
    );
    assert!(code.contains("extrude("), "extrude: {}", code);
}

#[test]
fn lower_core_program_to_build123d_accepts_profile_keywords() {
    let src = r#"(model
        (part body
          (extrude
            (profile :outer (circle 20 96) :holes (circle 10 96))
            10)))"#;
    let program = crate::ecky_scheme::compile_to_core_program(src).expect("compile core program");
    let code = lower_core_program_to_build123d(&program).expect("lower core program");
    assert!(code.contains("Circle(20.0)"), "outer circle: {}", code);
    assert!(code.contains("Circle(10.0)"), "hole circle: {}", code);
    assert!(
        code.contains("_ecky_face_with_holes("),
        "profile holes: {}",
        code
    );
    assert!(code.contains("extrude("), "extrude: {}", code);
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
    assert!(code.contains("_ecky_polygon("), "polygon");
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
    assert!(code.contains("_ecky_polygon("), "polygon");
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
    assert!(
        code.contains("_ecky_select_edges("),
        "edge selection helper"
    );
    assert!(code.contains(r#"{'kind': 'all'}"#), "all selector");
    assert!(code.contains("fillet("), "fillet call");
    assert!(code.contains(", 2.0)"), "radius");
}

#[test]
fn lower_to_build123d_fillet_top_edges() {
    let src = r#"(model (part body (fillet 1.5 :edges top (box 20 20 10))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(r#"_ecky_select_edges("#)
            && code.contains(r#"{'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'z', 'bound': 'max'}]}"#),
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
        code.contains(r#"_ecky_select_edges("#)
            && code.contains(r#"{'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'z', 'bound': 'min'}]}"#),
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
        code.contains(r#"_ecky_select_edges("#) && code.contains(r#"'kind': 'axis', 'axis': 'z'"#),
        "vertical edge selection: {}",
        code
    );
}

#[test]
fn lower_to_build123d_fillet_compound_selector_edges() {
    let src = r#"(model (part body (fillet 1 :edges "x-min+z-max" (box 30 30 20))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(r#"_ecky_select_edges("#)
            && code.contains(r#"'kind': 'boundary', 'axis': 'x', 'bound': 'min'"#)
            && code.contains(r#"'kind': 'boundary', 'axis': 'z', 'bound': 'max'"#),
        "compound edge selection: {}",
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
fn edge_selector_left_box() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let edges = detect_feature_edges(&mesh);
    let left = filter_edges(&edges, EdgeSelector::Left);
    assert_eq!(left.len(), 4, "box left plane has 4 edges");
}

#[test]
fn edge_selector_front_box() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let edges = detect_feature_edges(&mesh);
    let front = filter_edges(&edges, EdgeSelector::Front);
    assert_eq!(front.len(), 4, "box front plane has 4 edges");
}

#[test]
fn edge_selector_compound_box() {
    let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
    let edges = detect_feature_edges(&mesh);
    let selector = EdgeSelector::Compound(vec![
        super::super::edge_ops::EdgeSelectorClause::Boundary {
            axis: EdgeAxis::X,
            bound: super::super::edge_ops::EdgeBound::Min,
        },
        super::super::edge_ops::EdgeSelectorClause::Boundary {
            axis: EdgeAxis::Z,
            bound: super::super::edge_ops::EdgeBound::Max,
        },
    ]);
    let compound = filter_edges(&edges, selector);
    assert_eq!(
        compound.len(),
        1,
        "x-min + z-max should isolate one box edge"
    );
}

#[test]
fn edge_selector_aliases_canonicalize() {
    let selector = parse_edge_selector_value("left+vertical").expect("parse selector");
    assert_eq!(selector.canonical_string(), "x-min+axis-z");
}

#[test]
fn edge_selector_target_ids_canonicalize() {
    let selector =
        parse_edge_selector_value("target-ids:Body:edge:0-0-0_10-0-0|Body:edge:0-0-0_0-10-0")
            .expect("parse selector");
    assert_eq!(
        selector.canonical_string(),
        "target-ids:Body:edge:0-0-0_10-0-0|Body:edge:0-0-0_0-10-0"
    );
}

#[test]
fn lower_to_build123d_supports_exact_target_id_selectors() {
    let code = lower_to_build123d(
        r#"(model (part body (fillet 1 :edges "target-id:body:edge:0:0-0-0_10-0-0" (box 10 10 10))))"#,
    )
    .expect("exact target ids should lower");
    assert!(
        code.contains(r#"_ecky_select_edges("#)
            && code.contains(r#"{'kind': 'targetIds', 'targetIds': ["body:edge:0:0-0-0_10-0-0"]}"#)
            && code.contains(r#""body""#),
        "unexpected exact selector lowering: {}",
        code
    );
}

#[test]
fn lower_to_build123d_supports_exact_face_target_id_shell_selectors() {
    let code = lower_to_build123d(
        r#"(model (part body (shell 1 :faces "target-id:body:face:5:0-0-10:100" (box 10 10 10))))"#,
    )
    .expect("exact face target ids should lower");
    assert!(
        code.contains(r#"_ecky_select_shell_faces("#)
            && code.contains(r#"{'kind': 'targetIds', 'targetIds': ["body:face:5:0-0-10:100"]}"#)
            && code.contains(r#""body""#),
        "unexpected exact shell selector lowering: {}",
        code
    );
}

#[test]
fn lower_to_build123d_supports_coarse_face_shell_selectors() {
    let code = lower_to_build123d(r#"(model (part body (shell 1 :faces "top" (box 10 10 10))))"#)
        .expect("coarse face selectors should lower");
    assert!(
        code.contains(r#"_ecky_select_shell_faces("#)
            && code.contains(
                r#"{'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'z', 'bound': 'max'}]}"#
            ),
        "unexpected coarse shell selector lowering: {}",
        code
    );
}

#[test]
fn lower_to_build123d_supports_richer_face_shell_selectors() {
    let code = lower_to_build123d(
        r#"(model (part body (shell 1 :faces "planar+normal-z+area-max" (box 10 10 10))))"#,
    )
    .expect("richer face selectors should lower");
    assert!(
        code.contains(r#"_ecky_select_shell_faces("#)
            && code.contains(
                r#"{'kind': 'clauses', 'clauses': [{'kind': 'planar'}, {'kind': 'normal', 'axis': 'z'}, {'kind': 'area', 'rank': 'max'}]}"#
            ),
        "unexpected richer shell selector lowering: {}",
        code
    );
}

#[test]
fn lower_core_program_to_build123d_supports_typed_selector_nodes() {
    let program = crate::ecky_scheme::compile_to_core_program(
        r#"(model (part body (fillet 1 :edges "target-id:body:edge:0:0-0-0_10-0-0" (box 10 10 10))))"#,
    )
    .expect("program");
    let code = lower_core_program_to_build123d(&program).expect("typed selector lower");
    assert!(
        code.contains(r#"{'kind': 'targetIds', 'targetIds': ["body:edge:0:0-0-0_10-0-0"]}"#),
        "unexpected core selector lowering: {}",
        code
    );
}

#[test]
fn lower_core_program_to_build123d_supports_coarse_selector_payload_when_value_is_bad() {
    let mut program = crate::ecky_scheme::compile_to_core_program(
        r#"(model (part body (fillet 1 :edges "left+vertical" (box 10 10 10))))"#,
    )
    .expect("program");
    let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } = &mut program.parts[0].root.kind
    else {
        panic!("expected call");
    };
    *keywords[0].source_node_mut() = crate::ecky_core_ir::CoreNode::new(
        crate::ecky_core_ir::NodeId::new(99_001),
        crate::ecky_core_ir::CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(7.0)),
        crate::ecky_core_ir::CoreValueKind::Number,
    );
    let code = lower_core_program_to_build123d(&program).expect("typed selector lower");
    assert!(
        code.contains(
            r#"{'kind': 'clauses', 'clauses': [{'kind': 'boundary', 'axis': 'x', 'bound': 'min'}, {'kind': 'axis', 'axis': 'z'}]}"#
        ),
        "unexpected core selector lowering: {}",
        code
    );
}

#[test]
fn lower_core_program_to_build123d_rejects_missing_selector_payload_on_edges_keyword() {
    let mut program = crate::ecky_scheme::compile_to_core_program(
        r#"(model (part body (fillet 1 :edges "left+vertical" (box 10 10 10))))"#,
    )
    .expect("program");
    let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } = &mut program.parts[0].root.kind
    else {
        panic!("expected call");
    };
    keywords[0].set_selector_payload(None);

    let err = lower_core_program_to_build123d(&program)
        .expect_err("missing selector payload should fail");
    assert!(
        err.to_string()
            .contains("CoreProgram `:edges` keyword requires selector payload"),
        "{err}"
    );
}

#[test]
fn lower_core_program_to_build123d_rejects_wrong_kind_selector_payload_on_edges_keyword() {
    let mut program = crate::ecky_scheme::compile_to_core_program(
        r#"(model (part body (fillet 1 :edges "left+vertical" (box 10 10 10))))"#,
    )
    .expect("program");
    let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } = &mut program.parts[0].root.kind
    else {
        panic!("expected call");
    };
    keywords[0].set_selector_payload(Some(
        crate::ecky_core_ir::CoreSelectorPayload::FaceTargetIds(vec!["body:face:0:0-0-1:1".into()]),
    ));

    let err = lower_core_program_to_build123d(&program)
        .expect_err("wrong-kind selector payload should fail");
    assert!(
        err.to_string()
            .contains("CoreProgram `:edges` keyword requires edge selector payload"),
        "{err}"
    );
}

#[test]
fn lower_to_build123d_rejects_wrong_kind_exact_selectors() {
    let edge_err = lower_to_build123d(
        r#"(model (part body (fillet 1 :edges "target-id:body:face:5:0-0-10:100" (box 10 10 10))))"#,
    )
    .expect_err("face target id should fail edge selector");
    assert!(
        edge_err
            .message
            .contains("included non-edge target id `body:face:5:0-0-10:100`"),
        "{edge_err:?}"
    );

    let face_err = lower_to_build123d(
        r#"(model (part body (shell 1 :faces "target-id:body:edge:0:0-0-0_10-0-0" (box 10 10 10))))"#,
    )
    .expect_err("edge target id should fail face selector");
    assert!(
        face_err
            .message
            .contains("included non-face target id `body:edge:0:0-0-0_10-0-0`"),
        "{face_err:?}"
    );
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
fn lower_to_build123d_tooth_rotated_cutters_fixture() {
    let code = lower_to_build123d(include_str!(
        "../../tests/fixtures/cad/surface/tooth_rotated_cutters.ecky"
    ))
    .expect("lower");
    assert!(
        code.contains("for __ecky_rc_i in range(_b0):"),
        "repeat compound loop: {}",
        code
    );
    assert!(
        code.contains("Rot(0.0, (_i * 7.5), 0.0)"),
        "per-tooth rotation: {}",
        code
    );
    assert!(
        code.contains("_ecky_cut_many(_base, _cutters)"),
        "cut cutters from base: {}",
        code
    );
}

#[test]
fn lower_to_build123d_tooth_rotated_cutters_comprehension_fixture() {
    let code = lower_to_build123d(include_str!(
        "../../tests/fixtures/cad/surface/tooth_rotated_cutters_comprehension.ecky"
    ))
    .expect("lower");
    assert!(
        code.contains("for __ecky_map_i in _b"),
        "map loop: {}",
        code
    );
    assert!(
        code.contains("range(int(math.floor(0.0)), int(math.floor("),
        "dynamic range: {}",
        code
    );
    assert!(
        code.contains("Rot(0.0, (_i * 7.5), 0.0)"),
        "per-tooth rotation: {}",
        code
    );
    assert!(
        code.contains("_ecky_cut_many(_base, *_b"),
        "spliced cutter list: {}",
        code
    );
}

#[test]
fn lower_to_build123d_supports_deterministic_fancy_helpers() {
    let code = crate::ecky_ir::lower_to_build123d(
        r#"(model
          (params (number seed 7 :label "Seed" :min 0 :max 99))
          (part body
            (union
              (extrude (polygon (organic-loop 12 20 3 seed)) 4)
              (translate (* 10 (hash01 1 2 seed)) 0 6 (box 2 2 2)))))"#,
    )
    .expect("lower");

    assert!(
        code.contains("def _ecky_hash01"),
        "hash helper preamble: {}",
        code
    );
    assert!(code.contains("_ecky_hash01("), "hash helper call: {}", code);
    assert!(code.contains("_ecky_polygon"), "organic polygon: {}", code);
}

#[test]
fn lower_to_build123d_organic_bspline_loop_fixture() {
    let source = surface_fixture("organic_bspline_loop.ecky");
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert!(code.contains("Spline("), "bspline helper: {}", code);
    assert!(code.contains("periodic=True"), "closed loop: {}", code);
    assert!(code.contains("_ecky_hash_signed("), "seeded loop: {}", code);
    assert!(code.contains("_ecky_extrude("), "surface extrude: {}", code);
}

#[test]
fn lower_to_build123d_voronoi_perforated_panel_fixture() {
    let source = surface_fixture("voronoi_perforated_panel.ecky");
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert!(code.contains("Cylinder("), "cutout cylinders: {}", code);
    assert!(
        code.contains("_ecky_fuse_many("),
        "apply union cutouts: {}",
        code
    );
    assert!(
        code.contains("_ecky_cut_many("),
        "panel perforation cut: {}",
        code
    );
    assert!(
        code.contains("_ecky_hash_signed("),
        "seeded cells: {}",
        code
    );
}

#[test]
fn lower_to_build123d_thomas_modular_ramp_fixture() {
    let code = crate::ecky_ir::lower_to_build123d(include_str!(
        "../../tests/fixtures/cad/surface/thomas_modular_ramp.ecky"
    ))
    .expect("lower");

    assert!(code.contains("_ecky_polygon("), "polygon: {}", code);
    assert!(
        code.contains("tooth_phase_shift"),
        "keeps ramp helper bindings: {}",
        code
    );
}

#[test]
fn lower_to_build123d_direct_occt_frame_array_bracket_fixture_keeps_op_mix_markers() {
    let source = surface_fixture("direct_occt_frame_array_bracket.ecky");
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert!(code.contains("Bezier("), "rail bezier path: {}", code);
    assert!(
        code.contains("_ecky_path_frame("),
        "path-frame helper: {}",
        code
    );
    assert!(code.contains("_ecky_place("), "place helper: {}", code);
    assert!(
        code.contains("_ecky_clip_box("),
        "clip-box helper: {}",
        code
    );
    assert!(
        code.contains("for __ecky_ra_i in range(1, 6)"),
        "radial-array loop: {}",
        code
    );
    assert!(
        code.contains("for __ecky_ga_r in range(2):")
            && code.contains("for __ecky_ga_c in range(3):"),
        "grid-array loops: {}",
        code
    );
    assert!(
        code.contains(r#"("bracket","#),
        "fixture part tuple should stay stable: {}",
        code
    );
}

#[test]
fn lower_to_build123d_direct_occt_snap_clip_fixture_keeps_tuple_and_curve_markers() {
    let source = surface_fixture("direct_occt_snap_clip.ecky");
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert!(
        code.contains("RectangleRounded(42.0, 24.0, 3.0)"),
        "saddle base: {}",
        code
    );
    assert!(code.contains("fillet("), "fillet call: {}", code);
    assert!(
        code.contains("_ecky_cut_many("),
        "difference helper: {}",
        code
    );
    assert!(code.contains("Bezier("), "latch curve: {}", code);
    assert!(code.contains("sweep("), "latch sweep: {}", code);
    assert!(
        code.contains(r#"("saddle","#) && code.contains(r#"("latch","#),
        "fixture parts should stay stable: {}",
        code
    );
}

#[test]
fn lower_to_build123d_wall_pattern_fixture_reports_unsupported_operation_context() {
    let source = surface_fixture("wall_pattern_cellular.ecky");
    let err = crate::ecky_ir::lower_to_build123d(&source).expect_err("wall-pattern unsupported");
    let message = err.to_string();
    assert!(
        message.contains("Node `wall-pattern` is not yet supported by the build123d lowerer")
            && message.contains("Switch backend and rerender"),
        "unexpected error context: {}",
        message
    );
}

#[test]
fn lower_to_build123d_film_scanning_adapter_helicoid_fixture() {
    let source = example_fixture("film-scanning-adapter-helicoid.ecky");
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert!(
        code.contains("_ecky_helical_ridge("),
        "helicoid helper: {}",
        code
    );
    assert!(code.contains("Edge.make_helix("), "helix path: {}", code);
    assert!(
        code.contains(r#"("top_cover_integrated_helicoid","#)
            && code.contains(r#"("moving_lens_carrier","#),
        "fixture part list should stay stable: {}",
        code
    );
}

#[test]
fn lower_to_build123d_helicoid_thread_coupon_fixture_keeps_clearance_variants() {
    let source = example_fixture("helicoid-thread-coupon.ecky");
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert!(
        code.contains("_ecky_helical_ridge("),
        "helicoid helper: {}",
        code
    );
    assert!(code.contains("Edge.make_helix("), "helix path: {}", code);
    assert!(
        code.contains(r#"("coupon_male_020","#)
            && code.contains(r#"("coupon_female_020","#)
            && code.contains(r#"("coupon_male_025","#)
            && code.contains(r#"("coupon_female_025","#)
            && code.contains(r#"("coupon_male_030","#)
            && code.contains(r#"("coupon_female_030","#)
            && code.contains(r#"("coupon_male_035","#)
            && code.contains(r#"("coupon_female_035","#),
        "fixture part list should stay stable: {}",
        code
    );
    assert!(
        code.contains("clearance=0.2")
            && code.contains("clearance=0.25")
            && code.contains("clearance=0.3")
            && code.contains("clearance=0.35"),
        "female clearance variants should lower into helper calls: {}",
        code
    );
}

#[test]
fn lower_to_build123d_film_adapter_film_gap_coupon_fixture_keeps_boolean_and_part_tuple() {
    let source = example_fixture("film-adapter-film-gap-coupon.ecky");
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert!(
        code.contains(r#"float(params.get("film_gap", 0.35))"#),
        "film gap param should lower through params map: {}",
        code
    );
    assert!(
        code.contains("_ecky_apply_transform("),
        "translate helper: {}",
        code
    );
    assert!(
        code.contains("_ecky_cut_many("),
        "difference helper: {}",
        code
    );
    assert!(
        code.contains(r#"("film_gate","#) && code.contains(r#"("lens_adapter","#),
        "fixture part tuple should stay stable: {}",
        code
    );
}

#[test]
fn lower_to_build123d_film_path_gap_coupon_fixture_keeps_gap_variants_and_transforms() {
    let source = example_fixture("film-path-gap-coupon.ecky");
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert!(
        code.contains("_ecky_cut_many("),
        "difference helper: {}",
        code
    );
    assert!(
        code.contains("Pos(0.0, 0.0, 0.2)") && code.contains("Pos(0.0, 0.0, 0.1)"),
        "translate variants for lower/upper guides: {}",
        code
    );
    assert!(
        code.contains("Box(84.0, 0.35, 2.0")
            && code.contains("Box(84.0, 0.45, 2.0")
            && code.contains("Box(84.0, 0.55, 2.0"),
        "gap variants should lower into strip pass solids: {}",
        code
    );
    assert!(
        code.contains(r#"("film_path_lower_035","#)
            && code.contains(r#"("film_path_upper_clamp_035","#)
            && code.contains(r#"("film_path_lower_045","#)
            && code.contains(r#"("film_path_upper_clamp_045","#)
            && code.contains(r#"("film_path_lower_055","#)
            && code.contains(r#"("film_path_upper_clamp_055","#),
        "fixture part tuple should stay stable: {}",
        code
    );
}

#[test]
fn lower_to_build123d_dovetail_box_fixture_keeps_tuple_and_boolean_markers() {
    let fixture_name = "dovetail-box.ecky";
    let source = example_fixture_required(fixture_name);
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert_tuple_contains_fixture_parts(&code, &source, fixture_name);
    assert_marker_hits_at_least(
        &code,
        &[
            "_ecky_cut_many(",
            "_ecky_apply_transform(",
            "_ecky_fuse_many(",
        ],
        2,
        fixture_name,
    );
}

#[test]
fn lower_to_build123d_vermicomposter_lid_clearance_fixture_keeps_tuple_and_clearance_markers() {
    let fixture_name = "vermicomposter-lid-clearance.ecky";
    let source = example_fixture_required(fixture_name);
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert_tuple_contains_fixture_parts(&code, &source, fixture_name);
    assert_marker_hits_at_least(
        &code,
        &[
            r#"float(params.get("lid_clearance","#,
            "float(params.get(",
            "_ecky_cut_many(",
            "_ecky_apply_transform(",
        ],
        2,
        fixture_name,
    );
}

#[test]
fn lower_to_build123d_snap_hook_coupon_fixture_keeps_tuple_and_snap_markers() {
    let fixture_name = "snap-hook-coupon.ecky";
    let source = example_fixture_required(fixture_name);
    let code = crate::ecky_ir::lower_to_build123d(&source).expect("lower");

    assert_tuple_contains_fixture_parts(&code, &source, fixture_name);
    assert_marker_hits_at_least(
        &code,
        &[
            "Bezier(",
            "sweep(",
            "fillet(",
            "_ecky_cut_many(",
            "_ecky_apply_transform(",
        ],
        2,
        fixture_name,
    );
}

#[test]
fn lower_core_program_to_build123d_matches_public_entrypoint_for_comprehension_fixture() {
    let source =
        include_str!("../../tests/fixtures/cad/surface/tooth_rotated_cutters_comprehension.ecky");
    let program = crate::ecky_scheme::try_compile_to_core_program(source)
        .expect("compiled path")
        .expect("program");
    let direct = lower_core_program_to_build123d(&program).expect("direct");
    let public = crate::ecky_ir::lower_to_build123d(source).expect("public");

    assert_eq!(direct, public);
    assert!(
        direct.contains("for __ecky_map_") && direct.contains(" in _b"),
        "map loop: {}",
        direct
    );
    assert!(
        direct.contains("range(int(math.floor(0.0)), int(math.floor("),
        "dynamic range: {}",
        direct
    );
    assert!(
        direct.contains("_ecky_cut_many(_base, *_b"),
        "spliced cutter list: {}",
        direct
    );
}

#[test]
fn lower_core_program_to_build123d_supports_text_params_without_legacy_model_bridge() {
    use crate::ecky_core_ir::{
        CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CoreParameter,
        CoreParameterConstraints, CoreParameterKind, CoreParameterValue, CorePart, CorePrimitive,
        CoreProgram, CoreReference, CoreValueKind, NodeId, ParamId, PartId, ProgramId,
    };

    let label_id = ParamId::new(1);
    let root = CoreNode::new(
        NodeId::new(10),
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Text),
            args: vec![
                CoreNode::new(
                    NodeId::new(11),
                    CoreNodeKind::Reference(CoreReference::Parameter(label_id)),
                    CoreValueKind::Text,
                ),
                CoreNode::new(
                    NodeId::new(12),
                    CoreNodeKind::Literal(CoreLiteral::Number(10.0)),
                    CoreValueKind::Number,
                ),
            ],
            keywords: vec![],
        },
        CoreValueKind::Sketch,
    );
    let program = CoreProgram::new(
        ProgramId::new(1),
        vec![CoreParameter {
            id: label_id,
            key: "label".into(),
            label: "Label".into(),
            kind: CoreParameterKind::Text,
            default_value: CoreParameterValue::Text("hello".into()),
            frozen: false,
            constraints: CoreParameterConstraints::default(),
        }],
        vec![CorePart {
            id: PartId::new(2),
            key: "body".into(),
            label: "Body".into(),
            root,
        }],
    );

    let code = lower_core_program_to_build123d(&program).expect("lower");

    assert!(code.contains("Text("), "text primitive: {}", code);
    assert!(
        code.contains(r#"str(params.get("label", "hello"))"#),
        "text param default: {}",
        code
    );
}

#[test]
fn lower_core_program_to_build123d_keeps_root_build_and_let_bindings_distinct() {
    use crate::ecky_core_ir::{
        CoreBinding, CoreBooleanOp, CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CorePart,
        CorePrimitive, CoreProgram, CoreReference, CoreShapeBinding, CoreTransformOp,
        CoreValueKind, NodeId, PartId, ProgramId,
    };

    fn num(id: u64, value: f64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Literal(CoreLiteral::Number(value)),
            CoreValueKind::Number,
        )
    }

    fn local(id: u64, name: &str, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Local(name.into())),
            kind,
        )
    }

    fn node_ref(id: u64, target: u64, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Node(NodeId::new(target))),
            kind,
        )
    }

    fn call(id: u64, op: CoreOperation, args: Vec<CoreNode>, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Call {
                op,
                args,
                keywords: vec![],
            },
            kind,
        )
    }

    let left_box = call(
        10,
        CoreOperation::Primitive(CorePrimitive::Box),
        vec![num(11, 1.0), num(12, 1.0), num(13, 1.0)],
        CoreValueKind::Solid,
    );

    let right_box = call(
        20,
        CoreOperation::Transform(CoreTransformOp::Translate),
        vec![
            num(21, 10.0),
            num(22, 0.0),
            num(23, 0.0),
            call(
                24,
                CoreOperation::Primitive(CorePrimitive::Box),
                vec![num(25, 1.0), num(26, 1.0), num(27, 1.0)],
                CoreValueKind::Solid,
            ),
        ],
        CoreValueKind::Solid,
    );

    let body = call(
        40,
        CoreOperation::Boolean(CoreBooleanOp::Union),
        vec![
            call(
                41,
                CoreOperation::Transform(CoreTransformOp::Translate),
                vec![
                    local(42, "shift/one", CoreValueKind::Number),
                    num(43, 0.0),
                    num(44, 0.0),
                    node_ref(45, 10, CoreValueKind::Solid),
                ],
                CoreValueKind::Solid,
            ),
            call(
                46,
                CoreOperation::Transform(CoreTransformOp::Translate),
                vec![
                    local(47, "shift-one", CoreValueKind::Number),
                    num(48, 0.0),
                    num(49, 0.0),
                    node_ref(50, 20, CoreValueKind::Solid),
                ],
                CoreValueKind::Solid,
            ),
        ],
        CoreValueKind::Solid,
    );

    let root = CoreNode::new(
        NodeId::new(60),
        CoreNodeKind::Build {
            bindings: vec![
                CoreShapeBinding {
                    name: "left/box".into(),
                    value: left_box,
                },
                CoreShapeBinding {
                    name: "left-box".into(),
                    value: right_box,
                },
            ],
            result: Box::new(CoreNode::new(
                NodeId::new(61),
                CoreNodeKind::Let {
                    bindings: vec![
                        CoreBinding {
                            name: "shift/one".into(),
                            value: num(62, 1.0),
                        },
                        CoreBinding {
                            name: "shift-one".into(),
                            value: num(63, 2.0),
                        },
                    ],
                    body: Box::new(body),
                },
                CoreValueKind::Solid,
            )),
        },
        CoreValueKind::Solid,
    );

    let program = CoreProgram::new(
        ProgramId::new(1),
        vec![],
        vec![CorePart {
            id: PartId::new(2),
            key: "body".into(),
            label: "Body".into(),
            root,
        }],
    );

    let code = lower_core_program_to_build123d(&program).expect("lower");

    assert!(
        code.contains("_left_box = _v0"),
        "slash build binding collapsed: {}",
        code
    );
    assert!(
        code.contains("_left_box_2 = _v2"),
        "hyphen build binding missing: {}",
        code
    );
    assert!(
        code.contains("_shift_one = 1.0"),
        "slash let binding collapsed: {}",
        code
    );
    assert!(
        code.contains("_shift_one_2 = 2.0"),
        "hyphen let binding missing: {}",
        code
    );
    assert!(
        code.contains("_ecky_apply_transform(Pos(_shift_one, 0.0, 0.0), _left_box)"),
        "first branch captured wrong build/let bindings: {}",
        code
    );
    assert!(
        code.contains("_ecky_apply_transform(Pos(_shift_one_2, 0.0, 0.0), _left_box_2)"),
        "second branch captured wrong build/let bindings: {}",
        code
    );
}

#[test]
fn lower_core_program_to_build123d_materializes_direct_list_points() {
    use crate::ecky_core_ir::{
        CoreBinding, CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CorePart, CorePrimitive,
        CoreProgram, CoreReference, CoreSurfaceOp, CoreValueKind, NodeId, PartId, ProgramId,
    };

    fn num(id: u64, value: f64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Literal(CoreLiteral::Number(value)),
            CoreValueKind::Number,
        )
    }

    fn local(id: u64, name: &str) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Local(name.into())),
            CoreValueKind::Number,
        )
    }

    fn point2(id: u64, x: CoreNode, y: CoreNode) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::List(vec![x, y]),
            CoreValueKind::Point2,
        )
    }

    fn call(id: u64, op: CoreOperation, args: Vec<CoreNode>, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Call {
                op,
                args,
                keywords: vec![],
            },
            kind,
        )
    }

    let pts = CoreNode::new(
        NodeId::new(20),
        CoreNodeKind::List(vec![
            point2(21, local(22, "x"), num(23, 2.0)),
            point2(24, num(25, 3.0), num(26, 4.0)),
        ]),
        CoreValueKind::List,
    );
    let root = CoreNode::new(
        NodeId::new(30),
        CoreNodeKind::Let {
            bindings: vec![CoreBinding {
                name: "x".into(),
                value: num(31, 1.0),
            }],
            body: Box::new(CoreNode::new(
                NodeId::new(36),
                CoreNodeKind::Let {
                    bindings: vec![CoreBinding {
                        name: "pts".into(),
                        value: pts,
                    }],
                    body: Box::new(call(
                        32,
                        CoreOperation::Surface(CoreSurfaceOp::Extrude),
                        vec![
                            call(
                                33,
                                CoreOperation::Primitive(CorePrimitive::Polygon),
                                vec![CoreNode::new(
                                    NodeId::new(34),
                                    CoreNodeKind::Reference(CoreReference::Local("pts".into())),
                                    CoreValueKind::List,
                                )],
                                CoreValueKind::Sketch,
                            ),
                            num(35, 5.0),
                        ],
                        CoreValueKind::Solid,
                    )),
                },
                CoreValueKind::Solid,
            )),
        },
        CoreValueKind::Solid,
    );
    let program = CoreProgram::new(
        ProgramId::new(1),
        vec![],
        vec![CorePart {
            id: PartId::new(2),
            key: "body".into(),
            label: "Body".into(),
            root,
        }],
    );

    let code = lower_core_program_to_build123d(&program).expect("lower");

    assert!(code.contains("_ecky_polygon("), "polygon: {}", code);
    assert!(code.contains("_x = 1.0"), "x binding: {}", code);
    assert!(code.contains("(_x, 2.0)"), "first point: {}", code);
    assert!(code.contains("(3.0, 4.0)"), "second point: {}", code);
}

#[test]
fn lower_core_program_to_build123d_materializes_let_point_node_refs() {
    use crate::ecky_core_ir::{
        CoreBinding, CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CorePart, CorePrimitive,
        CoreProgram, CoreReference, CoreSurfaceOp, CoreValueKind, NodeId, PartId, ProgramId,
    };

    fn num(id: u64, value: f64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Literal(CoreLiteral::Number(value)),
            CoreValueKind::Number,
        )
    }

    fn node_ref(id: u64, target: u64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Node(NodeId::new(target))),
            CoreValueKind::Number,
        )
    }

    fn call(id: u64, op: CoreOperation, args: Vec<CoreNode>, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Call {
                op,
                args,
                keywords: vec![],
            },
            kind,
        )
    }

    let first_point = CoreNode::new(
        NodeId::new(20),
        CoreNodeKind::Let {
            bindings: vec![CoreBinding {
                name: "x".into(),
                value: num(21, 1.0),
            }],
            body: Box::new(CoreNode::new(
                NodeId::new(22),
                CoreNodeKind::List(vec![node_ref(23, 21), num(24, 2.0)]),
                CoreValueKind::Point2,
            )),
        },
        CoreValueKind::Point2,
    );
    let points = CoreNode::new(
        NodeId::new(30),
        CoreNodeKind::List(vec![
            first_point,
            CoreNode::new(
                NodeId::new(31),
                CoreNodeKind::List(vec![num(32, 3.0), num(33, 4.0)]),
                CoreValueKind::Point2,
            ),
        ]),
        CoreValueKind::List,
    );
    let root = call(
        40,
        CoreOperation::Surface(CoreSurfaceOp::Extrude),
        vec![
            call(
                41,
                CoreOperation::Primitive(CorePrimitive::Polygon),
                vec![points],
                CoreValueKind::Sketch,
            ),
            num(42, 5.0),
        ],
        CoreValueKind::Solid,
    );
    let program = CoreProgram::new(
        ProgramId::new(1),
        vec![],
        vec![CorePart {
            id: PartId::new(2),
            key: "body".into(),
            label: "Body".into(),
            root,
        }],
    );

    let code = lower_core_program_to_build123d(&program).expect("lower");

    assert!(code.contains("_ecky_polygon("), "polygon: {}", code);
    assert!(code.contains("(1.0, 2.0)"), "first point: {}", code);
    assert!(code.contains("(3.0, 4.0)"), "second point: {}", code);
}

#[test]
fn lower_core_program_to_build123d_group_prefix_let_shadow_does_not_leak_into_tail() {
    use crate::ecky_core_ir::{
        CoreBinding, CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CorePart, CorePrimitive,
        CoreProgram, CoreReference, CoreShapeBinding, CoreTransformOp, CoreValueKind, NodeId,
        PartId, ProgramId,
    };

    fn num(id: u64, value: f64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Literal(CoreLiteral::Number(value)),
            CoreValueKind::Number,
        )
    }

    fn local(id: u64, name: &str, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Local(name.into())),
            kind,
        )
    }

    fn node_ref(id: u64, target: u64, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Node(NodeId::new(target))),
            kind,
        )
    }

    fn call(id: u64, op: CoreOperation, args: Vec<CoreNode>, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Call {
                op,
                args,
                keywords: vec![],
            },
            kind,
        )
    }

    let base = call(
        10,
        CoreOperation::Primitive(CorePrimitive::Box),
        vec![num(11, 1.0), num(12, 1.0), num(13, 1.0)],
        CoreValueKind::Solid,
    );
    let prefix = CoreNode::new(
        NodeId::new(20),
        CoreNodeKind::Let {
            bindings: vec![CoreBinding {
                name: "shift".into(),
                value: num(21, 2.0),
            }],
            body: Box::new(call(
                22,
                CoreOperation::Transform(CoreTransformOp::Translate),
                vec![
                    local(23, "shift", CoreValueKind::Number),
                    num(24, 0.0),
                    num(25, 0.0),
                    node_ref(26, 10, CoreValueKind::Solid),
                ],
                CoreValueKind::Solid,
            )),
        },
        CoreValueKind::Solid,
    );
    let tail = call(
        30,
        CoreOperation::Transform(CoreTransformOp::Translate),
        vec![
            local(31, "shift", CoreValueKind::Number),
            num(32, 0.0),
            num(33, 0.0),
            node_ref(34, 10, CoreValueKind::Solid),
        ],
        CoreValueKind::Solid,
    );
    let root = CoreNode::new(
        NodeId::new(40),
        CoreNodeKind::Build {
            bindings: vec![CoreShapeBinding {
                name: "body".into(),
                value: base,
            }],
            result: Box::new(CoreNode::new(
                NodeId::new(41),
                CoreNodeKind::Let {
                    bindings: vec![CoreBinding {
                        name: "shift".into(),
                        value: num(42, 1.0),
                    }],
                    body: Box::new(CoreNode::new(
                        NodeId::new(43),
                        CoreNodeKind::Group(vec![prefix, tail]),
                        CoreValueKind::Solid,
                    )),
                },
                CoreValueKind::Solid,
            )),
        },
        CoreValueKind::Solid,
    );
    let program = CoreProgram::new(
        ProgramId::new(1),
        vec![],
        vec![CorePart {
            id: PartId::new(2),
            key: "body".into(),
            label: "Body".into(),
            root,
        }],
    );

    let code = lower_core_program_to_build123d(&program).expect("lower");

    assert!(
        code.contains("_body = _v0"),
        "outer build binding: {}",
        code
    );
    assert!(code.contains("_shift = 1.0"), "outer let binding: {}", code);
    assert!(
        code.contains("_shift_2 = 2.0"),
        "inner let shadow: {}",
        code
    );
    assert_eq!(
        code.matches("_ecky_apply_transform(").count(),
        2,
        "group should emit prefix and tail transforms: {}",
        code
    );

    let prefix_pos = code
        .find("Pos(_shift_2, 0.0, 0.0), _body)")
        .expect("prefix transform");
    let tail_pos = code
        .find("Pos(_shift, 0.0, 0.0), _body)")
        .expect("tail transform");

    assert!(
        prefix_pos < tail_pos,
        "group order/scope restore wrong: {}",
        code
    );
}

#[test]
fn lower_core_program_to_build123d_direct_if_keeps_branch_scopes_separate() {
    use crate::ecky_core_ir::{
        CoreBinding, CoreLiteral, CoreNode, CoreNodeKind, CoreOperation, CorePart, CorePrimitive,
        CoreProgram, CoreReference, CoreShapeBinding, CoreTransformOp, CoreValueKind, NodeId,
        PartId, ProgramId,
    };

    fn num(id: u64, value: f64) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Literal(CoreLiteral::Number(value)),
            CoreValueKind::Number,
        )
    }

    fn bool_lit(id: u64, value: bool) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Literal(CoreLiteral::Boolean(value)),
            CoreValueKind::Boolean,
        )
    }

    fn local(id: u64, name: &str, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Local(name.into())),
            kind,
        )
    }

    fn node_ref(id: u64, target: u64, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Reference(CoreReference::Node(NodeId::new(target))),
            kind,
        )
    }

    fn call(id: u64, op: CoreOperation, args: Vec<CoreNode>, kind: CoreValueKind) -> CoreNode {
        CoreNode::new(
            NodeId::new(id),
            CoreNodeKind::Call {
                op,
                args,
                keywords: vec![],
            },
            kind,
        )
    }

    let base = call(
        10,
        CoreOperation::Primitive(CorePrimitive::Box),
        vec![num(11, 1.0), num(12, 1.0), num(13, 1.0)],
        CoreValueKind::Solid,
    );
    let then_branch = CoreNode::new(
        NodeId::new(20),
        CoreNodeKind::Let {
            bindings: vec![CoreBinding {
                name: "shift".into(),
                value: num(21, 2.0),
            }],
            body: Box::new(call(
                22,
                CoreOperation::Transform(CoreTransformOp::Translate),
                vec![
                    local(23, "shift", CoreValueKind::Number),
                    num(24, 0.0),
                    num(25, 0.0),
                    node_ref(26, 10, CoreValueKind::Solid),
                ],
                CoreValueKind::Solid,
            )),
        },
        CoreValueKind::Solid,
    );
    let else_branch = CoreNode::new(
        NodeId::new(30),
        CoreNodeKind::Let {
            bindings: vec![CoreBinding {
                name: "shift".into(),
                value: num(31, 4.0),
            }],
            body: Box::new(call(
                32,
                CoreOperation::Transform(CoreTransformOp::Translate),
                vec![
                    local(33, "shift", CoreValueKind::Number),
                    num(34, 0.0),
                    num(35, 0.0),
                    node_ref(36, 10, CoreValueKind::Solid),
                ],
                CoreValueKind::Solid,
            )),
        },
        CoreValueKind::Solid,
    );
    let root = CoreNode::new(
        NodeId::new(40),
        CoreNodeKind::Build {
            bindings: vec![CoreShapeBinding {
                name: "body".into(),
                value: base,
            }],
            result: Box::new(CoreNode::new(
                NodeId::new(41),
                CoreNodeKind::Let {
                    bindings: vec![CoreBinding {
                        name: "cap".into(),
                        value: bool_lit(42, true),
                    }],
                    body: Box::new(CoreNode::new(
                        NodeId::new(43),
                        CoreNodeKind::If {
                            condition: Box::new(local(44, "cap", CoreValueKind::Boolean)),
                            then_branch: Box::new(then_branch),
                            else_branch: Box::new(else_branch),
                        },
                        CoreValueKind::Solid,
                    )),
                },
                CoreValueKind::Solid,
            )),
        },
        CoreValueKind::Solid,
    );
    let program = CoreProgram::new(
        ProgramId::new(1),
        vec![],
        vec![CorePart {
            id: PartId::new(2),
            key: "body".into(),
            label: "Body".into(),
            root,
        }],
    );

    let code = lower_core_program_to_build123d(&program).expect("lower");

    assert!(code.contains("_body = _v0"), "build binding: {}", code);
    assert!(code.contains("_cap = True"), "condition binding: {}", code);
    assert!(code.contains("if _cap:"), "direct conditional: {}", code);
    assert!(
        code.contains("_shift = 2.0"),
        "then branch binding: {}",
        code
    );
    assert!(
        code.contains("_shift_2 = 4.0"),
        "else branch binding: {}",
        code
    );
    assert!(
        code.contains("Pos(_shift, 0.0, 0.0), _body"),
        "then branch uses then scope: {}",
        code
    );
    assert!(
        code.contains("Pos(_shift_2, 0.0, 0.0), _body"),
        "else branch uses else scope: {}",
        code
    );
}

#[test]
fn lower_to_build123d_text() {
    let src = r#"(model (part body (text "hello" 10)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("Text(\"hello\", font_size=10.0)"));
}

#[test]
fn lower_to_build123d_begin_sequence_returns_last_geometry() {
    let src = r#"
        (define (finish-box)
          (begin
            (box 1 1 1)
            (translate 2 0 0 (box 1 1 1))))
        (model
          (part body
            (finish-box)))
    "#;
    let program = crate::ecky_scheme::try_compile_to_core_program(src)
        .expect("compiled path")
        .expect("program");
    let model = crate::ecky_ir::model::core_program_to_model(&program).expect("model");
    assert_eq!(
        model.parts[0].value_kind,
        Some(crate::ecky_core_ir::CoreValueKind::Solid)
    );
    let code = lower_core_program_to_build123d(&program).expect("lower");
    assert!(code.contains("Box(1.0, 1.0, 1.0"), "box: {}", code);
    assert!(
        code.contains("Pos(2.0, 0.0, 0.0)"),
        "translate from final begin form: {}",
        code
    );
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

#[test]
fn lower_to_build123d_sampled_radial_loft() {
    let src = r#"
        (model
          (part body
            (sampled-radial-loft
              (theta z fz)
              :height 40
              :z-steps 6
              :theta-steps 24
              :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
              :z-map (+ z (* fz 2)))))"#;
    let code = crate::ecky_ir::lower_to_build123d(src).expect("lower");
    assert!(code.contains("_zi in range("), "{code}");
    assert!(code.contains("_ti in range("), "{code}");
    assert!(code.contains("math.cos("), "{code}");
    assert!(code.contains("math.sin("), "{code}");
    assert!(code.contains("_ecky_face(Polygon("), "{code}");
    assert!(code.contains("Pos(0, 0,"), "{code}");
    assert!(code.contains("loft("), "{code}");
}

#[test]
fn lower_to_build123d_shell_sampled_radial_loft() {
    let src = r#"
        (model
          (part body
            (shell 2
              (sampled-radial-loft
                (theta z fz)
                :height 40
                :z-steps 6
                :theta-steps 24
                :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                :z-map (+ z (* fz 2))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.matches("loft(").count() >= 2, "{code}");
    assert!(code.contains("math.sin("), "{code}");
    assert!(code.contains(" - "), "{code}");
}

#[test]
fn lower_to_build123d_extrude_supports_symmetric_flag() {
    let src =
        r#"(model (part body (extrude (polygon ((0 0) (10 0) (10 5) (0 5))) 8 :symmetric #t)))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(code.contains("_ecky_extrude("), "extrude helper: {}", code);
    assert!(code.contains("True"), "symmetric flag: {}", code);
    assert!(
        code.contains("_ecky_extrude(_v0, 8.0, True)"),
        "centered offset: {}",
        code
    );
}

#[test]
fn lower_to_build123d_scale_accepts_literal_non_uniform_scale() {
    let src = r#"(model (part body (scale 2 1 3 (box 10 10 10))))"#;
    let code = lower_to_build123d(src).expect("non-uniform scale should lower");
    assert!(
        code.contains("_ecky_non_uniform_scale("),
        "helper call missing: {}",
        code
    );
}

#[test]
fn lower_to_build123d_bezier_path_rejects_bad_static_point_count() {
    let src = r#"(model (part body (sweep (circle 2) (bezier-path ((0 0 0) (1 0 0) (2 0 0) (3 0 0) (4 0 0))))))"#;
    let err = lower_to_build123d(src).expect_err("bad bezier count should fail");
    assert!(
        err.to_string()
            .contains("`bezier-path` expects 3n+1 control points"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn lower_to_build123d_primitives_support_align_keyword() {
    let src = r#"(model
      (part body
        (union
          (box 10 20 30 :align '(min center max))
          (translate 20 0 0 (cylinder 5 12 :align '(max min center)))
          (translate 40 0 0 (sphere 6 :align '(min max center)))
          (translate 60 0 0 (cone 8 4 12 :align '(center max min))))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains("align=(Align.MIN, Align.CENTER, Align.MAX)"),
        "box align: {}",
        code
    );
    assert!(
        code.contains("align=(Align.MAX, Align.MIN, Align.CENTER)"),
        "cylinder align: {}",
        code
    );
    assert!(
        code.contains("align=(Align.MIN, Align.MAX, Align.CENTER)"),
        "sphere align: {}",
        code
    );
    assert!(
        code.contains("align=(Align.CENTER, Align.MAX, Align.MIN)"),
        "cone align: {}",
        code
    );
}

#[test]
fn lower_to_build123d_supports_plane_location_and_place() {
    let src = r#"(model
      (part body
        (build
          (shape base (plane :origin (10 20 30) :x (0 1 0) :normal (0 0 1)))
          (shape peg (box 4 4 4))
          (shape pose (location base :offset (5 0 0) :rotate (0 90 0)))
          (result (place pose peg)))))"#;
    let code = lower_to_build123d(src).expect("lower");
    assert!(
        code.contains(
            "Plane(origin=(10.0, 20.0, 30.0), x_dir=(0.0, 1.0, 0.0), z_dir=(0.0, 0.0, 1.0))"
        ),
        "plane: {}",
        code
    );
    assert!(
        code.contains("_ecky_location("),
        "location helper: {}",
        code
    );
    assert!(code.contains("_ecky_place("), "place helper: {}", code);
}
