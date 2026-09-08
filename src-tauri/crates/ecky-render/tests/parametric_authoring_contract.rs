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
