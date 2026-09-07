use ecky_render::core_ir::{CoreMetadataValue, CoreNodeKind, CoreOperation, CorePrimitive};
use ecky_render::scheme::SchemeSourceCompiler;
use ecky_render::SourceCompiler;

#[test]
fn scheme_source_compiles_inside_platform_neutral_crate() {
    let program = SchemeSourceCompiler
        .compile(
            r#"
        (model
          (params (number width 12 :unit length))
          (part body (box width 20mm 30mm)))
        "#,
        )
        .expect("scheme source compiles");

    assert_eq!(program.parts.len(), 1);
    assert!(matches!(
        program.parts[0].root.kind,
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Box),
            ..
        }
    ));
}

#[test]
fn model_metadata_survives_runtime_compilation() {
    let program = SchemeSourceCompiler
        .compile(
            r#"
        (define-syntax passthrough
          (syntax-rules ()
            [(_ value) value]))
        (model
          (meta :title "Pasta Curl")
          (meta units strict)
          (part body (passthrough (box 10mm 20mm 3mm))))
        "#,
        )
        .expect("model metadata compiles through runtime path");

    assert_eq!(
        program.metadata.get(":title"),
        Some(&CoreMetadataValue::Text("Pasta Curl".into()))
    );
    assert_eq!(
        program.metadata.get("units"),
        Some(&CoreMetadataValue::Symbol("strict".into()))
    );
}
