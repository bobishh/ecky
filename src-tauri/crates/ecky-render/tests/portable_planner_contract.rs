use std::collections::BTreeMap;

use ecky_render::scheme::SchemeSourceCompiler;
use ecky_render::{KernelPlanner, PortableKernelPlanner, SourceCompiler};

#[test]
fn compiled_source_lowers_to_a_portable_kernel_plan() {
    let program = SchemeSourceCompiler
        .compile(
            r#"
            (model
              (part body
                (translate 1 2 3
                  (box 10 20 30))))
            "#,
        )
        .expect("source compiles");

    let plan = PortableKernelPlanner
        .plan(&program, &BTreeMap::new())
        .expect("normalized Core IR lowers");

    assert_eq!(plan.schema_version, 1);
    assert_eq!(plan.parts.len(), 1);
    assert_eq!(plan.parts[0].key, "body");
    assert_eq!(
        plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op.as_str())
            .collect::<Vec<_>>(),
        vec!["box", "translate"]
    );
    assert_eq!(plan.parts[0].root, plan.parts[0].commands[1].output);
}

#[test]
fn parameter_reference_in_let_star_rhs_reaches_the_kernel_as_a_number() {
    let program = SchemeSourceCompiler
        .compile(
            r#"
            (model
              (params
                (number legHeight 40 :min 1 :max 100))
              (part leg
                (let* ((resolvedHeight legHeight))
                  (cylinder 5 resolvedHeight))))
            "#,
        )
        .expect("source compiles");
    let parameters = BTreeMap::from([("legHeight".to_string(), serde_json::json!(55))]);

    let plan = PortableKernelPlanner
        .plan(&program, &parameters)
        .expect("parameter-backed let* lowers");

    let cylinder = &plan.parts[0].commands[0];
    assert_eq!(cylinder.op, "cylinder");
    assert_eq!(cylinder.args[1].kind, "number");
    assert_eq!(cylinder.args[1].value, serde_json::json!(55));
}

#[test]
fn component_body_with_suffixed_units_lowers_to_a_portable_kernel_plan() {
    let program = SchemeSourceCompiler
        .compile(
            r#"
            (define-component metric-bracket
              ((number width 20mm))
              (rotate 90deg 0deg 0deg
                (translate 0mm 2mm 0mm
                  (box width 10mm 5mm))))
            (model
              (part body (metric-bracket)))
            "#,
        )
        .expect("component unit literals compile");

    let plan = PortableKernelPlanner
        .plan(&program, &BTreeMap::new())
        .expect("component unit literals lower");

    assert_eq!(plan.parts.len(), 1);
    assert_eq!(
        plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op.as_str())
            .collect::<Vec<_>>(),
        vec!["box", "translate", "rotate"]
    );
}

#[test]
fn component_captures_top_level_pure_helper_in_portable_kernel_plan() {
    let program = SchemeSourceCompiler
        .compile(
            r#"
            (define (metric-box width)
              (box width 10mm 5mm))
            (define-component bracket
              ((number width 20mm))
              (translate 0mm 2mm 0mm
                (metric-box width)))
            (model
              (part body (bracket)))
            "#,
        )
        .expect("component captures declared helper");

    let plan = PortableKernelPlanner
        .plan(&program, &BTreeMap::new())
        .expect("captured helper lowers");

    assert_eq!(
        plan.parts[0]
            .commands
            .iter()
            .map(|command| command.op.as_str())
            .collect::<Vec<_>>(),
        vec!["box", "translate"]
    );
}

#[test]
fn local_component_front_and_side_ports_lower_once_to_portable_place_commands() {
    let program = SchemeSourceCompiler
        .compile(
            r#"
            (define-component latch ()
              (ports (port mount :type "mount.v1"
                :frame (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1))))
              (box 20 4 2))
            (model
              (part enclosure
                (ports
                  (port front :type "mount.v1" :frame
                    (frame :origin '(0 -25 15) :x-axis '(1 0 0) :z-axis '(0 -1 0)))
                  (port side :type "mount.v1" :frame
                    (frame :origin '(50 0 15) :x-axis '(0 1 0) :z-axis '(1 0 0))))
                (box 100 50 30))
              (part front-latch (place-component (latch) :from mount
                :to (port-ref enclosure front) :normal opposed))
              (part side-latch (place-component (latch) :from mount
                :to (port-ref enclosure side) :normal opposed)))
            "#,
        )
        .expect("ports compile");

    let plan = PortableKernelPlanner
        .plan(&program, &BTreeMap::new())
        .expect("portable placement plan");
    assert_eq!(plan.parts.len(), 3);
    for part in &plan.parts[1..] {
        let ops = part
            .commands
            .iter()
            .map(|command| command.op.as_str())
            .collect::<Vec<_>>();
        assert!(ops.ends_with(&["plane", "box", "place"]), "{ops:?}");
        assert_eq!(ops.iter().filter(|op| **op == "place").count(), 1);
        assert!(!ops.contains(&"rotate"));
    }
}

#[test]
fn portable_planner_rejects_unresolved_host_assets() {
    let program = SchemeSourceCompiler
        .compile(r#"(model (part body (svg "/tmp/host-only.svg" 20 20)))"#)
        .expect("source compiles");

    let error = PortableKernelPlanner
        .plan(&program, &BTreeMap::new())
        .expect_err("host asset must be resolved before portable lowering");

    assert_eq!(error.code(), "unresolvedAsset");
    assert!(error.to_string().contains("svg"));
}
