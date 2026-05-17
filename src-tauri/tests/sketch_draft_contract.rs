use base64::{engine::general_purpose::STANDARD, Engine as _};
use ecky_cad_lib::component_package_runtime::{
    install_component_package_archive, read_component_package_manifest,
    resolve_installed_component_source, write_component_package_archive,
};
use ecky_cad_lib::models::{
    component_package_header, validate_component_package, ArtifactBundle, ComponentPort,
    GeometryBackend, MacroDialect, ModelSourceKind, OperationKind, PathResolver, PortFrame,
    PortTypeDefinition, SketchAcceptedBrepComponentPackageRequest,
    SketchBrepCandidateAcceptRequest, SketchBrepCandidateRequest,
    SketchBrepCandidateSourceStrategy, SketchDefinition, SketchDocument, SketchDraftOperationKind,
    SketchDraftRequest, SketchPreviewHullRequest, SketchPrimitive, SketchPrimitiveKind,
    SketchSuggestionRequest, SketchView, SourceLanguage, ViewerEdgePoint, ViewerEdgeTarget,
    ViewerFaceTarget,
};
use ecky_cad_lib::sketch_draft_runtime::{
    accepted_brep_candidate_to_component_package, analyze_sketch_brep_candidates,
    generate_accepted_brep_candidate_source, generate_sketch_draft_preview,
    generate_sketch_draft_source, generate_sketch_preview_hull,
    generate_sketch_preview_hull_source, require_step_export_artifact,
    sketch_suggestion_to_draft_request, suggest_sketch_features,
    write_accepted_brep_component_package_project,
};

struct TempPathResolver {
    root: std::path::PathBuf,
}

impl PathResolver for TempPathResolver {
    fn app_config_dir(&self) -> std::path::PathBuf {
        self.root.join("config")
    }

    fn app_data_dir(&self) -> std::path::PathBuf {
        self.root.join("data")
    }

    fn resource_path(&self, _path: &str) -> Option<std::path::PathBuf> {
        None
    }
}

fn rectangle_sketch(closed: bool) -> SketchDefinition {
    SketchDefinition {
        sketch_id: "front_profile".to_string(),
        view: SketchView::Front,
        plane: None,
        primitives: vec![SketchPrimitive {
            primitive_id: "outer".to_string(),
            kind: SketchPrimitiveKind::Polyline,
            points: vec![[0.0, 0.0], [30.0, 0.0], [30.0, 12.0], [0.0, 12.0]],
            closed,
            radius: None,
            topology: None,
        }],
        constraints: vec![],
    }
}

fn rectangle_view_sketch(
    sketch_id: &str,
    view: SketchView,
    primitive_id: &str,
    points: Vec<[f64; 2]>,
) -> SketchDefinition {
    SketchDefinition {
        sketch_id: sketch_id.to_string(),
        view,
        plane: None,
        primitives: vec![SketchPrimitive {
            primitive_id: primitive_id.to_string(),
            kind: SketchPrimitiveKind::Polyline,
            points,
            closed: true,
            radius: None,
            topology: None,
        }],
        constraints: vec![],
    }
}

fn three_view_hull_document() -> SketchDocument {
    SketchDocument {
        document_id: "doc-hull".to_string(),
        active_sketch_id: Some("sketch-front".to_string()),
        units: Some("mm".to_string()),
        metadata: None,
        sketches: vec![
            rectangle_view_sketch(
                "sketch-front",
                SketchView::Front,
                "front-box",
                vec![
                    [10.0, 20.0],
                    [60.0, 20.0],
                    [60.0, 50.0],
                    [10.0, 50.0],
                    [10.0, 20.0],
                ],
            ),
            rectangle_view_sketch(
                "sketch-top",
                SketchView::Top,
                "top-footprint",
                vec![
                    [10.0, 5.0],
                    [60.0, 5.0],
                    [60.0, 27.0],
                    [10.0, 27.0],
                    [10.0, 5.0],
                ],
            ),
            rectangle_view_sketch(
                "sketch-side",
                SketchView::Side,
                "side-footprint",
                vec![
                    [5.0, 20.0],
                    [27.0, 20.0],
                    [27.0, 50.0],
                    [5.0, 50.0],
                    [5.0, 20.0],
                ],
            ),
        ],
    }
}

fn concave_front_hull_document() -> SketchDocument {
    SketchDocument {
        document_id: "doc-concave-hull".to_string(),
        active_sketch_id: Some("sketch-front".to_string()),
        units: Some("mm".to_string()),
        metadata: None,
        sketches: vec![
            rectangle_view_sketch(
                "sketch-front",
                SketchView::Front,
                "front-concave",
                vec![
                    [0.0, 0.0],
                    [20.0, 0.0],
                    [20.0, 10.0],
                    [10.0, 10.0],
                    [10.0, 20.0],
                    [0.0, 20.0],
                    [0.0, 0.0],
                ],
            ),
            rectangle_view_sketch(
                "sketch-top",
                SketchView::Top,
                "top-depth",
                vec![
                    [0.0, 0.0],
                    [20.0, 0.0],
                    [20.0, 10.0],
                    [0.0, 10.0],
                    [0.0, 0.0],
                ],
            ),
            rectangle_view_sketch(
                "sketch-side",
                SketchView::Side,
                "side-depth",
                vec![
                    [0.0, 0.0],
                    [10.0, 0.0],
                    [10.0, 20.0],
                    [0.0, 20.0],
                    [0.0, 0.0],
                ],
            ),
        ],
    }
}

fn holed_front_hull_document() -> SketchDocument {
    SketchDocument {
        document_id: "doc-holed-hull".to_string(),
        active_sketch_id: Some("sketch-front".to_string()),
        units: Some("mm".to_string()),
        metadata: None,
        sketches: vec![
            SketchDefinition {
                sketch_id: "sketch-front".to_string(),
                view: SketchView::Front,
                plane: None,
                primitives: vec![
                    SketchPrimitive {
                        primitive_id: "front-outer".to_string(),
                        kind: SketchPrimitiveKind::Polyline,
                        points: vec![
                            [0.0, 0.0],
                            [20.0, 0.0],
                            [20.0, 20.0],
                            [0.0, 20.0],
                            [0.0, 0.0],
                        ],
                        closed: true,
                        radius: None,
                        topology: None,
                    },
                    SketchPrimitive {
                        primitive_id: "front-hole".to_string(),
                        kind: SketchPrimitiveKind::Polyline,
                        points: vec![
                            [7.0, 7.0],
                            [13.0, 7.0],
                            [13.0, 13.0],
                            [7.0, 13.0],
                            [7.0, 7.0],
                        ],
                        closed: true,
                        radius: None,
                        topology: None,
                    },
                ],
                constraints: vec![],
            },
            rectangle_view_sketch(
                "sketch-top",
                SketchView::Top,
                "top-depth",
                vec![
                    [0.0, 0.0],
                    [20.0, 0.0],
                    [20.0, 10.0],
                    [0.0, 10.0],
                    [0.0, 0.0],
                ],
            ),
            rectangle_view_sketch(
                "sketch-side",
                SketchView::Side,
                "side-depth",
                vec![
                    [0.0, 0.0],
                    [10.0, 0.0],
                    [10.0, 20.0],
                    [0.0, 20.0],
                    [0.0, 0.0],
                ],
            ),
        ],
    }
}

fn circle_sketch(sketch_id: &str, primitive_id: &str, radius: f64) -> SketchDefinition {
    SketchDefinition {
        sketch_id: sketch_id.to_string(),
        view: SketchView::Top,
        plane: None,
        primitives: vec![SketchPrimitive {
            primitive_id: primitive_id.to_string(),
            kind: SketchPrimitiveKind::Circle,
            points: vec![[0.0, 0.0]],
            closed: true,
            radius: Some(radius),
            topology: None,
        }],
        constraints: vec![],
    }
}

#[test]
fn sketch_feature_suggestions_are_deterministic_and_sorted() {
    let document = SketchDocument {
        document_id: "doc-a".to_string(),
        sketches: vec![
            circle_sketch("z_last", "small", 3.0),
            rectangle_sketch(true),
            circle_sketch("a_first", "large", 6.0),
        ],
        active_sketch_id: None,
        units: Some("mm".to_string()),
        metadata: None,
    };
    let request = SketchSuggestionRequest {
        document,
        limit: None,
    };

    let first = suggest_sketch_features(request.clone());
    let second = suggest_sketch_features(request);

    assert_eq!(first, second);
    assert_eq!(
        first
            .suggestions
            .iter()
            .map(|suggestion| suggestion.suggestion_id.as_str())
            .collect::<Vec<_>>(),
        vec!["front_profile:outer:extrude"]
    );
    assert_eq!(first.suggestions[0].amount, 12.0);
    assert!(first.warnings.is_empty());
}

#[test]
fn sketch_feature_suggestions_only_offer_front_profile_default_depth() {
    let document = SketchDocument {
        document_id: "doc-orthographic".to_string(),
        sketches: vec![
            rectangle_view_sketch(
                "sketch-front",
                SketchView::Front,
                "front-loop",
                vec![[0.0, 0.0], [60.0, 0.0], [60.0, 33.87], [0.0, 33.87]],
            ),
            rectangle_view_sketch(
                "sketch-top",
                SketchView::Top,
                "top-loop",
                vec![[0.0, 0.0], [60.0, 0.0], [60.0, 20.73], [0.0, 20.73]],
            ),
            rectangle_view_sketch(
                "sketch-side",
                SketchView::Side,
                "side-loop",
                vec![[0.0, 0.0], [20.73, 0.0], [20.73, 33.87], [0.0, 33.87]],
            ),
        ],
        active_sketch_id: Some("sketch-front".to_string()),
        units: Some("mm".to_string()),
        metadata: None,
    };

    let response = suggest_sketch_features(SketchSuggestionRequest {
        document,
        limit: None,
    });

    assert_eq!(response.suggestions.len(), 1);
    assert_eq!(response.suggestions[0].sketch_id, "sketch-front");
    assert_eq!(
        response.suggestions[0].primitive_id.as_deref(),
        Some("front-loop")
    );
    assert_eq!(response.suggestions[0].amount, 12.0);
}

#[test]
fn sketch_feature_suggestions_warn_for_open_profiles_without_panicking() {
    let response = suggest_sketch_features(SketchSuggestionRequest {
        document: SketchDocument {
            document_id: "doc-open".to_string(),
            sketches: vec![rectangle_sketch(false)],
            active_sketch_id: None,
            units: None,
            metadata: None,
        },
        limit: None,
    });

    assert!(response.suggestions.is_empty());
    assert_eq!(
        response.warnings,
        vec!["sketch 'front_profile' primitive 'outer' is open; close it before creating a solid draft."]
    );
}

#[test]
fn accepted_sketch_suggestion_converts_to_ecky_source() {
    let sketch = rectangle_sketch(true);
    let response = suggest_sketch_features(SketchSuggestionRequest {
        document: SketchDocument {
            document_id: "doc-accept".to_string(),
            sketches: vec![sketch.clone()],
            active_sketch_id: None,
            units: None,
            metadata: None,
        },
        limit: Some(1),
    });

    let request = sketch_suggestion_to_draft_request(&sketch, &response.suggestions[0])
        .expect("draft request");
    let draft = generate_sketch_draft_source(request).expect("draft source");

    assert!(draft.source.contains("(part front_profile_outer"));
    assert!(draft.source.contains("(extrude"));
}

#[test]
fn sketch_draft_generates_ecky_extrude_source_from_closed_polyline() {
    let draft = generate_sketch_draft_source(SketchDraftRequest {
        part_id: "bottle_cage_side".to_string(),
        sketch: rectangle_sketch(true),
        operation: SketchDraftOperationKind::Extrude,
        amount: 8.0,
        symmetric: true,
    })
    .expect("draft source");

    assert_eq!(draft.source_language, SourceLanguage::EckyIrV0);
    assert_eq!(draft.geometry_backend, GeometryBackend::EckyRust);
    assert_eq!(draft.macro_dialect, MacroDialect::EckyIrV0);
    assert!(draft.source.contains("(part bottle_cage_side"));
    assert!(draft
        .source
        .contains("(polygon ((0 0) (30 0) (30 12) (0 12)))"));
    assert!(draft.source.contains("(extrude"));
    assert!(draft.source.contains(":symmetric #t"));
    assert!(draft.warnings.is_empty());
}

#[test]
fn sketch_draft_embeds_source_map_comment_and_preview_renders_through_it() {
    let request = SketchDraftRequest {
        part_id: "bottle_cage_side".to_string(),
        sketch: rectangle_sketch(true),
        operation: SketchDraftOperationKind::Extrude,
        amount: 8.0,
        symmetric: false,
    };

    let draft = generate_sketch_draft_source(request.clone()).expect("draft source");
    assert!(draft.source.starts_with("; ecky-sketch-document-base64: "));

    let encoded = draft
        .source
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("; ecky-sketch-document-base64: "))
        .expect("source map comment");
    let decoded = STANDARD.decode(encoded).expect("decoded source map");
    let document: SketchDocument =
        serde_json::from_slice(&decoded).expect("decoded sketch document json");

    assert_eq!(document.sketches.len(), 1);
    assert_eq!(document.sketches[0].sketch_id, "front_profile");
    assert_eq!(document.sketches[0].primitives.len(), 1);
    assert_eq!(document.sketches[0].primitives[0].primitive_id, "outer");
    assert!(document.sketches[0].primitives[0].closed);
    assert_eq!(
        document.sketches[0].primitives[0].points,
        vec![[0.0, 0.0], [30.0, 0.0], [30.0, 12.0], [0.0, 12.0]]
    );

    let temp_root = std::env::temp_dir().join(format!(
        "ecky-sketch-draft-source-map-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };

    let (preview_draft, bundle) =
        generate_sketch_draft_preview(request, &resolver).expect("preview render");

    assert_eq!(preview_draft.source, draft.source);
    assert!(bundle.preview_stl_path.ends_with(".stl"));
    assert!(std::path::Path::new(&bundle.preview_stl_path).exists());
    assert!(!bundle.viewer_assets.is_empty());

    std::fs::remove_dir_all(temp_root).ok();
}

#[test]
fn sketch_draft_compacts_dense_profile_before_steel_lowering() {
    let mut points = Vec::new();
    let count = 20_000usize;
    for index in 0..count {
        let t = index as f64 / count as f64;
        let angle = t * std::f64::consts::TAU;
        let radius = 30.0 + (angle * 7.0).sin() * 2.0;
        points.push([50.0 + radius * angle.cos(), 50.0 + radius * angle.sin()]);
    }
    points.push(points[0]);

    let draft = generate_sketch_draft_source(SketchDraftRequest {
        part_id: "dense_profile".to_string(),
        sketch: SketchDefinition {
            sketch_id: "dense-front".to_string(),
            view: SketchView::Front,
            plane: None,
            primitives: vec![SketchPrimitive {
                primitive_id: "dense-loop".to_string(),
                kind: SketchPrimitiveKind::Polyline,
                points,
                closed: true,
                radius: None,
                topology: None,
            }],
            constraints: vec![],
        },
        operation: SketchDraftOperationKind::Extrude,
        amount: 12.0,
        symmetric: false,
    })
    .expect("dense draft source");

    assert!(
        draft.source.len() < 524_288,
        "generated source should stay below Steel budget, got {} bytes",
        draft.source.len()
    );
    let encoded = draft
        .source
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("; ecky-sketch-document-base64: "))
        .expect("compacted source map comment");
    let decoded = STANDARD
        .decode(encoded)
        .expect("decoded compact source map");
    let document: SketchDocument =
        serde_json::from_slice(&decoded).expect("decoded compact sketch document json");
    assert!(
        document.sketches[0].primitives[0].points.len() <= 512,
        "source map should keep compact points, got {}",
        document.sketches[0].primitives[0].points.len()
    );
    assert!(
        draft
            .warnings
            .iter()
            .any(|warning| warning.contains("simplified sketch primitive")),
        "expected simplification warning: {:?}",
        draft.warnings
    );
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&draft.source)
        .expect("compacted source should compile before Steel lowering");
}

#[test]
fn sketch_draft_rejects_open_polyline_for_solid_draft() {
    let err = generate_sketch_draft_source(SketchDraftRequest {
        part_id: "bad_open_profile".to_string(),
        sketch: rectangle_sketch(false),
        operation: SketchDraftOperationKind::Extrude,
        amount: 8.0,
        symmetric: false,
    })
    .expect_err("open sketch should fail");

    assert_eq!(
        err.message,
        "sketch primitive 'outer' must be closed before it can generate a solid draft."
    );
}

#[test]
fn sketch_draft_preview_renders_generated_ecky_mesh_bundle() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-sketch-draft-preview-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };

    let (draft, bundle) = generate_sketch_draft_preview(
        SketchDraftRequest {
            part_id: "bottle_cage_side".to_string(),
            sketch: rectangle_sketch(true),
            operation: SketchDraftOperationKind::Extrude,
            amount: 8.0,
            symmetric: false,
        },
        &resolver,
    )
    .expect("preview render");

    assert!(draft.source.contains("(model"));
    assert_eq!(bundle.source_language, SourceLanguage::EckyIrV0);
    assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
    assert!(std::path::Path::new(&bundle.preview_stl_path).exists());
    assert!(!bundle.viewer_assets.is_empty());

    std::fs::remove_dir_all(temp_root).ok();
}

#[test]
fn sketch_preview_hull_source_uses_candidate_cell_search() {
    let draft = generate_sketch_preview_hull_source(SketchPreviewHullRequest {
        part_id: "sketch-preview-hull".to_string(),
        document: three_view_hull_document(),
        fallback_depth: 12.0,
    })
    .expect("preview hull source");

    assert_eq!(draft.source_language, SourceLanguage::EckyIrV0);
    assert_eq!(draft.geometry_backend, GeometryBackend::EckyRust);
    assert_eq!(draft.macro_dialect, MacroDialect::EckyIrV0);
    assert!(draft.source.contains("(part sketch-preview-hull"));
    assert!(draft.source.contains("(translate 35 35 5"));
    assert!(draft.source.contains("(box 50 30 22)"));
    assert!(draft.warnings.contains(
        &"preview hull from front/top/side candidate cell search; not accepted BRep.".to_string()
    ));

    let encoded = draft
        .source
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("; ecky-sketch-document-base64: "))
        .expect("source map comment");
    let decoded = STANDARD.decode(encoded).expect("decoded source map");
    let document: SketchDocument =
        serde_json::from_slice(&decoded).expect("decoded sketch document json");

    assert_eq!(document.sketches.len(), 3);
    assert_eq!(
        document
            .sketches
            .iter()
            .map(|sketch| sketch.view.clone())
            .collect::<Vec<_>>(),
        vec![SketchView::Front, SketchView::Top, SketchView::Side]
    );
}

#[test]
fn sketch_preview_hull_rejects_mismatched_top_and_side_depth_ranges() {
    let mut document = three_view_hull_document();
    document.sketches[2].primitives[0].points = vec![
        [8.0, 20.0],
        [30.0, 20.0],
        [30.0, 50.0],
        [8.0, 50.0],
        [8.0, 20.0],
    ];

    let err = generate_sketch_preview_hull_source(SketchPreviewHullRequest {
        part_id: "sketch-preview-hull".to_string(),
        document,
        fallback_depth: 12.0,
    })
    .expect_err("mismatched depth ranges should fail");

    assert_eq!(
        err.message,
        "Top view depth range 5..27mm must match Side view depth range 8..30mm."
    );
}

#[test]
fn sketch_preview_hull_preview_renders_candidate_cell_mesh_bundle() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-sketch-preview-hull-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };

    let (draft, bundle) = generate_sketch_preview_hull(
        SketchPreviewHullRequest {
            part_id: "sketch-preview-hull".to_string(),
            document: three_view_hull_document(),
            fallback_depth: 12.0,
        },
        &resolver,
    )
    .expect("preview hull render");

    assert!(draft.source.contains("(box 50 30 22)"));
    assert_eq!(bundle.source_language, SourceLanguage::EckyIrV0);
    assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
    assert!(std::path::Path::new(&bundle.preview_stl_path).exists());
    assert!(!bundle.viewer_assets.is_empty());

    std::fs::remove_dir_all(temp_root).ok();
}

#[test]
fn sketch_brep_candidate_graph_builds_vertices_edges_and_projection_replay() {
    let response = analyze_sketch_brep_candidates(SketchBrepCandidateRequest {
        document: three_view_hull_document(),
    })
    .expect("candidate graph");

    assert_eq!(response.graph.vertices.len(), 8);
    assert_eq!(response.graph.edges.len(), 12);
    assert!(response.validation.passed);
    assert_eq!(
        response.validation.evidence,
        vec![
            "front 4/4 edges covered",
            "top 4/4 edges covered",
            "side 4/4 edges covered"
        ]
    );
    assert!(response.validation.issues.is_empty());
    assert!(response
        .graph
        .vertices
        .iter()
        .all(|vertex| vertex.evidence_views
            == vec![SketchView::Front, SketchView::Top, SketchView::Side]));
}

#[test]
fn sketch_brep_candidate_graph_reports_projection_replay_gaps() {
    let mut document = three_view_hull_document();
    document.sketches[1].primitives[0].points = vec![
        [10.0, 5.0],
        [55.0, 5.0],
        [55.0, 27.0],
        [10.0, 27.0],
        [10.0, 5.0],
    ];

    let response = analyze_sketch_brep_candidates(SketchBrepCandidateRequest { document })
        .expect("candidate graph with projection gaps");

    assert!(!response.validation.passed);
    assert!(response
        .validation
        .evidence
        .iter()
        .any(|evidence| evidence.starts_with("top ") && evidence != "top 4/4 edges covered"));
    assert!(response.validation.issues.iter().any(|issue| issue
        .message
        .starts_with("Top projection replay covers")
        && !issue.message.contains("4/4")));
}

#[test]
fn sketch_brep_candidate_graph_promotes_concave_profile_to_exact_prism_strategy() {
    let response = analyze_sketch_brep_candidates(SketchBrepCandidateRequest {
        document: concave_front_hull_document(),
    })
    .expect("concave candidate graph");

    assert!(response.validation.passed);
    assert_eq!(
        response.search.solutions[0].source_strategy,
        SketchBrepCandidateSourceStrategy::FrontProfilePrism
    );
    assert_eq!(response.search.solutions[0].cell_ids.len(), 3);
    assert!(response
        .validation
        .evidence
        .iter()
        .any(|line| line == "front 6/6 edges preserved by exact front-profile prism"));
}

#[test]
fn accepted_brep_candidate_source_selects_solution_cells_without_preview_warning() {
    let accepted = generate_accepted_brep_candidate_source(SketchBrepCandidateAcceptRequest {
        part_id: "accepted-body".to_string(),
        document: three_view_hull_document(),
        solution_id: "solution0".to_string(),
        tolerance: None,
    })
    .expect("accepted candidate source");

    assert_eq!(accepted.accepted_solution.solution_id, "solution0");
    assert_eq!(accepted.accepted_solution.cell_ids, vec!["cell0"]);
    assert!(accepted.draft_source.source.contains("(part accepted-body"));
    assert!(accepted
        .draft_source
        .source
        .contains("; ecky-accepted-brep-candidate-solution: solution0"));
    assert!(accepted.draft_source.source.contains("(translate 35 35 5"));
    assert!(accepted.draft_source.source.contains("(box 50 30 22)"));
    assert!(accepted.draft_source.warnings.is_empty());
    assert!(accepted
        .evidence
        .iter()
        .any(|line| line == "accepted BRep candidate solution 'solution0' with 1 cell"));
}

#[test]
fn accepted_brep_candidate_source_uses_exact_front_profile_prism_when_depth_views_are_rectangular()
{
    let accepted = generate_accepted_brep_candidate_source(SketchBrepCandidateAcceptRequest {
        part_id: "accepted-concave".to_string(),
        document: concave_front_hull_document(),
        solution_id: "solution0".to_string(),
        tolerance: None,
    })
    .expect("accepted exact front-profile prism");

    assert_eq!(accepted.accepted_solution.cell_ids.len(), 3);
    assert!(accepted
        .draft_source
        .source
        .contains("(part accepted-concave"));
    assert!(accepted.draft_source.source.contains("(extrude"));
    assert!(accepted
        .draft_source
        .source
        .contains("(polygon ((0 0) (20 0) (20 10) (10 10) (10 20) (0 20) (0 0)))"));
    assert!(
        !accepted.draft_source.source.contains("(box "),
        "{}",
        accepted.draft_source.source
    );
    ecky_cad_lib::ecky_scheme::compile_to_core_program(&accepted.draft_source.source)
        .expect("accepted holed exact prism source compiles");
    assert!(accepted
        .evidence
        .iter()
        .any(|line| line == "accepted exact front-profile prism from rectangular depth views"));
}

#[test]
fn accepted_brep_candidate_source_preserves_front_profile_holes_in_exact_prism() {
    let accepted = generate_accepted_brep_candidate_source(SketchBrepCandidateAcceptRequest {
        part_id: "accepted-holed".to_string(),
        document: holed_front_hull_document(),
        solution_id: "solution0".to_string(),
        tolerance: None,
    })
    .expect("accepted exact front-profile prism with source hole");

    assert_eq!(
        accepted.accepted_solution.source_strategy,
        SketchBrepCandidateSourceStrategy::FrontProfilePrism
    );
    assert!(accepted
        .draft_source
        .source
        .contains("(part accepted-holed"));
    assert!(accepted.draft_source.source.contains("(profile :outer"));
    assert!(accepted
        .draft_source
        .source
        .contains("(polygon ((0 0) (20 0) (20 20) (0 20) (0 0)))"));
    assert!(accepted
        .draft_source
        .source
        .contains("(polygon ((7 7) (13 7) (13 13) (7 13) (7 7)))"));
    assert!(accepted.draft_source.source.contains(":holes"));
    assert!(
        !accepted.draft_source.source.contains("(box "),
        "{}",
        accepted.draft_source.source
    );
    assert!(accepted
        .evidence
        .iter()
        .any(|line| line == "front 8/8 edges preserved by exact front-profile prism"));
}

#[test]
fn accepted_brep_candidate_source_rejects_unknown_solution() {
    let err = generate_accepted_brep_candidate_source(SketchBrepCandidateAcceptRequest {
        part_id: "accepted-body".to_string(),
        document: three_view_hull_document(),
        solution_id: "missing".to_string(),
        tolerance: None,
    })
    .expect_err("unknown solution should fail");

    assert_eq!(
        err.message,
        "Accepted BRep candidate solution 'missing' was not found."
    );
}

#[test]
fn accepted_brep_step_gate_rejects_mesh_only_bundle() {
    let bundle = ecky_cad_lib::models::ArtifactBundle {
        schema_version: 1,
        model_id: "mesh-only".to_string(),
        source_kind: ecky_cad_lib::models::ModelSourceKind::Generated,
        engine_kind: ecky_cad_lib::models::EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash: "hash".to_string(),
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: "/tmp/manifest.json".to_string(),
        macro_path: None,
        preview_stl_path: "/tmp/preview.stl".to_string(),
        viewer_assets: vec![],
        edge_targets: vec![],
        face_targets: vec![],
        callout_anchors: vec![],
        measurement_guides: vec![],
        export_artifacts: vec![],
    };

    let err = require_step_export_artifact(&bundle).expect_err("mesh-only bundle should fail");

    assert_eq!(
        err.message,
        "Accepted BRep candidate requires a STEP export artifact; mesh preview fallback is not CAD acceptance."
    );
}

fn accepted_step_bundle_with_edge_target(target_id: &str) -> ArtifactBundle {
    let public_target_id = stable_test_topology_target_id(target_id);
    let alias_ids = topology_alias_ids(&public_target_id, target_id);
    let manifest_path = write_accepted_test_manifest("edge", &public_target_id, &alias_ids);
    ArtifactBundle {
        schema_version: 1,
        model_id: "accepted-step".to_string(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: ecky_cad_lib::models::EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash: "hash".to_string(),
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path,
        macro_path: None,
        preview_stl_path: "/tmp/preview.stl".to_string(),
        viewer_assets: vec![],
        edge_targets: vec![ViewerEdgeTarget {
            target_id: public_target_id,
            canonical_target_id: Some(target_id.to_string()),
            durable_target_id: None,
            alias_ids,
            part_id: "accepted-body".to_string(),
            viewer_node_id: "accepted-body".to_string(),
            label: "Mounting edge".to_string(),
            editable: true,
            start: ViewerEdgePoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            end: ViewerEdgePoint {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            },
        }],
        face_targets: vec![],
        callout_anchors: vec![],
        measurement_guides: vec![],
        export_artifacts: vec![ecky_cad_lib::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: "/tmp/accepted.step".to_string(),
            role: "primary".to_string(),
        }],
    }
}

fn accepted_step_bundle_with_face_target(target_id: &str) -> ArtifactBundle {
    let public_target_id = stable_test_topology_target_id(target_id);
    let alias_ids = topology_alias_ids(&public_target_id, target_id);
    let manifest_path = write_accepted_test_manifest("face", &public_target_id, &alias_ids);
    ArtifactBundle {
        schema_version: 1,
        model_id: "accepted-step".to_string(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: ecky_cad_lib::models::EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash: "hash".to_string(),
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path,
        macro_path: None,
        preview_stl_path: "/tmp/preview.stl".to_string(),
        viewer_assets: vec![],
        edge_targets: vec![],
        face_targets: vec![ViewerFaceTarget {
            target_id: public_target_id,
            canonical_target_id: Some(target_id.to_string()),
            durable_target_id: None,
            alias_ids,
            part_id: "accepted-body".to_string(),
            viewer_node_id: "accepted-body".to_string(),
            label: "Mounting face".to_string(),
            editable: true,
            center: ViewerEdgePoint {
                x: 5.0,
                y: 5.0,
                z: 0.0,
            },
            normal: Some([0.0, 0.0, 1.0]),
            area: Some(100.0),
        }],
        callout_anchors: vec![],
        measurement_guides: vec![],
        export_artifacts: vec![ecky_cad_lib::models::ExportArtifact {
            label: "STEP".to_string(),
            format: "step".to_string(),
            path: "/tmp/accepted.step".to_string(),
            role: "primary".to_string(),
        }],
    }
}

fn stable_test_topology_target_id(target_id: &str) -> String {
    for marker in [":edge:", ":face:"] {
        let Some((prefix, payload)) = target_id.split_once(marker) else {
            continue;
        };
        let parts = payload.split(':').collect::<Vec<_>>();
        let minimum_parts = if marker == ":edge:" { 2 } else { 3 };
        if parts.len() >= minimum_parts && parts[0].chars().all(|ch| ch.is_ascii_digit()) {
            return format!("{prefix}{marker}{}", parts[1..].join(":"));
        }
    }
    target_id.to_string()
}

fn topology_alias_ids(public_target_id: &str, canonical_target_id: &str) -> Vec<String> {
    if public_target_id == canonical_target_id {
        Vec::new()
    } else {
        vec![canonical_target_id.to_string()]
    }
}

fn write_accepted_test_manifest(
    kind: &str,
    public_target_id: &str,
    alias_ids: &[String],
) -> String {
    let root = std::env::temp_dir().join(format!(
        "ecky-accepted-step-manifest-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).expect("accepted manifest dir");
    let path = root.join("manifest.json");
    let selection_kind = match kind {
        "edge" => "edge",
        "face" => "face",
        _ => "object",
    };
    let manifest = serde_json::json!({
        "schemaVersion": ecky_cad_lib::models::MODEL_RUNTIME_SCHEMA_VERSION,
        "modelId": "accepted-step",
        "sourceKind": "generated",
        "engineKind": "eckyIrV0",
        "sourceLanguage": "eckyIrV0",
        "geometryBackend": "eckyRust",
        "document": {
            "documentName": "Accepted",
            "documentLabel": "Accepted",
            "sourcePath": null,
            "objectCount": 1,
            "warnings": [],
        },
        "parts": [{
            "partId": "accepted-body",
            "freecadObjectName": "accepted-body",
            "label": "Accepted Body",
            "kind": "solid",
            "semanticRole": "body",
            "viewerAssetPath": null,
            "viewerNodeIds": ["accepted-body"],
            "parameterKeys": [],
            "editable": true,
            "bounds": null,
            "volume": null,
            "area": null,
        }],
        "parameterGroups": [],
        "controlPrimitives": [],
        "controlRelations": [],
        "controlViews": [],
        "advisories": [],
        "selectionTargets": [{
            "targetId": public_target_id,
            "aliasIds": alias_ids,
            "partId": "accepted-body",
            "viewerNodeId": "accepted-body",
            "label": "Accepted target",
            "kind": selection_kind,
            "editable": true,
            "parameterKeys": [],
            "primitiveIds": [],
            "viewIds": [],
        }],
        "measurementAnnotations": [],
        "warnings": [],
        "enrichmentState": {
            "status": "none",
            "proposals": [],
        },
    });
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&manifest).expect("manifest json"),
    )
    .expect("manifest write");
    path.to_string_lossy().to_string()
}

#[test]
fn accepted_brep_component_package_requires_explicit_ports_and_exposes_header_ports() {
    let package =
        accepted_brep_candidate_to_component_package(SketchAcceptedBrepComponentPackageRequest {
            package_id: "sketch.accepted.body".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Accepted Body".to_string(),
            tags: vec!["accepted-brep".to_string()],
            component_id: "accepted-body".to_string(),
            component_version: "0.1.0".to_string(),
            component_display_name: "Accepted Body".to_string(),
            source_ref: "artifacts/accepted-body/model.step".to_string(),
            artifact_bundle: None,
            document: three_view_hull_document(),
            solution_id: "solution0".to_string(),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.plane.mount.v1".to_string(),
                display_name: "Plane Mount".to_string(),
                base: Some("plane".to_string()),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.plane.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
                params: vec![],
            }],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "front_mount".to_string(),
                type_id: "mechanical.plane.mount.v1".to_string(),
                target_ids: vec![],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.plane.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        })
        .expect("component package");

    validate_component_package(&package).expect("package contract");
    assert_eq!(
        package.components[0].source_ref.as_deref(),
        Some("artifacts/accepted-body/model.step")
    );
    assert_eq!(package.components[0].ports[0].port_id, "front_mount");
    assert_eq!(package.components[0].sketches.len(), 3);

    let header = component_package_header(&package).expect("package header");
    assert_eq!(header.components[0].ports[0].port_id, "front_mount");
    assert_eq!(header.components[0].params.len(), 0);
}

#[test]
fn accepted_brep_component_package_preserves_port_edge_target_refs() {
    let edge_target_id = "OuterShell:edge:0:0-0-0_10-0-0";
    let expected_edge_target_id = "OuterShell:edge:0-0-0_10-0-0";
    let package =
        accepted_brep_candidate_to_component_package(SketchAcceptedBrepComponentPackageRequest {
            package_id: "sketch.accepted.body".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Accepted Body".to_string(),
            tags: vec!["accepted-brep".to_string()],
            component_id: "accepted-body".to_string(),
            component_version: "0.1.0".to_string(),
            component_display_name: "Accepted Body".to_string(),
            source_ref: "artifacts/accepted-body/model.step".to_string(),
            artifact_bundle: Some(accepted_step_bundle_with_edge_target(edge_target_id)),
            document: three_view_hull_document(),
            solution_id: "solution0".to_string(),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.edge.mount.v1".to_string(),
                display_name: "Edge Mount".to_string(),
                base: Some("edge".to_string()),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.edge.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
                params: vec![],
            }],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "mounting_edge".to_string(),
                type_id: "mechanical.edge.mount.v1".to_string(),
                target_ids: vec![edge_target_id.to_string()],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.edge.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        })
        .expect("component package");

    validate_component_package(&package).expect("package contract");
    assert_eq!(
        package.components[0].ports[0].target_ids,
        vec![expected_edge_target_id.to_string()]
    );
}

#[test]
fn accepted_brep_component_package_preserves_port_face_target_refs() {
    let face_target_id = "OuterShell:face:0:5-5-0:100";
    let expected_face_target_id = "OuterShell:face:5-5-0:100";
    let package =
        accepted_brep_candidate_to_component_package(SketchAcceptedBrepComponentPackageRequest {
            package_id: "sketch.accepted.body".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Accepted Body".to_string(),
            tags: vec!["accepted-brep".to_string()],
            component_id: "accepted-body".to_string(),
            component_version: "0.1.0".to_string(),
            component_display_name: "Accepted Body".to_string(),
            source_ref: "artifacts/accepted-body/model.step".to_string(),
            artifact_bundle: Some(accepted_step_bundle_with_face_target(face_target_id)),
            document: three_view_hull_document(),
            solution_id: "solution0".to_string(),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.face.mount.v1".to_string(),
                display_name: "Face Mount".to_string(),
                base: Some("face".to_string()),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.face.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
                params: vec![],
            }],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "mounting_face".to_string(),
                type_id: "mechanical.face.mount.v1".to_string(),
                target_ids: vec![face_target_id.to_string()],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.face.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        })
        .expect("component package");

    validate_component_package(&package).expect("package contract");
    assert_eq!(
        package.components[0].ports[0].target_ids,
        vec![expected_face_target_id.to_string()]
    );
}

#[test]
fn accepted_brep_component_package_preserves_explicit_ui_contract() {
    let ui_spec = ecky_cad_lib::models::UiSpec {
        fields: vec![ecky_cad_lib::models::UiField::Number {
            key: "diameter".to_string(),
            label: "Diameter".to_string(),
            min: Some(10.0),
            max: Some(200.0),
            step: Some(1.0),
            min_from: None,
            max_from: None,
            frozen: false,
        }],
    };
    let initial_params: ecky_cad_lib::models::DesignParams = [(
        "diameter".to_string(),
        ecky_cad_lib::models::ParamValue::Number(55.0),
    )]
    .into_iter()
    .collect();
    let package =
        accepted_brep_candidate_to_component_package(SketchAcceptedBrepComponentPackageRequest {
            package_id: "sketch.accepted.body".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Accepted Body".to_string(),
            tags: vec!["accepted-brep".to_string()],
            component_id: "accepted-body".to_string(),
            component_version: "0.1.0".to_string(),
            component_display_name: "Accepted Body".to_string(),
            source_ref: "artifacts/accepted-body/model.step".to_string(),
            artifact_bundle: None,
            document: three_view_hull_document(),
            solution_id: "solution0".to_string(),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.plane.mount.v1".to_string(),
                display_name: "Plane Mount".to_string(),
                base: Some("plane".to_string()),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.plane.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
                params: vec![],
            }],
            params: Vec::new(),
            ui_spec: ui_spec.clone(),
            initial_params: initial_params.clone(),
            ports: vec![ComponentPort {
                port_id: "front_mount".to_string(),
                type_id: "mechanical.plane.mount.v1".to_string(),
                target_ids: vec![],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.plane.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        })
        .expect("component package");

    validate_component_package(&package).expect("package contract");
    assert_eq!(package.components[0].ui_spec, ui_spec);
    assert_eq!(package.components[0].initial_params, initial_params);
    assert_eq!(package.components[0].params.len(), 1);
    assert_eq!(package.components[0].params[0].key, "diameter");
}

#[test]
fn accepted_brep_component_package_rejects_unknown_port_edge_target_ref() {
    let err =
        accepted_brep_candidate_to_component_package(SketchAcceptedBrepComponentPackageRequest {
            package_id: "sketch.accepted.body".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Accepted Body".to_string(),
            tags: vec!["accepted-brep".to_string()],
            component_id: "accepted-body".to_string(),
            component_version: "0.1.0".to_string(),
            component_display_name: "Accepted Body".to_string(),
            source_ref: "artifacts/accepted-body/model.step".to_string(),
            artifact_bundle: Some(accepted_step_bundle_with_edge_target(
                "OuterShell:edge:0:0-0-0_10-0-0",
            )),
            document: three_view_hull_document(),
            solution_id: "solution0".to_string(),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.edge.mount.v1".to_string(),
                display_name: "Edge Mount".to_string(),
                base: Some("edge".to_string()),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.edge.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
                params: vec![],
            }],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "mounting_edge".to_string(),
                type_id: "mechanical.edge.mount.v1".to_string(),
                target_ids: vec!["missing-edge".to_string()],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.edge.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        })
        .expect_err("unknown edge target should fail");

    assert_eq!(
        err.message,
        "Accepted BRep component port 'mounting_edge' references unknown accepted BRep targetId 'missing-edge'."
    );
}

#[test]
fn accepted_brep_component_package_rejects_missing_explicit_ports() {
    let err =
        accepted_brep_candidate_to_component_package(SketchAcceptedBrepComponentPackageRequest {
            package_id: "sketch.accepted.body".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Accepted Body".to_string(),
            tags: vec![],
            component_id: "accepted-body".to_string(),
            component_version: "0.1.0".to_string(),
            component_display_name: "Accepted Body".to_string(),
            source_ref: "artifacts/accepted-body/model.step".to_string(),
            artifact_bundle: None,
            document: three_view_hull_document(),
            solution_id: "solution0".to_string(),
            port_types: vec![],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![],
        })
        .expect_err("missing explicit ports should fail");

    assert_eq!(
        err.message,
        "Accepted BRep component package requires at least one explicit accepted port."
    );
}

#[test]
fn accepted_brep_component_package_project_copies_step_and_rewrites_absolute_source_ref() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-accepted-package-project-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let project_dir = temp_root.join("project");
    std::fs::create_dir_all(&project_dir).expect("project dir");
    let step_path = temp_root.join("accepted.step");
    std::fs::write(&step_path, "STEP-DATA").expect("step file");

    let edge_target_id = "OuterShell:edge:0:0-0-0_10-0-0";
    let expected_edge_target_id = "OuterShell:edge:0-0-0_10-0-0";
    let mut bundle = accepted_step_bundle_with_edge_target(edge_target_id);
    bundle.export_artifacts[0].path = step_path.to_string_lossy().to_string();
    let ui_spec = ecky_cad_lib::models::UiSpec {
        fields: vec![ecky_cad_lib::models::UiField::Number {
            key: "diameter".to_string(),
            label: "Diameter".to_string(),
            min: Some(10.0),
            max: Some(200.0),
            step: Some(1.0),
            min_from: None,
            max_from: None,
            frozen: false,
        }],
    };
    let initial_params: ecky_cad_lib::models::DesignParams = [(
        "diameter".to_string(),
        ecky_cad_lib::models::ParamValue::Number(55.0),
    )]
    .into_iter()
    .collect();

    let package = write_accepted_brep_component_package_project(
        &project_dir,
        SketchAcceptedBrepComponentPackageRequest {
            package_id: "sketch.accepted.body".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Accepted Body".to_string(),
            tags: vec!["accepted-brep".to_string()],
            component_id: "accepted-body".to_string(),
            component_version: "0.1.0".to_string(),
            component_display_name: "Accepted Body".to_string(),
            source_ref: step_path.to_string_lossy().to_string(),
            artifact_bundle: Some(bundle),
            document: three_view_hull_document(),
            solution_id: "solution0".to_string(),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.edge.mount.v1".to_string(),
                display_name: "Edge Mount".to_string(),
                base: Some("edge".to_string()),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.edge.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
                params: vec![],
            }],
            params: Vec::new(),
            ui_spec: ui_spec.clone(),
            initial_params: initial_params.clone(),
            ports: vec![ComponentPort {
                port_id: "mounting_edge".to_string(),
                type_id: "mechanical.edge.mount.v1".to_string(),
                target_ids: vec![edge_target_id.to_string()],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.edge.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        },
    )
    .expect("portable accepted package project");

    assert_eq!(
        package.components[0].source_ref.as_deref(),
        Some("artifacts/accepted-body/model.step")
    );
    let copied_step_path = project_dir.join("artifacts/accepted-body/model.step");
    assert!(copied_step_path.is_file());
    assert_eq!(
        std::fs::read_to_string(&copied_step_path).expect("copied step"),
        "STEP-DATA"
    );

    let manifest = read_component_package_manifest(&project_dir).expect("read package manifest");
    assert_eq!(
        manifest.components[0].source_ref.as_deref(),
        Some("artifacts/accepted-body/model.step")
    );
    assert_eq!(
        manifest.components[0].ports[0].target_ids,
        vec![expected_edge_target_id.to_string()]
    );
    assert_eq!(manifest.components[0].ui_spec, ui_spec);
    assert_eq!(manifest.components[0].initial_params, initial_params);
    assert_eq!(manifest.components[0].params.len(), 1);

    let archive_path = temp_root.join("accepted-body.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    assert!(archive_path.is_file());

    std::fs::remove_dir_all(temp_root).ok();
}

#[test]
fn accepted_brep_component_package_project_installs_and_resolves_step_source() {
    let temp_root = std::env::temp_dir().join(format!(
        "ecky-accepted-package-install-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let resolver = TempPathResolver {
        root: temp_root.clone(),
    };
    let project_dir = temp_root.join("project");
    std::fs::create_dir_all(&project_dir).expect("project dir");
    let step_path = temp_root.join("accepted.step");
    std::fs::write(&step_path, "STEP-DATA").expect("step file");

    let edge_target_id = "OuterShell:edge:0:0-0-0_10-0-0";
    let expected_edge_target_id = "OuterShell:edge:0-0-0_10-0-0";
    let mut bundle = accepted_step_bundle_with_edge_target(edge_target_id);
    bundle.export_artifacts[0].path = step_path.to_string_lossy().to_string();

    write_accepted_brep_component_package_project(
        &project_dir,
        SketchAcceptedBrepComponentPackageRequest {
            package_id: "sketch.accepted.body".to_string(),
            version: "0.1.0".to_string(),
            display_name: "Accepted Body".to_string(),
            tags: vec!["accepted-brep".to_string()],
            component_id: "accepted-body".to_string(),
            component_version: "0.1.0".to_string(),
            component_display_name: "Accepted Body".to_string(),
            source_ref: step_path.to_string_lossy().to_string(),
            artifact_bundle: Some(bundle),
            document: three_view_hull_document(),
            solution_id: "solution0".to_string(),
            port_types: vec![PortTypeDefinition {
                type_id: "mechanical.edge.mount.v1".to_string(),
                display_name: "Edge Mount".to_string(),
                base: Some("edge".to_string()),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.edge.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
                params: vec![],
            }],
            params: Vec::new(),
            ui_spec: ecky_cad_lib::models::UiSpec::default(),
            initial_params: Default::default(),
            ports: vec![ComponentPort {
                port_id: "mounting_edge".to_string(),
                type_id: "mechanical.edge.mount.v1".to_string(),
                target_ids: vec![edge_target_id.to_string()],
                frame: Some(PortFrame::identity()),
                params: Default::default(),
                interfaces: vec!["mechanical_mount".to_string()],
                compatible_with: vec!["mechanical.edge.mount.v1".to_string()],
                allowed_ops: vec![OperationKind::Mate],
            }],
        },
    )
    .expect("portable accepted package project");

    let archive_path = temp_root.join("accepted-body.ecky");
    write_component_package_archive(&project_dir, &archive_path).expect("write archive");
    install_component_package_archive(&resolver, &archive_path).expect("install package");

    let resolved = resolve_installed_component_source(
        &resolver,
        "sketch.accepted.body",
        "0.1.0",
        "accepted-body",
    )
    .expect("resolve installed accepted component");

    assert!(resolved
        .source_path
        .ends_with("artifacts/accepted-body/model.step"));
    assert_eq!(
        std::fs::read_to_string(&resolved.source_path).expect("installed source"),
        "STEP-DATA"
    );
    assert_eq!(
        resolved.component.ports[0].target_ids,
        vec![expected_edge_target_id.to_string()]
    );

    std::fs::remove_dir_all(temp_root).ok();
}
