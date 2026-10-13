use ecky_render::scheme::SchemeSourceCompiler;
use ecky_render::SourceCompiler;

#[test]
fn component_parameter_names_do_not_inherit_builtin_keyword_types() {
    // Given a numeric component parameter, when its name coincides with a
    // built-in geometry option, the component signature owns its meaning.
    for key in [
        "x", "offset", "rotate", "origin", "normal", "edges", "faces", "openings",
    ] {
        let source = format!(
            "(define-component peg ((number {key} 3mm)) \
             (cylinder {key} 10mm)) (model (part body (peg :{key} 4mm)))"
        );
        SchemeSourceCompiler
            .compile(&source)
            .unwrap_or_else(|error| panic!("component parameter {key}: {error:?}"));
    }
}

#[test]
fn builtin_vector_keywords_still_reject_numeric_values() {
    let source = "(model (part body (place (plane :origin 4mm) (box 10mm 10mm 10mm))))";
    let error = SchemeSourceCompiler
        .compile(source)
        .expect_err("origin needs a vector");
    assert!(error.message.contains("origin"), "{error:?}");
}
