use ecky_cad_lib::ecky_cad_host::direct_occt::{plan_core_program, OcctArg};
use ecky_cad_lib::ecky_scheme::compile_to_core_program;

#[test]
fn given_camera_cutter_alias_when_native_planning_then_preserves_geometry() {
    let source = r#"
        (model
          (part case
            (let* ((body (box 40 30 10))
                   (camera-opening (hull (cylinder 5 12)
                     (translate 8 0 0 (cylinder 3 12))))
                   (camera-cutters camera-opening)
                   (all-cutters (union (box 2 2 12) camera-cutters)))
              (difference body all-cutters))))
    "#;
    let program = compile_to_core_program(source).expect("valid shape aliases compile");
    let plan = plan_core_program(&program).expect("valid shape aliases must reach native planning");
    assert_eq!(plan.parts.len(), 1);
    assert!(!plan.parts[0].commands.is_empty());
}

#[test]
fn given_chained_aliases_when_native_planning_then_matches_direct_use() {
    for suffix in ["", "mm"] {
        let direct = format!(
            "(model (part body (let* ((original (box 10{suffix} 12{suffix} 8{suffix})))
                (difference original (cylinder 2{suffix} 20{suffix})))))"
        );
        let aliased = format!(
            "(model (part body (let* ((original (box 10{suffix} 12{suffix} 8{suffix}))
                (alias original) (next-alias alias))
                (difference next-alias (cylinder 2{suffix} 20{suffix})))))"
        );
        let direct = plan_core_program(&compile_to_core_program(&direct).unwrap()).unwrap();
        let aliased = plan_core_program(&compile_to_core_program(&aliased).unwrap())
            .expect("chained shape aliases plan through both compiler paths");
        // Source node IDs differ when bindings are added. Compare operations,
        // literal arguments, and reference targets by command position instead.
        let commands = |plan: &ecky_cad_lib::ecky_cad_host::direct_occt::OcctPlan| {
            let part = &plan.parts[0];
            let index = |slot| {
                part.commands
                    .iter()
                    .position(|command| command.output == slot)
                    .unwrap()
            };
            (
                index(part.root),
                part.commands
                    .iter()
                    .map(|command| {
                        (
                            command.op,
                            command
                                .args
                                .iter()
                                .map(|arg| match arg {
                                    OcctArg::Ref(slot) => format!("ref:{}", index(*slot)),
                                    other => format!("{other:?}"),
                                })
                                .collect::<Vec<_>>(),
                            command.keywords.clone(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            commands(&direct),
            commands(&aliased),
            "aliases must preserve the native plan"
        );
    }
}

#[test]
fn given_text_as_shape_when_compiling_then_rejects_before_rendering() {
    let error = compile_to_core_program(r#"(model (part body (union (box 1 2 3) "cutter")))"#)
        .expect_err("actual text must remain invalid geometry");
    assert!(error.to_string().contains("type-mismatch"));
    assert!(error.to_string().contains("got text"));
}
