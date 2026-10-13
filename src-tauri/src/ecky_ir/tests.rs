#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_primitive_fallbacks_reject_segment_counts() {
        let env = std::collections::BTreeMap::new();
        for (operation, geometry) in [
            ("cylinder", "(cylinder 5 12 6)"),
            ("cone", "(cone 8 4 12 6)"),
            ("circle", "(extrude (circle 5 6) 12)"),
        ] {
            let expr = super::model::IrExpr::from_value(
                &lexpr::from_str(geometry).expect("expression"),
            )
            .expect("typed expression");
            let mesh_error = match super::mesh_ops::eval_geometry_expr(&expr, &env) {
                Err(error) => error,
                Ok(_) => panic!("mesh fallback must reject a segment count"),
            };
            assert!(mesh_error.to_string().contains(operation));

            let source = format!("(model (part body {geometry}))");
            let freecad_error = super::freecad_lowering::lower_to_freecad(&source)
                .expect_err("FreeCAD fallback must reject a segment count");
            assert!(freecad_error.to_string().contains(operation));
        }
    }

    #[test]
    fn renders_typed_polyhedron_tetrahedron_as_manifold_stl() {
        let root = std::env::temp_dir().join(format!(
            "ecky-polyhedron-tetrahedron-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
          (part tetra
            (polyhedron
              :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10))
              :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))))"#;

        let bundle = render_model(source, &DesignParams::new(), &resolver)
            .expect("typed polyhedron should render");
        let preview = Path::new(&bundle.model_stl_path);
        assert!(preview.is_file(), "model STL must exist");
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(preview)
                .expect("topology summary"),
            0,
            "tetrahedron must be manifold"
        );
        assert!(
            bundle
                .export_artifacts
                .iter()
                .any(|artifact| artifact.format == "stl" && Path::new(&artifact.path).is_file()),
            "mesh-native STL export must be explicit"
        );
        assert!(
            bundle
                .export_artifacts
                .iter()
                .all(|artifact| artifact.format != "step"),
            "pure mesh must not claim STEP"
        );

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn renders_formula_generated_polyhedron_vertices_with_same_topology() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("temp root");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
          (params (number size 10))
          (part tetra
            (polyhedron
              :vertices
                (map
                  (lambda (i)
                    (list
                      (* size (if (= i 1) 1 0))
                      (* size (if (= i 2) 1 0))
                      (* size (if (= i 3) 1 0))))
                  (range 0 4))
              :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))))"#;

        let bundle = render_model(source, &DesignParams::new(), &resolver)
            .expect("bounded map/range vertices should render");
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                Path::new(&bundle.model_stl_path),
            )
            .expect("topology summary"),
            0
        );
        let literal = render_model(
            r#"(model (part tetra
              (polyhedron
                :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10))
                :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("equivalent literal polyhedron");
        assert_eq!(
            std::fs::read(&bundle.model_stl_path).expect("formula STL"),
            std::fs::read(&literal.model_stl_path).expect("literal STL"),
            "formula and literal mesh digests must match"
        );
        let manifest = crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id)
            .expect("formula manifest");
        assert!(
            manifest
                .warnings
                .iter()
                .any(|warning| warning.contains("Mesh evidence:") && warning.contains("topology=closed")),
            "manifest must expose mesh digest and topology evidence"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_procedural_mesh_over_budget_before_render_allocation() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("temp root");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
          (part too-large
            (mesh
              :vertices
                (map (lambda (i) (list i 0 0)) (range 0 100001))
              :triangles ((0 1 2)))))"#;

        let error = render_model(source, &DesignParams::new(), &resolver)
            .expect_err("oversized procedural mesh must reject");
        assert!(error.to_string().contains("vertices count 100001"), "{error}");
        assert!(error.to_string().contains("allowed count 100000"), "{error}");
        assert!(
            !root.join("model-runtime").exists(),
            "budget rejection must happen before render directories"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_open_polyhedron_with_boundary_edge_evidence_before_artifact_write() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("temp root");
        let resolver = TestResolver { root: root.clone() };
        let last_good = render_model(
            r#"(model (part tetra
              (polyhedron
                :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10))
                :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("seed last-good artifact");
        let last_good_bytes = std::fs::read(&last_good.model_stl_path).expect("last-good STL");
        let source = r#"(model
          (part open-tetra
            (polyhedron
              :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10))
              :triangles ((0 2 1) (0 1 3) (1 2 3)))))"#;

        let error = render_model(source, &DesignParams::new(), &resolver)
            .expect_err("open polyhedron must reject");
        assert_eq!(error.operation.as_deref(), Some("polyhedron"));
        assert!(error.to_string().contains("boundary edges: 3"), "{error}");
        assert!(
            Path::new(&last_good.model_stl_path).is_file(),
            "invalid polyhedron must preserve last-good artifact"
        );
        assert_eq!(
            std::fs::read(&last_good.model_stl_path).expect("preserved last-good STL"),
            last_good_bytes
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn renders_same_open_surface_as_mesh_with_boundary_evidence() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("temp root");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
          (part open-tetra
            (mesh
              :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10))
              :triangles ((0 2 1) (0 1 3) (1 2 3)))))"#;

        let bundle = render_model(source, &DesignParams::new(), &resolver)
            .expect("open mesh should render without topology repair");
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                Path::new(&bundle.model_stl_path),
            )
            .expect("boundary evidence"),
            3
        );
        let manifest = crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id)
            .expect("open mesh manifest");
        assert_eq!(manifest.parts[0].kind, "mesh");
        assert!(
            manifest
                .warnings
                .iter()
                .any(|warning| warning.contains("topology=open-or-non-manifold"))
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn renders_grayscale_heightfield_as_closed_deterministic_stl() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("temp root");
        let image_path = root.join("heightmap.png");
        let mut image = image::GrayImage::new(3, 3);
        for y in 0..3 {
            for x in 0..3 {
                image.put_pixel(x, y, image::Luma([((x + y) * 48) as u8]));
            }
        }
        image.save(&image_path).expect("heightmap fixture");
        let resolver = TestResolver { root: root.clone() };
        let source = format!(
            r#"(model
              (part relief
                (heightfield "{}"
                  :width 30
                  :depth 20
                  :relief-height 4
                  :base-thickness 1.2
                  :invert #f)))"#,
            image_path.display()
        );

        let first = render_model(&source, &DesignParams::new(), &resolver)
            .expect("heightfield should render");
        let first_bytes = std::fs::read(&first.model_stl_path).expect("first preview");
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                Path::new(&first.model_stl_path),
            )
            .expect("heightfield topology"),
            0
        );
        let second = render_model(&source, &DesignParams::new(), &resolver)
            .expect("same heightfield should rerender");
        assert_eq!(
            first_bytes,
            std::fs::read(&second.model_stl_path).expect("second preview")
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn renders_alpha_encoded_stamp_as_luminance_relief() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("temp root");
        let image_path = root.join("alpha-stamp.png");
        let mut image = image::RgbaImage::new(2, 2);
        image.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, image::Rgba([0, 0, 0, 255]));
        image.put_pixel(0, 1, image::Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 1, image::Rgba([0, 0, 0, 255]));
        image.save(&image_path).expect("alpha stamp fixture");
        let resolver = TestResolver { root: root.clone() };
        let source = format!(
            r#"(model
              (part relief
                (heightfield "{}"
                  :width 30
                  :depth 20
                  :relief-height 2
                  :base-thickness 1
                  :invert #f)))"#,
            image_path.display()
        );

        let bundle = render_model(&source, &DesignParams::new(), &resolver)
            .expect("alpha stamp should render");
        let viewer_asset = bundle.viewer_assets.first().expect("viewer asset");
        let indexed_asset = crate::ecky_ir::mesh_asset::IndexedMeshAsset::read_cache(
            &Path::new(&viewer_asset.path).with_extension("indexed-mesh.json"),
        )
        .expect("indexed heightfield asset");
        let mut positive_z = indexed_asset
            .vertices()
            .iter()
            .map(|vertex| vertex[2])
            .filter(|z| *z > 0.0);
        let first_z = positive_z.next().expect("positive heightfield vertices");
        let (min_z, max_z) = positive_z.fold((first_z, first_z), |(min_z, max_z), z| {
            (min_z.min(z), max_z.max(z))
        });
        assert!(
            max_z - min_z > 1.5,
            "alpha-encoded black artwork must produce relief variation, got min={min_z} max={max_z}"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn heightfield_surfaces_raw_decoder_context_for_corrupt_image() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("temp root");
        let image_path = root.join("corrupt.png");
        image::GrayImage::from_pixel(2, 2, image::Luma([128]))
            .save(&image_path)
            .expect("valid seed fixture");
        let resolver = TestResolver { root: root.clone() };
        let source = format!(
            r#"(model (part relief
              (heightfield "{}" :width 30 :depth 20 :relief-height 4 :base-thickness 1)))"#,
            image_path.display()
        );

        let last_good = render_model(&source, &DesignParams::new(), &resolver)
            .expect("valid heightmap seeds last-good preview");
        let last_good_bytes = std::fs::read(&last_good.model_stl_path).expect("last-good STL");
        std::fs::write(&image_path, b"not a png").expect("corrupt fixture");

        let error = render_model(&source, &DesignParams::new(), &resolver)
            .expect_err("corrupt heightmap must reject");
        assert_eq!(error.operation.as_deref(), Some("heightfield"));
        assert!(error.to_string().contains("corrupt.png"), "{error}");
        assert!(
            error.to_string().contains("failed to inspect")
                || error.to_string().contains("failed to decode"),
            "{error}"
        );
        assert_eq!(
            std::fs::read(&last_good.model_stl_path).expect("preserved last-good STL"),
            last_good_bytes
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    fn render_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ecky-ir-test-{}", uuid::Uuid::new_v4()))
    }

    fn surface_fixture(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/cad/surface/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
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
                (part body (cylinder 20 80)))"#,
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
    fn derive_controls_from_core_program_matches_public_entrypoint() {
        let source = r#"
            (define base-radius 14)
            (model
              (params
                (number radius base-radius :label "Radius")
                (toggle vents true :label "Vents")
                (image litho "" :label "Litho"))
              (part body (extrude (circle radius) 20)))
        "#;
        let program = crate::ecky_scheme::try_compile_to_core_program(source)
            .expect("compiled path")
            .expect("program");

        let direct = super::runtime::derive_controls_from_core_program(&program).expect("direct");
        let public = derive_controls(source).expect("public");

        assert_eq!(direct.fields, public.fields);
        assert_eq!(direct.params, public.params);
    }

    #[test]
    fn source_uses_ecky_rust_only_cad_ops_detects_wall_pattern_heads() {
        assert!(source_uses_ecky_rust_only_cad_ops(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#
        ));
        assert!(source_uses_ecky_rust_only_cad_ops(
            r#"(model
                (part body
                  (pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#
        ));
        assert!(source_uses_ecky_rust_only_cad_ops(
            r#"
            (define (ribbed shape)
              (wall-pattern (:mode ribs :depth 0.4 :uFreq 8) shape))
            (model
              (part body
                (ribbed (extrude (circle 5) 18))))
            "#
        ));
    }

    #[test]
    fn source_uses_ecky_rust_only_cad_ops_ignores_wall_pattern_strings() {
        assert!(!source_uses_ecky_rust_only_cad_ops(
            r#"(model
                (part body
                  (extrude (text "wall-pattern") 2)))"#
        ));
    }

    #[test]
    fn source_uses_direct_occt_required_cad_ops_detects_native_only_heads() {
        assert!(source_uses_direct_occt_required_cad_ops(
            r#"(model
                (part body
                  (text "A" 12)))"#
        ));
        assert!(source_uses_direct_occt_required_cad_ops(
            r#"(model
                (part body
                  (import-stl "/tmp/part.stl")))"#
        ));
        assert!(source_uses_direct_occt_required_cad_ops(
            r#"(model
                (part body
                  (import-step "/tmp/part.step")))"#
        ));
        assert!(source_uses_direct_occt_required_cad_ops(
            r#"(model
                (part body
                  (helical-ridge
                    :radius 20
                    :pitch 6
                    :height 30
                    :base-width 2
                    :crest-width 1
                    :depth 1.5)))"#
        ));
        assert!(source_uses_direct_occt_required_cad_ops(
            r#"(model
                (part body
                  (chamfer 1 (box 20 20 10))))"#
        ));
        assert!(source_uses_direct_occt_required_cad_ops(
            r#"(model
                (part body
                  (fillet 1 (box 20 20 10))))"#
        ));
    }

    #[test]
    fn source_uses_direct_occt_required_cad_ops_ignores_strings() {
        assert!(!source_uses_direct_occt_required_cad_ops(
            r#"(model
                (params
                  (image decal "helical-ridge"))
                (part body
                  (box 1 1 1)))"#
        ));
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
        assert!(Path::new(&bundle.model_stl_path).exists());
        assert_eq!(bundle.viewer_assets.len(), 1);
    }

    #[test]
    fn render_model_resolves_scalar_build_bindings_in_mesh_pipeline() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part body
                  (build
                    (shape width (/ 10 2))
                    (shape block (box width 2 2))
                    (result block))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.engine_kind, EngineKind::EckyIrV0);
        assert!(Path::new(&bundle.model_stl_path).exists());
    }

    #[allow(dead_code)]
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
            err.message
                .contains("Unsupported on current geometry backend"),
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
                    (circle 12)))
                (part twisted
                  (translate 50 0 0
                    (twist 36 120 10
                      (rounded_rect 12 8 2 8)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 3);
        assert!(Path::new(&bundle.model_stl_path).exists());
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
                          (translate 0 0 2 (cylinder 8 16))
                          (box 10 10 10)))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.model_stl_path).exists());
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
                      (offset-rounded 4 (circle 10))
                      (circle 10))
                    8))
                (part shell-a
                  (translate 32 0 0
                    (shell 2
                      (cylinder 14 28))))
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
        assert!(Path::new(&bundle.model_stl_path).exists());
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
                    (shell 1.2 (cylinder 18 42))))
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
                      (twist 32 120 10 (rounded_rect 14 10 2 8)))))
                (part cellular
                  (translate 48 48 0
                    (wall-pattern
                      (:mode cellular :depth 0.7 :uFreq 7 :vFreq 7 :seed 12)
                      (shell 1.2 (cylinder 15 34)))))
                (part fbm
                  (translate -48 -48 0
                    (wall-pattern
                      (:mode fbm :depth 0.6 :uFreq 8 :vFreq 8 :seed 3)
                      (shell 1.0 (cylinder 14 30)))))
                (part gyroid
                  (translate 48 -48 0
                    (wall-pattern
                      (:mode gyroid :depth 0.6 :uFreq 4 :vFreq 5 :phase 0.2)
                      (shell 1.0 (cylinder 14 30))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 8);
        assert!(Path::new(&bundle.model_stl_path).exists());
    }

    #[test]
    fn render_model_supports_wall_pattern_fixture_modes() {
        for fixture in [
            "wall_pattern_cellular.ecky",
            "wall_pattern_fbm.ecky",
            "wall_pattern_gyroid.ecky",
        ] {
            let root = render_root();
            std::fs::create_dir_all(&root).unwrap();
            let resolver = TestResolver { root };
            let source = surface_fixture(fixture);
            let bundle = render_model(&source, &DesignParams::new(), &resolver)
                .unwrap_or_else(|err| panic!("{fixture}: {err}"));

            assert_eq!(bundle.engine_kind, EngineKind::EckyIrV0, "{fixture}");
            assert_eq!(bundle.viewer_assets.len(), 1, "{fixture}");
            assert!(Path::new(&bundle.model_stl_path).exists(), "{fixture}");
        }
    }

    #[test]
    fn wall_pattern_accepts_solid_mesh_targets() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part body
                  (wall-pattern
                    (:mode ribs :depth 1)
                    (box 20 20 20))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("wall-pattern accepts evaluated solid meshes");

        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                Path::new(&bundle.model_stl_path),
            )
            .expect("STL topology"),
            0
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
                          (:outer (circle 15))
                          (:holes (circle 8))))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 4);
        assert!(Path::new(&bundle.model_stl_path).exists());
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
                          (:holes (circle 10))))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.model_stl_path).exists());
    }

    #[test]
    fn render_model_supports_chaotic_and_implicit_wall_pattern_modes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part schwarz-p
                  (wall-pattern
                    (:mode schwarz-p :depth 0.5 :uFreq 4 :vFreq 5 :softness 0.12)
                    (shell 1.0 (cylinder 10 22))))
                (part diamond-field
                  (translate 28 0 0
                    (wall-pattern
                      (:mode diamond-field :depth 0.45 :uFreq 4 :vFreq 4 :phase 0.1)
                      (shell 1.0 (cylinder 10 22)))))
                (part neovius
                  (translate -28 0 0
                    (wall-pattern
                      (:mode neovius :depth 0.45 :uFreq 3 :vFreq 4 :bias 0.05)
                      (shell 1.0 (cylinder 10 22)))))
                (part attractor-field
                  (translate 0 28 0
                    (wall-pattern
                      (:mode attractor-field :depth 0.5 :uFreq 6 :vFreq 6 :seed 99)
                      (shell 1.0 (cylinder 10 22))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 4);
        assert!(Path::new(&bundle.model_stl_path).exists());
    }

    #[test]
    fn cad_fillet_rejects_mesh_renderer() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let err = render_model(
            r#"(model (part body (fillet 2 (box 20 20 10))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect_err("CAD fillet should not run through the mesh renderer");
        assert!(
            err.message.contains("Direct OCCT"),
            "unexpected error: {}",
            err.message
        );
    }

    #[test]
    fn mesh_fillet_box_all_edges() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model (part body (mesh-fillet 2 (box 20 20 10))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("mesh-fillet box should render");
        assert!(
            !bundle.viewer_assets.is_empty(),
            "should produce viewer assets"
        );
    }

    #[test]
    fn mesh_fillet_box_top_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        render_model(
            r#"(model (part body (mesh-fillet 1.5 :edges "top" (box 20 20 10))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("mesh-fillet box top edges should render");
    }

    #[test]
    fn cad_chamfer_rejects_mesh_renderer() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (chamfer 2 (box 20 20 10))))"#;
        let err = render_model(src, &DesignParams::new(), &resolver)
            .expect_err("CAD chamfer should not run through the mesh renderer");
        assert!(
            err.message.contains("Direct OCCT"),
            "unexpected error: {}",
            err.message
        );
    }

    #[test]
    fn mesh_chamfer_box_all_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (mesh-chamfer 2 (box 20 20 10))))"#;
        let bundle =
            render_model(src, &DesignParams::new(), &resolver).expect("mesh-chamfer box should render");
        assert!(
            !bundle.viewer_assets.is_empty(),
            "should produce viewer assets"
        );
    }

    #[test]
    fn mesh_chamfer_box_top_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (mesh-chamfer 2 :edges "top" (box 20 20 10))))"#;
        render_model(src, &DesignParams::new(), &resolver)
            .expect("mesh-chamfer box top edges should render");
    }

    #[test]
    fn mesh_fillet_box_compound_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (mesh-fillet 1 :edges "x-min+z-max" (box 20 20 10))))"#;
        render_model(src, &DesignParams::new(), &resolver)
            .expect("mesh-fillet box compound edges should render");
    }

    #[test]
    fn mesh_chamfer_cylinder() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (mesh-chamfer 1 (cylinder 10 20))))"#;
        render_model(src, &DesignParams::new(), &resolver)
            .expect("mesh-chamfer cylinder should render");
    }

    #[test]
    fn wall_pattern_accepts_llm_generated_polyhedron_target() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part body
                  (wall-pattern (:mode cellular :depth 0.4 :uFreq 5 :vFreq 5)
                    (polyhedron
                      :vertices
                        ((0 0 0) (2 0 0) (0 20 0) (2 20 0)
                         (0 0 30) (2 0 30) (0 20 30) (2 20 30))
                      :triangles
                        ((0 4 6) (0 6 2) (1 3 7) (1 7 5)
                         (0 1 5) (0 5 4) (2 6 7) (2 7 3)
                         (0 2 3) (0 3 1) (4 5 7) (4 7 6))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render wall-pattern over LLM-generated polyhedron");

        assert!(Path::new(&bundle.model_stl_path).is_file());
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                Path::new(&bundle.model_stl_path),
            )
            .expect("STL topology"),
            0
        );
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
        assert!(Path::new(&bundle.model_stl_path).exists());
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
        assert!((bounds.x_min - 18.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.x_max - 22.0).abs() < 0.25, "bounds: {:?}", bounds);
    }

    #[test]
    fn eval_geometry_extrude_preserves_sketch_coordinates() {
        let env = std::collections::BTreeMap::new();
        let expr = super::model::IrExpr::from_value(
            &lexpr::from_str("(extrude (polygon ((0 0) (100 0) (100 10) (0 10))) 5)")
                .expect("expr"),
        )
        .expect("typed expr");
        let geom = super::mesh_ops::eval_geometry_expr(&expr, &env).expect("eval");
        let mesh = geom.into_mesh("test").expect("mesh");
        let bounds = super::runtime::bounds_from_mesh(&mesh);
        assert!((bounds.x_min - 0.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.x_max - 100.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.y_min - 0.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.y_max - 10.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_min - 0.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_max - 5.0).abs() < 0.25, "bounds: {:?}", bounds);
    }

    #[test]
    fn eval_geometry_extrude_symmetric_centers_z() {
        let env = std::collections::BTreeMap::new();
        let expr = super::model::IrExpr::from_value(
            &lexpr::from_str("(extrude (polygon ((0 0) (10 0) (10 10) (0 10))) 8 :symmetric #t)")
                .expect("expr"),
        )
        .expect("typed expr");
        let geom = super::mesh_ops::eval_geometry_expr(&expr, &env).expect("eval");
        let mesh = geom.into_mesh("test").expect("mesh");
        let bounds = super::runtime::bounds_from_mesh(&mesh);
        assert!((bounds.z_min + 4.0).abs() < 0.25, "bounds: {:?}", bounds);
        assert!((bounds.z_max - 4.0).abs() < 0.25, "bounds: {:?}", bounds);
    }

    #[test]
    fn eval_geometry_primitives_honor_align_keyword() {
        let env = std::collections::BTreeMap::new();

        let box_expr = super::model::IrExpr::from_value(
            &lexpr::from_str("(box 10 20 30 :align (min center max))").expect("expr"),
        )
        .expect("typed expr");
        let box_bounds = super::runtime::bounds_from_mesh(
            &super::mesh_ops::eval_geometry_expr(&box_expr, &env)
                .expect("eval")
                .into_mesh("box")
                .expect("mesh"),
        );
        assert!(
            (box_bounds.x_min - 0.0).abs() < 0.25,
            "box: {:?}",
            box_bounds
        );
        assert!(
            (box_bounds.z_max - 0.0).abs() < 0.25,
            "box: {:?}",
            box_bounds
        );

        let cylinder_expr = super::model::IrExpr::from_value(
            &lexpr::from_str("(cylinder 5 12 :align (max min center))").expect("expr"),
        )
        .expect("typed expr");
        let cylinder_bounds = super::runtime::bounds_from_mesh(
            &super::mesh_ops::eval_geometry_expr(&cylinder_expr, &env)
                .expect("eval")
                .into_mesh("cylinder")
                .expect("mesh"),
        );
        assert!(
            (cylinder_bounds.x_max - 0.0).abs() < 0.25,
            "cylinder: {:?}",
            cylinder_bounds
        );
        assert!(
            (cylinder_bounds.y_min - 0.0).abs() < 0.25,
            "cylinder: {:?}",
            cylinder_bounds
        );

        let sphere_expr = super::model::IrExpr::from_value(
            &lexpr::from_str("(sphere 6 :align (min max center))").expect("expr"),
        )
        .expect("typed expr");
        let sphere_bounds = super::runtime::bounds_from_mesh(
            &super::mesh_ops::eval_geometry_expr(&sphere_expr, &env)
                .expect("eval")
                .into_mesh("sphere")
                .expect("mesh"),
        );
        assert!(
            (sphere_bounds.x_min - 0.0).abs() < 0.25,
            "sphere: {:?}",
            sphere_bounds
        );
        assert!(
            (sphere_bounds.y_max - 0.0).abs() < 0.25,
            "sphere: {:?}",
            sphere_bounds
        );

        let cone_expr = super::model::IrExpr::from_value(
            &lexpr::from_str("(cone 8 4 12 :align (center max min))").expect("expr"),
        )
        .expect("typed expr");
        let cone_bounds = super::runtime::bounds_from_mesh(
            &super::mesh_ops::eval_geometry_expr(&cone_expr, &env)
                .expect("eval")
                .into_mesh("cone")
                .expect("mesh"),
        );
        assert!(
            (cone_bounds.y_max - 0.0).abs() < 0.25,
            "cone: {:?}",
            cone_bounds
        );
        assert!(
            (cone_bounds.z_min - 0.0).abs() < 0.25,
            "cone: {:?}",
            cone_bounds
        );
    }

    #[test]
    fn eval_geometry_plane_location_and_place() {
        let env = std::collections::BTreeMap::new();
        let expr = super::model::IrExpr::from_value(
            &lexpr::from_str(
                "(build
                  (shape base (plane :origin (10 20 30) :x (1 0 0) :normal (0 0 1)))
                  (shape peg (box 4 4 4))
                  (shape pose (location base :offset (5 0 0)))
                  (result (place pose peg)))",
            )
            .expect("expr"),
        )
        .expect("typed expr");
        let geom = super::mesh_ops::eval_geometry_expr(&expr, &env).expect("eval");
        let mesh = geom.into_mesh("test").expect("mesh");
        let bounds = super::runtime::bounds_from_mesh(&mesh);
        assert!((bounds.x_min - 13.0).abs() < 0.75, "bounds: {:?}", bounds);
        assert!((bounds.x_max - 17.0).abs() < 0.75, "bounds: {:?}", bounds);
        assert!((bounds.z_min - 30.0).abs() < 0.75, "bounds: {:?}", bounds);
    }
}
