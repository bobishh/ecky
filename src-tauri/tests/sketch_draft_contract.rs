use base64::{engine::general_purpose::STANDARD, Engine as _};
use ecky_cad_lib::models::{
    GeometryBackend, MacroDialect, PathResolver, SketchBrepCandidateRequest, SketchDefinition,
    SketchDocument, SketchDraftOperationKind, SketchDraftRequest, SketchPreviewHullRequest,
    SketchPrimitive, SketchPrimitiveKind, SketchSuggestionRequest, SketchView, SourceLanguage,
};
use ecky_cad_lib::sketch_draft_runtime::{
    analyze_sketch_brep_candidates, generate_sketch_draft_preview, generate_sketch_draft_source,
    generate_sketch_preview_hull, generate_sketch_preview_hull_source,
    sketch_suggestion_to_draft_request, suggest_sketch_features,
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
        vec![
            "a_first:large:extrude",
            "front_profile:outer:extrude",
            "z_last:small:extrude"
        ]
    );
    assert_eq!(first.suggestions[0].amount, 12.0);
    assert!(first.warnings.is_empty());
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
    assert_eq!(document.sketches[0].primitives[0].closed, true);
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
fn sketch_preview_hull_source_intersects_front_top_side_silhouettes() {
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
    assert!(draft.source.contains("(intersection"));
    assert!(draft.source.contains("(translate 0 0 5"));
    assert!(draft.source.contains("(rotate 90 0 0"));
    assert!(draft.source.contains("(rotate 0 -90 0"));
    assert!(draft
        .warnings
        .contains(&"preview hull from front/top/side silhouettes; not accepted BRep.".to_string()));

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
fn sketch_preview_hull_preview_renders_intersection_mesh_bundle() {
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

    assert!(draft.source.contains("(intersection"));
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
