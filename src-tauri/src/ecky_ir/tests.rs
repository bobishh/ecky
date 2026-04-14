#[cfg(test)]
mod tests {
    use super::*;

    fn render_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ecky-ir-test-{}", uuid::Uuid::new_v4()))
    }

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl crate::models::PathResolver for TestResolver {
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

    #[test]
    fn derive_controls_round_trips_basic_params() {
        let parsed = derive_controls(
            r#"(model
                (params
                  (number width 120 :min 20 :max 300 :step 1 :label "Width")
                  (toggle vents #t :label "Vents")
                  (image litho "" :label "Litho"))
                (part body (cylinder 20 80 32)))"#,
        )
        .expect("controls");
        assert_eq!(parsed.fields.len(), 3);
        assert_eq!(parsed.params.get("width"), Some(&ParamValue::Number(120.0)));
        assert_eq!(parsed.params.get("vents"), Some(&ParamValue::Boolean(true)));
    }

    #[test]
    fn derive_controls_reads_steel_source_without_legacy_emit() {
        let parsed = derive_controls(
            r#"
            (define base-radius 14)
            (model
              (params
                (number radius base-radius :label "Radius")
                (toggle vents true :label "Vents"))
              (part body (extrude (circle radius) 20)))
            "#,
        )
        .expect("controls");

        assert_eq!(parsed.fields.len(), 2);
        assert_eq!(parsed.params.get("radius"), Some(&ParamValue::Number(14.0)));
        assert_eq!(parsed.params.get("vents"), Some(&ParamValue::Boolean(true)));
    }

    #[test]
    fn render_model_supports_boolean_mesh_pipeline() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (params
                  (number radius 24)
                  (number wall 2)
                  (number height 80))
                (part body
                  (difference
                    (cylinder radius height 48)
                    (translate 0 0 wall
                      (cylinder (- radius wall) height 48)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");
        assert_eq!(bundle.engine_kind, EngineKind::EckyIrV0);
        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert!(!bundle.viewer_assets.is_empty());
    }

    #[test]
    fn render_model_accepts_steel_source_without_legacy_emit() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"
            (define (cup-body radius height)
              (extrude (circle radius) height))

            (model
              (params (number radius 12))
              (part body (cup-body radius 30)))
            "#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.engine_kind, EngineKind::EckyIrV0);
        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert_eq!(bundle.viewer_assets.len(), 1);
    }

    #[test]
    fn render_model_reports_unsupported_nodes_explicitly() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let err = render_model(
            r#"(model
                (part body
                  (lithophane "todo")))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect_err("unsupported");
        assert!(
            err.message.contains("Unsupported by Ecky IR v0"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn render_model_supports_loft_taper_and_twist_nodes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part lofted
                  (translate -50 0 0
                    (loft 28
                      (rounded_rect 24 18 4 12)
                      (scale 0.55 0.75 1 (rounded_rect 24 18 4 12)))))
                (part tapered
                  (taper 32 0.45 0.7
                    (circle 12 40)))
                (part twisted
                  (translate 50 0 0
                    (twist 36 120 10
                      (rounded_rect 12 8 2 8)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 3);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_supports_mirror_grid_arc_and_xor_nodes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part body
                  (union
                    (arc-array 5 26 -45 45
                      (box 4 4 12))
                    (grid-array 2 3 14 10
                      (mirror x 0
                        (xor
                          (translate 0 0 2 (cylinder 8 16 36))
                          (box 10 10 10)))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_supports_offset_and_shell_nodes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part ring
                  (extrude
                    (difference
                      (offset-rounded 4 (circle 10 32))
                      (circle 10 32))
                    8))
                (part shell-a
                  (translate 32 0 0
                    (shell 2
                      (cylinder 14 28 48))))
                (part shell-b
                  (translate -32 0 0
                    (shell 1.5
                      (extrude
                        (rounded_rect 18 12 3 10)
                        26)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 3);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_supports_wall_pattern_modes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part ribs
                  (wall-pattern
                    (:mode ribs :depth 1.2 :uFreq 14 :softness 0.12)
                    (shell 1.2 (cylinder 18 42 48))))
                (part rings
                  (translate 45 0 0
                    (wall-pattern
                      (:mode rings :depth 1.0 :vFreq 10 :rimFade 0.14)
                      (extrude (rounded_rect 20 14 3 12) 36))))
                (part spiral
                  (translate -45 0 0
                    (wall-pattern
                      (:mode spiral :depth 1.1 :uFreq 11 :twistDeg 180)
                      (revolve
                        (polygon ((10 0) (14 0) (14 28) (10 28)))
                        360 48))))
                (part diamond
                  (translate 0 48 0
                    (wall-pattern
                      (:mode diamond :depth 0.8 :uFreq 12 :vFreq 8)
                      (taper 30 0.6 0.8 (rounded_rect 18 12 2 10)))))
                (part hammered
                  (translate 0 -48 0
                    (wall-pattern
                      (:mode hammered :depth 0.7 :uFreq 9 :vFreq 9 :seed 4)
                      (twist 32 120 10 (rounded_rect 14 10 2 8))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 5);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn wall_pattern_rejects_non_shell_surface_targets() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let err = render_model(
            r#"(model
                (part body
                  (wall-pattern
                    (:mode ribs :depth 1)
                    (box 20 20 20))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect_err("unsupported");

        assert!(
            err.to_string().contains("wall-pattern"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn render_model_supports_hole_aware_sweeps_and_new_primitives() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part complex-profile
                  (extrude
                    (profile
                      (:outer ((0 20) (19 6) (12 -16) (-12 -16) (-19 6)))
                      (:holes ((0 0) (5 0) (5 5) (0 5))))
                    10))
                (part rounded-bspline
                  (translate 50 0 0
                    (loft 20
                      (rounded-polygon ((0 10) (10 0) (0 -10) (-10 0)) 2 8)
                      (bspline ((0 5) (5 0) (0 -5) (-5 0)) #t 12))))
                (part twisted-hollow
                  (translate -50 0 0
                    (shell 2
                      (twist 40 90 12
                        (profile
                          (:outer ((0 15) (15 0) (0 -15) (-15 0)))
                          (:holes ((0 0) (5 0) (5 5) (0 5))))))))
                (part tapered-hollow
                  (translate 0 50 0
                    (shell 1.5
                      (taper 30 0.5 0.5
                        (profile
                          (:outer (circle 15 32))
                          (:holes (circle 8 16))))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 4);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_supports_wall_pattern_on_complex_shell_sweeps() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part vase
                  (wall-pattern (:mode ribs :depth 1.5 :uFreq 12)
                    (shell 2
                      (twist 60 45 12
                        (profile
                          (:outer (rounded_rect 30 30 5 12))
                          (:holes (circle 10 32))))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn fillet_box_all_edges() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model (part body (fillet 2 (box 20 20 10))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("fillet box should render");
        assert!(
            !bundle.viewer_assets.is_empty(),
            "should produce viewer assets"
        );
    }

    #[test]
    fn fillet_box_top_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        render_model(
            r#"(model (part body (fillet 1.5 :edges "top" (box 20 20 10))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("fillet box top edges should render");
    }

    #[test]
    fn chamfer_box_all_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (chamfer 2 (box 20 20 10))))"#;
        let bundle =
            render_model(src, &DesignParams::new(), &resolver).expect("chamfer box should render");
        assert!(
            !bundle.viewer_assets.is_empty(),
            "should produce viewer assets"
        );
    }

    #[test]
    fn chamfer_box_top_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (chamfer 2 :edges "top" (box 20 20 10))))"#;
        render_model(src, &DesignParams::new(), &resolver)
            .expect("chamfer box top edges should render");
    }

    #[test]
    fn chamfer_cylinder() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (chamfer 1 (cylinder 10 20))))"#;
        render_model(src, &DesignParams::new(), &resolver).expect("chamfer cylinder should render");
    }

    #[test]
    fn mesh_volume_unit_cube() {
        // A 10x10x10 cube has volume 1000
        let cube = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let vol = mesh_volume(&cube).expect("volume should be finite and positive");
        assert!((vol - 1000.0).abs() < 1.0, "expected ~1000, got {}", vol);
    }

    #[test]
    fn mesh_area_unit_cube() {
        // A 10x10x10 cube has surface area 6 * 100 = 600
        let cube = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let area = mesh_area(&cube).expect("area should be finite and positive");
        assert!((area - 600.0).abs() < 1.0, "expected ~600, got {}", area);
    }

    #[test]
    fn mesh_volume_empty_returns_none() {
        let empty = IrMesh::from_polygons(&[], None);
        assert_eq!(mesh_volume(&empty), None);
    }

    #[test]
    fn mesh_area_empty_returns_none() {
        let empty = IrMesh::from_polygons(&[], None);
        assert_eq!(mesh_area(&empty), None);
    }

    #[test]
    fn render_model_produces_volume_and_area_in_manifest() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root: root.clone() };
        let bundle = render_model(
            r#"(model
                (params (number size 10))
                (part body (box size size size)))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        let manifest_str = std::fs::read_to_string(&bundle.manifest_path).unwrap();
        let manifest: ModelManifest = serde_json::from_str(&manifest_str).unwrap();
        assert_eq!(manifest.parts.len(), 1);
        let part = &manifest.parts[0];
        assert!(
            part.volume.is_some(),
            "volume should be computed for IR parts"
        );
        assert!(part.area.is_some(), "area should be computed for IR parts");
        assert!(part.volume.unwrap() > 0.0);
        assert!(part.area.unwrap() > 0.0);
    }

    #[test]
    fn render_model_supports_build_compound_clip_box_path_frame_and_place() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part body
                  (build
                    (shape rail
                      (bezier-path ((0 0 0) (10 0 0) (20 0 10) (30 0 10))))
                    (shape peg (cylinder 2 6))
                    (shape end-frame (path-frame rail :at end))
                    (shape placed (place end-frame peg :offset (0 0 -3)))
                    (result
                      (clip-box placed
                        :x (20 40)
                        :y (-5 5)
                        :z (-10 20))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");
        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert_eq!(bundle.viewer_assets.len(), 1);
    }

    #[test]
    fn eval_geometry_clip_box_returns_empty_mesh_on_miss() {
        let env = std::collections::BTreeMap::new();
        let expr = super::model::IrExpr::from_value(
            &lexpr::from_str("(clip-box (box 10 10 10) :x (20 30) :y (20 30) :z (20 30))")
                .expect("expr"),
        )
        .expect("typed expr");
        let geom = super::mesh_ops::eval_geometry_expr(&expr, &env).expect("eval");
        let mesh = geom.into_mesh("test").expect("mesh");
        assert!(
            mesh.triangulate().polygons.is_empty(),
            "expected empty clip"
        );
    }

    #[test]
    fn eval_geometry_path_frame_and_place_anchor_at_end() {
        let env = std::collections::BTreeMap::new();
        let expr = super::model::IrExpr::from_value(
            &lexpr::from_str(
                "(build
                (shape rail (path (0 0 0) (20 0 0)))
                (shape peg (box 4 4 4))
                (shape end-frame (path-frame rail :at end))
                (result (place end-frame peg)))",
            )
            .expect("expr"),
        )
        .expect("typed expr");
        let geom = super::mesh_ops::eval_geometry_expr(&expr, &env).expect("eval");
        let mesh = geom.into_mesh("test").expect("mesh");
        let bounds = super::runtime::bounds_from_mesh(&mesh);
        assert!((bounds.x_min - 20.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.x_max - 24.0).abs() < 0.25, "bounds: {:?}", bounds);
    }
}
