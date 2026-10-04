use ecky_render::scheme::SchemeSourceCompiler;
use ecky_render::SourceCompiler;

#[test]
fn given_spoon_rest_with_editable_dimensions_when_compiling_then_keeps_controls_and_geometry() {
    let source = include_str!("fixtures/parametric-spoon-rest.ecky");
    let program = SchemeSourceCompiler.compile(source).expect(
        "pure helpers, parameter-dependent ranges and repeated profiles must remain parametric",
    );
    let keys: Vec<_> = program.parameters.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "bed-trim",
            "tube-diameter",
            "strand-gap",
            "tooth-height",
            "crown-rise",
            "handle-gap",
            "row-count",
            "tooth-count"
        ]
    );
    assert_eq!(program.parts.len(), 1);
}

#[test]
fn given_scalar_flat_map_callback_when_compiling_then_rejects_instead_of_flattening_numbers() {
    let error = SchemeSourceCompiler.compile(
        "(model (params (number count 3)) (part points (path (flat-map (lambda (i) i) (range count)))))",
    ).expect_err("flat-map callbacks must return lists");
    assert!(error.to_string().contains("list"), "{error}");
}

#[test]
fn given_live_grown_form_with_apply_loft_when_compiling_then_reports_unsupported_signature() {
    let source = include_str!("fixtures/grown-form-zip-regression.ecky");
    let error = SchemeSourceCompiler
        .compile(source)
        .expect_err("live model omits required loft distance");
    let message = error.to_string();
    assert!(message.contains("apply"), "{message}");
    assert!(message.contains("loft"), "{message}");
    assert!(message.contains("distance"), "{message}");
    assert!(
        !message.contains("/ expects a number"),
        "runtime fallback must not mask the expanded compiler diagnostic: {message}"
    );
}

#[test]
fn given_parametric_zip_helper_append_when_compiling_then_preserves_seven_controls() {
    let source = include_str!("fixtures/parametric-zip-helper-append.ecky");
    let program = SchemeSourceCompiler
        .compile(source)
        .expect("zip/map helper/append with param references must compile");
    let keys = program
        .parameters
        .iter()
        .map(|parameter| parameter.key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "height",
            "body-width",
            "bend",
            "twist-angle",
            "base-width",
            "base-depth",
            "base-height",
        ]
    );
    assert_eq!(program.parts.len(), 1);
}

#[test]
fn given_two_or_three_item_numeric_map_source_when_compiling_then_keeps_it_as_sequence() {
    for sequence in ["(list 0 1)", "(list 0 0.5 1)"] {
        let source = format!(
            r#"
        (define (section i width) (box (* width (+ i 1)) 1 1))
        (model
          (params (number body-width 45))
          (part body
            (apply compound
              (map (lambda (i) (section i body-width)) {sequence}))))
        "#
        );
        let program = SchemeSourceCompiler
            .compile(&source)
            .unwrap_or_else(|error| panic!("sequence {sequence} must compile: {error}"));
        assert_eq!(program.parameters.len(), 1);
        assert_eq!(program.parameters[0].key, "body-width");
    }
}

#[test]
fn given_invalid_meta_and_param_arithmetic_when_compiling_then_reports_primary_meta_error() {
    let error = SchemeSourceCompiler
        .compile(
            r#"
            (model
              (meta :title "Xeno Bloom" units strict)
              (params (number body-radius 12))
              (part body (cylinder (/ body-radius 2) 10)))
            "#,
        )
        .expect_err("invalid metadata arity must fail");
    let message = error.to_string();
    assert!(
        message.contains("`meta` expects exactly one key"),
        "{message}"
    );
    assert!(!message.contains("/ expects a number"), "{message}");
}

#[test]
fn given_quoted_tuple_map_source_when_compiling_then_returns_concise_zip_guidance() {
    let error = SchemeSourceCompiler
        .compile(
            r#"
            (model
              (part body
                (extrude
                  (polygon
                    (map (lambda ((x y)) (list x y))
                         '((0 0) (1 0) (0 1))))
                  1)))
            "#,
        )
        .expect_err("quoted tuple destructuring remains unsupported");
    let message = error.to_string();
    assert!(message.len() < 240, "error leaked compiler dump: {message}");
    assert!(
        message.contains("zip"),
        "error must name supported tuple source: {message}"
    );
    assert!(
        message.contains("enumerate"),
        "error must name supported tuple source: {message}"
    );
}
