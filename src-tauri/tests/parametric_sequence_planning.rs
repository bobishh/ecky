use ecky_cad_lib::contracts::{DesignParams, GeometryBackend, ParamValue};
use ecky_cad_lib::ecky_cad_host::direct_occt::{
    plan_core_program_with_params, plan_core_program_with_params_and_bindings, OcctArg, OcctOp,
};
use ecky_cad_lib::ecky_core_ir::CoreParameterValue;
use ecky_cad_lib::ecky_scheme::compile_to_core_program;
use ecky_cad_lib::models::PathResolver;
use ecky_cad_lib::services::render::render_cli_ecky;
use std::path::PathBuf;

struct TempPathResolver {
    root: PathBuf,
}

impl PathResolver for TempPathResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    fn app_data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    fn resource_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }
}

#[test]
fn given_flat_map_controls_when_native_parameters_change_then_path_points_change() {
    let source = r#"(model (params (number count 3) (number pitch 8mm))
      (part wire (sweep (make-face (circle 1mm))
        (path (flat-map
          (lambda (i) (list (list (* i pitch) 0 0) (list (* i pitch) pitch 0)))
          (range count))))))"#;
    let program = compile_to_core_program(source).expect("parametric flat-map compiles");
    for (count, pitch) in [(3, 8.0), (5, 12.0)] {
        let parameters: DesignParams = [
            ("count".into(), ParamValue::Number(count as f64)),
            ("pitch".into(), ParamValue::Number(pitch)),
        ]
        .into_iter()
        .collect();
        let plan = plan_core_program_with_params(&program, &parameters)
            .expect("native planner must resolve deferred list concatenation");
        let path = plan.parts[0]
            .commands
            .iter()
            .find(|c| c.op == OcctOp::Path)
            .expect("path command");
        let OcctArg::List(points) = &path.args[0] else {
            panic!("expected path points")
        };
        assert_eq!(points.len(), count * 2);
        let expected = [(count - 1) as f64 * pitch, pitch, 0.0];
        match points.last().unwrap() {
            OcctArg::Point3(point) => assert_eq!(*point, expected),
            OcctArg::List(values) => assert_eq!(
                values
                    .iter()
                    .map(|v| match v {
                        OcctArg::Number(n) => *n,
                        OcctArg::Param(name) if name == "pitch" => pitch,
                        other => panic!("unexpected coordinate {other:?}"),
                    })
                    .collect::<Vec<_>>(),
                expected.to_vec()
            ),
            other => panic!("expected numeric point, got {other:?}"),
        }
    }
}

#[test]
fn given_parametric_spoon_rest_when_native_planning_then_keeps_native_geometry() {
    let program = compile_to_core_program(include_str!(
        "../crates/ecky-render/tests/fixtures/parametric-spoon-rest.ecky"
    ))
    .expect("real source compiles");
    let plan =
        plan_core_program_with_params(&program, &DesignParams::new()).expect("real source plans");
    assert_eq!(plan.parts.len(), 1);
    assert!(plan.parts[0].commands.iter().any(|c| c.op == OcctOp::Sweep));
}

#[test]
fn given_math_constants_as_helper_arguments_when_planning_then_resolves_constants() {
    let source = "(define (point angle) (list (cos angle) (sin angle) 0)) (model (params (number count 3)) (part wire (path (flat-map (lambda (i) (list (point pi) (point tau))) (range count)))))";
    let program = compile_to_core_program(source).expect("compiles");
    plan_core_program_with_params(&program, &DesignParams::new())
        .expect("constants resolve inside deferred callbacks");
}

#[test]
fn given_parameter_alias_in_deferred_callback_when_planning_then_retains_lexical_value() {
    let source = "(model (params (number count 3) (number lift 2)) (part wire (path (flat-map (lambda (i) (let* ((height (if (= i 0) 0 lift))) (list (list i (+ height 1) 0)))) (range count)))))";
    let program = compile_to_core_program(source).expect("compiles");
    plan_core_program_with_params(&program, &DesignParams::new())
        .expect("parameter aliases resolve in callbacks");
}

#[test]
fn given_original_round_folds_when_closing_gap_then_preserves_sweep_and_controls() {
    let program = compile_to_core_program(include_str!(
        "../crates/ecky-render/tests/fixtures/close-fold-spoon-rest.ecky"
    ))
    .expect("original round folds compile");
    let plan = plan_core_program_with_params(&program, &DesignParams::new())
        .expect("original round folds plan");
    assert_eq!(program.parameters.len(), 8);
    assert_eq!(plan.parts.len(), 1);
    assert_eq!(
        plan.parts[0]
            .commands
            .iter()
            .filter(|c| c.op == OcctOp::Sweep)
            .count(),
        1
    );
    assert!(
        !plan.parts[0]
            .commands
            .iter()
            .any(|c| c.op == OcctOp::Extrude),
        "spacing repair must not fill the original round folds with solid teeth"
    );
}

#[test]
fn given_original_round_folds_when_native_export_enriches_defaults_then_matches_basic_plan() {
    let program = compile_to_core_program(include_str!(
        "../crates/ecky-render/tests/fixtures/close-fold-spoon-rest.ecky"
    ))
    .expect("original round folds compile");
    let basic_plan = plan_core_program_with_params(&program, &DesignParams::new())
        .expect("basic native planning remains green");

    let effective_defaults: DesignParams = program
        .parameters
        .iter()
        .map(|parameter| {
            let value = match &parameter.default_value {
                CoreParameterValue::Number(value) => ParamValue::Number(*value),
                CoreParameterValue::Boolean(value) => ParamValue::Boolean(*value),
                CoreParameterValue::Text(value)
                | CoreParameterValue::Choice(value)
                | CoreParameterValue::Image(value) => ParamValue::String(value.clone()),
            };
            (parameter.key.clone(), value)
        })
        .collect();

    let enriched = plan_core_program_with_params_and_bindings(&program, &effective_defaults)
        .expect("export planning must accept the source defaults");
    assert_eq!(enriched.plan, basic_plan);
}

#[test]
#[ignore = "requires prepared native OCCT runtime"]
fn given_original_round_folds_when_cli_native_export_then_writes_artifacts() {
    let source = include_str!("../crates/ecky-render/tests/fixtures/close-fold-spoon-rest.ecky");
    let root = std::env::temp_dir().join(format!("ecky-native-export-{}", uuid::Uuid::new_v4()));
    let resolver = TempPathResolver { root: root.clone() };

    let bundle = render_cli_ecky(
        source,
        &DesignParams::new(),
        GeometryBackend::EckyRust,
        None,
        &resolver,
    )
    .expect("native CLI render must export original round folds");

    assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
    assert!(bundle.export_artifacts.iter().any(
        |artifact| artifact.format == "step" && std::path::Path::new(&artifact.path).is_file()
    ));
    let _ = std::fs::remove_dir_all(root);
}

