use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::direct_occt::{
    OcctArg, OcctAuthoredShapeBinding, OcctCommand, OcctOp, OcctParameterKind, OcctPlan,
};
use super::direct_occt_runner;
use super::direct_occt_sdk::{DirectOcctSdkLayout, NativeExportOutcome};
use crate::contracts::{
    AppError, AppResult, AuthoringError, AuthoringReason, AuthoringResult, DesignParams, ParamValue,
};
use crate::ecky_core_ir::{CoreParameterValue, CoreProgram};
use crate::models::PathResolver;

/// Compatibility facade for callers that do not own an application resolver.
/// Production rendering uses [`export_core_program_step_stl_with_params_runner_first`].
struct LocalRunnerResolver;

impl PathResolver for LocalRunnerResolver {
    fn app_config_dir(&self) -> PathBuf {
        std::env::temp_dir().join("ecky-direct-occt-config")
    }

    fn app_data_dir(&self) -> PathBuf {
        std::env::temp_dir().join("ecky-direct-occt-data")
    }

    fn resource_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }
}

pub fn export_core_program_step_stl(
    program: &CoreProgram,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    export_core_program_step_stl_with_params(program, &DesignParams::new(), layout, output_dir)
}

pub fn export_core_program_step_stl_with_params(
    program: &CoreProgram,
    parameters: &DesignParams,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    export_core_program_step_stl_with_params_runner_first(
        program,
        parameters,
        layout,
        output_dir,
        &LocalRunnerResolver,
    )
}

pub fn export_core_program_step_stl_with_params_runner_first(
    program: &CoreProgram,
    parameters: &DesignParams,
    _layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
    app: &dyn PathResolver,
) -> AppResult<NativeExportOutcome> {
    let parameters = effective_program_parameters(program, parameters);
    let planned =
        super::direct_occt::plan_core_program_with_params_and_bindings(program, &parameters)?;
    export_resolved_plan_with_bindings(
        &planned.plan,
        &planned.authored_shape_bindings,
        &parameters,
        output_dir,
        app,
    )
}

pub fn export_core_program_display_stl_with_params_runner_first(
    program: &CoreProgram,
    parameters: &DesignParams,
    _layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
    app: &dyn PathResolver,
) -> AppResult<NativeExportOutcome> {
    let parameters = effective_program_parameters(program, parameters);
    let planned =
        super::direct_occt::plan_core_program_with_params_and_bindings(program, &parameters)?;
    let resolved_plan = resolve_plan_parameters(&planned.plan, &parameters)?;
    direct_occt_runner::run_plan_display_stl_if_available_with_bindings(
        &resolved_plan,
        &planned.authored_shape_bindings,
        output_dir,
        app,
    )?
    .ok_or_else(|| {
        AppError::render("Direct OCCT runner unavailable; generated-C++ fallback was removed.")
    })
}

pub fn export_plan_step_stl(
    plan: &OcctPlan,
    layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    export_plan_step_stl_with_params(plan, &DesignParams::new(), layout, output_dir)
}

pub fn export_plan_step_stl_with_params(
    plan: &OcctPlan,
    parameters: &DesignParams,
    _layout: &DirectOcctSdkLayout,
    output_dir: impl AsRef<Path>,
) -> AppResult<NativeExportOutcome> {
    export_resolved_plan(plan, parameters, output_dir, &LocalRunnerResolver)
}

fn export_resolved_plan(
    plan: &OcctPlan,
    parameters: &DesignParams,
    output_dir: impl AsRef<Path>,
    app: &dyn PathResolver,
) -> AppResult<NativeExportOutcome> {
    export_resolved_plan_with_bindings(plan, &[], parameters, output_dir, app)
}

fn export_resolved_plan_with_bindings(
    plan: &OcctPlan,
    authored_shape_bindings: &[OcctAuthoredShapeBinding],
    parameters: &DesignParams,
    output_dir: impl AsRef<Path>,
    app: &dyn PathResolver,
) -> AppResult<NativeExportOutcome> {
    let resolved_plan = resolve_plan_parameters(plan, parameters)?;
    direct_occt_runner::run_plan_step_stl_if_available_with_bindings(
        &resolved_plan,
        authored_shape_bindings,
        output_dir,
        app,
    )?
    .ok_or_else(|| {
        AppError::render("Direct OCCT runner unavailable; generated-C++ fallback was removed.")
    })
}

fn backend_validation(message: impl Into<String>) -> AuthoringError {
    AuthoringError::backend(AuthoringReason::Type, message)
}

fn effective_program_parameters(program: &CoreProgram, overrides: &DesignParams) -> DesignParams {
    let mut parameters = DesignParams::new();
    for parameter in &program.parameters {
        parameters.insert(
            parameter.key.clone(),
            match &parameter.default_value {
                CoreParameterValue::Number(value) => ParamValue::Number(*value),
                CoreParameterValue::Boolean(value) => ParamValue::Boolean(*value),
                CoreParameterValue::Text(value)
                | CoreParameterValue::Choice(value)
                | CoreParameterValue::Image(value) => ParamValue::String(value.clone()),
            },
        );
    }
    parameters.extend(overrides.clone());
    parameters
}

fn resolve_plan_parameters(
    plan: &OcctPlan,
    parameters: &DesignParams,
) -> AuthoringResult<OcctPlan> {
    let mut env = BTreeMap::new();
    for parameter in &plan.parameters {
        let value = parameters.get(&parameter.key).ok_or_else(|| {
            backend_validation(format!(
                "Direct OCCT runner missing runtime parameter `{}`.",
                parameter.key
            ))
        })?;
        let valid = matches!(
            (parameter.kind, value),
            (OcctParameterKind::Number, ParamValue::Number(_))
                | (OcctParameterKind::Boolean, ParamValue::Boolean(_))
                | (OcctParameterKind::Text, ParamValue::String(_))
                | (OcctParameterKind::Choice, ParamValue::String(_))
                | (OcctParameterKind::Choice, ParamValue::Number(_))
                | (OcctParameterKind::Image, ParamValue::String(_))
        );
        if !valid {
            return Err(backend_validation(format!(
                "Direct OCCT runner parameter `{}` has incompatible value kind {}.",
                parameter.key,
                value.kind()
            )));
        }
        let arg = match value {
            ParamValue::Number(value) => OcctArg::Number(*value),
            ParamValue::Boolean(value) => OcctArg::Boolean(*value),
            ParamValue::String(value) => OcctArg::Text(value.clone()),
            ParamValue::Null => {
                return Err(backend_validation(
                    "Direct OCCT runner does not support null runtime parameters.",
                ))
            }
        };
        env.insert(parameter.key.clone(), arg);
    }

    let mut resolved = plan.clone();
    for part in &mut resolved.parts {
        for command in &mut part.commands {
            for arg in &mut command.args {
                *arg = resolve_arg(arg, &env)?;
            }
            for keyword in &mut command.keywords {
                *keyword.source_arg_mut() = resolve_arg(keyword.source_arg(), &env)?;
            }
            canonicalize_runner_point_args(command)?;
        }
    }
    Ok(resolved)
}

fn canonicalize_runner_point_args(command: &mut OcctCommand) -> AuthoringResult<()> {
    match command.op {
        OcctOp::Path | OcctOp::BezierPath => canonicalize_point3_sequence_args(&mut command.args),
        OcctOp::Bspline => canonicalize_bspline_point2_args(command),
        _ => Ok(()),
    }
}

fn canonicalize_point3_sequence_args(args: &mut [OcctArg]) -> AuthoringResult<()> {
    if let [OcctArg::List(items)] = args {
        for item in items {
            *item = canonical_point3_arg(item)?;
        }
        return Ok(());
    }

    for arg in args {
        *arg = canonical_point3_arg(arg)?;
    }
    Ok(())
}

fn canonical_point3_arg(arg: &OcctArg) -> AuthoringResult<OcctArg> {
    match arg {
        OcctArg::Point3(_) => Ok(arg.clone()),
        OcctArg::List(items) if items.len() == 3 => {
            let mut point = [0.0; 3];
            for (index, item) in items.iter().enumerate() {
                let OcctArg::Number(value) = item else {
                    return Err(backend_validation(
                        "Direct OCCT runner path point lists must resolve to numeric point3 values.",
                    ));
                };
                point[index] = *value;
            }
            Ok(OcctArg::Point3(point))
        }
        _ => Err(backend_validation(
            "Direct OCCT runner path arguments must resolve to point3 values.",
        )),
    }
}

fn canonicalize_bspline_point2_args(command: &mut OcctCommand) -> AuthoringResult<()> {
    let Some(points) = command.args.get_mut(0) else {
        return Err(backend_validation(
            "Direct OCCT runner bspline arguments must include a point2 list.",
        ));
    };
    *points = canonical_point2_list_arg(points, "bspline")?;

    for keyword in &mut command.keywords {
        if keyword.name == "tangents" {
            *keyword.source_arg_mut() =
                canonical_point2_list_arg(keyword.source_arg(), "bspline :tangents")?;
        }
    }
    Ok(())
}

fn canonical_point2_list_arg(arg: &OcctArg, label: &str) -> AuthoringResult<OcctArg> {
    let OcctArg::List(items) = arg else {
        return Err(backend_validation(format!(
            "Direct OCCT runner {label} points must resolve to a point2 list.",
        )));
    };
    let points = items
        .iter()
        .map(canonical_point2_arg)
        .collect::<AuthoringResult<Vec<_>>>()?;
    Ok(OcctArg::List(points))
}

fn canonical_point2_arg(arg: &OcctArg) -> AuthoringResult<OcctArg> {
    match arg {
        OcctArg::Point2(_) => Ok(arg.clone()),
        OcctArg::List(items) if items.len() == 2 => {
            let mut point = [0.0; 2];
            for (index, item) in items.iter().enumerate() {
                let OcctArg::Number(value) = item else {
                    return Err(backend_validation(
                        "Direct OCCT runner bspline point lists must resolve to numeric point2 values.",
                    ));
                };
                point[index] = *value;
            }
            Ok(OcctArg::Point2(point))
        }
        _ => Err(backend_validation(
            "Direct OCCT runner bspline points must resolve to point2 values.",
        )),
    }
}

fn resolve_arg(arg: &OcctArg, env: &BTreeMap<String, OcctArg>) -> AuthoringResult<OcctArg> {
    match arg {
        OcctArg::Param(key) => env.get(key).cloned().ok_or_else(|| {
            backend_validation(format!(
                "Direct OCCT runner could not resolve runtime parameter `{key}`."
            ))
        }),
        OcctArg::List(items) => Ok(OcctArg::List(
            items
                .iter()
                .map(|item| resolve_arg(item, env))
                .collect::<AuthoringResult<Vec<_>>>()?,
        )),
        other => Ok(other.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::direct_occt::{OcctCommand, OcctOp, OcctParameter, OcctPartPlan, OcctSlot};
    use super::*;

    #[test]
    fn runner_only_executor_has_no_generated_cpp_emitter() {
        let source = include_str!("direct_occt_executor.rs")
            .split_once("#[cfg(test)]")
            .map(|(head, _)| head)
            .unwrap_or_else(|| include_str!("direct_occt_executor.rs"));
        assert!(!source.contains("emit_plan_export_source"));
        assert!(!source.contains("run_native_export_source"));
    }

    #[test]
    fn resolves_path_point_like_lists_to_point3_before_runner() {
        let plan = OcctPlan {
            parameters: vec![OcctParameter {
                key: "length".to_string(),
                kind: OcctParameterKind::Number,
            }],
            parts: vec![OcctPartPlan {
                key: "body".to_string(),
                label: "Body".to_string(),
                root: OcctSlot(1),
                commands: vec![OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Path,
                    args: vec![
                        OcctArg::Point3([0.0, 0.0, 0.0]),
                        OcctArg::List(vec![
                            OcctArg::Number(0.0),
                            OcctArg::Number(0.0),
                            OcctArg::Param("length".to_string()),
                        ]),
                    ],
                    keywords: Vec::new(),
                }],
            }],
        };
        let params = DesignParams::from([("length".to_string(), ParamValue::Number(24.0))]);

        let resolved = resolve_plan_parameters(&plan, &params).expect("resolve");

        assert_eq!(
            resolved.parts[0].commands[0].args,
            vec![
                OcctArg::Point3([0.0, 0.0, 0.0]),
                OcctArg::Point3([0.0, 0.0, 24.0]),
            ]
        );
    }

    #[test]
    fn resolves_bspline_point_like_lists_to_point2_before_runner() {
        let plan = OcctPlan {
            parameters: vec![OcctParameter {
                key: "height".to_string(),
                kind: OcctParameterKind::Number,
            }],
            parts: vec![OcctPartPlan {
                key: "body".to_string(),
                label: "Body".to_string(),
                root: OcctSlot(1),
                commands: vec![OcctCommand {
                    output: OcctSlot(1),
                    op: OcctOp::Bspline,
                    args: vec![OcctArg::List(vec![
                        OcctArg::List(vec![OcctArg::Number(0.0), OcctArg::Number(0.0)]),
                        OcctArg::List(vec![
                            OcctArg::Number(4.0),
                            OcctArg::Param("height".to_string()),
                        ]),
                    ])],
                    keywords: vec![super::super::direct_occt::OcctKeyword::arg(
                        "tangents".to_string(),
                        OcctArg::List(vec![
                            OcctArg::List(vec![OcctArg::Number(1.0), OcctArg::Number(0.0)]),
                            OcctArg::List(vec![
                                OcctArg::Number(0.0),
                                OcctArg::Param("height".to_string()),
                            ]),
                        ]),
                    )],
                }],
            }],
        };
        let params = DesignParams::from([("height".to_string(), ParamValue::Number(6.0))]);

        let resolved = resolve_plan_parameters(&plan, &params).expect("resolve");

        assert_eq!(
            resolved.parts[0].commands[0].args[0],
            OcctArg::List(vec![
                OcctArg::Point2([0.0, 0.0]),
                OcctArg::Point2([4.0, 6.0]),
            ])
        );
        assert_eq!(
            resolved.parts[0].commands[0].keywords[0].source_arg(),
            &OcctArg::List(vec![
                OcctArg::Point2([1.0, 0.0]),
                OcctArg::Point2([0.0, 6.0]),
            ])
        );
    }
}
