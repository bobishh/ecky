use ecky_cad_lib::models::{
    BrepHiddenLineProjectionResponse, BrepHiddenLineProjectionView, BrepProjectedEdge2d,
    BrepProjectedLoop2d, BrepProjectedLoopRole, SketchDefinition, SketchDocument, SketchPrimitive,
    SketchPrimitiveKind, SketchValidationSeverity, SketchView,
};
use ecky_cad_lib::sketch_brep_validation::validate_sketch_brep_hidden_line_projection;

fn rectangle_sketch(sketch_id: &str, view: SketchView, points: Vec<[f64; 2]>) -> SketchDefinition {
    SketchDefinition {
        sketch_id: sketch_id.to_string(),
        view,
        plane: None,
        primitives: vec![SketchPrimitive {
            primitive_id: "profile".to_string(),
            kind: SketchPrimitiveKind::Polyline,
            points,
            closed: true,
            radius: None,
        }],
        constraints: vec![],
    }
}

fn polyline(primitive_id: &str, points: Vec<[f64; 2]>) -> SketchPrimitive {
    SketchPrimitive {
        primitive_id: primitive_id.to_string(),
        kind: SketchPrimitiveKind::Polyline,
        points,
        closed: true,
        radius: None,
    }
}

fn multi_profile_front_document(front_primitives: Vec<SketchPrimitive>) -> SketchDocument {
    let mut document = document();
    document.sketches[0].primitives = front_primitives;
    document
}

fn document() -> SketchDocument {
    SketchDocument {
        document_id: "doc".to_string(),
        sketches: vec![
            rectangle_sketch(
                "front-sketch",
                SketchView::Front,
                vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]],
            ),
            rectangle_sketch(
                "top-sketch",
                SketchView::Top,
                vec![[0.0, 1.0], [10.0, 1.0], [10.0, 7.0], [0.0, 7.0]],
            ),
            rectangle_sketch(
                "side-sketch",
                SketchView::Side,
                vec![[2.0, 0.0], [8.0, 0.0], [8.0, 5.0], [2.0, 5.0]],
            ),
        ],
        active_sketch_id: Some("front-sketch".to_string()),
        units: Some("mm".to_string()),
        metadata: None,
    }
}

fn edge(edge_id: &str, points: Vec<[f64; 2]>, source_class: &str) -> BrepProjectedEdge2d {
    BrepProjectedEdge2d {
        edge_id: edge_id.to_string(),
        points,
        source_class: source_class.to_string(),
    }
}

fn projection_view(
    view: SketchView,
    visible_edges: Vec<BrepProjectedEdge2d>,
    hidden_edges: Vec<BrepProjectedEdge2d>,
) -> BrepHiddenLineProjectionView {
    BrepHiddenLineProjectionView {
        view,
        direction: [0.0, 0.0, 1.0],
        visible_edges,
        hidden_edges,
        loops: Vec::new(),
    }
}

fn loop2d(
    loop_id: &str,
    edge_ids: Vec<&str>,
    points: Vec<[f64; 2]>,
    role: BrepProjectedLoopRole,
) -> BrepProjectedLoop2d {
    BrepProjectedLoop2d {
        loop_id: loop_id.to_string(),
        edge_ids: edge_ids.into_iter().map(str::to_string).collect(),
        points,
        role,
        source_class: "derived".to_string(),
    }
}

fn projection() -> BrepHiddenLineProjectionResponse {
    BrepHiddenLineProjectionResponse {
        model_id: "model".to_string(),
        source_artifact_path: "/tmp/model.FCStd".to_string(),
        views: vec![
            projection_view(
                SketchView::Front,
                vec![edge(
                    "front-visible",
                    vec![[0.0, 0.0], [10.0, 5.0]],
                    "visible",
                )],
                vec![],
            ),
            projection_view(
                SketchView::Top,
                vec![edge(
                    "top-visible",
                    vec![[0.0, 1.0], [10.0, 7.0]],
                    "visible",
                )],
                vec![],
            ),
            projection_view(
                SketchView::Side,
                vec![],
                vec![edge("side-hidden", vec![[2.0, 0.0], [8.0, 5.0]], "hidden")],
            ),
        ],
        warnings: vec![],
        validation: None,
    }
}

fn outer_hole_front_projection() -> BrepHiddenLineProjectionResponse {
    let mut projection = projection();
    projection.views[0].visible_edges = vec![
        edge("outer-bottom", vec![[0.0, 0.0], [10.0, 0.0]], "visible"),
        edge("outer-right", vec![[10.0, 0.0], [10.0, 5.0]], "visible"),
        edge("outer-top", vec![[10.0, 5.0], [0.0, 5.0]], "visible"),
        edge("outer-left", vec![[0.0, 5.0], [0.0, 0.0]], "visible"),
        edge("hole-bottom", vec![[3.0, 2.0], [7.0, 2.0]], "visible"),
        edge("hole-right", vec![[7.0, 2.0], [7.0, 3.0]], "visible"),
        edge("hole-top", vec![[7.0, 3.0], [3.0, 3.0]], "visible"),
        edge("hole-left", vec![[3.0, 3.0], [3.0, 2.0]], "visible"),
    ];
    projection.views[0].loops = vec![
        loop2d(
            "outer",
            vec!["outer-bottom", "outer-right", "outer-top", "outer-left"],
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0], [0.0, 0.0]],
            BrepProjectedLoopRole::Outer,
        ),
        loop2d(
            "hole",
            vec!["hole-bottom", "hole-right", "hole-top", "hole-left"],
            vec![[3.0, 2.0], [7.0, 2.0], [7.0, 3.0], [3.0, 3.0], [3.0, 2.0]],
            BrepProjectedLoopRole::Hole,
        ),
    ];
    projection
}

#[test]
fn validates_matching_sketch_profiles_against_hidden_line_bounds() {
    let validation = validate_sketch_brep_hidden_line_projection(&document(), &projection(), 0.01);

    assert!(validation.passed);
    assert!(validation.issues.is_empty());
    assert!(validation
        .evidence
        .iter()
        .any(|line| line.contains("Front bounds match within tolerance 0.010000")));
    assert!(validation
        .evidence
        .iter()
        .any(|line| line.contains("Front containment pass within tolerance 0.010000")));
}

#[test]
fn brep_hidden_line_projection_response_carries_optional_sketch_validation() {
    let mut response = projection();
    response.validation = Some(validate_sketch_brep_hidden_line_projection(
        &document(),
        &projection(),
        0.01,
    ));

    let validation = response.validation.expect("validation should be present");

    assert!(validation.passed);
    assert!(validation.evidence.len() >= 3);
}

#[test]
fn reports_bounds_mismatch_with_view_and_values() {
    let mut projection = projection();
    projection.views[0].visible_edges[0].points = vec![[0.0, 0.0], [12.0, 5.0]];

    let validation = validate_sketch_brep_hidden_line_projection(&document(), &projection, 0.1);

    assert!(!validation.passed);
    assert_eq!(validation.issues.len(), 1);
    assert_eq!(validation.issues[0].sketch_id, "front-sketch");
    assert_eq!(
        validation.issues[0].primitive_id.as_deref(),
        Some("profile")
    );
    assert_eq!(
        validation.issues[0].severity,
        SketchValidationSeverity::Error
    );
    assert_eq!(
        validation.issues[0].message,
        "Front bounds mismatch: sketch minX=0.000000 maxX=10.000000 minY=0.000000 maxY=5.000000, brep minX=0.000000 maxX=12.000000 minY=0.000000 maxY=5.000000, maxDelta=2.000000, tolerance=0.100000."
    );
}

#[test]
fn reports_missing_projection_edges_for_sketch_view() {
    let mut projection = projection();
    projection.views[1].visible_edges.clear();
    projection.views[1].hidden_edges.clear();

    let validation = validate_sketch_brep_hidden_line_projection(&document(), &projection, 0.01);

    assert!(!validation.passed);
    assert_eq!(validation.issues.len(), 1);
    assert_eq!(validation.issues[0].sketch_id, "top-sketch");
    assert_eq!(
        validation.issues[0].primitive_id.as_deref(),
        Some("profile")
    );
    assert_eq!(
        validation.issues[0].severity,
        SketchValidationSeverity::Error
    );
    assert_eq!(
        validation.issues[0].message,
        "Top BRep projection has no visible or hidden edge points."
    );
}

#[test]
fn reports_topology_mismatch_when_brep_projection_has_hole_but_source_has_one_profile() {
    let projection = outer_hole_front_projection();

    let validation = validate_sketch_brep_hidden_line_projection(&document(), &projection, 0.01);

    assert!(!validation.passed);
    let issue = validation
        .issues
        .iter()
        .find(|issue| issue.message.contains("Front topology mismatch"))
        .expect("front topology mismatch");
    assert_eq!(issue.sketch_id, "front-sketch");
    assert_eq!(issue.primitive_id.as_deref(), Some("profile"));
    assert_eq!(
        issue.message,
        "Front topology mismatch: BRep projection has 2 closed loops (1 holes) but source sketch has 1 closed profile."
    );
}

#[test]
fn accepts_source_profile_hole_when_brep_projection_has_matching_hole_loop() {
    let document = multi_profile_front_document(vec![
        polyline(
            "outer-profile",
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0], [0.0, 0.0]],
        ),
        polyline(
            "hole-profile",
            vec![[3.0, 2.0], [7.0, 2.0], [7.0, 3.0], [3.0, 3.0], [3.0, 2.0]],
        ),
    ]);

    let validation = validate_sketch_brep_hidden_line_projection(
        &document,
        &outer_hole_front_projection(),
        0.01,
    );

    assert!(validation.passed, "{validation:?}");
    assert!(validation
        .evidence
        .iter()
        .any(|line| { line == "Front topology pass: 2 BRep loops, 2 source profiles, 1 holes." }));
}

#[test]
fn rejects_same_loop_count_when_source_profile_roles_do_not_match_brep_holes() {
    let document = multi_profile_front_document(vec![
        polyline(
            "outer-profile",
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0], [0.0, 0.0]],
        ),
        polyline(
            "disjoint-profile",
            vec![
                [12.0, 0.0],
                [14.0, 0.0],
                [14.0, 2.0],
                [12.0, 2.0],
                [12.0, 0.0],
            ],
        ),
    ]);

    let validation = validate_sketch_brep_hidden_line_projection(
        &document,
        &outer_hole_front_projection(),
        0.01,
    );

    assert!(!validation.passed);
    assert!(validation.issues.iter().any(|issue| {
        issue.message
            == "Front topology mismatch: BRep projection has 2 closed loops (1 holes) but source sketch has 2 closed profiles (0 holes)."
    }));
}

#[test]
fn reports_topology_mismatch_from_derived_edge_loops_when_report_has_no_loop_list() {
    let mut projection = projection();
    projection.views[0].visible_edges = vec![
        edge("outer-bottom", vec![[0.0, 0.0], [10.0, 0.0]], "visible"),
        edge("hole-top", vec![[7.0, 3.0], [3.0, 3.0]], "visible"),
        edge("outer-right", vec![[10.0, 0.0], [10.0, 5.0]], "visible"),
        edge("hole-left", vec![[3.0, 3.0], [3.0, 2.0]], "visible"),
        edge("outer-top", vec![[10.0, 5.0], [0.0, 5.0]], "visible"),
        edge("hole-bottom", vec![[3.0, 2.0], [7.0, 2.0]], "visible"),
        edge("outer-left", vec![[0.0, 5.0], [0.0, 0.0]], "visible"),
        edge("hole-right", vec![[7.0, 2.0], [7.0, 3.0]], "visible"),
    ];
    projection.views[0].loops.clear();

    let validation = validate_sketch_brep_hidden_line_projection(&document(), &projection, 0.01);

    assert!(!validation.passed);
    assert!(validation.issues.iter().any(|issue| {
        issue.message
            == "Front topology mismatch: BRep projection has 2 closed loops (1 holes) but source sketch has 1 closed profile."
    }));
}
