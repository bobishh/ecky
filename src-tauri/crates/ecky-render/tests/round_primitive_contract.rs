use ecky_render::scheme::SchemeSourceCompiler;
use ecky_render::SourceCompiler;

#[test]
fn round_primitives_reject_segment_counts_before_rendering() {
    // Given round geometry, when a facet count is supplied, compilation rejects
    // the extra argument instead of accepting a shape that differs by backend.
    for (operation, geometry) in [
        ("cylinder", "(cylinder 4.85mm 3.6mm 6)"),
        ("cone", "(cone 8mm 4mm 12mm 6)"),
        ("circle", "(extrude (circle 4.85mm 6) 3.6mm)"),
        ("ring", "(extrude (ring 8mm 4mm 6) 3.6mm)"),
    ] {
        let source = format!("(model (part body {geometry}))");
        let error = SchemeSourceCompiler
            .compile(&source)
            .expect_err("round primitive must reject a segment count");
        assert!(error.message.contains(operation), "{error:?}");
    }
}

#[test]
fn round_primitives_and_explicit_hexagons_compile() {
    // Given geometric dimensions only, round solids and a six-sided prism are
    // distinct valid authoring operations on the shared compiler boundary.
    for geometry in [
        "(cylinder 4.85mm 3.6mm :align '(center center min))",
        "(cone 8mm 4mm 12mm :align '(center center min))",
        "(extrude (circle 4.85mm) 3.6mm)",
        "(extrude (ring 8mm 4mm) 3.6mm)",
        "(extrude (regular-polygon 6 4.85mm) 3.6mm)",
    ] {
        let source = format!("(model (part body {geometry}))");
        SchemeSourceCompiler
            .compile(&source)
            .unwrap_or_else(|error| panic!("valid geometry {geometry}: {error:?}"));
    }
}
